//! The host itself and its row type.

use super::super::*;
use super::kinds::PopoverKind;
use super::state::{
    BranchPickerState, CloneRepoState, CommitPromptState, CommitSearchPickerState,
    ContextMenuState, CreateBranchState, CreateTagState, FileHistoryState, GitignoreState,
    HistoryAuthorFilterState, HistoryRefFilterState, MrPushState, PushUpstreamState,
    RebaseOntoState, RebaseRewordState, RemotePickerState, RemotePromptsState, RepoPickerState,
    RepoSettingsState, SquashState, StashPickerState, StashState, SubmoduleAddState,
    SubmodulePickerState, TagPickerState, UpstreamPickerState, WorkspacePickerState,
    WorktreeAddState, WorktreePickerState,
};

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::view) enum RemoteRow {
    Header(String),
    Branch { remote: String, name: String },
}

pub(in crate::view) struct PopoverHost {
    pub(in crate::view::panels::popover) store: Arc<AppStore>,

    pub(in crate::view::panels::popover) state: Arc<AppState>,

    pub(in crate::view::panels::popover) theme: AppTheme,

    pub(in crate::view::panels::popover) theme_mode: ThemeMode,

    pub(in crate::view::panels::popover) date_time_format: DateTimeFormat,

    pub(in crate::view::panels::popover) timezone: Timezone,

    pub(in crate::view::panels::popover) show_timezone: bool,

    pub(in crate::view::panels::popover) change_tracking_view: ChangeTrackingView,

    pub(in crate::view::panels::popover) commit_amend_enabled: bool,

    pub(in crate::view::panels::popover) commit_push_after_enabled: bool,

    pub(in crate::view::panels::popover) push_pull_retry_enabled: bool,

    pub(in crate::view::panels::popover) diff_content_mode: DiffContentMode,

    pub(in crate::view::panels::popover) diff_whitespace_mode: DiffWhitespaceMode,

    pub(in crate::view::panels::popover) diff_reveal_whitespace_chars: bool,

    pub(in crate::view::panels::popover) diff_word_wrap: bool,

    pub(in crate::view::panels::popover) diff_show_line_numbers: bool,

    pub(in crate::view::panels::popover) _ui_model_subscription: gpui::Subscription,

    pub(in crate::view::panels::popover) repo_picker: RepoPickerState,

    pub(in crate::view::panels::popover) branch_picker: BranchPickerState,

    pub(in crate::view::panels::popover) worktree_picker: WorktreePickerState,

    pub(in crate::view::panels::popover) workspace_picker: WorkspacePickerState,

    pub(in crate::view::panels::popover) upstream_picker: UpstreamPickerState,

    pub(in crate::view::panels::popover) submodule_picker: SubmodulePickerState,

    pub(in crate::view::panels::popover) remote_picker: RemotePickerState,

    pub(in crate::view::panels::popover) tag_picker: TagPickerState,

    pub(in crate::view::panels::popover) commit_search_picker: CommitSearchPickerState,

    pub(in crate::view::panels::popover) file_history: FileHistoryState,

    pub(in crate::view::panels::popover) history_author_filter: HistoryAuthorFilterState,

    pub(in crate::view::panels::popover) history_ref_filter: HistoryRefFilterState,

    pub(in crate::view::panels::popover) squash: SquashState,

    pub(in crate::view::panels::popover) _prompt_input_subscriptions: Vec<gpui::Subscription>,

    pub(in crate::view::panels::popover) notify_fingerprint: u64,

    pub(in crate::view::panels::popover) root_view: WeakEntity<WorkTreeView>,

    /// Mirror of the root view's mode, which is fixed for the window's lifetime.
    /// Held here because menu models are built while the root view's update
    /// borrow is active, so its entity can't be read at that point.
    pub(in crate::view::panels::popover) root_view_mode: WorkTreeViewMode,

    pub(in crate::view::panels::popover) tooltip_host: WeakEntity<TooltipHost>,

    pub(in crate::view::panels::popover) main_pane: Entity<MainPaneView>,

    pub(in crate::view::panels::popover) details_pane: Entity<DetailsPaneView>,

    pub(in crate::view::panels::popover) reflog_pane: Entity<ReflogPaneView>,

    pub(in crate::view::panels::popover) sidebar_pane: Entity<SidebarPaneView>,

    /// Mirror of the sidebar pane's pinned branches, keyed by repository
    /// workdir. Kept here because context menus are built from click handlers
    /// that already hold the sidebar pane's update borrow, so its entity can't
    /// be read at that point.
    pub(in crate::view::panels::popover) pinned_branches_by_repo:
        std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,

    /// Mirror of the sidebar's collapse set, kept here for the same reason as
    /// [`Self::pinned_branches_by_repo`]: the branch group menu is built while
    /// the sidebar pane's update borrow is already held.
    pub(in crate::view::panels::popover) collapsed_items_by_repo:
        std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,

    /// Mirror of the sidebar's branch filter, for the same reason.
    pub(in crate::view::panels::popover) branch_filter_query: String,

    pub(in crate::view::panels::popover) popover: Option<PopoverKind>,

