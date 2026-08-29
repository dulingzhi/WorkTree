use super::*;
use worktree_core::services::InteractiveRebaseAction;

const COMMIT_DETAILS_MESSAGE_MAX_HEIGHT_PX: f32 = 240.0;
const COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX: f32 = 200.0;

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
}

#[derive(Clone)]
enum ContextMenuItem {
    Separator,
    Header(components::ContextMenuText),
    /// Muted helper text placed directly under the menu's header.
    Description(components::ContextMenuText),
    Label(components::ContextMenuText),
    Entry {
        label: SharedString,
        icon: Option<SharedString>,
        shortcut: Option<SharedString>,
        disabled: bool,
        action: Box<ContextMenuAction>,
    },
    /// A collapsible group of entries rendered inline: activating the row
    /// reveals the children indented beneath it. Menu lists that would run
    /// long (the repo tab's external tools) fold into one row this way.
    Submenu {
        /// Stable key tracking the group's open state across rebuilds.
        id: SharedString,
        label: SharedString,
        icon: Option<SharedString>,
        children: Vec<ContextMenuItem>,
    },
    /// A caption plus a segmented control, for settings whose options are
    /// mutually exclusive and read better side by side than as a checked list
    /// (the merge tool's view mode). Segments are clicked, not
    /// keyboard-selected, so the row is skipped by arrow navigation.
    Segmented {
        label: SharedString,
        segments: Vec<ContextMenuSegment>,
    },
}

/// One option inside a [`ContextMenuItem::Segmented`] row.
#[derive(Clone)]
struct ContextMenuSegment {
    /// Stable element id, also used as the debug selector.
    id: SharedString,
    label: SharedString,
    tooltip: Option<SharedString>,
    selected: bool,
    action: ContextMenuAction,
}

#[derive(Clone)]
struct ContextMenuModel {
    items: Vec<ContextMenuItem>,
    /// Render shortcut labels as individual keycaps, matching the Command
    /// Palette. Most context menus use shortcuts as single-key mnemonics, so
    /// this remains opt-in.
    shortcut_keycaps: bool,
    /// Optional hover tooltip per entry index (e.g. the full commit message in the
    /// browse-history menu). Sparse — most menus leave this empty.
    entry_tooltips: FxHashMap<usize, SharedString>,
    /// Stable debug selectors for menus whose entries predate the shared context-menu
    /// renderer. Sparse so ordinary menus continue deriving selectors from labels.
    entry_debug_selectors: FxHashMap<usize, SharedString>,
}

impl ContextMenuModel {
    fn new(items: Vec<ContextMenuItem>) -> Self {
        Self {
            items,
            shortcut_keycaps: false,
            entry_tooltips: FxHashMap::default(),
            entry_debug_selectors: FxHashMap::default(),
        }
    }

    fn with_shortcut_keycaps(mut self) -> Self {
        self.shortcut_keycaps = true;
        self
    }

    fn with_entry_tooltips(mut self, entry_tooltips: FxHashMap<usize, SharedString>) -> Self {
        self.entry_tooltips = entry_tooltips;
        self
    }

    fn with_entry_debug_selectors(
        mut self,
        entry_debug_selectors: FxHashMap<usize, SharedString>,
    ) -> Self {
        self.entry_debug_selectors = entry_debug_selectors;
        self
    }
}

/// The model flattened into the rows a menu actually shows: top-level items
/// with the children of open submenus spliced in beneath their parent.
/// Selection indices (`context_menu_selected_ix`) refer to positions here,
/// because opening or closing a submenu changes which rows exist.
#[derive(Clone)]
pub(super) struct ContextMenuRows {
    rows: Vec<(ContextMenuItem, u8)>,
}

impl ContextMenuRows {
    fn from_model(model: &ContextMenuModel, open_submenus: &FxHashSet<SharedString>) -> Self {
        let mut rows = Vec::with_capacity(model.items.len());
        fn flatten_into(
            target: &mut Vec<(ContextMenuItem, u8)>,
            items: Vec<ContextMenuItem>,
            depth: u8,
            open_submenus: &FxHashSet<SharedString>,
        ) {
            for item in items {
                match item {
                    ContextMenuItem::Submenu {
                        id,
                        label,
                        icon,
                        children,
                    } => {
                        let is_open = open_submenus.contains(&id);
                        // The pushed row keeps no children: rendering only
                        // needs the row itself, and the open set decides
                        // whether the children were spliced in below.
                        target.push((
                            ContextMenuItem::Submenu {
                                id,
                                label,
                                icon,
                                children: Vec::new(),
                            },
                            depth,
                        ));
                        if is_open {
                            flatten_into(target, children, depth + 1, open_submenus);
                        }
                    }
                    other => target.push((other, depth)),
                }
            }
        }
        flatten_into(&mut rows, model.items.clone(), 0, open_submenus);
        Self { rows }
    }

    fn get(&self, ix: usize) -> Option<&(ContextMenuItem, u8)> {
        self.rows.get(ix)
    }

    fn into_iter(self) -> impl Iterator<Item = (ContextMenuItem, u8)> {
        self.rows.into_iter()
    }

    fn iter(&self) -> impl Iterator<Item = &(ContextMenuItem, u8)> + '_ {
        self.rows.iter()
    }

    fn is_selectable(&self, ix: usize) -> bool {
        match self.rows.get(ix) {
            Some((ContextMenuItem::Entry { disabled, .. }, _)) => !*disabled,
            // A submenu row toggles its children; it is always actionable.
            Some((ContextMenuItem::Submenu { .. }, _)) => true,
            _ => false,
        }
    }

    fn first_selectable(&self) -> Option<usize> {
        (0..self.rows.len()).find(|&ix| self.is_selectable(ix))
    }

    fn last_selectable(&self) -> Option<usize> {
        (0..self.rows.len())
            .rev()
            .find(|&ix| self.is_selectable(ix))
    }

    fn next_selectable(&self, from: Option<usize>, dir: isize) -> Option<usize> {
        if self.rows.is_empty() {
            return None;
        }
        let Some(mut ix) = from else {
            return if dir >= 0 {
                self.first_selectable()
            } else {
                self.last_selectable()
            };
        };

        let n = self.rows.len() as isize;
        for _ in 0..self.rows.len() {
            ix = ((ix as isize + dir).rem_euclid(n)) as usize;
            if self.is_selectable(ix) {
                return Some(ix);
            }
        }
        None
    }
}

// HistoryColResizeDragGhost moved to view/mod.rs for accessibility from panes::HistoryView.

mod action_bar;
mod bars;
mod bottom_status_bar;
mod layout;
mod main;
mod popover;
mod repo_tabs_bar;

pub(super) use action_bar::{ActionBarView, action_bar_height};
pub(super) use bottom_status_bar::BottomStatusBarView;
pub(super) use popover::PopoverHost;
/// The reflog pane's header button asks whether an undo is available before
/// rendering, so the resolver ships one hop out of the private popover tree.
pub(in crate::view) use popover::undo_last_action::{UndoResolution, resolve_undo};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use popover::{benchmark_branch_checkout_rows, benchmark_workspace_rows};
pub(in crate::view) use popover::merge_request_push::git_output;
/// Layout guards outside this module assert against the tab padding, so they
/// follow the constant instead of hardcoding the current value.
#[cfg(test)]
pub(in crate::view) use repo_tabs_bar::REPO_TAB_SIDE_PADDING_PX;
pub(super) use repo_tabs_bar::RepoTabsBarView;
#[allow(unused_imports)]
pub(in crate::view) use repo_tabs_bar::repo_tab_insert_before_for_drag_cursor;

#[cfg(test)]
mod tests;
