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
    pub fn from_repo_command_kind(kind: &RepoCommandKind) -> Self {
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

            // Tag set changes.
            CreateTag { .. } | DeleteTag { .. } | PruneLocalTags => RepoChange::TagsChanged,

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
