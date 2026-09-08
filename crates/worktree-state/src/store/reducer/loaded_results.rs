use super::util::{
    EffectAccumulator, append_diff_reload_effects, apply_selected_diff_load_plan_state,
    diff_reload_effect_count, push_diagnostic, push_notification, selected_diff_load_plan,
};
use super::{ReduceOutcome, begin_local_action, conflict_interactions, loaded_results, util};
use crate::model::{
    AiCommitContext, AppState, CommitMultiSelection, ConflictFileLoadMode, DiagnosticKind,
    ForeignDiffOrigin, Loadable, RangeSelection, RepoId, RepoLoadsInFlight, RepoState,
    SidebarDataRequest, SidebarMode,
};
use crate::msg::{CommitSelectMode, Effect, Msg};
use crate::store::repo_load_trace;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::domain::{
    Branch, Commit, CommitDetails, CommitFileChange, CommitId, ContributorCommit, EMPTY_TREE_ID,
    FileEntry, FileSource, FileStatus, FileStatusKind, LogPage, RecentCommitMessage, RefMetadata,
    ReflogEntry, Remote, RemoteBranch, RemoteTag, RepoStatus, StashEntry, Submodule, Tag,
    UpstreamDivergence, Worktree, WorktreeDirtySummary,
};
use worktree_core::error::Error;

mod conflict;
mod squash;

pub(super) use conflict::{conflict_file_loaded, load_conflict_file};
pub(super) use squash::{
    prepare_squash, squash_message_preview_loaded, squash_plan_for_repo, squash_rebase_setup_loaded,
};

pub(super) fn file_history_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    result: std::result::Result<LogPage, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.history_state.file_history_path.as_ref() == Some(&path)
    {
        repo_state.history_state.file_history = match result {
            Ok(v) => Loadable::Ready(Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
    }
    Vec::new()
}

/// Stores the best-effort author name → email map. Cosmetic data (author
/// avatars), so failures are dropped silently rather than surfaced — the
/// initials fallback is the pre-existing behavior anyway.
pub(super) fn author_emails_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<FxHashMap<String, String>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && let Ok(emails) = result
    {
        repo_state.author_emails = emails;
    }
    Vec::new()
}

pub(super) fn blame_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    source: worktree_core::domain::BlameSource,
    result: std::result::Result<Vec<worktree_core::services::BlameLine>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.history_state.blame_path.as_ref() == Some(&path)
        && repo_state.history_state.blame_source.as_ref() == Some(&source)
    {
        let retained = repo_state.history_state.retained_blame_while_loading.take();
        repo_state.history_state.blame = match result {
            // Reuse the retained allocation when the reload produced identical
            // annotations, so the view's `Arc`-identity fingerprints and the
            // memoized blame time range stay valid and nothing repaints.
            Ok(v) => Loadable::Ready(match retained {
                Some(prev) if *prev == v => prev,
                _ => Arc::new(v),
            }),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
    }
    Vec::new()
}

pub(super) fn worktrees_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Worktree>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let worktrees = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_worktrees(worktrees);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::WORKTREES)
        {
            effects.push(Effect::LoadWorktrees { repo_id });
        }
    }
    effects
}

pub(super) fn worktree_dirty_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<WorktreeDirtySummary>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    let mut inline_refresh = None;
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(v) => repo_state.set_worktree_dirty(Loadable::Ready(v)),
            // A worktree that cannot be opened (removed, on an unmounted
            // volume) is a routine condition, not something worth a diagnostic
            // banner -- the scan simply reports nothing for it, per worktree,
            // inside the scan.
            //
            // A failure of the whole reply is a different thing: it means the
            // scan never ran (cancelled load, repo handle gone, git runtime
            // unavailable), not that the worktrees are clean. Overwriting a good
            // list with it would blank every row and -- through
            // `selected_worktree_is_gone` below -- drop the selection and close
            // the inline diff the user is reading. Keep the last known counts on
            // screen, and record the error only when there is nothing to keep.
            Err(e) => {
                if !matches!(repo_state.worktree_dirty, Loadable::Ready(_)) {
                    // A cancelled scan is not a failure worth showing: the load
                    // it belonged to was abandoned deliberately, and the trigger
                    // that abandoned it queues another. Anything else is a real
                    // failure and the pane should say so rather than sit on
                    // `Loading` forever.
                    let next = if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                        Loadable::NotLoaded
                    } else {
                        Loadable::Error(e.to_string())
                    };
                    repo_state.set_worktree_dirty(next);
                }
            }
        }
        // A selected worktree row only exists while that worktree has changes.
        // Once it goes clean -- committed, stashed, reverted -- or drops out of a
        // failed scan, its row is gone, and a selection pointing at a row nothing
        // renders leaves the details pane with nothing to show and no way back.
        let selected_worktree_is_gone = repo_state
            .history_state
            .worktree_selection
            .as_ref()
            .is_some_and(|selected| match &repo_state.worktree_dirty {
                Loadable::Ready(dirty) => !dirty.iter().any(|summary| &summary.path == selected),
                // Anything else is the absence of an answer, not the answer that
                // the row is gone. Dropping the selection on it would close the
                // user's open diff every time a scan is cancelled.
                _ => false,
            });
        if selected_worktree_is_gone {
            repo_state.set_worktree_selection(None);
        }
        inline_refresh = refresh_worktree_inline_diff_entries(repo_state);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::WORKTREE_DIRTY)
        {
            // Rebuilt rather than repeated: the selection may have moved while
            // the finished scan was running, and the repeat should carry the
            // file lists of whatever is selected now.
            effects.push(worktree_dirty_effect(repo_state));
        }
    }
    // Outside the borrow above.
    match inline_refresh {
        // The file changed sides (staged <-> unstaged): a different target, so
        // the pane must drop what it is showing and load the new one.
        Some(WorktreeInlineRefresh::Reselect(ix)) => {
            effects.extend(super::diff_selection::select_inline_submodule_diff(
                state, repo_id, ix,
            ));
        }
        // The target did not move, but this scan is the only notice we get that
        // the file behind it may have been edited -- nothing else invalidates a
        // linked worktree's patch.
        Some(WorktreeInlineRefresh::Reload) => {
            effects.extend(
                super::diff_selection::refresh_inline_submodule_selected_diff(state, repo_id),
            );
        }
        None => {}
    }
    effects
}

/// What a landed scan asks of the linked-worktree diff that is open over it.
enum WorktreeInlineRefresh {
    /// The selected file now sits at another index, under another target.
    Reselect(usize),
    /// The selected row still points at the same target; only its contents can
    /// have moved.
    Reload,
}

/// Re-resolves an open linked-worktree inline diff against a scan that has just
/// landed.
///
/// The entry list is a snapshot of the worktree's changed files taken when a row
/// was clicked, while the rows themselves are rebuilt from every scan. Left
/// alone, a rescan that adds or removes a file shifts the row indices out from
/// under `selected_ix`: the pane highlights whichever file now sits at that
/// index, and steps to neighbours that may no longer be changed at all. Submodule
/// inline diffs need none of this -- their entries come from a fixed commit.
///
/// Returns what the caller should do with the diff once the borrow ends. `None`
/// when there is nothing open to refresh -- and when the file the diff shows is
/// no longer changed, in which case the diff is closed outright, the same way a
/// vanished row retires one.
fn refresh_worktree_inline_diff_entries(
    repo_state: &mut RepoState,
) -> Option<WorktreeInlineRefresh> {
    let (entries, selected, origin) = {
        let inline = repo_state.diff_state.inline_submodule_diff.as_ref()?;
        if !matches!(inline.origin, ForeignDiffOrigin::Worktree { .. }) {
            return None;
        }
        let Loadable::Ready(dirty) = &repo_state.worktree_dirty else {
            return None;
        };
        let summary = dirty
            .iter()
            .find(|summary| summary.path == inline.submodule_repo_path)?;
        let entries = crate::model::worktree_inline_diff_entries(summary);
        let selected = inline.entries.get(inline.selected_ix).and_then(|shown| {
            // Matched on the whole target, not the path: a file that is staged
            // *and* modified again appears twice, once per half, and a path-only
            // match always resolves to the staged copy -- so the pane silently
            // swapped sides under anyone reading the unstaged one.
            entries
                .iter()
                .position(|entry| entry.target == shown.target)
                // Only once the exact target is gone does the same path in the
                // other half become the best answer: staging what is on screen
                // retires its unstaged entry, and following the file there beats
                // closing the diff.
                .or_else(|| entries.iter().position(|entry| entry.path == shown.path))
        });
        // The chip labelling the diff reads `origin`, which was captured when the
        // row was clicked. A checkout in that worktree moves the branch under it.
        let origin = ForeignDiffOrigin::Worktree {
            branch: summary.branch.clone(),
            detached: summary.detached,
        };
        (entries, selected, origin)
    };

    let Some(selected) = selected else {
        repo_state.diff_state.inline_submodule_diff = None;
        repo_state.bump_diff_state_rev();
        return None;
    };

    let inline = repo_state.diff_state.inline_submodule_diff.as_mut()?;
    let target_moved = entries[selected].target != inline.target;
    let changed =
        entries != inline.entries || selected != inline.selected_ix || origin != inline.origin;
    inline.entries = entries;
    inline.selected_ix = selected;
    inline.origin = origin;
    if changed {
        repo_state.bump_diff_state_rev();
    }
    Some(if target_moved {
        WorktreeInlineRefresh::Reselect(selected)
    } else {
        WorktreeInlineRefresh::Reload
    })
}

