//! The row element itself, the document-level entry point, and the query/reveal
//! state the preview rows are rendered against.

use super::chrome::{
    MarkdownPreviewSharedHighlightsText, markdown_preview_alert_color,
    markdown_preview_alert_title_label, markdown_preview_blockquote_gutter,
    markdown_preview_code_background, markdown_preview_row_background, markdown_preview_row_marker,
    markdown_preview_row_styled_text,
};
use super::images::{
    markdown_preview_image_row, markdown_preview_inline_image, markdown_preview_no_picture_sizes,
};
use super::metrics::{
    MARKDOWN_PREVIEW_ALERT_BADGE_FONT_PX, MARKDOWN_PREVIEW_ALERT_BADGE_GAP_PX,
    MARKDOWN_PREVIEW_ALERT_BADGE_PAD_X_PX, MARKDOWN_PREVIEW_BASE_FONT_PX,
    MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX, MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX,
    MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX, MARKDOWN_PREVIEW_SHELL_PAD_X_PX,
    markdown_preview_row_height, markdown_preview_row_horizontal_padding,
    markdown_preview_row_layout, markdown_preview_row_typography, markdown_preview_scaled_px,
};
use super::wrap::markdown_preview_expanded_slice_range;
use super::{
    AnyElement, AppTheme, Arc, CachedDiffStyledText, DiffSearchMatchEmphasis, DiffSearchMatcher,
    DiffTextRegion, DiffTextSelectionOverlay, Entity, FontWeight, IntoElement, MainPaneView,
    MarkdownInlineImage, MarkdownPreviewDocument, MarkdownPreviewRow, MarkdownPreviewRowKind,
    MarkdownPreviewVisualRow, MarkdownPreviewWrapPlan, Pixels, Range, SharedString,
    ViewPerfRenderLane, ViewPerfSpan, build_cached_diff_query_overlay_styled_text, div, perf, px,
    slice_cached_diff_styled_text, with_alpha,
};
use gpui::InteractiveElement;
use gpui::ParentElement;
use gpui::Styled;
use gpui::prelude::FluentBuilder;

pub(in crate::view::rows) struct MarkdownPreviewRenderContext<'a> {
    pub(in crate::view::rows) theme: AppTheme,
    pub(in crate::view::rows) min_width: Pixels,
    pub(in crate::view::rows) editor_font_family: SharedString,
    pub(in crate::view::rows) ui_scale_percent: u32,
    pub(in crate::view::rows) view: Option<Entity<MainPaneView>>,
    pub(in crate::view::rows) text_region: DiffTextRegion,
    /// Visual-row mapping when word wrap is on; `None` renders one row per
    /// source row with horizontal overflow clipped.
    pub(in crate::view::rows) wrap_plan: Option<&'a MarkdownPreviewWrapPlan>,
    /// Directory relative image paths resolve against.
    pub(in crate::view::rows) image_base_dir: Option<Arc<std::path::Path>>,
    /// Quick-search state, when the search box is open over this preview.
    pub(in crate::view::rows) query: Option<MarkdownPreviewQuery>,
}

pub(in crate::view::rows) fn render_markdown_preview_document_rows(
    document: &MarkdownPreviewDocument,
    range: Range<usize>,
    context: &MarkdownPreviewRenderContext<'_>,
) -> Vec<AnyElement> {
    let requested_rows = range.len();
    let mut rows = Vec::with_capacity(requested_rows);
    if let Some(plan) = context.wrap_plan {
        let start = range.start.min(plan.len());
        let end = range.end.min(plan.len());
        for visual_ix in start..end {
            let Some(visual_row) = plan.get(visual_ix) else {
                continue;
            };
            let Some(row) = document.rows.get(visual_row.row_ix) else {
                continue;
            };
            rows.push(markdown_preview_row_element(
                row,
                visual_ix,
                Some(visual_row),
                context,
            ));
        }
    } else {
        let start = range.start.min(document.rows.len());
        let end = range.end.min(document.rows.len());
        for (offset, row) in document.rows[start..end].iter().enumerate() {
            rows.push(markdown_preview_row_element(
                row,
                start + offset,
                None,
                context,
            ));
        }
    }
    perf::record_row_batch(
        ViewPerfRenderLane::MarkdownPreview,
        requested_rows,
        rows.len(),
    );
    rows
}

