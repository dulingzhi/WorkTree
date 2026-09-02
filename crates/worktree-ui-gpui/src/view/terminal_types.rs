use super::terminal_alacritty::AlacrittyTermLock;
use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct TerminalMenuContext {
    pub(super) has_session: bool,
    pub(super) has_selection: bool,
    pub(super) connected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TerminalTextMetrics {
    pub(super) font_size: Pixels,
    pub(super) line_height: Pixels,
    pub(super) cell_width: Pixels,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TerminalGridSize {
    pub(super) rows: u16,
    pub(super) cols: u16,
    pub(super) pixel_width: u16,
    pub(super) pixel_height: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TerminalLayoutKey {
    pub(super) font_size_bits: u32,
    pub(super) line_height_bits: u32,
    pub(super) cell_width_bits: u32,
}

#[derive(Clone, Debug)]
pub(super) struct TerminalLayoutCache {
    pub(super) rem_size: Pixels,
    pub(super) key: TerminalLayoutKey,
    pub(super) base_style: gpui::TextStyle,
    pub(super) metrics: TerminalTextMetrics,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TerminalCachedRow {
    pub(super) fingerprint: u64,
    pub(super) layout_key: TerminalLayoutKey,
    pub(super) shaped: Option<ShapedLine>,
    pub(super) background_rects: Vec<super::terminal_alacritty::TerminalBackgroundRect>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TerminalViewportCacheKey {
    pub(super) content_epoch: u64,
    pub(super) scrollback: usize,
    pub(super) rows: u16,
    pub(super) cols: u16,
    pub(super) layout_key: TerminalLayoutKey,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TerminalRenderCache {
    pub(super) viewport_key: Option<TerminalViewportCacheKey>,
    pub(super) rows: Vec<TerminalCachedRow>,
}

pub(super) struct TerminalViewportView {
    pub(super) theme: AppTheme,
    pub(super) focus_handle: FocusHandle,
    pub(super) term_lock: Option<AlacrittyTermLock>,
    pub(super) pty_sender: Option<super::terminal_alacritty::PtySender>,
    pub(super) layout_cache: Option<TerminalLayoutCache>,
    pub(super) render_cache: TerminalRenderCache,
    pub(super) cursor_blink_visible: bool,
    pub(super) cursor_blink_hold_until: Instant,
    pub(super) cursor_blink_active: bool,
    pub(super) cursor_blink_task_scheduled: bool,
    pub(super) cursor_blink_seq: u64,
    pub(super) content_epoch: u64,
    pub(super) last_content: Option<super::terminal_alacritty::TerminalContent>,
    pub(super) viewport_bounds: Option<Bounds<Pixels>>,
    pub(super) pressed_mouse_button: Option<gpui::MouseButton>,
    /// Last grid cell reported to the PTY for mouse-motion tracking. Used to
    /// dedupe motion reports so a TUI in any-event mode (1003) receives at most
    /// one report per cell instead of one per pixel-level move event.
    pub(super) last_motion_cell: Option<TerminalGridPoint>,
    pub(super) was_focused: bool,
    /// Selection endpoints in grid coordinates. Note these are *not* rotated
    /// when the PTY emits output: alacritty shifts existing content to
    /// more-negative rows as lines scroll off, so text can slide under a
    /// stationary highlight during a drag. Autoscroll itself is safe because
    /// `scroll_display` moves the viewport, not the content.
    pub(super) selection_start: Option<TerminalGridPoint>,
    pub(super) selection_end: Option<TerminalGridPoint>,
    /// Set by "select all" so Copy grabs the entire buffer through the trimming
    /// `copy_entire_buffer` path. Cleared as soon as a manual selection begins.
    pub(super) select_all_active: bool,
    /// True while the left button is held down for a selection drag. Drives the
    /// window-level `TerminalSelectionTracker` listeners and the autoscroll
    /// ticker, both of which keep working after the pointer leaves the viewport.
    pub(super) selecting: bool,
    /// Most recent pointer position seen during a drag. The autoscroll ticker
    /// re-reads it every frame so scrolling continues while the pointer is held
    /// still outside the viewport.
    pub(super) selection_last_mouse_pos: Point<Pixels>,
    /// Whether the current drag has actually moved (pointer motion, a wheel
    /// scroll, or an autoscroll step). The ticker refuses to re-resolve the free
    /// end until it has: otherwise the first tick after a double- or
    /// triple-click would drag that word/line selection back to the press cell.
    pub(super) selection_drag_moved: bool,
    /// Bumped whenever a drag starts or ends so a stale autoscroll ticker exits.
    pub(super) selection_autoscroll_seq: u64,
    pub(super) ime_state: Option<super::terminal_alacritty::TerminalImeState>,
}

/// A single terminal (one PTY + alacritty + rendered viewport). A repo can hold
/// several of these as tabs.
pub(super) struct TerminalInstance {
    pub(super) focus_handle: FocusHandle,
    pub(super) pty_sender: Option<super::terminal_alacritty::PtySender>,
    pub(super) child_pid: Option<u32>,
    pub(super) events_rx:
        Option<smol::channel::Receiver<super::terminal_alacritty::TerminalBackendEvent>>,
    pub(super) connected: bool,
    pub(super) exit_status: Option<String>,
    pub(super) viewport: Entity<TerminalViewportView>,
    pub(super) session_seq: u64,
    pub(super) title: String,
}

pub(super) struct RepoTerminalSession {
    pub(super) workdir: std::path::PathBuf,
    pub(super) repo_name: String,
    pub(super) instances: Vec<TerminalInstance>,
    pub(super) active_index: usize,
}

impl RepoTerminalSession {
    pub(super) fn active_instance(&self) -> Option<&TerminalInstance> {
        self.instances.get(self.active_index)
    }

    pub(super) fn instance_by_seq_mut(&mut self, seq: u64) -> Option<&mut TerminalInstance> {
        self.instances.iter_mut().find(|i| i.session_seq == seq)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TerminalShutdownSummary {
    pub(crate) terminal_count: usize,
    pub(crate) running_command_count: usize,
    pub(crate) repo_names: Vec<String>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum TerminalShutdownAction {
    CloseRepo { repo_id: RepoId },
    CloseTerminalForRepo { repo_id: RepoId },
    CloseTerminalTab { repo_id: RepoId, index: usize },
    CloseWindow,
    QuitApp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct TerminalShutdownPrompt {
    pub(in crate::view) action: TerminalShutdownAction,
    pub(in crate::view) summary: TerminalShutdownSummary,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TerminalPanelResizeState {
    pub(super) start_y: Pixels,
    pub(super) start_height: Pixels,
}

/// Which content the bottom panel currently shows for a repository, when more
/// than one of its panels (terminal, reflog, …) is open at once. A tab strip
/// only appears once a second panel is available; with just one open, that
/// panel fills the area exactly like before this switcher existed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BottomPanelTab {
    Terminal,
    Reflog,
}

/// A cell in alacritty's grid coordinate space. `row` is a `Line`: `0` is the
/// top of the visible screen at the live tail, and scrollback history is
/// negative down to `-history_size`. Field order matters — the derived `Ord`
/// gives row-major ordering, which is what normalises a selection's
/// `start`/`end` pair.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TerminalGridPoint {
    pub(super) row: i32,
    pub(super) col: u16,
}

impl TerminalGridPoint {
    pub(super) fn new(row: i32, col: u16) -> Self {
        Self { row, col }
    }
}
