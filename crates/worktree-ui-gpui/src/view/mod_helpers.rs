use super::*;
use rustc_hash::FxHashMap;

/// What the window was about to do when unsaved edits were found.
///
/// Only the two irreversible ones: switching files keeps the buffer, so it
/// needs no prompt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum UnsavedFileEditsAction {
    /// Carries the window that asked: the retry can run seconds later, after a
    /// slow write drains, by which time "the active window" may be another one.
    CloseWindow(gpui::WindowId),
    QuitApp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct UnsavedFileEditsPrompt {
    pub(in crate::view) action: UnsavedFileEditsAction,
    /// Display labels, repo-qualified when the list spans more than one repo.
    pub(in crate::view) files: Vec<SharedString>,
}

pub struct WorkTreeView {
    pub(super) store: Arc<AppStore>,
    pub(super) state: Arc<AppState>,
    pub(super) window_handle: gpui::AnyWindowHandle,
    pub(super) _ui_model: Entity<AppUiModel>,
    pub(super) _poller: Poller,
    pub(super) _ui_model_subscription: gpui::Subscription,
    pub(super) _activation_subscription: gpui::Subscription,
    pub(super) _appearance_subscription: gpui::Subscription,
    pub(super) _terminal_keystroke_interceptor: gpui::Subscription,
    pub(super) _auth_prompt_username_input_subscription: gpui::Subscription,
    pub(super) _auth_prompt_secret_input_subscription: gpui::Subscription,
    pub(super) _open_repo_input_subscription: gpui::Subscription,
    pub(super) view_mode: WorkTreeViewMode,
    pub(super) theme_mode: ThemeMode,
    pub(super) theme: AppTheme,
    pub(super) title_bar: Entity<TitleBarView>,
    pub(super) sidebar_pane: Entity<SidebarPaneView>,
    pub(super) main_pane: Entity<MainPaneView>,
    pub(super) details_pane: Entity<DetailsPaneView>,
    pub(super) repo_tabs_bar: Entity<RepoTabsBarView>,
    pub(super) action_bar: Entity<ActionBarView>,
    pub(super) bottom_status_bar: Entity<BottomStatusBarView>,
    pub(super) tooltip_host: Entity<TooltipHost>,
    pub(super) toast_host: Entity<ToastHost>,
    pub(super) history_refs_hover_host: Entity<HistoryRefsHoverHost>,
    pub(super) commit_message_hover_host: Entity<CommitMessageHoverHost>,
    pub(super) popover_host: Entity<PopoverHost>,
    pub(super) command_palette: Entity<super::command_palette::CommandPaletteView>,
    pub(super) command_palette_open: bool,
    pub(super) pre_palette_focus: Option<FocusHandle>,
    pub(super) focused_mergetool_bootstrap: Option<FocusedMergetoolBootstrap>,
    pub(super) submodule_diff_bootstrap: Option<SubmoduleDiffBootstrap>,
    pub(super) deferred_repo_bootstrap: Option<DeferredRepoBootstrap>,
    pub(super) startup_repo_bootstrap_pending: bool,
    pub(super) splash_backdrop_image: Arc<gpui::Image>,

    pub(super) last_window_size: Size<Pixels>,
    pub(super) ui_window_size_last_seen: Size<Pixels>,
    pub(super) ui_settings_persist_seq: u64,
    pub(super) last_repo_activation_dispatch_at: FxHashMap<RepoId, Instant>,
    /// Set when a deactivation was caused by a move/resize grab we requested, so
    /// the matching re-activation does not trigger a repo refresh.
    pub(super) window_grab_activation_suppressed_at: Option<Instant>,