pub(super) fn ref_metadata_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<(String, RefMetadata)>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let ref_metadata = match result {
            Ok(entries) => Loadable::Ready(entries.into_iter().collect()),
            // A backend that does not implement this will never implement it,
            // so latch an empty map rather than `Error` — callers retry on
            // `Error`, which would re-schedule a doomed load on every open.
            Err(e) if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) => {
                Loadable::Ready(FxHashMap::default())
            }
            // Deliberately no diagnostic: this data only decorates picker rows,
            // which fall back to name-only. A transient failure must not raise
            // an error banner on every picker open.
            Err(e) => Loadable::Error(e.to_string()),
        };
        repo_state.set_ref_metadata(ref_metadata);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REF_METADATA)
        {
            effects.push(Effect::LoadRefMetadata { repo_id });
        }
    }
    effects
}

pub(super) fn submodules_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Submodule>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let submodules = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_submodules(submodules);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::SUBMODULES)
        {
            effects.push(Effect::LoadSubmodules { repo_id });
        }
    }
    effects
}

pub(super) fn select_commit(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    select_commit_multi(
        state,
        repo_id,
        commit_id,
        CommitSelectMode::Single,
        None,
        None,
    )
}

pub(super) fn select_commit_multi(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    mode: CommitSelectMode,
    clicked_index: Option<usize>,
    visible_order: Option<Vec<CommitId>>,
) -> Vec<Effect> {
    // The working-tree sentinel can arrive here from a nav-history replay (its
    // snapshot stores `selected_commit`). It is not a commit to select — the
    // uncommitted-changes row owns that id — so route it to its own selection
    // rather than letting it reach the multi-selection machinery, whose entries
    // must always name real commits.
    if commit_id.is_uncommitted() {
        return select_working_tree_summary(state, repo_id);
    }

    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    let log_rev = repo_state.history_state.log_rev;
    let mut sel = repo_state.history_state.multi_selection.clone();

    let focus = match mode {
        CommitSelectMode::Single => {
            collapse_multi_selection_to(&mut sel, commit_id.clone(), clicked_index, log_rev);
            commit_id
        }
        CommitSelectMode::Toggle => {
            if let Some(ix) = sel.commits.iter().position(|c| *c == commit_id) {
                sel.commits.remove(ix);
                let Some(focus) = sel.commits.last().cloned() else {
                    // Toggled the last commit away: clear the selection
                    // entirely (also dissolves the multi-selection).
                    repo_state.set_selected_commit(None);
                    repo_state.set_commit_details(Loadable::NotLoaded);
                    return Vec::new();
                };
                focus
            } else {
                sel.commits.push(commit_id.clone());
                sel.anchor = Some(commit_id.clone());
                sel.anchor_index = clicked_index;
                sel.anchor_log_rev = Some(log_rev);
                commit_id
            }
        }
        CommitSelectMode::Range => {
            let entries = visible_order.as_deref().unwrap_or(&[]);
            let clicked_ix = commit_selection_entry_index(entries, &commit_id, clicked_index);
            match clicked_ix {
                None => {
                    collapse_multi_selection_to(
                        &mut sel,
                        commit_id.clone(),
                        clicked_index,
                        log_rev,
                    );
                }
                Some(clicked_ix) => {
                    let anchor_ix = sel
                        .anchor
                        .as_ref()
                        .and_then(|anchor| {
                            let trusted_hint = sel
                                .anchor_index
                                .filter(|_| sel.anchor_log_rev == Some(log_rev));
                            commit_selection_entry_index(entries, anchor, trusted_hint)
                        })
                        .unwrap_or(clicked_ix);
                    let (a, b) = if anchor_ix <= clicked_ix {
                        (anchor_ix, clicked_ix)
                    } else {
                        (clicked_ix, anchor_ix)
                    };
                    sel.commits = entries[a..=b].to_vec();
                    if sel.anchor.is_none() {
                        sel.anchor = Some(commit_id.clone());
                    }
                    sel.anchor_index = Some(anchor_ix);
                    sel.anchor_log_rev = Some(log_rev);
                }
            }
            commit_id
        }
        CommitSelectMode::PreserveIfSelected => {
            // Keep an existing multi-selection intact when the clicked commit
            // is already part of it — only the focus moves. Otherwise collapse
            // to the clicked commit like a plain click.
            if !sel.commits.contains(&commit_id) {
                collapse_multi_selection_to(&mut sel, commit_id.clone(), clicked_index, log_rev);
            }
            commit_id
        }
    };

    repo_state.set_commit_multi_selection(sel);

    // Two or more selected commits enter "compare" mode: the details pane shows
    // the merged diff of the whole selection — every selected commit's own
    // changes, combined — instead of a plain list. A single commit (or a
    // selection that can't be resolved in the loaded log) falls back to the
    // single/multi-list behavior below.
    let range_pair = {
        let selected = &repo_state.history_state.multi_selection.commits;
        (selected.len() >= 2)
            .then(|| merged_selection_range(repo_state, selected))
            .flatten()
    };

    match range_pair {
        Some((from, to)) => {
            // Keep the focused commit selected (selection-derived UI stays
            // coherent) but don't load its details — the comparison view takes
            // over the details pane, so a single-commit detail load is wasted.
            // Leaving comparison mode is what reconciles the details pane again.
            repo_state.set_selected_commit(Some(focus));
            let from_label = range_endpoint_label(&from);
            let to_label = range_endpoint_label(&to);
            compare_range(
                state,
                repo_id,
                from,
                Some(to),
                from_label,
                to_label,
                ComparisonSource::MultiSelection,
            )
        }
        None => {
            let left_comparison = repo_state.clear_range_comparison();
            let mut effects = select_commit_and_load_details(repo_state, repo_id, focus);
            if left_comparison && effects.is_empty() {
                // `select_commit_and_load_details` no-ops when the focus is
                // already selected — exactly the case when collapsing a
                // comparison back to its focused commit, whose selection was
                // made without a details load. Only comparisons can leave that
                // gap, so re-selecting a commit otherwise stays a no-op.
                effects = reconcile_selected_commit_details(repo_state, repo_id);
            }
            effects
        }
    }
}

/// Emit a details load when the loaded commit details don't describe
/// `selected_commit`. Entering comparison mode deliberately moves the selection
/// without loading details, so every path that leaves comparison mode has to
/// reconcile — otherwise the pane keeps rendering the previously loaded commit's
/// message and file list under a different commit's selection.
fn reconcile_selected_commit_details(repo_state: &mut RepoState, repo_id: RepoId) -> Vec<Effect> {
    let Some(commit_id) = repo_state.history_state.selected_commit.clone() else {
        return Vec::new();
    };
    if matches!(
        &repo_state.history_state.commit_details,
        Loadable::Ready(details) if details.id == commit_id
    ) {
        return Vec::new();
    }
    repo_state.set_commit_details(Loadable::NotLoaded);
    vec![Effect::LoadCommitDetails { repo_id, commit_id }]
}

/// Endpoints for the merged diff of a multi-commit selection. `to` is the newest
/// selected commit; `from` is the *parent* of the oldest selected commit, so the
/// combined patch includes every selected commit's own changes — matching the
/// "merged diff of N commits" the comparison view presents. The history log is
/// newest-first, so the smallest index is the newest commit and the largest is
/// the oldest.
///
/// Falls back to the empty tree as `from` when the oldest selected commit is a
/// root commit (no parent), so the changes it introduces are part of the merged
/// diff like every other selected commit's — using the root itself as the base
/// would silently drop them. Returns `None` unless the log is loaded and every
/// selected commit resolves within it, so the caller leaves comparison mode
/// rather than guess.
fn merged_selection_range(
    repo_state: &RepoState,
    selected: &[CommitId],
) -> Option<(CommitId, CommitId)> {
    let Loadable::Ready(page) = &repo_state.history_state.log else {
        return None;
    };
    let mut newest_ix: Option<usize> = None;
    let mut oldest_ix: Option<usize> = None;
    for id in selected {
        let ix = page.commits.iter().position(|c| &c.id == id)?;
        newest_ix = Some(newest_ix.map_or(ix, |n: usize| n.min(ix)));
        oldest_ix = Some(oldest_ix.map_or(ix, |o: usize| o.max(ix)));
    }
    let newest = &page.commits[newest_ix?];
    let oldest = &page.commits[oldest_ix?];
    let from = oldest
        .parent_ids
        .first()
        .cloned()
        .unwrap_or_else(|| CommitId(EMPTY_TREE_ID.into()));
    Some((from, newest.id.clone()))
}

/// Label for a comparison endpoint in the UI/menus: an abbreviated commit id,
/// or a name for the empty-tree base, whose sha would be meaningless on screen.
fn range_endpoint_label(id: &CommitId) -> String {
    let full = id.as_ref();
    if full == EMPTY_TREE_ID {
        return rust_i18n::t!("store.reducer.start_of_history").to_string();
    }
    full.get(..8).unwrap_or(full).to_string()
}

/// Where a comparison came from, which decides whether the multi-selection
/// describes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ComparisonSource {
    /// The multi-selection *is* the comparison — its merged diff. The selection
    /// stays, and the UI names the comparison after it.
    MultiSelection,
    /// An explicit two-point compare: mark/compare, a branch, a tag, or the
    /// working tree. Any multi-selection left over from earlier clicks describes
    /// something else entirely, so it is dropped rather than left to mislabel
    /// the comparison and supply the wrong preview cards.
    Explicit,
}

