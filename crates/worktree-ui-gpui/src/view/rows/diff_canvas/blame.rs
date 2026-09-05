//! Blame annotation column: per-row blame paint data, the fixed sub-column
//! layout, painting and hover/click handling, the annotation hitboxes, and
//! the file editor gutter's blame row canvas.

use super::*;

use super::geometry::{diff_scaled_px, paint_gutter_text};
use super::streamed::hash_shared_string;

/// Default width of the blame/annotate column shown to the left of the diff
/// content when annotate is enabled. The live width is stored on the view and
/// is user-resizable; this is only the initial value and clamp reference.
pub(in crate::view) const DIFF_ANNOTATION_COLUMN_WIDTH_PX: f32 = 300.0;
/// Min/max bounds the annotation column can be dragged to.
pub(in crate::view) const DIFF_ANNOTATION_MIN_WIDTH_PX: f32 = 170.0;
pub(in crate::view) const DIFF_ANNOTATION_MAX_WIDTH_PX: f32 = 640.0;
/// Width of the recency "heat" bar at the far left of the annotation column.
pub(in crate::view) const DIFF_ANNOTATION_BORDER_WIDTH_PX: f32 = 3.0;
/// Gap between annotation sub-columns.
pub(in crate::view) const DIFF_ANNOTATION_GAP_PX: f32 = 6.0;
/// Fixed width of the "X ago" sub-column.
pub(in crate::view) const DIFF_ANNOTATION_WHEN_WIDTH_PX: f32 = 82.0;
/// Fixed width of the author-initials sub-column.
pub(in crate::view) const DIFF_ANNOTATION_INITIALS_WIDTH_PX: f32 = 22.0;
/// Cell width reserved for each trailing action icon.
pub(in crate::view) const DIFF_ANNOTATION_ICON_WIDTH_PX: f32 = 16.0;
/// Rendered (square) size of each action icon within its cell.
pub(in crate::view) const DIFF_ANNOTATION_ICON_GLYPH_PX: f32 = 13.0;
/// Diameter of the "viewing file at this commit" dot in the trailing slot.
pub(in crate::view) const DIFF_ANNOTATION_DOT_DIAMETER_PX: f32 = 6.0;

pub(in crate::view) const DIFF_ANNOTATION_PRIOR_ICON: &str = "icons/undo.svg";
pub(in crate::view) const DIFF_ANNOTATION_BROWSE_ICON: &str = "icons/history.svg";

/// Per-row blame data prepared for painting in the annotation column.
#[derive(Clone)]
pub(in crate::view) struct RowBlamePaint {
    /// Recency "heat" color for the left border bar (older → newer).
    pub(in crate::view) border: gpui::Rgba,
    /// Whether to paint the textual annotation + action icons. `false` on
    /// interior lines of a same-commit run (only the border bar is painted).
    pub(in crate::view) show_text: bool,
    /// Relative time ("3 days ago").
    pub(in crate::view) when: SharedString,
    /// Author initials.
    pub(in crate::view) initials: SharedString,
    /// Commit summary (truncated with an ellipsis at paint time).
    pub(in crate::view) summary: SharedString,
    /// Commit message body (everything after the first line), used for tooltips.
    pub(in crate::view) body: Option<SharedString>,
    /// Commit attributed to this line, used for click handling.
    pub(in crate::view) commit_id: worktree_core::domain::CommitId,
    /// File path of the annotated view, used by the "view prior change" action.
    pub(in crate::view) path: std::sync::Arc<std::path::Path>,
    /// The file's path at `commit_id` when it differs from `path` because the
    /// file was renamed at/after that commit. Navigation actions ("view file at
    /// this commit" / "prior revision") use this historical name so they don't
    /// look up the current path in an older tree where it doesn't exist.
    pub(in crate::view) source_path: Option<std::sync::Arc<std::path::Path>>,
    /// Whether the file existed in this commit's parent. When `false`, the
    /// "view file at parent commit" icon is hidden and non-interactive (the
    /// commit introduced the file, so there is no prior revision to show).
    pub(in crate::view) prior_exists: bool,
    /// For an uncommitted ("Now") line, the base revision the working-tree change
    /// was made against (git blame porcelain `previous`). When `Some`, the
    /// "view file at parent commit" icon is shown on the local-change row and
    /// navigating opens that revision directly. `None` for committed lines (which
    /// resolve their parent from `commit_id`) and for newly-added files.
    pub(in crate::view) prior_commit: Option<worktree_core::domain::CommitId>,
    /// Whether this line's commit is the revision currently being viewed. When
    /// `true`, the "view file at this commit" icon is hidden and non-interactive
    /// (navigating there would be a no-op).
    pub(in crate::view) is_viewed_commit: bool,
}