fn markdown_preview_row_element(
    row: &MarkdownPreviewRow,
    row_ix: usize,
    visual_row: Option<&MarkdownPreviewVisualRow>,
    context: &MarkdownPreviewRenderContext<'_>,
) -> AnyElement {
    let theme = context.theme;
    let min_width = context.min_width;
    let text_region = context.text_region;
    let ui_scale_percent = context.ui_scale_percent;
    let is_interactive = context.view.is_some();
    let _perf_scope = perf::span(ViewPerfSpan::MarkdownPreviewStyledRowBuild);
    if matches!(row.kind, MarkdownPreviewRowKind::Spacer) {
        return div()
            .relative()
            .h(markdown_preview_row_height(ui_scale_percent))
            .min_h(markdown_preview_row_height(ui_scale_percent))
            .w(min_width)
            .min_w(min_width)
            .into_any_element();
    }

    if let MarkdownPreviewRowKind::Image {
        slice_ix,
        slice_count,
    } = row.kind
    {
        // Image bands carry none of the text machinery — no marker, no
        // selection overlay, no styled runs — so they short-circuit here.
        let padding = markdown_preview_row_horizontal_padding(row, ui_scale_percent);
        return div()
            .relative()
            .h(markdown_preview_row_height(ui_scale_percent))
            .min_h(markdown_preview_row_height(ui_scale_percent))
            .w(min_width)
            .min_w(min_width)
            .flex()
            .items_center()
            .when_some(markdown_preview_row_background(theme, row), |div, bg| {
                div.bg(bg)
            })
            .child(
                div()
                    .flex_grow(1.)
                    .min_w(px(0.0))
                    .w_full()
                    .h_full()
                    .pl(px(padding.left_px))
                    .pr(px(padding.right_px))
                    .child(markdown_preview_image_row(
                        row,
                        row_ix,
                        slice_ix,
                        slice_count,
                        context,
                    )),
            )
            .into_any_element();
    }

    let is_continuation = visual_row.is_some_and(MarkdownPreviewVisualRow::is_continuation);
    let row_layout = markdown_preview_row_layout(row, ui_scale_percent);
    let typography =
        markdown_preview_row_typography(theme, row, &context.editor_font_family, ui_scale_percent);
    let full_styled =
        markdown_preview_styled_row_with_query(theme, row, row_ix, context.query.as_ref());
    let full_styled = full_styled.as_ref();
    // Wrapped rows paint one slice of the row's text each; the marker and
    // alert badge belong to the first slice so continuations stay aligned
    // under the text they continue.
    let sliced_styled = visual_row
        .filter(|visual| visual.byte_range != (0..row.text.len()))
        .map(|visual| {
            slice_cached_diff_styled_text(
                full_styled,
                markdown_preview_expanded_slice_range(
                    row.text.as_ref(),
                    full_styled.text.len(),
                    &visual.byte_range,
                ),
            )
        });
    let styled = sliced_styled.as_ref().unwrap_or(full_styled);
    let horizontal_padding = markdown_preview_row_horizontal_padding(row, ui_scale_percent);
    // Continuations keep the marker slot but leave it blank, so wrapped list
    // and footnote text stays indented under the first line instead of
    // sliding back under the bullet.
    let marker = markdown_preview_row_marker(row).map(|marker| {
        if is_continuation {
            SharedString::default()
        } else {
            marker
        }
    });
    let alert_title = markdown_preview_alert_title_label(row).filter(|_| !is_continuation);
    // Pictures written on this line. A wrapped continuation already showed
    // them on its first visual row.
    let inline_images: &[MarkdownInlineImage] = if is_continuation {
        &[]
    } else {
        row.inline_images.as_ref()
    };

    // Rows that need a content_shell wrapper for border/background styling.
    let needs_content_shell = matches!(
        row.kind,
        MarkdownPreviewRowKind::Heading { level: 1 | 2 }
            | MarkdownPreviewRowKind::CodeLine { .. }
            | MarkdownPreviewRowKind::TableRow { .. }
            | MarkdownPreviewRowKind::PlainFallback
    );
    let flatten_shell_text_directly = !is_interactive
        && needs_content_shell
        && marker.is_none()
        && alert_title.is_none()
        && inline_images.is_empty();

    let build_content_shell = || {
        let mut content_shell = div()
            .flex_grow(1.)
            .min_w(px(0.0))
            .w_full()
            .h_full()
            .relative()
            .flex()
            .items_center();
        content_shell = match row.kind {
            MarkdownPreviewRowKind::Heading { level: 1 | 2 } => {
                content_shell.border_b_1().border_color(with_alpha(
                    theme.colors.stroke.default,
                    if theme.is_dark { 0.85 } else { 0.92 },
                ))
            }
            MarkdownPreviewRowKind::CodeLine { is_first, is_last } => {
                let code_border = with_alpha(
                    theme.colors.stroke.default,
                    if theme.is_dark { 0.90 } else { 0.80 },
                );
                let mut shell = content_shell
                    .px(markdown_preview_scaled_px(
                        MARKDOWN_PREVIEW_SHELL_PAD_X_PX,
                        ui_scale_percent,
                    ))
                    .bg(markdown_preview_code_background(theme))
                    .border_l_1()
                    .border_r_1()
                    .border_color(code_border);
                if is_first {
                    shell = shell.border_t_1();
                }
                if is_last {
                    shell = shell.border_b_1();
                }
                shell
            }
            MarkdownPreviewRowKind::TableRow { is_header } => {
                let bg = if is_header {
                    with_alpha(
                        theme.colors.surface.raised,
                        if theme.is_dark { 0.64 } else { 0.86 },
                    )
                } else {
                    with_alpha(
                        theme.colors.surface.raised,
                        if theme.is_dark { 0.42 } else { 0.72 },
                    )
                };
                content_shell
                    .px(markdown_preview_scaled_px(
                        MARKDOWN_PREVIEW_SHELL_PAD_X_PX,
                        ui_scale_percent,
                    ))
                    .bg(bg)
                    .border_b_1()
                    .border_color(with_alpha(
                        theme.colors.stroke.default,
                        if theme.is_dark { 0.88 } else { 0.86 },
                    ))
            }
            MarkdownPreviewRowKind::PlainFallback => content_shell
                .px(markdown_preview_scaled_px(
                    MARKDOWN_PREVIEW_SHELL_PAD_X_PX,
                    ui_scale_percent,
                ))
                .bg(with_alpha(
                    theme.colors.status.warning.foreground,
                    if theme.is_dark { 0.12 } else { 0.08 },
                )),
            _ => unreachable!(),
        };
        if matches!(row.kind, MarkdownPreviewRowKind::CodeLine { .. }) && is_interactive {
            content_shell =
                content_shell.debug_selector(|| format!("markdown_preview_code_shell_{row_ix}"));
        }
        content_shell
    };

    let row_body = if flatten_shell_text_directly {
        // Benchmarked non-interactive rows do not need the extra inner content
        // wrapper when a shell already provides sizing/background/border styles.
        let mut content_shell = build_content_shell()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(typography.font_size))
            .line_height(px(typography.line_height))
            .text_color(typography.text_color);
        if let Some(font_weight) = typography.font_weight {
            content_shell = content_shell.font_weight(font_weight);
        }
        if let Some(font_family) = typography.font_family.clone() {
            content_shell = content_shell.font_family(font_family);
        }
        if styled.highlights.is_empty() {
            content_shell.child(styled.text.clone())
        } else {
            content_shell.child(MarkdownPreviewSharedHighlightsText::new(
                styled.text.clone(),
                Arc::clone(&styled.highlights),
            ))
        }
    } else {
        let mut content = div()
            .relative()
            .flex_grow(1.)
            .min_w(px(0.0))
            .w_full()
            .h(px(typography.line_height))
            .min_h(px(typography.line_height))
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_size(px(typography.font_size))
            .line_height(px(typography.line_height))
            .text_color(typography.text_color);
        if is_interactive {
            // Preview text is selectable, so the pointer should say so.
            content = content
                .cursor(gpui::CursorStyle::IBeam)
                .debug_selector(|| format!("markdown_preview_text_box_{row_ix}"));
        }

        if let Some(font_weight) = typography.font_weight {
            content = content.font_weight(font_weight);
        }
        if let Some(font_family) = typography.font_family.clone() {
            content = content.font_family(font_family);
        }
        if let Some(view) = context.view.clone() {
            // Hit testing and copy resolve rows through
            // `markdown_preview_row_text`, which works in `row.text`
            // coordinates, so the overlay shapes the raw slice rather than the
            // tab-expanded one this row paints.
            let selection_text = match visual_row {
                Some(visual) if sliced_styled.is_some() => visual.text_slice(row),
                _ => row.text.clone(),
            };
            content = content.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(DiffTextSelectionOverlay {
                        view,
                        visible_ix: row_ix,
                        region: text_region,
                        text: selection_text,
                    }),
            );
        }

        let body = match row.kind {
            MarkdownPreviewRowKind::ThematicBreak => div()
                .flex_grow(1.)
                .min_w(px(0.0))
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .child(div().w_full().h(px(1.0)).bg(with_alpha(
                    theme.colors.stroke.default,
                    if theme.is_dark { 0.92 } else { 0.88 },
                ))),
            _ if marker.is_none() && alert_title.is_none() && inline_images.is_empty() => {
                // Fast path: no marker or alert badge — use content div directly
                // as body, skipping the intermediate line wrapper div.
                if styled.highlights.is_empty() {
                    content.child(styled.text.clone())
                } else {
                    content.child(MarkdownPreviewSharedHighlightsText::new(
                        styled.text.clone(),
                        Arc::clone(&styled.highlights),
                    ))
                }
            }
            _ => {
                let text = if styled.highlights.is_empty() {
                    content.child(styled.text.clone()).into_any_element()
                } else {
                    content
                        .child(MarkdownPreviewSharedHighlightsText::new(
                            styled.text.clone(),
                            Arc::clone(&styled.highlights),
                        ))
                        .into_any_element()
                };

                let mut line = div()
                    .flex_grow(1.)
                    .min_w(px(0.0))
                    .w_full()
                    .h_full()
                    .flex()
                    .items_center();
                if let Some(marker) = marker {
                    line = line.child(
                        div()
                            .flex_none()
                            .h_full()
                            .min_w(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX,
                                ui_scale_percent,
                            ))
                            .mr(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX,
                                ui_scale_percent,
                            ))
                            .flex()
                            .items_center()
                            .justify_end()
                            .text_size(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_BASE_FONT_PX,
                                ui_scale_percent,
                            ))
                            .line_height(px(typography.line_height))
                            .text_color(theme.colors.foreground.secondary)
                            .child(marker),
                    );
                }
                if let Some(alert_title) = alert_title {
                    let alert_color = markdown_preview_alert_color(theme, row.alert_kind.unwrap());
                    line = line.child(
                        div()
                            .flex_none()
                            .mr(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_ALERT_BADGE_GAP_PX,
                                ui_scale_percent,
                            ))
                            .px(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_ALERT_BADGE_PAD_X_PX,
                                ui_scale_percent,
                            ))
                            .py(markdown_preview_scaled_px(2.0, ui_scale_percent))
                            .rounded(markdown_preview_scaled_px(2.0, ui_scale_percent))
                            .bg(with_alpha(
                                alert_color,
                                if theme.is_dark { 0.18 } else { 0.12 },
                            ))
                            .text_size(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_ALERT_BADGE_FONT_PX,
                                ui_scale_percent,
                            ))
                            .font_weight(FontWeight::BOLD)
                            .text_color(alert_color)
                            .child(alert_title),
                    );
                }
                // The diff preview's rows are a fixed height, so an inline
                // picture is capped to the line and sits ahead of the text
                // rather than flowing at the offset it was written at.
                for inline in inline_images.iter() {
                    line = line.child(
                        div()
                            .flex_none()
                            .h_full()
                            .mr(markdown_preview_scaled_px(
                                MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX,
                                ui_scale_percent,
                            ))
                            .overflow_hidden()
                            .child(markdown_preview_inline_image(
                                inline,
                                theme,
                                ui_scale_percent,
                                context.image_base_dir.as_deref(),
                                markdown_preview_no_picture_sizes(),
                            )),
                    );
                }
                line.child(text)
            }
        };

        if needs_content_shell {
            build_content_shell().child(body)
        } else {
            body
        }
    };
    // The row's horizontal padding always lives on a wrapper, never on the
    // text box itself: the selection overlay is absolutely positioned inside
    // that box, so padding applied there would shift the highlight left of the
    // glyphs it is meant to cover and cut it short at the end of the line.
    let build_row_content = move || {
        let mut row_content = div()
            .flex_grow(1.)
            .min_w(px(0.0))
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .pl(px(horizontal_padding.left_px))
            .pr(px(horizontal_padding.right_px));
        if let Some(blockquote_gutter) = markdown_preview_blockquote_gutter(
            theme,
            row.blockquote_level,
            row.alert_kind,
            ui_scale_percent,
        ) {
            row_content = row_content.child(blockquote_gutter);
        }
        row_content
    };

    if let Some(view) = context.view.clone() {
        // Interactive markdown preview row with text selection + context menu.
        let row_container = div()
            .id(("md_preview_row", row_ix))
            .debug_selector(|| format!("markdown_preview_row_box_{row_ix}"))
            .relative()
            .h(markdown_preview_row_height(ui_scale_percent))
            .min_h(markdown_preview_row_height(ui_scale_percent))
            .w(min_width)
            .flex()
            .items_center()
            .pt(px(row_layout.top_inset_px))
            .pb(px(row_layout.bottom_inset_px))
            .when_some(markdown_preview_row_background(theme, row), |div, bg| {
                div.bg(bg)
            })
            .min_w(min_width)
            .on_mouse_down(gpui::MouseButton::Left, {
                let view = view.clone();
                move |event, window, cx| {
                    let focus = view.read(cx).diff_panel_focus_handle.clone();
                    window.focus(&focus, cx);
                    let click_count = event.click_count;
                    let position = event.position;
                    view.update(cx, |this, cx| {
                        if !this.handle_markdown_preview_link_click(
                            row_ix,
                            text_region,
                            position,
                            click_count,
                            window,
                            cx,
                        ) {
                            this.handle_diff_text_mouse_down(
                                row_ix,
                                text_region,
                                position,
                                click_count,
                                cx,
                            );
                        }
                        cx.notify();
                    });
                }
            })
            .on_mouse_down(gpui::MouseButton::Right, {
                let view = view.clone();
                move |event, window, cx| {
                    view.update(cx, |this, cx| {
                        this.open_diff_editor_context_menu(
                            row_ix,
                            text_region,
                            event.position,
                            window,
                            cx,
                        );
                        cx.notify();
                    });
                }
            });
        row_container
            .child(build_row_content().child(row_body))
            .into_any_element()
    } else {
        // Non-interactive markdown preview row (benchmarks, conflict resolver).
        let row_container = div()
            .relative()
            .h(markdown_preview_row_height(ui_scale_percent))
            .min_h(markdown_preview_row_height(ui_scale_percent))
            .w(min_width)
            .flex()
            .items_center()
            .pt(px(row_layout.top_inset_px))
            .pb(px(row_layout.bottom_inset_px))
            .when_some(markdown_preview_row_background(theme, row), |div, bg| {
                div.bg(bg)
            })
            .min_w(min_width);
        row_container
            .child(build_row_content().child(row_body))
            .into_any_element()
    }
}

