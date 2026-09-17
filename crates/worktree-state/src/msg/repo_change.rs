//! Semantic description of "what changed in a repository" — the single
//! vocabulary the reducer turns into panel-refresh effects.
//!
//! See `docs/superpowers/plans/2026-09-16-repo-change-unified-refresh.md`.
//! The two refresh paths (`repo_command_finished` for user commands,
//! `repo_externally_changed` for file-system events) each translate their raw
//! trigger into a `RepoChange`; `dispatch_repo_change` is then the only place
//! that decides which panels to refresh, so the two paths can no longer drift.

use super::{RepoActionKind, RepoCommandKind, RepoExternalChange};

/// A semantic change to a repository, independent of how it was triggered
/// (a user command, a local action, or an external file-system event).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepoChange {
    /// Nothing changed.
    ///
    /// Used for a command that failed without leaving any state behind — a tag
    /// command that was rejected, say. Dispatching it emits no refresh at all,
    /// which is the point: translating the command kind alone would schedule a
    /// refresh the failure never warranted, and visibly undo state the user is
    /// still looking at (a failed "create tag" used to blank the tag list).
    ///
    /// Deliberately rare. Most commands refresh even when they fail, because a
    /// failure can leave real state behind — a rebase that stops on a conflict
    /// still rewrote HEAD and opened a sequencer session, so its failure must
    /// refresh. Only map a failing command here when its failure provably left
    /// nothing to re-read.
    None,
    /// A commit was created or rewritten (HEAD advanced with new history).
    Committed,
    /// Branches / tags / remotes changed, including refs brought in by a
    /// fetch / pull / push.
    RefsChanged,
    /// HEAD moved: checkout, detached HEAD, branch switch.
    HeadMoved,
    /// The index changed: stage / unstage / restore --staged.
    IndexChanged,
    /// Working-tree files changed (add / modify / delete / untracked).
    WorktreeChanged,
    /// The tag set changed.
    TagsChanged,
    /// The local branch set changed (added / removed / renamed).
    BranchesChanged,
    /// Status (staged + unstaged) changed.
    StatusChanged,
    /// A rescan / forced full refresh.
    Anything,
}

impl RepoChange {
    /// Translate a completed repo command into the semantic change it produced.
    /// Translate a finished repo command into the semantic change it produced.
    ///
    /// `succeeded` is the command's own result. It only matters for commands
    /// whose failure provably leaves nothing to re-read — currently tag CRUD,
    /// which is why a rejected `CreateTag` maps to [`RepoChange::None`] instead
    /// of clearing the tag list. Every other arm ignores it on purpose: a
    /// failed rebase / merge / cherry-pick still moved HEAD or left a sequencer
    /// session behind, so those must refresh either way.
    pub fn from_repo_command_kind(kind: &RepoCommandKind, succeeded: bool) -> Self {
        use RepoCommandKind::*;
        match kind {
            // Ref-affecting network / branch operations.
            FetchAll
            | AutoFetchAll
            | Pull { .. }
            | PullBranch { .. }
            | Push
            | PushAfterCommit { .. }
            | ForcePush
            | ForcePushWithLease { .. }
            | PushMergeRequest { .. }
            | PushSetUpstream { .. }
            | SetUpstreamBranch { .. }
            | UnsetUpstreamBranch { .. }
            | FastForwardBranch { .. }
            | DeleteRemoteBranch { .. }
            | DeleteRemoteBranches { .. }
            | PushTag { .. }
            | DeleteRemoteTag { .. }
            | AddRemote { .. }
            | RemoveRemote { .. }
            | SetRemoteUrl { .. }
            | SetRemoteSshKey { .. } => RepoChange::RefsChanged,

            // Tag set changes. A rejected tag command changed nothing — in
            // particular it must not clear the list the user is looking at, so
            // the failure is a no-op rather than a TagsChanged refresh.
            CreateTag { .. } | DeleteTag { .. } | PruneLocalTags => {
                if succeeded {
                    RepoChange::TagsChanged
                } else {
                    RepoChange::None
                }
            }

            // HEAD / history rewrites.
            MergeRef { .. }
            | SquashRef { .. }
            | Reset { .. }
            | SquashCommits { .. }
            | Rebase { .. }
            | RebaseContinue
            | RebaseAbort
            | BisectStart { .. }
            | BisectMark { .. }
            | BisectReset
            | InteractiveRebase { .. }
            | InteractiveCherryPick { .. }
            | CherryPick { .. }
            | MergeAbort => RepoChange::HeadMoved,

            PruneMergedBranches => RepoChange::BranchesChanged,

            // Conflict-resolution tooling: rescan the affected views.
            CheckoutConflict { .. }
            | AcceptConflictDeletion { .. }
            | CheckoutConflictBase { .. }
            | LaunchMergetool { .. } => RepoChange::Anything,

            // Index changes.
            StageHunk | UnstageHunk | ApplyWorktreePatch { .. } => RepoChange::IndexChanged,

            // Working-tree content changes.
            SaveWorktreeFile { .. } | AppendGitignorePatterns { .. } => RepoChange::WorktreeChanged,

            // Linked-worktree add/remove.
            AddWorktree { .. } | RemoveWorktree { .. } | ForceRemoveWorktree { .. } => {
                RepoChange::WorktreeChanged
            }

            // Submodule pointer / set changes: covered by a full refresh.
            AddSubmodule { .. }
            | UpdateSubmodules { .. }
            | LoadSubmodule { .. }
            | ChangeSubmodulePointer { .. }
            | RemoveSubmodule { .. } => RepoChange::Anything,

            // Export / archive / gc / patch application: full rescan.
            ExportPatch { .. } | ArchiveZip { .. } | Cleanup | ApplyPatch { .. } => {
                RepoChange::Anything
            }
        }
    }

