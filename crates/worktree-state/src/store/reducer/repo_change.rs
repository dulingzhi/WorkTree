//! The single dispatch point that turns a [`RepoChange`] into the concrete set
//! of panel-refresh effects. See
//! `docs/superpowers/plans/2026-09-16-repo-change-unified-refresh.md`.

use crate::model::{AppState, RepoId};
use crate::msg::{Effect, RepoChange};

use super::repo_management::append_cancel_repo_loads_effect_for_repo;
use super::util::{append_refresh_full_effects, refresh_full_effect_capacity};

/// Emit the effects needed to bring every panel back in sync after `change`.
///
/// Always cancels in-flight loads first (mirroring `repo_action_finished`): a
/// repo change makes every prior load stale, and a slow log walk in flight
/// would otherwise swallow the refresh and leave the commit list stale for tens
/// of seconds on a large repository. This is the fix for the "commit list is
/// stale after a fetch/pull/push" race.
///
/// P1 note: the refresh set is currently a uniform full refresh for every
/// variant — the exact superset of what both caller paths did before
/// converging here. Per-variant narrowing (e.g. `Committed` → primary + branch
/// lists + recent-commit-messages; `RefsChanged` → + tags) is the next
/// increment and is guarded by the unit tests below so it cannot silently
/// regress a panel.
pub(super) fn dispatch_repo_change(
    state: &mut AppState,
    repo_id: RepoId,
    _change: RepoChange,
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
    append_refresh_full_effects(repo_state, git_log_settings, &mut effects);
    effects
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
        state.repos[0].loads_in_flight.request(RepoLoadsInFlight::LOG);

        let effects = dispatch_repo_change(&mut state, repo_id, RepoChange::RefsChanged);

        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::CancelRepoLoads { repo_id: id, .. } if *id == repo_id)),
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
}