/// Fixed sub-column geometry for the annotation column, shared by painting and
/// hit-testing so they stay in sync.
pub(in crate::view) struct BlameColumnLayout {
    pub(in crate::view) border: Bounds<Pixels>,
    pub(in crate::view) when_x: Pixels,
    pub(in crate::view) initials_x: Pixels,
    pub(in crate::view) message: Bounds<Pixels>,
    pub(in crate::view) prior_icon: Bounds<Pixels>,
    pub(in crate::view) browse_icon: Bounds<Pixels>,
}

pub(in crate::view) fn blame_column_layout(
    column_left: Pixels,
    column_width: Pixels,
    row_top: Pixels,
    row_height: Pixels,
    ui_scale_percent: u32,
) -> BlameColumnLayout {
    let border_w = diff_scaled_px(DIFF_ANNOTATION_BORDER_WIDTH_PX, ui_scale_percent);
    let gap = diff_scaled_px(DIFF_ANNOTATION_GAP_PX, ui_scale_percent);
    let when_w = diff_scaled_px(DIFF_ANNOTATION_WHEN_WIDTH_PX, ui_scale_percent);
    let initials_w = diff_scaled_px(DIFF_ANNOTATION_INITIALS_WIDTH_PX, ui_scale_percent);
    let icon_w = diff_scaled_px(DIFF_ANNOTATION_ICON_WIDTH_PX, ui_scale_percent);

    let right = column_left + column_width;
    let cell = |x: Pixels, w: Pixels| Bounds::new(point(x, row_top), size(w, row_height));

    let browse_x = right - gap - icon_w;
    let prior_x = browse_x - gap - icon_w;
    let when_x = column_left + border_w + gap;
    let initials_x = when_x + when_w + gap;
    let message_x = initials_x + initials_w + gap;
    let message_w = (prior_x - gap - message_x).max(px(0.0));

    BlameColumnLayout {
        border: cell(column_left, border_w),
        when_x,
        initials_x,
        message: cell(message_x, message_w),
        prior_icon: cell(prior_x, icon_w),
        browse_icon: cell(browse_x, icon_w),
    }
}

/// Fold blame content into a row's canvas revision key so the cached canvas
/// repaints when the blame attribution (color/text/commit/width) or this row's
/// own hover highlight changes.
pub(super) fn mix_blame_revision(
    base: u64,
    annotation_width: Pixels,
    hover: Option<AnnotArea>,
    blame: Option<&RowBlamePaint>,
) -> u64 {
    let mut hasher = FxHasher::default();
    base.hash(&mut hasher);
    f32::from(annotation_width).to_bits().hash(&mut hasher);
    // Only this row's own hover state feeds the cache key (not the raw cursor
    // position), so the cached canvas is invalidated only when the highlighted
    // sub-area of this specific row changes — not on every mouse move.
    hover.hash(&mut hasher);
    if let Some(blame) = blame {
        hash_rgba(&mut hasher, blame.border);
        blame.show_text.hash(&mut hasher);
        // `when` is time-relative ("3 days ago") so it changes for the same commit
        // as time passes — it must stay in the key. `initials`, `summary` and
        // `body` are NOT hashed: they are pure functions of `commit_id` (a given
        // commit always yields the same author/summary/body), so the commit id
        // hashed below already covers them. This avoids re-hashing a potentially
        // multi-KB commit body for every annotated row on every frame.
        hash_shared_string(&mut hasher, &blame.when);
        blame.commit_id.0.as_ref().hash(&mut hasher);
        blame.prior_exists.hash(&mut hasher);
        blame
            .prior_commit
            .as_ref()
            .map(|c| c.0.as_ref())
            .hash(&mut hasher);
        // Folded in so the trailing slot repaints when the viewed revision
        // changes (it toggles the browse icon vs. the "viewing here" dot).
        blame.is_viewed_commit.hash(&mut hasher);
    } else {
        u8::MAX.hash(&mut hasher);
    }
    hasher.finish()
}