/// Enter "compare two points" mode: record the ordered `from`/`to` pair and load
/// the changed-file list. A `to` of `None` compares `from` against the live
/// working tree. The diff pane is left empty (any prior selection is cleared) so
/// the comparison presents the file side-selection first — the user opens an
/// individual file's range diff by clicking it, rather than the whole range
/// patch opening automatically. Reused by multi-commit selection, the
/// mark/compare context-menu flow, and the compare-with-working-tree action.
pub(super) fn compare_range(
    state: &mut AppState,
    repo_id: RepoId,
    from: CommitId,
    to: Option<CommitId>,
    from_label: String,
    to_label: String,
    source: ComparisonSource,
) -> Vec<Effect> {
    let request = {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        if source == ComparisonSource::Explicit {
            repo_state.set_commit_multi_selection(CommitMultiSelection::default());
        }
        repo_state.set_range_selection(Some(RangeSelection {
            from: from.clone(),
            to: to.clone(),
            from_label,
            to_label,
        }));
        repo_state.set_range_files(Loadable::Loading);
        repo_state.begin_range_files_load()
    };

    let mut effects = super::diff_selection::clear_diff_selection(state, repo_id);
    effects.push(Effect::LoadRangeFiles {
        repo_id,
        from,
        to,
        request,
    });
    effects
}

/// Dismiss an active range comparison: clear the selection, the file list, and
/// the range diff from the diff pane, then put the details pane back on the
/// commit that stays selected.
pub(super) fn clear_comparison(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let mut effects = {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        repo_state.set_commit_multi_selection(CommitMultiSelection::default());
        repo_state.clear_range_comparison();
        // Entering the comparison moved `selected_commit` without loading its
        // details, so the pane would otherwise fall back to whichever commit's
        // details happened to be loaded last.
        reconcile_selected_commit_details(repo_state, repo_id)
    };
    effects.extend(super::diff_selection::clear_diff_selection(state, repo_id));
    effects
}

pub(super) fn mark_for_comparison(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    label: String,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.comparison_mark = Some(crate::model::ComparisonMark { commit_id, label });
    }
    Vec::new()
}

pub(super) fn clear_comparison_mark(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.comparison_mark = None;
    }
    Vec::new()
}

/// Compare the marked point (base) against `commit_id` (tip). No-op when nothing
/// is marked or the mark equals the target.
pub(super) fn compare_with_marked(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    label: String,
) -> Vec<Effect> {
    let mark = {
        let Some(repo_state) = state.repos.iter().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        match &repo_state.comparison_mark {
            Some(mark) if mark.commit_id != commit_id => mark.clone(),
            _ => return Vec::new(),
        }
    };
    compare_range(
        state,
        repo_id,
        mark.commit_id,
        Some(commit_id),
        mark.label,
        label,
        ComparisonSource::Explicit,
    )
}

pub(super) fn range_files_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    from: CommitId,
    to: Option<CommitId>,
    request: u64,
    result: std::result::Result<Vec<CommitFileChange>, Error>,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // Only the newest issued load may land. A commit↔working-tree comparison
    // keeps the same `(from, to)` across every refresh, so that pair cannot tell
    // an overtaken reply from a current one — the request id can.
    if request != repo_state.history_state.range_files_request {
        return Vec::new();
    }
    repo_state.history_state.range_files_in_flight = false;

    // Two different guards, both needed. The id above rejects an *overtaken*
    // reply — one this repo did ask for, just not most recently. This one
    // rejects a reply that does not describe the comparison on screen at all,
    // whatever its id, so the list can never be filled from endpoints the user
    // is not looking at. See `range_files_loaded_populates_only_the_current_comparison`.
    let still_current = repo_state
        .history_state
        .range_selection
        .as_ref()
        .is_some_and(|range| range.from == from && range.to == to);
    if !still_current {
        repo_state.history_state.range_files_refresh_queued = false;
        return Vec::new();
    }

    let next = match result {
        Ok(files) => Loadable::Ready(Arc::new(files)),
        Err(e) => {
            push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
            Loadable::Error(e.to_string())
        }
    };
    repo_state.set_range_files(next);

    // The worktree moved again while this load was running; run one more so the
    // list ends up describing the final state rather than the state mid-flight.
    if !std::mem::take(&mut repo_state.history_state.range_files_refresh_queued) {
        return Vec::new();
    }
    vec![Effect::LoadRangeFiles {
        repo_id,
        from,
        to,
        request: repo_state.begin_range_files_load(),
    }]
}

fn collapse_multi_selection_to(
    sel: &mut crate::model::CommitMultiSelection,
    commit_id: CommitId,
    clicked_index: Option<usize>,
    log_rev: u64,
) {
    sel.commits.clear();
    sel.commits.push(commit_id.clone());
    sel.anchor = Some(commit_id);
    sel.anchor_index = clicked_index;
    sel.anchor_log_rev = Some(log_rev);
}

/// Resolves `target`'s index in `entries`, preferring the index hint when it
/// still points at the target.
fn commit_selection_entry_index(
    entries: &[CommitId],
    target: &CommitId,
    index_hint: Option<usize>,
) -> Option<usize> {
    index_hint
        .filter(|&ix| entries.get(ix) == Some(target))
        .or_else(|| entries.iter().position(|id| id == target))
}

pub(super) fn select_commit_and_load_details(
    repo_state: &mut RepoState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    if repo_state.history_state.selected_commit.as_ref() == Some(&commit_id) {
        return Vec::new();
    }

    repo_state.set_selected_commit(Some(commit_id.clone()));
    let already_loaded = matches!(
        &repo_state.history_state.commit_details,
        Loadable::Ready(details) if details.id == commit_id
    );
    if already_loaded {
        return Vec::new();
    }

    if matches!(
        repo_state.history_state.commit_details,
        Loadable::Error(_) | Loadable::NotLoaded
    ) {
        repo_state.set_commit_details(Loadable::NotLoaded);
    }
    vec![Effect::LoadCommitDetails { repo_id, commit_id }]
}

pub(super) fn select_worktree_uncommitted(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // Idempotent on purpose. A pending history reveal re-drives on every render
    // of the history panel, so this message arrives once per frame for as long
    // as pagination takes to reach the target. Re-running the body each time
    // would bump `commit_details_rev` -- which the details pane hashes, so the
    // repaint drives the next render -- and re-arm a full `git status` walk
    // across every linked worktree.
    if repo_state.history_state.worktree_selection.as_deref() == Some(path.as_path()) {
        return Vec::new();
    }
    // Whatever this displaces -- another worktree's open diff, say -- is retired
    // by `retire_orphaned_worktree_diffs` once the reducer settles.
    repo_state.set_worktree_selection(Some(path));
    repo_state.set_commit_details(Loadable::NotLoaded);

    // Only the selected worktree's changed files are carried in state, so the row
    // that was just selected needs a scan to fetch its own. The counts are already
    // on screen and stay there while it runs.
    request_worktree_dirty_effect(repo_state)
        .into_iter()
        .collect()
}

/// Select the uncommitted-changes row pinned atop the history list: this
/// checkout's working tree as a virtual node. The details pane shows a
/// working-tree review instead of a commit.
///
/// The selection is the all-zeros "not committed yet" id, so every
/// `is_uncommitted` check recognizes it — but it is deliberately never put in
/// `multi_selection`, whose entries must always name real commits for the
/// selection-driven commands (cherry-pick, squash, …).
pub(super) fn select_working_tree_summary(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    // Idempotent on purpose, like `select_worktree_uncommitted` above:
    // `set_selected_commit` bumps its rev unconditionally, and keyboard
    // navigation re-drives selection messages while a key is held.
    let already_selected = repo_state
        .history_state
        .selected_commit
        .as_ref()
        .is_some_and(CommitId::is_uncommitted);
    if already_selected {
        // A prior half-state (or a cancelled load) can still leave the details
        // behind; restore the invariant every other selection maintains.
        resync_working_tree_details_if_selected(repo_state);
        return Vec::new();
    }

    // Reading this checkout's uncommitted changes displaces every other kind of
    // history selection, exactly as selecting a linked worktree does.
    repo_state.set_commit_multi_selection(Default::default());
    repo_state.clear_range_comparison();
    repo_state.set_selected_commit(Some(CommitId::uncommitted()));
    repo_state.set_commit_details(Loadable::Ready(Arc::new(working_tree_details(repo_state))));
    Vec::new()
}

/// The details the uncommitted-changes row "selects": every staged and unstaged
/// entry as one file list (staged first, matching the status sections), parented
/// on HEAD so the row reads as the commit that has not been made yet. Kept in
/// step with status replies by [`resync_working_tree_details_if_selected`].
fn working_tree_details(repo_state: &RepoState) -> CommitDetails {
    let files = |entries: &[FileStatus]| -> Vec<CommitFileChange> {
        entries
            .iter()
            .map(|file| CommitFileChange {
                path: file.path.clone(),
                kind: file.kind,
                is_submodule: false,
                additions: None,
                deletions: None,
            })
            .collect()
    };
    let staged = match &repo_state.staged_status {
        Loadable::Ready(entries) => entries.as_slice(),
        _ => &[],
    };
    let unstaged = match &repo_state.worktree_status {
        Loadable::Ready(entries) => entries.as_slice(),
        _ => &[],
    };
    CommitDetails {
        id: CommitId::uncommitted(),
        message: String::new(),
        author_name: String::new(),
        author_email: String::new(),
        authored_at_unix: 0,
        committed_at: String::new(),
        committed_at_unix: 0,
        parent_ids: repo_state.head_commit_id().into_iter().collect(),
        files: files(staged).into_iter().chain(files(unstaged)).collect(),
        signed: false,
    }
}

