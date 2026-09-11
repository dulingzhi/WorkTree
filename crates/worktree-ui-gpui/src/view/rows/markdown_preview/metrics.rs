//! Typography and geometry: the pixel constants, the `ui_scale` helpers, and the
//! per-row layout typed the renderer reads.

use super::chrome::{markdown_preview_inline_highlight, markdown_preview_row_text_color};
use super::{
    AppTheme, FontWeight, FxHasher, MarkdownPreviewRow, MarkdownPreviewRowKind, Pixels, Range,
    SharedString, Window, px,
};

const MARKDOWN_PREVIEW_ROW_HEIGHT_PX: f32 = 28.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_BASE_FONT_PX: f32 = 13.0;
const MARKDOWN_PREVIEW_BASE_LINE_HEIGHT_PX: f32 = 20.0;
pub(in crate::view) const MARKDOWN_PREVIEW_CONTENT_PAD_X_PX: f32 = 18.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX: f32 = 8.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_INDENT_STEP_PX: f32 = 24.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX: f32 = 4.0;
pub(super) const MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_GAP_PX: f32 = 8.0;
pub(super) const MARKDOWN_PREVIEW_BLOCKQUOTE_GUTTER_MARGIN_RIGHT_PX: f32 = 12.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX: f32 = 22.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX: f32 = 10.0;
pub(super) const MARKDOWN_PREVIEW_ALERT_BADGE_FONT_PX: f32 = 11.0;
pub(super) const MARKDOWN_PREVIEW_ALERT_BADGE_PAD_X_PX: f32 = 6.0;
pub(super) const MARKDOWN_PREVIEW_ALERT_BADGE_GAP_PX: f32 = 10.0;
pub(in crate::view::rows) const MARKDOWN_PREVIEW_SHELL_PAD_X_PX: f32 = 12.0;
pub(super) const MARKDOWN_PREVIEW_CODE_BORDER_PX: f32 = 1.0;

pub(super) fn markdown_preview_scaled_px(value: f32, ui_scale_percent: u32) -> Pixels {
    crate::ui_scale::design_px_from_percent(value, ui_scale_percent)
}

pub(super) fn markdown_preview_scaled_value(value: f32, ui_scale_percent: u32) -> f32 {
    let scaled: f32 = markdown_preview_scaled_px(value, ui_scale_percent).into();
    scaled
}

pub(in crate::view::rows) fn markdown_preview_row_height(ui_scale_percent: u32) -> Pixels {
    markdown_preview_scaled_px(MARKDOWN_PREVIEW_ROW_HEIGHT_PX, ui_scale_percent)
}

