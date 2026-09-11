//! Row wrap measurement: how far a row reaches and where its wrap points are.

use super::chrome::{markdown_preview_alert_title_label, markdown_preview_row_marker};
use super::metrics::{
    MARKDOWN_PREVIEW_ALERT_BADGE_FONT_PX, MARKDOWN_PREVIEW_ALERT_BADGE_GAP_PX,
    MARKDOWN_PREVIEW_ALERT_BADGE_PAD_X_PX, MARKDOWN_PREVIEW_BASE_FONT_PX,
    MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_GAP_PX, MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX,
    MARKDOWN_PREVIEW_BLOCKQUOTE_GUTTER_MARGIN_RIGHT_PX, MARKDOWN_PREVIEW_CODE_BORDER_PX,
    MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX, MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX,
    MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX, MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX,
    MARKDOWN_PREVIEW_SHELL_PAD_X_PX, markdown_preview_row_horizontal_padding,
    markdown_preview_row_typography, markdown_preview_row_width_cache_key,
    markdown_preview_scaled_px, markdown_preview_scaled_value, markdown_preview_shape_text_width,
    markdown_preview_width_affecting_highlights,
};
use super::{
    AppTheme, DIFF_WRAP_TAB_EXPANDED_COLUMNS, FontWeight, MarkdownPreviewRow,
    MarkdownPreviewRowKind, MarkdownPreviewWrapKey, Pixels, Range, SharedString, Window, px,
};

/// Inputs shared by every row of one wrap pass.
pub(in crate::view::rows) struct MarkdownPreviewWrapMeasure {
    pub(in crate::view::rows) key: MarkdownPreviewWrapKey,
    pub(in crate::view::rows) wrap_width: Pixels,
    pub(in crate::view::rows) editor_font_family: SharedString,
    pub(in crate::view::rows) ui_scale_percent: u32,
}

impl MarkdownPreviewWrapMeasure {
    /// Per-row wrap callback for the plan builders.
    pub(in crate::view::rows) fn wrap_row_fn<'a>(
        &'a self,
        window: &'a mut Window,
        theme: AppTheme,
    ) -> impl FnMut(&MarkdownPreviewRow) -> Vec<Range<usize>> + 'a {
        move |row| {
            markdown_preview_row_wrap_ranges(
                window,
                theme,
                row,
                self.wrap_width,
                &self.editor_font_family,
                self.ui_scale_percent,
            )
        }
    }
}

pub(in crate::view::rows) fn markdown_preview_row_required_width(
    window: &mut Window,
    theme: AppTheme,
    row: &MarkdownPreviewRow,
    editor_font_family: &SharedString,
    ui_scale_percent: u32,
) -> Pixels {
    if matches!(row.kind, MarkdownPreviewRowKind::Spacer) {
        return px(0.0);
    }

    let typography =
        markdown_preview_row_typography(theme, row, editor_font_family, ui_scale_percent);
    // Word wrap measures every row of the document, so the ambient text style
    // — which `Window::text_style` rebuilds from the style stack on each call
    // — is only consulted for rows that do not carry their own family.
    let resolved_font_family = typography
        .font_family
        .clone()
        .unwrap_or_else(|| window.text_style().font_family.clone());
    let cache_key = markdown_preview_row_width_cache_key(
        typography.font_size,
        typography.font_weight.unwrap_or(FontWeight::NORMAL),
        resolved_font_family.as_ref(),
    );
    let base_width = row.measured_width_px.get_or_init(cache_key, || {
        let base_font_weight = typography.font_weight.unwrap_or(FontWeight::NORMAL);
        let text_width = if matches!(row.kind, MarkdownPreviewRowKind::ThematicBreak) {
            px(0.0)
        } else {
            let highlights = markdown_preview_width_affecting_highlights(theme, row);
            markdown_preview_shape_text_width(
                window,
                row.text.clone(),
                typography.font_size,
                base_font_weight,
                typography.font_family.as_ref().map(SharedString::as_ref),
                &highlights,
            )
        };

        let width = text_width + markdown_preview_row_chrome_width(window, row, ui_scale_percent);
        u32::from(width.round())
    });

    px(base_width as f32)
}

