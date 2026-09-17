use crate::view::{
    AutosquashMode, BranchSection, ChangeTrackingView, ConflictResolverJoinTarget, DiffContentMode,
    DiffTextRegion, DiffWhitespaceMode, PopoverKind, ResolverPickTarget,
};
use gpui::SharedString;
use worktree_core::domain::{CommitId, DiffArea, DiffTarget};
use worktree_core::services::{InteractiveRebaseAction, PullMode};
use worktree_state::model::RepoId;

#[derive(Clone)]
pub(in crate::view) enum AppMenuAction {
    CommandPalette,
    /// Reveal the open file in the sidebar's file explorer. Like every other
    /// variant here, whether it can run is carried by the menu item's own
    /// `disabled` flag rather than duplicated in the payload.
    LocateFileInExplorer,
    Settings,
    OpenInCodeEditor {
        path: Option<std::path::PathBuf>,
    },
    ApplyPatch {
        repo_id: Option<RepoId>,
    },
    /// Show the reflog panel for the active repository, in the bottom panel.
    ShowReflog {
        repo_id: Option<RepoId>,
    },
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    InstallDesktopIntegration,
    Quit,
    CloseWindow,
}

#[derive(Clone, Copy)]
pub(in crate::view) enum AddRepoMenuAction {
    Open,
    Clone,
    Initialize,
}

