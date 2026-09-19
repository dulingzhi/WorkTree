use crate::model::{ConflictFileLoadMode, RepoId};
use std::path::PathBuf;
use worktree_core::auth::StagedGitAuth;
use worktree_core::domain::*;
use worktree_core::external_merge_tool::ExternalMergeToolSelection;
use worktree_core::services::{
    BisectVerdict, ConflictSide, ForcePushLease, InteractiveRebaseEntry, MergeRequestPushOptions,
    PullMode, RemoteUrlKind, ResetMode, SafePushAfterCommitContext, SafePushAfterCommitTarget,
    SubmoduleTrustTarget,
};

use super::RepoPathList;

#[derive(Clone, Debug)]
pub enum Effect {
    PersistSession {
        repo_id: Option<RepoId>,
        action: &'static str,
    },
    PersistRecentRepo {
        repo_id: Option<RepoId>,
        workdir: PathBuf,
        action: &'static str,
    },
    PersistRepoHistoryMode {
        repo_id: Option<RepoId>,
        workdir: PathBuf,
        mode: HistoryMode,
        action: &'static str,
    },
    PersistRepoHistoryModesBatch {
        repo_id: Option<RepoId>,
        updates: Vec<(PathBuf, HistoryMode)>,
        action: &'static str,
    },
    PersistRepoHistoryAuthorFilter {
        repo_id: Option<RepoId>,
        workdir: PathBuf,
        author: Option<String>,
        action: &'static str,
    },
    PersistRepoHistoryRefFilters {
        repo_id: Option<RepoId>,
        workdir: PathBuf,
        refs: Vec<String>,
        action: &'static str,
    },
    OpenRepo {
        repo_id: RepoId,
        path: PathBuf,
    },
    CancelRepoLoads {
        repo_id: RepoId,
        load_epoch: u64,
    },
    LoadBranches {
        repo_id: RepoId,
    },
    LoadRemotes {
        repo_id: RepoId,
    },
    LoadRemoteBranches {
        repo_id: RepoId,
    },
    LoadWorktreeStatus {
        repo_id: RepoId,
    },
    LoadStagedStatus {
        repo_id: RepoId,
    },
    LoadStatus {
        repo_id: RepoId,
    },
    /// Path-targeted status rescan for an externally changed path set —
    /// the incremental lane; falls back to a full scan on renames/errors.
    LoadStatusForPaths {
        repo_id: RepoId,
        paths: std::sync::Arc<[std::path::PathBuf]>,
    },
    LoadLfsImagePreview {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadHeadBranch {
        repo_id: RepoId,
    },
    LoadUpstreamDivergence {
        repo_id: RepoId,
    },
    LoadLog {
        repo_id: RepoId,
        /// Identifies this walk, so its replies can be told from those of a
        /// walk a newer request superseded. See [`crate::model::LogLoadSeq`].
        seq: crate::model::LogLoadSeq,
        scope: LogScope,
        /// Case-insensitive author filter, or `None` for all authors.
        author: Option<String>,
        /// Full ref names the walk is seeded from instead of HEAD / every
        /// ref; empty means no ref filter.
        refs: Vec<String>,
        limit: usize,
        cursor: Option<LogCursor>,
    },
    LoadTags {
        repo_id: RepoId,
    },
    LoadRemoteTags {
        repo_id: RepoId,
    },
    LoadStashes {
        repo_id: RepoId,
        limit: usize,
    },
    LoadReflog {
        repo_id: RepoId,
        limit: usize,
    },
    LoadRecentCommitMessages {
        repo_id: RepoId,
        limit: usize,
        request_rev: u64,
    },
    SearchCommits {
        repo_id: RepoId,
        query: String,
        request_rev: u64,
    },
    LoadAiCommitContext {
        repo_id: RepoId,
        request_rev: u64,
    },
    LoadFileHistory {
        repo_id: RepoId,
        path: PathBuf,
        limit: usize,
    },
    /// Author name → email map for per-email author avatars. Best-effort:
    /// failures land as `AuthorEmailsLoaded` with `Err` and the UI keeps
    /// the initials fallback.
    LoadAuthorEmails {
        repo_id: RepoId,
    },
    LoadBlame {
        repo_id: RepoId,
        path: PathBuf,
        source: worktree_core::domain::BlameSource,
    },
    LoadWorktrees {
        repo_id: RepoId,
    },
    LoadWorktreeDirty {
        repo_id: RepoId,
        workdir: PathBuf,
        /// Worktree whose changed-file lists the scan should carry back; every
        /// other worktree reports counts alone. `None` while no worktree row is
        /// selected. See [`worktree_core::domain::WorktreeDirtySummary`].
        files_for: Option<PathBuf>,
    },
    LoadRefMetadata {
        repo_id: RepoId,
    },
    LoadSubmodules {
        repo_id: RepoId,
    },
    LoadFileBrowser {
        repo_id: RepoId,
        source: FileSource,
    },
    LoadRebaseAndMergeState {
        repo_id: RepoId,
    },
    LoadRebaseState {
        repo_id: RepoId,
    },
    LoadBisectState {
        repo_id: RepoId,
    },
    LoadMergeCommitMessage {
        repo_id: RepoId,
    },
    LoadCommitDetails {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    LoadHoverCommitMessage {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    /// Resolve a possibly abbreviated commit reference and load its details in
    /// one call, so a reveal can show the commit before the log reaches it.
    ResolveCommitForReveal {
        repo_id: RepoId,
        reference: CommitId,
    },
    LoadRangeFiles {
        repo_id: RepoId,
        from: CommitId,
        /// `None` lists files between `from` and the working tree.
        to: Option<CommitId>,
        /// Echoed back on the reply so a completion that lost a race against a
        /// newer load can be dropped. See `HistoryState::range_files_request`.
        request: u64,
    },
    LoadSquashMessagePreview {
        repo_id: RepoId,
        oldest: CommitId,
        head: CommitId,
    },
    LoadSquashRebaseSetup {
        repo_id: RepoId,
        base: CommitId,
        /// The repo HEAD the plan was validated against. Re-checked once the
        /// live `base..HEAD` list loads, so a HEAD move during the async gap
        /// cancels the squash instead of rewriting an unintended range.
        actual_head: CommitId,
        selected_ids: Vec<CommitId>,
        reword_id: CommitId,
        message: String,
        count: usize,
    },
    OpenFileAtCommitParent {
        repo_id: RepoId,
        commit_id: CommitId,
        path: PathBuf,
    },
    OpenFileAtCommit {
        repo_id: RepoId,
        commit_id: CommitId,
        path: PathBuf,
    },
    LoadDiff {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadDirectoryDiff {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadRepoHooks {
        repo_id: RepoId,
    },
    SetRepoHookEnabled {
        repo_id: RepoId,
        name: RepoHookName,
        enabled: bool,
    },
    CreateRepoHook {
        repo_id: RepoId,
        name: RepoHookName,
        from_sample: bool,
    },
    DeleteRepoHook {
        repo_id: RepoId,
        name: RepoHookName,
    },
    LoadDiffFile {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadDiffPreviewTextFile {
        repo_id: RepoId,
        target: DiffTarget,
        side: DiffPreviewTextSide,
    },
    LoadSubmoduleSummary {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadInlineSubmoduleSelectedDiff {
        repo_id: RepoId,
        inline_rev: u64,
    },
    LoadInlineSubmoduleSelectedDiffFile {
        repo_id: RepoId,
        inline_rev: u64,
    },
    LoadInlineSubmoduleSelectedDiffFileImage {
        repo_id: RepoId,
        inline_rev: u64,
    },
    LoadDiffFileImage {
        repo_id: RepoId,
        target: DiffTarget,
    },
    LoadSelectedDiff {
        repo_id: RepoId,
        load_patch_diff: bool,
        load_file_text: bool,
        preview_text_side: Option<DiffPreviewTextSide>,
        load_submodule_summary: bool,
        load_file_image: bool,
    },
    LoadSelectedConflictFile {
        repo_id: RepoId,
        mode: ConflictFileLoadMode,
    },
    LoadConflictFile {
        repo_id: RepoId,
        path: PathBuf,
        mode: ConflictFileLoadMode,
    },
    SaveWorktreeFile {
        repo_id: RepoId,
        path: PathBuf,
        contents: String,
        stage: bool,
    },
    AppendGitignorePatterns {
        repo_id: RepoId,
        patterns: Vec<String>,
    },

    CheckoutBranch {
        repo_id: RepoId,
        name: String,
    },
    CheckoutRemoteBranch {
        repo_id: RepoId,
        remote: String,
        branch: String,
        local_branch: String,
    },
    CheckoutPullRequest {
        repo_id: RepoId,
        remote: String,
        number: u64,
    },
    CheckoutCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    CherryPickCommit {
        repo_id: RepoId,
        commit_id: CommitId,
        commit: bool,
        mainline: Option<usize>,
        summary: String,
    },
    RevertCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    CreateBranch {
        repo_id: RepoId,
        name: String,
        target: String,
    },
    CreateBranchAndCheckout {
        repo_id: RepoId,
        name: String,
        target: String,
    },
    RenameBranch {
        repo_id: RepoId,
        old_name: String,
        new_name: String,
    },
    DeleteBranch {
        repo_id: RepoId,
        name: String,
    },
    ForceDeleteBranch {
        repo_id: RepoId,
        name: String,
    },
    DeleteBranches {
        repo_id: RepoId,
        names: Vec<String>,
        force: bool,
    },
    CloneRepo {
        url: String,
        dest: PathBuf,
        ssh_key: Option<String>,
        auth: Option<StagedGitAuth>,
    },
    AbortCloneRepo {
        dest: PathBuf,
    },
    ExportPatch {
        repo_id: RepoId,
        commit_id: CommitId,
        dest: PathBuf,
    },
    ArchiveZip {
        repo_id: RepoId,
        revision: String,
        dest: PathBuf,
    },
    CleanupRepo {
        repo_id: RepoId,
    },
    ApplyPatch {
        repo_id: RepoId,
        patch: PathBuf,
    },
    AddWorktree {
        repo_id: RepoId,
        path: PathBuf,
        reference: Option<String>,
    },
    RemoveWorktree {
        repo_id: RepoId,
        path: PathBuf,
    },
    ForceRemoveWorktree {
        repo_id: RepoId,
        path: PathBuf,
    },
    CheckSubmoduleAddTrust {
        repo_id: RepoId,
        url: String,
        path: PathBuf,
        branch: Option<String>,
        name: Option<String>,
        force: bool,
    },
    CheckSubmoduleUpdateTrust {
        repo_id: RepoId,
    },
    AddSubmodule {
        repo_id: RepoId,
        url: String,
        path: PathBuf,
        branch: Option<String>,
        name: Option<String>,
        force: bool,
        approved_sources: Vec<SubmoduleTrustTarget>,
        auth: Option<StagedGitAuth>,
    },
    UpdateSubmodules {
        repo_id: RepoId,
        approved_sources: Vec<SubmoduleTrustTarget>,
        auth: Option<StagedGitAuth>,
    },
    CheckSubmoduleLoadTrust {
        repo_id: RepoId,
        path: PathBuf,
    },
    LoadSubmodule {
        repo_id: RepoId,
        path: PathBuf,
        approved_sources: Vec<SubmoduleTrustTarget>,
        auth: Option<StagedGitAuth>,
    },
    ChangeSubmodulePointer {
        repo_id: RepoId,
        path: PathBuf,
        reference: String,
    },
    RemoveSubmodule {
        repo_id: RepoId,
        path: PathBuf,
    },
    StageHunk {
        repo_id: RepoId,
        patch: String,
    },
    UnstageHunk {
        repo_id: RepoId,
        patch: String,
    },
    ApplyWorktreePatch {
        repo_id: RepoId,
        patch: String,
        reverse: bool,
    },
    StagePath {
        repo_id: RepoId,
        path: PathBuf,
    },
    StagePaths {
        repo_id: RepoId,
        paths: RepoPathList,
    },
    UnstagePath {
        repo_id: RepoId,
        path: PathBuf,
    },
    UnstagePaths {
        repo_id: RepoId,
        paths: RepoPathList,
    },
    DiscardWorktreeChangesPath {
        repo_id: RepoId,
        path: PathBuf,
    },
    DiscardWorktreeChangesPaths {
        repo_id: RepoId,
        paths: Vec<PathBuf>,
    },
    Commit {
        repo_id: RepoId,
        message: String,
        auth: Option<StagedGitAuth>,
    },
    CommitAmend {
        repo_id: RepoId,
        message: String,
        auth: Option<StagedGitAuth>,
    },
    SafePushAfterCommit {
        repo_id: RepoId,
        context: SafePushAfterCommitContext,
        auth: Option<StagedGitAuth>,
    },
    FetchAll {
        repo_id: RepoId,
        prune: bool,
        auth: Option<StagedGitAuth>,
    },
    /// The activation-triggered twin of [`Effect::FetchAll`]: same command,
    /// reported as `RepoCommandKind::AutoFetchAll` so completion stays quiet.
    AutoFetchAll {
        repo_id: RepoId,
        prune: bool,
        auth: Option<StagedGitAuth>,
    },
    PruneMergedBranches {
        repo_id: RepoId,
    },
    PruneLocalTags {
        repo_id: RepoId,
    },
    Pull {
        repo_id: RepoId,
        mode: PullMode,
        auth: Option<StagedGitAuth>,
    },
    PullBranch {
        repo_id: RepoId,
        remote: String,
        branch: String,
        auth: Option<StagedGitAuth>,
    },
    MergeRef {
        repo_id: RepoId,
        reference: String,
    },
    SquashRef {
        repo_id: RepoId,
        reference: String,
    },
    Push {
        repo_id: RepoId,
        auth: Option<StagedGitAuth>,
    },
    PushAfterCommit {
        repo_id: RepoId,
        target: SafePushAfterCommitTarget,
        set_upstream: bool,
        auth: Option<StagedGitAuth>,
    },
    ForcePush {
        repo_id: RepoId,
        auth: Option<StagedGitAuth>,
    },
    ForcePushWithLease {
        repo_id: RepoId,
        lease: ForcePushLease,
        auth: Option<StagedGitAuth>,
    },
    PushMergeRequest {
        repo_id: RepoId,
        options: MergeRequestPushOptions,
        auth: Option<StagedGitAuth>,
    },
    PushSetUpstream {
        repo_id: RepoId,
        remote: String,
        branch: String,
        auth: Option<StagedGitAuth>,
    },
    SetUpstreamBranch {
        repo_id: RepoId,
        branch: String,
        upstream: String,
    },
    UnsetUpstreamBranch {
        repo_id: RepoId,
        branch: String,
    },
    FastForwardBranch {
        repo_id: RepoId,
        branch: String,
    },
    DeleteRemoteBranch {
        repo_id: RepoId,
        remote: String,
        branch: String,
        auth: Option<StagedGitAuth>,
    },
    DeleteRemoteBranches {
        repo_id: RepoId,
        remote: String,
        branches: Vec<String>,
        auth: Option<StagedGitAuth>,
    },
    Reset {
        repo_id: RepoId,
        target: String,
        mode: ResetMode,
    },
    SquashCommits {
        repo_id: RepoId,
        oldest: CommitId,
        expected_head: CommitId,
        message: String,
        count: usize,
    },
    Rebase {
        repo_id: RepoId,
        onto: String,
    },
    RebaseContinue {
        repo_id: RepoId,
        /// Signing auth (e.g. an ssh/gpg key passphrase) staged for the
        /// replayed commit when the continue retries a sequencer step that
        /// previously failed on a passphrase prompt.
        auth: Option<StagedGitAuth>,
    },
    RebaseAbort {
        repo_id: RepoId,
    },
    /// Bisect commands never sign and never touch the network, so unlike
    /// `RebaseContinue` they carry no staged-auth slot.
    BisectStart {
        repo_id: RepoId,
        bad: Option<String>,
        goods: Vec<String>,
    },
    BisectMark {
        repo_id: RepoId,
        verdict: BisectVerdict,
        commit: Option<String>,
    },
    BisectReset {
        repo_id: RepoId,
    },
    LoadInteractiveRebaseSetup {
        repo_id: RepoId,
        base: String,
    },
    /// List `base..HEAD` (`git log --reverse --no-merges`) so the reducer can
    /// fold `fixup!`/`squash!` commits into their targets for the autosquash
    /// confirmation. Reuses the same git call as the interactive-rebase editor.
    LoadAutosquashSetup {
        repo_id: RepoId,
        base: String,
    },
    /// Preview merging `other` into `head` (`git merge-tree --write-tree`),
    /// read-only: the worktree, index and refs are untouched.
    LoadMergePreview {
        repo_id: RepoId,
        head: CommitId,
        other: CommitId,
    },
    InteractiveRebase {
        repo_id: RepoId,
        base: String,
        entries: Vec<InteractiveRebaseEntry>,
        /// True for the user-opened editor; false for automated todo-list
        /// rebases (e.g. squashing history without HEAD).
        interactive: bool,
    }, // entries held here so the effect dispatcher can pass them to the scheduler
    InteractiveCherryPick {
        repo_id: RepoId,
        entries: Vec<InteractiveRebaseEntry>,
    },
    /// Load the full `%B` messages of the commits selected for an
    /// interactive cherry-pick: the log page only carries subjects, and a
    /// reword edited from a subject-only seed would silently drop the body.
    LoadInteractiveCherryPickMessages {
        repo_id: RepoId,
        ids: Vec<String>,
    },
    MergeAbort {
        repo_id: RepoId,
    },
    CreateTag {
        repo_id: RepoId,
        name: String,
        target: String,
        message: Option<String>,
        annotated: bool,
    },
    DeleteTag {
        repo_id: RepoId,
        name: String,
    },
    PushTag {
        repo_id: RepoId,
        remote: String,
        name: String,
        auth: Option<StagedGitAuth>,
    },
    DeleteRemoteTag {
        repo_id: RepoId,
        remote: String,
        name: String,
        auth: Option<StagedGitAuth>,
    },
    AddRemote {
        repo_id: RepoId,
        name: String,
        url: String,
    },
    RemoveRemote {
        repo_id: RepoId,
        name: String,
    },
    SetRemoteUrl {
        repo_id: RepoId,
        name: String,
        url: String,
        kind: RemoteUrlKind,
    },
    SetRemoteSshKey {
        repo_id: RepoId,
        remote: String,
        key: Option<String>,
    },
    CheckoutConflictSide {
        repo_id: RepoId,
        path: PathBuf,
        side: ConflictSide,
    },
    AcceptConflictDeletion {
        repo_id: RepoId,
        path: PathBuf,
    },
    CheckoutConflictBase {
        repo_id: RepoId,
        path: PathBuf,
    },
    LaunchMergetool {
        repo_id: RepoId,
        path: PathBuf,
        preference: ExternalMergeToolSelection,
    },
    Stash {
        repo_id: RepoId,
        message: String,
        include_untracked: bool,
        keep_index: bool,
        paths: RepoPathList,
    },
    ApplyStash {
        repo_id: RepoId,
        index: usize,
    },
    PopStash {
        repo_id: RepoId,
        index: usize,
    },
    DropStash {
        repo_id: RepoId,
        index: usize,
    },
    StashBranch {
        repo_id: RepoId,
        index: usize,
        branch: String,
    },
    SetAssumeUnchanged {
        repo_id: RepoId,
        path: PathBuf,
        enable: bool,
    },
    LoadAssumeUnchanged {
        repo_id: RepoId,
    },
    /// Fetch the statistics window's commits (see
    /// `GitRepository::contributor_commits_since`). The cutoff is the
    /// executing worker's — long enough to cover a year in any timezone.
    LoadRepoStatistics {
        repo_id: RepoId,
    },
}
