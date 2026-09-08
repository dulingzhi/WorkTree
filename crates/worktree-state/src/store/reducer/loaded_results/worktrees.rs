use super::util::push_diagnostic;
use crate::model::{
    AppState, DiagnosticKind, ForeignDiffOrigin, Loadable, RepoId, RepoLoadsInFlight, RepoState,
};
use crate::msg::Effect;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::domain::{
    CommitDetails, CommitFileChange, CommitId, FileStatus, Worktree, WorktreeDirtySummary,
};
use worktree_core::error::Error;

pub(in crate::store::reducer) fn worktrees_loaded(
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

pub(in crate::store::reducer) fn worktree_dirty_loaded(
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

pub(in crate::store::reducer) fn select_worktree_uncommitted(
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
pub(in crate::store::reducer) fn select_working_tree_summary(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
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
pub(in crate::store::reducer) fn resync_working_tree_details_if_selected(
    repo_state: &mut RepoState,
) {
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
pub(in crate::store::reducer) fn retire_orphaned_worktree_diffs(state: &mut AppState) {
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

pub(in crate::store::reducer) fn load_worktrees(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
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

pub(in crate::store::reducer) fn load_worktree_dirty(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
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
pub(in crate::store::reducer) fn request_worktree_dirty_effect(
    repo_state: &mut RepoState,
) -> Option<Effect> {
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
pub(in crate::store::reducer) fn worktree_dirty_effect(repo_state: &RepoState) -> Effect {
    Effect::LoadWorktreeDirty {
        repo_id: repo_state.id,
        workdir: repo_state.spec.workdir.clone(),
        files_for: repo_state.history_state.worktree_selection.clone(),
    }
}
