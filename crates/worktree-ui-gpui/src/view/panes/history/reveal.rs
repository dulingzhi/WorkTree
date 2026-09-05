//! The history pane's selection and reveal vocabulary: the selected-index
//! caches, selection highlights, worktree reveal targets and the pending
//! reveal decision.

use super::*;

use smallvec::SmallVec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HistorySelectedListIndexCache {
    pub(super) repo_id: RepoId,
    pub(super) log_rev: u64,
    pub(super) stashes_rev: u64,
    pub(super) history_scope: LogScope,
    pub(super) show_working_tree_summary_row: bool,
    /// Identity of the row interleaving the cached `list_ix` was computed
    /// against; a worktree row appearing or moving shifts every index below it.
    pub(super) plan_fingerprint: u64,
    pub(super) selected_commit: Option<CommitId>,
    pub(super) list_ix: usize,
}

/// Memo for [`HistoryView::history_selected_lane_color_ix`].
/// What the lane highlight is anchored to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HistoryLaneAnchor {
    /// A commit, highlighting the lane it is drawn on.
    Commit(CommitId),
    /// A linked worktree. Its HEAD locates the row, but the lane is the
    /// *branch's* — which for a branch that has fallen behind is the fork lane
    /// beside that commit rather than the commit's own.
    Worktree { head: CommitId, on_branch: bool },
}

/// Keyed on the base cache's whole request rather than its `log_fingerprint`:
/// the answer is read out of `graph_rows`, which is recomputed for every field
/// of that request. Creating, deleting or checking out a branch changes which
/// rows `force_branch_head_lane` fires on and so which colour index each lane
/// draws, all without touching the fingerprint — a fingerprint-only key would
/// keep saturating the lane the selection used to be on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HistorySelectedLaneColorCache {
    pub(super) base_request: HistoryBaseCacheRequest,
    pub(super) anchor: HistoryLaneAnchor,
    /// `None` when the anchor is not on screen — then no lane is highlighted.
    pub(super) lane: Option<crate::view::rows::history_graph_paint::SelectedLane>,
    /// One flag per visible row: whether that row's commit is reachable from
    /// the anchor through the page's parent links. `None` exactly when `lane`
    /// is — the two halves of one highlight.
    pub(super) related_rows: Option<Arc<[bool]>>,
}

/// The selection highlight: which lane stays at full colour, and which rows
/// belong to the selection. Both are `None` when nothing is highlighted (no
/// selection, a multi-selection, or the anchor scrolled off the page).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct HistorySelectionHighlight {
    pub(super) lane: Option<crate::view::rows::history_graph_paint::SelectedLane>,
    pub(super) related_rows: Option<Arc<[bool]>>,
}

/// Marks every row reachable from `anchor_row` through `parent_visible_ixs` —
/// the page-local equivalent of `git log <anchor commit>`, following *all*
/// parents so commits that arrived through a merge count as belonging to the
/// branch. A plain DFS over indices; the page is already in memory and the
/// result is memoised by the caller.
pub(super) fn rows_reachable_from(
    parent_visible_ixs: &[SmallVec<[usize; 2]>],
    anchor_row: usize,
) -> Arc<[bool]> {
    let mut related = vec![false; parent_visible_ixs.len()];
    if anchor_row >= parent_visible_ixs.len() {
        return related.into();
    }
    related[anchor_row] = true;
    let mut pending = vec![anchor_row];
    while let Some(row_ix) = pending.pop() {
        for parent_ix in parent_visible_ixs[row_ix].iter().copied() {
            if parent_ix < related.len() && !related[parent_ix] {
                related[parent_ix] = true;
                pending.push(parent_ix);
            }
        }
    }
    related.into()
}

