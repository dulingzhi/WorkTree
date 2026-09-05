//! Diff-canvas geometry: design constants, scaled pixel and row-size
//! helpers, column bounds math, and the shaped gutter-line layout cache.

use super::*;

const GUTTER_TEXT_LAYOUT_CACHE_MAX_ENTRIES: usize = 16_384;
const DIFF_TEXT_WRAP_WIDTH_SAMPLE: &str = "WWWWWWWWWW";
const DIFF_ROW_HEIGHT_PX: f32 = 20.0;
/// Width of a line-number cell (excluding the shared horizontal padding).
/// Sized to fit six digits of the diff monospace font; numbers right-align
/// toward the content so any slack sits before the digits, not between the
/// digits and the code.
const DIFF_GUTTER_BASE_WIDTH_PX: f32 = 38.0;
const DIFF_ROW_HORIZONTAL_PADDING_PX: f32 = 8.0;
pub(super) const DIFF_ROW_TEXT_TRAILING_PADDING_PX: f32 = 16.0;
pub(super) const DIFF_CHANGE_BAR_WIDTH_PX: f32 = 3.0;
pub(super) const DIFF_ROW_BACKGROUND_OVERDRAW_PX: f32 = 1.0;

thread_local! {
    static GUTTER_TEXT_LAYOUT_CACHE: RefCell<FxLruCache<u64, gpui::ShapedLine>> =
        RefCell::new(new_fx_lru_cache(GUTTER_TEXT_LAYOUT_CACHE_MAX_ENTRIES));
}

pub(super) fn row_bg_fill_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        bounds.origin,
        size(
            bounds.size.width,
            bounds.size.height + px(DIFF_ROW_BACKGROUND_OVERDRAW_PX),
        ),
    )
}

/// Width of one wrapped diff-text column, measured in `editor_font_family`.
///
/// The family must be passed in rather than taken from the ambient text style:
/// wrap columns are computed while the diff pane builds its element tree, which
/// is before the rows container pushes `.font_family(editor_font)` onto the
/// window text style stack. `window.text_style()` still resolves to the
/// proportional UI font at that point, and measuring the `W` sample there
/// overestimates the column width by ~1.5x (IBM Plex Sans `W` is 0.891em vs
/// Lilex 0.600em), so every line wrapped at about two thirds of the width it
/// actually had.
pub(in crate::view) fn diff_text_wrap_char_width(
    window: &mut Window,
    editor_font_family: impl Into<gpui::SharedString>,
) -> Pixels {
    let mut style = diff_text_style(window);
    style.font_family = editor_font_family.into();
    let font_size = style.font_size.to_pixels(window.rem_size()) * DIFF_FONT_SCALE;
    let run = style.to_run(DIFF_TEXT_WRAP_WIDTH_SAMPLE.len());
    let layout = window.text_system().shape_line(
        DIFF_TEXT_WRAP_WIDTH_SAMPLE.into(),
        font_size,
        &[run],
        None,
    );
    if DIFF_TEXT_WRAP_WIDTH_SAMPLE.is_empty() {
        px(1.0)
    } else {
        (layout.width / DIFF_TEXT_WRAP_WIDTH_SAMPLE.len() as f32).max(px(1.0))
    }
}

pub(in crate::view) fn diff_scaled_px(value: f32, ui_scale_percent: u32) -> Pixels {
    crate::ui_scale::design_px_from_percent(value, ui_scale_percent)
}

pub(in crate::view) fn diff_row_height(ui_scale_percent: u32) -> Pixels {
    diff_scaled_px(DIFF_ROW_HEIGHT_PX, ui_scale_percent)
}

pub(in crate::view) fn diff_row_horizontal_padding(ui_scale_percent: u32) -> Pixels {
    diff_scaled_px(DIFF_ROW_HORIZONTAL_PADDING_PX, ui_scale_percent)
}

pub(super) fn diff_gutter_total_width(ui_scale_percent: u32) -> Pixels {
    gutter_cell_total_width(
        diff_row_horizontal_padding(ui_scale_percent),
        ui_scale_percent,
    )
}

/// Width of the bar marking a wholly added or removed file, which the row's
/// content is inset past.
pub(in crate::view) fn diff_change_bar_width(ui_scale_percent: u32) -> Pixels {
    diff_scaled_px(DIFF_CHANGE_BAR_WIDTH_PX, ui_scale_percent)
}

pub(in crate::view) fn diff_single_column_text_start(ui_scale_percent: u32) -> Pixels {
    diff_gutter_total_width(ui_scale_percent) + diff_row_horizontal_padding(ui_scale_percent)
}

pub(in crate::view) fn diff_inline_text_start(ui_scale_percent: u32) -> Pixels {
    diff_gutter_total_width(ui_scale_percent) * 2.0 + diff_row_horizontal_padding(ui_scale_percent)
}