/// Width a row spends on everything that is not its text: padding, blockquote
/// gutter, list marker, alert badge, and the code/table shell.
///
/// `markdown_preview_row_required_width` adds this to the shaped text width;
/// word wrap subtracts it from the viewport to get the width the text may
/// occupy.
fn markdown_preview_row_chrome_width(
    window: &mut Window,
    row: &MarkdownPreviewRow,
    ui_scale_percent: u32,
) -> Pixels {
    let horizontal_padding = markdown_preview_row_horizontal_padding(row, ui_scale_percent);
    let mut width = px(horizontal_padding.left_px + horizontal_padding.right_px);

    if row.blockquote_level > 0 {
        width += px(f32::from(row.blockquote_level)
            * markdown_preview_scaled_value(
                MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX,
                ui_scale_percent,
            )
            + f32::from(row.blockquote_level.saturating_sub(1))
                * markdown_preview_scaled_value(
                    MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_GAP_PX,
                    ui_scale_percent,
                )
            + markdown_preview_scaled_value(
                MARKDOWN_PREVIEW_BLOCKQUOTE_GUTTER_MARGIN_RIGHT_PX,
                ui_scale_percent,
            ));
    }

    if let Some(marker) = markdown_preview_row_marker(row) {
        let marker_width = markdown_preview_shape_text_width(
            window,
            marker,
            markdown_preview_scaled_value(MARKDOWN_PREVIEW_BASE_FONT_PX, ui_scale_percent),
            FontWeight::NORMAL,
            None,
            &[],
        );
        width += marker_width.max(markdown_preview_scaled_px(
            MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX,
            ui_scale_percent,
        ));
        width += markdown_preview_scaled_px(MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX, ui_scale_percent);
    }

    if let Some(alert_title) = markdown_preview_alert_title_label(row) {
        let alert_width = markdown_preview_shape_text_width(
            window,
            alert_title,
            markdown_preview_scaled_value(MARKDOWN_PREVIEW_ALERT_BADGE_FONT_PX, ui_scale_percent),
            FontWeight::BOLD,
            None,
            &[],
        );
        width += alert_width
            + markdown_preview_scaled_px(
                MARKDOWN_PREVIEW_ALERT_BADGE_PAD_X_PX * 2.0,
                ui_scale_percent,
            );
        width += markdown_preview_scaled_px(MARKDOWN_PREVIEW_ALERT_BADGE_GAP_PX, ui_scale_percent);
    }

    // Pictures painted on this line push the text right and widen the row.
    // Their natural size is only known once loaded, so a declared width is used
    // where there is one and the inline height cap stands in otherwise — the
    // point is that the row is not measured as if the pictures were absent.
    for inline in row.inline_images.iter() {
        let reserved = inline
            .image
            .width_px
            .map(|width| width as f32)
            .unwrap_or(MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX);
        width += markdown_preview_scaled_px(reserved, ui_scale_percent);
        width += markdown_preview_scaled_px(MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX, ui_scale_percent);
    }

    width += match row.kind {
        MarkdownPreviewRowKind::CodeLine { .. } => markdown_preview_scaled_px(
            MARKDOWN_PREVIEW_SHELL_PAD_X_PX * 2.0 + MARKDOWN_PREVIEW_CODE_BORDER_PX * 2.0,
            ui_scale_percent,
        ),
        MarkdownPreviewRowKind::TableRow { .. } | MarkdownPreviewRowKind::PlainFallback => {
            markdown_preview_scaled_px(MARKDOWN_PREVIEW_SHELL_PAD_X_PX * 2.0, ui_scale_percent)
        }
        _ => px(0.0),
    };

    width
}

