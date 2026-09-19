//! Reducer for the native `.git/hooks` UI (feature 4).
//!
//! Handles `Msg::RequestRepoHooks` / `CancelRepoHooks` / `SetRepoHookEnabled` /
//! `CreateRepoHook` / `DeleteRepoHook` (request → effect, or clear on cancel)
//! and the `InternalMsg::RepoHooksLoaded` reply that stores the result.

use super::ReduceOutcome;
use crate::model::{AppState, DiagnosticKind, Loadable, RepoId, RepoState};
use crate::msg::{Effect, InternalMsg, Msg};
use std::sync::Arc;
use worktree_core::domain::RepoHookList;

pub(super) fn reduce_repo_hooks(msg: Msg, state: &mut AppState) -> ReduceOutcome {
    match msg {
        Msg::RequestRepoHooks { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                set_loading(repo_state);
                return ReduceOutcome::Handled(vec![Effect::LoadRepoHooks { repo_id }]);
            }
            ReduceOutcome::NotHandled(Msg::RequestRepoHooks { repo_id })
        }
        Msg::CancelRepoHooks { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.repo_hooks = Loadable::NotLoaded;
            }
            ReduceOutcome::Handled(Vec::new())
        }
        Msg::SetRepoHookEnabled {
            repo_id,
            name,
            enabled,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                set_loading(repo_state);
                return ReduceOutcome::Handled(vec![Effect::SetRepoHookEnabled {
                    repo_id,
                    name,
                    enabled,
                }]);
            }
            ReduceOutcome::NotHandled(Msg::SetRepoHookEnabled {
                repo_id,
                name,
                enabled,
            })
        }
        Msg::CreateRepoHook {
            repo_id,
            name,
            from_sample,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                set_loading(repo_state);
                return ReduceOutcome::Handled(vec![Effect::CreateRepoHook {
                    repo_id,
                    name,
                    from_sample,
                }]);
            }
            ReduceOutcome::NotHandled(Msg::CreateRepoHook {
                repo_id,
                name,
                from_sample,
            })
        }
        Msg::DeleteRepoHook { repo_id, name } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                set_loading(repo_state);
                return ReduceOutcome::Handled(vec![Effect::DeleteRepoHook { repo_id, name }]);
            }
            ReduceOutcome::NotHandled(Msg::DeleteRepoHook { repo_id, name })
        }
        Msg::Internal(InternalMsg::RepoHooksLoaded { repo_id, result }) => {
            repo_hooks_loaded(state, repo_id, result);
            ReduceOutcome::Handled(Vec::new())
        }
        other => ReduceOutcome::NotHandled(other),
    }
}

fn set_loading(repo_state: &mut RepoState) {
    repo_state.repo_hooks = Loadable::Loading;
    repo_state.repo_hooks_rev = repo_state.repo_hooks_rev.wrapping_add(1);
}

pub(super) fn repo_hooks_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: Result<Arc<RepoHookList>, String>,
) {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(v) => {
                repo_state.repo_hooks = Loadable::Ready(v);
                repo_state.repo_hooks_rev = repo_state.repo_hooks_rev.wrapping_add(1);
            }
            Err(e) => {
                super::util::push_diagnostic(repo_state, DiagnosticKind::Error, e.clone());
                repo_state.repo_hooks = Loadable::Error(e);
                repo_state.repo_hooks_rev = repo_state.repo_hooks_rev.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppState, Loadable, RepoId, RepoState};
    use crate::msg::{Effect, Msg};
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::RepoSpec;
    use worktree_core::domain::{RepoHookList, RepoHookName};

    fn state_with_repo(repo_id: RepoId) -> AppState {
        let mut state = AppState::default();
        state.repos.push(RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        ));
        state
    }

    #[test]
    fn request_repo_hooks_emits_load_effect_and_sets_loading() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_repo_hooks(Msg::RequestRepoHooks { repo_id }, &mut state);
        match outcome {
            ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::LoadRepoHooks { repo_id: id } if id == repo_id
                ));
            }
            ReduceOutcome::NotHandled(_) => panic!("RequestRepoHooks should be Handled"),
        }
        assert!(matches!(state.repos[0].repo_hooks, Loadable::Loading));
    }

    #[test]
    fn cancel_repo_hooks_clears_loading_and_emits_no_effect() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        state.repos[0].repo_hooks = Loadable::Loading;
        let outcome = reduce_repo_hooks(Msg::CancelRepoHooks { repo_id }, &mut state);
        assert!(matches!(outcome, ReduceOutcome::Handled(ref e) if e.is_empty()));
        assert!(matches!(state.repos[0].repo_hooks, Loadable::NotLoaded));
    }

    #[test]
    fn set_repo_hook_enabled_emits_effect_and_loads() {
        let repo_id = RepoId(1);
        let name = RepoHookName::from("pre-commit");
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_repo_hooks(
            Msg::SetRepoHookEnabled {
                repo_id,
                name: name.clone(),
                enabled: true,
            },
            &mut state,
        );
        match outcome {
            ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::SetRepoHookEnabled { repo_id: id, enabled, .. }
                        if id == repo_id && enabled
                ));
            }
            ReduceOutcome::NotHandled(_) => panic!("SetRepoHookEnabled should be Handled"),
        }
        assert!(matches!(state.repos[0].repo_hooks, Loadable::Loading));
    }

    #[test]
    fn create_repo_hook_emits_effect() {
        let repo_id = RepoId(1);
        let name = RepoHookName::from("pre-commit");
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_repo_hooks(
            Msg::CreateRepoHook {
                repo_id,
                name: name.clone(),
                from_sample: true,
            },
            &mut state,
        );
        match outcome {
            ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::CreateRepoHook { repo_id: id, from_sample, .. }
                        if id == repo_id && from_sample
                ));
            }
            ReduceOutcome::NotHandled(_) => panic!("CreateRepoHook should be Handled"),
        }
    }

    #[test]
    fn delete_repo_hook_emits_effect() {
        let repo_id = RepoId(1);
        let name = RepoHookName::from("pre-commit");
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_repo_hooks(
            Msg::DeleteRepoHook {
                repo_id,
                name: name.clone(),
            },
            &mut state,
        );
        match outcome {
            ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::DeleteRepoHook { repo_id: id, .. } if id == repo_id
                ));
            }
            ReduceOutcome::NotHandled(_) => panic!("DeleteRepoHook should be Handled"),
        }
    }

    #[test]
    fn repo_hooks_loaded_stores_ready_list_and_bumps_rev() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let list = Arc::new(RepoHookList(vec![]));
        let before = state.repos[0].repo_hooks_rev;
        repo_hooks_loaded(&mut state, repo_id, Ok(list.clone()));
        match &state.repos[0].repo_hooks {
            Loadable::Ready(v) => assert!(Arc::ptr_eq(v, &list)),
            other => panic!("expected Ready, got {other:?}"),
        }
        assert_eq!(state.repos[0].repo_hooks_rev, before + 1);
    }

    #[test]
    fn repo_hooks_loaded_stores_error_on_failure() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        repo_hooks_loaded(&mut state, repo_id, Err("boom".to_string()));
        assert!(matches!(state.repos[0].repo_hooks, Loadable::Error(_)));
    }

    #[test]
    fn unknown_message_is_not_handled() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_repo_hooks(Msg::LoadRepoStatistics { repo_id }, &mut state);
        assert!(matches!(outcome, ReduceOutcome::NotHandled(_)));
    }
}
