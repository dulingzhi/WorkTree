//! The single dispatch point that turns a [`RepoChange`] into the concrete set
//! of panel-refresh effects. See
//! `docs/superpowers/plans/2026-09-16-repo-change-unified-refresh.md` §4.6.
//!
//! **`dispatch_repo_change` does NOT cancel in-flight loads.** Cancellation is a
//! caller concern, because the three trigger paths disagree on it: the command
//! path and the local-action path cancel (so a slow log walk in flight cannot
//! swallow a refresh — the "commit list is stale after fetch/pull/push" race),
//! but the external-monitor path must NOT cancel, or it would lose the
//! incremental worktree merge that `loads_in_flight` dedups into a single scan.
//! Each caller decides its own cancel policy and calls `dispatch_repo_change`
//! afterwards; see `repo_command_finished` / `repo_action_finished` /
//! `repo_externally_changed`.

use crate::model::{AppState, Loadable, RepoId, RepoLoadsInFlight, RepoState};
use crate::msg::{Effect, RepoChange};

use super::util::{
    append_refresh_full_effects, append_refresh_primary_effects,
    append_requested_status_refresh_effects, append_targeted_status_refresh,
    refresh_full_effect_capacity,
};
use std::path::PathBuf;

/// Emit the effects needed to bring the relevant panels back in sync after
/// `change`, narrowing the refresh to what the semantic change actually touched.
///
/// Cancellation is the caller's responsibility (see the module docs): this
/// function only issues the effects the `change` variant implies, deduped
/// through `loads_in_flight` so an already-in-flight load is coalesced rather
/// than re-issued. `incremental_status_paths` is the targeted-status knob: when
/// a status variant (`IndexChanged` / `StatusChanged` / `WorktreeChanged`) knows
/// exactly which paths moved, and no coarse status scan is in flight against a
/// settled snapshot, the refresh is answered by a `LoadStatusForPaths` merge
/// instead of a full worktree walk — keeping external save storms and path-scoped
/// stage/unstage actions from rescanning the whole tree.
///
/// The refresh set is keyed on the `RepoChange` variant, not on how the change
/// was triggered (command / external / action): every caller translates its raw
/// trigger into a `RepoChange` and this is the only place that decides which
/// panels reload, so the paths can no longer drift. `Anything` stays a full
/// rescan for the "unknown" variants (submodule pointer changes, conflict
/// tooling, export/archive/gc); the rest are precise supersets of what each
/// change touches. The unit tests below lock each variant's effect set so a
/// narrowing can never silently drop a panel.
pub(super) fn dispatch_repo_change(
    state: &mut AppState,
    repo_id: RepoId,
    change: RepoChange,
    incremental_status_paths: Option<&[PathBuf]>,
) -> Vec<Effect> {
    let mut effects = Vec::with_capacity(refresh_full_effect_capacity());
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return effects;
    };
    let git_log_settings = state.git_log_settings;

    match change {
        // A rescan / forced full refresh: the exact superset the callers did
        // before converging here. Keeps the "unknown" variants (submodule
        // pointer changes, conflict tooling, export/archive/gc) on a full
        // rescan so nothing they touch is missed.
        RepoChange::Anything => {
            append_refresh_full_effects(repo_state, git_log_settings, &mut effects);
        }
        // Refs (branches/tags/remotes) brought in by fetch/pull/push/branch ops,
        // plus tag CRUD: primary panes + branch/remote lists + the tag list, and
        // drop the cached recent-commit-messages. Also covers PushTag /
        // DeleteRemoteTag, whose tag reload the old full-refresh path missed.
        RepoChange::RefsChanged => {
            append_refresh_primary_effects(repo_state, &mut effects);
            append_branch_list_effects(repo_state, &mut effects);
            append_tags_effects(repo_state, &mut effects);
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
        }
        // The tag set changed (create/delete/prune local tags): only the tag list.
        RepoChange::TagsChanged => {
            append_tags_effects(repo_state, &mut effects);
        }
        // HEAD moved (checkout/reset/rebase/cherry-pick/merge-abort): primary
        // panes + branch lists + drop recent-commit-messages.
        RepoChange::HeadMoved => {
            append_refresh_primary_effects(repo_state, &mut effects);
            append_branch_list_effects(repo_state, &mut effects);
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
        }
        // A commit was created or rewritten: primary panes (which include the
        // log and head) + branch lists + drop recent-commit-messages. Same
        // refresh shape as a ref move; kept distinct so callers can specialise.
        RepoChange::Committed => {
            append_refresh_primary_effects(repo_state, &mut effects);
            append_branch_list_effects(repo_state, &mut effects);
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
        }
        // Index changed (stage/unstage/restore --staged) / status changed /
        // working-tree content changed (save file / .gitignore / add-remove
        // worktree): refresh the status lanes. When the exact paths are known
        // and no coarse scan is in flight against a settled snapshot, merge them
        // through `LoadStatusForPaths` instead of a full walk; otherwise fall back
        // to the requested full status refresh. The active diff / blame
        // invalidation is the caller's specialized extra (see
        // `repo_command_finished` / `repo_externally_changed`).
        RepoChange::IndexChanged | RepoChange::StatusChanged | RepoChange::WorktreeChanged => {
            let merged = incremental_status_paths.is_some_and(|paths| {
                append_targeted_status_refresh(repo_state, &mut effects, paths)
            });
            if !merged {
                append_requested_status_refresh_effects(repo_state, &mut effects);
            }
        }
        // Local branch set changed (prune merged): refresh the branch lists.
        RepoChange::BranchesChanged => {
            append_branch_list_effects(repo_state, &mut effects);
        }
    }

    // The coarse "repo changed" ping: one signal every UI can subscribe to.
    repo_state.bump_content_rev();
    effects
}

