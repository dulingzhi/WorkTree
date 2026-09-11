//! Row chrome: markers, alert bars, blockquote gutters and the background/colour
//! helpers the row element paints with.

use super::IntoColor;
use super::metrics::{
    MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_GAP_PX, MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX,
    MARKDOWN_PREVIEW_BLOCKQUOTE_GUTTER_MARGIN_RIGHT_PX, markdown_preview_scaled_px,
};
use super::{
    AnyElement, App, AppTheme, Arc, CachedDiffStyledText, DiffSyntaxMode, FontWeight, MainPaneView,
    MarkdownAlertKind, MarkdownChangeHint, MarkdownInlineStyle, MarkdownPreviewRow,
    MarkdownPreviewRowKind, Pixels, Range, SharedString, Window, build_cached_diff_styled_text,
    build_cached_diff_styled_text_from_relative_highlights, div, px, with_alpha,
};
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Styled;

pub(super) struct MarkdownPreviewSharedHighlightsText {
    text: SharedString,
    highlights: Arc<[(Range<usize>, gpui::HighlightStyle)]>,
    inner: Option<gpui::StyledText>,
}

impl MarkdownPreviewSharedHighlightsText {
    pub(super) fn new(
        text: SharedString,
        highlights: Arc<[(Range<usize>, gpui::HighlightStyle)]>,
    ) -> Self {
        Self {
            text,
            highlights,
            inner: None,
        }
    }
}

impl gpui::Element for MarkdownPreviewSharedHighlightsText {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut inner = gpui::StyledText::new(self.text.clone())
            .with_default_highlights(&window.text_style(), self.highlights.iter().cloned());
        let layout = inner.request_layout(id, inspector_id, window, cx);
        self.inner = Some(inner);
        layout
    }

    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .as_mut()
            .expect("markdown preview shared-highlights text should be laid out before prepaint")
            .prepaint(id, inspector_id, bounds, request_layout, window, cx);
    }

    fn paint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.inner
            .as_mut()
            .expect("markdown preview shared-highlights text should be laid out before paint")
            .paint(
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            );
    }
}

impl gpui::IntoElement for MarkdownPreviewSharedHighlightsText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// Accent colour for an alert blockquote, shared with the flowing renderer.
pub(in crate::view) fn markdown_preview_alert_bar_color(
    theme: AppTheme,
    kind: MarkdownAlertKind,
) -> gpui::Rgba {
    markdown_preview_alert_color(theme, kind)
}

/// Badge label for an alert blockquote, shared with the flowing renderer.
pub(in crate::view) fn markdown_preview_alert_label(
    kind: MarkdownAlertKind,
) -> Option<SharedString> {
    Some(SharedString::new_static(match kind {
        MarkdownAlertKind::Note => "NOTE",
        MarkdownAlertKind::Tip => "TIP",
        MarkdownAlertKind::Important => "IMPORTANT",
        MarkdownAlertKind::Warning => "WARNING",
        MarkdownAlertKind::Caution => "CAUTION",
    }))
}

/// Gutter colour the flowing markdown preview marks a wholly added or removed
/// file with, shared with the source preview so the two agree.
pub(in crate::view) fn worktree_markdown_preview_bar_color(
    this: &MainPaneView,
    theme: AppTheme,
) -> Option<gpui::Rgba> {
    worktree_preview_bar_color(this, theme)
}

pub(in crate::view::rows) fn worktree_preview_bar_color(
    this: &MainPaneView,
    theme: AppTheme,
) -> Option<gpui::Rgba> {
    let highlight_deleted_file = this.deleted_file_preview_abs_path().is_some();
    let highlight_new_file = this.untracked_worktree_preview_path().is_some()
        || this.added_file_preview_abs_path().is_some()
        || this.diff_preview_is_new_file;
    if highlight_deleted_file {
        Some(theme.colors.status.danger.foreground)
    } else if highlight_new_file {
        Some(theme.colors.status.success.foreground)
    } else {
        None
    }
}