/// Re-derive the uncommitted-changes details after a status reply, so the file
/// list behind the row's selection stays live as files change. Skips the write
/// (and the `commit_details_rev` bump the details pane hashes) when nothing
/// actually moved.
pub(super) fn resync_working_tree_details_if_selected(repo_state: &mut RepoState) {
    let selected = repo_state
        .history_state
        .selected_commit
        .as_ref()
        .is_some_and(CommitId::is_uncommitted);
    if !selected {
        return;
    }
    let next = working_tree_details(repo_state);
    if let Loadable::Ready(details) = &repo_state.history_state.commit_details
        && details.id == next.id
        && details.parent_ids == next.parent_ids
        && details.files == next.files
    {
        return;
    }
    repo_state.set_commit_details(Loadable::Ready(Arc::new(next)));
}

/// Retires an inline diff belonging to a linked worktree that is no longer the
/// selected one.
///
/// The diff pane renders an inline foreign diff in preference to the diff target,
/// so one whose worktree row is gone keeps another checkout's file -- and its
/// origin chip -- on screen with no row left to deselect it. A worktree selection
/// ends in more ways than it begins: switching worktrees, selecting any commit
/// (`set_selected_commit` clears it as a side effect), clearing the selection, and
/// a scan that no longer lists the worktree. Rather than remember all four, this
/// runs once after every message and states the invariant directly.
///
/// Submodule-origin inline diffs are untouched: they never had a worktree row.
pub(super) fn retire_orphaned_worktree_diffs(state: &mut AppState) {
    for repo_state in &mut state.repos {
        let selected = repo_state.history_state.worktree_selection.as_deref();
        let orphaned = repo_state
            .diff_state
            .inline_submodule_diff
            .as_ref()
            .is_some_and(|inline| {
                matches!(inline.origin, ForeignDiffOrigin::Worktree { .. })
                    && Some(inline.submodule_repo_path.as_path()) != selected
            });
        if !orphaned {
            continue;
        }

        // Exactly what `CloseInlineSubmoduleDiff` clears, and no more. The inline
        // diff carries its own `diff`/`diff_file`/`diff_file_image` inside
        // `InlineSubmoduleDiffState`, so dropping it drops every loadable it ever
        // owned. `diff_target` and the diff-state loadables beside it belong to
        // the commit or working-tree file selected *behind* the inline diff --
        // opening one never touched them -- and the pane falls back to that file
        // once the inline diff is gone. Clearing them here blanked the pane
        // instead.
        repo_state.diff_state.inline_submodule_diff = None;
        repo_state.bump_diff_state_rev();
    }
}

pub(super) fn clear_commit_selection(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    repo_state.set_selected_commit(None);
    repo_state.set_commit_details(Loadable::NotLoaded);
    Vec::new()
}

pub(super) fn append_ensure_sidebar_data_effects(
    repo_state: &mut RepoState,
    effects: &mut impl EffectAccumulator,
) {
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return;
    }

    let repo_id = repo_state.id;
    let request = repo_state.sidebar_data_request;

    if request.worktrees && matches!(repo_state.worktrees, Loadable::NotLoaded) {
        repo_state.set_worktrees(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::WORKTREES)
        {
            effects.push_effect(Effect::LoadWorktrees { repo_id });
        }
    }

    if request.submodules && matches!(repo_state.submodules, Loadable::NotLoaded) {
        repo_state.set_submodules(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::SUBMODULES)
        {
            effects.push_effect(Effect::LoadSubmodules { repo_id });
        }
    }

    if request.stashes && matches!(repo_state.stashes, Loadable::NotLoaded) {
        repo_state.set_stashes(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::STASHES)
        {
            effects.push_effect(Effect::LoadStashes { repo_id, limit: 50 });
        }
    }

    if request.tags && matches!(repo_state.tags, Loadable::NotLoaded) {
        repo_state.set_tags(Loadable::Loading);
        if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
            effects.push_effect(Effect::LoadTags { repo_id });
        }
    }
}

pub(super) fn ensure_sidebar_data(
    state: &mut AppState,
    repo_id: RepoId,
    request: SidebarDataRequest,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    repo_state.set_sidebar_data_request(request);
    let mut effects = Vec::new();
    append_ensure_sidebar_data_effects(repo_state, &mut effects);
    effects
}

pub(super) fn load_stashes(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_stashes(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::STASHES)
    {
        vec![Effect::LoadStashes { repo_id, limit: 50 }]
    } else {
        Vec::new()
    }
}