/// Paint a single line of text truncated to `max_width` with a trailing "…".
fn paint_truncated_text(
    text: &SharedString,
    x: Pixels,
    y: Pixels,
    max_width: Pixels,
    color: gpui::Rgba,
    metrics: LineMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() || max_width <= px(0.0) {
        return;
    }
    let mut style = diff_text_style(window);
    style.color = color.into_color();
    let runs = vec![style.to_run(text.len())];
    let mut wrapper = window
        .text_system()
        .line_wrapper(style.font(), metrics.font_size);
    let (truncated, runs) =
        wrapper.truncate_line(text.clone(), max_width, "…", &runs, TruncateFrom::End);
    let shaped = window
        .text_system()
        .shape_line(truncated, metrics.font_size, runs.as_ref(), None);
    let _ = shaped.paint(
        point(x, y),
        metrics.line_height,
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

/// Whether a blame entry points at a real commit. Working-tree blame surfaces
/// uncommitted lines with an empty or all-zero object id ("Not Committed Yet");
/// those have no commit to open, so their action icons and click handlers are
/// suppressed.
fn blame_commit_is_navigable(commit_id: &worktree_core::domain::CommitId) -> bool {
    !commit_id.is_uncommitted()
}

/// Paint the annotation column for one row: a recency border bar and, on run
/// starts, the "X ago | initials | summary" sub-columns plus two action icons.
/// When `hovered` is Some, that area gets a highlight + underline (message) or
/// brighter color (icons).
#[allow(clippy::too_many_arguments)]
fn paint_blame_annotation(
    blame: &RowBlamePaint,
    layout: &BlameColumnLayout,
    y: Pixels,
    text_color: gpui::Rgba,
    theme: AppTheme,
    metrics: LineMetrics,
    when_metrics: LineMetrics,
    hovered: Option<AnnotArea>,
    prior_enabled: bool,
    browse_enabled: bool,
    ui_scale_percent: u32,
    window: &mut Window,
    cx: &mut App,
) {
    window.paint_quad(fill(layout.border, blame.border));

    if !blame.show_text {
        return;
    }

    paint_gutter_text(
        &blame.when,
        layout.when_x,
        y,
        text_color,
        when_metrics,
        window,
        cx,
    );
    paint_gutter_text(
        &blame.initials,
        layout.initials_x,
        y,
        text_color,
        metrics,
        window,
        cx,
    );
    window.paint_layer(layout.message, |window| {
        let message_color = if hovered == Some(AnnotArea::Message) {
            theme.colors.accent.foreground
        } else {
            text_color
        };
        paint_truncated_text(
            &blame.summary,
            layout.message.left(),
            y,
            layout.message.size.width,
            message_color,
            metrics,
            window,
            cx,
        );
        if hovered == Some(AnnotArea::Message) {
            let underline_y = y + metrics.line_height + px(0.5);
            window.paint_quad(fill(
                Bounds::new(
                    point(layout.message.left(), underline_y),
                    size(layout.message.size.width, px(1.0)),
                ),
                theme.colors.accent.foreground,
            ));
        }
    });

    // The "view file at parent commit" icon. Shown for committed lines whose
    // parent has the file, and for uncommitted ("Now") lines that carry a base
    // revision (the committed state before the local change).
    if prior_enabled {
        let icon_color = if hovered == Some(AnnotArea::PriorIcon) {
            theme.colors.accent.foreground
        } else {
            crate::theme::with_alpha(text_color, 0.6)
        };
        paint_blame_icon(
            DIFF_ANNOTATION_PRIOR_ICON,
            layout.prior_icon,
            icon_color,
            ui_scale_percent,
            window,
            cx,
        );
    }
    if blame.is_viewed_commit {
        // The file is currently open at this line's commit: mark it with a dot in
        // the trailing slot (where the browse icon would otherwise sit) instead of
        // the "view file at this commit" icon, which would be a no-op here.
        paint_blame_dot(
            layout.browse_icon,
            crate::theme::with_alpha(theme.colors.accent.foreground, 0.7),
            ui_scale_percent,
            window,
        );
    } else if browse_enabled {
        let icon_color = if hovered == Some(AnnotArea::BrowseIcon) {
            theme.colors.accent.foreground
        } else {
            crate::theme::with_alpha(text_color, 0.6)
        };
        paint_blame_icon(
            DIFF_ANNOTATION_BROWSE_ICON,
            layout.browse_icon,
            icon_color,
            ui_scale_percent,
            window,
            cx,
        );
    }
}

/// Paint an action icon centered within its cell.
fn paint_blame_icon(
    path: &'static str,
    cell: Bounds<Pixels>,
    color: gpui::Rgba,
    ui_scale_percent: u32,
    window: &mut Window,
    cx: &mut App,
) {
    paint_centered_svg_icon(
        path,
        cell,
        diff_scaled_px(DIFF_ANNOTATION_ICON_GLYPH_PX, ui_scale_percent),
        color,
        window,
        cx,
    );
}

/// Paint `path` as a square icon of `glyph` size, centered in `cell` and clamped
/// so it never spills out of it.
pub(in crate::view::rows) fn paint_centered_svg_icon(
    path: &'static str,
    cell: Bounds<Pixels>,
    glyph: Pixels,
    color: gpui::Rgba,
    window: &mut Window,
    cx: &mut App,
) {
    let glyph = glyph.min(cell.size.width).min(cell.size.height);
    let ox = cell.left() + (cell.size.width - glyph) * 0.5;
    let oy = cell.top() + (cell.size.height - glyph) * 0.5;
    let bounds = Bounds::new(point(ox, oy), size(glyph, glyph));
    let _ = window.paint_svg(
        bounds,
        path.into(),
        None,
        TransformationMatrix::unit(),
        color.into_color(),
        cx,
    );
}

/// Paint a small filled dot centered within `cell`, marking the line's commit as
/// the revision the file is currently being viewed at.
fn paint_blame_dot(
    cell: Bounds<Pixels>,
    color: gpui::Rgba,
    ui_scale_percent: u32,
    window: &mut Window,
) {
    let diameter = diff_scaled_px(DIFF_ANNOTATION_DOT_DIAMETER_PX, ui_scale_percent)
        .min(cell.size.width)
        .min(cell.size.height);
    let ox = cell.left() + (cell.size.width - diameter) * 0.5;
    let oy = cell.top() + (cell.size.height - diameter) * 0.5;
    let bounds = Bounds::new(point(ox, oy), size(diameter, diameter));
    window.paint_quad(fill(bounds, color).corner_radii(diameter * 0.5));
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::view) enum AnnotArea {
    Message,
    PriorIcon,
    BrowseIcon,
}

/// Hitboxes for annotation sub-column interactive areas.
#[derive(Clone, Debug)]
pub(super) struct AnnotHitboxes {
    message: Hitbox,
    prior_icon: Hitbox,
    browse_icon: Hitbox,
}

/// Register click handling for a row's annotation column.
#[allow(clippy::too_many_arguments)]
fn install_blame_annotation_mouse_handler(
    window: &mut Window,
    view: &Entity<MainPaneView>,
    message_hitbox: &Hitbox,
    prior_icon_hitbox: &Hitbox,
    browse_icon_hitbox: &Hitbox,
    commit_id: worktree_core::domain::CommitId,
    path: std::sync::Arc<std::path::Path>,
    source_path: Option<std::sync::Arc<std::path::Path>>,
    prior_commit: Option<worktree_core::domain::CommitId>,
    message_enabled: bool,
    prior_enabled: bool,
    browse_enabled: bool,
) {
    window.on_mouse_event({
        let view = view.clone();
        let message_hitbox = message_hitbox.clone();
        let prior_icon_hitbox = prior_icon_hitbox.clone();
        let browse_icon_hitbox = browse_icon_hitbox.clone();
        move |event: &gpui::MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != gpui::MouseButton::Left {
                return;
            }
            let commit_id = commit_id.clone();
            let path = path.clone();
            // For renamed files, navigate to the historical name at this commit
            // rather than the current path (which may not exist in that tree).
            let historical_path = source_path
                .as_deref()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| path.to_path_buf());
            // `is_hovered`, not `contains`: this is a window-level listener, so
            // it runs even for clicks that landed on something painted over the
            // diff. Only the hit test knows what actually owns the pointer.
            let action = if browse_enabled && browse_icon_hitbox.is_hovered(window) {
                BlameClickAction::Browse
            } else if prior_enabled && prior_icon_hitbox.is_hovered(window) {
                BlameClickAction::PriorRevision
            } else if message_enabled && message_hitbox.is_hovered(window) {
                BlameClickAction::OpenDetails
            } else {
                return;
            };
            let prior_commit = prior_commit.clone();
            view.update(cx, |this, cx| {
                let Some(repo_id) = this.active_repo_id() else {
                    return;
                };
                let msg = match action {
                    BlameClickAction::Browse => Msg::OpenFileAtCommit {
                        repo_id,
                        commit_id,
                        path: historical_path,
                    },
                    // An uncommitted ("Now") line's prior is the base revision it
                    // was edited from: open that commit directly. A committed line
                    // resolves and opens its commit's parent.
                    BlameClickAction::PriorRevision => match prior_commit {
                        Some(base) => Msg::OpenFileAtCommit {
                            repo_id,
                            commit_id: base,
                            path: historical_path,
                        },
                        None => Msg::OpenFileAtCommitParent {
                            repo_id,
                            commit_id,
                            path: historical_path,
                        },
                    },
                    BlameClickAction::OpenDetails => Msg::SelectCommit { repo_id, commit_id },
                };
                this.store.dispatch(msg);
                cx.notify();
            });
        }
    });
}