    pub(in crate::view::panels::popover) popover_anchor: Option<PopoverAnchor>,

    /// Explicit 1-based mainline selected for the currently open single
    /// merge-commit cherry-pick confirmation. Reset every time that dialog
    /// opens; drafts are intentionally session-local.
    pub(in crate::view::panels::popover) cherry_pick_mainline: Option<usize>,

    /// Period tab shown by the statistics popover. View-local rather than a
    /// `PopoverKind` field so switching tabs doesn't reopen the popover (which
    /// would refocus and re-request data); reset to Week on open.
    pub(in crate::view::panels::popover) statistics_period: statistics::StatisticsPeriod,

    /// Reset mode chosen in the undo prompt, once the user departs from the
    /// plan's suggestion. `None` until then; reset on open.
    pub(in crate::view::panels::popover) undo_reset_mode: Option<ResetMode>,

    /// The diff view's pending (or landed) AI explanation, paired with the
    /// hunk snapshot it was requested against. Reset on open; every open
    /// starts a fresh request.
    pub(in crate::view::panels::popover) hunk_explanation:
        Option<hunk_explanation::HunkExplanation>,

    /// Test seams standing in for the network call test builds cannot make:
    /// how many explanation requests were driven, and the patch the last one
    /// carried.
    #[cfg(test)]
    pub(in crate::view::panels::popover) hunk_explanation_test_requests: usize,

    #[cfg(test)]
    pub(in crate::view::panels::popover) hunk_explanation_test_last_patch: Option<String>,

    pub(in crate::view::panels::popover) context_menu_focus_handle: FocusHandle,

    /// Focus held by the App/Add Repository menu invoker, restored when that
    /// menu is dismissed without replacing it with another prompt.
    pub(in crate::view::panels::popover) menu_invoker_focus: Option<FocusHandle>,

    /// Whether the open popover was invoked from inside the diff panel.
    ///
    /// Some menus — the web link menu above all — can be raised from either the
    /// diff panel or the commit details pane, and only the former should hand
    /// focus back to the diff panel when it closes.
    pub(in crate::view::panels::popover) popover_opened_from_diff_panel: bool,

    pub(in crate::view::panels::popover) prompt_tab_group_focus_handle: FocusHandle,

    pub(in crate::view::panels::popover) prompt_tab_wrap_end_focus_handle: FocusHandle,

    pub(in crate::view::panels::popover) context_menu: ContextMenuState,

    /// Repository row whose context menu floats over the picker, and the window
    /// position it was invoked at. The picker stays open underneath it.
    pub(in crate::view::panels::popover) picker_row_menu: Option<picker_row_menu::PickerRowMenu>,

    pub(in crate::view::panels::popover) worktree_add: WorktreeAddState,

    pub(in crate::view::panels::popover) stash_picker: StashPickerState,

    pub(in crate::view::panels::popover) picker_prompt_scroll: ScrollHandle,

    pub(in crate::view::panels::popover) clone_repo: CloneRepoState,

    pub(in crate::view::panels::popover) repo_settings: RepoSettingsState,

    pub(in crate::view::panels::popover) rebase_onto: RebaseOntoState,

    pub(in crate::view::panels::popover) create_tag: CreateTagState,

    pub(in crate::view::panels::popover) gitignore: GitignoreState,

    pub(in crate::view::panels::popover) remote_prompts: RemotePromptsState,

    pub(in crate::view::panels::popover) create_branch: CreateBranchState,

    /// Set while a row menu floating over a picker runs one of its entries. The
    /// menu has already closed itself by then, and the popover underneath is the
    /// picker — which stays up so the next row can be acted on.
    pub(in crate::view::panels::popover) suppress_popover_close_after_action: bool,

    pub(in crate::view::panels::popover) checkout_remote_branch_focus: DialogFocus,

    pub(in crate::view::panels::popover) stash: StashState,

    pub(in crate::view::panels::popover) mr_push: MrPushState,

    pub(in crate::view::panels::popover) stash_branch_focus: DialogFocus,

    pub(in crate::view::panels::popover) commit_prompt: CommitPromptState,

    pub(in crate::view::panels::popover) push_upstream: PushUpstreamState,

    pub(in crate::view::panels::popover) submodule_add: SubmoduleAddState,

    pub(in crate::view::panels::popover) rebase_reword: RebaseRewordState,
}

/// Rows the branch badge's checkout picker would show for `query`, for the
/// picker benchmarks. The builder is a pure function of the repository, so the
/// benchmark measures exactly what a frame used to rebuild.
#[cfg(feature = "benchmarks")]
pub(in crate::view) fn benchmark_branch_checkout_rows(
    repo: &RepoState,
    query: &str,
    now: std::time::SystemTime,
) -> Vec<components::PickerPromptItem> {
    branch_picker::rows(repo, query, now).items
}

/// Rows the workspace badge's picker would show for `query`, for the picker
/// benchmarks.
#[cfg(feature = "benchmarks")]
pub(in crate::view) fn benchmark_workspace_rows(
    repo: &RepoState,
    query: &str,
) -> Vec<components::PickerPromptItem> {
    workspace_picker::rows(repo, query).items
}
