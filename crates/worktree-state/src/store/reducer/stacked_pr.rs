//! Reducer for the stacked-PR feature (iteration 07), first cut: the data layer.
//!
//! Handles the load/store round-trip plus the pure in-memory mutations that
//! record and reorder a repository's stack relationship:
//!
//! * `InternalMsg::StackMetadataLoaded` — stores the loaded metadata (or an error).
//! * `Msg::LoadStackMetadata` — kicks off an on-demand load when nothing is loaded.
//! * `Msg::CreateStackedBranch` — records a branch as part of a stack.
//! * `Msg::ReorderStack` — reassigns every branch's `order` from an explicit list.
//! * `Msg::DeleteStackBranch` — removes a branch and reparents its children.
//! * `Msg::RestackStack` — replays every branch on top of its (rebased) parent
//!   via `git rebase --onto`, in dependency order (design doc §7).
//!
//! `Msg::CreateStackedBranch` records the relationship in `StackMetadata` and
//! reuses the existing `Effect::CreateBranch` to actually create the git branch
//! (started at the parent's tip, or HEAD for a root). `Msg::RestackStack`
//! computes the ordered plan and dispatches `Effect::RestackStack`; the gix
//! backend performs the actual `rebase --onto` replay (design doc §7).

use super::ReduceOutcome;
use crate::model::{AppState, DiagnosticKind, Loadable, RepoId, RepoState};
use crate::msg::{Effect, InternalMsg, Msg};
use worktree_core::domain::{
    CommitId, StackBranch, StackMetadata, StackRestackPlan, StackRestackStep,
};

