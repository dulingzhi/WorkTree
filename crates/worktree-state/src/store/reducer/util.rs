use crate::model::{
    AppNotification, AppNotificationKind, AppState, AuthPromptKind, CommandLogEntry,
    ConflictFileLoadMode, DiagnosticEntry, DiagnosticKind, GitLogSettings, Loadable, RepoId,
    RepoLoadsInFlight, RepoState,
};
use crate::msg::{ConflictAutosolveMode, ConflictAutosolveStats, Effect, RepoCommandKind};
use rustc_hash::FxHashSet;
use smallvec::{Array, SmallVec};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
#[cfg(test)]
use worktree_core::auth::stage_git_auth;
use worktree_core::auth::{GitAuthKind, StagedGitAuth, clear_staged_git_auth};
use worktree_core::domain::{DiffArea, DiffTarget, FileStatusKind};
use worktree_core::error::{Error, ErrorKind, GitFailure};
use worktree_core::services::CommandOutput;

/// Default page size for log fetches.
pub(super) const DEFAULT_LOG_PAGE_SIZE: usize = 200;
pub(super) const CONFLICT_RELOAD_EFFECT_COUNT: usize = 1;
const DIFF_RELOAD_MAX_EFFECTS: usize = 3;
const PRIMARY_REFRESH_MAX_EFFECTS: usize = 6;
const FULL_REFRESH_MAX_EFFECTS: usize = 9;
const BACKGROUND_METADATA_MAX_EFFECTS: usize = 3;

pub(super) trait EffectAccumulator {
    fn push_effect(&mut self, effect: Effect);
}

impl EffectAccumulator for Vec<Effect> {
    fn push_effect(&mut self, effect: Effect) {
        self.push(effect);
    }
}

impl<A> EffectAccumulator for SmallVec<A>
where
    A: Array<Item = Effect>,
{
    fn push_effect(&mut self, effect: Effect) {
        self.push(effect);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DiffTargetPreviewFlags {
    pub wants_image: bool,
    pub is_svg: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SelectedDiffLoadPlan {
    pub load_patch_diff: bool,
    pub load_file_text: bool,
    pub preview_text_side: Option<worktree_core::domain::DiffPreviewTextSide>,
    pub load_submodule_summary: bool,
    pub load_file_image: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SelectedConflictTarget<'a> {
    Current,
    Path(&'a Path),
}

fn path_preview_flags(path: &Path) -> DiffTargetPreviewFlags {
    let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
        return DiffTargetPreviewFlags::default();
    };

    match ext.as_bytes() {
        [a, b, c] => match (
            a.to_ascii_lowercase(),
            b.to_ascii_lowercase(),
            c.to_ascii_lowercase(),
        ) {
            (b's', b'v', b'g') => DiffTargetPreviewFlags {
                wants_image: true,
                is_svg: true,
            },
            (b'p', b'n', b'g')
            | (b'j', b'p', b'g')
            | (b'g', b'i', b'f')
            | (b'b', b'm', b'p')
            | (b'i', b'c', b'o')
            | (b't', b'i', b'f') => DiffTargetPreviewFlags {
                wants_image: true,
                is_svg: false,
            },
            _ => DiffTargetPreviewFlags::default(),
        },
        [a, b, c, d] => match (
            a.to_ascii_lowercase(),
            b.to_ascii_lowercase(),
            c.to_ascii_lowercase(),
            d.to_ascii_lowercase(),
        ) {
            (b'j', b'p', b'e', b'g') | (b'w', b'e', b'b', b'p') => DiffTargetPreviewFlags {
                wants_image: true,
                is_svg: false,
            },
            (b't', b'i', b'f', b'f') => DiffTargetPreviewFlags {
                wants_image: true,
                is_svg: false,
            },
            _ => DiffTargetPreviewFlags::default(),
        },
        _ => DiffTargetPreviewFlags::default(),
    }
}

pub(super) fn diff_target_preview_flags(target: &DiffTarget) -> DiffTargetPreviewFlags {
    match target {
        DiffTarget::WorkingTree { path, .. } => path_preview_flags(path),
        DiffTarget::Commit {
            path: Some(path), ..
        }
        | DiffTarget::CommitRange {
            path: Some(path), ..
        } => path_preview_flags(path),
        _ => DiffTargetPreviewFlags::default(),
    }
}

#[cfg(test)]
pub(super) fn diff_target_wants_image_preview(target: &DiffTarget) -> bool {
    diff_target_preview_flags(target).wants_image
}

#[cfg(test)]
pub(super) fn diff_target_is_svg(target: &DiffTarget) -> bool {
    diff_target_preview_flags(target).is_svg
}

fn diff_target_is_preview_only(repo_state: &RepoState, target: &DiffTarget) -> bool {
    match target {
        DiffTarget::WorkingTree { path, area } => {
            let Some(entries) = repo_state.status_entries_for_area(*area) else {
                return false;
            };

            entries.iter().any(|entry| {
                entry.path == *path
                    && matches!(
                        entry.kind,
                        FileStatusKind::Untracked | FileStatusKind::Added | FileStatusKind::Deleted
                    )
            })
        }
        DiffTarget::Commit {
            commit_id,
            path: Some(path),
        } => {
            let Loadable::Ready(details) = &repo_state.history_state.commit_details else {
                return false;
            };
            if &details.id != commit_id {
                return false;
            }

            details.files.iter().any(|file| {
                file.path == *path
                    && !file.is_submodule
                    && matches!(file.kind, FileStatusKind::Added | FileStatusKind::Deleted)
            })
        }
        DiffTarget::Commit { path: None, .. } | DiffTarget::CommitRange { .. } => false,
    }
}

fn diff_target_preview_text_side(
    repo_state: &RepoState,
    target: &DiffTarget,
) -> Option<worktree_core::domain::DiffPreviewTextSide> {
    match target {
        DiffTarget::WorkingTree { path, area } => {
            let entries = repo_state.status_entries_for_area(*area)?;

            entries.iter().find_map(|entry| {
                (entry.path == *path).then_some(match entry.kind {
                    FileStatusKind::Untracked | FileStatusKind::Added => {
                        Some(worktree_core::domain::DiffPreviewTextSide::New)
                    }
                    FileStatusKind::Deleted => {
                        Some(worktree_core::domain::DiffPreviewTextSide::Old)
                    }
                    FileStatusKind::Modified
                    | FileStatusKind::Renamed
                    | FileStatusKind::Conflicted => None,
                })?
            })
        }
        DiffTarget::Commit {
            commit_id,
            path: Some(path),
        } => {
            let Loadable::Ready(details) = &repo_state.history_state.commit_details else {
                return None;
            };
            if &details.id != commit_id {
                return None;
            }

            details.files.iter().find_map(|file| {
                (file.path == *path && !file.is_submodule).then_some(match file.kind {
                    FileStatusKind::Added => Some(worktree_core::domain::DiffPreviewTextSide::New),
                    FileStatusKind::Deleted => {
                        Some(worktree_core::domain::DiffPreviewTextSide::Old)
                    }
                    FileStatusKind::Modified
                    | FileStatusKind::Renamed
                    | FileStatusKind::Conflicted
                    | FileStatusKind::Untracked => None,
                })?
            })
        }
        DiffTarget::Commit { path: None, .. } | DiffTarget::CommitRange { .. } => None,
    }
}

pub(super) fn selected_diff_load_plan(
    repo_state: &RepoState,
    target: &DiffTarget,
) -> SelectedDiffLoadPlan {
    if diff_target_is_submodule(repo_state, target) {
        return SelectedDiffLoadPlan {
            load_patch_diff: false,
            load_file_text: false,
            preview_text_side: None,
            load_submodule_summary: true,
            load_file_image: false,
        };
    }

    let supports_file = matches!(
        target,
        DiffTarget::WorkingTree { .. }
            | DiffTarget::Commit { path: Some(_), .. }
            | DiffTarget::CommitRange { path: Some(_), .. }
    );
    let preview = diff_target_preview_flags(target);
    let content_preview = repo_state.diff_state.content_preview;
    let preview_only = content_preview || diff_target_is_preview_only(repo_state, target);
    let preview_text_side = if supports_file && (!preview.wants_image || preview.is_svg) {
        if content_preview {
            // Commit content is read from a blob temp file (New side); working-tree
            // content is read straight from disk by the worktree preview and needs
            // no preview-text-file load.
            matches!(target, DiffTarget::Commit { .. })
                .then_some(worktree_core::domain::DiffPreviewTextSide::New)
        } else {
            diff_target_preview_text_side(repo_state, target)
        }
    } else {
        None
    };

    SelectedDiffLoadPlan {
        load_patch_diff: !preview_only,
        // An SVG counts as an image, so it never reaches the text-file preview
        // path and the diff pane's Code view is the only place its source is
        // ever shown. That view reads the loaded file text, so it has to load
        // even for the preview-only targets — added, deleted, untracked — that
        // a plain text file would render straight from the worktree instead.
        load_file_text: supports_file
            && (preview.is_svg || (!preview.wants_image && !preview_only)),
        preview_text_side,
        load_submodule_summary: false,
        load_file_image: supports_file && preview.wants_image,
    }
}

pub(super) fn apply_selected_diff_load_plan_state(
    repo_state: &mut RepoState,
    load_plan: SelectedDiffLoadPlan,
) {
    apply_selected_diff_load_plan_state_with_reload_mode(
        repo_state,
        load_plan,
        DiffReloadMode::Blank,
    );
}

/// What a reload does with content that is already on screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffReloadMode {
    /// Drop it: the diff is switching to something else, so the old content is
    /// not what the user asked for and must not linger.
    Blank,
    /// Keep it until the new content arrives. For a reload of the *same* target
    /// — after staging a hunk or line, say — blanking makes the pane flash a
    /// "Loading" placeholder for a frame or two even when almost nothing about
    /// the file changed.
    ///
    /// What stays on screen is then a generation behind the index, so this also
    /// raises `diff_reload_in_flight` for as long as that is true.
    KeepLoaded,
}

pub(super) fn apply_selected_diff_load_plan_state_with_reload_mode(
    repo_state: &mut RepoState,
    load_plan: SelectedDiffLoadPlan,
    mode: DiffReloadMode,
) {
    fn reloading<T>(current: &Loadable<T>, mode: DiffReloadMode) -> Loadable<T>
    where
        T: Clone,
    {
        match (mode, current) {
            (DiffReloadMode::KeepLoaded, Loadable::Ready(value)) => Loadable::Ready(value.clone()),
            _ => Loadable::Loading,
        }
    }

    // Blanking leaves nothing stale to build a patch out of, so only the keeping
    // mode raises the flag — and it lowers it again, which is what stops a
    // target change from stranding it set.
    repo_state.diff_state.diff_reload_in_flight = matches!(mode, DiffReloadMode::KeepLoaded);

    repo_state.diff_state.diff = if load_plan.load_patch_diff {
        reloading(&repo_state.diff_state.diff, mode)
    } else {
        Loadable::NotLoaded
    };
    repo_state.diff_state.diff_file = if load_plan.load_file_text {
        reloading(&repo_state.diff_state.diff_file, mode)
    } else {
        Loadable::NotLoaded
    };
    // The LFS pointer load is the redirected form of the text load, so it
    // follows the same flag: whichever the worker resolves, the other stays
    // NotLoaded.
    repo_state.diff_state.diff_file_lfs = if load_plan.load_file_text {
        reloading(&repo_state.diff_state.diff_file_lfs, mode)
    } else {
        Loadable::NotLoaded
    };
    repo_state.diff_state.diff_preview_text_file = if load_plan.preview_text_side.is_some() {
        reloading(&repo_state.diff_state.diff_preview_text_file, mode)
    } else {
        Loadable::NotLoaded
    };
    repo_state.diff_state.submodule_summary = if load_plan.load_submodule_summary {
        reloading(&repo_state.diff_state.submodule_summary, mode)
    } else {
        Loadable::NotLoaded
    };
    repo_state.diff_state.diff_file_image = if load_plan.load_file_image {
        reloading(&repo_state.diff_state.diff_file_image, mode)
    } else {
        Loadable::NotLoaded
    };
}

fn diff_target_is_submodule(repo_state: &RepoState, target: &DiffTarget) -> bool {
    match target {
        DiffTarget::WorkingTree { path, area } => {
            let Some(entry) = repo_state.status_entry_for_path(*area, path) else {
                return false;
            };
            if entry.kind == FileStatusKind::Untracked {
                return false;
            }

            if let Loadable::Ready(submodules) = &repo_state.submodules
                && submodules.iter().any(|submodule| submodule.path == *path)
            {
                return true;
            }

            if entry.kind == FileStatusKind::Deleted {
                return head_path_is_gitlink(&repo_state.spec.workdir, path);
            }

            let dot_git = repo_state.spec.workdir.join(path).join(".git");
            dot_git.is_file() || dot_git.is_dir()
        }
        DiffTarget::Commit {
            commit_id,
            path: Some(path),
        } => {
            let Loadable::Ready(details) = &repo_state.history_state.commit_details else {
                return false;
            };
            if &details.id != commit_id {
                return false;
            }

            details
                .files
                .iter()
                .any(|file| file.path == *path && file.is_submodule)
        }
        DiffTarget::Commit { path: None, .. } | DiffTarget::CommitRange { .. } => false,
    }
}

fn head_path_is_gitlink(workdir: &Path, path: &Path) -> bool {
    let path = if path.is_absolute() {
        match path.strip_prefix(workdir) {
            Ok(path) => path,
            Err(_) => return false,
        }
    } else {
        path
    };

    let Ok(repo) = crate::store::open_worktree_repo(workdir) else {
        return false;
    };
    let Ok(head_id) = repo.head_id() else {
        return false;
    };
    let Ok(object) = repo.find_object(head_id.detach()) else {
        return false;
    };
    let Ok(tree) = object.peel_to_tree() else {
        return false;
    };
    let Ok(Some(entry)) = tree.lookup_entry_by_path(path) else {
        return false;
    };
    entry.mode().is_commit()
}

pub(super) fn selected_conflict_target<'a>(
    repo_state: &RepoState,
    target: &'a DiffTarget,
) -> Option<SelectedConflictTarget<'a>> {
    let DiffTarget::WorkingTree { path, area } = target else {
        return None;
    };
    if *area != DiffArea::Unstaged {
        return None;
    }

    if repo_state.conflict_state.conflict_file_path.as_deref() == Some(path.as_path()) {
        return Some(SelectedConflictTarget::Current);
    }

    // Fast path: skip the full scan when status has no unstaged conflicts.
    if !repo_state.has_unstaged_conflicts {
        return None;
    }

    repo_state
        .worktree_status_entries()?
        .iter()
        .find(|entry| entry.path == *path && entry.kind == FileStatusKind::Conflicted)
        .map(|_| SelectedConflictTarget::Path(path.as_path()))
}

