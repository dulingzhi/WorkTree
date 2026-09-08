use super::util::{
    append_diff_reload_effects, apply_selected_diff_load_plan_state, diff_reload_effect_count,
    push_diagnostic, push_notification, selected_diff_load_plan,
};
use super::worktrees::select_working_tree_summary;
use crate::model::{
    AppState, CommitMultiSelection, DiagnosticKind, Loadable, RangeSelection, RepoId, RepoState,
};
use crate::msg::{CommitSelectMode, Effect};
use std::sync::Arc;
use worktree_core::domain::{CommitDetails, CommitFileChange, CommitId, EMPTY_TREE_ID};
use worktree_core::error::Error;

pub(in crate::store::reducer) fn select_commit(
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

pub(in crate::store::reducer) fn select_commit_multi(
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
pub(in crate::store::reducer) enum ComparisonSource {
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
pub(in crate::store::reducer) fn compare_range(
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
pub(in crate::store::reducer) fn clear_comparison(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
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

pub(in crate::store::reducer) fn mark_for_comparison(
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

pub(in crate::store::reducer) fn clear_comparison_mark(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.comparison_mark = None;
    }
    Vec::new()
}

/// Compare the marked point (base) against `commit_id` (tip). No-op when nothing
/// is marked or the mark equals the target.
pub(in crate::store::reducer) fn compare_with_marked(
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

pub(in crate::store::reducer) fn range_files_loaded(
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

pub(in crate::store::reducer) fn select_commit_and_load_details(
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

pub(in crate::store::reducer) fn clear_commit_selection(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    repo_state.set_selected_commit(None);
    repo_state.set_commit_details(Loadable::NotLoaded);
    Vec::new()
}

/// Start revealing a commit referenced from elsewhere.
///
/// The reference is remembered and resolved off-thread. Selecting only happens
/// once it resolves, so a reference that turns out to be a build id or a Gerrit
/// change id never sends the log walking.
pub(in crate::store::reducer) fn reveal_commit(
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

pub(in crate::store::reducer) fn finish_commit_reveal(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.set_reveal_target(None);
    }
    Vec::new()
}

pub(in crate::store::reducer) fn commit_reveal_resolved(
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

pub(in crate::store::reducer) fn commit_details_loaded(
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