/// The quick-search state a markdown preview renders under.
///
/// Carried by both preview renderers — the virtualized lists and the flowing
/// single document — so a Ctrl+F match is washed in place instead of the view
/// having to fall back to the markdown source.
#[derive(Clone)]
pub(in crate::view) struct MarkdownPreviewQuery {
    pub(in crate::view) matcher: Arc<DiffSearchMatcher>,
    /// Visible index of the row the search cursor is on, if it is in this list.
    pub(in crate::view) current_row: Option<usize>,
}

impl MarkdownPreviewQuery {
    fn emphasis(&self, visible_ix: usize) -> DiffSearchMatchEmphasis {
        if self.current_row == Some(visible_ix) {
            DiffSearchMatchEmphasis::Current
        } else {
            DiffSearchMatchEmphasis::Other
        }
    }
}

/// A pending "bring this row into view" request for the flowing markdown
/// preview.
///
/// The flowing document has no fixed row height and is not a `uniform_list`, so
/// there is no `scroll_to_item` to hand the work to: the offset can only be
/// computed once the target row has been laid out. The request is therefore
/// shared into the renderer, which reports the row's bounds back through
/// [`Self::take`] during prepaint and applies the scroll then.
#[derive(Clone, Default)]
pub(in crate::view) struct MarkdownPreviewRevealRequest(
    std::rc::Rc<std::cell::Cell<Option<usize>>>,
);

