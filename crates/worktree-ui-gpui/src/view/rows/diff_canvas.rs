use super::canvas::keyed_canvas;
use super::diff_text::{
    PreparedDocumentByteRangeHighlights, build_cached_diff_query_overlay_styled_text,
    build_cached_diff_styled_text, build_cached_diff_styled_text_from_relative_highlights,
    hash_rgba_bits as hash_rgba, slice_cached_diff_styled_text,
    syntax_highlights_for_streamed_line_slice_heuristic, whitespace_visible_line_styled_text,
    whitespace_visible_line_styled_text_for_raw, whitespace_visible_line_text,
    whitespace_visible_styled_text,
};
use super::*;
use crate::kit::diff_text_metrics::{
    DIFF_FONT_SCALE, LineMetrics, center_text_y, diff_text_style, line_metrics,
    line_metrics_annot_when, px_2,
};
use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};
use crate::view::panes::main::DiffHorizontalScrollColumn;
use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, HighlightStyle, Hitbox, HitboxBehavior, Pixels,
    Styled, TextRun, TextStyle, TransformationMatrix, TruncateFrom, Window, fill, point, px, size,
};
use palette::IntoColor;
use rustc_hash::{FxHashMap, FxHasher};
use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::sync::OnceLock;
use worktree_core::domain::{DiffArea, DiffLineKind};

mod blame;
mod geometry;
mod stage_gutter;
mod streamed;

// Re-exported at the original `diff_canvas::…` paths (the `rows` module's
// re-export surface and in-tree `diff_canvas::` consumers depend on them).
pub(super) use self::blame::paint_centered_svg_icon;
pub(in crate::view) use self::blame::{
    AnnotArea, DIFF_ANNOTATION_COLUMN_WIDTH_PX, DIFF_ANNOTATION_MAX_WIDTH_PX,
    DIFF_ANNOTATION_MIN_WIDTH_PX, RowBlamePaint, blame_gutter_row_canvas,
};
pub(in crate::view) use self::geometry::{
    diff_change_bar_width, diff_inline_text_start, diff_row_horizontal_padding,
    diff_single_column_text_start, diff_text_wrap_char_width,
};
pub(in crate::view) use self::stage_gutter::{DiffStageHover, DiffStageSlot, StageGutterSpec};
pub(in crate::view) use self::streamed::{
    DiffTextWrapSlice, DiffWrapByteRange, is_streamable_diff_text,
    whitespace_visible_diff_offset_map,
};
pub(super) use self::streamed::{StreamedDiffTextPaintSpec, StreamedDiffTextSyntaxSource};
// Bridge for the domains still living inline in this file; each shrinks away
// as its consumers move into their own child modules.
use self::blame::{AnnotHitboxes, build_annot_hitboxes, mix_blame_revision, render_blame_column};
use self::geometry::{
    DIFF_CHANGE_BAR_WIDTH_PX, DIFF_ROW_TEXT_TRAILING_PADDING_PX, column_text_bounds,
    diff_row_height, diff_scaled_px, gutter_cell_total_width, inline_text_bounds, inset_left,
    paint_gutter_text_right_aligned, row_bg_fill_bounds, single_column_text_bounds, split_columns,
};
use self::stage_gutter::{
    StageGutterMouse, StageGutterPrepaint, build_stage_gutter, mix_stage_gutter_revision,
    paint_stage_gutter, should_handle_row_mouse_event,
};
use self::streamed::{
    STREAMED_DIFF_TEXT_OVERSCAN_COLUMNS, build_streamed_diff_slice_styled_text,
    diff_text_paint_payload, hash_shared_string, should_stream_diff_text,
    streamed_diff_text_ascii_cell_width, streamed_diff_text_visible_slice_range,
};

/// Paint the neutral sidebar quad behind the annotation column (width `annot_w`)
/// so selection tint does not bleed into it.
fn paint_annotation_sidebar(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    annot_w: Pixels,
    sidebar_bg: gpui::Rgba,
) {
    window.paint_quad(fill(
        row_bg_fill_bounds(Bounds::new(
            bounds.origin,
            size(annot_w, bounds.size.height),
        )),
        sidebar_bg,
    ));
}

/// Fill a single-content-area row background, reserving a neutral annotation
/// sidebar (width `annot_w`) at the left. With `annot_w == 0` this simply fills
/// `bounds` with `bg`.
fn paint_row_bg_with_annotation(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    annot_w: Pixels,
    bg: gpui::Rgba,
    sidebar_bg: gpui::Rgba,
) {
    if annot_w > px(0.0) {
        paint_annotation_sidebar(window, bounds, annot_w, sidebar_bg);
        window.paint_quad(fill(row_bg_fill_bounds(inset_left(bounds, annot_w)), bg));
    } else {
        window.paint_quad(fill(row_bg_fill_bounds(bounds), bg));
    }
}

fn inline_row_canvas_revision_key(
    old: &SharedString,
    new: &SharedString,
    bg: gpui::Rgba,
    fg: gpui::Rgba,
    gutter_fg: gpui::Rgba,
    text_hash: u64,
    highlights_hash: u64,
) -> u64 {
    let mut hasher = FxHasher::default();
    hash_shared_string(&mut hasher, old);
    hash_shared_string(&mut hasher, new);
    hash_rgba(&mut hasher, bg);
    hash_rgba(&mut hasher, fg);
    hash_rgba(&mut hasher, gutter_fg);
    text_hash.hash(&mut hasher);
    highlights_hash.hash(&mut hasher);
    hasher.finish()
}

#[allow(clippy::too_many_arguments)]
fn split_row_canvas_revision_key(
    old: &SharedString,
    new: &SharedString,
    left_bg: gpui::Rgba,
    left_fg: gpui::Rgba,
    left_gutter: gpui::Rgba,
    right_bg: gpui::Rgba,
    right_fg: gpui::Rgba,
    right_gutter: gpui::Rgba,
    left_text_hash: u64,
    left_highlights_hash: u64,
    right_text_hash: u64,
    right_highlights_hash: u64,
) -> u64 {
    let mut hasher = FxHasher::default();
    hash_shared_string(&mut hasher, old);
    hash_shared_string(&mut hasher, new);
    hash_rgba(&mut hasher, left_bg);
    hash_rgba(&mut hasher, left_fg);
    hash_rgba(&mut hasher, left_gutter);
    hash_rgba(&mut hasher, right_bg);
    hash_rgba(&mut hasher, right_fg);
    hash_rgba(&mut hasher, right_gutter);
    left_text_hash.hash(&mut hasher);
    left_highlights_hash.hash(&mut hasher);
    right_text_hash.hash(&mut hasher);
    right_highlights_hash.hash(&mut hasher);
    hasher.finish()
}

fn patch_split_row_canvas_revision_key(
    line_no: &SharedString,
    bg: gpui::Rgba,
    fg: gpui::Rgba,
    gutter_fg: gpui::Rgba,
    text_hash: u64,
    highlights_hash: u64,
) -> u64 {
    let mut hasher = FxHasher::default();
    hash_shared_string(&mut hasher, line_no);
    hash_rgba(&mut hasher, bg);
    hash_rgba(&mut hasher, fg);
    hash_rgba(&mut hasher, gutter_fg);
    text_hash.hash(&mut hasher);
    highlights_hash.hash(&mut hasher);
    hasher.finish()
}

fn semantic_diff_row_bg(theme: AppTheme, bg: gpui::Rgba) -> Option<gpui::Rgba> {
    (bg != theme.colors.surface.canvas).then_some(bg)
}