pub(super) fn current_conflict_load_mode(repo_state: &RepoState) -> ConflictFileLoadMode {
    repo_state.conflict_state.conflict_file_load_mode
}

pub(super) fn append_start_current_conflict_target_reload(
    effects: &mut impl EffectAccumulator,
    repo_state: &mut RepoState,
) {
    let mode = current_conflict_load_mode(repo_state);
    append_start_current_conflict_target_reload_with_mode(effects, repo_state, mode);
}

pub(super) fn append_start_conflict_target_reload(
    effects: &mut impl EffectAccumulator,
    repo_state: &mut RepoState,
    path: &Path,
) {
    let mode = current_conflict_load_mode(repo_state);
    append_start_conflict_target_reload_with_mode(effects, repo_state, path, mode);
}

pub(super) fn reset_conflict_target_reload_state(
    repo_state: &mut RepoState,
    mode: ConflictFileLoadMode,
    same_path: bool,
) {
    repo_state.set_conflict_file_load_mode(mode);
    repo_state.set_conflict_file(Loadable::Loading);
    if !same_path {
        repo_state.conflict_state.session_pending_restore = None;
    }
    // section 30 split/join round-trip: stash (not drop) the session across
    // same-path reloads so `conflict_file_loaded` restores resolutions and
    // does not re-run the on-open autosolve. Dropping it outright wiped
    // unsaved resolutions on every watcher reload.
    if let Some(session) = repo_state.conflict_state.conflict_session.take() {
        if same_path {
            repo_state.conflict_state.session_pending_restore = Some(session);
        }
        repo_state.bump_conflict_rev();
    }
    repo_state.set_conflict_hide_resolved(false);
}

fn append_start_current_conflict_target_reload_with_mode(
    effects: &mut impl EffectAccumulator,
    repo_state: &mut RepoState,
    mode: ConflictFileLoadMode,
) {
    debug_assert!(repo_state.conflict_state.conflict_file_path.is_some());
    reset_conflict_target_reload_state(repo_state, mode, true);
    effects.push_effect(Effect::LoadSelectedConflictFile {
        repo_id: repo_state.id,
        mode,
    });
}

pub(super) fn append_start_conflict_target_reload_with_mode(
    effects: &mut impl EffectAccumulator,
    repo_state: &mut RepoState,
    path: &Path,
    mode: ConflictFileLoadMode,
) {
    if repo_state.conflict_state.conflict_file_path.as_deref() == Some(path) {
        append_start_current_conflict_target_reload_with_mode(effects, repo_state, mode);
        return;
    }

    repo_state.set_conflict_file_path(Some(path.to_path_buf()));
    reset_conflict_target_reload_state(repo_state, mode, false);
    effects.push_effect(Effect::LoadSelectedConflictFile {
        repo_id: repo_state.id,
        mode,
    });
}

pub(super) fn diff_reload_effect_count(repo_state: &RepoState, target: &DiffTarget) -> usize {
    let plan = selected_diff_load_plan(repo_state, target);

    let mut count = usize::from(plan.load_patch_diff);
    if plan.load_submodule_summary {
        count += 1;
    }
    if plan.load_file_image {
        count += 1;
    }
    if plan.load_file_text {
        count += 1;
    }
    if plan.preview_text_side.is_some() {
        count += 1;
    }

    debug_assert!(count <= DIFF_RELOAD_MAX_EFFECTS);
    count
}

pub(super) fn append_diff_reload_effects(
    effects: &mut impl EffectAccumulator,
    repo_state: &RepoState,
    repo_id: RepoId,
    target: DiffTarget,
) {
    let plan = selected_diff_load_plan(repo_state, &target);

    if plan.load_submodule_summary {
        effects.push_effect(Effect::LoadSubmoduleSummary {
            repo_id,
            target: target.clone(),
        });
    }
    if plan.load_patch_diff {
        effects.push_effect(Effect::LoadDiff {
            repo_id,
            target: target.clone(),
        });
    }
    if plan.load_file_image {
        effects.push_effect(Effect::LoadDiffFileImage {
            repo_id,
            target: target.clone(),
        });
    }
    if let Some(side) = plan.preview_text_side {
        effects.push_effect(Effect::LoadDiffPreviewTextFile {
            repo_id,
            target: target.clone(),
            side,
        });
    }
    if plan.load_file_text {
        effects.push_effect(Effect::LoadDiffFile { repo_id, target });
    }
}

pub(super) fn refresh_primary_effect_capacity() -> usize {
    PRIMARY_REFRESH_MAX_EFFECTS
}

fn should_auto_fetch_history_tags(git_log_settings: GitLogSettings) -> bool {
    git_log_settings.show_history_tags && git_log_settings.auto_fetch_tags_on_repo_activation()
}

pub(super) fn append_requested_status_refresh_effects(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
) {
    let repo_id = repo_state.id;
    let load_worktree = repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::WORKTREE_STATUS);
    let load_staged = repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::STAGED_STATUS);

    match (load_worktree, load_staged) {
        (true, true) => effects.push_effect(Effect::LoadStatus { repo_id }),
        (true, false) => effects.push_effect(Effect::LoadWorktreeStatus { repo_id }),
        (false, true) => effects.push_effect(Effect::LoadStagedStatus { repo_id }),
        (false, false) => {}
    }
}

/// Answer a status refresh whose affected paths are known — a finished
/// stage/unstage/commit — through the targeted lane instead of the full
/// worktree walk: one pathspec `git status` whose merge lands in
/// milliseconds, where the walk re-scans the whole tree (the action's index
/// rewrite means the gix staged cache cannot serve it) and holds the staged
/// panel back by that much longer.
///
/// Takes both lane flags, because the merge patches both lanes and finishes
/// both; a caller that runs `refresh_primary_effects` afterwards gets its
/// status leg coalesced away by them, while the head/log legs still run.
/// Does nothing unless the flags are free and a settled snapshot exists to
/// merge onto — the merge requires one, and a stranded flag would coalesce
/// every later refresh of that lane.
pub(in crate::store::reducer) fn append_targeted_status_refresh(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
    paths: &[std::path::PathBuf],
) {
    if paths.is_empty()
        || !matches!(repo_state.status, Loadable::Ready(_))
        || repo_state
            .loads_in_flight
            .is_in_flight(RepoLoadsInFlight::WORKTREE_STATUS)
        || repo_state
            .loads_in_flight
            .is_in_flight(RepoLoadsInFlight::STAGED_STATUS)
    {
        return;
    }
    repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::WORKTREE_STATUS);
    repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::STAGED_STATUS);
    let repo_id = repo_state.id;
    effects.push_effect(Effect::LoadStatusForPaths {
        repo_id,
        paths: paths.to_vec().into(),
    });
}

fn push_rebase_and_merge_refresh_effect(effects: &mut impl EffectAccumulator, repo_id: RepoId) {
    effects.push_effect(Effect::LoadRebaseAndMergeState { repo_id });
}

fn append_requested_rebase_and_merge_refresh_effects(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
) {
    let repo_id = repo_state.id;
    let load_rebase = repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REBASE_STATE);
    let load_merge_commit_message = repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::MERGE_COMMIT_MESSAGE);
    // The bisect snapshot rides along with the sequencer loads on every
    // primary refresh, but keeps its own in-flight flag so a late reply never
    // wedges the dedup gate shared by rebase and merge state.
    let load_bisect = repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::BISECT_STATE);

    match (load_rebase, load_merge_commit_message) {
        (true, true) => push_rebase_and_merge_refresh_effect(effects, repo_id),
        (true, false) => effects.push_effect(Effect::LoadRebaseState { repo_id }),
        (false, true) => effects.push_effect(Effect::LoadMergeCommitMessage { repo_id }),
        (false, false) => {}
    }
    if load_bisect {
        effects.push_effect(Effect::LoadBisectState { repo_id });
    }
}

/// The request for a fresh first page of `repo_state`'s history, under whatever
/// scope and author filter it currently has.
pub(super) fn first_page_log_request(repo_state: &RepoState) -> crate::model::PendingLogLoad {
    crate::model::PendingLogLoad {
        scope: repo_state.history_state.history_scope,
        author: repo_state.history_state.history_author_filter.clone(),
        refs: repo_state.history_state.history_ref_filters.clone(),
        limit: DEFAULT_LOG_PAGE_SIZE,
        cursor: None,
    }
}

/// Requests `load` and returns the effect that starts it, or `None` when it was
/// coalesced into a walk already in flight. The effect carries the sequence
/// number the request was given, which is how its replies are recognised.
pub(super) fn request_log_effect(
    repo_state: &mut RepoState,
    load: crate::model::PendingLogLoad,
) -> Option<Effect> {
    let repo_id = repo_state.id;
    let seq = repo_state.loads_in_flight.request_log(load.clone())?;
    let crate::model::PendingLogLoad {
        scope,
        author,
        refs,
        limit,
        cursor,
    } = load;
    Some(Effect::LoadLog {
        repo_id,
        seq,
        scope,
        author,
        refs,
        limit,
        cursor,
    })
}