pub(super) fn refresh_branches(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::BRANCHES)
    {
        vec![Effect::LoadBranches { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_tags(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_tags(Loadable::Loading);
    if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
        vec![Effect::LoadTags { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_remote_tags(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_remote_tags(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTE_TAGS)
    {
        vec![Effect::LoadRemoteTags { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_reflog(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    repo_state.set_reflog(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REFLOG)
    {
        vec![Effect::LoadReflog {
            repo_id,
            limit: 200,
        }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_hover_commit_message(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    // Already showing or fetching this commit: hovering the same row again must
    // not re-issue the read.
    if repo_state
        .hover_commit_message
        .as_ref()
        .is_some_and(|(id, state)| *id == commit_id && !matches!(state, Loadable::Error(_)))
    {
        return Vec::new();
    }
    repo_state.set_hover_commit_message(commit_id.clone(), Loadable::Loading);
    vec![Effect::LoadHoverCommitMessage { repo_id, commit_id }]
}

pub(super) fn hover_commit_message_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    result: std::result::Result<String, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        // A result for a commit the pointer has already left is stale.
        && repo_state
            .hover_commit_message
            .as_ref()
            .is_some_and(|(id, _)| *id == commit_id)
    {
        let value = match result {
            Ok(message) => Loadable::Ready(Arc::from(message.as_str())),
            // Deliberately not a diagnostic: a hover that loses its race with a
            // background fetch is not something to tell the user about.
            Err(e) => Loadable::Error(e.to_string()),
        };
        repo_state.set_hover_commit_message(commit_id, value);
    }
    Vec::new()
}

pub(super) fn load_recent_commit_messages(
    state: &mut AppState,
    repo_id: RepoId,
    limit: usize,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(()))
        || matches!(repo_state.recent_commit_messages, Loadable::Loading)
    {
        return Vec::new();
    }
    repo_state.set_recent_commit_messages(Loadable::Loading);
    let request_rev = repo_state.recent_commit_messages_rev;
    vec![Effect::LoadRecentCommitMessages {
        repo_id,
        limit,
        request_rev,
    }]
}

pub(super) fn recent_commit_messages_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<Vec<RecentCommitMessage>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.recent_commit_messages_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_recent_commit_messages(value);
    }
    Vec::new()
}

/// Cross-history search. A new search replaces any in-flight one — the
/// request-rev guard drops the stale reply — and an empty query just clears
/// the previous results (the picker's local rows are all it needs then).
pub(super) fn search_commits(state: &mut AppState, repo_id: RepoId, query: String) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    let query = query.trim().to_string();
    if query.is_empty() {
        repo_state.set_commit_search(Loadable::NotLoaded);
        repo_state.commit_search_query = None;
        return Vec::new();
    }
    repo_state.set_commit_search(Loadable::Loading);
    // The trimmed query is both what the backend searches for and what the
    // picker compares its trimmed input against when deciding whether the
    // stored results answer the typed query.
    repo_state.commit_search_query = Some(query.clone());
    let request_rev = repo_state.commit_search_rev;
    vec![Effect::SearchCommits {
        repo_id,
        query,
        request_rev,
    }]
}

pub(super) fn commits_searched(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<Vec<Commit>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.commit_search_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_commit_search(value);
    }
    Vec::new()
}

/// The ✨ button's data fetch. Every click starts a fresh load — the staged
/// diff changes between clicks, so unlike `recent_commit_messages` the result
/// is never reused.
pub(super) fn load_ai_commit_context(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_ai_commit_context(Loadable::Loading);
    let request_rev = repo_state.ai_commit_context_rev;
    vec![Effect::LoadAiCommitContext {
        repo_id,
        request_rev,
    }]
}

pub(super) fn ai_commit_context_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<AiCommitContext, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.ai_commit_context_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_ai_commit_context(value);
    }
    Vec::new()
}

pub(super) fn load_file_history(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    limit: usize,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    repo_state.history_state.file_history_path = Some(path.clone());
    repo_state.history_state.file_history = Loadable::Loading;
    vec![Effect::LoadFileHistory {
        repo_id,
        path,
        limit,
    }]
}

pub(super) fn load_blame(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    source: worktree_core::domain::BlameSource,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // The view dispatches this from `MainPaneView::render` against an
    // asynchronously pushed `AppState` snapshot, and `AppStore::dispatch` is a
    // channel send, so several frames can ask for the same blame before the
    // `Loading` snapshot reaches the view. Without this guard each of those
    // frames forks another `git blame --line-porcelain` for the same file.
    // `blame_path` + `blame_source` identify the request exactly, which a
    // repo-wide `RepoLoadsInFlight` bit could not.
    let same_target = repo_state.history_state.blame_path.as_ref() == Some(&path)
        && repo_state.history_state.blame_source.as_ref() == Some(&source);
    if same_target && repo_state.history_state.blame.is_loading() {
        return Vec::new();
    }
    if same_target {
        // Reloading the same file: keep the current annotations painted until
        // the new ones land.
        repo_state.retain_blame_while_loading();
    } else {
        // Re-targeting: anything held over describes a different file.
        repo_state.clear_retained_blame();
    }
    repo_state.history_state.blame_path = Some(path.clone());
    repo_state.history_state.blame_source = Some(source.clone());
    repo_state.history_state.blame = Loadable::Loading;
    vec![Effect::LoadBlame {
        repo_id,
        path,
        source,
    }]
}

pub(super) fn load_worktrees(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_worktrees(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::WORKTREES)
    {
        vec![Effect::LoadWorktrees { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_worktree_dirty(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    // Unlike the other loaders this one does not flip to `Loading`: the counts
    // stay on screen while a rescan runs, so a window-focus refresh does not
    // blank the rows it is about to redraw identically.
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::WORKTREE_DIRTY)
    {
        vec![worktree_dirty_effect(repo_state)]
    } else {
        Vec::new()
    }
}

/// Queues a rescan of the other worktrees' uncommitted changes, if one is not
/// already running. Returns `None` when a scan is in flight, so callers can
/// fire this from several triggers without stacking up repeated full scans.
///
/// The watcher-driven trigger fires on every git-state flush, and a full scan
/// runs `status` on every other worktree, so what bounds the cost is worth
/// spelling out. First, what does *not* reach here: `.git/index` is classified
/// as `RepoExternalChange::Index`, not `git_state` (`repo_monitor.rs`,
/// `is_git_index_path`), so the common edit-stage-unstage loop -- which writes
/// nothing else -- costs no scan at all. A linked worktree's own index sits at
/// `.git/worktrees/<name>/index` and is deliberately outside that test, so
/// changes there do still arrive as git-state and do still earn a scan.
/// Then, for what does reach here: the monitor debounces raw events at 250ms
/// with a 2s ceiling
/// (`repo_monitor.rs`), and `request` admits at most one scan in flight plus one
/// queued. A storm therefore costs one scan at a time, never a growing queue,
/// and always ends with one trailing scan — dropping the queued repeat instead
/// would be cheaper but could leave the counts stale after the last event.
/// There is deliberately no time-based throttle here: this reducer has no clock,
/// and the ones that do (window focus, `view/mod.rs`) ride their own.
pub(super) fn request_worktree_dirty_effect(repo_state: &mut RepoState) -> Option<Effect> {
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return None;
    }
    repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::WORKTREE_DIRTY)
        .then(|| worktree_dirty_effect(repo_state))
}

/// The scan effect, aimed at whichever worktree row is selected.
///
/// Built in one place so every trigger -- watcher flush, window focus, selecting
/// a row -- asks for the file lists of the worktree that is actually on screen,
/// and for counts alone everywhere else.
pub(super) fn worktree_dirty_effect(repo_state: &RepoState) -> Effect {
    Effect::LoadWorktreeDirty {
        repo_id: repo_state.id,
        workdir: repo_state.spec.workdir.clone(),
        files_for: repo_state.history_state.worktree_selection.clone(),
    }
}

pub(super) fn load_ref_metadata(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_ref_metadata(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REF_METADATA)
    {
        vec![Effect::LoadRefMetadata { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_submodules(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_submodules(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::SUBMODULES)
    {
        vec![Effect::LoadSubmodules { repo_id }]
    } else {
        Vec::new()
    }
}

pub(super) fn load_file_browser(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    let source_changed = repo_state.file_browser.source != source;
    repo_state.file_browser.source = source;
    // Blank the tree only when there is nothing worth keeping: rows from another
    // source would be actively wrong, but a same-source refresh can leave them up.
    if source_changed || !matches!(repo_state.file_browser.entries, Loadable::Ready(_)) {
        repo_state.file_browser.entries = Loadable::Loading;
    }
    repo_state.file_browser.bump_rev();
    request_file_browser_load(repo_state).into_iter().collect()
}

/// Expand every directory on the way to `path` so the file explorer can show it.
///
/// Also clears the search query: the filtered view builds its rows from matches
/// and force-expands their ancestors, ignoring `expanded_dirs` entirely, so a
/// reveal into a filtered tree would scroll to a row index that does not mean
/// what the caller computed.
pub(super) fn reveal_file_browser_path(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // `ancestors()` yields the path itself first — skip it, a file is not a
    // directory to expand — and stops before the empty root component.
    for ancestor in path.ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        repo_state
            .file_browser
            .expanded_dirs
            .insert(Arc::new(ancestor.to_path_buf()));
    }
    if !repo_state.file_browser.search_query.is_empty() {
        repo_state.file_browser.search_query.clear();
    }
    repo_state.file_browser.bump_rev();
    Vec::new()
}

/// Whether a query actually filters the file tree, and so force-expands every
/// directory and ignores `expanded_dirs`.
///
/// The search input is multiline and stores what was typed verbatim, so a lone
/// space is a non-empty query that filters nothing. Mirrors the view's
/// `file_browser_search_is_active`.
fn file_browser_query_filters(query: &str) -> bool {
    query.lines().any(|line| !line.trim().is_empty())
}

fn file_browser_is_filtered(repo_state: &RepoState) -> bool {
    file_browser_query_filters(&repo_state.file_browser.search_query)
}

pub(super) fn reduce_loaded_results(msg: Msg, state: &mut AppState) -> ReduceOutcome {
    let effects = match msg {
        Msg::SelectCommit { repo_id, commit_id } => {
            loaded_results::select_commit(state, repo_id, commit_id)
        }
        Msg::SelectCommitMulti {
            repo_id,
            commit_id,
            mode,
            clicked_index,
            visible_order,
        } => loaded_results::select_commit_multi(
            state,
            repo_id,
            commit_id,
            mode,
            clicked_index,
            visible_order,
        ),
        Msg::ClearCommitSelection { repo_id } => {
            loaded_results::clear_commit_selection(state, repo_id)
        }
        Msg::CompareCommitRange {
            repo_id,
            from,
            to,
            from_label,
            to_label,
        } => loaded_results::compare_range(
            state,
            repo_id,
            from,
            Some(to),
            from_label,
            to_label,
            loaded_results::ComparisonSource::Explicit,
        ),
        Msg::CompareWithWorkingTree {
            repo_id,
            from,
            from_label,
        } => loaded_results::compare_range(
            state,
            repo_id,
            from,
            None,
            from_label,
            rust_i18n::t!("store.reducer.label_working_tree").to_string(),
            loaded_results::ComparisonSource::Explicit,
        ),
        Msg::ClearComparison { repo_id } => loaded_results::clear_comparison(state, repo_id),
        Msg::MarkForComparison {
            repo_id,
            commit_id,
            label,
        } => loaded_results::mark_for_comparison(state, repo_id, commit_id, label),
        Msg::CompareWithMarked {
            repo_id,
            commit_id,
            label,
        } => loaded_results::compare_with_marked(state, repo_id, commit_id, label),
        Msg::ClearComparisonMark { repo_id } => {
            loaded_results::clear_comparison_mark(state, repo_id)
        }
        Msg::EnsureSidebarData { repo_id, request } => {
            loaded_results::ensure_sidebar_data(state, repo_id, request)
        }
        Msg::LoadStashes { repo_id } => loaded_results::load_stashes(state, repo_id),
        Msg::LoadConflictFile {
            repo_id,
            path,
            mode,
        } => loaded_results::load_conflict_file(state, repo_id, path, mode),
        Msg::LoadReflog { repo_id } => loaded_results::load_reflog(state, repo_id),
        Msg::LoadHoverCommitMessage { repo_id, commit_id } => {
            loaded_results::load_hover_commit_message(state, repo_id, commit_id)
        }
        Msg::LoadRecentCommitMessages { repo_id, limit } => {
            loaded_results::load_recent_commit_messages(state, repo_id, limit)
        }
        Msg::SearchCommits { repo_id, query } => {
            loaded_results::search_commits(state, repo_id, query)
        }
        Msg::LoadAiCommitContext { repo_id } => {
            loaded_results::load_ai_commit_context(state, repo_id)
        }
        Msg::LoadFileHistory {
            repo_id,
            path,
            limit,
        } => loaded_results::load_file_history(state, repo_id, path, limit),
        Msg::LoadBlame {
            repo_id,
            path,
            source,
        } => loaded_results::load_blame(state, repo_id, path, source),
        Msg::LoadWorktrees { repo_id } => loaded_results::load_worktrees(state, repo_id),
        Msg::LoadWorktreeDirty { repo_id } => loaded_results::load_worktree_dirty(state, repo_id),
        Msg::SelectWorktreeUncommitted { repo_id, path } => {
            loaded_results::select_worktree_uncommitted(state, repo_id, path)
        }
        Msg::SelectWorkingTreeSummary { repo_id } => {
            loaded_results::select_working_tree_summary(state, repo_id)
        }
        Msg::LoadRefMetadata { repo_id } => loaded_results::load_ref_metadata(state, repo_id),
        Msg::LoadSubmodules { repo_id } => loaded_results::load_submodules(state, repo_id),
        // The GitHub API call itself is spawned by the UI (the store has no
        // HTTP); this arm only flips the loadable so the section renders its
        // loading state and re-loads are not double-spawned.
        Msg::LoadPullRequests { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_pull_requests(Loadable::Loading);
                worktree_core::applog_info!(
                    "pull-request load state: loading (repo_id={})",
                    repo_id.0
                );
            }
            Vec::new()
        }
        Msg::LoadTags { repo_id } => loaded_results::load_tags(state, repo_id),
        Msg::LoadRemoteTags { repo_id } => loaded_results::load_remote_tags(state, repo_id),
        Msg::RefreshBranches { repo_id } => loaded_results::refresh_branches(state, repo_id),
        Msg::LoadFileBrowser { repo_id, source } => {
            loaded_results::load_file_browser(state, repo_id, source)
        }
        Msg::ToggleFileBrowserDir { repo_id, path } => {
            loaded_results::toggle_file_browser_dir(state, repo_id, path)
        }
        Msg::SetFileBrowserDirExpandedRecursive {
            repo_id,
            path,
            expanded,
        } => {
            loaded_results::set_file_browser_dir_expanded_recursive(state, repo_id, path, expanded)
        }
        Msg::SetFileBrowserSearch { repo_id, query } => {
            loaded_results::set_file_browser_search(state, repo_id, query)
        }
        Msg::RevealFileBrowserPath { repo_id, path } => {
            loaded_results::reveal_file_browser_path(state, repo_id, path)
        }
        Msg::SetFileBrowserSource { repo_id, source } => {
            loaded_results::set_file_browser_source(state, repo_id, source)
        }
        Msg::BrowseRepositoryAtCommit { repo_id, commit_id } => {
            loaded_results::browse_repository_at_commit(state, repo_id, commit_id)
        }
        Msg::RevealCommit { repo_id, reference } => {
            loaded_results::reveal_commit(state, repo_id, reference)
        }
        Msg::FinishCommitReveal { repo_id } => loaded_results::finish_commit_reveal(state, repo_id),
        Msg::ResetBrowseToLive { repo_id } => loaded_results::reset_browse_to_live(state, repo_id),
        Msg::SetSidebarMode { mode } => loaded_results::set_sidebar_mode(state, mode),
        Msg::SetCoverage { repo_id, report } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_coverage(Some(report));
            }
            Vec::new()
        }
        Msg::ClearCoverage { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_coverage(None);
            }
            Vec::new()
        }
        Msg::PrepareSquash { repo_id } => loaded_results::prepare_squash(state, repo_id),
        Msg::LoadRepoStatistics { repo_id } => loaded_results::load_repo_statistics(state, repo_id),
        Msg::Internal(crate::msg::InternalMsg::BranchesLoaded { repo_id, result }) => {
            loaded_results::branches_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemotesLoaded { repo_id, result }) => {
            loaded_results::remotes_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemoteBranchesLoaded { repo_id, result }) => {
            loaded_results::remote_branches_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded { repo_id, result }) => {
            loaded_results::worktree_status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded { repo_id, result }) => {
            loaded_results::staged_status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StatusLoaded { repo_id, result }) => {
            loaded_results::status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result,
        }) => loaded_results::status_for_paths_loaded(state, repo_id, paths, result),
        Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded { repo_id, result }) => {
            loaded_results::head_branch_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::UpstreamDivergenceLoaded { repo_id, result }) => {
            loaded_results::upstream_divergence_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::TagsLoaded { repo_id, result }) => {
            loaded_results::tags_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemoteTagsLoaded { repo_id, result }) => {
            loaded_results::remote_tags_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StashesLoaded { repo_id, result }) => {
            loaded_results::stashes_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::AssumeUnchangedListLoaded { repo_id, result }) => {
            loaded_results::assume_unchanged_list_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RepoStatisticsLoaded { repo_id, result }) => {
            loaded_results::repo_statistics_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::ReflogLoaded { repo_id, result }) => {
            loaded_results::reflog_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::HoverCommitMessageLoaded {
            repo_id,
            commit_id,
            result,
        }) => loaded_results::hover_commit_message_loaded(state, repo_id, commit_id, result),
        Msg::Internal(crate::msg::InternalMsg::FileHistoryLoaded {
            repo_id,
            path,
            result,
        }) => loaded_results::file_history_loaded(state, repo_id, path, result),
        Msg::Internal(crate::msg::InternalMsg::AuthorEmailsLoaded { repo_id, result }) => {
            loaded_results::author_emails_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::BlameLoaded {
            repo_id,
            path,
            source,
            result,
        }) => loaded_results::blame_loaded(state, repo_id, path, source, result),
        Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
            repo_id,
            path,
            result,
            conflict_session,
        }) => loaded_results::conflict_file_loaded(state, repo_id, path, *result, conflict_session),
        Msg::Internal(crate::msg::InternalMsg::WorktreesLoaded { repo_id, result }) => {
            loaded_results::worktrees_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::WorktreeDirtyLoaded { repo_id, result }) => {
            loaded_results::worktree_dirty_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RefMetadataLoaded { repo_id, result }) => {
            loaded_results::ref_metadata_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::SubmodulesLoaded { repo_id, result }) => {
            loaded_results::submodules_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::FileBrowserLoaded {
            repo_id,
            source,
            result,
        }) => loaded_results::file_browser_loaded(state, repo_id, source, result),
        Msg::Internal(crate::msg::InternalMsg::CommitDetailsLoaded {
            repo_id,
            commit_id,
            result,
        }) => loaded_results::commit_details_loaded(state, repo_id, commit_id, result),
        Msg::Internal(crate::msg::InternalMsg::CommitRevealResolved {
            repo_id,
            reference,
            result,
        }) => loaded_results::commit_reveal_resolved(state, repo_id, reference, result),
        Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
            repo_id,
            from,
            to,
            request,
            result,
        }) => loaded_results::range_files_loaded(state, repo_id, from, to, request, result),
        Msg::Internal(crate::msg::InternalMsg::SquashMessagePreviewLoaded {
            repo_id,
            oldest,
            head,
            result,
        }) => loaded_results::squash_message_preview_loaded(state, repo_id, oldest, head, result),
        Msg::Internal(crate::msg::InternalMsg::SquashRebaseSetupLoaded {
            repo_id,
            base,
            actual_head,
            selected_ids,
            reword_id,
            message,
            count,
            result,
        }) => loaded_results::squash_rebase_setup_loaded(
            state,
            repo_id,
            base,
            actual_head,
            selected_ids,
            reword_id,
            message,
            count,
            result,
        ),
        Msg::Internal(crate::msg::InternalMsg::RecentCommitMessagesLoaded {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::recent_commit_messages_loaded(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::CommitsSearched {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::commits_searched(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::AiCommitContextLoaded {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::ai_commit_context_loaded(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::PullRequestsLoaded { repo_id, result }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                match &result {
                    Ok(pull_requests) => worktree_core::applog_info!(
                        "pull-request load state: {} pull requests stored (repo_id={})",
                        pull_requests.len(),
                        repo_id.0
                    ),
                    Err(error) => worktree_core::applog_warn!(
                        "pull-request load state: error stored: {error} (repo_id={})",
                        repo_id.0
                    ),
                }
                repo_state.set_pull_requests(match result {
                    Ok(pull_requests) => Loadable::Ready(pull_requests),
                    Err(error) => Loadable::Error(error.to_string()),
                });
            }
            Vec::new()
        }
        Msg::Internal(crate::msg::InternalMsg::PullRequestChecksLoaded {
            repo_id,
            number,
            checks,
        }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_pull_request_checks(number, checks);
            }
            Vec::new()
        }
        other => return ReduceOutcome::NotHandled(other),
    };
    ReduceOutcome::Handled(effects)
}

#[cfg(test)]
mod file_browser_filter_tests;

pub(super) fn toggle_file_browser_dir(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        // A filtered tree renders every directory expanded and never reads
        // `expanded_dirs`, so a toggle here would move nothing on screen and
        // then silently reshape the tree the moment the search was cleared.
        if file_browser_is_filtered(repo_state) {
            return Vec::new();
        }
        let path = Arc::new(path);
        if repo_state.file_browser.expanded_dirs.contains(&path) {
            repo_state.file_browser.expanded_dirs.remove(&path);
        } else {
            repo_state.file_browser.expanded_dirs.insert(path);
        }
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

/// Expand or collapse `path` and every directory under it.
///
/// The backend enumerates the whole tree in one pass, so every descendant is
/// already in `entries` and this needs no loading. `starts_with` on the flat
/// list also covers `path` itself, which is what makes "Expand all under here"
/// open the folder it was invoked on.
pub(super) fn set_file_browser_dir_expanded_recursive(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    expanded: bool,
) -> Vec<Effect> {
    // `Path::starts_with("")` is true of every path, so an empty path would
    // reach the whole tree and a collapse would wipe `expanded_dirs` outright.
    // The branch-group sibling guards this the same way.
    if path.as_os_str().is_empty() {
        return Vec::new();
    }
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // Frozen while a search filters the tree, for the same reason a single
    // toggle is.
    if file_browser_is_filtered(repo_state) {
        return Vec::new();
    }
    let Loadable::Ready(entries) = &repo_state.file_browser.entries else {
        return Vec::new();
    };

    // Cloning the Arc releases the borrow on `file_browser` so `expanded_dirs`
    // can be written while the entry list is walked.
    let entries = Arc::clone(entries);
    let mut changed = false;
    for entry in entries.iter() {
        if entry.kind != worktree_core::domain::FileEntryKind::Directory
            || !entry.path.starts_with(&path)
        {
            continue;
        }
        // Each entry already owns its path as an `Arc`, so expanding reuses it
        // rather than allocating a second copy per directory.
        changed |= if expanded {
            repo_state
                .file_browser
                .expanded_dirs
                .insert(Arc::clone(&entry.path))
        } else {
            repo_state.file_browser.expanded_dirs.remove(&entry.path)
        };
    }

    if changed {
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

pub(super) fn set_file_browser_search(
    state: &mut AppState,
    repo_id: RepoId,
    query: String,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.file_browser.search_query != query
    {
        repo_state.file_browser.search_query = query;
        repo_state.file_browser.bump_rev();
    }
    Vec::new()
}

pub(super) fn request_file_browser_load(repo_state: &mut RepoState) -> Option<Effect> {
    repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::FILE_BROWSER)
        .then(|| Effect::LoadFileBrowser {
            repo_id: repo_state.id,
            source: repo_state.file_browser.source.clone(),
        })
}

pub(super) fn set_file_browser_source(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.file_browser.source != source
    {
        repo_state.file_browser.source = source;
        repo_state.file_browser.entries = Loadable::NotLoaded;
        repo_state.file_browser.expanded_dirs.clear();
        repo_state.file_browser.search_query.clear();
        repo_state.file_browser.stale = false;
        repo_state.file_browser.bump_rev();
        return request_file_browser_load(repo_state).into_iter().collect();
    }
    Vec::new()
}

pub(super) fn set_sidebar_mode(state: &mut AppState, mode: SidebarMode) -> Vec<Effect> {
    if state.sidebar_mode != mode {
        state.sidebar_mode = mode;

        if mode == SidebarMode::Files
            && let Some(repo_id) = state.active_repo
            && let Some(repo) = state.repos.iter_mut().find(|r| r.id == repo_id)
            && repo.file_browser.needs_load()
        {
            return request_file_browser_load(repo).into_iter().collect();
        }
    }
    Vec::new()
}

pub(super) fn browse_repository_at_commit(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    const BROWSE_HISTORY_CAP: usize = 32;
    // Capture the open file (if any) before re-targeting it to the new point.
    let reopen_path = browse_open_content_path(state, repo_id);
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && !repo_state.browse_history.contains(&commit_id)
    {
        repo_state.browse_history.push(commit_id.clone());
        if repo_state.browse_history.len() > BROWSE_HISTORY_CAP {
            repo_state.browse_history.remove(0);
        }
    }
    state.sidebar_mode = SidebarMode::Files;
    let mut effects =
        set_file_browser_source(state, repo_id, FileSource::Commit(commit_id.clone()));
    if let Some(path) = reopen_path
        && effects
            .iter()
            .any(|e| matches!(e, Effect::LoadFileBrowser { .. }))
    {
        effects.extend(super::diff_selection::open_file_content(
            state,
            repo_id,
            FileSource::Commit(commit_id),
            path,
        ));
    }
    effects
}

pub(super) fn reset_browse_to_live(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let reopen_path = browse_open_content_path(state, repo_id);
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.browse_history.clear();
    }
    let mut effects = set_file_browser_source(state, repo_id, FileSource::WorkingDirectory);
    if let Some(path) = reopen_path
        && effects
            .iter()
            .any(|e| matches!(e, Effect::LoadFileBrowser { .. }))
    {
        effects.extend(super::diff_selection::open_file_content(
            state,
            repo_id,
            FileSource::WorkingDirectory,
            path,
        ));
    }
    effects
}

/// Path of the file currently shown as full content (if any), so a browse-point
/// change can re-open the same file at the new point.
fn browse_open_content_path(state: &AppState, repo_id: RepoId) -> Option<std::path::PathBuf> {
    let repo = state.repos.iter().find(|r| r.id == repo_id)?;
    if !repo.diff_state.content_preview {
        return None;
    }
    match &repo.diff_state.diff_target {
        Some(worktree_core::domain::DiffTarget::Commit { path: Some(p), .. }) => Some(p.clone()),
        Some(worktree_core::domain::DiffTarget::WorkingTree { path, .. }) => Some(path.clone()),
        _ => None,
    }
}

pub(super) fn file_browser_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    source: FileSource,
    result: std::result::Result<Vec<FileEntry>, worktree_core::error::Error>,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    // Release the lane before the stale-source guard: a reply for a source the
    // user has already navigated away from still ends the walk that was running,
    // and the request queued behind it is the one that matters now.
    let has_pending = repo_state
        .loads_in_flight
        .finish(RepoLoadsInFlight::FILE_BROWSER);

    if repo_state.file_browser.source == source {
        repo_state.file_browser.entries = match result {
            Ok(v) => Loadable::Ready(Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.file_browser.stale = false;
        repo_state.file_browser.bump_rev();
    }

    if has_pending {
        return vec![Effect::LoadFileBrowser {
            repo_id,
            source: repo_state.file_browser.source.clone(),
        }];
    }
    Vec::new()
}

pub(super) fn branches_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Branch>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let branches = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_branches(branches);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::BRANCHES)
        {
            effects.push(Effect::LoadBranches { repo_id });
        }
    }
    effects
}

pub(super) fn remotes_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Remote>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let remotes = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_remotes(remotes);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTES)
        {
            effects.push(Effect::LoadRemotes { repo_id });
        }
    }
    effects
}

