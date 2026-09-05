//! Stage gutter: the hover-revealed stage/unstage button over a change
//! row's line-number gutter — its specs, prepaint/paint, hover handling, and
//! the click routing it feeds into the row mouse handlers.

use super::*;

use super::geometry::{diff_row_horizontal_padding, diff_scaled_px};

/// Size of the hover-revealed stage/unstage button painted over the empty slack
/// at the far left of a change row. It overlays the line-number gutter rather
/// than reserving a column of its own, so enabling it never shifts diff text.
const DIFF_STAGE_GUTTER_CELL_PX: f32 = 16.0;
/// Rendered (square) size of the icon within the button.
const DIFF_STAGE_GUTTER_GLYPH_PX: f32 = 11.0;
const DIFF_STAGE_GUTTER_STAGE_ICON: &str = "icons/plus.svg";
const DIFF_STAGE_GUTTER_UNSTAGE_ICON: &str = "icons/minus.svg";

/// Fold a row's stage-gutter button into its canvas revision key so the cached
/// canvas repaints when the button appears, disappears, or flips direction
/// (staging vs. unstaging). `hovered` is this row's own stored hover state, not
/// the raw cursor position, so mouse movement invalidates only the two rows
/// whose button visibility actually changed.
pub(super) fn mix_stage_gutter_revision(
    base: u64,
    specs: &[Option<StageGutterSpec>],
    stage_hover: Option<DiffStageHover>,
    visible_ix: usize,
) -> u64 {
    if specs.iter().all(Option::is_none) {
        return base;
    }
    let mut hasher = FxHasher::default();
    base.hash(&mut hasher);
    for spec in specs {
        match spec {
            Some(spec) => {
                spec.area.hash(&mut hasher);
                spec.slot.hash(&mut hasher);
                // The kind decides which patch line a click resolves to, and the
                // paint closure captures it, so a cached canvas must not outlive
                // a change to it.
                std::mem::discriminant(&spec.kind).hash(&mut hasher);
                stage_hover
                    .filter(|hover| hover.visible_ix == visible_ix && hover.slot == spec.slot)
                    .map(|hover| hover.on_button)
                    .hash(&mut hasher);
            }
            None => u8::MAX.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Which column's left gutter a stage/unstage button belongs to. Split views
/// paint one button per column for the same row, so the row index alone cannot
/// identify a button.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(in crate::view) enum DiffStageSlot {
    Inline,
    SplitLeft,
    SplitRight,
}

/// The stage-gutter button the pointer is currently on or near. Hovering
/// anywhere in the row reveals its button; hovering the button itself is tracked
/// separately so it can render brighter and show a tooltip.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(in crate::view) struct DiffStageHover {
    pub(in crate::view) visible_ix: usize,
    pub(in crate::view) slot: DiffStageSlot,
    pub(in crate::view) on_button: bool,
}

/// What a row's stage-gutter button does. Rows that get no button (context
/// lines, headers, diffs that are not worktree diffs) pass `None` instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct StageGutterSpec {
    /// Which side of the index the shown diff is: an unstaged diff stages the
    /// line, a staged diff unstages it.
    pub(in crate::view) area: DiffArea,
    pub(in crate::view) slot: DiffStageSlot,
    /// The change this button acts on, used to resolve the row back to a single
    /// patch source line on click.
    pub(in crate::view) kind: DiffLineKind,
}

impl StageGutterSpec {
    fn icon(self) -> &'static str {
        match self.area {
            DiffArea::Unstaged => DIFF_STAGE_GUTTER_STAGE_ICON,
            DiffArea::Staged => DIFF_STAGE_GUTTER_UNSTAGE_ICON,
        }
    }

    fn color(self, theme: AppTheme) -> gpui::Rgba {
        match self.area {
            DiffArea::Unstaged => theme.colors.diff.added.foreground,
            DiffArea::Staged => theme.colors.diff.removed.foreground,
        }
    }

    fn tooltip(self) -> SharedString {
        match self.area {
            DiffArea::Unstaged => SharedString::from("Stage line"),
            DiffArea::Staged => SharedString::from("Unstage line"),
        }
    }
}

/// Cell the stage/unstage button occupies: the far left of a row, which with
/// line numbers shown is the empty slack ahead of the right-aligned number.
/// Shared by painting and hit-testing so the two cannot drift apart.
///
/// With line numbers hidden there is no such slack — the diff text starts here
/// — so the button overlaps its first characters and takes the clicks landing on
/// them. That is deliberate: reaching the button everywhere is worth more than
/// the couple of characters it sits on, and the chip is painted opaque so what
/// it covers reads as covered rather than as garbled text.
fn stage_gutter_cell(
    content_left: Pixels,
    row_top: Pixels,
    row_height: Pixels,
    ui_scale_percent: u32,
) -> Bounds<Pixels> {
    let pad = diff_row_horizontal_padding(ui_scale_percent);
    let width = diff_scaled_px(DIFF_STAGE_GUTTER_CELL_PX, ui_scale_percent);
    let height = width.min(row_height);
    Bounds::new(
        point(
            content_left + pad * 0.5,
            row_top + (row_height - height) * 0.5,
        ),
        size(width, height),
    )
}

#[derive(Clone, Debug)]
pub(super) struct StageGutterPrepaint {
    spec: StageGutterSpec,
    cell: Bounds<Pixels>,
    hitbox: Hitbox,
}