#[derive(Clone)]
pub(in crate::view) enum ContextMenuAction {
    AppMenu(AppMenuAction),
    AddRepoMenu(AddRepoMenuAction),
    SelectDiff {
        repo_id: RepoId,
        target: DiffTarget,
    },
    SelectConflictDiff {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    OpenFile {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    OpenFileLocation {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    OpenRepositoryLocation {
        path: std::path::PathBuf,
    },
    OpenInCodeEditor {
        repo_id: Option<RepoId>,
        path: std::path::PathBuf,
    },
    /// Open the target in one specific detected editor, carrying the editor's
    /// detection identity so the launch path can rebuild its setting.
    OpenInDetectedEditor {
        repo_id: Option<RepoId>,
        path: std::path::PathBuf,
        id: String,
        editor_path: std::path::PathBuf,
    },
    OpenFileContent {
        repo_id: RepoId,
        source: worktree_core::domain::FileSource,
        path: std::path::PathBuf,
    },
    /// Open the working-tree file in WorkTree's own editor. Carries no source:
    /// editing is always of the workspace copy, whatever view it was invoked
    /// from.
    EditFile {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    /// Throw away the editor's unsaved buffer for this file and reload it from
    /// disk. Handled in the view rather than dispatched: the buffer lives in
    /// `MainPaneView`, and the store has no message for it.
    DiscardFileEdits {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    /// Open or close one folder in the file explorer — the menu's counterpart
    /// to clicking the row.
    ToggleFileBrowserDir {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    /// Flip one sidebar collapse key — the branch tree's counterpart to
    /// clicking a group or section header.
    ToggleSidebarCollapseKey {
        collapse_key: SharedString,
    },
    /// Drive one sidebar collapse key to an explicit state.
    ///
    /// For rows whose rendered state can diverge from the stored key — a live
    /// branch filter force-expands the pinned sections — where a flip would
    /// move the key the opposite way from what the entry's label promised.
    SetSidebarCollapseKey {
        collapse_key: SharedString,
        collapsed: bool,
    },
    /// Open or close a branch group together with every group beneath it.
    SetBranchGroupCollapsedRecursive {
        section: BranchSection,
        remote: Option<String>,
        path: String,
        collapsed: bool,
    },
    /// Drop every pin in one branch section.
    UnpinAllBranches {
        repo_id: RepoId,
        section: BranchSection,
    },
    /// Resolve a branch group's members and open the delete confirmation.
    ///
    /// Carries the group rather than the resolved names: the menu model is
    /// rebuilt on every repaint while it is open, and materialising a few
    /// hundred branch names per frame to render one count is waste. The confirm
    /// still freezes the list it is handed.
    ConfirmDeleteBranchGroup {
        repo_id: RepoId,
        section: BranchSection,
        remote: Option<String>,
        path: String,
        group_label: String,
    },
    /// Open or close a folder together with every directory beneath it.
    SetFileBrowserDirExpandedRecursive {
        repo_id: RepoId,
        path: std::path::PathBuf,
        expanded: bool,
    },
    /// Scroll the history to a commit referenced from somewhere else and
    /// show its details.
    RevealHistoryCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    BrowseRepositoryAtCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    ResetBrowseToLive {
        repo_id: RepoId,
    },
    OpenRepo {
        path: std::path::PathBuf,
    },
    ActivateRepo {
        repo_id: RepoId,
    },
    CloseRepo {
        repo_id: RepoId,
    },
    CloseRepos {
        repo_ids: Vec<RepoId>,
        activate_after: Option<RepoId>,
    },
    /// Keep a repository in the picker's Pinned section. Pins outlive both the
    /// recents cap and the repository being closed, so this is what keeps one
    /// reachable for good.
    PinRepository {
        path: std::path::PathBuf,
    },
    UnpinRepository {
        path: std::path::PathBuf,
    },
    /// Drop a repository from the session's recent list. A pinned repository is
    /// refused: the pin is what keeps a closed repository listed at all, so
    /// forgetting one would strand it with nothing to bring it back.
    ForgetRecentRepository {
        path: std::path::PathBuf,
    },
    OpenSubmoduleDiffInTab {
        path: std::path::PathBuf,
        target: DiffTarget,
    },
    ExportPatch {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    /// Export `revision`'s tree as a zip archive. `suggested_name` pre-fills
    /// the platform save dialog (`archive-<short-ref>.zip`).
    ArchiveZip {
        repo_id: RepoId,
        revision: String,
        suggested_name: String,
    },
    MarkForComparison {
        repo_id: RepoId,
        commit_id: CommitId,
        label: String,
    },
    CompareWithMarked {
        repo_id: RepoId,
        commit_id: CommitId,
        label: String,
    },
    CompareWithWorkingTree {
        repo_id: RepoId,
        commit_id: CommitId,
        label: String,
    },
    ClearComparisonMark {
        repo_id: RepoId,
    },
    CheckoutCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    /// Starts a bisect session anchored at the right-clicked commit. `bad` is
    /// the commit marked bad up front (the "start here as bad" gesture); the
    /// good anchor is marked afterwards from another commit's menu.
    BisectStartAt {
        repo_id: RepoId,
        bad: Option<String>,
        goods: Vec<String>,
    },
    /// Marks a specific commit good/bad/skip in the running bisect session.
    BisectMarkCommit {
        repo_id: RepoId,
        verdict: worktree_core::services::BisectVerdict,
        commit: String,
    },
    CherryPickCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    RevertCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    /// Produce a `fixup! <target subject>` commit on top of HEAD, targeting the
    /// right-clicked commit. Rides the ordinary commit path; the next autosquash
    /// folds it back into `commit_id`. Never pushes on its own.
    FixupCommit {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    /// Opens the squash confirmation prompt for the current multi-selection.
    SquashSelectedCommits {
        repo_id: RepoId,
    },
    CheckoutBranch {
        repo_id: RepoId,
        name: String,
    },
    /// Checks a pull request out into a local `pr/N` branch (fetching
    /// `refs/pull/N/head` first when the branch does not exist yet).
    CheckoutPullRequest {
        repo_id: RepoId,
        remote: String,
        number: u64,
    },
    /// Open the forge's prefilled create-request page (GitHub compare /
    /// GitLab merge-request form) for the active repo's current branch.
    CreateWebRequestPage,
    DeleteBranch {
        repo_id: RepoId,
        name: String,
    },
    /// Pins/unpins a branch in the sidebar's dedicated "Pinned" section.
    ToggleBranchPin {
        repo_id: RepoId,
        section: BranchSection,
        name: String,
    },
    SetHistoryScope {
        repo_id: RepoId,
        scope: worktree_core::domain::LogScope,
    },
    SetDiffContentMode {
        mode: DiffContentMode,
    },
    SetDiffWhitespaceMode {
        mode: DiffWhitespaceMode,
    },
    SetDiffRevealWhitespaceChars {
        enabled: bool,
    },
    SetDiffWordWrap {
        enabled: bool,
    },
    SetDiffShowLineNumbers {
        enabled: bool,
    },
    SetChangeTrackingView {
        view: ChangeTrackingView,
    },
    SetCommitAmendEnabled {
        enabled: bool,
    },
    SetCommitPushAfterEnabled {
        enabled: bool,
    },
    SetPushPullRetryEnabled {
        enabled: bool,
    },
    UseCommitMessage {
        message: String,
    },
    SetUiScale {
        percent: u32,
    },
    StageSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    UnstageSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    DiscardWorktreeChangesSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    AddToGitignoreSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    /// Opens the stash prompt pre-seeded with the clicked row — or the whole
    /// multi-selection when the click belongs to one.
    StashSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    /// Marks one tracked status file assume-unchanged in the index.
    SetAssumeUnchangedPath {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    CheckoutConflictSideSelectionOrPath {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
        side: worktree_core::services::ConflictSide,
    },
    LaunchMergetool {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    FetchAll {
        repo_id: RepoId,
    },
    PruneMergedBranches {
        repo_id: RepoId,
    },
    PruneLocalTags {
        repo_id: RepoId,
    },
    UpdateSubmodules {
        repo_id: RepoId,
    },
    LoadSubmodule {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    LoadWorktrees {
        repo_id: RepoId,
    },
    Pull {
        repo_id: RepoId,
        mode: PullMode,
    },
    PullBranch {
        repo_id: RepoId,
        remote: String,
        branch: String,
    },
    MergeRef {
        repo_id: RepoId,
        reference: String,
    },
    SquashRef {
        repo_id: RepoId,
        reference: String,
    },
    ApplyStash {
        repo_id: RepoId,
        index: usize,
    },
    PopStash {
        repo_id: RepoId,
        index: usize,
    },
    DropStashConfirm {
        repo_id: RepoId,
        index: usize,
        message: String,
    },
    Push {
        repo_id: RepoId,
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
    OpenPopover {
        kind: PopoverKind,
    },
    LoadInteractiveRebaseSetup {
        repo_id: RepoId,
        base: String,
    },
    OpenInteractiveCherryPickSetup {
        repo_id: RepoId,
        entries: Vec<worktree_core::services::InteractiveRebaseEntry>,
        source_colors: Vec<(String, u8)>,
    },
    SetInteractiveRebaseAction {
        ix: usize,
        action: InteractiveRebaseAction,
    },
    SetInteractiveRebaseAutosquashMode {
        mode: AutosquashMode,
    },
    ConflictResolverPick {
        target: ResolverPickTarget,
    },
    ConflictResolverUnresolve {
        conflict_ix: usize,
    },
    ConflictResolverSplitSelection,
    /// kdiff3 manual diff help: pin the marked lines onto one another.
    ConflictResolverAlignManually,
    /// kdiff3 manual diff help: drop every pin and replan automatically.
    ConflictResolverClearManualAlignments,
    ConflictResolverJoinRegions {
        target: ConflictResolverJoinTarget,
    },
    SetMergetoolAutoAdvance {
        enabled: bool,
    },
    ToggleMergetoolCollapseUnchanged,
    SetMergetoolOutputScrollSync {
        enabled: bool,
    },
    SetMergetoolShowLineNumbers {
        enabled: bool,
    },
    SetMergetoolThreeWayView {
        enabled: bool,
    },
    ConflictResolverOutputCut {
        text: String,
    },
    ConflictResolverOutputPaste,
    CopyText {
        text: String,
    },
    /// Copy a link's destination, and say so.
    ///
    /// Separate from [`ContextMenuAction::CopyText`] because a link's address
    /// is never on screen — the document shows its text — so the reader has no
    /// way to tell the copy happened without being told.
    CopyLinkAddress {
        url: String,
    },
    OpenWebUrl {
        url: String,
    },
    CopyDiffSelection {
        text: String,
    },
    CopyDiffText {
        visible_ix: usize,
        region: DiffTextRegion,
    },
    TerminalCopy {
        repo_id: RepoId,
    },
    TerminalPaste {
        repo_id: RepoId,
    },
    TerminalSelectAll {
        repo_id: RepoId,
    },
    TerminalClear {
        repo_id: RepoId,
    },
    TerminalOpenExternal {
        repo_id: RepoId,
    },
    ApplyIndexPatch {
        repo_id: RepoId,
        patch: String,
        reverse: bool,
    },
    ApplyWorktreePatch {
        repo_id: RepoId,
        patch: String,
        reverse: bool,
    },
    StageHunk {
        repo_id: RepoId,
        src_ix: usize,
    },
    UnstageHunk {
        repo_id: RepoId,
        src_ix: usize,
    },
    ExplainHunk {
        repo_id: RepoId,
        src_ix: usize,
    },
    DeleteTag {
        repo_id: RepoId,
        name: String,
    },
    PushTag {
        repo_id: RepoId,
        remote: String,
        name: String,
    },
    DeleteRemoteTag {
        repo_id: RepoId,
        remote: String,
        name: String,
    },
    /// Open the directory diff (SmartGit-style folder comparison) for this
    /// folder, scoped to the active commit-range compare view.
    CompareDirectory {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
}
