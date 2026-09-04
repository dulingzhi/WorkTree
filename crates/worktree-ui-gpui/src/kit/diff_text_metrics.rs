use gpui::{Bounds, FontWeight, Pixels, TextStyle, Window, px};

pub(crate) const DIFF_FONT_SCALE: f32 = 0.80;

#[derive(Clone, Copy, Debug)]
pub(crate) struct LineMetrics {
    pub(crate) font_size: Pixels,
    pub(crate) line_height: Pixels,
}

pub(crate) fn diff_text_style(window: &Window) -> TextStyle {
    let mut style = window.text_style();
    style.font_weight = FontWeight::NORMAL;
    style
}

pub(crate) fn line_metrics(window: &Window) -> LineMetrics {
    line_metrics_scaled(window, 1.0)
}

/// Diff-text metrics at `extra_scale` times the base diff font size (1.0 = the
/// regular row text; the annotation "when" column uses a slightly smaller scale).
fn line_metrics_scaled(window: &Window, extra_scale: f32) -> LineMetrics {
    let style = diff_text_style(window);
    let font_size = style.font_size.to_pixels(window.rem_size()) * DIFF_FONT_SCALE * extra_scale;
    let line_height = style
        .line_height
        .to_pixels(font_size.into(), window.rem_size());
    LineMetrics {
        font_size,
        line_height,
    }
}

/// Smaller font metrics for the "X ago" sub-column in the annotation panel.
pub(crate) fn line_metrics_annot_when(window: &Window) -> LineMetrics {
    line_metrics_scaled(window, 0.85)
}

pub(crate) fn center_text_y(bounds: Bounds<Pixels>, line_height: Pixels) -> Pixels {
    let extra = (bounds.size.height - line_height).max(px(0.0));
    bounds.top() + extra * 0.5
}

/// Horizontal row padding: two design units, i.e. half a rem (8px at 100% UI
/// scale).
pub(crate) fn px_2(window: &Window) -> Pixels {
    window.rem_size() * 0.5
}