enum BlameClickAction {
    OpenDetails,
    PriorRevision,
    Browse,
}

/// Paint the annotation column for one row: border bar, text, icons, and
/// hover effects. Installs click handlers and sets cursor + tooltip via hitboxes.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_blame_column(
    blame: &RowBlamePaint,
    row_bounds: Bounds<Pixels>,
    annot_w: Pixels,
    y: Pixels,
    theme: AppTheme,
    metrics: LineMetrics,
    when_metrics: LineMetrics,
    ui_scale_percent: u32,
    visible_ix: usize,
    annot_hitboxes: Option<&AnnotHitboxes>,
    view: &Entity<MainPaneView>,
    window: &mut Window,
    cx: &mut App,
) {
    let layout = blame_column_layout(
        row_bounds.left(),
        annot_w,
        row_bounds.top(),
        row_bounds.size.height,
        ui_scale_percent,
    );

    let navigable = blame_commit_is_navigable(&blame.commit_id);
    // "View file at parent commit": committed lines whose parent has the file, or
    // uncommitted ("Now") lines carrying a base revision (the state before the
    // local change).
    let prior_enabled = (blame.prior_exists && navigable) || blame.prior_commit.is_some();
    // The commit message opens commit details — only for real commits.
    let message_enabled = navigable;
    // Hide "view file at this commit" when already viewing that commit (a dot is
    // painted there instead) and for uncommitted lines (no commit to browse).
    let browse_enabled = navigable && !blame.is_viewed_commit;
    // Drive the hover highlight from stored hover state (updated by the hover
    // handler on real hover transitions) rather than the live cursor position,
    // so this matches the value folded into the canvas revision key.
    let hovered = view
        .read(cx)
        .blame_annot_hover
        .and_then(|(ix, area)| (ix == visible_ix).then_some(area));

    if let Some(hb) = annot_hitboxes {
        if message_enabled {
            window.set_cursor_style(CursorStyle::PointingHand, &hb.message);
        }
        if prior_enabled {
            window.set_cursor_style(CursorStyle::PointingHand, &hb.prior_icon);
        }
        if browse_enabled {
            window.set_cursor_style(CursorStyle::PointingHand, &hb.browse_icon);
        }
    }

    paint_blame_annotation(
        blame,
        &layout,
        y,
        theme.colors.foreground.secondary,
        theme,
        metrics,
        when_metrics,
        hovered,
        prior_enabled,
        browse_enabled,
        ui_scale_percent,
        window,
        cx,
    );

    if blame.show_text
        && (message_enabled || prior_enabled || browse_enabled)
        && let Some(hb) = annot_hitboxes
    {
        install_blame_annotation_mouse_handler(
            window,
            view,
            &hb.message,
            &hb.prior_icon,
            &hb.browse_icon,
            blame.commit_id.clone(),
            blame.path.clone(),
            blame.source_path.clone(),
            blame.prior_commit.clone(),
            message_enabled,
            prior_enabled,
            browse_enabled,
        );
        install_blame_annotation_hover_handler(
            window,
            view,
            visible_ix,
            hb,
            blame.summary.clone(),
            blame.body.clone(),
            message_enabled,
            prior_enabled,
            browse_enabled,
        );
    }
}