impl MarkdownPreviewRevealRequest {
    pub(in crate::view) fn request(&self, row_ix: usize) {
        self.0.set(Some(row_ix));
    }

    pub(in crate::view) fn clear(&self) {
        self.0.set(None);
    }

    pub(in crate::view) fn pending(&self) -> Option<usize> {
        self.0.get()
    }

    /// Claim the request, so the reveal runs once instead of fighting the user
    /// on every later frame.
    pub(in crate::view) fn take(&self) -> Option<usize> {
        self.0.take()
    }
}

/// The vertical extent of a laid-out row, from the bounds of its parts.
///
/// A row shell holds a marker, an alert badge and the text line; the row is the
/// band they span together.
pub(in crate::view) fn markdown_preview_row_extent(
    children: &[gpui::Bounds<Pixels>],
) -> Option<(Pixels, Pixels)> {
    let top = children
        .iter()
        .map(|bounds| bounds.origin.y)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
    let bottom = children
        .iter()
        .map(|bounds| bounds.bottom())
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
    Some((top, (bottom - top).max(px(0.0))))
}

/// Where a row sits inside a scroll container, and how tall it is.
///
/// Split out from the prepaint listener so the arithmetic that decides the new
/// offset is testable without a window.
pub(in crate::view) fn markdown_preview_reveal_offset_y(
    row_top_in_content: Pixels,
    row_height: Pixels,
    viewport_height: Pixels,
    max_offset_y: Pixels,
    current_y: Pixels,
) -> Option<Pixels> {
    if viewport_height <= px(0.0) {
        return None;
    }
    // Centre the row the way a uniform list would, then clamp into the
    // scrollable range. Offsets are negative as you scroll down.
    let centered = row_top_in_content + row_height / 2.0 - viewport_height / 2.0;
    let target = (-centered).clamp(-max_offset_y.max(px(0.0)), px(0.0));
    (target != current_y).then_some(target)
}