pub(super) fn reduce_stacked_pr(msg: Msg, state: &mut AppState) -> ReduceOutcome {
    match msg {
        Msg::Internal(InternalMsg::StackMetadataLoaded { repo_id, result }) => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                match result {
                    Ok(metadata) => {
                        repo_state.set_stacks(Loadable::Ready(metadata));
                        Vec::new()
                    }
                    Err(error) => {
                        super::util::push_diagnostic(
                            repo_state,
                            DiagnosticKind::Error,
                            error.to_string(),
                        );
                        repo_state.set_stacks(Loadable::Error(error.to_string()));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::LoadStackMetadata { repo_id } => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                if matches!(repo_state.stacks, Loadable::NotLoaded) {
                    repo_state.stacks = Loadable::Loading;
                    vec![Effect::LoadStackMetadata { repo_id }]
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::CreateStackedBranch {
            repo_id,
            name,
            parent,
        } => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                let already_in_stack = matches!(&repo_state.stacks, Loadable::Ready(shared) if shared.by_name(&name).is_some());
                if already_in_stack {
                    super::util::push_diagnostic(
                        repo_state,
                        DiagnosticKind::Error,
                        format!("Branch `{name}` is already part of a stack"),
                    );
                    Vec::new()
                } else {
                    // Start point for the new git branch: the parent branch's
                    // tip, or HEAD for a root branch.
                    let target = parent
                        .as_deref()
                        .and_then(|p| branch_tip(repo_state, p))
                        .or_else(|| repo_state.head_commit_id());

                    let mut metadata = current_metadata(repo_state);
                    let order = parent
                        .as_ref()
                        .and_then(|p| metadata.by_name(p))
                        .map(|b| b.order + 1)
                        .unwrap_or(0);
                    metadata
                        .branches
                        .push(StackBranch::new(name.clone(), parent, order));
                    repo_state.set_stacks(Loadable::Ready(metadata.clone()));

                    let mut effects = vec![Effect::PersistStackMetadata { repo_id, metadata }];
                    match target {
                        Some(target) => {
                            effects.insert(
                                0,
                                Effect::CreateBranch {
                                    repo_id,
                                    name,
                                    target: target.to_string(),
                                },
                            );
                        }
                        None => {
                            super::util::push_diagnostic(
                                repo_state,
                                DiagnosticKind::Error,
                                "Could not resolve a start point (parent tip or HEAD) to create the branch"
                                    .to_string(),
                            );
                        }
                    }
                    effects
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::ReorderStack {
            repo_id,
            ordered_names,
        } => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                if matches!(repo_state.stacks, Loadable::Ready(_)) {
                    let mut metadata = current_metadata(repo_state);
                    for (index, branch_name) in ordered_names.iter().enumerate() {
                        if let Some(branch) = metadata
                            .branches
                            .iter_mut()
                            .find(|b| &b.name == branch_name)
                        {
                            branch.order = index;
                        }
                    }
                    apply_and_persist(repo_state, repo_id, metadata)
                } else {
                    super::util::push_diagnostic(
                        repo_state,
                        DiagnosticKind::Error,
                        "Cannot reorder a stack that has not loaded yet".to_string(),
                    );
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::DeleteStackBranch { repo_id, name } => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                if matches!(repo_state.stacks, Loadable::Ready(_)) {
                    remove_branch_from_stack(repo_state, repo_id, &name)
                } else {
                    super::util::push_diagnostic(
                        repo_state,
                        DiagnosticKind::Error,
                        "Cannot delete from a stack that has not loaded yet".to_string(),
                    );
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::RestackStack {
            repo_id,
            base_branch,
        } => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                if matches!(repo_state.stacks, Loadable::Ready(_)) {
                    let metadata = current_metadata(repo_state);
                    let steps = metadata
                        .ordered()
                        .into_iter()
                        .map(|b| StackRestackStep {
                            branch: b.name.clone(),
                            parent: b.parent.clone(),
                        })
                        .collect::<Vec<_>>();
                    let plan = StackRestackPlan { base_branch, steps };
                    vec![Effect::RestackStack { repo_id, plan }]
                } else {
                    super::util::push_diagnostic(
                        repo_state,
                        DiagnosticKind::Error,
                        "Cannot restack a stack that has not loaded yet".to_string(),
                    );
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        Msg::Internal(InternalMsg::RestackStackDone { repo_id, result }) => {
            let effects = if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                match result {
                    Ok(outcome) => {
                        let summary = if outcome.rebased.is_empty() {
                            "Stack is already up to date".to_string()
                        } else {
                            format!("Restacked {} branch(es)", outcome.rebased.len())
                        };
                        super::util::push_diagnostic(repo_state, DiagnosticKind::Info, summary);
                        vec![Effect::LoadBranches { repo_id }]
                    }
                    Err(error) => {
                        super::util::push_diagnostic(
                            repo_state,
                            DiagnosticKind::Error,
                            format!("Restack failed: {error}"),
                        );
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            ReduceOutcome::Handled(effects)
        }
        other => ReduceOutcome::NotHandled(other),
    }
}

/// Store the (mutated) metadata on `repo_state` and emit the persist effect.
fn apply_and_persist(
    repo_state: &mut RepoState,
    repo_id: RepoId,
    metadata: StackMetadata,
) -> Vec<Effect> {
    repo_state.set_stacks(Loadable::Ready(metadata.clone()));
    vec![Effect::PersistStackMetadata { repo_id, metadata }]
}

/// Removes `name` from the repo's loaded stack metadata, reparenting any
/// children onto the removed branch's parent so the stack stays contiguous.
///
/// Returns the persist effect when the branch was actually part of the stack,
/// and nothing when it was not (or when no stack is loaded) — a no-op delete
/// must not rewrite the metadata file.
///
/// This is also what keeps the stack consistent when a git branch is deleted
/// through the ordinary delete flow: a stale entry would otherwise leave
/// `restack`/`reorder` pointing at a branch that no longer resolves.
pub(super) fn remove_branch_from_stack(
    repo_state: &mut RepoState,
    repo_id: RepoId,
    name: &str,
) -> Vec<Effect> {
    let mut metadata = current_metadata(repo_state);
    if metadata.by_name(name).is_none() {
        return Vec::new();
    }
    let deleted_parent = metadata.by_name(name).and_then(|b| b.parent.clone());
    for branch in metadata.branches.iter_mut() {
        if branch.parent.as_deref() == Some(name) {
            branch.parent = deleted_parent.clone();
        }
    }
    metadata.branches.retain(|b| b.name != name);
    apply_and_persist(repo_state, repo_id, metadata)
}

/// Read the currently-loaded stack metadata, or an empty one when nothing has
/// loaded yet (so a create before the first load still records the branch).
fn current_metadata(repo_state: &RepoState) -> StackMetadata {
    match &repo_state.stacks {
        Loadable::Ready(shared) => (**shared).clone(),
        _ => StackMetadata::new(),
    }
}

/// Resolve a branch's tip commit from the loaded git branch list, used as the
/// start point when creating a child stacked branch. Returns `None` when the
/// branch list has not loaded yet (callers fall back to HEAD).
fn branch_tip(repo_state: &RepoState, name: &str) -> Option<CommitId> {
    match &repo_state.branches {
        Loadable::Ready(branches) => branches
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.target.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::reduce_stacked_pr;
    use crate::model::{AppState, Loadable, RepoId, RepoState};
    use crate::msg::{Effect, InternalMsg, Msg};
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{Branch, CommitId, RepoSpec, StackBranch, StackMetadata};

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

    fn with_loaded_stack(state: &mut AppState, repo_id: RepoId, meta: StackMetadata) {
        let idx = state.repos.iter().position(|r| r.id == repo_id).unwrap();
        state.repos[idx].stacks = Loadable::Ready(Arc::new(meta));
    }

    fn meta_of(state: &AppState, repo_id: RepoId) -> Option<StackMetadata> {
        let repo = state.repos.iter().find(|r| r.id == repo_id).unwrap();
        match &repo.stacks {
            Loadable::Ready(shared) => Some((**shared).clone()),
            _ => None,
        }
    }

    #[test]
    fn create_stacked_branch_records_branch_and_persists() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        with_loaded_stack(&mut state, repo_id, StackMetadata::new());

        let outcome = reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/a".to_string(),
                parent: None,
            },
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                // In this bare setup no start point resolves (no branches/HEAD
                // loaded), so only the stack relationship is persisted. The
                // CreateBranch effect is covered by the resolved-target tests.
                assert_eq!(effects.len(), 1);
                assert!(effects.iter().any(|e| matches!(
                    e,
                    Effect::PersistStackMetadata { repo_id: id, .. } if *id == repo_id
                )));
            }
            super::ReduceOutcome::NotHandled(_) => panic!("CreateStackedBranch should be Handled"),
        }
        let meta = meta_of(&state, repo_id).expect("stack should be ready");
        assert_eq!(meta.branches.len(), 1);
        assert_eq!(meta.branches[0].name, "feature/a");
        assert_eq!(meta.branches[0].order, 0);
        assert!(meta.branches[0].parent.is_none());
    }

    #[test]
    fn create_child_inherits_parent_depth() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("base", None, 0));
        with_loaded_stack(&mut state, repo_id, meta);

        reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/a".to_string(),
                parent: Some("base".to_string()),
            },
            &mut state,
        );
        let meta = meta_of(&state, repo_id).unwrap();
        let child = meta.by_name("feature/a").unwrap();
        assert_eq!(child.order, 1);
        assert_eq!(child.parent.as_deref(), Some("base"));
    }

    #[test]
    fn create_stacked_branch_emits_create_branch_effect() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        // HEAD resolves to the "main" branch tip, so a root stacked branch has
        // a concrete start point.
        {
            let repo = state.repos.iter_mut().find(|r| r.id == repo_id).unwrap();
            repo.head_branch = Loadable::Ready("main".to_string());
            repo.branches = Loadable::Ready(Arc::new(vec![Branch {
                name: "main".to_string(),
                target: CommitId(Arc::from("deadbeef")),
                upstream: None,
                divergence: None,
            }]));
        }
        with_loaded_stack(&mut state, repo_id, StackMetadata::new());

        let outcome = reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/a".to_string(),
                parent: None,
            },
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 2);
                let create = effects
                    .iter()
                    .find(|e| matches!(e, Effect::CreateBranch { .. }))
                    .expect("CreateBranch effect should be emitted");
                if let Effect::CreateBranch { target, .. } = create {
                    assert_eq!(target, "deadbeef");
                }
            }
            super::ReduceOutcome::NotHandled(_) => panic!("CreateStackedBranch should be Handled"),
        }
    }

    #[test]
    fn create_child_stacked_branch_starts_at_parent_tip() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        {
            let repo = state.repos.iter_mut().find(|r| r.id == repo_id).unwrap();
            repo.branches = Loadable::Ready(Arc::new(vec![
                Branch {
                    name: "base".to_string(),
                    target: CommitId(Arc::from("aaa111")),
                    upstream: None,
                    divergence: None,
                },
                Branch {
                    name: "feature/a".to_string(),
                    target: CommitId(Arc::from("bbb222")),
                    upstream: None,
                    divergence: None,
                },
            ]));
        }
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("base", None, 0));
        with_loaded_stack(&mut state, repo_id, meta);

        let outcome = reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/b".to_string(),
                parent: Some("feature/a".to_string()),
            },
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                let create = effects
                    .iter()
                    .find(|e| matches!(e, Effect::CreateBranch { .. }))
                    .expect("CreateBranch effect should be emitted");
                if let Effect::CreateBranch { name, target, .. } = create {
                    assert_eq!(name, "feature/b");
                    assert_eq!(target, "bbb222");
                }
            }
            super::ReduceOutcome::NotHandled(_) => panic!("CreateStackedBranch should be Handled"),
        }
    }

    #[test]
    fn duplicate_create_is_rejected() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        with_loaded_stack(&mut state, repo_id, StackMetadata::new());

        reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/a".to_string(),
                parent: None,
            },
            &mut state,
        );
        let outcome = reduce_stacked_pr(
            Msg::CreateStackedBranch {
                repo_id,
                name: "feature/a".to_string(),
                parent: None,
            },
            &mut state,
        );
        // Second create is rejected: no persist effect, no new branch.
        match outcome {
            super::ReduceOutcome::Handled(effects) => assert!(effects.is_empty()),
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
        assert_eq!(meta_of(&state, repo_id).unwrap().branches.len(), 1);
    }

    #[test]
    fn reorder_stack_reassigns_orders() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("a", None, 0));
        meta.branches
            .push(StackBranch::new("b", Some("a".to_string()), 1));
        meta.branches
            .push(StackBranch::new("c", Some("b".to_string()), 2));
        with_loaded_stack(&mut state, repo_id, meta);

        reduce_stacked_pr(
            Msg::ReorderStack {
                repo_id,
                ordered_names: vec!["c".to_string(), "a".to_string(), "b".to_string()],
            },
            &mut state,
        );
        let meta = meta_of(&state, repo_id).unwrap();
        assert_eq!(meta.by_name("c").unwrap().order, 0);
        assert_eq!(meta.by_name("a").unwrap().order, 1);
        assert_eq!(meta.by_name("b").unwrap().order, 2);
        // Parent links are preserved by a reorder.
        assert_eq!(meta.by_name("b").unwrap().parent.as_deref(), Some("a"));
        assert_eq!(meta.by_name("c").unwrap().parent.as_deref(), Some("b"));
    }

    #[test]
    fn delete_stack_branch_reparents_children() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("a", None, 0));
        meta.branches
            .push(StackBranch::new("b", Some("a".to_string()), 1));
        meta.branches
            .push(StackBranch::new("c", Some("b".to_string()), 2));
        with_loaded_stack(&mut state, repo_id, meta);

        reduce_stacked_pr(
            Msg::DeleteStackBranch {
                repo_id,
                name: "b".to_string(),
            },
            &mut state,
        );
        let meta = meta_of(&state, repo_id).unwrap();
        assert!(meta.by_name("b").is_none());
        // c (formerly child of b) is reparented to b's parent a.
        assert_eq!(meta.by_name("c").unwrap().parent.as_deref(), Some("a"));
        // a is untouched.
        assert!(meta.by_name("a").is_some());
    }

    #[test]
    fn stack_metadata_loaded_stores_metadata() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("a", None, 0));

        let outcome = reduce_stacked_pr(
            Msg::Internal(InternalMsg::StackMetadataLoaded {
                repo_id,
                result: Ok(meta),
            }),
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => assert!(effects.is_empty()),
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
        assert_eq!(meta_of(&state, repo_id).unwrap().branches.len(), 1);
    }

    #[test]
    fn stack_metadata_loaded_error_sets_error_state() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_stacked_pr(
            Msg::Internal(InternalMsg::StackMetadataLoaded {
                repo_id,
                result: Err(worktree_core::error::Error::new(
                    worktree_core::error::ErrorKind::Backend("boom".to_string()),
                )),
            }),
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => assert!(effects.is_empty()),
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
        assert!(matches!(state.repos[0].stacks, Loadable::Error(_)));
    }

    #[test]
    fn load_stack_metadata_sets_loading_and_emits_effect() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_stacked_pr(Msg::LoadStackMetadata { repo_id }, &mut state);
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::LoadStackMetadata { repo_id: id } if id == repo_id
                ));
            }
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
        assert!(matches!(state.repos[0].stacks, Loadable::Loading));
    }

    #[test]
    fn restack_stack_emits_ordered_plan() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let mut meta = StackMetadata::new();
        meta.branches.push(StackBranch::new("a", None, 0));
        meta.branches
            .push(StackBranch::new("b", Some("a".to_string()), 1));
        meta.branches
            .push(StackBranch::new("c", Some("b".to_string()), 2));
        with_loaded_stack(&mut state, repo_id, meta);

        let outcome = reduce_stacked_pr(
            Msg::RestackStack {
                repo_id,
                base_branch: None,
            },
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                if let Effect::RestackStack { repo_id: id, plan } = &effects[0] {
                    assert_eq!(*id, repo_id);
                    assert_eq!(plan.base_branch, None);
                    // Ordered base-first: a (root), b (child of a), c (child of b).
                    assert_eq!(plan.steps.len(), 3);
                    assert_eq!(plan.steps[0].branch, "a");
                    assert_eq!(plan.steps[0].parent, None);
                    assert_eq!(plan.steps[1].branch, "b");
                    assert_eq!(plan.steps[1].parent.as_deref(), Some("a"));
                    assert_eq!(plan.steps[2].branch, "c");
                    assert_eq!(plan.steps[2].parent.as_deref(), Some("b"));
                } else {
                    panic!("expected Effect::RestackStack");
                }
            }
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
    }

    #[test]
    fn restack_stack_before_load_is_noop() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        let outcome = reduce_stacked_pr(
            Msg::RestackStack {
                repo_id,
                base_branch: None,
            },
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => assert!(effects.is_empty()),
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
    }

    #[test]
    fn restack_stack_done_success_reloads_branches() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        with_loaded_stack(&mut state, repo_id, StackMetadata::new());
        let outcome = reduce_stacked_pr(
            Msg::Internal(InternalMsg::RestackStackDone {
                repo_id,
                result: Ok(worktree_core::domain::StackRestackOutcome {
                    rebased: vec!["b".to_string()],
                }),
            }),
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => {
                assert_eq!(effects.len(), 1);
                assert!(matches!(
                    effects[0],
                    Effect::LoadBranches { repo_id: id } if id == repo_id
                ));
            }
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
    }

    #[test]
    fn restack_stack_done_error_pushes_diagnostic() {
        let repo_id = RepoId(1);
        let mut state = state_with_repo(repo_id);
        with_loaded_stack(&mut state, repo_id, StackMetadata::new());
        let outcome = reduce_stacked_pr(
            Msg::Internal(InternalMsg::RestackStackDone {
                repo_id,
                result: Err(worktree_core::error::Error::new(
                    worktree_core::error::ErrorKind::Backend("conflict at b".to_string()),
                )),
            }),
            &mut state,
        );
        match outcome {
            super::ReduceOutcome::Handled(effects) => assert!(effects.is_empty()),
            super::ReduceOutcome::NotHandled(_) => panic!("should be Handled"),
        }
    }
}
