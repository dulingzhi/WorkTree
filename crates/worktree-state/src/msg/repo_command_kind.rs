use std::path::PathBuf;
use worktree_core::domain::CommitId;
use worktree_core::external_merge_tool::ExternalMergeToolSelection;
use worktree_core::services::{
    BisectVerdict, ConflictSide, ForcePushLease, InteractiveRebaseEntry, MergeRequestPushOptions,
    PullMode, RemoteUrlKind, ResetMode, SafePushAfterCommitTarget, SubmoduleTrustTarget,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepoCommandKind {
    FetchAll,
    /// `git fetch --all` started by repository activation, not by the user.
    /// Reported through the same pipeline but quietly: no success toast and no
    /// failure banner, so an offline machine does not nag on every activation.
    AutoFetchAll,
    PruneMergedBranches,
    PruneLocalTags,
    Pull {
        mode: PullMode,
    },
    PullBranch {
        remote: String,
        branch: String,
    },
    MergeRef {
        reference: String,
    },
    SquashRef {
        reference: String,
    },
    Push,
    PushAfterCommit {
        target: SafePushAfterCommitTarget,
        set_upstream: bool,
    },
    ForcePush,
    ForcePushWithLease {
        lease: ForcePushLease,
    },
    /// Push HEAD with `git push -o merge_request.*` options so GitLab opens
    /// the merge request from the push itself.
    PushMergeRequest {
        options: MergeRequestPushOptions,
    },
    PushSetUpstream {
        remote: String,
        branch: String,
    },
    SetUpstreamBranch {
        branch: String,
        upstream: String,
    },
    UnsetUpstreamBranch {
        branch: String,
    },
    FastForwardBranch {
        branch: String,
    },
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    DeleteRemoteBranches {
        remote: String,
        branches: Vec<String>,
    },
    Reset {
        mode: ResetMode,
        target: String,
    },
    SquashCommits {
        oldest: CommitId,
        expected_head: CommitId,
        message: String,
        count: usize,
    },
    Rebase {
        onto: String,
    },
    RebaseContinue,
    RebaseAbort,
    BisectStart {
        bad: Option<String>,
        goods: Vec<String>,
    },
    BisectMark {
        verdict: BisectVerdict,
        commit: Option<String>,
    },
    BisectReset,
    InteractiveRebase {
        base: String,
        /// True when the interactive-rebase editor was opened by the user;
        /// false for automated todo-list rebases (e.g. squashing history that
        /// doesn't include HEAD), which report as a plain "Rebase".
        interactive: bool,
    },
    InteractiveCherryPick {
        entries: Vec<InteractiveRebaseEntry>,
    },
    CherryPick {
        commit_id: CommitId,
        commit: bool,
        /// Git's 1-based mainline parent for a single merge commit.
        mainline: Option<usize>,
        summary: String,
    },
    MergeAbort,
    CreateTag {
        name: String,
        target: String,
        message: Option<String>,
        annotated: bool,
    },
    DeleteTag {
        name: String,
    },
    PushTag {
        remote: String,
        name: String,
    },
    DeleteRemoteTag {
        remote: String,
        name: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    RemoveRemote {
        name: String,
    },
    SetRemoteUrl {
        name: String,
        url: String,
        kind: RemoteUrlKind,
    },
    SetRemoteSshKey {
        remote: String,
        key: Option<String>,
    },
    CheckoutConflict {
        path: PathBuf,
        side: ConflictSide,
    },
    AcceptConflictDeletion {
        path: PathBuf,
    },
    CheckoutConflictBase {
        path: PathBuf,
    },
    LaunchMergetool {
        path: PathBuf,
        preference: ExternalMergeToolSelection,
    },
    SaveWorktreeFile {
        path: PathBuf,
        stage: bool,
    },
    AppendGitignorePatterns {
        patterns: Vec<String>,
    },
    ExportPatch {
        commit_id: CommitId,
        dest: PathBuf,
    },
    ArchiveZip {
        revision: String,
        dest: PathBuf,
    },
    /// `git gc` followed by `git lfs prune` when LFS is enabled.
    Cleanup,
    ApplyPatch {
        patch: PathBuf,
    },
    AddWorktree {
        path: PathBuf,
        reference: Option<String>,
    },
    RemoveWorktree {
        path: PathBuf,
    },
    ForceRemoveWorktree {
        path: PathBuf,
    },
    AddSubmodule {
        url: String,
        path: PathBuf,
        branch: Option<String>,
        name: Option<String>,
        force: bool,
        approved_sources: Vec<SubmoduleTrustTarget>,
    },
    UpdateSubmodules {
        approved_sources: Vec<SubmoduleTrustTarget>,
    },
    LoadSubmodule {
        path: PathBuf,
        approved_sources: Vec<SubmoduleTrustTarget>,
    },
    ChangeSubmodulePointer {
        path: PathBuf,
        reference: String,
    },
    RemoveSubmodule {
        path: PathBuf,
    },
    StageHunk,
    UnstageHunk,
    ApplyWorktreePatch {
        reverse: bool,
    },
}
