//! The single dispatch point that turns a [`RepoChange`] into the concrete set
//! of panel-refresh effects. See
//! `docs/superpowers/plans/2026-09-16-repo-change-unified-refresh.md`.

use crate::model::{AppState, Loadable, RepoId, RepoLoadsInFlight, RepoState};
use crate::msg::{Effect, RepoChange};

use super::repo_management::append_cancel_repo_loads_effect_for_repo;
use super::util::{
    append_refresh_full_effects, append_refresh_primary_effects,
    append_requested_status_refresh_effects, refresh_full_effect_capacity,
};

/// Emit the effects needed to bring the relevant panels back in sync after
/// `change`, narrowing the refresh to what the semantic change actually touched.
///
/// Always cancels in-flight loads first (mirroring `repo_action_finished`): a
/// repo change makes every prior load stale, and a slow log walk in flight
/// would otherwise swallow the refresh and leave the commit list stale for tens
/// of seconds on a large repository. This is the fix for the "commit list is
/// stale after a fetch/pull/push" race.
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
) -> Vec<Effect> {
    let mut effects = Vec::with_capacity(refresh_full_effect_capacity());
    // Cancel every in-flight load before re-issuing: the bumped epoch drops the
    // stale replies, the cleared flags let the refresh dispatch fresh loads, and
    // the effect cancels the orphaned worker tasks.
    append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects);
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
        // Index changed (stage/unstage/restore --staged) or status changed:
        // refresh both status lanes. The active diff / blame invalidation is the
        // caller's responsibility (see `repo_command_finished`).
        RepoChange::IndexChanged | RepoChange::StatusChanged => {
            append_requested_status_refresh_effects(repo_state, &mut effects);
        }
        // Working-tree content changed (save file / .gitignore / add-remove
        // worktree): refresh status (both lanes). File-browser and the active
        // diff are the caller's specialized extras.
        RepoChange::WorktreeChanged => {
            append_requested_status_refresh_effects(repo_state, &mut effects);
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
    use std::sync::Arc;
    use worktree_core::domain::{LogPage, RepoSpec};

    fn state_with_ready_repo(repo_id: RepoId) -> AppState {
        let mut state = AppState::default();
        let mut repo_state = RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: std::path::PathBuf::from("/tmp/repo"),
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

    #[test]
    fn dispatch_cancels_in_flight_then_refreshes() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);
        // Simulate a log walk already in flight.
        state.repos[0]
            .loads_in_flight
            .request(RepoLoadsInFlight::LOG);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::RefsChanged);

        assert!(
            effects.iter().any(
                |e| matches!(e, Effect::CancelRepoLoads { repo_id: id, .. } if *id == repo_id)
            ),
            "dispatch must cancel in-flight loads before refreshing"
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadLog { repo_id: id, .. } if *id == repo_id)),
            "dispatch must re-issue the log load after cancelling the stale one"
        );
    }

    #[test]
    fn dispatch_refreshes_even_when_nothing_was_in_flight() {
        let repo_id = RepoId(1);
        let mut state = state_with_ready_repo(repo_id);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::Anything);

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

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::Committed);

        assert!(
            effects.iter().any(
                |e| matches!(e, Effect::CancelRepoLoads { repo_id: id, .. } if *id == repo_id)
            ),
            "Committed must cancel in-flight loads before refreshing"
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

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::TagsChanged);

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

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::IndexChanged);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadStatus { repo_id: id, .. } if *id == repo_id)),
            "IndexChanged must reload status"
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

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::HeadMoved);

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
}