/// Styled text for one row with the search wash layered on, shared with the
/// flowing renderer.
///
/// The base styling lives in a `OnceLock` on the row itself — it belongs to the
/// document, which outlives any one query — so the wash is merged on top per
/// frame rather than stored. Rows with no match return the base untouched, so
/// the extra work is a substring scan per visible row.
pub(in crate::view) fn markdown_preview_styled_row_with_query<'a>(
    theme: AppTheme,
    row: &'a MarkdownPreviewRow,
    visible_ix: usize,
    query: Option<&MarkdownPreviewQuery>,
) -> std::borrow::Cow<'a, CachedDiffStyledText> {
    let base = markdown_preview_row_styled_text(theme, row);
    let Some(query) = query else {
        return std::borrow::Cow::Borrowed(base);
    };
    if !query.matcher.is_match(base.text.as_ref()) {
        return std::borrow::Cow::Borrowed(base);
    }
    std::borrow::Cow::Owned(build_cached_diff_query_overlay_styled_text(
        theme,
        base,
        &query.matcher,
        query.emphasis(visible_ix),
    ))
}

/// Text element carrying inline highlights, shared with the flowing renderer.
pub(in crate::view) fn markdown_preview_highlighted_text(
    text: SharedString,
    highlights: Arc<[(Range<usize>, gpui::HighlightStyle)]>,
) -> impl IntoElement {
    MarkdownPreviewSharedHighlightsText::new(text, highlights)
}

/// List bullet or number for a row, shared with the flowing renderer.
pub(in crate::view) fn markdown_preview_marker_label(
    row: &MarkdownPreviewRow,
) -> Option<SharedString> {
    markdown_preview_row_marker(row)
}