pub(in crate::view::rows) fn markdown_preview_row_styled_text(
    theme: AppTheme,
    row: &MarkdownPreviewRow,
) -> &CachedDiffStyledText {
    row.styled_text_cache.get_or_init(theme.is_dark, || {
        if matches!(row.kind, MarkdownPreviewRowKind::CodeLine { .. }) {
            return build_cached_diff_styled_text(
                theme,
                row.text.as_ref(),
                &[],
                "",
                row.code_language,
                DiffSyntaxMode::Auto,
                None,
            );
        }

        let highlights = row
            .inline_spans
            .iter()
            .filter_map(|span| {
                let style = markdown_preview_inline_highlight(theme, span.style);
                (style != gpui::HighlightStyle::default())
                    .then_some((span.byte_range.start..span.byte_range.end, style))
            })
            .collect::<Vec<_>>();
        build_cached_diff_styled_text_from_relative_highlights(row.text.as_ref(), &highlights)
    })
}

pub(in crate::view::rows) fn markdown_preview_row_marker(
    row: &MarkdownPreviewRow,
) -> Option<SharedString> {
    if let Some(label) = row.footnote_label.as_ref() {
        return Some(format!("[^{}]:", label.as_ref()).into());
    }

    match row.kind {
        MarkdownPreviewRowKind::DetailsSummary => Some("v".into()),
        MarkdownPreviewRowKind::ListItem { number: Some(n) } => Some(format!("{n}.").into()),
        MarkdownPreviewRowKind::ListItem { number: None } => Some("•".into()),
        _ => None,
    }
}

pub(in crate::view::rows) fn markdown_preview_alert_title_label(
    row: &MarkdownPreviewRow,
) -> Option<&'static str> {
    if !row.starts_alert {
        return None;
    }

    match row.alert_kind? {
        MarkdownAlertKind::Note => Some("NOTE"),
        MarkdownAlertKind::Tip => Some("TIP"),
        MarkdownAlertKind::Important => Some("IMPORTANT"),
        MarkdownAlertKind::Warning => Some("WARNING"),
        MarkdownAlertKind::Caution => Some("CAUTION"),
    }
}

pub(super) fn markdown_preview_alert_color(theme: AppTheme, kind: MarkdownAlertKind) -> gpui::Rgba {
    match kind {
        MarkdownAlertKind::Note => theme.colors.accent.foreground,
        MarkdownAlertKind::Tip => theme.colors.status.success.foreground,
        MarkdownAlertKind::Important => with_alpha(theme.colors.accent.foreground, 0.85),
        MarkdownAlertKind::Warning => theme.colors.status.warning.foreground,
        MarkdownAlertKind::Caution => theme.colors.status.danger.foreground,
    }
}

pub(super) fn markdown_preview_blockquote_gutter(
    theme: AppTheme,
    blockquote_level: u8,
    alert_kind: Option<MarkdownAlertKind>,
    ui_scale_percent: u32,
) -> Option<AnyElement> {
    if blockquote_level == 0 {
        return None;
    }

    let quote_bar_color = with_alpha(
        theme.colors.stroke.default,
        if theme.is_dark { 0.96 } else { 0.86 },
    );
    let alert_bar_color = alert_kind.map(|kind| markdown_preview_alert_color(theme, kind));
    let bars = (0..blockquote_level)
        .map(|ix| {
            let bar_color = if ix + 1 == blockquote_level {
                alert_bar_color.unwrap_or(quote_bar_color)
            } else {
                quote_bar_color
            };
            div()
                .w(markdown_preview_scaled_px(
                    MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX,
                    ui_scale_percent,
                ))
                .h_full()
                .bg(bar_color)
                .rounded(markdown_preview_scaled_px(2.0, ui_scale_percent))
                .into_any_element()
        })
        .collect::<Vec<_>>();

    Some(
        div()
            .flex_none()
            .h_full()
            .flex()
            .gap(markdown_preview_scaled_px(
                MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_GAP_PX,
                ui_scale_percent,
            ))
            .mr(markdown_preview_scaled_px(
                MARKDOWN_PREVIEW_BLOCKQUOTE_GUTTER_MARGIN_RIGHT_PX,
                ui_scale_percent,
            ))
            .children(bars)
            .into_any_element(),
    )
}