/// Register hover handling for a row's annotation column. On every mouse move it
/// resolves which sub-area (message / prior icon / browse icon) the cursor is
/// over and, only when that changes for this row, updates the view's hover state
/// (which drives the accent highlight on repaint) and the shared tooltip host.
fn install_blame_annotation_hover_handler(
    window: &mut Window,
    view: &Entity<MainPaneView>,
    visible_ix: usize,
    hitboxes: &AnnotHitboxes,
    summary: SharedString,
    body: Option<SharedString>,
    message_enabled: bool,
    prior_enabled: bool,
    browse_enabled: bool,
) {
    window.on_mouse_event({
        let view = view.clone();
        let message = hitboxes.message.clone();
        let prior_icon = hitboxes.prior_icon.clone();
        let browse_icon = hitboxes.browse_icon.clone();
        move |_event: &gpui::MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // Hover follows the hit test, so a panel painted over the diff hides
            // the annotations underneath instead of highlighting them through it.
            let area = if message_enabled && message.is_hovered(window) {
                Some(AnnotArea::Message)
            } else if prior_enabled && prior_icon.is_hovered(window) {
                Some(AnnotArea::PriorIcon)
            } else if browse_enabled && browse_icon.is_hovered(window) {
                Some(AnnotArea::BrowseIcon)
            } else {
                None
            };

            // Cheap gate so plain mouse movement doesn't borrow/notify the view
            // for every visible row: only act when this row's hover changes, and
            // never clear a hover that belongs to a different row.
            let next = area.map(|a| (visible_ix, a));
            let current = view.read(cx).blame_annot_hover;
            if current == next {
                return;
            }
            if next.is_none() && !matches!(current, Some((ix, _)) if ix == visible_ix) {
                return;
            }

            let tooltip = match area {
                Some(AnnotArea::Message) => Some(body.clone().unwrap_or_else(|| summary.clone())),
                Some(AnnotArea::PriorIcon) => Some(crate::i18n::tr("misc.diff_canvas.view_prior")),
                Some(AnnotArea::BrowseIcon) => {
                    Some(crate::i18n::tr("misc.diff_canvas.view_at_commit"))
                }
                None => None,
            };

            view.update(cx, |this, cx| {
                this.update_blame_annot_hover(next, tooltip, cx);
            });
        }
    });
}