/// Reserve the button's hitbox during prepaint. Called after the text hitbox so
/// the button's pointer cursor — and its clicks — win over the text I-beam, and
/// clipped to the content mask so a scrolled-away button cannot be hovered.
pub(super) fn build_stage_gutter(
    window: &mut Window,
    spec: Option<StageGutterSpec>,
    content_left: Pixels,
    row_bounds: Bounds<Pixels>,
    ui_scale_percent: u32,
) -> Option<StageGutterPrepaint> {
    let spec = spec?;
    let cell = stage_gutter_cell(
        content_left,
        row_bounds.top(),
        row_bounds.size.height,
        ui_scale_percent,
    );
    let visible = cell.intersect(&window.content_mask().bounds);
    if visible.size.width <= px(0.0) || visible.size.height <= px(0.0) {
        return None;
    }
    Some(StageGutterPrepaint {
        spec,
        cell,
        hitbox: window.insert_hitbox(visible, HitboxBehavior::Normal),
    })
}

/// Paint a row's stage/unstage button and register its hover handling, returning
/// the click routing for it. Hovering anywhere in the row reveals the button;
/// hovering the button itself brightens it. The hover state comes from the view
/// (never from the live cursor) so it matches the value folded into the canvas
/// revision key. Clicks are handled by `install_diff_row_mouse_handlers`.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_stage_gutter(
    prepaint: Option<&StageGutterPrepaint>,
    visible_ix: usize,
    theme: AppTheme,
    row_bg: gpui::Rgba,
    ui_scale_percent: u32,
    row_hitbox: &Hitbox,
    column_bounds: Option<Bounds<Pixels>>,
    view: &Entity<MainPaneView>,
    window: &mut Window,
    cx: &mut App,
) -> Option<StageGutterMouse> {
    let prepaint = prepaint?;
    let spec = prepaint.spec;
    window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);

    let hover = view.update(cx, |this, _cx| {
        this.set_diff_stage_gutter_cell(visible_ix, spec.slot, prepaint.cell);
        this.diff_stage_gutter_hover
            .filter(|hover| hover.visible_ix == visible_ix && hover.slot == spec.slot)
    });

    if let Some(hover) = hover {
        let color = spec.color(theme);
        // The chip is opaque so it masks any line-number digits it covers. Over
        // the row it stays quiet; under the pointer it takes a tint of the
        // action it performs.
        let (chip, icon) = if hover.on_button {
            (
                crate::theme::composite_over(row_bg, with_alpha(color, 0.22)),
                color,
            )
        } else {
            (row_bg, with_alpha(color, 0.75))
        };
        window.paint_quad(fill(prepaint.cell, chip).corner_radii(px(theme.radii.control)));
        paint_centered_svg_icon(
            spec.icon(),
            prepaint.cell,
            diff_scaled_px(DIFF_STAGE_GUTTER_GLYPH_PX, ui_scale_percent),
            icon,
            window,
            cx,
        );
    }

    install_stage_gutter_hover_handler(
        window,
        view,
        visible_ix,
        spec,
        &prepaint.hitbox,
        row_hitbox,
        column_bounds,
    );

    Some(StageGutterMouse {
        hitbox: prepaint.hitbox.clone(),
        kind: spec.kind,
    })
}

/// Register hover handling for a row's stage-gutter button: on every mouse move
/// it resolves whether the cursor is over this row (which reveals the button) and
/// whether it is on the button itself (which brightens it and shows a tooltip),
/// updating the view only when that changes. `column_bounds` narrows the row to
/// one side in split views, where both columns paint their own button.
fn install_stage_gutter_hover_handler(
    window: &mut Window,
    view: &Entity<MainPaneView>,
    visible_ix: usize,
    spec: StageGutterSpec,
    hitbox: &Hitbox,
    row_hitbox: &Hitbox,
    column_bounds: Option<Bounds<Pixels>>,
) {
    window.on_mouse_event({
        let view = view.clone();
        let hitbox = hitbox.clone();
        let row_hitbox = row_hitbox.clone();
        move |event: &gpui::MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // Hover follows the hit test, so a panel painted over the diff hides
            // the button underneath instead of revealing it through the overlay.
            let on_row = row_hitbox.is_hovered(window)
                && column_bounds.is_none_or(|bounds| bounds.contains(&event.position));
            let next = on_row.then(|| DiffStageHover {
                visible_ix,
                slot: spec.slot,
                on_button: hitbox.is_hovered(window),
            });

            // Cheap gate so plain mouse movement doesn't borrow/notify the view
            // for every visible row: only act when this button's hover changes,
            // and never clear a hover that belongs to a different button.
            let current = view.read(cx).diff_stage_gutter_hover;
            if current == next {
                return;
            }
            if next.is_none()
                && !current
                    .is_some_and(|hover| hover.visible_ix == visible_ix && hover.slot == spec.slot)
            {
                return;
            }

            let tooltip = next.filter(|hover| hover.on_button).map(|_| spec.tooltip());
            view.update(cx, |this, cx| {
                this.update_diff_stage_gutter_hover(next, tooltip, cx);
            });
        }
    });
}

/// Click routing for one stage/unstage button.
#[derive(Clone, Debug)]
pub(super) struct StageGutterMouse {
    hitbox: Hitbox,
    kind: DiffLineKind,
}

impl StageGutterMouse {
    pub(super) fn hovered(buttons: &[Self], window: &Window) -> Option<DiffLineKind> {
        buttons
            .iter()
            .find(|button| button.hitbox.is_hovered(window))
            .map(|button| button.kind)
    }
}

/// Row mouse handlers are window-level listeners: they see every event, whatever
/// is painted over the diff. Asking the row's hitbox (rather than its bounds)
/// whether it is hovered defers to the hit test, so a click on a panel floating
/// above the diff — the collapsed sidebar's section popover, say — stops there
/// instead of also landing on the row beneath it.
pub(super) fn should_handle_row_mouse_event(
    phase: DispatchPhase,
    row_hitbox: &Hitbox,
    window: &Window,
) -> bool {
    phase == DispatchPhase::Bubble && row_hitbox.is_hovered(window)
}