pub(in crate::view::rows) fn markdown_preview_inline_highlight(
    theme: AppTheme,
    style: MarkdownInlineStyle,
) -> gpui::HighlightStyle {
    match style {
        MarkdownInlineStyle::Normal => gpui::HighlightStyle::default(),
        MarkdownInlineStyle::Bold => gpui::HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::Italic => gpui::HighlightStyle {
            font_style: Some(gpui::FontStyle::Italic),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::BoldItalic => gpui::HighlightStyle {
            font_weight: Some(FontWeight::BOLD),
            font_style: Some(gpui::FontStyle::Italic),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::Code => gpui::HighlightStyle {
            background_color: Some(
                with_alpha(
                    theme.colors.interaction.selected_background,
                    if theme.is_dark { 0.75 } else { 0.55 },
                )
                .into_color(),
            ),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::Strikethrough => gpui::HighlightStyle {
            color: Some(theme.colors.foreground.secondary.into_color()),
            strikethrough: Some(gpui::StrikethroughStyle {
                thickness: px(1.0),
                color: Some(theme.colors.foreground.secondary.into_color()),
            }),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::Link => gpui::HighlightStyle {
            color: Some(theme.colors.accent.foreground.into_color()),
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.0),
                color: Some(theme.colors.accent.foreground.into_color()),
                wavy: false,
            }),
            ..gpui::HighlightStyle::default()
        },
        MarkdownInlineStyle::Underline => gpui::HighlightStyle {
            underline: Some(gpui::UnderlineStyle {
                thickness: px(1.0),
                color: Some(theme.colors.foreground.primary.into_color()),
                wavy: false,
            }),
            ..gpui::HighlightStyle::default()
        },
    }
}

pub(super) fn markdown_preview_row_text_color(
    theme: AppTheme,
    row: &MarkdownPreviewRow,
) -> gpui::Rgba {
    if row.alert_kind.is_some() {
        return theme.colors.foreground.primary;
    }

    match row.kind {
        MarkdownPreviewRowKind::Heading { level: 6 } | MarkdownPreviewRowKind::BlockquoteLine => {
            theme.colors.foreground.secondary
        }
        MarkdownPreviewRowKind::Heading { .. } => theme.colors.foreground.primary,
        MarkdownPreviewRowKind::ThematicBreak => theme.colors.foreground.secondary,
        MarkdownPreviewRowKind::PlainFallback => theme.colors.status.warning.foreground,
        _ => theme.colors.foreground.primary,
    }
}

pub(super) fn markdown_preview_code_background(theme: AppTheme) -> gpui::Rgba {
    if theme.is_dark {
        with_alpha(theme.colors.surface.raised, 0.88)
    } else {
        with_alpha(theme.colors.surface.panel, 0.86)
    }
}

/// The wash a row carries in its own right: a diff change hint, an alert's
/// tint, or the warning band on a line the parser could not interpret.
pub(in crate::view) fn markdown_preview_row_background(
    theme: AppTheme,
    row: &MarkdownPreviewRow,
) -> Option<gpui::Rgba> {
    use MarkdownChangeHint as Hint;
    use MarkdownPreviewRowKind as Kind;

    match row.change_hint {
        Hint::Added => Some(with_alpha(
            theme.colors.status.success.foreground,
            if theme.is_dark { 0.18 } else { 0.12 },
        )),
        Hint::Removed => Some(with_alpha(
            theme.colors.status.danger.foreground,
            if theme.is_dark { 0.16 } else { 0.10 },
        )),
        Hint::Modified => Some(with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.18 } else { 0.10 },
        )),
        Hint::None => {
            if let Some(alert_kind) = row.alert_kind {
                return Some(with_alpha(
                    markdown_preview_alert_color(theme, alert_kind),
                    if theme.is_dark { 0.10 } else { 0.06 },
                ));
            }

            match row.kind {
                Kind::PlainFallback => Some(with_alpha(
                    theme.colors.status.warning.foreground,
                    if theme.is_dark { 0.08 } else { 0.06 },
                )),
                _ => None,
            }
        }
    }
}