pub(in crate::view::rows) struct MarkdownPreviewRowTypography {
    pub(in crate::view::rows) font_size: f32,
    pub(in crate::view::rows) line_height: f32,
    pub(in crate::view::rows) font_weight: Option<FontWeight>,
    pub(in crate::view::rows) font_family: Option<SharedString>,
    pub(in crate::view::rows) text_color: gpui::Rgba,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(in crate::view::rows) struct MarkdownPreviewRowLayout {
    pub(in crate::view::rows) top_inset_px: f32,
    pub(in crate::view::rows) bottom_inset_px: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::view::rows) struct MarkdownPreviewRowHorizontalPadding {
    pub(in crate::view::rows) left_px: f32,
    pub(in crate::view::rows) right_px: f32,
}

/// Tallest an inline picture may be when the document declares no size, so a
/// stray screenshot written mid-sentence cannot push the line open.
pub(super) const MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX: f32 = 26.0;

/// Space between an inline picture and whatever shares its line.
///
/// Both previews use it, but only the row grid has to reserve it: that preview
/// measures a row's width to drive horizontal scrolling, so the gap is part of
/// the row chrome there and purely visual in the flowing renderer.
pub(in crate::view) const MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX: f32 = 4.0;

/// Slot an inline picture of unknown size holds while it loads. Wide enough for
/// the badges a README opens with, which is what this mostly stands in for.
pub(super) const MARKDOWN_PREVIEW_INLINE_IMAGE_LOADING_WIDTH_PX: f32 = 90.0;

pub(in crate::view::rows) fn markdown_preview_font_family_hash(font_family: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = FxHasher::default();
    font_family.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn markdown_preview_row_width_cache_key(
    font_size: f32,
    font_weight: FontWeight,
    font_family: &str,
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = FxHasher::default();
    font_size.to_bits().hash(&mut hasher);
    font_weight.hash(&mut hasher);
    font_family.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn markdown_preview_width_affecting_highlights(
    theme: AppTheme,
    row: &MarkdownPreviewRow,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    row.inline_spans
        .iter()
        .filter_map(|span| {
            let style = markdown_preview_inline_highlight(theme, span.style);
            (style.font_weight.is_some() || style.font_style.is_some())
                .then_some((span.byte_range.start..span.byte_range.end, style))
        })
        .collect()
}

pub(super) fn markdown_preview_shape_text_width(
    window: &mut Window,
    text: impl Into<SharedString>,
    font_size_px: f32,
    font_weight: FontWeight,
    font_family: Option<&str>,
    highlights: &[(Range<usize>, gpui::HighlightStyle)],
) -> Pixels {
    let text: SharedString = text.into();
    if text.is_empty() {
        return px(0.0);
    }

    let mut style = window.text_style();
    style.font_weight = font_weight;
    if let Some(font_family) = font_family {
        style.font_family = font_family.to_string().into();
    }

    let runs = crate::text_runs::text_runs_for_highlights(text.as_ref(), &style, highlights);

    window
        .text_system()
        .shape_line(text, px(font_size_px), &runs, None)
        .width
}

pub(in crate::view::rows) fn markdown_preview_row_layout(
    row: &MarkdownPreviewRow,
    ui_scale_percent: u32,
) -> MarkdownPreviewRowLayout {
    let scaled = |value: f32| markdown_preview_scaled_value(value, ui_scale_percent);
    match row.kind {
        // Headings are inset evenly so the text sits centred in its row rather
        // than riding high with a gap underneath. The section break above a
        // top-level heading is a spacer row; these insets are the smaller gap
        // that surrounds the heading text itself.
        MarkdownPreviewRowKind::Heading { level: 1 | 2 } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(2.0),
            bottom_inset_px: scaled(2.0),
        },
        MarkdownPreviewRowKind::Heading { level: 3 } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(3.0),
            bottom_inset_px: scaled(3.0),
        },
        MarkdownPreviewRowKind::Heading { .. } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(4.0),
            bottom_inset_px: scaled(4.0),
        },
        MarkdownPreviewRowKind::DetailsSummary => MarkdownPreviewRowLayout {
            top_inset_px: scaled(0.0),
            bottom_inset_px: scaled(0.0),
        },
        MarkdownPreviewRowKind::Paragraph => MarkdownPreviewRowLayout {
            top_inset_px: scaled(2.0),
            bottom_inset_px: scaled(6.0),
        },
        MarkdownPreviewRowKind::BlockquoteLine => MarkdownPreviewRowLayout {
            top_inset_px: scaled(2.0),
            bottom_inset_px: scaled(6.0),
        },
        MarkdownPreviewRowKind::ListItem { .. } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(0.0),
            bottom_inset_px: scaled(0.0),
        },
        MarkdownPreviewRowKind::CodeLine { is_first, is_last } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(if is_first { 5.0 } else { 0.0 }),
            bottom_inset_px: scaled(if is_last { 5.0 } else { 0.0 }),
        },
        MarkdownPreviewRowKind::ThematicBreak => MarkdownPreviewRowLayout {
            top_inset_px: scaled(6.0),
            bottom_inset_px: scaled(6.0),
        },
        // The bands of an image block must tile without gaps.
        MarkdownPreviewRowKind::Image { .. } => MarkdownPreviewRowLayout {
            top_inset_px: scaled(0.0),
            bottom_inset_px: scaled(0.0),
        },
        MarkdownPreviewRowKind::Spacer => MarkdownPreviewRowLayout {
            top_inset_px: scaled(0.0),
            bottom_inset_px: scaled(0.0),
        },
        MarkdownPreviewRowKind::TableRow { .. } | MarkdownPreviewRowKind::PlainFallback => {
            MarkdownPreviewRowLayout {
                top_inset_px: scaled(2.0),
                bottom_inset_px: scaled(2.0),
            }
        }
    }
}

