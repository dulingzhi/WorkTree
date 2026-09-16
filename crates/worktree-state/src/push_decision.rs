//! One place that answers "what should pushing do for this repo right now?".
//!
//! Every push entry point used to re-derive this policy inline — detecting a
//! missing upstream and picking which remote to publish to — which is how the
//! toolbar drifted away from the commit panel's post-commit push. Keeping the
//! decision here makes it shared and testable, and leaves the views to decide
//! only how to *present* the outcome.

use crate::model::{Loadable, RepoState};

/// What the app should do when the user asks to push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushDecision {
    /// HEAD already has an upstream, or we cannot tell: run the push.
    Push,
    /// HEAD has no upstream yet — publish it first, preferring `remote`.
    NeedsUpstream { remote: Option<String> },
    /// No remotes are configured, so there is nowhere to push.
    NoRemotes,
}

/// Decide what a push request turns into for `repo`.
pub fn push_decision(repo: &RepoState) -> PushDecision {
    // Without HEAD we cannot tell whether it has an upstream; pushing is the
    // safer bet and is what the toolbar has always done in that case.
    let Loadable::Ready(head) = &repo.head_branch else {
        return PushDecision::Push;
    };

    let upstream_missing = match &repo.branches {
        Loadable::Ready(branches) => branches
            .iter()
            .find(|branch| branch.name == *head)
            .is_some_and(|branch| branch.upstream.is_none()),
        // Branch list still loading: pushing beats nagging the user to set an
        // upstream that may well already exist.
        _ => false,
    };

    if !upstream_missing {
        return PushDecision::Push;
    }

    match &repo.remotes {
        Loadable::Ready(remotes) if remotes.is_empty() => PushDecision::NoRemotes,
        Loadable::Ready(remotes) => PushDecision::NeedsUpstream {
            remote: remotes
                .iter()
                .find(|remote| remote.name == "origin")
                .or_else(|| remotes.first())
                .map(|remote| remote.name.clone()),
        },
        // Remotes not loaded yet: assume the conventional name rather than
        // blocking the flow — the prompt still lets the user change it.
        _ => PushDecision::NeedsUpstream {
            remote: Some("origin".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RepoId;
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{Branch, CommitId, Remote, RepoSpec, Upstream};

    fn repo(
        head: Option<&str>,
        branches: Option<Vec<Branch>>,
        remotes: Option<Vec<Remote>>,
    ) -> RepoState {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        );
        repo.head_branch = match head {
            Some(name) => Loadable::Ready(name.to_string()),
            None => Loadable::NotLoaded,
        };
        repo.branches = match branches {
            Some(list) => Loadable::Ready(Arc::new(list)),
            None => Loadable::NotLoaded,
        };
        repo.remotes = match remotes {
            Some(list) => Loadable::Ready(Arc::new(list)),
            None => Loadable::NotLoaded,
        };
        repo
    }

    fn branch(name: &str, upstream_remote: Option<&str>) -> Branch {
        Branch {
            name: name.to_string(),
            target: CommitId(Arc::from("0000000")),
            upstream: upstream_remote.map(|remote| Upstream {
                remote: remote.to_string(),
                branch: name.to_string(),
            }),
            divergence: None,
        }
    }

    fn remote(name: &str) -> Remote {
        Remote {
            name: name.to_string(),
            url: None,
        }
    }

    #[test]
    fn pushes_when_head_has_an_upstream() {
        let repo = repo(
            Some("main"),
            Some(vec![branch("main", Some("origin"))]),
            Some(vec![remote("origin")]),
        );
        assert_eq!(push_decision(&repo), PushDecision::Push);
    }

    #[test]
    fn pushes_while_the_branch_list_is_still_loading() {
        let repo = repo(Some("main"), None, Some(vec![remote("origin")]));
        assert_eq!(push_decision(&repo), PushDecision::Push);
    }

    #[test]
    fn pushes_when_head_is_unknown() {
        let repo = repo(
            None,
            Some(vec![branch("main", None)]),
            Some(vec![remote("origin")]),
        );
        assert_eq!(push_decision(&repo), PushDecision::Push);
    }

    #[test]
    fn asks_for_upstream_preferring_origin() {
        let repo = repo(
            Some("main"),
            Some(vec![branch("main", None)]),
            Some(vec![remote("upstream"), remote("origin")]),
        );
        assert_eq!(
            push_decision(&repo),
            PushDecision::NeedsUpstream {
                remote: Some("origin".to_string())
            }
        );
    }

    #[test]
    fn asks_for_upstream_falling_back_to_the_only_remote() {
        let repo = repo(
            Some("main"),
            Some(vec![branch("main", None)]),
            Some(vec![remote("gitlab")]),
        );
        assert_eq!(
            push_decision(&repo),
            PushDecision::NeedsUpstream {
                remote: Some("gitlab".to_string())
            }
        );
    }

    #[test]
    fn reports_no_remotes_when_the_list_is_empty() {
        let repo = repo(Some("main"), Some(vec![branch("main", None)]), Some(vec![]));
        assert_eq!(push_decision(&repo), PushDecision::NoRemotes);
    }

    #[test]
    fn assumes_origin_while_remotes_load() {
        let repo = repo(Some("main"), Some(vec![branch("main", None)]), None);
        assert_eq!(
            push_decision(&repo),
            PushDecision::NeedsUpstream {
                remote: Some("origin".to_string())
            }
        );
    }
}
