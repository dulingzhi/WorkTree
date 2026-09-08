use super::util::{
    EffectAccumulator, append_diff_reload_effects, apply_selected_diff_load_plan_state,
    diff_reload_effect_count, push_diagnostic, push_notification, selected_diff_load_plan,
};
use super::{
    ReduceOutcome, begin_local_action, conflict_interactions, diff_selection, loaded_results, util,
};
use crate::model::{
    AiCommitContext, AppState, CommitMultiSelection, DiagnosticKind, Loadable, RangeSelection,
    RepoId, RepoLoadsInFlight, RepoState, SidebarDataRequest, SidebarMode,
};
use crate::msg::{CommitSelectMode, Effect, Msg};
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::domain::{
    Commit, CommitDetails, CommitFileChange, CommitId, EMPTY_TREE_ID, FileEntry, FileSource,
    LogPage, RecentCommitMessage, ReflogEntry, StashEntry,
};
use worktree_core::error::Error;

mod conflict;
mod squash;
mod status_refs;
mod worktrees;

pub(super) use conflict::{conflict_file_loaded, load_conflict_file};
pub(super) use squash::{
    prepare_squash, squash_message_preview_loaded, squash_plan_for_repo, squash_rebase_setup_loaded,
};

pub(super) use status_refs::{
    assume_unchanged_list_loaded, branches_loaded, head_branch_loaded, load_ref_metadata,
    load_remote_tags, load_repo_statistics, load_submodules, load_tags, ref_metadata_loaded,
    refresh_branches, remote_branches_loaded, remote_tags_loaded, remotes_loaded,
    repo_statistics_loaded, staged_status_loaded, status_for_paths_loaded, status_loaded,
    submodules_loaded, tags_loaded, upstream_divergence_loaded, worktree_status_loaded,
};
pub(super) use worktrees::{
    load_worktree_dirty, load_worktrees, request_worktree_dirty_effect,
    retire_orphaned_worktree_diffs, select_working_tree_summary, select_worktree_uncommitted,
    worktree_dirty_loaded, worktrees_loaded,
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