pub(in crate::view::rows) fn markdown_preview_row_typography(
    theme: AppTheme,
    row: &MarkdownPreviewRow,
    editor_font_family: &SharedString,
    ui_scale_percent: u32,
) -> MarkdownPreviewRowTypography {
    let text_color = markdown_preview_row_text_color(theme, row);
    let scaled = |value: f32| markdown_preview_scaled_value(value, ui_scale_percent);
    match row.kind {
        MarkdownPreviewRowKind::Heading { level: 1 } => MarkdownPreviewRowTypography {
            font_size: scaled(28.0),
            line_height: scaled(28.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::Heading { level: 2 } => MarkdownPreviewRowTypography {
            font_size: scaled(24.0),
            line_height: scaled(24.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::Heading { level: 3 } => MarkdownPreviewRowTypography {
            font_size: scaled(20.0),
            line_height: scaled(22.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::Heading { level: 4 } => MarkdownPreviewRowTypography {
            font_size: scaled(18.0),
            line_height: scaled(20.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::Heading { level: 5 } => MarkdownPreviewRowTypography {
            font_size: scaled(16.0),
            line_height: scaled(18.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::Heading { level: 6 } => MarkdownPreviewRowTypography {
            font_size: scaled(14.0),
            line_height: scaled(16.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::DetailsSummary => MarkdownPreviewRowTypography {
            font_size: scaled(MARKDOWN_PREVIEW_BASE_FONT_PX),
            line_height: scaled(28.0),
            font_weight: Some(FontWeight::BOLD),
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::ListItem { .. } => MarkdownPreviewRowTypography {
            font_size: scaled(MARKDOWN_PREVIEW_BASE_FONT_PX),
            line_height: scaled(MARKDOWN_PREVIEW_BASE_LINE_HEIGHT_PX),
            font_weight: None,
            font_family: None,
            text_color,
        },
        MarkdownPreviewRowKind::CodeLine { .. } => MarkdownPreviewRowTypography {
            font_size: scaled(12.0),
            line_height: scaled(18.0),
            font_weight: None,
            font_family: Some(editor_font_family.clone()),
            text_color,
        },
        MarkdownPreviewRowKind::TableRow { is_header } => MarkdownPreviewRowTypography {
            font_size: scaled(12.0),
            line_height: scaled(18.0),
            font_weight: is_header.then_some(FontWeight::BOLD),
            font_family: Some(editor_font_family.clone()),
            text_color,
        },
        MarkdownPreviewRowKind::PlainFallback => MarkdownPreviewRowTypography {
            font_size: scaled(12.0),
            line_height: scaled(18.0),
            font_weight: None,
            font_family: Some(editor_font_family.clone()),
            text_color,
        },
        _ => MarkdownPreviewRowTypography {
            font_size: scaled(MARKDOWN_PREVIEW_BASE_FONT_PX),
            line_height: scaled(MARKDOWN_PREVIEW_BASE_LINE_HEIGHT_PX),
            font_weight: None,
            font_family: None,
            text_color,
        },
    }
}

pub(in crate::view::rows) fn markdown_preview_row_horizontal_padding(
    row: &MarkdownPreviewRow,
    ui_scale_percent: u32,
) -> MarkdownPreviewRowHorizontalPadding {
    let indent_steps = f32::from(row.indent_level.saturating_sub(1));
    let default_left_px = markdown_preview_scaled_value(
        MARKDOWN_PREVIEW_CONTENT_PAD_X_PX + indent_steps * MARKDOWN_PREVIEW_INDENT_STEP_PX,
        ui_scale_percent,
    );

    match row.kind {
        MarkdownPreviewRowKind::CodeLine { .. } => MarkdownPreviewRowHorizontalPadding {
            // Fenced code blocks ignore surrounding list indentation but keep
            // a small edge gap so the boxed shell does not touch the preview edge.
            left_px: markdown_preview_scaled_value(
                MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX,
                ui_scale_percent,
            ),
            right_px: markdown_preview_scaled_value(
                MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX,
                ui_scale_percent,
            ),
        },
        _ => MarkdownPreviewRowHorizontalPadding {
            left_px: default_left_px,
            right_px: markdown_preview_scaled_value(
                MARKDOWN_PREVIEW_CONTENT_PAD_X_PX,
                ui_scale_percent,
            ),
        },
    }
}