pub(super) fn remote_branches_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<RemoteBranch>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let branches = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_remote_branches(branches);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTE_BRANCHES)
        {
            effects.push(Effect::LoadRemoteBranches { repo_id });
        }
    }
    effects
}

/// Condensed status-lane payload for the repo-load trace: enough of the path
/// list to recognize a wrongful staged-list clear in one reproduction without
/// flooding the log.
fn trace_status_paths(entries: &[FileStatus]) -> String {
    const MAX_PATHS: usize = 4;
    let mut names: Vec<String> = entries
        .iter()
        .take(MAX_PATHS)
        .map(|entry| entry.path.display().to_string())
        .collect();
    if entries.len() > MAX_PATHS {
        names.push(format!("+{}", entries.len() - MAX_PATHS));
    }
    format!("[{}]", names.join(", "))
}

pub(super) fn status_for_paths_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    paths: std::sync::Arc<[std::path::PathBuf]>,
    result: std::result::Result<worktree_core::services::StatusForPaths, Error>,
) -> Vec<Effect> {
    use worktree_core::services::StatusForPaths;
    let mut effects = Vec::new();
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return effects;
    };
    let fallback_error = result
        .as_ref()
        .err()
        .map(|error| error.to_string())
        .unwrap_or_else(|| "<unmergeable payload>".to_string());
    match result {
        // The targeted lane only ever merges onto a settled full snapshot;
        // anything else (or an unmergeable shape) falls back to a full scan,
        // whose landing finishes this lane's in-flight bit — no retry loop.
        Ok(StatusForPaths::Lists { unstaged, staged })
            if matches!(repo_state.status, Loadable::Ready(_)) =>
        {
            repo_load_trace::trace!(
                "status_for_paths_merge repo_id={:?} covered={:?} staged_scan={} unstaged_scan={}",
                repo_id,
                paths.iter().collect::<Vec<_>>(),
                trace_status_paths(&staged),
                trace_status_paths(&unstaged)
            );
            repo_state.patch_status_for_paths(&paths, unstaged, staged);
            repo_load_trace::trace!(
                "status_for_paths_merged repo_id={:?} staged_now={}",
                repo_id,
                repo_state
                    .staged_status_entries()
                    .map_or(0, |entries| entries.len())
            );
            resync_working_tree_details_if_selected(repo_state);
            // A change folded while the targeted scan was in flight re-runs
            // the lane coarsely: the folded burst's paths are unknown here.
            finish_status_lane_replay(
                repo_state,
                RepoLoadsInFlight::WORKTREE_STATUS,
                Effect::LoadWorktreeStatus { repo_id },
                &mut effects,
            );
            // The merge replaces a covered path's staged half too, so it also
            // finishes the staged lane. Dispatches that hold only the
            // worktree flag (the external watcher's) finish a lane they never
            // took — a no-op — while the action-completion refresh, which
            // holds both, would strand its staged flag here otherwise.
            finish_status_lane_replay(
                repo_state,
                RepoLoadsInFlight::STAGED_STATUS,
                Effect::LoadStagedStatus { repo_id },
                &mut effects,
            );
        }
        Ok(_) | Err(_) => {
            repo_load_trace::trace!(
                "status_for_paths_fallback_full_scan repo_id={:?} covered={:?} reason={}",
                repo_id,
                paths.iter().collect::<Vec<_>>(),
                fallback_error
            );
            effects.push(Effect::LoadStatus { repo_id });
        }
    }
    effects
}