    /// Translate the four-lane external change into a semantic change.
    ///
    /// `git_state` is the "something structural changed" lane and is the
    /// broadest signal, so it wins when several lanes fire at once. `tags` is
    /// independent of `git_state` in the watcher, but a tag-only event is a
    /// `TagsChanged`; an index-only event is `IndexChanged`; a worktree-only
    /// event is `WorktreeChanged`.
    pub fn from_repo_external_change(change: &RepoExternalChange) -> Self {
        if change.git_state {
            RepoChange::HeadMoved
        } else if change.tags {
            RepoChange::TagsChanged
        } else if change.index {
            RepoChange::IndexChanged
        } else if change.worktree {
            RepoChange::WorktreeChanged
        } else {
            RepoChange::Anything
        }
    }

    /// Translate a completed local action into the semantic change it produced.
    pub fn from_repo_action_kind(kind: &RepoActionKind) -> Self {
        use RepoActionKind::*;
        match kind {
            CheckoutBranch
            | CheckoutRemoteBranch
            | CheckoutPullRequest
            | CheckoutCommit
            | CherryPickCommit
            | RevertCommit
            | CreateBranch
            | CreateBranchAndCheckout
            | RenameBranch
            | DeleteBranch
            | ForceDeleteBranch
            | DeleteBranches => RepoChange::HeadMoved,

            StagePath | StagePaths | UnstagePath | UnstagePaths => RepoChange::IndexChanged,

            DiscardWorktreeChangesPath { .. } | DiscardWorktreeChangesPaths { .. } => {
                RepoChange::WorktreeChanged
            }

            Stash | ApplyStash | PopStash | DropStash | StashBranch | SetAssumeUnchanged => {
                RepoChange::Anything
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_tag() -> RepoCommandKind {
        RepoCommandKind::CreateTag {
            name: "v2.0.0".to_string(),
            target: "HEAD".to_string(),
            message: None,
            annotated: false,
        }
    }

    /// A rejected tag command changed nothing, so it must not schedule a tag
    /// refresh — that would blank the list the user is still looking at. Only
    /// the outcome decides; the command kind alone cannot tell.
    #[test]
    fn a_failed_tag_command_requests_no_refresh() {
        assert_eq!(
            RepoChange::from_repo_command_kind(&create_tag(), true),
            RepoChange::TagsChanged
        );
        assert_eq!(
            RepoChange::from_repo_command_kind(&create_tag(), false),
            RepoChange::None
        );
        assert_eq!(
            RepoChange::from_repo_command_kind(
                &RepoCommandKind::DeleteTag {
                    name: "v2.0.0".to_string(),
                },
                false,
            ),
            RepoChange::None
        );
        assert_eq!(
            RepoChange::from_repo_command_kind(&RepoCommandKind::PruneLocalTags, false),
            RepoChange::None
        );
    }

    /// Most commands must refresh even when they fail: a rebase that stops on a
    /// conflict has already moved HEAD and left a sequencer session behind, and
    /// the panels stay stale until they re-read it. Pinned so the tag-CRUD
    /// special case above is never generalised into "failures never refresh".
    #[test]
    fn a_failed_history_command_still_requests_a_refresh() {
        for kind in [
            RepoCommandKind::Rebase {
                onto: "origin/main".to_string(),
            },
            RepoCommandKind::RebaseContinue,
            RepoCommandKind::CherryPick {
                commit_id: worktree_core::domain::CommitId("abc".into()),
                commit: true,
                mainline: None,
                summary: "s".to_string(),
            },
        ] {
            assert_eq!(
                RepoChange::from_repo_command_kind(&kind, false),
                RepoChange::HeadMoved,
                "{kind:?} must refresh even when it fails"
            );
        }
    }

    /// A failing command that only ever touched refs keeps its refresh: a
    /// half-finished fetch may well have moved some.
    #[test]
    fn a_failed_ref_command_keeps_its_refresh() {
        assert_eq!(
            RepoChange::from_repo_command_kind(&RepoCommandKind::FetchAll, false),
            RepoChange::RefsChanged
        );
    }
}