fn focused_row_outline_color(theme: AppTheme, bg: gpui::Rgba) -> gpui::Rgba {
    with_alpha(bg, if theme.is_dark { 0.72 } else { 0.56 })
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(in crate::view) struct DiffPaintRecord {
    pub(in crate::view) visible_ix: usize,
    pub(in crate::view) region: DiffTextRegion,
    pub(in crate::view) text: SharedString,
    pub(in crate::view) highlights: Vec<(Range<usize>, Option<gpui::Hsla>, Option<gpui::Hsla>)>,
    pub(in crate::view) row_bg: Option<gpui::Rgba>,
}

#[cfg(test)]
thread_local! {
    static DIFF_PAINT_LOG: RefCell<Vec<DiffPaintRecord>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_diff_paint_for_tests(
    visible_ix: usize,
    region: DiffTextRegion,
    text: &SharedString,
    highlights: &[(Range<usize>, HighlightStyle)],
    row_bg: Option<gpui::Rgba>,
) {
    DIFF_PAINT_LOG.with(|log| {
        log.borrow_mut().push(DiffPaintRecord {
            visible_ix,
            region,
            text: text.clone(),
            highlights: highlights
                .iter()
                .map(|(range, style)| (range.clone(), style.color, style.background_color))
                .collect(),
            row_bg,
        });
    });
}

#[cfg(test)]
pub(in crate::view) fn clear_diff_paint_log_for_tests() {
    DIFF_PAINT_LOG.with(|log| log.borrow_mut().clear());
}

#[cfg(test)]
pub(in crate::view) fn diff_paint_log_for_tests() -> Vec<DiffPaintRecord> {
    DIFF_PAINT_LOG.with(|log| log.borrow().clone())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn inline_diff_line_row_canvas(
    theme: AppTheme,
    view: Entity<MainPaneView>,
    ui_scale_percent: u32,
    visible_ix: usize,
    min_width: Pixels,
    selected: bool,
    old: SharedString,
    new: SharedString,
    bg: gpui::Rgba,
    fg: gpui::Rgba,
    gutter_fg: gpui::Rgba,
    styled: Option<&CachedDiffStyledText>,
    streamed_spec: Option<StreamedDiffTextPaintSpec>,
    raw_text: Option<&str>,
    reveal_whitespace_chars: bool,
    show_line_numbers: bool,
    wrap: Option<DiffTextWrapSlice>,
    annotation_width: Pixels,
    blame: Option<RowBlamePaint>,
    annot_hover: Option<(usize, AnnotArea)>,
    stage: Option<StageGutterSpec>,
    stage_hover: Option<DiffStageHover>,
) -> AnyElement {
    let paint_payload = diff_text_paint_payload(
        styled,
        streamed_spec.as_ref(),
        raw_text,
        reveal_whitespace_chars,
        DiffTextRegion::Inline,
        wrap,
    );
    let revision = inline_row_canvas_revision_key(
        &old,
        &new,
        bg,
        fg,
        gutter_fg,
        paint_payload.text_hash,
        paint_payload.highlights_hash,
    );
    let row_hover = annot_hover.and_then(|(ix, area)| (ix == visible_ix).then_some(area));
    let revision = mix_blame_revision(revision, annotation_width, row_hover, blame.as_ref());
    let revision = mix_stage_gutter_revision(revision, &[stage], stage_hover, visible_ix);
    let text = paint_payload.text;
    let highlights = paint_payload.highlights;
    let highlights_hash = paint_payload.highlights_hash;
    let text_hash = paint_payload.text_hash;
    let offset_map = paint_payload.offset_map;
    let canvas_id: gpui::ElementId = ("diff_row_canvas_inline", visible_ix).into();
    let test_row_bg = semantic_diff_row_bg(theme, bg);

    keyed_canvas(
        (canvas_id, format!("{revision:016x}")),
        move |bounds, window, _cx| {
            let pad = px_2(window);
            let gutter_total = if show_line_numbers {
                gutter_cell_total_width(pad, ui_scale_percent)
            } else {
                px(0.0)
            };
            let content_bounds = inset_left(bounds, annotation_width);
            let text_bounds = inline_text_bounds(content_bounds, gutter_total, pad);
            // Everything but the annotation column, which owns its own clicks.
            let row_hitbox = window.insert_hitbox(content_bounds, HitboxBehavior::Normal);
            let text_hitbox = window.insert_hitbox(text_bounds, HitboxBehavior::Normal);
            let annot_hitboxes =
                build_annot_hitboxes(window, bounds, annotation_width, ui_scale_percent);
            let stage_gutter = build_stage_gutter(
                window,
                stage,
                content_bounds.left(),
                bounds,
                ui_scale_percent,
            );

            InlineRowPrepaintState {
                bounds,
                pad,
                gutter_total,
                annot_w: annotation_width,
                text_bounds,
                row_hitbox,
                text_hitbox,
                annot_hitboxes,
                stage_gutter,
            }
        },
        move |bounds, prepaint, window, cx| {
            let line_metrics = line_metrics(window);
            let when_metrics = line_metrics_annot_when(window);
            let y = center_text_y(bounds, line_metrics.line_height);

            window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);

            // Selection must not tint the annotation sidebar: fill it with a
            // neutral panel color and fill the content area with the row bg.
            paint_row_bg_with_annotation(
                window,
                prepaint.bounds,
                prepaint.annot_w,
                bg,
                theme.colors.surface.panel,
            );

            if let Some(blame) = &blame {
                render_blame_column(
                    blame,
                    prepaint.bounds,
                    prepaint.annot_w,
                    y,
                    theme,
                    line_metrics,
                    when_metrics,
                    ui_scale_percent,
                    visible_ix,
                    prepaint.annot_hitboxes.as_ref(),
                    &view,
                    window,
                    cx,
                );
            }

            if show_line_numbers {
                paint_gutter_text_right_aligned(
                    &old,
                    prepaint.bounds.left() + prepaint.annot_w + prepaint.gutter_total
                        - prepaint.pad,
                    y,
                    gutter_fg,
                    line_metrics,
                    window,
                    cx,
                );
                paint_gutter_text_right_aligned(
                    &new,
                    prepaint.bounds.left() + prepaint.annot_w + prepaint.gutter_total * 2.0
                        - prepaint.pad,
                    y,
                    gutter_fg,
                    line_metrics,
                    window,
                    cx,
                );
            }

            window.paint_layer(prepaint.text_bounds, |window| {
                paint_selectable_diff_text(
                    &view,
                    visible_ix,
                    DiffTextRegion::Inline,
                    prepaint.text_bounds,
                    &text,
                    &highlights,
                    streamed_spec.as_ref(),
                    test_row_bg,
                    highlights_hash,
                    text_hash,
                    offset_map.as_ref(),
                    reveal_whitespace_chars,
                    y,
                    fg,
                    line_metrics,
                    ui_scale_percent,
                    show_line_numbers,
                    wrap,
                    theme,
                    window,
                    cx,
                );
            });

            let stage_buttons = paint_stage_gutter(
                prepaint.stage_gutter.as_ref(),
                visible_ix,
                theme,
                bg,
                ui_scale_percent,
                &prepaint.row_hitbox,
                None,
                &view,
                window,
                cx,
            )
            .into_iter()
            .collect();

            let text_bounds = prepaint.text_bounds;
            let clip_bounds = window.content_mask().bounds;
            let visible_text_bounds = text_bounds.intersect(&clip_bounds);
            install_diff_row_mouse_handlers(
                window,
                &view,
                visible_ix,
                DiffRowMouseHandlers {
                    row_hitbox: prepaint.row_hitbox.clone(),
                    regions: DiffRowTextRegions::single(
                        DiffTextRegion::Inline,
                        visible_text_bounds,
                    ),
                    right_click: DiffRowRightClickBehavior::OpenContextMenu,
                    mouse_up: DiffRowMouseUpBehavior::HandlePatchRowClick,
                    stage: stage_buttons,
                },
            );

            if selected {
                window.paint_quad(gpui::outline(
                    inset_left(bounds, prepaint.annot_w),
                    focused_row_outline_color(theme, bg),
                    gpui::BorderStyle::default(),
                ));
            }
        },
    )
    .h(diff_row_height(ui_scale_percent))
    .min_w(min_width)
    .w_full()
    .bg(bg)
    .text_xs()
    .whitespace_nowrap()
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn split_diff_line_row_canvas(
    theme: AppTheme,
    view: Entity<MainPaneView>,
    ui_scale_percent: u32,
    visible_ix: usize,
    min_width: Pixels,
    selected: bool,
    old: SharedString,
    new: SharedString,
    left_bg: gpui::Rgba,
    left_fg: gpui::Rgba,
    left_gutter: gpui::Rgba,
    right_bg: gpui::Rgba,
    right_fg: gpui::Rgba,
    right_gutter: gpui::Rgba,
    left_styled: Option<&CachedDiffStyledText>,
    right_styled: Option<&CachedDiffStyledText>,
    left_streamed_spec: Option<StreamedDiffTextPaintSpec>,
    right_streamed_spec: Option<StreamedDiffTextPaintSpec>,
    left_raw_text: Option<&str>,
    right_raw_text: Option<&str>,
    reveal_whitespace_chars: bool,
    show_line_numbers: bool,
    wrap: Option<DiffTextWrapSlice>,
    annotation_width: Pixels,
    blame: Option<RowBlamePaint>,
    annot_hover: Option<(usize, AnnotArea)>,
    stage_left: Option<StageGutterSpec>,
    stage_right: Option<StageGutterSpec>,
    stage_hover: Option<DiffStageHover>,
) -> AnyElement {
    let left_payload = diff_text_paint_payload(
        left_styled,
        left_streamed_spec.as_ref(),
        left_raw_text,
        reveal_whitespace_chars,
        DiffTextRegion::SplitLeft,
        wrap,
    );
    let right_payload = diff_text_paint_payload(
        right_styled,
        right_streamed_spec.as_ref(),
        right_raw_text,
        reveal_whitespace_chars,
        DiffTextRegion::SplitRight,
        wrap,
    );
    let revision = split_row_canvas_revision_key(
        &old,
        &new,
        left_bg,
        left_fg,
        left_gutter,
        right_bg,
        right_fg,
        right_gutter,
        left_payload.text_hash,
        left_payload.highlights_hash,
        right_payload.text_hash,
        right_payload.highlights_hash,
    );
    let row_hover = annot_hover.and_then(|(ix, area)| (ix == visible_ix).then_some(area));
    let revision = mix_blame_revision(revision, annotation_width, row_hover, blame.as_ref());
    let revision = mix_stage_gutter_revision(
        revision,
        &[stage_left, stage_right],
        stage_hover,
        visible_ix,
    );
    let left_text = left_payload.text;
    let left_highlights = left_payload.highlights;
    let left_highlights_hash = left_payload.highlights_hash;
    let left_text_hash = left_payload.text_hash;
    let left_offset_map = left_payload.offset_map;
    let right_text = right_payload.text;
    let right_highlights = right_payload.highlights;
    let right_highlights_hash = right_payload.highlights_hash;
    let right_text_hash = right_payload.text_hash;
    let right_offset_map = right_payload.offset_map;
    let canvas_id: gpui::ElementId = ("diff_row_canvas_split", visible_ix).into();
    let left_test_row_bg = semantic_diff_row_bg(theme, left_bg);
    let right_test_row_bg = semantic_diff_row_bg(theme, right_bg);

    keyed_canvas(
        (canvas_id, format!("{revision:016x}")),
        move |bounds, window, _cx| {
            let pad = px_2(window);
            let gutter_total = if show_line_numbers {
                gutter_cell_total_width(pad, ui_scale_percent)
            } else {
                px(0.0)
            };
            let content_bounds = inset_left(bounds, annotation_width);
            let (left_col, sep_bounds, right_col) = split_columns(content_bounds);
            let left_text_bounds = column_text_bounds(left_col, gutter_total, pad);
            let right_text_bounds = column_text_bounds(right_col, gutter_total, pad);

            // Everything but the annotation column, which owns its own clicks.
            let row_hitbox = window.insert_hitbox(content_bounds, HitboxBehavior::Normal);
            let left_hitbox = window.insert_hitbox(left_text_bounds, HitboxBehavior::Normal);
            let right_hitbox = window.insert_hitbox(right_text_bounds, HitboxBehavior::Normal);
            let annot_hitboxes =
                build_annot_hitboxes(window, bounds, annotation_width, ui_scale_percent);
            let left_stage_gutter = build_stage_gutter(
                window,
                stage_left,
                left_col.left(),
                bounds,
                ui_scale_percent,
            );
            let right_stage_gutter = build_stage_gutter(
                window,
                stage_right,
                right_col.left(),
                bounds,
                ui_scale_percent,
            );

            SplitRowPrepaintState {
                bounds,
                pad,
                annot_w: annotation_width,
                left_col,
                sep_bounds,
                right_col,
                left_text_bounds,
                right_text_bounds,
                row_hitbox,
                left_hitbox,
                right_hitbox,
                annot_hitboxes,
                left_stage_gutter,
                right_stage_gutter,
            }
        },
        move |bounds, prepaint, window, cx| {
            let line_metrics = line_metrics(window);
            let when_metrics = line_metrics_annot_when(window);
            let y = center_text_y(bounds, line_metrics.line_height);

            window.set_cursor_style(CursorStyle::IBeam, &prepaint.left_hitbox);
            window.set_cursor_style(CursorStyle::IBeam, &prepaint.right_hitbox);

            // Neutral panel bg under the annotation column so selection does
            // not tint it.
            if prepaint.annot_w > px(0.0) {
                paint_annotation_sidebar(
                    window,
                    prepaint.bounds,
                    prepaint.annot_w,
                    theme.colors.surface.panel,
                );
            }
            window.paint_quad(fill(row_bg_fill_bounds(prepaint.left_col), left_bg));
            window.paint_quad(fill(
                row_bg_fill_bounds(prepaint.sep_bounds),
                theme.colors.stroke.default,
            ));
            window.paint_quad(fill(row_bg_fill_bounds(prepaint.right_col), right_bg));

            if let Some(blame) = &blame {
                render_blame_column(
                    blame,
                    prepaint.bounds,
                    prepaint.annot_w,
                    y,
                    theme,
                    line_metrics,
                    when_metrics,
                    ui_scale_percent,
                    visible_ix,
                    prepaint.annot_hitboxes.as_ref(),
                    &view,
                    window,
                    cx,
                );
            }

            if show_line_numbers {
                let gutter_total = gutter_cell_total_width(prepaint.pad, ui_scale_percent);
                paint_gutter_text_right_aligned(
                    &old,
                    prepaint.left_col.left() + gutter_total - prepaint.pad,
                    y,
                    left_gutter,
                    line_metrics,
                    window,
                    cx,
                );
                paint_gutter_text_right_aligned(
                    &new,
                    prepaint.right_col.left() + gutter_total - prepaint.pad,
                    y,
                    right_gutter,
                    line_metrics,
                    window,
                    cx,
                );
            }

            window.paint_layer(prepaint.left_text_bounds, |window| {
                paint_selectable_diff_text(
                    &view,
                    visible_ix,
                    DiffTextRegion::SplitLeft,
                    prepaint.left_text_bounds,
                    &left_text,
                    &left_highlights,
                    left_streamed_spec.as_ref(),
                    left_test_row_bg,
                    left_highlights_hash,
                    left_text_hash,
                    left_offset_map.as_ref(),
                    reveal_whitespace_chars,
                    y,
                    left_fg,
                    line_metrics,
                    ui_scale_percent,
                    show_line_numbers,
                    wrap,
                    theme,
                    window,
                    cx,
                );
            });

            window.paint_layer(prepaint.right_text_bounds, |window| {
                paint_selectable_diff_text(
                    &view,
                    visible_ix,
                    DiffTextRegion::SplitRight,
                    prepaint.right_text_bounds,
                    &right_text,
                    &right_highlights,
                    right_streamed_spec.as_ref(),
                    right_test_row_bg,
                    right_highlights_hash,
                    right_text_hash,
                    right_offset_map.as_ref(),
                    reveal_whitespace_chars,
                    y,
                    right_fg,
                    line_metrics,
                    ui_scale_percent,
                    show_line_numbers,
                    wrap,
                    theme,
                    window,
                    cx,
                );
            });

            let stage_buttons = paint_stage_gutter(
                prepaint.left_stage_gutter.as_ref(),
                visible_ix,
                theme,
                left_bg,
                ui_scale_percent,
                &prepaint.row_hitbox,
                Some(prepaint.left_col),
                &view,
                window,
                cx,
            )
            .into_iter()
            .chain(paint_stage_gutter(
                prepaint.right_stage_gutter.as_ref(),
                visible_ix,
                theme,
                right_bg,
                ui_scale_percent,
                &prepaint.row_hitbox,
                Some(prepaint.right_col),
                &view,
                window,
                cx,
            ))
            .collect();

            let left_text_bounds = prepaint.left_text_bounds;
            let right_text_bounds = prepaint.right_text_bounds;
            let clip_bounds = window.content_mask().bounds;
            let visible_left_text_bounds = left_text_bounds.intersect(&clip_bounds);
            let visible_right_text_bounds = right_text_bounds.intersect(&clip_bounds);
            install_diff_row_mouse_handlers(
                window,
                &view,
                visible_ix,
                DiffRowMouseHandlers {
                    row_hitbox: prepaint.row_hitbox.clone(),
                    regions: DiffRowTextRegions::split(
                        visible_left_text_bounds,
                        visible_right_text_bounds,
                    ),
                    right_click: DiffRowRightClickBehavior::OpenContextMenu,
                    mouse_up: DiffRowMouseUpBehavior::HandlePatchRowClick,
                    stage: stage_buttons,
                },
            );

            if selected {
                window.paint_quad(gpui::outline(
                    inset_left(bounds, prepaint.annot_w),
                    focused_row_outline_color(theme, left_bg),
                    gpui::BorderStyle::default(),
                ));
            }
        },
    )
    .h(diff_row_height(ui_scale_percent))
    .min_w(min_width)
    .w_full()
    .text_xs()
    .whitespace_nowrap()
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn patch_split_column_row_canvas(
    theme: AppTheme,
    view: Entity<MainPaneView>,
    ui_scale_percent: u32,
    column: super::diff::PatchSplitColumn,
    visible_ix: usize,
    min_width: Pixels,
    selected: bool,
    bg: gpui::Rgba,
    fg: gpui::Rgba,
    gutter_fg: gpui::Rgba,
    line_no: SharedString,
    styled: Option<&CachedDiffStyledText>,
    streamed_spec: Option<StreamedDiffTextPaintSpec>,
    raw_text: Option<&str>,
    reveal_whitespace_chars: bool,
    show_line_numbers: bool,
    wrap: Option<DiffTextWrapSlice>,
    annotation_width: Pixels,
    blame: Option<RowBlamePaint>,
    annot_hover: Option<(usize, AnnotArea)>,
    stage: Option<StageGutterSpec>,
    stage_hover: Option<DiffStageHover>,
) -> AnyElement {
    let region = match column {
        super::diff::PatchSplitColumn::Left => DiffTextRegion::SplitLeft,
        super::diff::PatchSplitColumn::Right => DiffTextRegion::SplitRight,
    };
    let paint_payload = diff_text_paint_payload(
        styled,
        streamed_spec.as_ref(),
        raw_text,
        reveal_whitespace_chars,
        region,
        wrap,
    );
    let text = paint_payload.text;
    let highlights = paint_payload.highlights;
    let highlights_hash = paint_payload.highlights_hash;
    let text_hash = paint_payload.text_hash;
    let offset_map = paint_payload.offset_map;
    let revision = patch_split_row_canvas_revision_key(
        &line_no,
        bg,
        fg,
        gutter_fg,
        text_hash,
        highlights_hash,
    );
    let row_hover = annot_hover.and_then(|(ix, area)| (ix == visible_ix).then_some(area));
    let revision = mix_blame_revision(revision, annotation_width, row_hover, blame.as_ref());
    let revision = mix_stage_gutter_revision(revision, &[stage], stage_hover, visible_ix);
    let canvas_id: gpui::ElementId = (
        match column {
            super::diff::PatchSplitColumn::Left => "diff_row_canvas_file_split_left",
            super::diff::PatchSplitColumn::Right => "diff_row_canvas_file_split_right",
        },
        visible_ix,
    )
        .into();
    let test_row_bg = semantic_diff_row_bg(theme, bg);

    keyed_canvas(
        (canvas_id, format!("{revision:016x}")),
        move |bounds, window, _cx| {
            let pad = px_2(window);
            let gutter_total = if show_line_numbers {
                gutter_cell_total_width(pad, ui_scale_percent)
            } else {
                px(0.0)
            };
            let content_bounds = inset_left(bounds, annotation_width);
            let text_bounds = single_column_text_bounds(content_bounds, gutter_total, pad);
            // Everything but the annotation column, which owns its own clicks.
            let row_hitbox = window.insert_hitbox(content_bounds, HitboxBehavior::Normal);
            let text_hitbox = window.insert_hitbox(text_bounds, HitboxBehavior::Normal);
            let annot_hitboxes =
                build_annot_hitboxes(window, bounds, annotation_width, ui_scale_percent);
            let stage_gutter = build_stage_gutter(
                window,
                stage,
                content_bounds.left(),
                bounds,
                ui_scale_percent,
            );
            SingleColumnRowPrepaintState {
                bounds,
                pad,
                annot_w: annotation_width,
                text_bounds,
                row_hitbox,
                text_hitbox,
                annot_hitboxes,
                stage_gutter,
            }
        },
        move |bounds, prepaint, window, cx| {
            let line_metrics = line_metrics(window);
            let when_metrics = line_metrics_annot_when(window);
            let y = center_text_y(bounds, line_metrics.line_height);

            window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);

            // Selection must not tint the annotation sidebar: fill it with a
            // neutral panel color and fill the content area with the row bg.
            paint_row_bg_with_annotation(
                window,
                prepaint.bounds,
                prepaint.annot_w,
                bg,
                theme.colors.surface.panel,
            );

            if let Some(blame) = &blame {
                render_blame_column(
                    blame,
                    prepaint.bounds,
                    prepaint.annot_w,
                    y,
                    theme,
                    line_metrics,
                    when_metrics,
                    ui_scale_percent,
                    visible_ix,
                    prepaint.annot_hitboxes.as_ref(),
                    &view,
                    window,
                    cx,
                );
            }

            if show_line_numbers {
                let gutter_total = gutter_cell_total_width(prepaint.pad, ui_scale_percent);
                paint_gutter_text_right_aligned(
                    &line_no,
                    prepaint.bounds.left() + prepaint.annot_w + gutter_total - prepaint.pad,
                    y,
                    gutter_fg,
                    line_metrics,
                    window,
                    cx,
                );
            }

            window.paint_layer(prepaint.text_bounds, |window| {
                paint_selectable_diff_text(
                    &view,
                    visible_ix,
                    region,
                    prepaint.text_bounds,
                    &text,
                    &highlights,
                    streamed_spec.as_ref(),
                    test_row_bg,
                    highlights_hash,
                    text_hash,
                    offset_map.as_ref(),
                    reveal_whitespace_chars,
                    y,
                    fg,
                    line_metrics,
                    ui_scale_percent,
                    show_line_numbers,
                    wrap,
                    theme,
                    window,
                    cx,
                );
            });

            let stage_buttons = paint_stage_gutter(
                prepaint.stage_gutter.as_ref(),
                visible_ix,
                theme,
                bg,
                ui_scale_percent,
                &prepaint.row_hitbox,
                None,
                &view,
                window,
                cx,
            )
            .into_iter()
            .collect();

            let text_bounds = prepaint.text_bounds;
            let clip_bounds = window.content_mask().bounds;
            let visible_text_bounds = text_bounds.intersect(&clip_bounds);
            install_diff_row_mouse_handlers(
                window,
                &view,
                visible_ix,
                DiffRowMouseHandlers {
                    row_hitbox: prepaint.row_hitbox.clone(),
                    regions: DiffRowTextRegions::single(region, visible_text_bounds),
                    right_click: DiffRowRightClickBehavior::OpenContextMenu,
                    mouse_up: DiffRowMouseUpBehavior::HandlePatchRowClick,
                    stage: stage_buttons,
                },
            );

            if selected {
                window.paint_quad(gpui::outline(
                    inset_left(bounds, prepaint.annot_w),
                    focused_row_outline_color(theme, bg),
                    gpui::BorderStyle::default(),
                ));
            }
        },
    )
    .h(diff_row_height(ui_scale_percent))
    .min_w(min_width)
    .w_full()
    .text_xs()
    .whitespace_nowrap()
    .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn worktree_preview_row_canvas(
    theme: AppTheme,
    view: Entity<MainPaneView>,
    ui_scale_percent: u32,
    ix: usize,
    min_width: Pixels,
    annotation_width: Pixels,
    blame: Option<RowBlamePaint>,
    bar_color: Option<gpui::Rgba>,
    line_no: SharedString,
    styled: Option<&CachedDiffStyledText>,
    streamed_spec: Option<StreamedDiffTextPaintSpec>,
    raw_text: Option<&str>,
    reveal_whitespace_chars: bool,
    wrap: Option<DiffTextWrapSlice>,
) -> AnyElement {
    let paint_payload = diff_text_paint_payload(
        styled,
        streamed_spec.as_ref(),
        raw_text,
        reveal_whitespace_chars,
        DiffTextRegion::Inline,
        wrap,
    );
    let text = paint_payload.text;
    let highlights = paint_payload.highlights;
    let highlights_hash = paint_payload.highlights_hash;
    let text_hash = paint_payload.text_hash;
    let offset_map = paint_payload.offset_map;

    keyed_canvas(
        ("worktree_preview_row_canvas", ix),
        move |bounds, window, _cx| {
            let pad = px_2(window);
            let gutter_total = gutter_cell_total_width(pad, ui_scale_percent);
            let bar_w = if bar_color.is_some() {
                diff_scaled_px(DIFF_CHANGE_BAR_WIDTH_PX, ui_scale_percent)
            } else {
                px(0.0)
            };
            // Inline annotate reserves a fixed column at the left edge of the row;
            // the content (change bar, gutter, text) is inset past it.
            let content = inset_left(bounds, annotation_width);
            let inner = Bounds::new(
                point(content.left() + bar_w, content.top()),
                size(
                    (content.size.width - bar_w).max(px(0.0)),
                    content.size.height,
                ),
            );
            let text_bounds = single_column_text_bounds(inner, gutter_total, pad);
            let text_hitbox = window.insert_hitbox(text_bounds, HitboxBehavior::Normal);
            let annot_hitboxes =
                build_annot_hitboxes(window, bounds, annotation_width, ui_scale_percent);
            WorktreePreviewRowPrepaintState {
                inner,
                pad,
                bar_w,
                text_bounds,
                text_hitbox,
                annot_w: annotation_width,
                annot_hitboxes,
            }
        },
        move |bounds, prepaint, window, cx| {
            let line_metrics = line_metrics(window);
            let y = center_text_y(bounds, line_metrics.line_height);

            // Reserve the annotation sidebar with the neutral panel color (matching
            // the diff renderers) so the blame column background is consistent with
            // the diff view; fill the content area with the row background.
            paint_row_bg_with_annotation(
                window,
                bounds,
                prepaint.annot_w,
                theme.colors.surface.canvas,
                theme.colors.surface.panel,
            );
            if let Some(color) = bar_color
                && prepaint.bar_w > px(0.0)
            {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.left() + prepaint.annot_w, bounds.top()),
                        size(prepaint.bar_w, bounds.size.height),
                    ),
                    color,
                ));
            }

            if let Some(blame) = &blame {
                let when_metrics = line_metrics_annot_when(window);
                render_blame_column(
                    blame,
                    bounds,
                    prepaint.annot_w,
                    y,
                    theme,
                    line_metrics,
                    when_metrics,
                    ui_scale_percent,
                    ix,
                    prepaint.annot_hitboxes.as_ref(),
                    &view,
                    window,
                    cx,
                );
            }

            window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);

            paint_gutter_text_right_aligned(
                &line_no,
                prepaint.inner.left() + gutter_cell_total_width(prepaint.pad, ui_scale_percent)
                    - prepaint.pad,
                y,
                theme.colors.foreground.secondary,
                line_metrics,
                window,
                cx,
            );

            window.paint_layer(prepaint.text_bounds, |window| {
                paint_selectable_diff_text(
                    &view,
                    ix,
                    DiffTextRegion::Inline,
                    prepaint.text_bounds,
                    &text,
                    &highlights,
                    streamed_spec.as_ref(),
                    None,
                    highlights_hash,
                    text_hash,
                    offset_map.as_ref(),
                    reveal_whitespace_chars,
                    y,
                    theme.colors.foreground.primary,
                    line_metrics,
                    ui_scale_percent,
                    true,
                    None,
                    theme,
                    window,
                    cx,
                );
            });

            window.on_mouse_event({
                let view = view.clone();
                // The hitbox covers the same text area, but consulting it rather
                // than the bounds keeps clicks on anything painted over the
                // preview (a floating popover, a menu) from reaching this row.
                let text_hitbox = prepaint.text_hitbox.clone();
                move |event: &gpui::MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !text_hitbox.is_hovered(window) {
                        return;
                    }

                    if event.button == gpui::MouseButton::Left {
                        let focus = view.read(cx).diff_panel_focus_handle.clone();
                        window.focus(&focus, cx);
                        let click_count = event.click_count;
                        let position = event.position;
                        view.update(cx, |this, cx| {
                            this.handle_diff_text_mouse_down(
                                ix,
                                DiffTextRegion::Inline,
                                position,
                                click_count,
                                cx,
                            );
                            cx.notify();
                        });
                    } else if event.button == gpui::MouseButton::Right {
                        view.update(cx, |this, cx| {
                            this.open_diff_editor_context_menu(
                                ix,
                                DiffTextRegion::Inline,
                                event.position,
                                window,
                                cx,
                            );
                            cx.notify();
                        });
                    }
                }
            });
        },
    )
    .h(diff_row_height(ui_scale_percent))
    .min_w(min_width + annotation_width)
    .w_full()
    .text_xs()
    .whitespace_nowrap()
    .into_any_element()
}