pub(super) fn append_refresh_primary_effects(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
) {
    let repo_id = repo_state.id;
    let log_request = first_page_log_request(repo_state);

    if let Some(seq) = repo_state
        .loads_in_flight
        .request_primary_refresh_batch(log_request.clone())
    {
        repo_state.set_log_loading_more(false);
        effects.push_effect(Effect::LoadHeadBranch { repo_id });
        effects.push_effect(Effect::LoadUpstreamDivergence { repo_id });
        push_rebase_and_merge_refresh_effect(effects, repo_id);
        // The batch above does not cover the bisect snapshot; keep its own
        // in-flight flag so the fast path and the per-flag fallback below
        // agree on when a bisect load is already running.
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::BISECT_STATE)
        {
            effects.push_effect(Effect::LoadBisectState { repo_id });
        }
        effects.push_effect(Effect::LoadStatus { repo_id });
        // One cheap format-only walk; powers author avatars wherever only the
        // author name is available (history rows, hover cards).
        effects.push_effect(Effect::LoadAuthorEmails { repo_id });
        effects.push_effect(Effect::LoadLog {
            repo_id,
            seq,
            scope: log_request.scope,
            author: log_request.author,
            refs: log_request.refs,
            limit: log_request.limit,
            cursor: log_request.cursor,
        });
        return;
    }

    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::HEAD_BRANCH)
    {
        effects.push_effect(Effect::LoadHeadBranch { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::UPSTREAM_DIVERGENCE)
    {
        effects.push_effect(Effect::LoadUpstreamDivergence { repo_id });
    }
    append_requested_rebase_and_merge_refresh_effects(repo_state, effects);
    append_requested_status_refresh_effects(repo_state, effects);
    if let Some(effect) = request_log_effect(repo_state, log_request) {
        // Block pagination while a refresh log load is in flight, to avoid concurrent LogLoaded
        // merges with different cursors.
        repo_state.set_log_loading_more(false);
        effects.push_effect(effect);
    }
}

pub(super) fn refresh_full_effect_capacity() -> usize {
    FULL_REFRESH_MAX_EFFECTS
}

pub(super) fn background_metadata_effect_capacity() -> usize {
    BACKGROUND_METADATA_MAX_EFFECTS
}

pub(super) fn append_refresh_full_effects(
    repo_state: &mut RepoState,
    _git_log_settings: GitLogSettings,
    effects: &mut impl EffectAccumulator,
) {
    let repo_id = repo_state.id;

    // Prioritize UI-critical loads (status + log) early. The executor is a FIFO queue, so this
    // ordering can materially impact perceived responsiveness when switching repositories.
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::HEAD_BRANCH)
    {
        effects.push_effect(Effect::LoadHeadBranch { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::UPSTREAM_DIVERGENCE)
    {
        effects.push_effect(Effect::LoadUpstreamDivergence { repo_id });
    }
    append_requested_status_refresh_effects(repo_state, effects);
    let log_request = first_page_log_request(repo_state);
    if let Some(effect) = request_log_effect(repo_state, log_request) {
        repo_state.set_log_loading_more(false);
        effects.push_effect(effect);
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::BRANCHES)
    {
        effects.push_effect(Effect::LoadBranches { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTES)
    {
        effects.push_effect(Effect::LoadRemotes { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTE_BRANCHES)
    {
        effects.push_effect(Effect::LoadRemoteBranches { repo_id });
    }
    append_requested_rebase_and_merge_refresh_effects(repo_state, effects);
    // Same cheap format-only walk the primary refresh path queues: without it
    // the map only ever loaded on the rare primary refresh, so freshly opened
    // repositories had no author emails (and no remote avatars) until some
    // unrelated action happened to trigger that path.
    effects.push_effect(Effect::LoadAuthorEmails { repo_id });
}

pub(super) fn append_auto_background_metadata_effects(
    repo_state: &mut RepoState,
    git_log_settings: GitLogSettings,
    effects: &mut impl EffectAccumulator,
) {
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return;
    }

    let repo_id = repo_state.id;
    if should_auto_fetch_history_tags(git_log_settings) {
        if matches!(repo_state.tags, Loadable::NotLoaded | Loadable::Error(_)) {
            repo_state.set_tags(Loadable::Loading);
            if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
                effects.push_effect(Effect::LoadTags { repo_id });
            }
        }

        if matches!(
            repo_state.remote_tags,
            Loadable::NotLoaded | Loadable::Error(_)
        ) {
            repo_state.set_remote_tags(Loadable::Loading);
            if repo_state
                .loads_in_flight
                .request(RepoLoadsInFlight::REMOTE_TAGS)
            {
                effects.push_effect(Effect::LoadRemoteTags { repo_id });
            }
        }
    }

    if matches!(
        repo_state.submodules,
        Loadable::NotLoaded | Loadable::Error(_)
    ) {
        repo_state.set_submodules(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::SUBMODULES)
        {
            effects.push_effect(Effect::LoadSubmodules { repo_id });
        }
    }
}

pub(super) fn dedup_paths_in_order(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::with_capacity(paths.len());
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    for p in paths {
        if !seen.insert(p.clone()) {
            continue;
        }
        out.push(p);
    }
    out
}

pub(super) fn normalize_repo_path(path: PathBuf) -> PathBuf {
    let path = if path.is_relative() {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    } else {
        path
    };

    canonicalize_path(path)
}

pub(super) fn canonicalize_path(path: PathBuf) -> PathBuf {
    super::super::canonicalize_path(path)
}

pub(super) fn push_notification(state: &mut AppState, kind: AppNotificationKind, message: String) {
    const MAX_NOTIFICATIONS: usize = 200;
    state.notifications.push(AppNotification {
        time: SystemTime::now(),
        kind,
        message,
    });
    if state.notifications.len() > MAX_NOTIFICATIONS {
        let extra = state.notifications.len() - MAX_NOTIFICATIONS;
        state.notifications.drain(0..extra);
    }
}

pub(super) fn clear_banner_error_for_repo(state: &mut AppState, repo_id: RepoId) {
    if state
        .banner_error
        .as_ref()
        .is_some_and(|banner| banner.repo_id == Some(repo_id))
    {
        state.banner_error = None;
    }
}

pub(super) fn push_diagnostic(repo_state: &mut RepoState, kind: DiagnosticKind, message: String) {
    const MAX_DIAGNOSTICS: usize = 200;
    repo_state.diagnostics.push(DiagnosticEntry {
        time: SystemTime::now(),
        kind,
        message,
    });
    if repo_state.diagnostics.len() > MAX_DIAGNOSTICS {
        let extra = repo_state.diagnostics.len() - MAX_DIAGNOSTICS;
        repo_state.diagnostics.drain(0..extra);
    }
}

pub(super) fn handle_session_persist_result(
    state: &mut AppState,
    repo_id: Option<RepoId>,
    action: &'static str,
    result: io::Result<()>,
) {
    let Err(error) = result else {
        return;
    };
    // The untranslated action key keeps the daily log greppable regardless
    // of the UI locale.
    worktree_core::applog_warn!("session persist failed ({action}): {error}");
    // The action phrase arrives as the historical English `&'static str`
    // (gettext-style): look it up dynamically so zh-CN users see the
    // translated phrase, while the en fallback returns the input verbatim.
    let action = rust_i18n::t!(action).into_owned();
    let message = rust_i18n::t!(
        "store.reducer.persist_failed",
        action = action,
        error = error
    )
    .to_string();
    push_notification(state, AppNotificationKind::Error, message.clone());
    if let Some(repo_id) = repo_id
        && let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
    {
        push_diagnostic(repo_state, DiagnosticKind::Error, message);
    }
}

/// Staging and unstaging a hunk or line is a direct, visible edit: the diff
/// redraws without the change, which is the whole feedback the user needs. A
/// toast for each one just stacks up while working through a file.
fn command_success_is_worth_announcing(command: &RepoCommandKind) -> bool {
    !matches!(
        command,
        RepoCommandKind::StageHunk | RepoCommandKind::UnstageHunk | RepoCommandKind::AutoFetchAll
    )
}

fn command_failure_is_worth_announcing(command: &RepoCommandKind) -> bool {
    !matches!(command, RepoCommandKind::AutoFetchAll)
}

pub(super) fn push_command_log(
    repo_state: &mut RepoState,
    ok: bool,
    command: &RepoCommandKind,
    output: &CommandOutput,
    error: Option<&Error>,
) {
    const MAX_COMMAND_LOG: usize = 200;

    let (command_text, summary) = summarize_command(command, output, ok, error);

    repo_state.command_log.push(CommandLogEntry {
        time: SystemTime::now(),
        ok,
        command: command_text,
        summary,
        stdout: output.stdout.clone(),
        stderr: if output.stderr.is_empty() {
            error.map(format_error_for_user).unwrap_or_default()
        } else {
            output.stderr.clone()
        },
        announce_success: command_success_is_worth_announcing(command),
        announce_failure: command_failure_is_worth_announcing(command),
    });
    if repo_state.command_log.len() > MAX_COMMAND_LOG {
        let extra = repo_state.command_log.len() - MAX_COMMAND_LOG;
        repo_state.command_log.drain(0..extra);
    }
}

pub(super) fn push_action_log(
    repo_state: &mut RepoState,
    ok: bool,
    command: String,
    summary: String,
    error: Option<&Error>,
) {
    const MAX_COMMAND_LOG: usize = 200;

    repo_state.command_log.push(CommandLogEntry {
        time: SystemTime::now(),
        ok,
        command,
        summary,
        stdout: String::new(),
        stderr: error.map(format_error_for_user).unwrap_or_default(),
        announce_success: true,
        announce_failure: true,
    });
    if repo_state.command_log.len() > MAX_COMMAND_LOG {
        let extra = repo_state.command_log.len() - MAX_COMMAND_LOG;
        repo_state.command_log.drain(0..extra);
    }
}

pub(super) fn conflict_autosolve_telemetry_command(
    mode: ConflictAutosolveMode,
    path: Option<&Path>,
) -> String {
    let mut command = format!("telemetry.conflict_autosolve.{}", mode.as_str());
    if let Some(path) = path {
        command.push(' ');
        command.push_str(
            &path
                .to_str()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{path:?}")),
        );
    }
    command
}

pub(super) fn conflict_autosolve_telemetry_summary(
    mode: ConflictAutosolveMode,
    path: Option<&Path>,
    total_conflicts_before: usize,
    total_conflicts_after: usize,
    unresolved_before: usize,
    unresolved_after: usize,
    stats: ConflictAutosolveStats,
) -> String {
    let resolved = stats.total_resolved();
    let mode_label = match mode {
        ConflictAutosolveMode::Safe => {
            rust_i18n::t!("store.reducer.autosolve_mode_safe").to_string()
        }
        ConflictAutosolveMode::Regex => {
            rust_i18n::t!("store.reducer.autosolve_mode_regex").to_string()
        }
        ConflictAutosolveMode::History => {
            rust_i18n::t!("store.reducer.autosolve_mode_history").to_string()
        }
    };

    let path_label = path
        .map(|p| rust_i18n::t!("store.reducer.autosolve_in_path", path = p.display()).to_string())
        .unwrap_or_default();

    let mut details = Vec::new();
    if stats.pass1 > 0 {
        details.push(format!("pass1={}", stats.pass1));
    }
    if stats.pass2_split > 0 {
        details.push(format!("pass2_split={}", stats.pass2_split));
    }
    if stats.pass1_after_split > 0 {
        details.push(format!("pass1_after_split={}", stats.pass1_after_split));
    }
    if stats.regex > 0 {
        details.push(format!("regex={}", stats.regex));
    }
    if stats.history > 0 {
        details.push(format!("history={}", stats.history));
    }
    let details = if details.is_empty() {
        "details=none".to_string()
    } else {
        details.join(", ")
    };

    rust_i18n::t!(
        "store.reducer.autosolve_summary",
        mode = mode_label,
        resolved = resolved,
        before = unresolved_before,
        after = unresolved_after,
        total_before = total_conflicts_before,
        total_after = total_conflicts_after,
        path = path_label,
        details = details
    )
    .to_string()
}

/// A sequencer command (rebase / cherry-pick / continue) that reported Ok
/// with a non-zero exit paused at a conflict — the backend maps only
/// genuine pauses to Ok — so its summary must not read as completed.
fn sequencer_paused(output: &CommandOutput) -> bool {
    output.exit_code.is_some_and(|code| code != 0)
}

/// Continue/abort share one UI action and backend entry point for rebases,
/// `git am`, and cherry-picks. Use the command that actually ran so native
/// cherry-picks are not recorded as rebases in action history.
fn sequencer_operation_label(output: &CommandOutput, error: Option<&Error>) -> String {
    let is_cherry_pick = |command: &str| command.trim_start().starts_with("git cherry-pick");
    if is_cherry_pick(&output.command) {
        return rust_i18n::t!("store.reducer.label_cherry_pick").to_string();
    }
    if let Some((command, _)) = error.and_then(try_format_git_backend_error)
        && is_cherry_pick(&command)
    {
        return rust_i18n::t!("store.reducer.label_cherry_pick").to_string();
    }
    rust_i18n::t!("store.reducer.label_rebase").to_string()
}

fn summarize_command(
    command: &RepoCommandKind,
    output: &CommandOutput,
    ok: bool,
    error: Option<&Error>,
) -> (String, String) {
    use worktree_core::services::ConflictSide;

    if !ok {
        let label = match command {
            RepoCommandKind::FetchAll | RepoCommandKind::AutoFetchAll => {
                rust_i18n::t!("store.reducer.label_fetch").to_string()
            }
            RepoCommandKind::PruneMergedBranches => {
                rust_i18n::t!("store.reducer.label_prune_merged_branches").to_string()
            }
            RepoCommandKind::PruneLocalTags => {
                rust_i18n::t!("store.reducer.label_prune_local_tags").to_string()
            }
            RepoCommandKind::Pull { .. } => rust_i18n::t!("store.reducer.label_pull").to_string(),
            RepoCommandKind::PullBranch { .. } => {
                rust_i18n::t!("store.reducer.label_pull").to_string()
            }
            RepoCommandKind::MergeRef { .. } => {
                rust_i18n::t!("store.reducer.label_merge").to_string()
            }
            RepoCommandKind::SquashRef { .. } => {
                rust_i18n::t!("store.reducer.label_squash").to_string()
            }
            RepoCommandKind::Push => rust_i18n::t!("store.reducer.label_push").to_string(),
            RepoCommandKind::PushAfterCommit { .. } => {
                rust_i18n::t!("store.reducer.label_push_after_commit").to_string()
            }
            RepoCommandKind::ForcePush => {
                rust_i18n::t!("store.reducer.label_force_push").to_string()
            }
            RepoCommandKind::ForcePushWithLease { .. } => {
                rust_i18n::t!("store.reducer.label_force_push_with_lease").to_string()
            }
            RepoCommandKind::PushMergeRequest { .. } => {
                rust_i18n::t!("store.reducer.label_push_merge_request").to_string()
            }
            RepoCommandKind::PushSetUpstream { .. } => {
                rust_i18n::t!("store.reducer.label_push").to_string()
            }
            RepoCommandKind::SetUpstreamBranch { .. } => {
                rust_i18n::t!("store.reducer.label_set_tracking_upstream").to_string()
            }
            RepoCommandKind::UnsetUpstreamBranch { .. } => {
                rust_i18n::t!("store.reducer.label_unlink_upstream").to_string()
            }
            RepoCommandKind::FastForwardBranch { .. } => {
                rust_i18n::t!("store.reducer.label_fast_forward_branch").to_string()
            }
            RepoCommandKind::DeleteRemoteBranch { .. } => {
                rust_i18n::t!("store.reducer.label_delete_remote_branch").to_string()
            }
            RepoCommandKind::DeleteRemoteBranches { .. } => {
                rust_i18n::t!("store.reducer.label_delete_remote_branches").to_string()
            }
            RepoCommandKind::PushTag { .. } => {
                rust_i18n::t!("store.reducer.label_push_tag").to_string()
            }
            RepoCommandKind::DeleteRemoteTag { .. } => {
                rust_i18n::t!("store.reducer.label_delete_remote_tag").to_string()
            }
            RepoCommandKind::Reset { .. } => rust_i18n::t!("store.reducer.label_reset").to_string(),
            RepoCommandKind::SquashCommits { .. } => {
                rust_i18n::t!("store.reducer.label_squash").to_string()
            }
            RepoCommandKind::Rebase { .. } => {
                rust_i18n::t!("store.reducer.label_rebase").to_string()
            }
            RepoCommandKind::RebaseContinue | RepoCommandKind::RebaseAbort => {
                sequencer_operation_label(output, error)
            }
            RepoCommandKind::BisectStart { .. } => {
                rust_i18n::t!("store.reducer.label_bisect_start").to_string()
            }
            RepoCommandKind::BisectMark { verdict, .. } => {
                rust_i18n::t!("store.reducer.label_bisect_mark", kind = verdict.as_str())
                    .to_string()
            }
            RepoCommandKind::BisectReset => {
                rust_i18n::t!("store.reducer.label_bisect_reset").to_string()
            }
            RepoCommandKind::InteractiveRebase { interactive, .. } => {
                if *interactive {
                    rust_i18n::t!("store.reducer.label_interactive_rebase").to_string()
                } else {
                    rust_i18n::t!("store.reducer.label_rebase").to_string()
                }
            }
            RepoCommandKind::InteractiveCherryPick { .. } => {
                rust_i18n::t!("store.reducer.label_cherry_pick").to_string()
            }
            RepoCommandKind::CherryPick { .. } => {
                rust_i18n::t!("store.reducer.label_cherry_pick").to_string()
            }
            RepoCommandKind::MergeAbort => rust_i18n::t!("store.reducer.label_merge").to_string(),
            RepoCommandKind::CreateTag { .. } => {
                rust_i18n::t!("store.reducer.label_tag").to_string()
            }
            RepoCommandKind::DeleteTag { .. } => {
                rust_i18n::t!("store.reducer.label_tag").to_string()
            }
            RepoCommandKind::AddRemote { .. } => {
                rust_i18n::t!("store.reducer.label_remote").to_string()
            }
            RepoCommandKind::RemoveRemote { .. } => {
                rust_i18n::t!("store.reducer.label_remote").to_string()
            }
            RepoCommandKind::SetRemoteUrl { .. } => {
                rust_i18n::t!("store.reducer.label_remote").to_string()
            }
            RepoCommandKind::SetRemoteSshKey { .. } => {
                rust_i18n::t!("store.reducer.label_remote_ssh_key").to_string()
            }
            RepoCommandKind::CheckoutConflict { side, .. } => match side {
                ConflictSide::Ours => {
                    rust_i18n::t!("store.reducer.label_checkout_ours").to_string()
                }
                ConflictSide::Theirs => {
                    rust_i18n::t!("store.reducer.label_checkout_theirs").to_string()
                }
            },
            RepoCommandKind::AcceptConflictDeletion { .. } => {
                rust_i18n::t!("store.reducer.label_accept_deletion").to_string()
            }
            RepoCommandKind::CheckoutConflictBase { .. } => {
                rust_i18n::t!("store.reducer.label_checkout_base").to_string()
            }
            RepoCommandKind::LaunchMergetool { .. } => {
                rust_i18n::t!("store.reducer.label_mergetool").to_string()
            }
            RepoCommandKind::SaveWorktreeFile { .. } => {
                rust_i18n::t!("store.reducer.label_save_file").to_string()
            }
            RepoCommandKind::AppendGitignorePatterns { .. } => {
                rust_i18n::t!("store.reducer.label_update_gitignore").to_string()
            }
            RepoCommandKind::ExportPatch { .. } | RepoCommandKind::ApplyPatch { .. } => {
                rust_i18n::t!("store.reducer.label_patch").to_string()
            }
            RepoCommandKind::ArchiveZip { .. } => {
                rust_i18n::t!("store.reducer.label_archive").to_string()
            }
            RepoCommandKind::Cleanup => rust_i18n::t!("store.reducer.label_cleanup").to_string(),
            RepoCommandKind::AddWorktree { .. }
            | RepoCommandKind::RemoveWorktree { .. }
            | RepoCommandKind::ForceRemoveWorktree { .. } => {
                rust_i18n::t!("store.reducer.label_worktree").to_string()
            }
            RepoCommandKind::AddSubmodule { .. }
            | RepoCommandKind::UpdateSubmodules { .. }
            | RepoCommandKind::LoadSubmodule { .. }
            | RepoCommandKind::ChangeSubmodulePointer { .. }
            | RepoCommandKind::RemoveSubmodule { .. } => {
                rust_i18n::t!("store.reducer.label_submodule").to_string()
            }
            RepoCommandKind::StageHunk | RepoCommandKind::UnstageHunk => {
                rust_i18n::t!("store.reducer.label_hunk").to_string()
            }
            RepoCommandKind::ApplyWorktreePatch { reverse } => {
                if *reverse {
                    rust_i18n::t!("store.reducer.label_discard").to_string()
                } else {
                    rust_i18n::t!("store.reducer.label_patch").to_string()
                }
            }
        };
        if let Some(error) = error
            && let Some((git_command, details)) = try_format_git_backend_error(error)
        {
            return (
                git_command,
                rust_i18n::t!(
                    "store.reducer.cmd_failed_details",
                    label = label,
                    details = details
                )
                .to_string(),
            );
        }

        return (
            output.command.clone().if_empty_else(|| label.clone()),
            error
                .map(|e| {
                    rust_i18n::t!(
                        "store.reducer.cmd_failed_details",
                        label = label,
                        details = format_error_for_user(e)
                    )
                    .to_string()
                })
                .unwrap_or_else(|| {
                    rust_i18n::t!("store.reducer.cmd_failed", label = label).to_string()
                }),
        );
    }

    let summary = match command {
        RepoCommandKind::FetchAll | RepoCommandKind::AutoFetchAll => {
            if output.stderr.trim().is_empty() && output.stdout.trim().is_empty() {
                rust_i18n::t!("store.reducer.fetch_up_to_date").to_string()
            } else {
                rust_i18n::t!("store.reducer.fetch_synchronized").to_string()
            }
        }
        RepoCommandKind::PruneMergedBranches => {
            rust_i18n::t!("store.reducer.prune_merged_branches_done").to_string()
        }
        RepoCommandKind::PruneLocalTags => {
            rust_i18n::t!("store.reducer.prune_local_tags_done").to_string()
        }
        RepoCommandKind::Pull { .. } => {
            if output.stdout.contains("Already up to date") {
                rust_i18n::t!("store.reducer.pull_up_to_date").to_string()
            } else if output.stdout.starts_with("Updating") {
                rust_i18n::t!("store.reducer.pull_fast_forwarded").to_string()
            } else if output.stdout.starts_with("Merge") {
                rust_i18n::t!("store.reducer.pull_merged").to_string()
            } else if output.stdout.contains("Successfully rebased") {
                rust_i18n::t!("store.reducer.pull_rebase_complete").to_string()
            } else {
                rust_i18n::t!("store.reducer.pull_done").to_string()
            }
        }
        RepoCommandKind::PullBranch { remote, branch } => {
            let base = if output.stdout.contains("Already up to date") {
                rust_i18n::t!("store.reducer.base_up_to_date")
            } else if output.stdout.starts_with("Updating") {
                rust_i18n::t!("store.reducer.base_fast_forwarded")
            } else if output.stdout.starts_with("Merge") {
                rust_i18n::t!("store.reducer.base_merged")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            rust_i18n::t!(
                "store.reducer.pull_branch",
                remote = remote,
                branch = branch,
                base = base
            )
            .to_string()
        }
        RepoCommandKind::MergeRef { reference } => {
            let base = if output.stdout.contains("Already up to date") {
                rust_i18n::t!("store.reducer.base_up_to_date")
            } else if output.stdout.contains("Fast-forward")
                || output.stdout.starts_with("Updating")
            {
                rust_i18n::t!("store.reducer.base_fast_forwarded")
            } else if output.stdout.contains("Merge made by") {
                rust_i18n::t!("store.reducer.base_merged")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            rust_i18n::t!(
                "store.reducer.merge_ref",
                reference = reference,
                base = base
            )
            .to_string()
        }
        RepoCommandKind::SquashRef { reference } => {
            let base = if output.stdout.contains("Already up to date") {
                rust_i18n::t!("store.reducer.base_up_to_date")
            } else if output.stdout.contains("Squash commit -- not updating HEAD")
                || output
                    .stdout
                    .contains("Automatic merge went well; stopped before committing as requested")
            {
                rust_i18n::t!("store.reducer.base_staged")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            rust_i18n::t!(
                "store.reducer.squash_ref",
                reference = reference,
                base = base
            )
            .to_string()
        }
        RepoCommandKind::Push => {
            if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.push_uptodate").to_string()
            } else {
                rust_i18n::t!("store.reducer.push_done").to_string()
            }
        }
        RepoCommandKind::PushAfterCommit { set_upstream, .. } => {
            let base = if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.base_everything_up_to_date")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            if *set_upstream {
                rust_i18n::t!("store.reducer.push_after_commit_u_done", base = base).to_string()
            } else {
                rust_i18n::t!("store.reducer.push_after_commit_done", base = base).to_string()
            }
        }
        RepoCommandKind::ForcePush => {
            if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.force_push_uptodate").to_string()
            } else {
                rust_i18n::t!("store.reducer.force_push_done").to_string()
            }
        }
        RepoCommandKind::ForcePushWithLease { .. } => {
            if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.force_push_lease_uptodate").to_string()
            } else {
                rust_i18n::t!("store.reducer.force_push_lease_done").to_string()
            }
        }
        RepoCommandKind::PushMergeRequest { .. } => {
            if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.push_merge_request_uptodate").to_string()
            } else {
                rust_i18n::t!("store.reducer.push_merge_request_done").to_string()
            }
        }
        RepoCommandKind::PushSetUpstream { remote, branch } => {
            let base = if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!("store.reducer.base_everything_up_to_date")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            rust_i18n::t!(
                "store.reducer.push_set_upstream",
                remote = remote,
                branch = branch,
                base = base
            )
            .to_string()
        }
        RepoCommandKind::SetUpstreamBranch { branch, upstream } => rust_i18n::t!(
            "store.reducer.upstream_set",
            branch = branch,
            upstream = upstream
        )
        .to_string(),
        RepoCommandKind::UnsetUpstreamBranch { branch } => {
            rust_i18n::t!("store.reducer.upstream_unlinked", branch = branch).to_string()
        }
        RepoCommandKind::FastForwardBranch { branch } => {
            // `git merge --ff-only` says this when the branch already sits on
            // the upstream tip; the fetch path is silent when nothing moved.
            if output.stdout.contains("Already up to date") {
                rust_i18n::t!(
                    "store.reducer.branch_fast_forward_up_to_date",
                    branch = branch
                )
                .to_string()
            } else {
                rust_i18n::t!("store.reducer.branch_fast_forwarded", branch = branch).to_string()
            }
        }
        RepoCommandKind::DeleteRemoteBranch { remote, branch } => rust_i18n::t!(
            "store.reducer.remote_branch_deleted",
            remote = remote,
            branch = branch
        )
        .to_string(),
        RepoCommandKind::DeleteRemoteBranches { remote, branches } => {
            let noun = crate::name_summary::branch_noun(branches.len());
            rust_i18n::t!(
                "store.reducer.remote_branches_deleted",
                count = branches.len(),
                noun = noun,
                remote = remote
            )
            .to_string()
        }
        RepoCommandKind::PushTag { remote, name } => {
            if output.stderr.contains("Everything up-to-date") {
                rust_i18n::t!(
                    "store.reducer.tag_already_up_to_date",
                    name = name,
                    remote = remote
                )
                .to_string()
            } else {
                rust_i18n::t!("store.reducer.tag_pushed", name = name, remote = remote).to_string()
            }
        }
        RepoCommandKind::DeleteRemoteTag { remote, name } => rust_i18n::t!(
            "store.reducer.remote_tag_deleted",
            name = name,
            remote = remote
        )
        .to_string(),
        RepoCommandKind::CheckoutConflict { side, .. } => match side {
            ConflictSide::Ours => rust_i18n::t!("store.reducer.resolved_ours").to_string(),
            ConflictSide::Theirs => rust_i18n::t!("store.reducer.resolved_theirs").to_string(),
        },
        RepoCommandKind::AcceptConflictDeletion { path } => {
            rust_i18n::t!("store.reducer.resolved_by_deletion", path = path.display()).to_string()
        }
        RepoCommandKind::CheckoutConflictBase { path } => {
            rust_i18n::t!("store.reducer.resolved_base", path = path.display()).to_string()
        }
        RepoCommandKind::LaunchMergetool { path, .. } => {
            rust_i18n::t!("store.reducer.mergetool_resolved", path = path.display()).to_string()
        }
        RepoCommandKind::SaveWorktreeFile { path, stage } => {
            if *stage {
                rust_i18n::t!("store.reducer.saved_and_staged", path = path.display()).to_string()
            } else {
                rust_i18n::t!("store.reducer.saved", path = path.display()).to_string()
            }
        }
        // Deliberately "added to .gitignore" rather than "ignored": a later
        // negation, a nested .gitignore or .git/info/exclude can still win, and
        // promising an outcome we did not verify would be a lie the user only
        // catches when the file stays in the list.
        // The worker skips the write when every pattern is already there, and
        // announcing "Added …" for a run that changed nothing would send the
        // user looking for a file that has not moved.
        RepoCommandKind::AppendGitignorePatterns { patterns } => {
            if output.stdout.trim() == worktree_core::gitignore::NOTHING_TO_ADD {
                rust_i18n::t!("store.reducer.gitignore_nothing_added").to_string()
            } else {
                match patterns.as_slice() {
                    [pattern] => rust_i18n::t!("store.reducer.gitignore_added", pattern = pattern)
                        .to_string(),
                    patterns => {
                        rust_i18n::t!("store.reducer.gitignore_added_many", count = patterns.len())
                            .to_string()
                    }
                }
            }
        }
        RepoCommandKind::Reset { mode, target } => {
            let mode = match mode {
                worktree_core::services::ResetMode::Soft => "soft",
                worktree_core::services::ResetMode::Mixed => "mixed",
                worktree_core::services::ResetMode::Hard => "hard",
            };
            rust_i18n::t!("store.reducer.reset_done", mode = mode, target = target).to_string()
        }
        RepoCommandKind::SquashCommits { count, .. } => {
            rust_i18n::t!("store.reducer.squash_commits_done", count = count).to_string()
        }
        RepoCommandKind::Rebase { onto } => {
            rust_i18n::t!("store.reducer.rebase_onto_done", onto = onto).to_string()
        }
        RepoCommandKind::RebaseContinue => {
            let operation = sequencer_operation_label(output, None);
            if sequencer_paused(output) {
                rust_i18n::t!("store.reducer.sequencer_paused_next", operation = operation)
                    .to_string()
            } else {
                rust_i18n::t!("store.reducer.sequencer_continued", operation = operation)
                    .to_string()
            }
        }
        RepoCommandKind::RebaseAbort => rust_i18n::t!(
            "store.reducer.sequencer_aborted",
            operation = sequencer_operation_label(output, None)
        )
        .to_string(),
        RepoCommandKind::BisectStart { .. } => {
            rust_i18n::t!("store.reducer.bisect_started").to_string()
        }
        RepoCommandKind::BisectMark { verdict, .. } => {
            // Git prints `<sha> is the first bad commit` (stdout) once the
            // range collapses; that line is the answer the whole session
            // exists for, so surface it instead of a generic "marked".
            if let Some(sha) = output
                .stdout
                .lines()
                .find(|line| line.contains("is the first bad commit"))
                .and_then(|line| line.split_whitespace().next())
            {
                rust_i18n::t!("store.reducer.bisect_first_bad", sha = sha).to_string()
            } else {
                rust_i18n::t!("store.reducer.bisect_marked", kind = verdict.as_str()).to_string()
            }
        }
        RepoCommandKind::BisectReset => {
            rust_i18n::t!("store.reducer.bisect_reset_done").to_string()
        }
        RepoCommandKind::InteractiveRebase { base, interactive } => {
            let state = if sequencer_paused(output) {
                rust_i18n::t!("store.reducer.state_paused")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            if *interactive {
                rust_i18n::t!(
                    "store.reducer.interactive_rebase_state",
                    base = base,
                    state = state
                )
                .to_string()
            } else {
                rust_i18n::t!(
                    "store.reducer.rebase_onto_state",
                    base = base,
                    state = state
                )
                .to_string()
            }
        }
        RepoCommandKind::InteractiveCherryPick { entries } => {
            let state = if sequencer_paused(output) {
                rust_i18n::t!("store.reducer.state_paused")
            } else {
                rust_i18n::t!("store.reducer.base_completed")
            };
            rust_i18n::t!(
                "store.reducer.cherry_pick_state",
                count = entries.len(),
                state = state
            )
            .to_string()
        }
        RepoCommandKind::CherryPick {
            commit_id,
            commit,
            summary,
            ..
        } => {
            if output
                .stdout
                .contains("WORKTREE_CHERRY_PICK_ALREADY_APPLIED")
            {
                rust_i18n::t!("store.reducer.cherry_pick_already_applied").to_string()
            } else {
                let sha = commit_id.as_ref();
                let short = sha.get(0..7).unwrap_or(sha);
                let summary = summary.lines().next().unwrap_or("").trim();
                if *commit {
                    rust_i18n::t!(
                        "store.reducer.cherry_picked",
                        sha = short,
                        summary = summary
                    )
                    .to_string()
                } else {
                    rust_i18n::t!(
                        "store.reducer.cherry_picked_no_commit",
                        sha = short,
                        summary = summary
                    )
                    .to_string()
                }
            }
        }
        RepoCommandKind::MergeAbort => rust_i18n::t!("store.reducer.merge_aborted").to_string(),
        RepoCommandKind::CreateTag { name, target, .. } => {
            rust_i18n::t!("store.reducer.tag_created", name = name, target = target).to_string()
        }
        RepoCommandKind::DeleteTag { name } => {
            rust_i18n::t!("store.reducer.tag_deleted", name = name).to_string()
        }
        RepoCommandKind::AddRemote { name, .. } => {
            rust_i18n::t!("store.reducer.remote_added", name = name).to_string()
        }
        RepoCommandKind::RemoveRemote { name } => {
            rust_i18n::t!("store.reducer.remote_removed", name = name).to_string()
        }
        RepoCommandKind::SetRemoteUrl { name, kind, .. } => {
            let kind = match kind {
                worktree_core::services::RemoteUrlKind::Fetch => "fetch",
                worktree_core::services::RemoteUrlKind::Push => "push",
            };
            rust_i18n::t!("store.reducer.remote_url_updated", name = name, kind = kind).to_string()
        }
        RepoCommandKind::SetRemoteSshKey { remote, key } => match key {
            Some(key) => rust_i18n::t!(
                "store.reducer.remote_ssh_key_set",
                name = remote,
                path = key
            )
            .to_string(),
            None => {
                rust_i18n::t!("store.reducer.remote_ssh_key_cleared", name = remote).to_string()
            }
        },
        RepoCommandKind::ExportPatch { dest, .. } => {
            rust_i18n::t!("store.reducer.patch_exported", path = dest.display()).to_string()
        }
        RepoCommandKind::ArchiveZip { dest, .. } => {
            rust_i18n::t!("store.reducer.archive_exported", path = dest.display()).to_string()
        }
        RepoCommandKind::Cleanup => rust_i18n::t!("store.reducer.cleanup_finished").to_string(),
        RepoCommandKind::ApplyPatch { patch } => {
            rust_i18n::t!("store.reducer.patch_applied_to", path = patch.display()).to_string()
        }
        RepoCommandKind::AddWorktree { path, reference } => {
            if let Some(reference) = reference {
                rust_i18n::t!(
                    "store.reducer.worktree_added_ref",
                    path = path.display(),
                    reference = reference
                )
                .to_string()
            } else {
                rust_i18n::t!("store.reducer.worktree_added", path = path.display()).to_string()
            }
        }
        RepoCommandKind::RemoveWorktree { path } => {
            rust_i18n::t!("store.reducer.worktree_removed", path = path.display()).to_string()
        }
        RepoCommandKind::ForceRemoveWorktree { path } => rust_i18n::t!(
            "store.reducer.worktree_force_removed",
            path = path.display()
        )
        .to_string(),
        RepoCommandKind::AddSubmodule { path, .. } => {
            rust_i18n::t!("store.reducer.submodule_added", path = path.display()).to_string()
        }
        RepoCommandKind::UpdateSubmodules { .. } => {
            rust_i18n::t!("store.reducer.submodules_updated").to_string()
        }
        RepoCommandKind::LoadSubmodule { path, .. } => {
            rust_i18n::t!("store.reducer.submodule_loaded", path = path.display()).to_string()
        }
        RepoCommandKind::ChangeSubmodulePointer { path, reference } => rust_i18n::t!(
            "store.reducer.submodule_pointer_updated",
            path = path.display(),
            reference = reference
        )
        .to_string(),
        RepoCommandKind::RemoveSubmodule { path } => {
            rust_i18n::t!("store.reducer.submodule_removed", path = path.display()).to_string()
        }
        RepoCommandKind::StageHunk => rust_i18n::t!("store.reducer.hunk_staged").to_string(),
        RepoCommandKind::UnstageHunk => rust_i18n::t!("store.reducer.hunk_unstaged").to_string(),
        RepoCommandKind::ApplyWorktreePatch { reverse } => {
            if *reverse {
                rust_i18n::t!("store.reducer.changes_discarded").to_string()
            } else {
                rust_i18n::t!("store.reducer.patch_applied").to_string()
            }
        }
    };

    (output.command.clone(), summary)
}

pub(super) fn format_error_for_user(error: &Error) -> String {
    match error.kind() {
        ErrorKind::Git(failure) => failure.to_string(),
        ErrorKind::Backend(message) => message.clone(),
        _ => error.to_string(),
    }
}

pub(super) fn format_failure_summary(label: &str, error: &Error) -> String {
    if let Some((_git_command, details)) = try_format_git_backend_error(error) {
        return rust_i18n::t!(
            "store.reducer.cmd_failed_details",
            label = label,
            details = details
        )
        .to_string();
    }
    rust_i18n::t!(
        "store.reducer.cmd_failed_details",
        label = label,
        details = format_error_for_user(error)
    )
    .to_string()
}

pub(super) fn detect_auth_prompt_kind(error: &Error) -> Option<AuthPromptKind> {
    match error.kind() {
        ErrorKind::Git(failure) => detect_auth_prompt_kind_from_git_failure(failure),
        ErrorKind::Backend(message) => detect_auth_prompt_kind_from_message(message),
        _ => None,
    }
}

pub(super) fn detect_auth_prompt_kind_from_message(message: &str) -> Option<AuthPromptKind> {
    let lower = message.to_ascii_lowercase();

    let host_verification = lower.contains("host key verification failed")
        || lower.contains("the authenticity of host")
        || lower.contains("this key is not known by any other names")
        || (lower.contains("are you sure you want to continue connecting")
            && lower.contains("yes/no"));
    if host_verification {
        return Some(AuthPromptKind::HostVerification);
    }

    let passphrase = lower.contains("could not read passphrase")
        || lower.contains("enter passphrase for key")
        || lower.contains("read_passphrase")
        || lower.contains("passphrase for key")
        || (lower.contains("passphrase") && lower.contains("terminal prompts disabled"));
    let ssh_publickey = lower.contains("permission denied (publickey")
        || (lower.contains("could not read from remote repository") && lower.contains("publickey"));
    if passphrase || ssh_publickey {
        return Some(AuthPromptKind::Passphrase);
    }

    let user_password = lower.contains("could not read username")
        || lower.contains("could not read password")
        || lower.contains("authentication failed")
        || lower.contains("invalid username or password")
        || lower.contains("http basic: access denied")
        || (lower.contains("terminal prompts disabled")
            && (lower.contains("https://")
                || lower.contains("http://")
                || lower.contains("username")
                || lower.contains("password")));
    if user_password {
        return Some(AuthPromptKind::UsernamePassword);
    }

    None
}

pub(super) fn clear_staged_git_auth_env() {
    clear_staged_git_auth();
}

pub(super) fn prepare_staged_git_auth(
    kind: AuthPromptKind,
    username: Option<&str>,
    secret: &str,
) -> Result<StagedGitAuth, Error> {
    let normalized_secret = match kind {
        AuthPromptKind::HostVerification => {
            let trimmed = secret.trim();
            if trimmed.eq_ignore_ascii_case("yes") {
                "yes".to_string()
            } else {
                trimmed.to_string()
            }
        }
        AuthPromptKind::UsernamePassword | AuthPromptKind::Passphrase => secret.to_string(),
    };

    if normalized_secret.trim().is_empty() {
        return Err(Error::new(ErrorKind::Backend(
            rust_i18n::t!("store.reducer.auth_secret_empty").to_string(),
        )));
    }
    if kind.requires_username() && username.unwrap_or_default().trim().is_empty() {
        return Err(Error::new(ErrorKind::Backend(
            rust_i18n::t!("store.reducer.auth_username_empty").to_string(),
        )));
    }

    Ok(StagedGitAuth {
        kind: match kind {
            AuthPromptKind::UsernamePassword => GitAuthKind::UsernamePassword,
            AuthPromptKind::Passphrase => GitAuthKind::Passphrase,
            AuthPromptKind::HostVerification => GitAuthKind::HostVerification,
        },
        username: username.map(ToOwned::to_owned),
        secret: normalized_secret,
    })
}

#[cfg(test)]
pub(super) fn stage_git_auth_env(
    kind: AuthPromptKind,
    username: Option<&str>,
    secret: &str,
) -> Result<(), Error> {
    stage_git_auth(prepare_staged_git_auth(kind, username, secret)?);
    Ok(())
}

fn try_format_git_backend_error(error: &Error) -> Option<(String, String)> {
    match error.kind() {
        ErrorKind::Git(failure) => try_format_structured_git_failure(failure),
        ErrorKind::Backend(message) => try_format_git_backend_error_message(message),
        _ => None,
    }
}

fn try_format_structured_git_failure(failure: &GitFailure) -> Option<(String, String)> {
    let command = failure.command().trim().to_string();
    if !command.starts_with("git ") {
        return None;
    }
    let rendered = render_command_and_output(&command, failure.detail());
    Some((command, rendered))
}

fn detect_auth_prompt_kind_from_git_failure(failure: &GitFailure) -> Option<AuthPromptKind> {
    let stderr = String::from_utf8_lossy(failure.stderr());
    detect_auth_prompt_kind_from_message(&stderr)
        .or_else(|| {
            detect_auth_prompt_kind_from_message(&String::from_utf8_lossy(failure.stdout()))
        })
        .or_else(|| detect_auth_prompt_kind_from_message(&failure.to_string()))
}

/// Whether a failed push was rejected because the remote is ahead
/// (`! [rejected] … (fetch first)`, or the diverged hint
/// "tip of your current branch is behind"). Only those can be recovered by
/// pulling and re-pushing; auth or hook failures must surface normally.
pub(super) fn push_failure_needs_pull_retry(error: &Error) -> bool {
    let ErrorKind::Git(failure) = error.kind() else {
        return false;
    };
    let looks_behind_remote = |text: &str| {
        let lower = text.to_ascii_lowercase();
        lower.contains("(fetch first)")
            || lower.contains("(non-fast-forward)")
            || lower.contains("tip of your current branch is behind")
    };
    looks_behind_remote(&String::from_utf8_lossy(failure.stderr()))
        || looks_behind_remote(&String::from_utf8_lossy(failure.stdout()))
        || failure.detail().is_some_and(looks_behind_remote)
}

fn try_format_git_backend_error_message(message: &str) -> Option<(String, String)> {
    let (command, output) = parse_failed_command_message(message)?;
    if !command.trim_start().starts_with("git ") {
        return None;
    }

    let rendered = render_command_and_output(&command, output.as_deref());
    Some((command, rendered))
}

fn parse_failed_command_message(message: &str) -> Option<(String, Option<String>)> {
    if let Some(idx) = message.find(" failed:") {
        let command = message[..idx].trim_end().to_string();
        let mut output = &message[(idx + " failed:".len())..];
        if output.starts_with(' ') {
            output = &output[1..];
        }
        let output = output.trim_end_matches(['\r', '\n']).to_string();
        return Some((command, (!output.is_empty()).then_some(output)));
    }

    let trimmed = message.trim_end_matches(['\r', '\n']);
    if let Some(command) = trimmed.strip_suffix(" failed") {
        return Some((command.trim_end().to_string(), None));
    }

    None
}

fn render_command_and_output(command: &str, output: Option<&str>) -> String {
    let command = command.replace(['\n', '\r'], " ");
    let command = command.trim();

    let output_len = output.map_or(0, |s| s.len());
    let mut rendered = String::with_capacity(command.len() + output_len + 16);
    append_code_block(&mut rendered, command);

    if let Some(output) = output {
        let output = output.trim_end_matches(['\r', '\n']);
        if !output.is_empty() {
            rendered.push_str("\n\n");
            append_code_block(&mut rendered, output);
        }
    }

    rendered
}

fn append_code_block(out: &mut String, text: &str) {
    for (ix, line) in text.lines().enumerate() {
        if ix > 0 {
            out.push('\n');
        }
        out.push_str("    ");
        out.push_str(line);
    }
}

trait IfEmptyElse {
    fn if_empty_else(self, f: impl FnOnce() -> String) -> String;
}

impl IfEmptyElse for String {
    fn if_empty_else(self, f: impl FnOnce() -> String) -> String {
        if self.trim().is_empty() { f() } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppNotificationKind, DiagnosticKind};
    use crate::msg::RepoCommandKind;
    use std::path::Path;
    use worktree_core::domain::{CommitId, DiffArea, DiffTarget, RepoSpec};
    use worktree_core::error::{GitFailure, GitFailureId};
    use worktree_core::services::{PullMode, RemoteUrlKind, ResetMode};

    fn repo_state(id: u64) -> RepoState {
        RepoState::new_opening(
            RepoId(id),
            RepoSpec {
                workdir: PathBuf::from("/tmp/worktree-state-util-tests"),
            },
        )
    }

    fn command_output(command: &str, stdout: &str, stderr: &str) -> CommandOutput {
        CommandOutput {
            command: command.to_string(),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_code: Some(0),
        }
    }

    fn dummy_log_entry(ix: usize) -> CommandLogEntry {
        CommandLogEntry {
            time: SystemTime::UNIX_EPOCH,
            ok: true,
            command: format!("cmd-{ix}"),
            summary: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            announce_success: true,
            announce_failure: true,
        }
    }

    #[test]
    fn diff_reload_effects_cover_image_svg_and_non_file_targets() {
        let repo_id = RepoId(7);
        let repo_state = repo_state(repo_id.0);
        let png = DiffTarget::WorkingTree {
            path: PathBuf::from("img.PNG"),
            area: DiffArea::Unstaged,
        };
        let mut png_effects = Vec::with_capacity(diff_reload_effect_count(&repo_state, &png));
        append_diff_reload_effects(&mut png_effects, &repo_state, repo_id, png.clone());
        assert!(diff_target_wants_image_preview(&png));
        assert!(!diff_target_is_svg(&png));
        assert_eq!(png_effects.len(), 2);
        assert!(matches!(png_effects[0], Effect::LoadDiff { .. }));
        assert!(matches!(png_effects[1], Effect::LoadDiffFileImage { .. }));

        let svg = DiffTarget::WorkingTree {
            path: PathBuf::from("diagram.svg"),
            area: DiffArea::Unstaged,
        };
        let mut svg_effects = Vec::with_capacity(diff_reload_effect_count(&repo_state, &svg));
        append_diff_reload_effects(&mut svg_effects, &repo_state, repo_id, svg.clone());
        assert!(diff_target_wants_image_preview(&svg));
        assert!(diff_target_is_svg(&svg));
        assert_eq!(svg_effects.len(), 3);
        assert!(matches!(svg_effects[2], Effect::LoadDiffFile { .. }));

        let text_no_ext = DiffTarget::WorkingTree {
            path: PathBuf::from("README"),
            area: DiffArea::Unstaged,
        };
        assert!(!diff_target_wants_image_preview(&text_no_ext));
        let mut text_effects =
            Vec::with_capacity(diff_reload_effect_count(&repo_state, &text_no_ext));
        append_diff_reload_effects(&mut text_effects, &repo_state, repo_id, text_no_ext);
        assert_eq!(text_effects.len(), 2);

        let commit_without_path = DiffTarget::Commit {
            commit_id: CommitId("abc123".into()),
            path: None,
        };
        assert!(!diff_target_wants_image_preview(&commit_without_path));
        assert!(!diff_target_is_svg(&commit_without_path));
        let mut commit_effects =
            Vec::with_capacity(diff_reload_effect_count(&repo_state, &commit_without_path));
        append_diff_reload_effects(
            &mut commit_effects,
            &repo_state,
            repo_id,
            commit_without_path,
        );
        assert_eq!(commit_effects.len(), 1);
    }

    #[test]
    fn content_preview_forces_full_content_preview_plan() {
        let mut repo = repo_state(9);
        repo.diff_state.content_preview = true;

        // Working-tree content is read from disk: no patch diff, no file text, no
        // preview-text-file load.
        let worktree = DiffTarget::WorkingTree {
            path: PathBuf::from("src/lib.rs"),
            area: DiffArea::Unstaged,
        };
        let plan = selected_diff_load_plan(&repo, &worktree);
        assert!(!plan.load_patch_diff);
        assert!(!plan.load_file_text);
        assert_eq!(plan.preview_text_side, None);
        assert!(!plan.load_file_image);

        // Commit content reads the New-side blob via a preview text file.
        let commit = DiffTarget::Commit {
            commit_id: CommitId("abc123".into()),
            path: Some(PathBuf::from("src/lib.rs")),
        };
        let plan = selected_diff_load_plan(&repo, &commit);
        assert!(!plan.load_patch_diff);
        assert_eq!(
            plan.preview_text_side,
            Some(worktree_core::domain::DiffPreviewTextSide::New)
        );

        // An image is still loaded as an image, not as text.
        let image = DiffTarget::Commit {
            commit_id: CommitId("abc123".into()),
            path: Some(PathBuf::from("logo.png")),
        };
        let plan = selected_diff_load_plan(&repo, &image);
        assert!(plan.load_file_image);
        assert_eq!(plan.preview_text_side, None);

        // Without the flag, a tracked file still gets a normal patch diff.
        repo.diff_state.content_preview = false;
        assert!(selected_diff_load_plan(&repo, &worktree).load_patch_diff);
    }

    #[test]
    fn preview_only_svg_still_loads_file_text_for_the_code_view() {
        use crate::model::Shared;
        use worktree_core::domain::{FileStatus, RepoStatus};

        let mut repo = repo_state(11);
        let svg_path = PathBuf::from("assets/diagram.svg");
        let png_path = PathBuf::from("assets/logo.png");
        repo.status = Loadable::Ready(Shared::new(RepoStatus {
            unstaged: vec![
                FileStatus {
                    path: svg_path.clone(),
                    kind: FileStatusKind::Untracked,
                    conflict: None,
                },
                FileStatus {
                    path: png_path.clone(),
                    kind: FileStatusKind::Untracked,
                    conflict: None,
                },
            ],
            staged: vec![],
        }));

        // An untracked SVG has no patch, but its source still has to load: the
        // Code view is the only place an SVG's text is ever shown.
        let svg = DiffTarget::WorkingTree {
            path: svg_path,
            area: DiffArea::Unstaged,
        };
        let plan = selected_diff_load_plan(&repo, &svg);
        assert!(!plan.load_patch_diff);
        assert!(plan.load_file_text);
        assert!(plan.load_file_image);
        // Image + preview text + file text is the widest SVG fan-out; it has to
        // stay inside the reload cap that `diff_reload_effect_count` asserts.
        assert!(plan.preview_text_side.is_some());
        assert_eq!(diff_reload_effect_count(&repo, &svg), 3);

        // A non-SVG image has no text view at all.
        let png = DiffTarget::WorkingTree {
            path: png_path,
            area: DiffArea::Unstaged,
        };
        let plan = selected_diff_load_plan(&repo, &png);
        assert!(!plan.load_patch_diff);
        assert!(!plan.load_file_text);
        assert!(plan.load_file_image);

        // Content preview does not suppress the SVG file text either.
        repo.diff_state.content_preview = true;
        assert!(selected_diff_load_plan(&repo, &svg).load_file_text);
    }

    #[test]
    fn refresh_effects_request_expected_loads_and_reset_log_loading_more() {
        let mut primary = repo_state(1);
        primary.set_log_loading_more(true);
        let mut primary_effects = Vec::with_capacity(refresh_primary_effect_capacity());
        append_refresh_primary_effects(&mut primary, &mut primary_effects);
        assert_eq!(primary_effects.len(), 7);
        assert!(!primary.log_loading_more);
        assert!(matches!(primary_effects[0], Effect::LoadHeadBranch { .. }));
        assert!(
            primary_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadStatus { .. }))
        );
        assert!(
            primary_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadAuthorEmails { .. })),
            "primary refresh loads author emails for the surfaces that only know names"
        );
        assert!(
            primary_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadBisectState { .. })),
            "primary refresh loads the bisect snapshot alongside sequencer state"
        );
        assert!(matches!(
            primary_effects[6],
            Effect::LoadLog {
                limit: DEFAULT_LOG_PAGE_SIZE,
                ..
            }
        ));
        assert!(
            !primary_effects.iter().any(|effect| {
                matches!(
                    effect,
                    Effect::LoadWorktreeStatus { .. } | Effect::LoadStagedStatus { .. }
                )
            }),
            "primary refresh should coalesce staged and worktree status into LoadStatus"
        );
        assert!(
            primary_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadRebaseAndMergeState { .. }))
        );

        let mut full = repo_state(2);
        full.set_log_loading_more(true);
        let mut full_effects = Vec::with_capacity(refresh_full_effect_capacity());
        append_refresh_full_effects(&mut full, GitLogSettings::default(), &mut full_effects);
        assert_eq!(full_effects.len(), 10);
        assert!(!full.log_loading_more);
        assert!(
            full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadBisectState { .. })),
            "full refresh loads the bisect snapshot alongside sequencer state"
        );
        assert!(
            full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadAuthorEmails { .. })),
            "full refresh loads author emails — the repo-open path relies on it, and \
             without it the history list has no remote avatars until an unrelated \
             action happens to request them"
        );
        assert!(
            full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadStatus { .. }))
        );
        assert!(
            !full_effects.iter().any(|effect| {
                matches!(
                    effect,
                    Effect::LoadWorktreeStatus { .. } | Effect::LoadStagedStatus { .. }
                )
            }),
            "full refresh should coalesce staged and worktree status into LoadStatus"
        );
        assert!(
            !full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTags { .. })),
            "tags should lazy-load by default instead of refresh_full_effects"
        );
        assert!(
            !full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadRemoteTags { .. })),
            "remote tags should lazy-load from tag-specific UI instead of refresh_full_effects"
        );
        assert!(
            !full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadStashes { .. })),
            "stashes should now lazy-load from the sidebar instead of refresh_full_effects"
        );
        assert!(
            full_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadRebaseAndMergeState { .. }))
        );

        let mut metadata = repo_state(3);
        metadata.set_open(Loadable::Ready(()));
        let mut metadata_effects = Vec::new();
        append_auto_background_metadata_effects(
            &mut metadata,
            GitLogSettings::default(),
            &mut metadata_effects,
        );
        assert!(
            metadata_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadTags { .. })),
            "auto-idle metadata should request LoadTags"
        );
        assert!(
            metadata_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadRemoteTags { .. })),
            "auto-idle metadata should request LoadRemoteTags"
        );
        assert!(
            metadata_effects
                .iter()
                .any(|effect| matches!(effect, Effect::LoadSubmodules { .. })),
            "auto-idle metadata should request LoadSubmodules"
        );
    }

    #[test]
    fn dedup_and_normalize_path_cover_duplicate_and_relative_branches() {
        let deduped = dedup_paths_in_order(vec![
            PathBuf::from("a"),
            PathBuf::from("b"),
            PathBuf::from("a"),
        ]);
        assert_eq!(deduped, vec![PathBuf::from("a"), PathBuf::from("b")]);

        let normalized = normalize_repo_path(PathBuf::from("."));
        assert!(normalized.is_absolute());
    }

    #[test]
    fn push_notification_and_diagnostic_cap_old_entries() {
        let mut state = AppState::default();
        for ix in 0..205 {
            push_notification(
                &mut state,
                AppNotificationKind::Info,
                format!("notification-{ix}"),
            );
        }
        assert_eq!(state.notifications.len(), 200);
        assert_eq!(state.notifications[0].message, "notification-5");

        let mut repo = repo_state(3);
        for ix in 0..205 {
            push_diagnostic(&mut repo, DiagnosticKind::Info, format!("diagnostic-{ix}"));
        }
        assert_eq!(repo.diagnostics.len(), 200);
        assert_eq!(repo.diagnostics[0].message, "diagnostic-5");
    }

    #[test]
    fn command_and_action_logs_use_expected_stderr_and_trim_history() {
        let mut repo = repo_state(4);
        repo.command_log = (0..200).map(dummy_log_entry).collect();
        push_command_log(
            &mut repo,
            true,
            &RepoCommandKind::FetchAll,
            &command_output("git fetch", "", "stderr from git"),
            None,
        );
        assert_eq!(repo.command_log.len(), 200);
        assert_eq!(repo.command_log[0].command, "cmd-1");
        assert_eq!(
            repo.command_log
                .last()
                .expect("last command log entry")
                .stderr,
            "stderr from git"
        );

        repo.command_log = (0..200).map(dummy_log_entry).collect();
        push_action_log(
            &mut repo,
            false,
            "manual action".to_string(),
            "action failed".to_string(),
            Some(&Error::new(ErrorKind::Backend(
                "backend failure".to_string(),
            ))),
        );
        assert_eq!(repo.command_log.len(), 200);
        assert_eq!(repo.command_log[0].command, "cmd-1");
    }

    #[test]
    fn conflict_autosolve_summary_covers_mode_and_detail_variants() {
        let history_summary = conflict_autosolve_telemetry_summary(
            ConflictAutosolveMode::History,
            Some(Path::new("conflict.txt")),
            6,
            3,
            4,
            1,
            ConflictAutosolveStats {
                history: 2,
                ..ConflictAutosolveStats::default()
            },
        );
        assert!(history_summary.contains("(history)"));
        assert!(history_summary.contains("history=2"));
        assert!(history_summary.contains("in conflict.txt"));

        let safe_summary = conflict_autosolve_telemetry_summary(
            ConflictAutosolveMode::Safe,
            None,
            1,
            1,
            1,
            1,
            ConflictAutosolveStats::default(),
        );
        assert!(safe_summary.contains("(safe)"));
        assert!(safe_summary.contains("details=none"));
    }

    #[test]
    fn summarize_command_failure_covers_error_labels() {
        let failing_cases = vec![
            (RepoCommandKind::FetchAll, "Fetch"),
            (
                RepoCommandKind::PruneMergedBranches,
                "Prune merged branches",
            ),
            (RepoCommandKind::PruneLocalTags, "Prune local tags"),
            (
                RepoCommandKind::Pull {
                    mode: PullMode::Default,
                },
                "Pull",
            ),
            (
                RepoCommandKind::PullBranch {
                    remote: "origin".into(),
                    branch: "main".into(),
                },
                "Pull",
            ),
            (
                RepoCommandKind::MergeRef {
                    reference: "feature".into(),
                },
                "Merge",
            ),
            (
                RepoCommandKind::SquashRef {
                    reference: "feature".into(),
                },
                "Squash",
            ),
            (RepoCommandKind::Push, "Push"),
            (RepoCommandKind::ForcePush, "Force push"),
            (
                RepoCommandKind::PushMergeRequest {
                    options: worktree_core::services::MergeRequestPushOptions::default(),
                },
                "Push with merge request",
            ),
            (
                RepoCommandKind::PushSetUpstream {
                    remote: "origin".into(),
                    branch: "main".into(),
                },
                "Push",
            ),
            (
                RepoCommandKind::SetUpstreamBranch {
                    branch: "main".into(),
                    upstream: "origin/main".into(),
                },
                "Set as tracking upstream",
            ),
            (
                RepoCommandKind::UnsetUpstreamBranch {
                    branch: "main".into(),
                },
                "Unlink upstream branch",
            ),
            (
                RepoCommandKind::FastForwardBranch {
                    branch: "main".into(),
                },
                "Fast-forward branch",
            ),
            (
                RepoCommandKind::DeleteRemoteBranch {
                    remote: "origin".into(),
                    branch: "old".into(),
                },
                "Delete remote branch",
            ),
            (
                RepoCommandKind::DeleteRemoteBranches {
                    remote: "origin".into(),
                    branches: vec!["feat/a".into(), "feat/b".into()],
                },
                "Delete remote branches",
            ),
            (
                RepoCommandKind::PushTag {
                    remote: "origin".into(),
                    name: "v1".into(),
                },
                "Push tag",
            ),
            (
                RepoCommandKind::DeleteRemoteTag {
                    remote: "origin".into(),
                    name: "v1".into(),
                },
                "Delete remote tag",
            ),
            (
                RepoCommandKind::Reset {
                    mode: ResetMode::Hard,
                    target: "HEAD~1".into(),
                },
                "Reset",
            ),
            (
                RepoCommandKind::Rebase {
                    onto: "main".into(),
                },
                "Rebase",
            ),
            (RepoCommandKind::RebaseContinue, "Rebase"),
            (RepoCommandKind::RebaseAbort, "Rebase"),
            (
                RepoCommandKind::BisectStart {
                    bad: Some("HEAD".into()),
                    goods: vec!["main".into()],
                },
                "Bisect",
            ),
            (
                RepoCommandKind::BisectMark {
                    verdict: worktree_core::services::BisectVerdict::Good,
                    commit: None,
                },
                "Bisect good",
            ),
            (RepoCommandKind::BisectReset, "Bisect reset"),
            (
                RepoCommandKind::InteractiveRebase {
                    base: "HEAD~3".into(),
                    interactive: true,
                },
                "Interactive rebase",
            ),
            (RepoCommandKind::MergeAbort, "Merge"),
            (
                RepoCommandKind::CreateTag {
                    name: "v2".into(),
                    target: "HEAD".into(),
                    message: None,
                    annotated: false,
                },
                "Tag",
            ),
            (RepoCommandKind::DeleteTag { name: "v2".into() }, "Tag"),
            (
                RepoCommandKind::AddRemote {
                    name: "origin".into(),
                    url: "https://example.com/repo.git".into(),
                },
                "Remote",
            ),
            (
                RepoCommandKind::RemoveRemote {
                    name: "origin".into(),
                },
                "Remote",
            ),
            (
                RepoCommandKind::SetRemoteUrl {
                    name: "origin".into(),
                    url: "https://example.com/repo.git".into(),
                    kind: RemoteUrlKind::Fetch,
                },
                "Remote",
            ),
            (
                RepoCommandKind::SetRemoteSshKey {
                    remote: "origin".into(),
                    key: Some("~/.ssh/id_ed25519".into()),
                },
                "SSH key",
            ),
        ];

        for (command, label) in failing_cases {
            let (rendered_command, summary) =
                summarize_command(&command, &CommandOutput::default(), false, None);
            assert_eq!(rendered_command, label);
            assert_eq!(summary, format!("{label} failed"));
        }
    }

    #[test]
    fn summarize_command_success_covers_status_variants() {
        let (_, fetch_summary) = summarize_command(
            &RepoCommandKind::FetchAll,
            &command_output("git fetch", "synced", ""),
            true,
            None,
        );
        assert_eq!(fetch_summary, "Fetch: Synchronized");

        let gitignore_command = RepoCommandKind::AppendGitignorePatterns {
            patterns: vec!["/build/out.log".to_string()],
        };
        let (_, gitignore_written) = summarize_command(
            &gitignore_command,
            &command_output("Update .gitignore", "", ""),
            true,
            None,
        );
        assert_eq!(gitignore_written, "Added /build/out.log to .gitignore");

        let (_, gitignore_noop) = summarize_command(
            &gitignore_command,
            &command_output(
                "Update .gitignore",
                worktree_core::gitignore::NOTHING_TO_ADD,
                "",
            ),
            true,
            None,
        );
        assert_eq!(
            gitignore_noop, "Already in .gitignore; nothing added",
            "the worker skipped the write, so announcing \"Added …\" would send \
             the user looking for a change that never happened"
        );

        let (_, gitignore_many) = summarize_command(
            &RepoCommandKind::AppendGitignorePatterns {
                patterns: vec!["/a".to_string(), "/b".to_string()],
            },
            &command_output("Update .gitignore", "", ""),
            true,
            None,
        );
        assert_eq!(gitignore_many, "Added 2 patterns to .gitignore");

        let (_, pull_up_to_date) = summarize_command(
            &RepoCommandKind::Pull {
                mode: PullMode::Default,
            },
            &command_output("git pull", "Already up to date", ""),
            true,
            None,
        );
        assert_eq!(pull_up_to_date, "Pull: Already up to date");

        let (_, pull_fast_forward) = summarize_command(
            &RepoCommandKind::Pull {
                mode: PullMode::Default,
            },
            &command_output("git pull", "Updating abc..def", ""),
            true,
            None,
        );
        assert_eq!(pull_fast_forward, "Pull: Fast-forwarded");

        let (_, pull_merged) = summarize_command(
            &RepoCommandKind::Pull {
                mode: PullMode::Default,
            },
            &command_output("git pull", "Merge branch 'feature'", ""),
            true,
            None,
        );
        assert_eq!(pull_merged, "Pull: Merged");

        let (_, pull_rebased) = summarize_command(
            &RepoCommandKind::Pull {
                mode: PullMode::Default,
            },
            &command_output(
                "git pull",
                "Successfully rebased and updated refs/heads/main.",
                "",
            ),
            true,
            None,
        );
        assert_eq!(pull_rebased, "Pull: Rebasing complete");

        let (_, pull_branch_summary) = summarize_command(
            &RepoCommandKind::PullBranch {
                remote: "origin".into(),
                branch: "main".into(),
            },
            &command_output("git pull origin main", "Updating abc..def", ""),
            true,
            None,
        );
        assert_eq!(pull_branch_summary, "Pull origin/main: Fast-forwarded");

        let (_, merge_ref_summary) = summarize_command(
            &RepoCommandKind::MergeRef {
                reference: "feature".into(),
            },
            &command_output("git merge feature", "Fast-forward", ""),
            true,
            None,
        );
        assert_eq!(merge_ref_summary, "Merge feature: Fast-forwarded");

        let (_, squash_ref_summary) = summarize_command(
            &RepoCommandKind::SquashRef {
                reference: "feature".into(),
            },
            &command_output(
                "git merge --squash feature",
                "Squash commit -- not updating HEAD\nAutomatic merge went well; stopped before committing as requested",
                "",
            ),
            true,
            None,
        );
        assert_eq!(squash_ref_summary, "Squash feature: Staged");

        let (_, push_uptodate) = summarize_command(
            &RepoCommandKind::Push,
            &command_output("git push", "", "Everything up-to-date"),
            true,
            None,
        );
        assert_eq!(push_uptodate, "Push: Everything up-to-date");

        let (_, force_push_uptodate) = summarize_command(
            &RepoCommandKind::ForcePush,
            &command_output("git push --force", "", "Everything up-to-date"),
            true,
            None,
        );
        assert_eq!(force_push_uptodate, "Force push: Everything up-to-date");

        let (_, push_upstream_uptodate) = summarize_command(
            &RepoCommandKind::PushSetUpstream {
                remote: "origin".into(),
                branch: "main".into(),
            },
            &command_output("git push -u origin main", "", "Everything up-to-date"),
            true,
            None,
        );
        assert_eq!(
            push_upstream_uptodate,
            "Push -u origin/main: Everything up-to-date"
        );

        let (_, set_upstream_summary) = summarize_command(
            &RepoCommandKind::SetUpstreamBranch {
                branch: "feature".into(),
                upstream: "origin/feature".into(),
            },
            &command_output(
                "git branch --set-upstream-to origin/feature feature",
                "",
                "",
            ),
            true,
            None,
        );
        assert_eq!(
            set_upstream_summary,
            "Branch feature: Upstream set to origin/feature"
        );

        let (_, unset_upstream_summary) = summarize_command(
            &RepoCommandKind::UnsetUpstreamBranch {
                branch: "feature".into(),
            },
            &command_output("git branch --unset-upstream feature", "", ""),
            true,
            None,
        );
        assert_eq!(unset_upstream_summary, "Branch feature: Upstream unlinked");

        let (_, fast_forwarded) = summarize_command(
            &RepoCommandKind::FastForwardBranch {
                branch: "feature".into(),
            },
            &command_output("git fetch . origin/feature:feature", "", ""),
            true,
            None,
        );
        assert_eq!(
            fast_forwarded,
            "Branch feature: Fast-forwarded to its upstream"
        );

        let (_, fast_forward_up_to_date) = summarize_command(
            &RepoCommandKind::FastForwardBranch {
                branch: "feature".into(),
            },
            &command_output("git merge --ff-only", "Already up to date.", ""),
            true,
            None,
        );
        assert_eq!(
            fast_forward_up_to_date,
            "Branch feature: Already up to date with its upstream"
        );

        let (_, push_tag_uptodate) = summarize_command(
            &RepoCommandKind::PushTag {
                remote: "origin".into(),
                name: "v1".into(),
            },
            &command_output("git push origin v1", "", "Everything up-to-date"),
            true,
            None,
        );
        assert_eq!(push_tag_uptodate, "Tag v1 → origin: Already up-to-date");

        let (_, reset_soft) = summarize_command(
            &RepoCommandKind::Reset {
                mode: ResetMode::Soft,
                target: "HEAD~1".into(),
            },
            &command_output("git reset --soft HEAD~1", "", ""),
            true,
            None,
        );
        assert_eq!(reset_soft, "Reset (--soft) HEAD~1: Completed");

        let (_, reset_mixed) = summarize_command(
            &RepoCommandKind::Reset {
                mode: ResetMode::Mixed,
                target: "HEAD~1".into(),
            },
            &command_output("git reset --mixed HEAD~1", "", ""),
            true,
            None,
        );
        assert_eq!(reset_mixed, "Reset (--mixed) HEAD~1: Completed");

        let (_, reset_hard) = summarize_command(
            &RepoCommandKind::Reset {
                mode: ResetMode::Hard,
                target: "HEAD~1".into(),
            },
            &command_output("git reset --hard HEAD~1", "", ""),
            true,
            None,
        );
        assert_eq!(reset_hard, "Reset (--hard) HEAD~1: Completed");

        let (_, rebase_summary) = summarize_command(
            &RepoCommandKind::Rebase {
                onto: "origin/main".into(),
            },
            &command_output("git rebase origin/main", "", ""),
            true,
            None,
        );
        assert_eq!(rebase_summary, "Rebase onto origin/main: Completed");

        let (_, rebase_continue_summary) = summarize_command(
            &RepoCommandKind::RebaseContinue,
            &command_output("git rebase --continue", "", ""),
            true,
            None,
        );
        assert_eq!(rebase_continue_summary, "Rebase: Continued");

        let (_, rebase_abort_summary) = summarize_command(
            &RepoCommandKind::RebaseAbort,
            &command_output("git rebase --abort", "", ""),
            true,
            None,
        );
        assert_eq!(rebase_abort_summary, "Rebase: Aborted");

        let (_, bisect_start_summary) = summarize_command(
            &RepoCommandKind::BisectStart {
                bad: Some("HEAD".into()),
                goods: Vec::new(),
            },
            &command_output("git bisect start HEAD", "", ""),
            true,
            None,
        );
        assert_eq!(bisect_start_summary, "Bisect: Started");

        let (_, bisect_mark_summary) = summarize_command(
            &RepoCommandKind::BisectMark {
                verdict: worktree_core::services::BisectVerdict::Bad,
                commit: None,
            },
            &command_output("git bisect bad", "Bisecting: 3 revisions left to test", ""),
            true,
            None,
        );
        assert_eq!(bisect_mark_summary, "Bisect: Marked bad");

        let (_, bisect_converged_summary) = summarize_command(
            &RepoCommandKind::BisectMark {
                verdict: worktree_core::services::BisectVerdict::Bad,
                commit: None,
            },
            &command_output(
                "git bisect bad",
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef is the first bad commit",
                "",
            ),
            true,
            None,
        );
        assert_eq!(
            bisect_converged_summary,
            "Bisect: deadbeefdeadbeefdeadbeefdeadbeefdeadbeef is the first bad commit"
        );

        let (_, bisect_reset_summary) = summarize_command(
            &RepoCommandKind::BisectReset,
            &command_output("git bisect reset", "", ""),
            true,
            None,
        );
        assert_eq!(bisect_reset_summary, "Bisect: Reset");

        let (_, cherry_pick_continue_summary) = summarize_command(
            &RepoCommandKind::RebaseContinue,
            &command_output("git cherry-pick --continue", "", ""),
            true,
            None,
        );
        assert_eq!(cherry_pick_continue_summary, "Cherry-pick: Continued");

        let (_, cherry_pick_abort_summary) = summarize_command(
            &RepoCommandKind::RebaseAbort,
            &command_output("git cherry-pick --abort", "", ""),
            true,
            None,
        );
        assert_eq!(cherry_pick_abort_summary, "Cherry-pick: Aborted");

        let mut paused_cherry_pick = command_output("git cherry-pick --continue", "", "");
        paused_cherry_pick.exit_code = Some(1);
        let (_, cherry_pick_pause_summary) = summarize_command(
            &RepoCommandKind::RebaseContinue,
            &paused_cherry_pick,
            true,
            None,
        );
        assert_eq!(
            cherry_pick_pause_summary,
            "Cherry-pick: Paused at the next conflict"
        );

        let (_, interactive_rebase_summary) = summarize_command(
            &RepoCommandKind::InteractiveRebase {
                base: "HEAD~3".into(),
                interactive: true,
            },
            &command_output("git rebase -i HEAD~3", "", ""),
            true,
            None,
        );
        assert_eq!(
            interactive_rebase_summary,
            "Interactive rebase onto HEAD~3: Completed"
        );

        // An automated squash rebase (no editor window) reports as "Rebase".
        let (_, squash_rebase_summary) = summarize_command(
            &RepoCommandKind::InteractiveRebase {
                base: "HEAD~3".into(),
                interactive: false,
            },
            &command_output("git rebase -i HEAD~3", "", ""),
            true,
            None,
        );
        assert_eq!(squash_rebase_summary, "Rebase onto HEAD~3: Completed");

        let commit_id = CommitId("abcdef1234567890".into());
        let (_, cherry_pick_summary) = summarize_command(
            &RepoCommandKind::CherryPick {
                commit_id: commit_id.clone(),
                commit: true,
                mainline: None,
                summary: "fix parser\n\nbody".into(),
            },
            &command_output("git cherry-pick abcdef1", "", ""),
            true,
            None,
        );
        assert_eq!(cherry_pick_summary, "Cherry-picked abcdef1: fix parser");

        let (_, cherry_pick_no_commit_summary) = summarize_command(
            &RepoCommandKind::CherryPick {
                commit_id: commit_id.clone(),
                commit: false,
                mainline: None,
                summary: "fix parser".into(),
            },
            &command_output("git cherry-pick --no-commit abcdef1", "", ""),
            true,
            None,
        );
        assert_eq!(
            cherry_pick_no_commit_summary,
            "Cherry-picked abcdef1 without committing: fix parser"
        );

        let (_, cherry_pick_already_applied_summary) = summarize_command(
            &RepoCommandKind::CherryPick {
                commit_id,
                commit: true,
                mainline: None,
                summary: "fix parser".into(),
            },
            &command_output(
                "git cherry-pick abcdef1",
                "WORKTREE_CHERRY_PICK_ALREADY_APPLIED",
                "",
            ),
            true,
            None,
        );
        assert_eq!(
            cherry_pick_already_applied_summary,
            "Current branch already has all the changes from the cherry-picked commit."
        );

        let (_, merge_abort_summary) = summarize_command(
            &RepoCommandKind::MergeAbort,
            &command_output("git merge --abort", "", ""),
            true,
            None,
        );
        assert_eq!(merge_abort_summary, "Merge: Aborted");

        let (_, create_tag_summary) = summarize_command(
            &RepoCommandKind::CreateTag {
                name: "v2".into(),
                target: "HEAD".into(),
                message: None,
                annotated: false,
            },
            &command_output("git tag v2 HEAD", "", ""),
            true,
            None,
        );
        assert_eq!(create_tag_summary, "Tag v2 → HEAD: Created");

        let (_, delete_tag_summary) = summarize_command(
            &RepoCommandKind::DeleteTag { name: "v2".into() },
            &command_output("git tag -d v2", "", ""),
            true,
            None,
        );
        assert_eq!(delete_tag_summary, "Tag v2: Deleted");

        let (_, add_remote_summary) = summarize_command(
            &RepoCommandKind::AddRemote {
                name: "origin".into(),
                url: "https://example.com/repo.git".into(),
            },
            &command_output("git remote add origin ...", "", ""),
            true,
            None,
        );
        assert_eq!(add_remote_summary, "Remote origin: Added");

        let (_, remove_remote_summary) = summarize_command(
            &RepoCommandKind::RemoveRemote {
                name: "origin".into(),
            },
            &command_output("git remote remove origin", "", ""),
            true,
            None,
        );
        assert_eq!(remove_remote_summary, "Remote origin: Removed");

        let (_, set_remote_url_summary) = summarize_command(
            &RepoCommandKind::SetRemoteUrl {
                name: "origin".into(),
                url: "https://example.com/repo.git".into(),
                kind: RemoteUrlKind::Push,
            },
            &command_output("git remote set-url --push origin ...", "", ""),
            true,
            None,
        );
        assert_eq!(set_remote_url_summary, "Remote origin (push): URL updated");

        let (_, set_key_summary) = summarize_command(
            &RepoCommandKind::SetRemoteSshKey {
                remote: "origin".into(),
                key: Some("~/.ssh/id_ed25519".into()),
            },
            &command_output("git config remote.origin.sshkey ...", "", ""),
            true,
            None,
        );
        assert_eq!(
            set_key_summary,
            "Remote origin: SSH key set → ~/.ssh/id_ed25519"
        );

        let (_, cleared_key_summary) = summarize_command(
            &RepoCommandKind::SetRemoteSshKey {
                remote: "origin".into(),
                key: None,
            },
            &command_output("git config --unset remote.origin.sshkey", "", ""),
            true,
            None,
        );
        assert_eq!(cleared_key_summary, "Remote origin: SSH key cleared");
    }

    #[test]
    fn error_format_helpers_cover_non_git_and_failed_suffix_cases() {
        let git_error = Error::new(ErrorKind::Git(GitFailure::new(
            "git fetch --all",
            GitFailureId::CommandFailed,
            Some(128),
            Vec::new(),
            b"fatal: network down\n".to_vec(),
            Some("fatal: network down".to_string()),
        )));
        let formatted = format_failure_summary("Fetch", &git_error);
        assert!(formatted.contains("Fetch failed"));
        assert!(formatted.contains("git fetch --all"));
        assert!(formatted.contains("fatal: network down"));
        assert_eq!(
            format_error_for_user(&git_error),
            "git fetch --all failed: fatal: network down"
        );

        let backend_error = Error::new(ErrorKind::Backend(
            "git fetch --all failed: fatal: network down".to_string(),
        ));
        assert!(format_failure_summary("Fetch", &backend_error).contains("git fetch --all"));

        let io_error = Error::new(ErrorKind::Io(io::ErrorKind::Other));
        let io_rendered = format_error_for_user(&io_error);
        assert_eq!(io_rendered, io_error.to_string());
        assert!(!io_rendered.is_empty());
        assert!(try_format_git_backend_error(&io_error).is_none());
        assert!(try_format_git_backend_error_message("curl failed: timeout").is_none());
        assert_eq!(
            parse_failed_command_message("git status failed"),
            Some(("git status".to_string(), None))
        );

        let rendered = render_command_and_output("git status", Some(""));
        assert!(rendered.contains("    git status"));
        assert!(!rendered.contains("\n\n    "));

        assert_eq!(
            "value".to_string().if_empty_else(|| "fallback".to_string()),
            "value"
        );
    }

    #[test]
    fn detect_auth_prompt_kind_classifies_username_password_passphrase_and_host_verification() {
        assert_eq!(
            detect_auth_prompt_kind_from_message(
                "git pull failed: fatal: could not read Username for 'https://example.com': terminal prompts disabled"
            ),
            Some(crate::model::AuthPromptKind::UsernamePassword)
        );
        assert_eq!(
            detect_auth_prompt_kind_from_message(
                "git push failed: Enter passphrase for key '/home/user/.ssh/id_ed25519': terminal prompts disabled"
            ),
            Some(crate::model::AuthPromptKind::Passphrase)
        );
        assert_eq!(
            detect_auth_prompt_kind_from_message(
                "git clone --progress git@github.com:org/repo.git C:\\git\\repo failed: git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."
            ),
            Some(crate::model::AuthPromptKind::Passphrase)
        );
        assert_eq!(
            detect_auth_prompt_kind_from_message(
                "git pull --no-rebase origin main failed: Host key verification failed.\nfatal: Could not read from remote repository."
            ),
            Some(crate::model::AuthPromptKind::HostVerification)
        );
        assert_eq!(
            detect_auth_prompt_kind_from_message(
                "git fetch origin failed: The authenticity of host 'github.com (140.82.121.3)' can't be established.\nED25519 key fingerprint is: SHA256:+DiY...\nAre you sure you want to continue connecting (yes/no/[fingerprint])?"
            ),
            Some(crate::model::AuthPromptKind::HostVerification)
        );
        assert!(detect_auth_prompt_kind_from_message("git status failed").is_none());

        let structured = Error::new(ErrorKind::Git(GitFailure::new(
            "git fetch origin",
            GitFailureId::CommandFailed,
            Some(128),
            Vec::new(),
            b"Host key verification failed.\nfatal: Could not read from remote repository.\n"
                .to_vec(),
            None,
        )));
        assert_eq!(
            detect_auth_prompt_kind(&structured),
            Some(crate::model::AuthPromptKind::HostVerification)
        );
    }

    #[test]
    fn push_failure_needs_pull_retry_matches_only_behind_remote_rejections() {
        let behind_remote = |stderr: &[u8]| {
            Error::new(ErrorKind::Git(GitFailure::new(
                "git push",
                GitFailureId::CommandFailed,
                Some(1),
                Vec::new(),
                stderr.to_vec(),
                None,
            )))
        };

        assert!(push_failure_needs_pull_retry(&behind_remote(
            b" ! [rejected]        HEAD -> main (fetch first)\n"
        )));
        assert!(push_failure_needs_pull_retry(&behind_remote(
            b" ! [rejected]        HEAD -> main (non-fast-forward)\n"
        )));
        assert!(push_failure_needs_pull_retry(&behind_remote(
            b"hint: Updates were rejected because the tip of your current branch is behind\n"
        )));
        // The same rejection reported via the failure detail instead of stderr.
        assert!(push_failure_needs_pull_retry(&Error::new(ErrorKind::Git(
            GitFailure::new(
                "git push",
                GitFailureId::CommandFailed,
                Some(1),
                Vec::new(),
                Vec::new(),
                Some("(fetch first)".to_string()),
            )
        ))));
        // Auth, hook and other failures must surface normally.
        assert!(!push_failure_needs_pull_retry(&behind_remote(
            b"git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."
        )));
        assert!(!push_failure_needs_pull_retry(&behind_remote(
            b"remote: [policy] failed to push some refs to 'repo'\npre-receive hook declined"
        )));
        assert!(!push_failure_needs_pull_retry(&Error::new(
            ErrorKind::Backend("authentication failed".to_string())
        )));
    }

    #[test]
    fn stage_git_auth_env_stages_and_clears_shared_auth_slot() {
        let _lock = crate::store::tests::staged_auth_test_lock();
        worktree_core::auth::clear_staged_git_auth();
        stage_git_auth_env(
            crate::model::AuthPromptKind::UsernamePassword,
            Some("alice"),
            "secret-token",
        )
        .expect("staging auth");

        let staged = worktree_core::auth::take_staged_git_auth().expect("staged auth to exist");
        assert_eq!(staged.username.as_deref(), Some("alice"));
        assert_eq!(staged.secret, "secret-token");
        assert_eq!(
            staged.kind,
            worktree_core::auth::GitAuthKind::UsernamePassword
        );

        stage_git_auth_env(
            crate::model::AuthPromptKind::Passphrase,
            None,
            "ssh-passphrase",
        )
        .expect("staging passphrase");

        let staged =
            worktree_core::auth::take_staged_git_auth().expect("staged passphrase to exist");
        assert_eq!(staged.kind, worktree_core::auth::GitAuthKind::Passphrase);

        stage_git_auth_env(
            crate::model::AuthPromptKind::HostVerification,
            None,
            " YES ",
        )
        .expect("staging host verification");

        let staged =
            worktree_core::auth::take_staged_git_auth().expect("staged host verification to exist");
        assert_eq!(
            staged.kind,
            worktree_core::auth::GitAuthKind::HostVerification
        );
        assert_eq!(staged.secret, "yes");

        clear_staged_git_auth_env();
        assert!(worktree_core::auth::take_staged_git_auth().is_none());
    }
}

#[cfg(test)]
mod delete_remote_branches_summary_tests {
    use super::*;
    use worktree_core::services::CommandOutput;

    fn summary_for(branches: Vec<String>) -> String {
        let (_message, summary) = super::summarize_command(
            &RepoCommandKind::DeleteRemoteBranches {
                remote: "origin".into(),
                branches,
            },
            &CommandOutput::empty_success("git push --delete"),
            true,
            None,
        );
        summary
    }

    #[test]
    fn summary_pluralises_on_the_branch_count() {
        assert_eq!(
            summary_for(vec!["feat/a".into()]),
            "1 remote branch on origin: Deleted"
        );
        assert_eq!(
            summary_for(vec!["feat/a".into(), "feat/b".into()]),
            "2 remote branches on origin: Deleted"
        );
    }
}