pub(super) fn gutter_cell_total_width(pad: Pixels, ui_scale_percent: u32) -> Pixels {
    diff_scaled_px(DIFF_GUTTER_BASE_WIDTH_PX, ui_scale_percent) + pad * 2.0
}

pub(super) fn inline_text_bounds(
    bounds: Bounds<Pixels>,
    gutter_total: Pixels,
    pad: Pixels,
) -> Bounds<Pixels> {
    let left = bounds.left() + gutter_total * 2.0 + pad;
    let width = (bounds.size.width - gutter_total * 2.0 - pad * 2.0).max(px(0.0));
    Bounds::new(point(left, bounds.top()), size(width, bounds.size.height))
}

/// Shrink `bounds` from the left by `dx`, reserving that space (e.g. for the
/// annotation column). Width is clamped to zero.
pub(super) fn inset_left(bounds: Bounds<Pixels>, dx: Pixels) -> Bounds<Pixels> {
    Bounds::new(
        point(bounds.left() + dx, bounds.top()),
        size((bounds.size.width - dx).max(px(0.0)), bounds.size.height),
    )
}

pub(super) fn single_column_text_bounds(
    bounds: Bounds<Pixels>,
    gutter_total: Pixels,
    pad: Pixels,
) -> Bounds<Pixels> {
    let left = bounds.left() + gutter_total + pad;
    let width = (bounds.size.width - gutter_total - pad * 2.0).max(px(0.0));
    Bounds::new(point(left, bounds.top()), size(width, bounds.size.height))
}

pub(super) fn split_columns(
    bounds: Bounds<Pixels>,
) -> (Bounds<Pixels>, Bounds<Pixels>, Bounds<Pixels>) {
    let sep = px(1.0);
    let total_w = bounds.size.width.max(px(0.0));
    let inner_w = (total_w - sep).max(px(0.0));
    let left_w = (inner_w * 0.5).floor();
    let right_w = (inner_w - left_w).max(px(0.0));
    let left = Bounds::new(bounds.origin, size(left_w, bounds.size.height));
    let sep_bounds = Bounds::new(
        point(bounds.left() + left_w, bounds.top()),
        size(sep, bounds.size.height),
    );
    let right = Bounds::new(
        point(bounds.left() + left_w + sep, bounds.top()),
        size(right_w, bounds.size.height),
    );
    (left, sep_bounds, right)
}

pub(super) fn column_text_bounds(
    col: Bounds<Pixels>,
    gutter_total: Pixels,
    pad: Pixels,
) -> Bounds<Pixels> {
    single_column_text_bounds(col, gutter_total, pad)
}

/// Paints `text` with its right edge at `right`; used for line numbers so the
/// digits hug the content edge of their gutter cell.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_gutter_text_right_aligned(
    text: &SharedString,
    right: Pixels,
    y: Pixels,
    color: gpui::Rgba,
    metrics: LineMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let shaped = shaped_gutter_line(text, color, metrics, window);
    let _ = shaped.paint(
        point(right - shaped.width, y),
        metrics.line_height,
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

pub(super) fn paint_gutter_text(
    text: &SharedString,
    x: Pixels,
    y: Pixels,
    color: gpui::Rgba,
    metrics: LineMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let shaped = shaped_gutter_line(text, color, metrics, window);
    let _ = shaped.paint(
        point(x, y),
        metrics.line_height,
        gpui::TextAlign::Left,
        None,
        window,
        cx,
    );
}

fn shaped_gutter_line(
    text: &SharedString,
    color: gpui::Rgba,
    metrics: LineMetrics,
    window: &mut Window,
) -> gpui::ShapedLine {
    let mut style = diff_text_style(window);
    style.color = color.into_color();
    let key = {
        let mut hasher = FxHasher::default();
        text.as_ref().hash(&mut hasher);
        metrics.font_size.hash(&mut hasher);
        style.font_family.hash(&mut hasher);
        style.font_weight.hash(&mut hasher);
        color.red.to_bits().hash(&mut hasher);
        color.green.to_bits().hash(&mut hasher);
        color.blue.to_bits().hash(&mut hasher);
        color.alpha.to_bits().hash(&mut hasher);
        hasher.finish()
    };

    let shaped = GUTTER_TEXT_LAYOUT_CACHE.with(|cache| cache.borrow_mut().get(&key).cloned());
    shaped.unwrap_or_else(|| {
        let run = style.to_run(text.len());
        let shaped = window
            .text_system()
            .shape_line(text.clone(), metrics.font_size, &[run], None);

        GUTTER_TEXT_LAYOUT_CACHE.with(|cache| {
            cache.borrow_mut().put(key, shaped.clone());
        });

        shaped
    })
}