#[derive(Clone, Debug)]
struct InlineRowPrepaintState {
    bounds: Bounds<Pixels>,
    pad: Pixels,
    gutter_total: Pixels,
    annot_w: Pixels,
    text_bounds: Bounds<Pixels>,
    row_hitbox: Hitbox,
    text_hitbox: Hitbox,
    annot_hitboxes: Option<AnnotHitboxes>,
    stage_gutter: Option<StageGutterPrepaint>,
}

#[derive(Clone, Debug)]
struct SplitRowPrepaintState {
    bounds: Bounds<Pixels>,
    pad: Pixels,
    annot_w: Pixels,
    left_col: Bounds<Pixels>,
    sep_bounds: Bounds<Pixels>,
    right_col: Bounds<Pixels>,
    left_text_bounds: Bounds<Pixels>,
    right_text_bounds: Bounds<Pixels>,
    row_hitbox: Hitbox,
    left_hitbox: Hitbox,
    right_hitbox: Hitbox,
    annot_hitboxes: Option<AnnotHitboxes>,
    left_stage_gutter: Option<StageGutterPrepaint>,
    right_stage_gutter: Option<StageGutterPrepaint>,
}

#[derive(Clone, Debug)]
struct SingleColumnRowPrepaintState {
    bounds: Bounds<Pixels>,
    pad: Pixels,
    annot_w: Pixels,
    text_bounds: Bounds<Pixels>,
    row_hitbox: Hitbox,
    text_hitbox: Hitbox,
    annot_hitboxes: Option<AnnotHitboxes>,
    stage_gutter: Option<StageGutterPrepaint>,
}