/// Resolves the anchor against one cache page: the lane its row draws on, and
/// the set of rows reachable from it through the page's parent links.
pub(super) fn build_selection_highlight(
    cache: &HistoryCache,
    anchor: HistoryLaneAnchor,
) -> HistorySelectionHighlight {
    let (head, on_branch) = match &anchor {
        HistoryLaneAnchor::Commit(head) => (head, None),
        HistoryLaneAnchor::Worktree { head, on_branch } => (head, Some(*on_branch)),
    };
    let Some(anchor_row) = cache.base.visible_ix_by_commit.get(head).copied() else {
        return HistorySelectionHighlight::default();
    };

    let lane = cache.base.graph_rows.get(anchor_row).and_then(|row| {
        let color_ix = match on_branch {
            Some(on_branch) => {
                crate::view::rows::history_graph_paint::band_node_for(row, on_branch).color_ix
            }
            None => row.node_color_ix,
        };
        // The colour alone would also match unrelated lanes elsewhere on
        // the page that recycled the index; this resolves it to the one
        // lane's row span.
        crate::view::rows::history_graph_paint::selected_lane_at(
            &cache.base.graph_rows,
            anchor_row,
            color_ix,
        )
    });

    HistorySelectionHighlight {
        lane,
        related_rows: Some(rows_reachable_from(
            &cache.base.parent_visible_ixs,
            anchor_row,
        )),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingHistoryReveal {
    pub(super) repo_id: RepoId,
    pub(super) commit_id: CommitId,
    pub(super) fallback_scope: Option<LogScope>,
    /// Set when the reveal is aimed at a linked worktree's row rather than the
    /// commit itself. The commit is still what gets located — the row sits
    /// directly above it — but the selection and the scroll land on the row.
    pub(super) worktree_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PendingHistoryRevealDecision {
    pub(super) set_scope: Option<LogScope>,
    pub(super) select_commit: Option<CommitId>,
    pub(super) scroll_to_list_ix: Option<usize>,
    pub(super) load_more: bool,
    pub(super) clear_pending: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HistoryCommitReferenceMatch {
    Unique { list_ix: usize, commit_id: CommitId },
    Ambiguous,
    Missing,
}

fn commit_id_matches_reference(commit_id: &CommitId, reference: &CommitId) -> bool {
    let commit_id = commit_id.as_ref();
    let reference = reference.as_ref();
    commit_id.eq_ignore_ascii_case(reference)
        || (reference.len() >= 7
            && reference.len() < commit_id.len()
            && commit_id
                .get(..reference.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(reference)))
}

fn history_selected_list_index_cache_matches(
    cache: &HistorySelectedListIndexCache,
    repo_id: RepoId,
    log_rev: u64,
    stashes_rev: u64,
    history_scope: LogScope,
    plan: &HistoryListPlan,
    selected_commit: Option<&CommitId>,
) -> bool {
    cache.repo_id == repo_id
        && cache.log_rev == log_rev
        && cache.stashes_rev == stashes_rev
        && cache.history_scope == history_scope
        && cache.show_working_tree_summary_row == plan.show_working_tree_summary_row()
        && cache.plan_fingerprint == plan.fingerprint()
        && cache.selected_commit.as_ref() == selected_commit
}

pub(super) fn set_history_selected_list_index_cache(
    cache: &mut Option<HistorySelectedListIndexCache>,
    repo_id: RepoId,
    log_rev: u64,
    stashes_rev: u64,
    history_scope: LogScope,
    plan: &HistoryListPlan,
    selected_commit: Option<CommitId>,
    list_ix: usize,
) {
    *cache = Some(HistorySelectedListIndexCache {
        repo_id,
        log_rev,
        stashes_rev,
        history_scope,
        show_working_tree_summary_row: plan.show_working_tree_summary_row(),
        plan_fingerprint: plan.fingerprint(),
        selected_commit,
        list_ix,
    });
}

/// What the history selection currently rests on, for the list-index
/// bookkeeping. The three states are mutually exclusive: a commit, a worktree
/// row, or -- when neither -- the working-tree row.
#[derive(Clone, Copy)]
pub(super) struct HistorySelectionRef<'a> {
    pub(super) commit: Option<&'a CommitId>,
    pub(super) worktree_selected: bool,
}

pub(super) fn peek_history_selected_list_index(
    cache: Option<&HistorySelectedListIndexCache>,
    repo_id: RepoId,
    log_rev: u64,
    stashes_rev: u64,
    history_scope: LogScope,
    plan: &HistoryListPlan,
    selection: HistorySelectionRef<'_>,
    visible_indices: &HistoryVisibleIndices,
    commits: &[Commit],
) -> Option<usize> {
    // A selected worktree row also leaves the commit selection empty, but it is
    // not the working-tree row -- claiming index 0 here would leave two rows
    // looking selected and send the scroll bookkeeping to the wrong one. The
    // worktree row's own index comes from `worktree_row_list_ix`.
    if selection.worktree_selected {
        return None;
    }
    let selected_commit = selection.commit;
    // The uncommitted sentinel is the pinned row's own selection, not a
    // commit to look up among the visible ones: it sits at list index 0
    // whenever the row shows, and anchors to nothing when it does not.
    let working_tree_row_selected = selected_commit.is_some_and(CommitId::is_uncommitted);
    if working_tree_row_selected {
        return plan.show_working_tree_summary_row().then_some(0);
    }
    if plan.show_working_tree_summary_row() && selected_commit.is_none() {
        return Some(0);
    }

    if let Some(list_ix) = cache
        .filter(|entry| {
            history_selected_list_index_cache_matches(
                entry,
                repo_id,
                log_rev,
                stashes_rev,
                history_scope,
                plan,
                selected_commit,
            )
        })
        .map(|entry| entry.list_ix)
    {
        return Some(list_ix);
    }

    let selected_commit = selected_commit?;
    match visible_commit_match_for_reference(selected_commit, visible_indices, commits, plan) {
        HistoryCommitReferenceMatch::Unique { list_ix, .. } => Some(list_ix),
        HistoryCommitReferenceMatch::Ambiguous | HistoryCommitReferenceMatch::Missing => None,
    }
}

fn visible_commit_match_for_reference(
    reference: &CommitId,
    visible_indices: &HistoryVisibleIndices,
    commits: &[Commit],
    plan: &HistoryListPlan,
) -> HistoryCommitReferenceMatch {
    let mut found = None;

    for (visible_ix, commit_ix) in visible_indices.iter().enumerate() {
        let Some(commit) = commits.get(commit_ix) else {
            continue;
        };
        if !commit_id_matches_reference(&commit.id, reference) {
            continue;
        }

        let next = (plan.list_ix_for_visible(visible_ix), commit.id.clone());
        if found.is_some() {
            return HistoryCommitReferenceMatch::Ambiguous;
        }
        found = Some(next);
    }

    if let Some((list_ix, commit_id)) = found {
        HistoryCommitReferenceMatch::Unique { list_ix, commit_id }
    } else {
        HistoryCommitReferenceMatch::Missing
    }
}

/// What clicking a worktree in the sidebar should focus in the log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum WorktreeRevealTarget {
    /// The pinned row at the top -- only this tab's own changes live there.
    WorkingTreeSummaryRow,
    /// A linked worktree's own uncommitted-changes row.
    WorktreeRow {
        head: CommitId,
        fallback_scope: Option<LogScope>,
    },
    Commit {
        head: CommitId,
        fallback_scope: Option<LogScope>,
    },
    /// A clean worktree whose HEAD we could not resolve; nothing to aim at.
    Nothing,
}

/// One rule for every worktree row: its changes if it has any, otherwise the
/// commit it sits on. Where "its changes" live differs -- this tab's are pinned
/// at the top of the log, every other worktree's are a row of their own.
///
/// `worktree_is_dirty` is `None` while the scan has not answered for this
/// worktree yet, which is not the same as answering that it is clean: aiming at
/// the commit on an unknown commits to a row set that is about to grow, and the
/// first scan reply then shifts the log under the user. Aiming at the row
/// instead costs nothing when the guess is wrong -- the reveal keeps the commit
/// as its scroll target until the row exists, and a worktree that turns out
/// clean has its selection dropped by the reducer.
pub(super) fn worktree_reveal_target(
    is_current: bool,
    current_has_changes: bool,
    worktree_is_dirty: Option<bool>,
    head: Option<CommitId>,
) -> WorktreeRevealTarget {
    if is_current && current_has_changes {
        return WorktreeRevealTarget::WorkingTreeSummaryRow;
    }
    let Some(head) = head else {
        return WorktreeRevealTarget::Nothing;
    };
    // A linked worktree's branch need not be in the current scope -- the same
    // reason a non-HEAD branch row falls back to all branches. It applies to the
    // row as much as to the commit: the row is anchored to the same commit, and
    // without the fallback a dirty worktree on an out-of-scope branch had
    // nothing to scroll to.
    let fallback_scope = (!is_current).then_some(LogScope::AllBranches);
    if !is_current && worktree_is_dirty != Some(false) {
        return WorktreeRevealTarget::WorktreeRow {
            head,
            fallback_scope,
        };
    }
    WorktreeRevealTarget::Commit {
        head,
        fallback_scope,
    }
}

/// Where the worktree row for `path` currently sits, if anywhere.
pub(super) fn worktree_row_list_ix(
    plan: &HistoryListPlan,
    repo: Option<&RepoState>,
    path: &std::path::Path,
) -> Option<usize> {
    let Loadable::Ready(dirty) = &repo?.worktree_dirty else {
        return None;
    };
    let worktree_ix = dirty.iter().position(|summary| summary.path == path)?;
    plan.list_ix_for_worktree(worktree_ix)
}

pub(super) fn resolve_history_selected_list_index(
    cache: &mut Option<HistorySelectedListIndexCache>,
    repo_id: RepoId,
    log_rev: u64,
    stashes_rev: u64,
    history_scope: LogScope,
    plan: &HistoryListPlan,
    selection: HistorySelectionRef<'_>,
    visible_indices: &HistoryVisibleIndices,
    commits: &[Commit],
) -> Option<usize> {
    let list_ix = peek_history_selected_list_index(
        cache.as_ref(),
        repo_id,
        log_rev,
        stashes_rev,
        history_scope,
        plan,
        selection,
        visible_indices,
        commits,
    )?;
    set_history_selected_list_index_cache(
        cache,
        repo_id,
        log_rev,
        stashes_rev,
        history_scope,
        plan,
        selection.commit.cloned(),
        list_ix,
    );
    Some(list_ix)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decide_pending_history_reveal(
    pending: &PendingHistoryReveal,
    active_repo_id: Option<RepoId>,
    current_scope: Option<LogScope>,
    selected_commit: Option<&CommitId>,
    _log_rev: u64,
    _stashes_rev: u64,
    log_loading_more: bool,
    display_page: Option<&LogPage>,
    live_page_has_more: Option<bool>,
    cache_request_matches: bool,
    visible_indices: Option<&HistoryVisibleIndices>,
    plan: &HistoryListPlan,
    _selected_list_index_cache: Option<&HistorySelectedListIndexCache>,
) -> PendingHistoryRevealDecision {
    let mut decision = PendingHistoryRevealDecision::default();

    if active_repo_id != Some(pending.repo_id) {
        decision.clear_pending = true;
        return decision;
    }

    let Some(current_scope) = current_scope else {
        decision.clear_pending = true;
        return decision;
    };

    // Selecting a target that is *not* loaded yet is `Msg::RevealCommit`'s job:
    // it resolves the reference against the object database and shows the commit
    // straight away, without this deciding anything about a row it cannot see.
    //
    // A full id already sitting in the loaded page is the exception. Selecting it
    // needs no round-trip, and cannot flicker either: page reconciliation only
    // clears a selection the page does not contain.
    let Some(display_page) = display_page else {
        return decision;
    };
    if selected_commit != Some(&pending.commit_id)
        && display_page
            .commits
            .iter()
            .any(|commit| commit.id == pending.commit_id)
    {
        decision.select_commit = Some(pending.commit_id.clone());
    }

    if !cache_request_matches {
        return decision;
    }
    let Some(visible_indices) = visible_indices else {
        return decision;
    };

    match visible_commit_match_for_reference(
        &pending.commit_id,
        visible_indices,
        &display_page.commits,
        plan,
    ) {
        HistoryCommitReferenceMatch::Unique { list_ix, commit_id } => {
            // The row carries the full id; an abbreviated reference upgrades to
            // it here even if the resolve reply has not landed yet.
            if selected_commit != Some(&commit_id) {
                decision.select_commit = Some(commit_id);
            }
            decision.scroll_to_list_ix = Some(list_ix);
            decision.clear_pending = true;
            return decision;
        }
        HistoryCommitReferenceMatch::Ambiguous => {
            decision.select_commit = None;
            decision.clear_pending = true;
            return decision;
        }
        HistoryCommitReferenceMatch::Missing => {}
    }

    match live_page_has_more {
        Some(true) => {
            decision.load_more = !log_loading_more;
            return decision;
        }
        Some(false) => {}
        None => return decision,
    }

    if let Some(fallback_scope) = pending.fallback_scope
        && current_scope != fallback_scope
    {
        decision.set_scope = Some(fallback_scope);
        return decision;
    }

    decision.clear_pending = true;
    decision
}