pub(super) fn status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<RepoStatus, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(
                    &repo_state.status,
                    Loadable::Ready(prev) if prev.as_ref() == &next
                );
                repo_load_trace::trace!(
                    "status_loaded repo_id={:?} staged={} unstaged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next.staged),
                    trace_status_paths(&next.unstaged),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_status(Loadable::Ready(Arc::new(next)));
                }
                clear_resolved_conflict_context(repo_state);
            }
            Err(e) => {
                repo_load_trace::trace!("status_loaded_error repo_id={:?} error={}", repo_id, e);
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::WORKTREE_STATUS,
            Effect::LoadWorktreeStatus { repo_id },
            &mut effects,
        );
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::STAGED_STATUS,
            Effect::LoadStagedStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

pub(super) fn worktree_status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<worktree_core::domain::FileStatus>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(&repo_state.worktree_status, Loadable::Ready(prev) if prev.as_slice() == next.as_slice());
                repo_load_trace::trace!(
                    "worktree_status_loaded repo_id={:?} unstaged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_worktree_status(Loadable::Ready(next));
                }
                clear_resolved_conflict_context(repo_state);
            }
            Err(e) => {
                repo_load_trace::trace!(
                    "worktree_status_loaded_error repo_id={:?} error={}",
                    repo_id,
                    e
                );
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_worktree_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::WORKTREE_STATUS,
            Effect::LoadWorktreeStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

pub(super) fn staged_status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<worktree_core::domain::FileStatus>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(&repo_state.staged_status, Loadable::Ready(prev) if prev.as_slice() == next.as_slice());
                repo_load_trace::trace!(
                    "staged_status_loaded repo_id={:?} staged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_staged_status(Loadable::Ready(next));
                }
            }
            Err(e) => {
                repo_load_trace::trace!(
                    "staged_status_loaded_error repo_id={:?} error={}",
                    repo_id,
                    e
                );
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_staged_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::STAGED_STATUS,
            Effect::LoadStagedStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

fn finish_status_lane_replay(
    repo_state: &mut crate::model::RepoState,
    flag: u32,
    replay_effect: Effect,
    effects: &mut Vec<Effect>,
) {
    // A pending request means a refresh was coalesced while this load was in flight — a genuine
    // external change or a just-completed action. Always replay it, even when the loaded payload
    // matches what is currently displayed: the in-flight load may have read the working tree or
    // index just *before* the change landed, so the coalesced refresh is the only chance to
    // observe it. Suppressing it on an unchanged payload (as a previous revision did) drops real
    // external changes and leaves stale entries in the uncommitted view.
    //
    // This cannot self-sustain a refresh loop: status reads are read-only (the gix backend's
    // `maybe_persist_*` helpers never rewrite `.git/index`, and worktree reads emit only ignored
    // `Access` events), so a completed status load never manufactures the filesystem event that
    // would set `pending` again.
    if repo_state.loads_in_flight.finish(flag) {
        effects.push(replay_effect);
    }
}

/// Clear conflict-file/session state when the tracked conflict path is no longer
/// present as an unresolved conflict in status.
fn clear_resolved_conflict_context(repo_state: &mut crate::model::RepoState) {
    let Some(conflict_path) = repo_state.conflict_state.conflict_file_path.as_ref() else {
        return;
    };
    let still_conflicted = repo_state.worktree_status_entries().is_none_or(|status| {
        status
            .iter()
            .any(|entry| entry.path == *conflict_path && entry.kind == FileStatusKind::Conflicted)
    });
    if still_conflicted {
        return;
    }

    repo_state.set_conflict_file_path(None);
    repo_state.set_conflict_file_load_mode(ConflictFileLoadMode::CurrentOnly);
    repo_state.set_conflict_file(Loadable::NotLoaded);
    repo_state.conflict_state.session_pending_restore = None;
    repo_state.set_conflict_session(None);
    repo_state.set_conflict_hide_resolved(false);
}

pub(super) fn head_branch_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<String, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let head_branch = match result {
            Ok(v) => {
                if v == "HEAD" {
                    if repo_state.detached_head_commit.is_none()
                        && repo_state
                            .history_state
                            .history_scope
                            .guarantees_head_visibility()
                        && let Loadable::Ready(page) = &repo_state.log
                    {
                        repo_state
                            .set_detached_head_commit(page.commits.first().map(|c| c.id.clone()));
                    }
                } else {
                    repo_state.set_detached_head_commit(None);
                }
                Loadable::Ready(v)
            }
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_head_branch(head_branch);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::HEAD_BRANCH)
        {
            effects.push(Effect::LoadHeadBranch { repo_id });
        }
    }
    effects
}