    pub(super) date_time_format: DateTimeFormat,
    pub(super) timezone: Timezone,
    pub(super) show_timezone: bool,
    pub(super) change_tracking_view: ChangeTrackingView,
    pub(super) terminal_preferences: TerminalPreferences,
    pub(super) terminal_sessions: FxHashMap<RepoId, RepoTerminalSession>,
    /// Agent workbench sessions (claude code / codex): one per repo,
    /// keyed alongside the terminal session that hosts them.
    pub(super) agent_sessions: FxHashMap<RepoId, agent_workbench::AgentSessionState>,
    pub(super) terminal_panel_height: Pixels,
    pub(super) terminal_panel_resize: Option<TerminalPanelResizeState>,
    pub(super) next_terminal_session_seq: u64,
    pub(super) terminal_cursor_blink_visible: bool,
    pub(super) terminal_cursor_blink_hold_until: Instant,
    pub(super) terminal_cursor_blink_active: bool,
    pub(super) terminal_cursor_blink_task_scheduled: bool,
    pub(super) terminal_cursor_blink_seq: u64,
    /// The reflog panel. It owns its own per-repository state (filter text,
    /// scroll, selection) — a separate entity so that hovering one of its rows
    /// repaints the panel instead of the whole application window.
    pub(super) reflog_pane: Entity<ReflogPaneView>,
    /// Which of the bottom panel's contents is currently visible for a repo,
    /// when more than one is open. Absent (and single-panel repos) fall back
    /// to whichever panel is actually open.
    pub(super) active_bottom_panel: FxHashMap<RepoId, BottomPanelTab>,
    pub(super) commit_push_after_enabled: bool,
    /// Toolbar push-menu toggle: when on, a push rejected because the remote
    /// is ahead automatically pulls (rebase) and pushes once more.
    pub(super) push_pull_retry_enabled: bool,
    pub(super) diff_scroll_sync: DiffScrollSync,
    pub(super) diff_content_mode: DiffContentMode,
    pub(super) diff_whitespace_mode: DiffWhitespaceMode,
    pub(super) diff_view_mode: DiffViewMode,
    pub(super) annotate_enabled: bool,
    pub(super) diff_reveal_whitespace_chars: bool,
    pub(super) diff_word_wrap: bool,
    pub(super) diff_show_line_numbers: bool,
    pub(super) auto_save_file_edits: bool,
    pub(super) ui_scale_percent: u32,
    /// Row-rhythm tier for the main lists; mirrors `ui_scale_percent` but for
    /// density. Density only adjusts row heights and list insets — never font
    /// sizes — and composes with the percentage scale.
    pub(super) ui_density: crate::density::Density,

    pub(super) open_repo_panel: bool,
    pub(super) open_repo_input: Entity<components::TextInput>,

    pub(super) hover_resize_edge: Option<ResizeEdge>,

    pub(super) sidebar_collapsed: bool,
    /// Which sidebar section is currently shown in the collapsed-rail popover, if
    /// any. Only meaningful while `sidebar_collapsed` is true.
    pub(super) sidebar_collapsed_popover: Option<CollapsedSidebarSection>,
    /// A section whose popover is fading out. Kept mounted (invisible input) for
    /// the fade-out duration, then cleared by a timer keyed on the anim seq.
    pub(super) sidebar_collapsed_popover_closing: Option<CollapsedSidebarSection>,
    /// Bumped on every open/close transition; keys the fade animation (so it
    /// restarts each time) and guards the close timer against races.
    pub(super) sidebar_collapsed_popover_anim_seq: u64,
    pub(super) sidebar_collapsed_before_merge_view: Option<bool>,
    pub(super) details_collapsed: bool,
    pub(super) sidebar_width_design: f32,
    pub(super) details_width_design: f32,
    pub(super) sidebar_width: Pixels,
    pub(super) details_width: Pixels,
    pub(super) sidebar_render_width: Pixels,
    pub(super) details_render_width: Pixels,
    pub(super) sidebar_width_anim_seq: u64,
    pub(super) details_width_anim_seq: u64,
    pub(super) sidebar_width_animating: bool,
    pub(super) details_width_animating: bool,
    pub(super) pane_resize: Option<PaneResizeState>,

    pub(super) last_mouse_pos: Point<Pixels>,
    pub(super) pending_terminal_shutdown_prompt: Option<TerminalShutdownPrompt>,
    pub(super) pending_unsaved_file_edits_prompt: Option<UnsavedFileEditsPrompt>,
    /// Waits for the dispatched writes to drain before the close/quit it was
    /// asked to retry.
    pub(super) pending_unsaved_file_edits_flush: Option<gpui::Task<()>>,
    pub(super) pending_quit_other_views: Vec<gpui::WeakEntity<WorkTreeView>>,
    pub(super) pending_pull_reconcile_prompt: Option<RepoId>,
    pub(super) pending_force_delete_branch_prompt: Option<(RepoId, String)>,
    pub(super) pending_force_delete_branch_centered: bool,
    pub(super) pending_force_remove_worktree_prompt:
        Option<(RepoId, std::path::PathBuf, Option<String>)>,
    pub(super) pending_submodule_trust_prompt:
        Option<worktree_state::model::SubmoduleTrustPromptState>,
    pub(super) pending_submodule_trust_check:
        Option<worktree_state::model::SubmoduleTrustCheckState>,
    pub(super) pending_worktree_branch_removals: FxHashMap<(RepoId, std::path::PathBuf), String>,
    pub(super) startup_crash_report: Option<StartupCrashReport>,
    #[cfg(target_os = "macos")]
    pub(super) recent_repos_menu_fingerprint: Vec<std::path::PathBuf>,

    pub(super) error_banner_input: Entity<components::TextInput>,
    pub(super) auth_prompt_username_input: Entity<components::TextInput>,
    pub(super) auth_prompt_secret_input: Entity<components::TextInput>,
    pub(super) auth_prompt_key: Option<String>,
    pub(super) active_context_menu_invoker: Option<SharedString>,
}