/// Re-issue the branch / remote / remote-branch lists. Used by every variant
/// whose change touches refs. Unconditional (not gated on `active_repo`) to
/// preserve the pre-convergence command-path behaviour, where the full refresh
/// reloaded these for every repo.
fn append_branch_list_effects(repo_state: &mut RepoState, effects: &mut Vec<Effect>) {
    let repo_id = repo_state.id;
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::BRANCHES)
    {
        effects.push(Effect::LoadBranches { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTES)
    {
        effects.push(Effect::LoadRemotes { repo_id });
    }
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTE_BRANCHES)
    {
        effects.push(Effect::LoadRemoteBranches { repo_id });
    }
}

/// Re-issue the tag list: mark it `NotLoaded` and request a fresh load. The
/// `loads_in_flight` flag dedupes against any caller-supplied tag refresh.
fn append_tags_effects(repo_state: &mut RepoState, effects: &mut Vec<Effect>) {
    let repo_id = repo_state.id;
    repo_state.set_tags(Loadable::NotLoaded);
    if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
        effects.push(Effect::LoadTags { repo_id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Loadable, RepoId, RepoLoadsInFlight, RepoState};
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{LogPage, RepoSpec, RepoStatus};

    fn state_with_ready_repo(repo_id: RepoId) -> AppState {
        let mut state = AppState::default();
        let mut repo_state = RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        );
        repo_state.history_state.log = Loadable::Ready(Arc::new(LogPage {
            commits: vec![],
            next_cursor: None,
        }));
        state.repos.push(repo_state);
        state.active_repo = Some(repo_id);
        state
    }

    fn state_with_ready_status(repo_id: RepoId) -> AppState {
        let mut state = state_with_ready_repo(repo_id);
        state.repos[0].status = Loadable::Ready(Arc::new(RepoStatus::default()));
        state
    }

    #[test]
    fn dispatch_does_not_cancel_in_flight() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);
        // Simulate a log walk already in flight.
        state.repos[0]
            .loads_in_flight
            .request(RepoLoadsInFlight::LOG);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::RefsChanged, None);

        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::CancelRepoLoads { repo_id: id, .. } if *id == repo_id)
            ),
            "dispatch must NOT cancel in-flight loads (callers own the cancel policy)"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "dispatch must still re-issue the log load"
        );
    }

    #[test]
    fn dispatch_refreshes_even_when_nothing_was_in_flight() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::Anything, None);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "a change with nothing in flight still refreshes the log"
        );
    }

    #[test]
    fn dispatch_committed_refreshes_primary_and_branch_lists() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);
        // Simulate a log walk already in flight.
        state.repos[0]
            .loads_in_flight
            .request(RepoLoadsInFlight::LOG);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::Committed, None);

        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::CancelRepoLoads { repo_id: id, .. } if *id == repo_id)
            ),
            "dispatch must NOT cancel in-flight loads"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "Committed must re-issue the log load"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadHeadBranch { repo_id: id, .. } if *id == repo_id)),
            "Committed must refresh the head branch"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadBranches { repo_id: id, .. } if *id == repo_id)),
            "Committed must refresh the branch list"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadTags { repo_id: id, .. } if *id == repo_id)),
            "Committed must NOT reload tags (precise, not full)"
        );
    }

    #[test]
    fn dispatch_tags_changed_reloads_only_tags() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::TagsChanged, None);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadTags { repo_id: id, .. } if *id == repo_id)),
            "TagsChanged must reload the tag list"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "TagsChanged must NOT reload the log (precise, not full)"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadBranches { repo_id: id, .. } if *id == repo_id)),
            "TagsChanged must NOT reload branches (precise, not full)"
        );
    }

    #[test]
    fn dispatch_index_changed_reloads_status_only() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::IndexChanged, None);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadStatus { repo_id: id, .. } if *id == repo_id)),
            "IndexChanged must reload status"
        );
        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::LoadStatusForPaths { repo_id: id, .. } if *id == repo_id)
            ),
            "IndexChanged without paths must NOT use the targeted merge"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "IndexChanged must NOT reload the log (precise, not full)"
        );
    }

    #[test]
    fn dispatch_head_moved_reloads_primary_and_branches_not_tags() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::HeadMoved, None);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "HeadMoved must reload the log"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadBranches { repo_id: id, .. } if *id == repo_id)),
            "HeadMoved must reload branches"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadTags { repo_id: id, .. } if *id == repo_id)),
            "HeadMoved must NOT reload tags (precise, not full)"
        );
    }

    #[test]
    fn dispatch_worktree_changed_with_paths_merges_targeted() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_status(repo_id);
        let paths = vec![PathBuf::from("/tmp/repo/foo.txt")];

        let effects = dispatch_repo_change(
            &mut state,
            repo_id,
            RepoChange::WorktreeChanged,
            Some(&paths),
        );

        assert!(
            effects.iter().any(
                |e| matches!(e, Effect::LoadStatusForPaths { repo_id: id, .. } if *id == repo_id)
            ),
            "WorktreeChanged with known paths must merge through LoadStatusForPaths"
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadStatus { repo_id: id, .. } if *id == repo_id)),
            "WorktreeChanged with known paths must NOT also issue a full LoadStatus"
        );
    }

    #[test]
    fn dispatch_worktree_changed_falls_back_to_full_without_paths() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_status(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::WorktreeChanged, None);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadStatus { repo_id: id, .. } if *id == repo_id)),
            "WorktreeChanged without paths must fall back to a full status refresh"
        );
        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::LoadStatusForPaths { repo_id: id, .. } if *id == repo_id)
            ),
            "WorktreeChanged without paths must NOT use the targeted merge"
        );
    }

    #[test]
    fn dispatch_worktree_changed_falls_back_to_full_when_status_not_settled() {
        let repo_id = RepoId(1);
        // status is NotLoaded (no settled snapshot to merge onto).
        let mut state = state_with_ready_repo(repo_id);
        let paths = vec![PathBuf::from("/tmp/repo/foo.txt")];

        let effects = dispatch_repo_change(
            &mut state,
            repo_id,
            RepoChange::WorktreeChanged,
            Some(&paths),
        );

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadStatus { repo_id: id, .. } if *id == repo_id)),
            "WorktreeChanged with paths but no settled snapshot must fall back to full"
        );
        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::LoadStatusForPaths { repo_id: id, .. } if *id == repo_id)
            ),
            "no settled snapshot means no targeted merge"
        );
    }
}