pub(super) fn upstream_divergence_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Option<UpstreamDivergence>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_upstream_divergence(value);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::UPSTREAM_DIVERGENCE)
        {
            effects.push(Effect::LoadUpstreamDivergence { repo_id });
        }
    }
    effects
}

pub(super) fn tags_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Tag>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let tags = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) {
                    Loadable::Ready(Vec::new())
                } else if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_tags(tags);
        if repo_state.loads_in_flight.finish(RepoLoadsInFlight::TAGS) {
            effects.push(Effect::LoadTags { repo_id });
        }
    }
    effects
}

pub(super) fn remote_tags_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<RemoteTag>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let remote_tags = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) {
                    Loadable::Ready(Vec::new())
                } else if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_remote_tags(remote_tags);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTE_TAGS)
        {
            effects.push(Effect::LoadRemoteTags { repo_id });
        }
    }
    effects
}

pub(super) fn assume_unchanged_list_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<std::path::PathBuf>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let list = match result {
            Ok(v) => Loadable::Ready(std::sync::Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.assume_unchanged = list;
        repo_state.assume_unchanged_rev = repo_state.assume_unchanged_rev.wrapping_add(1);
        repo_state.bump_ops_rev();
    }
    Vec::new()
}

/// On-demand like the stash list: nothing asks until the statistics dialog
/// opens, and while it loads the dialog shows the same pending state every
/// other load does.
pub(super) fn load_repo_statistics(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.statistics = Loadable::Loading;
    repo_state.statistics_rev = repo_state.statistics_rev.wrapping_add(1);
    repo_state.bump_ops_rev();
    vec![Effect::LoadRepoStatistics { repo_id }]
}

pub(super) fn repo_statistics_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<ContributorCommit>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let statistics = match result {
            Ok(v) => Loadable::Ready(std::sync::Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.statistics = statistics;
        repo_state.statistics_rev = repo_state.statistics_rev.wrapping_add(1);
        repo_state.bump_ops_rev();
    }
    Vec::new()
}

pub(super) fn stashes_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<StashEntry>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let stashes = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_stashes(stashes);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::STASHES)
        {
            effects.push(Effect::LoadStashes { repo_id, limit: 50 });
        }
    }
    effects
}

pub(super) fn reflog_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<ReflogEntry>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let next = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_reflog(next);
        if repo_state.loads_in_flight.finish(RepoLoadsInFlight::REFLOG) {
            effects.push(Effect::LoadReflog {
                repo_id,
                limit: 200,
            });
        }
    }
    effects
}

/// Start revealing a commit referenced from elsewhere.
///
/// The reference is remembered and resolved off-thread. Selecting only happens
/// once it resolves, so a reference that turns out to be a build id or a Gerrit
/// change id never sends the log walking.
pub(super) fn reveal_commit(
    state: &mut AppState,
    repo_id: RepoId,
    reference: CommitId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    repo_state.set_reveal_target(Some(reference.clone()));
    vec![Effect::ResolveCommitForReveal { repo_id, reference }]
}

pub(super) fn finish_commit_reveal(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.set_reveal_target(None);
    }
    Vec::new()
}

pub(super) fn commit_reveal_resolved(
    state: &mut AppState,
    repo_id: RepoId,
    reference: CommitId,
    result: std::result::Result<CommitDetails, Error>,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // A reply for a reveal the user has already left behind.
    if repo_state.history_state.reveal_target.as_ref() != Some(&reference) {
        return Vec::new();
    }

    let details = match result {
        Ok(details) => details,
        Err(e) => {
            repo_state.set_reveal_target(None);
            push_notification(
                state,
                crate::model::AppNotificationKind::Warning,
                rust_i18n::t!(
                    "store.reducer.commit_reveal_failed",
                    reference = reference,
                    error = e
                )
                .to_string(),
            );
            return Vec::new();
        }
    };

    // Publish the details before selecting: the selection path then sees them
    // already loaded and does not ask git for the same commit twice.
    let commit_id = details.id.clone();
    repo_state.set_reveal_target(Some(commit_id.clone()));
    repo_state.set_commit_details(Loadable::Ready(Arc::new(details)));
    select_commit(state, repo_id, commit_id)
}

pub(super) fn commit_details_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    result: std::result::Result<CommitDetails, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.history_state.selected_commit.as_ref() == Some(&commit_id)
    {
        let selected_target = repo_state.diff_state.diff_target.clone();
        let previous_plan = selected_target
            .as_ref()
            .map(|target| selected_diff_load_plan(repo_state, target));
        let value = match result {
            Ok(v) => Loadable::Ready(Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_commit_details(value);

        if let Some(target @ worktree_core::domain::DiffTarget::Commit { .. }) = selected_target {
            let next_plan = selected_diff_load_plan(repo_state, &target);
            if previous_plan != Some(next_plan) {
                apply_selected_diff_load_plan_state(repo_state, next_plan);
                repo_state.bump_diff_state_rev();
                let mut effects = Vec::with_capacity(diff_reload_effect_count(repo_state, &target));
                append_diff_reload_effects(&mut effects, repo_state, repo_id, target);
                return effects;
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