/// The blame column for one row of the file editor's gutter.
///
/// The editor's gutter is otherwise plain divs, but blame is not a label — it
/// has a hover highlight, a tooltip carrying the commit body, and three click
/// targets (the message opens commit details, the two icons walk to the file at
/// that commit and at its parent). All of that lives in [`render_blame_column`],
/// so the gutter hands it a canvas sized to exactly the annotation column and
/// gets the same behaviour the diff and preview have.
///
/// A row whose `blame.show_text` is false paints only its recency bar and
/// installs no handlers — which is how the editor represents blame it cannot
/// stand behind, on the interior lines of a same-commit run and over a buffer
/// with unsaved edits.
pub(in crate::view) fn blame_gutter_row_canvas(
    theme: AppTheme,
    view: Entity<MainPaneView>,
    ui_scale_percent: u32,
    visual_ix: usize,
    row_height: Pixels,
    annotation_width: Pixels,
    annot_hover: Option<(usize, AnnotArea)>,
    blame: Option<RowBlamePaint>,
) -> AnyElement {
    // Same revision key the diff rows build, for the same reason: GPUI keys
    // element state by id, so a canvas identified by its row index alone keeps
    // last frame's hitboxes and hover after the blame under it changes.
    let row_hover = annot_hover.and_then(|(ix, area)| (ix == visual_ix).then_some(area));
    let revision = mix_blame_revision(0, annotation_width, row_hover, blame.as_ref());
    keyed_canvas(
        (
            gpui::ElementId::from(("file_editor_blame_row_canvas", visual_ix)),
            format!("{revision:016x}"),
        ),
        move |bounds, window, _cx| {
            build_annot_hitboxes(window, bounds, annotation_width, ui_scale_percent)
        },
        move |bounds, annot_hitboxes, window, cx| {
            let line_metrics = line_metrics(window);
            let y = center_text_y(bounds, line_metrics.line_height);

            // The whole canvas *is* the annotation column, so it takes the
            // sidebar colour the diff and preview rows give theirs — not the
            // editor canvas, which left the strip with no edge at all.
            window.paint_quad(fill(bounds, theme.colors.surface.panel));

            if let Some(blame) = &blame {
                let when_metrics = line_metrics_annot_when(window);
                render_blame_column(
                    blame,
                    bounds,
                    annotation_width,
                    y,
                    theme,
                    line_metrics,
                    when_metrics,
                    ui_scale_percent,
                    visual_ix,
                    annot_hitboxes.as_ref(),
                    &view,
                    window,
                    cx,
                );
            }
        },
    )
    .w(annotation_width)
    .h(row_height)
    .into_any_element()
}

pub(super) fn build_annot_hitboxes(
    window: &mut Window,
    row_bounds: Bounds<Pixels>,
    annot_w: Pixels,
    ui_scale_percent: u32,
) -> Option<AnnotHitboxes> {
    if annot_w <= px(0.0) {
        return None;
    }
    let layout = blame_column_layout(
        row_bounds.left(),
        annot_w,
        row_bounds.top(),
        row_bounds.size.height,
        ui_scale_percent,
    );
    let clip = window.content_mask().bounds;
    Some(AnnotHitboxes {
        message: window.insert_hitbox(layout.message.intersect(&clip), HitboxBehavior::Normal),
        prior_icon: window
            .insert_hitbox(layout.prior_icon.intersect(&clip), HitboxBehavior::Normal),
        browse_icon: window
            .insert_hitbox(layout.browse_icon.intersect(&clip), HitboxBehavior::Normal),
    })
}