/// Byte ranges of `row.text` that fit `available_width`, one per visual row.
///
/// Returns fewer than two ranges when the row needs no wrapping, which
/// `build_markdown_preview_wrap_plan` collapses back to a single visual row.
/// Wrapping is measured with the row's own typography — headings, code, and
/// body text all use different fonts — via `gpui`'s line wrapper rather than a
/// character-count approximation, because preview text is proportional.
///
/// Ranges are in `row.text` coordinates; the renderer maps them onto the
/// tab-expanded text it paints (see `markdown_preview_expanded_slice_range`).
pub(in crate::view::rows) fn markdown_preview_row_wrap_ranges(
    window: &mut Window,
    theme: AppTheme,
    row: &MarkdownPreviewRow,
    available_width: Pixels,
    editor_font_family: &SharedString,
    ui_scale_percent: u32,
) -> Vec<Range<usize>> {
    if row.text.is_empty()
        || matches!(
            row.kind,
            MarkdownPreviewRowKind::Spacer | MarkdownPreviewRowKind::ThematicBreak
        )
    {
        return Vec::new();
    }

    // Rows that already fit need no wrapper pass at all. The required width is
    // cached per row and keyed only by font, so on a resize this is a hash and
    // a comparison rather than a re-measure — which is what keeps a wide
    // document from re-shaping every row on every frame of a resize drag.
    if markdown_preview_row_required_width(window, theme, row, editor_font_family, ui_scale_percent)
        <= available_width
    {
        return Vec::new();
    }

    let chrome = markdown_preview_row_chrome_width(window, row, ui_scale_percent);
    let wrap_width = available_width - chrome;
    if wrap_width <= px(0.0) {
        return Vec::new();
    }

    let typography =
        markdown_preview_row_typography(theme, row, editor_font_family, ui_scale_percent);
    let mut font = window.text_style().font();
    if let Some(font_family) = typography.font_family.clone() {
        font.family = font_family;
    }
    if let Some(font_weight) = typography.font_weight {
        font.weight = font_weight;
    }

    let text = row.text.clone();
    // A tab is painted as four spaces, so it is fed to the wrapper as an
    // element of that width rather than as a single character.
    let tab_width = text.contains('\t').then(|| {
        markdown_preview_shape_text_width(
            window,
            "    ",
            typography.font_size,
            typography.font_weight.unwrap_or(FontWeight::NORMAL),
            typography.font_family.as_ref().map(SharedString::as_ref),
            &[],
        )
    });
    let mut handle = window
        .text_system()
        .line_wrapper(font, px(typography.font_size));
    // Prose has no tabs, so the common case stays on the stack.
    let tabbed_fragments =
        tab_width.map(|width| markdown_preview_wrap_fragments(text.as_ref(), width));
    let plain_fragment = [gpui::LineFragment::text(text.as_ref())];
    let fragments: &[gpui::LineFragment<'_>] = match tabbed_fragments.as_deref() {
        Some(fragments) => fragments,
        None => &plain_fragment,
    };
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for boundary in handle.wrap_line(fragments, wrap_width) {
        if boundary.ix <= start || !text.is_char_boundary(boundary.ix) {
            continue;
        }
        ranges.push(start..boundary.ix);
        start = boundary.ix;
    }
    if ranges.is_empty() {
        return Vec::new();
    }
    ranges.push(start..text.len());
    ranges
}

/// Split `text` into wrap fragments, giving each tab the width it is painted
/// at ([`DIFF_WRAP_TAB_EXPANDED_COLUMNS`] spaces) instead of a single character.
fn markdown_preview_wrap_fragments(text: &str, tab_width: Pixels) -> Vec<gpui::LineFragment<'_>> {
    let mut fragments = Vec::new();
    let mut segment_start = 0usize;
    for (ix, _) in text.match_indices('\t') {
        if ix > segment_start {
            fragments.push(gpui::LineFragment::text(&text[segment_start..ix]));
        }
        fragments.push(gpui::LineFragment::element(tab_width, 1));
        segment_start = ix + 1;
    }
    if segment_start < text.len() {
        fragments.push(gpui::LineFragment::text(&text[segment_start..]));
    }
    fragments
}

/// Map a `row.text` byte range onto the tab-expanded text that is painted.
///
/// Styled preview text replaces every tab with [`DIFF_WRAP_TAB_EXPANDED_COLUMNS`]
/// spaces, so raw offsets would slice the painted text in the wrong place —
/// shifted by three bytes per preceding tab, and cutting the tail short.
pub(in crate::view::rows) fn markdown_preview_expanded_slice_range(
    raw_text: &str,
    expanded_len: usize,
    range: &Range<usize>,
) -> Range<usize> {
    if expanded_len == raw_text.len() {
        return range.clone();
    }

    let expand = |offset: usize| {
        let offset = offset.min(raw_text.len());
        let tabs = raw_text.as_bytes()[..offset]
            .iter()
            .filter(|byte| **byte == b'\t')
            .count();
        offset + tabs * (DIFF_WRAP_TAB_EXPANDED_COLUMNS - 1)
    };

    expand(range.start)..expand(range.end)
}