#[derive(Clone, Debug)]
struct WorktreePreviewRowPrepaintState {
    inner: Bounds<Pixels>,
    pad: Pixels,
    bar_w: Pixels,
    text_bounds: Bounds<Pixels>,
    text_hitbox: Hitbox,
    annot_w: Pixels,
    annot_hitboxes: Option<AnnotHitboxes>,
}

#[derive(Clone, Debug)]
enum DiffRowTextRegions {
    Single {
        region: DiffTextRegion,
        bounds: Bounds<Pixels>,
    },
    Split {
        left_bounds: Bounds<Pixels>,
        right_bounds: Bounds<Pixels>,
    },
}

impl DiffRowTextRegions {
    fn single(region: DiffTextRegion, bounds: Bounds<Pixels>) -> Self {
        Self::Single { region, bounds }
    }

    fn split(left_bounds: Bounds<Pixels>, right_bounds: Bounds<Pixels>) -> Self {
        Self::Split {
            left_bounds,
            right_bounds,
        }
    }

    fn region_at(&self, position: gpui::Point<Pixels>) -> Option<DiffTextRegion> {
        match self {
            Self::Single { region, bounds } => bounds.contains(&position).then_some(*region),
            Self::Split {
                left_bounds,
                right_bounds,
            } => {
                if left_bounds.contains(&position) {
                    Some(DiffTextRegion::SplitLeft)
                } else if right_bounds.contains(&position) {
                    Some(DiffTextRegion::SplitRight)
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffRowRightClickBehavior {
    OpenContextMenu,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffRowMouseUpBehavior {
    None,
    HandlePatchRowClick,
}

#[derive(Clone, Debug)]
struct DiffRowMouseHandlers {
    row_hitbox: Hitbox,
    regions: DiffRowTextRegions,
    right_click: DiffRowRightClickBehavior,
    mouse_up: DiffRowMouseUpBehavior,
    /// Stage/unstage buttons painted in this row's gutter(s): one per column, so
    /// at most two in split views and at most one anywhere else.
    stage: Vec<StageGutterMouse>,
}

fn install_diff_row_mouse_handlers(
    window: &mut Window,
    view: &Entity<MainPaneView>,
    visible_ix: usize,
    handlers: DiffRowMouseHandlers,
) {
    let DiffRowMouseHandlers {
        row_hitbox,
        regions,
        right_click,
        mouse_up,
        stage,
    } = handlers;
    let row_hitbox_for_down = row_hitbox.clone();
    let regions = regions.clone();
    let stage_for_down = stage.clone();
    window.on_mouse_event({
        let view = view.clone();
        move |event: &gpui::MouseDownEvent, phase, window, cx| {
            if !should_handle_row_mouse_event(phase, &row_hitbox_for_down, window) {
                return;
            }

            let region = regions.region_at(event.position);

            if event.button == gpui::MouseButton::Left {
                let focus = view.read(cx).diff_panel_focus_handle.clone();
                window.focus(&focus, cx);
                // The gutter button owns the whole press: staging happens here,
                // and neither text selection nor row selection may see it.
                // Claiming the press is what stands the release handlers down —
                // staging reloads the diff, so the release lands on a repainted
                // row whose fresh handlers would otherwise read it as an
                // ordinary click. The claim outlives the release and is cleared
                // by the next press, so it cannot swallow a later click.
                // A repeat click of a double-click stages nothing: the first one
                // is still in flight, and window-activating clicks
                // (`first_mouse`) must not act at all.
                if let Some(kind) = StageGutterMouse::hovered(&stage_for_down, window) {
                    crate::press_gesture::claim_press(cx);
                    cx.stop_propagation();
                    if event.click_count > 1 || event.first_mouse {
                        return;
                    }
                    view.update(cx, |this, cx| {
                        this.stage_or_unstage_diff_line(visible_ix, kind, cx);
                        cx.notify();
                    });
                    return;
                }
                if let Some(region) = region {
                    let click_count = event.click_count;
                    let position = event.position;
                    view.update(cx, |this, cx| {
                        this.handle_diff_text_mouse_down(
                            visible_ix,
                            region,
                            position,
                            click_count,
                            cx,
                        );
                        cx.notify();
                    });
                }
            } else if event.button == gpui::MouseButton::Right
                && let Some(region) = region
            {
                match right_click {
                    DiffRowRightClickBehavior::OpenContextMenu => {
                        view.update(cx, |this, cx| {
                            this.open_diff_editor_context_menu(
                                visible_ix,
                                region,
                                event.position,
                                window,
                                cx,
                            );
                            cx.notify();
                        });
                    }
                }
            }
        }
    });

    if mouse_up == DiffRowMouseUpBehavior::None {
        return;
    }

    window.on_mouse_event({
        let view = view.clone();
        move |event: &gpui::MouseUpEvent, phase, window, cx| {
            if event.button != gpui::MouseButton::Left
                || !should_handle_row_mouse_event(phase, &row_hitbox, window)
            {
                return;
            }

            // A canvas cannot lean on `on_click` to pair press and release, so
            // it asks who owns the press instead.
            if crate::press_gesture::is_press_claimed(cx) {
                return;
            }

            // A release over a gutter button belongs to that button (it already
            // staged on press): it must not also move the row selection.
            if StageGutterMouse::hovered(&stage, window).is_some() {
                return;
            }

            let shift = event.modifiers.shift;
            view.update(cx, |this, cx| {
                if this.consume_suppress_click_after_drag() {
                    cx.notify();
                    return;
                }
                this.handle_patch_row_click(visible_ix, DiffClickKind::Line, shift);
                cx.notify();
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn paint_selectable_diff_text(
    view: &Entity<MainPaneView>,
    visible_ix: usize,
    region: DiffTextRegion,
    bounds: Bounds<Pixels>,
    text: &SharedString,
    highlights: &Arc<[(Range<usize>, HighlightStyle)]>,
    streamed_spec: Option<&StreamedDiffTextPaintSpec>,
    row_bg: Option<gpui::Rgba>,
    highlights_hash: u64,
    text_hash: u64,
    offset_map: Option<&DiffTextOffsetMap>,
    reveal_whitespace_chars: bool,
    y: Pixels,
    base_fg: gpui::Rgba,
    metrics: LineMetrics,
    ui_scale_percent: u32,
    show_line_numbers: bool,
    wrap: Option<DiffTextWrapSlice>,
    theme: AppTheme,
    window: &mut Window,
    cx: &mut App,
) {
    let mut base_style = diff_text_style(window);
    base_style.color = base_fg.into_color();
    base_style.white_space = gpui::WhiteSpace::Nowrap;
    base_style.text_overflow = None;

    let pad = px_2(window);
    let gutter_total = gutter_cell_total_width(pad, ui_scale_percent);
    let row_extra = match region {
        DiffTextRegion::Inline if show_line_numbers => gutter_total * 2.0 + pad * 2.0,
        DiffTextRegion::SplitLeft | DiffTextRegion::SplitRight if show_line_numbers => {
            gutter_total + pad * 2.0
        }
        _ => pad * 2.0,
    };
    let total_text_len = streamed_spec
        .filter(|spec| should_stream_diff_text(Some(spec)))
        .map(|spec| spec.raw_text.len())
        .unwrap_or_else(|| text.len());
    let source_text_len = offset_map
        .map(DiffTextOffsetMap::source_len)
        .unwrap_or(total_text_len);
    let (source_visible_ix, visual_text_range) = view
        .read(cx)
        .diff_text_visual_source_range_for_region(visible_ix, region);
    let selection = view
        .read(cx)
        .diff_text_local_selection_range(visible_ix, region);

    let mut streamed_styled = None;
    let mut streamed_slice_range = None;
    let mut streamed_slice_is_wrap = false;
    let mut paint_x = bounds.left();
    let mut hitbox_cell_width = None;
    let mut pending_prepared_syntax = false;

    let (layout_key, layout, shaped_new, required_row_w) = if let Some(spec) =
        streamed_spec.filter(|spec| should_stream_diff_text(Some(spec)))
    {
        let cell_width =
            streamed_diff_text_ascii_cell_width(&base_style, metrics.font_size, window);
        let clip_bounds = window.content_mask().bounds;
        let overscan_columns = STREAMED_DIFF_TEXT_OVERSCAN_COLUMNS.max(spec.query.as_ref().len());
        let wrap_range = wrap.map(|wrap| wrap.range_for_region(region));
        streamed_slice_is_wrap = wrap_range.is_some();
        let slice_range = wrap_range.clone().unwrap_or_else(|| {
            streamed_diff_text_visible_slice_range(
                bounds,
                clip_bounds,
                spec.raw_text.len(),
                cell_width,
                overscan_columns,
            )
        });
        let (mut slice_styled, pending, resolved_slice_range) =
            build_streamed_diff_slice_styled_text(theme, spec, &slice_range);
        if reveal_whitespace_chars {
            let append_eol_marker = resolved_slice_range.end >= spec.raw_text.len();
            slice_styled = whitespace_visible_styled_text(&slice_styled, append_eol_marker);
        }
        let (layout_key, layout, shaped_new) = ensure_layout_cached(
            view,
            slice_styled.text_hash,
            &slice_styled.text,
            &base_style,
            base_fg,
            slice_styled.highlights.as_ref(),
            slice_styled.highlights_hash,
            metrics,
            window,
            cx,
        );
        paint_x = if wrap_range.is_some() {
            bounds.left()
        } else {
            bounds.left() + cell_width * resolved_slice_range.start as f32
        };
        hitbox_cell_width = Some(cell_width);
        pending_prepared_syntax = pending;
        streamed_slice_range = Some(resolved_slice_range);
        let total_text_cells = spec
            .raw_text
            .len()
            .saturating_add(usize::from(reveal_whitespace_chars));
        let required_row_w = (row_extra
            + cell_width * total_text_cells as f32
            + diff_scaled_px(DIFF_ROW_TEXT_TRAILING_PADDING_PX, ui_scale_percent))
        .round();
        streamed_styled = Some(slice_styled);
        (layout_key, layout, shaped_new, required_row_w)
    } else {
        let (layout_key, layout, shaped_new) = ensure_layout_cached(
            view,
            text_hash,
            text,
            &base_style,
            base_fg,
            highlights.as_ref(),
            highlights_hash,
            metrics,
            window,
            cx,
        );
        let required_row_w = (row_extra
            + layout.width
            + diff_scaled_px(DIFF_ROW_TEXT_TRAILING_PADDING_PX, ui_scale_percent))
        .round();
        (layout_key, layout, shaped_new, required_row_w)
    };

    let paint_text = streamed_styled
        .as_ref()
        .map(|styled| &styled.text)
        .unwrap_or(text);
    let paint_highlights = streamed_styled
        .as_ref()
        .map(|styled| styled.highlights.as_ref())
        .unwrap_or_else(|| highlights.as_ref());

    #[cfg(test)]
    record_diff_paint_for_tests(visible_ix, region, paint_text, paint_highlights, row_bg);
    #[cfg(not(test))]
    let _ = row_bg;

    if let Some(r) = selection {
        let (x0, x1) = if let Some(cell_width) = hitbox_cell_width {
            let (start, end) = if streamed_slice_is_wrap {
                (r.start.min(total_text_len), r.end.min(total_text_len))
            } else {
                let start = streamed_slice_range
                    .as_ref()
                    .map(|slice_range| r.start.max(slice_range.start))
                    .unwrap_or(r.start)
                    .min(total_text_len);
                let end = streamed_slice_range
                    .as_ref()
                    .map(|slice_range| r.end.min(slice_range.end))
                    .unwrap_or(r.end)
                    .min(total_text_len);
                (start, end)
            };
            (cell_width * start as f32, cell_width * end as f32)
        } else if let Some(offset_map) = offset_map {
            let start = offset_map.display_offset_for_source(r.start.min(source_text_len));
            let end = offset_map.display_offset_for_source(r.end.min(source_text_len));
            (layout.x_for_index(start), layout.x_for_index(end))
        } else {
            (
                layout.x_for_index(r.start.min(total_text_len)),
                layout.x_for_index(r.end.min(total_text_len)),
            )
        };

        if x1 > x0 {
            let color = view.read(cx).diff_text_selection_color();
            window.paint_quad(fill(
                Bounds::from_corners(
                    point(bounds.left() + x0, bounds.top()),
                    point(bounds.left() + x1, bounds.bottom()),
                ),
                color,
            ));
        }
    }

    let hitbox = DiffTextHitbox {
        bounds,
        layout_key,
        source_visible_ix,
        text_start_offset: if streamed_slice_is_wrap {
            streamed_slice_range
                .as_ref()
                .map(|range| range.start)
                .unwrap_or(visual_text_range.start)
        } else {
            visual_text_range.start
        },
        text_len: if streamed_slice_is_wrap {
            streamed_slice_range
                .as_ref()
                .map(|range| range.end.saturating_sub(range.start))
                .unwrap_or(text.len())
        } else if let Some(offset_map) = offset_map {
            offset_map.display_len()
        } else {
            total_text_len
        },
        offset_map: offset_map.cloned(),
        painted_text: paint_text.clone(),
        streamed_ascii_monospace_cell_width: hitbox_cell_width,
        wrapped: None,
    };

    view.update(cx, |this, cx| {
        this.set_diff_text_hitbox(visible_ix, region, hitbox);
        this.touch_diff_text_layout_cache(layout_key, shaped_new);
        if pending_prepared_syntax {
            this.ensure_prepared_syntax_chunk_poll(cx);
        }
        let column = match region {
            DiffTextRegion::Inline | DiffTextRegion::SplitLeft => {
                DiffHorizontalScrollColumn::Primary
            }
            DiffTextRegion::SplitRight => DiffHorizontalScrollColumn::SplitRight,
        };
        this.record_diff_horizontal_content_width_for_column(column, required_row_w, cx);
    });

    if paint_text.is_empty() {
        return;
    }

    if paint_highlights.is_empty() {
        let _ = layout.paint(
            point(paint_x, y),
            metrics.line_height,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        return;
    }

    let _ = layout.paint_background(
        point(paint_x, y),
        metrics.line_height,
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
    let _ = layout.paint(
        point(paint_x, y),
        metrics.line_height,
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

fn diff_layout_base_key(
    text_hash: u64,
    base_style: &TextStyle,
    base_fg: gpui::Rgba,
    metrics: LineMetrics,
) -> u64 {
    let mut hasher = FxHasher::default();
    text_hash.hash(&mut hasher);
    metrics.font_size.hash(&mut hasher);
    base_style.font_family.hash(&mut hasher);
    base_style.font_weight.hash(&mut hasher);
    base_fg.red.to_bits().hash(&mut hasher);
    base_fg.green.to_bits().hash(&mut hasher);
    base_fg.blue.to_bits().hash(&mut hasher);
    base_fg.alpha.to_bits().hash(&mut hasher);
    hasher.finish()
}

#[allow(clippy::too_many_arguments)]
fn ensure_layout_cached(
    view: &Entity<MainPaneView>,
    text_hash: u64,
    text: &SharedString,
    base_style: &TextStyle,
    base_fg: gpui::Rgba,
    highlights: &[(Range<usize>, HighlightStyle)],
    highlights_hash: u64,
    metrics: LineMetrics,
    window: &mut Window,
    cx: &mut App,
) -> (u64, gpui::ShapedLine, Option<gpui::ShapedLine>) {
    let base_key = diff_layout_base_key(text_hash, base_style, base_fg, metrics);

    let layout_key = if highlights.is_empty() {
        base_key
    } else {
        let mut hasher = FxHasher::default();
        base_key.hash(&mut hasher);
        highlights_hash.hash(&mut hasher);
        highlights.len().hash(&mut hasher);
        hasher.finish()
    };

    if let Some(entry) = view.read(cx).diff_text_layout_cache.get(&layout_key) {
        return (layout_key, entry.layout.clone(), None);
    }

    let shaped = if highlights.is_empty() {
        let run = base_style.to_run(text.len());
        window
            .text_system()
            .shape_line(text.clone(), metrics.font_size, &[run], None)
    } else {
        let runs = compute_runs(text.as_ref(), base_style, highlights);
        window
            .text_system()
            .shape_line(text.clone(), metrics.font_size, &runs, None)
    };
    (layout_key, shaped.clone(), Some(shaped))
}

fn compute_runs(
    text: &str,
    default_style: &TextStyle,
    highlights: &[(Range<usize>, HighlightStyle)],
) -> Vec<TextRun> {
    crate::text_runs::text_runs_for_highlights(text, default_style, highlights)
}

#[cfg(test)]
mod tests {
    use super::geometry::DIFF_ROW_BACKGROUND_OVERDRAW_PX;
    use super::streamed::STREAMED_DIFF_TEXT_MIN_BYTES;
    use super::*;

    fn rgba(r: f32, g: f32, b: f32) -> gpui::Rgba {
        gpui::Rgba::new(r, g, b, 1.0)
    }

    /// `gpui` shapes a line by splitting the text at each run boundary, so a
    /// run that ends inside a multi-byte character aborts the process in
    /// `str::split_at`. This pins the diff canvas to the shared guard: without
    /// it, a highlight pointing into a character reaches `shape_line` as a
    /// run length that splits it.
    #[test]
    fn compute_runs_never_splits_a_multibyte_char() {
        let text = "— dash — end";
        let style = TextStyle::default();
        let bold = HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..HighlightStyle::default()
        };

        for highlights in [
            // Inside the leading em dash, from both sides.
            vec![(0..1, bold)],
            vec![(1..3, bold)],
            vec![(2..4, bold)],
            // Inside the second em dash, after valid text.
            vec![(0..2, bold), (7..9, bold)],
            // Past the end, overlapping, and out of order.
            vec![(5..99, bold)],
            vec![(0..6, bold), (2..4, bold)],
            vec![(6..9, bold), (0..3, bold)],
            vec![],
        ] {
            let runs = compute_runs(text, &style, &highlights);
            let total: usize = runs.iter().map(|run| run.len).sum();
            assert_eq!(
                total,
                text.len(),
                "runs must tile the text for {highlights:?}"
            );

            let mut rest = text;
            for run in &runs {
                assert!(
                    rest.is_char_boundary(run.len),
                    "run of {} bytes splits a character in {rest:?} for {highlights:?}",
                    run.len
                );
                rest = &rest[run.len..];
            }
        }
    }

    fn test_bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn streamed_query_spec(
        raw_text: &str,
        query: &str,
        query_options: DiffSearchOptions,
    ) -> StreamedDiffTextPaintSpec {
        StreamedDiffTextPaintSpec {
            raw_text: worktree_core::file_diff::FileDiffLineText::from(raw_text),
            query: query.to_owned().into(),
            query_options,
            query_matcher: (!query.is_empty())
                .then(|| Arc::new(DiffSearchMatcher::new(query, query_options))),
            query_emphasis: DiffSearchMatchEmphasis::Other,
            word_ranges: Arc::from(Vec::<Range<usize>>::new()),
            word_kind: None,
            syntax: StreamedDiffTextSyntaxSource::None,
        }
    }

    fn highlight_ranges(styled: &CachedDiffStyledText) -> Vec<Range<usize>> {
        styled
            .highlights
            .iter()
            .map(|(range, _)| range.clone())
            .collect()
    }

    #[test]
    fn diff_text_paint_payload_reveals_whitespace_markers() {
        let style = HighlightStyle::default();
        let styled = CachedDiffStyledText {
            text: "a b\t".into(),
            highlights: Arc::from(vec![(1..4, style)]),
            highlights_hash: 7,
            text_hash: 11,
        };

        let payload = diff_text_paint_payload(
            Some(&styled),
            None,
            Some("a b\t"),
            true,
            DiffTextRegion::Inline,
            None,
        );

        assert_eq!(payload.text.as_ref(), "a·b→↵");
        assert_eq!(payload.highlights[0].0, 1..7);
        assert_ne!(payload.text_hash, styled.text_hash);

        let offset_map = payload.offset_map.expect("reveal whitespace offset map");
        assert_eq!(offset_map.source_offset_for_display(0), 0);
        assert_eq!(offset_map.source_offset_for_display("a·".len()), 2);
        assert_eq!(offset_map.source_offset_for_display("a·b→".len()), 7);
        assert_eq!(offset_map.display_offset_for_source(2), "a·".len());
        assert_eq!(offset_map.display_offset_for_source(7), "a·b→".len());
    }

    #[test]
    fn diff_text_paint_payload_keeps_streamed_whitespace_rows_unmaterialized() {
        let raw = "a ".repeat((STREAMED_DIFF_TEXT_MIN_BYTES / 2).saturating_add(1));
        let spec = streamed_query_spec(raw.as_str(), "", DiffSearchOptions::default());

        assert!(should_stream_diff_text(Some(&spec)));

        let payload =
            diff_text_paint_payload(None, Some(&spec), None, true, DiffTextRegion::Inline, None);

        assert!(payload.text.is_empty());
        assert!(payload.highlights.is_empty());
        assert_ne!(payload.text_hash, 0);
        assert!(payload.offset_map.is_none());
    }

    #[test]
    fn row_bg_fill_bounds_overdraws_bottom_without_changing_origin_or_width() {
        let bounds = test_bounds(4.0, 8.0, 120.0, 20.0);
        let painted = row_bg_fill_bounds(bounds);

        assert_eq!(painted.origin, bounds.origin);
        assert_eq!(painted.size.width, bounds.size.width);
        assert_eq!(
            painted.size.height,
            bounds.size.height + px(DIFF_ROW_BACKGROUND_OVERDRAW_PX)
        );
    }

    #[test]
    fn diff_row_text_regions_single_only_hits_inside_text() {
        let regions =
            DiffRowTextRegions::single(DiffTextRegion::Inline, test_bounds(5.0, 5.0, 20.0, 10.0));

        assert_eq!(
            regions.region_at(point(px(10.0), px(10.0))),
            Some(DiffTextRegion::Inline)
        );
        assert_eq!(regions.region_at(point(px(1.0), px(10.0))), None);
    }

    #[test]
    fn diff_row_text_regions_split_maps_left_and_right_regions() {
        let regions = DiffRowTextRegions::split(
            test_bounds(0.0, 0.0, 40.0, 20.0),
            test_bounds(41.0, 0.0, 40.0, 20.0),
        );

        assert_eq!(
            regions.region_at(point(px(10.0), px(10.0))),
            Some(DiffTextRegion::SplitLeft)
        );
        assert_eq!(
            regions.region_at(point(px(60.0), px(10.0))),
            Some(DiffTextRegion::SplitRight)
        );
        assert_eq!(regions.region_at(point(px(40.5), px(10.0))), None);
    }

    #[test]
    fn inline_row_canvas_revision_key_tracks_rendered_payload() {
        let base = inline_row_canvas_revision_key(
            &"1".into(),
            &"2".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            11,
            17,
        );

        assert_eq!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"3".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(1.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                12,
                17,
            )
        );
    }

    #[test]
    fn split_row_canvas_revision_key_tracks_both_sides() {
        let base = split_row_canvas_revision_key(
            &"10".into(),
            &"20".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            3,
            5,
            7,
            11,
        );

        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"20".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                4,
                5,
                7,
                11,
            )
        );
        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"20".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                3,
                5,
                7,
                11,
            )
        );
        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"21".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                3,
                5,
                7,
                11,
            )
        );
    }

    #[test]
    fn patch_split_row_canvas_revision_key_tracks_line_number_and_style() {
        let base = patch_split_row_canvas_revision_key(
            &"42".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            13,
            17,
        );

        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"43".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                13,
                17,
            )
        );
        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"42".into(),
                rgba(0.0, 1.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                13,
                17,
            )
        );
        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"42".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                14,
                17,
            )
        );
    }

    #[test]
    fn streamed_query_overlay_skips_whole_word_on_partial_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "foo_suffix",
            "foo",
            DiffSearchOptions {
                whole_word: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) = build_streamed_diff_slice_styled_text(theme, &spec, &(0..3));

        assert_eq!(resolved, 0..3);
        assert_eq!(styled.text.as_ref(), "foo");
        assert!(styled.highlights.is_empty());
    }

    #[test]
    fn streamed_query_overlay_skips_regex_anchor_on_partial_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "prefixfoo suffix",
            r"^foo",
            DiffSearchOptions {
                regex: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) = build_streamed_diff_slice_styled_text(theme, &spec, &(6..9));

        assert_eq!(resolved, 6..9);
        assert_eq!(styled.text.as_ref(), "foo");
        assert!(styled.highlights.is_empty());
    }

    #[test]
    fn streamed_query_overlay_keeps_boundary_sensitive_matches_on_full_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "foo suffix",
            r"^foo",
            DiffSearchOptions {
                regex: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) =
            build_streamed_diff_slice_styled_text(theme, &spec, &(0.."foo suffix".len()));

        assert_eq!(resolved, 0.."foo suffix".len());
        assert_eq!(highlight_ranges(&styled), vec![0..3]);
    }
}
