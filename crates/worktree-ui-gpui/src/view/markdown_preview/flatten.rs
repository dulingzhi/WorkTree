//! Flatten markdown events into preview rows.
//!
//! [`flatten_to_rows`] is the orchestration shell: it walks the pulldown-cmark
//! event stream and hands each event to [`FlattenState::handle_event`], which
//! dispatches to a handler per event family. The row producer family
//! (`push_row*`) applies the finishing passes each row needs.

use super::html::{HtmlHandling, classify_supported_html};
use super::inline::{
    current_link_url, normalize_whitespace_with_spans, parse_inline_markdown_fragment,
    pop_matching_inline_style, resolve_style_stack, web_link_url,
};
use super::model::{
    MAX_INLINE_SPANS_PER_ROW, MAX_PREVIEW_ROWS, MarkdownAlertKind, MarkdownBlockQuoteContext,
    MarkdownChangeHint, MarkdownFootnoteContext, MarkdownImage, MarkdownInlineImage,
    MarkdownInlineSpan, MarkdownInlineStyle, MarkdownPreviewRow, MarkdownPreviewRowDecoration,
    MarkdownPreviewRowInput, MarkdownPreviewRowKind, MarkdownPreviewRowStyledTextCache,
    MarkdownPreviewRowWidthCache, MarkdownRowContext, markdown_preview_spacer_row_with_range,
};
use super::parse::{markdown_parser_options, source_line_range};
use super::tables::align_table_columns;
use gpui::SharedString;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ListContext {
    Unordered,
    Ordered { next_number: u64 },
}

impl ListContext {
    fn next_item_kind(&mut self) -> MarkdownPreviewRowKind {
        match self {
            Self::Unordered => MarkdownPreviewRowKind::ListItem { number: None },
            Self::Ordered { next_number } => {
                let number = *next_number;
                *next_number = next_number.saturating_add(1);
                MarkdownPreviewRowKind::ListItem {
                    number: Some(number),
                }
            }
        }
    }
}

/// A picture whose markdown tag is still open: its source, and where its alt
/// text begins in the buffer so the alt can be split back off at the closing
/// tag.
struct PendingImage {
    source: SharedString,
    alt_start: usize,
}

/// The parse state the event walk mutates: everything the flatten pass kept as
/// locals, gathered so each event-family handler can act on it.
struct FlattenState<'a> {
    line_starts: &'a [usize],
    rows: Vec<MarkdownPreviewRow>,
    text_buf: String,
    span_stack: Vec<MarkdownInlineStyle>,
    link_stack: Vec<Option<SharedString>>,
    pending_image: Option<PendingImage>,
    inline_spans: Vec<MarkdownInlineSpan>,
    source_start_byte: usize,
    indent_level: u8,
    list_stack: Vec<ListContext>,
    list_item_stack: Vec<MarkdownPreviewRowKind>,
    in_heading: bool,
    in_paragraph: bool,
    in_blockquote: u8,
    row_ctx: MarkdownRowContext,
    in_code_block: bool,
    in_table_row: bool,
    table_row_is_header: bool,
    code_block_start_byte: usize,
    code_block_starts_after_fence: bool,
    code_block_language: Option<crate::view::rows::DiffSyntaxLanguage>,
    footnote_context: Option<MarkdownFootnoteContext>,
}

impl<'a> FlattenState<'a> {
    fn new(line_starts: &'a [usize]) -> Self {
        // Rows are per markdown *block*, not per line, and `push_row_with_context`
        // bails at MAX_PREVIEW_ROWS regardless, so the line count is only an upper
        // bound worth honouring up to that cap.
        Self {
            line_starts,
            rows: Vec::with_capacity(line_starts.len().min(MAX_PREVIEW_ROWS)),
            text_buf: String::new(),
            span_stack: Vec::new(),
            link_stack: Vec::new(),
            pending_image: None,
            inline_spans: Vec::new(),
            source_start_byte: 0,
            indent_level: 0,
            list_stack: Vec::new(),
            list_item_stack: Vec::new(),
            in_heading: false,
            in_paragraph: false,
            in_blockquote: 0,
            row_ctx: MarkdownRowContext::default(),
            in_code_block: false,
            in_table_row: false,
            table_row_is_header: false,
            code_block_start_byte: 0,
            code_block_starts_after_fence: false,
            code_block_language: None,
            footnote_context: None,
        }
    }

    /// Apply the finishing passes and hand back the flattened rows.
    fn finish(mut self) -> Vec<MarkdownPreviewRow> {
        align_table_columns(&mut self.rows);
        insert_top_level_heading_spacer_rows(&mut self.rows);
        self.rows
    }

    fn handle_event(&mut self, event: Event<'_>, event_range: Range<usize>) -> Option<()> {
        // Block-level HTML stands on its own line with no paragraph around it,
        // so anything it produces has to close its own row.
        let is_block_html = matches!(event, Event::Html(_));
        match event {
            Event::Start(Tag::Heading { .. }) => {
                self.text_buf.clear();
                self.inline_spans.clear();
                self.source_start_byte = event_range.start;
                self.in_heading = true;
            }
            Event::End(TagEnd::Heading(level)) => self.end_heading(level as u8, &event_range)?,

            Event::Start(Tag::Paragraph) => {
                self.text_buf.clear();
                self.inline_spans.clear();
                self.source_start_byte = event_range.start;
                self.in_paragraph = true;
            }
            Event::End(TagEnd::Paragraph) => self.end_paragraph(&event_range)?,

            Event::Start(Tag::List(first_number)) => self.start_list(first_number, &event_range)?,
            Event::End(TagEnd::List(_)) => {
                self.list_stack.pop();
                self.indent_level = self.indent_level.saturating_sub(1);
            }

            Event::Start(Tag::Item) => {
                self.text_buf.clear();
                self.inline_spans.clear();
                self.source_start_byte = event_range.start;
                if let Some(context) = self.list_stack.last_mut() {
                    self.list_item_stack.push(context.next_item_kind());
                }
            }
            Event::End(TagEnd::Item) => self.end_item(&event_range)?,

            Event::Start(Tag::BlockQuote(kind)) => {
                self.row_ctx
                    .blockquote_stack
                    .push(MarkdownBlockQuoteContext {
                        alert_kind: kind.and_then(markdown_alert_kind_from_blockquote_kind),
                        emitted_row: false,
                    });
                self.in_blockquote = self.in_blockquote.saturating_add(1);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.row_ctx.blockquote_stack.pop();
                self.in_blockquote = self.in_blockquote.saturating_sub(1);
            }

            Event::Start(Tag::FootnoteDefinition(label)) => {
                self.footnote_context = Some(MarkdownFootnoteContext {
                    label: label.to_string().into(),
                    emitted_label: false,
                });
                self.indent_level = self.indent_level.saturating_add(1);
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.footnote_context = None;
                self.indent_level = self.indent_level.saturating_sub(1);
            }

            Event::Start(Tag::CodeBlock(kind)) => self.start_code_block(&kind, &event_range),
            Event::End(TagEnd::CodeBlock) => self.end_code_block(&event_range)?,

            Event::Start(Tag::TableHead) => {
                self.text_buf.clear();
                self.inline_spans.clear();
                self.source_start_byte = event_range.start;
                self.in_table_row = true;
                self.table_row_is_header = true;
            }
            Event::Start(Tag::TableRow) => {
                self.text_buf.clear();
                self.inline_spans.clear();
                self.source_start_byte = event_range.start;
                self.in_table_row = true;
                self.table_row_is_header = false;
            }
            Event::End(TagEnd::TableRow) | Event::End(TagEnd::TableHead) => {
                self.end_table_row(&event_range)?;
            }
            Event::End(TagEnd::TableCell) => {
                // Separate cells with a tab character for display.
                self.text_buf.push('\t');
            }

            // Inline styling tags
            Event::Start(Tag::Strong) => {
                self.span_stack.push(MarkdownInlineStyle::Bold);
            }
            Event::Start(Tag::Emphasis) => {
                self.span_stack.push(MarkdownInlineStyle::Italic);
            }
            Event::Start(Tag::Strikethrough) => {
                self.span_stack.push(MarkdownInlineStyle::Strikethrough);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.span_stack.push(MarkdownInlineStyle::Link);
                self.link_stack.push(web_link_url(dest_url.as_ref()));
            }
            Event::End(TagEnd::Link) => {
                self.span_stack.pop();
                self.link_stack.pop();
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                self.span_stack.pop();
            }

            // Pulldown reports an image's alt text as ordinary text between
            // Start and End, so the alt is taken back out of the buffer and the
            // picture is recorded at the offset it occupies. Whether it ends up
            // inline or as a block of its own is decided when the row closes.
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.pending_image = Some(PendingImage {
                    source: SharedString::from(dest_url.as_ref().to_owned()),
                    alt_start: self.text_buf.len(),
                });
            }
            Event::End(TagEnd::Image) => self.end_image(&event_range)?,

            Event::Text(cow) => self.push_styled_text(&cow),
            Event::Code(cow) => self.push_inline_code(&cow),
            Event::FootnoteReference(label) => self.push_footnote_reference(&label),

            Event::SoftBreak => self.handle_soft_break(&event_range)?,
            Event::HardBreak => self.handle_hard_break(&event_range)?,

            Event::Rule => self.push_rule_row(&event_range)?,

            Event::TaskListMarker(checked) => self.push_task_list_marker(checked),

            Event::Html(cow) => self.handle_html(cow.as_ref(), &event_range, is_block_html)?,
            Event::InlineHtml(cow) => {
                self.handle_html(cow.as_ref(), &event_range, is_block_html)?
            }

            // Ignore footnotes, metadata, and math in v1.
            _ => {}
        }
        Some(())
    }

    fn end_heading(&mut self, level: u8, event_range: &Range<usize>) -> Option<()> {
        let range = source_line_range(self.source_start_byte, event_range.end, self.line_starts);
        push_row_with_context(
            &mut self.rows,
            MarkdownPreviewRowInput::plain(
                MarkdownPreviewRowKind::Heading { level },
                &self.text_buf,
                &self.inline_spans,
                range,
                self.indent_level,
                self.in_blockquote,
            ),
            self.footnote_context.as_mut(),
            &mut self.row_ctx,
        )?;
        self.in_heading = false;
        self.text_buf.clear();
        self.inline_spans.clear();
        Some(())
    }

    fn end_paragraph(&mut self, event_range: &Range<usize>) -> Option<()> {
        // A paragraph whose only content was block-level HTML has
        // already closed its own row; closing it again would add a
        // blank row under it.
        if self.text_buf.is_empty()
            && self.row_ctx.pending_images.is_empty()
            && self.rows.last().is_some_and(|row| row.kind.is_image())
        {
            self.in_paragraph = false;
            self.inline_spans.clear();
            return Some(());
        }
        let kind = current_row_kind(&self.list_item_stack, self.in_blockquote);

        let range = source_line_range(self.source_start_byte, event_range.end, self.line_starts);
        push_row_with_context(
            &mut self.rows,
            MarkdownPreviewRowInput::plain(
                kind,
                &self.text_buf,
                &self.inline_spans,
                range,
                self.indent_level,
                self.in_blockquote,
            ),
            self.footnote_context.as_mut(),
            &mut self.row_ctx,
        )?;
        self.in_paragraph = false;
        self.text_buf.clear();
        self.inline_spans.clear();
        Some(())
    }

    fn start_list(&mut self, first_number: Option<u64>, event_range: &Range<usize>) -> Option<()> {
        // Flush any accumulated item text — or picture — before entering
        // the sub-list, so the parent item gets its own row at the
        // current indent level.
        if (!self.text_buf.is_empty() || self.row_ctx.has_pending_images())
            && !self.list_item_stack.is_empty()
        {
            let kind = self
                .list_item_stack
                .last()
                .copied()
                .unwrap_or(MarkdownPreviewRowKind::ListItem { number: None });
            let range =
                source_line_range(self.source_start_byte, event_range.start, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    kind,
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
        }
        self.list_stack.push(match first_number {
            Some(next_number) => ListContext::Ordered { next_number },
            None => ListContext::Unordered,
        });
        self.indent_level = self.indent_level.saturating_add(1);
        Some(())
    }

    fn end_item(&mut self, event_range: &Range<usize>) -> Option<()> {
        // Only emit a row if there is text that hasn't already been
        // emitted by a nested paragraph or sub-list — or a picture,
        // which a tight list item like `- ![badge](b.svg)` leaves as
        // the item's only content.
        if !self.text_buf.is_empty() || self.row_ctx.has_pending_images() {
            let kind = self
                .list_item_stack
                .last()
                .copied()
                .unwrap_or(MarkdownPreviewRowKind::ListItem { number: None });
            let range =
                source_line_range(self.source_start_byte, event_range.end, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    kind,
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
        }
        self.list_item_stack.pop();
        Some(())
    }

    fn start_code_block(&mut self, kind: &CodeBlockKind, event_range: &Range<usize>) {
        self.in_code_block = true;
        self.code_block_start_byte = event_range.start;
        self.code_block_language = match kind {
            CodeBlockKind::Fenced(info) => {
                crate::view::rows::diff_syntax_language_for_code_fence_info(info.as_ref())
            }
            CodeBlockKind::Indented => None,
        };
        self.code_block_starts_after_fence = matches!(kind, CodeBlockKind::Fenced(_));
        self.text_buf.clear();
        self.inline_spans.clear();
    }

    fn end_code_block(&mut self, event_range: &Range<usize>) -> Option<()> {
        // Emit one row per code line.
        let block_range = source_line_range(
            self.code_block_start_byte,
            event_range.end,
            self.line_starts,
        );
        let block_start_line = block_range.start;
        let block_end_line = block_range.end.saturating_sub(1);
        let content_start_line = block_start_line + usize::from(self.code_block_starts_after_fence);
        let code_text = self.text_buf.strip_suffix('\n').unwrap_or(&self.text_buf);
        let code_lines: Vec<&str> = if code_text.is_empty() {
            vec![""]
        } else {
            code_text.split('\n').collect()
        };
        let code_block_horizontal_scroll_hint = code_lines
            .iter()
            .any(|line| line.contains('\t') || line.chars().count() > 80);
        let last_ix = code_lines.len().saturating_sub(1);
        for (i, line) in code_lines.iter().enumerate() {
            let line_ix = (content_start_line + i).min(block_end_line);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::code(
                    MarkdownPreviewRowKind::CodeLine {
                        is_first: i == 0,
                        is_last: i == last_ix,
                    },
                    line,
                    line_ix..line_ix + 1,
                    self.code_block_language,
                    code_block_horizontal_scroll_hint,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
        }
        self.in_code_block = false;
        self.code_block_starts_after_fence = false;
        self.code_block_language = None;
        self.text_buf.clear();
        self.inline_spans.clear();
        Some(())
    }

    fn end_table_row(&mut self, event_range: &Range<usize>) -> Option<()> {
        let range = source_line_range(self.source_start_byte, event_range.end, self.line_starts);
        push_row_with_context(
            &mut self.rows,
            MarkdownPreviewRowInput::plain(
                MarkdownPreviewRowKind::TableRow {
                    is_header: self.table_row_is_header,
                },
                &self.text_buf,
                &self.inline_spans,
                range,
                self.indent_level,
                self.in_blockquote,
            ),
            self.footnote_context.as_mut(),
            &mut self.row_ctx,
        )?;
        self.in_table_row = false;
        self.table_row_is_header = false;
        self.text_buf.clear();
        self.inline_spans.clear();
        Some(())
    }

    /// Close the open blockquote line at a soft or hard break, if one is open.
    ///
    /// Returns `true` when the break was consumed by closing a blockquote
    /// line — the one case in which the surrounding event's own handling of
    /// the break does not also run.
    fn close_blockquote_line_at_break(&mut self, break_range: &Range<usize>) -> Option<bool> {
        if self.in_blockquote == 0 || !self.list_item_stack.is_empty() || self.in_code_block {
            return Some(false);
        }
        if !self.text_buf.is_empty() {
            let range =
                source_line_range(self.source_start_byte, break_range.start, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    MarkdownPreviewRowKind::BlockquoteLine,
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
        }
        self.source_start_byte = break_range.end;
        Some(true)
    }

    fn handle_soft_break(&mut self, event_range: &Range<usize>) -> Option<()> {
        if self.close_blockquote_line_at_break(event_range)? {
            return Some(());
        }
        if !self.text_buf.is_empty() {
            self.text_buf.push(' ');
        }
        Some(())
    }

    fn handle_hard_break(&mut self, event_range: &Range<usize>) -> Option<()> {
        if self.close_blockquote_line_at_break(event_range)? {
            return Some(());
        }
        if !self.in_code_block && !self.in_heading && !self.text_buf.is_empty() {
            let range =
                source_line_range(self.source_start_byte, event_range.start, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    current_row_kind(&self.list_item_stack, self.in_blockquote),
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
            self.source_start_byte = event_range.end;
        } else if !self.text_buf.is_empty() {
            self.text_buf.push(' ');
        }
        Some(())
    }

    fn push_rule_row(&mut self, event_range: &Range<usize>) -> Option<()> {
        let range = source_line_range(event_range.start, event_range.end, self.line_starts);
        push_row_with_context(
            &mut self.rows,
            MarkdownPreviewRowInput::plain(
                MarkdownPreviewRowKind::ThematicBreak,
                "───",
                &[],
                range,
                self.indent_level,
                self.in_blockquote,
            ),
            self.footnote_context.as_mut(),
            &mut self.row_ctx,
        )
    }

    fn push_task_list_marker(&mut self, checked: bool) {
        let marker = if checked { "[x] " } else { "[ ] " };
        self.text_buf.insert_str(0, marker);
        // Shift existing span byte ranges.
        let shift = marker.len();
        for span in &mut self.inline_spans {
            span.byte_range.start += shift;
            span.byte_range.end += shift;
        }
    }

    fn end_image(&mut self, event_range: &Range<usize>) -> Option<()> {
        let Some(pending) = self.pending_image.take() else {
            return Some(());
        };
        let alt = self
            .text_buf
            .split_off(pending.alt_start.min(self.text_buf.len()));
        if self.in_table_row {
            // A table row is painted as one string whose columns are
            // aligned by padding, so a picture cannot sit in a cell
            // without breaking that alignment. Its description stays in
            // the cell instead, which keeps the column readable and in
            // the right place.
            self.text_buf.push_str(&alt);
            return Some(());
        }
        // The alt text is not painted, so anything styled inside it —
        // an image inside a link records the link on its alt — would
        // leave a span pointing past the end of the row.
        clamp_inline_spans_to_len(&mut self.inline_spans, self.text_buf.len());
        self.row_ctx.pending_images.push(MarkdownInlineImage {
            byte_offset: self.text_buf.len(),
            source_byte: event_range.start,
            // Markdown image syntax cannot declare a size.
            image: Arc::new(MarkdownImage {
                source: pending.source,
                width_px: None,
                height_px: None,
            }),
            alt: SharedString::from(alt),
            link_url: current_link_url(&self.link_stack),
        });
        Some(())
    }

    fn push_styled_text(&mut self, text: &str) {
        let style = resolve_style_stack(&self.span_stack);
        let link_url = current_link_url(&self.link_stack);
        let start = self.text_buf.len();
        self.text_buf.push_str(text);
        let end = self.text_buf.len();
        if (style != MarkdownInlineStyle::Normal || link_url.is_some()) && !self.in_code_block {
            self.inline_spans.push(MarkdownInlineSpan {
                byte_range: start..end,
                style,
                link_url,
            });
        }
    }

    fn push_inline_code(&mut self, code: &str) {
        let start = self.text_buf.len();
        self.text_buf.push_str(code);
        let end = self.text_buf.len();
        if !self.in_code_block {
            self.inline_spans.push(MarkdownInlineSpan {
                byte_range: start..end,
                style: MarkdownInlineStyle::Code,
                link_url: current_link_url(&self.link_stack),
            });
        }
    }

    fn push_footnote_reference(&mut self, label: &str) {
        let start = self.text_buf.len();
        self.text_buf.push('[');
        self.text_buf.push_str(label);
        self.text_buf.push(']');
        let end = self.text_buf.len();
        if !self.in_code_block {
            self.inline_spans.push(MarkdownInlineSpan {
                byte_range: start..end,
                style: MarkdownInlineStyle::Link,
                // A footnote reference points inside the document, not
                // at the web.
                link_url: None,
            });
        }
    }

    fn should_append_html(&self) -> bool {
        html_event_should_append(
            self.in_paragraph,
            self.in_heading,
            !self.list_stack.is_empty(),
            self.in_blockquote,
            self.in_code_block,
            self.in_table_row,
        )
    }

    fn handle_html(
        &mut self,
        html: &str,
        event_range: &Range<usize>,
        is_block_html: bool,
    ) -> Option<()> {
        match classify_supported_html(html) {
            HtmlHandling::Ignore => Some(()),
            HtmlHandling::HardBreak => self.handle_html_break(event_range),
            HtmlHandling::DetailsSummary(summary_source) => {
                self.handle_html_summary(&summary_source, event_range)
            }
            HtmlHandling::StartInlineStyle(style) => {
                self.span_stack.push(style);
                Some(())
            }
            HtmlHandling::EndInlineStyle(style) => {
                pop_matching_inline_style(&mut self.span_stack, style);
                Some(())
            }
            HtmlHandling::Images(images) => {
                self.handle_html_images(images, event_range, is_block_html)
            }
            HtmlHandling::AppendText(text) => self.handle_html_append_text(text, event_range),
            HtmlHandling::AppendLiteral => self.handle_html_append_literal(html, event_range),
        }
    }

    fn handle_html_break(&mut self, event_range: &Range<usize>) -> Option<()> {
        if self.close_blockquote_line_at_break(event_range)? {
            return Some(());
        }
        if !self.in_code_block && !self.in_heading && !self.text_buf.is_empty() {
            let range =
                source_line_range(self.source_start_byte, event_range.start, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    current_row_kind(&self.list_item_stack, self.in_blockquote),
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
            self.source_start_byte = event_range.end;
        }
        Some(())
    }

    fn handle_html_summary(
        &mut self,
        summary_source: &str,
        event_range: &Range<usize>,
    ) -> Option<()> {
        if !self.text_buf.is_empty() {
            let range =
                source_line_range(self.source_start_byte, event_range.start, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    current_row_kind(&self.list_item_stack, self.in_blockquote),
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
        }

        let (summary_text, summary_spans) = parse_inline_markdown_fragment(summary_source);
        if !summary_text.is_empty() {
            let range = source_line_range(event_range.start, event_range.end, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    MarkdownPreviewRowKind::DetailsSummary,
                    &summary_text,
                    &summary_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
        }
        self.source_start_byte = event_range.end;
        Some(())
    }

    fn handle_html_images(
        &mut self,
        images: Vec<(usize, MarkdownImage, String)>,
        event_range: &Range<usize>,
        is_block_html: bool,
    ) -> Option<()> {
        if self.in_table_row {
            // As with a markdown image: a table cell keeps the
            // description rather than a picture that cannot be
            // placed in its column.
            for (_, _, alt) in &images {
                self.text_buf.push_str(alt);
            }
            return Some(());
        }
        // An `<img>` records itself the way a markdown image
        // does; the row it closes decides whether it is inline
        // or a block.
        for (tag_offset, image, alt) in images {
            self.row_ctx.pending_images.push(MarkdownInlineImage {
                byte_offset: self.text_buf.len(),
                // Several tags can share one event, so the id
                // is the tag's own position, not the event's.
                source_byte: event_range.start.saturating_add(tag_offset),
                image: Arc::new(image),
                alt: SharedString::from(alt),
                link_url: current_link_url(&self.link_stack),
            });
        }
        // A block-level tag has no paragraph to close it, so it
        // flushes its own row.
        if is_block_html {
            let range =
                source_line_range(self.source_start_byte, event_range.end, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    current_row_kind(&self.list_item_stack, self.in_blockquote),
                    &self.text_buf,
                    &self.inline_spans,
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
            self.text_buf.clear();
            self.inline_spans.clear();
            self.source_start_byte = event_range.end;
        }
        Some(())
    }

    fn handle_html_append_text(&mut self, text: String, event_range: &Range<usize>) -> Option<()> {
        if self.should_append_html() {
            self.text_buf.push_str(&text);
        } else {
            let range = source_line_range(event_range.start, event_range.end, self.line_starts);
            push_row_with_context(
                &mut self.rows,
                MarkdownPreviewRowInput::plain(
                    current_row_kind(&self.list_item_stack, self.in_blockquote),
                    &text,
                    &[],
                    range,
                    self.indent_level,
                    self.in_blockquote,
                ),
                self.footnote_context.as_mut(),
                &mut self.row_ctx,
            )?;
        }
        Some(())
    }

    fn handle_html_append_literal(&mut self, html: &str, event_range: &Range<usize>) -> Option<()> {
        if self.should_append_html() {
            self.text_buf.push_str(html);
        } else {
            push_plain_fallback_rows(
                &mut self.rows,
                html,
                event_range.start,
                event_range.end,
                self.line_starts,
                self.indent_level,
                self.in_blockquote,
                &mut self.row_ctx,
            )?;
        }
        Some(())
    }
}

/// Flatten markdown events into preview rows.
pub(super) fn flatten_to_rows(
    source: &str,
    line_starts: &[usize],
) -> Option<Vec<MarkdownPreviewRow>> {
    let mut state = FlattenState::new(line_starts);
    let options = markdown_parser_options();
    for (event, event_range) in Parser::new_ext(source, options).into_offset_iter() {
        state.handle_event(event, event_range)?;
    }
    Some(state.finish())
}

fn insert_top_level_heading_spacer_rows(rows: &mut Vec<MarkdownPreviewRow>) {
    if rows.len() < 2 {
        return;
    }

    let mut spaced_rows = Vec::with_capacity(rows.len() + rows.len() / 4);
    let mut pending_gap_after_heading: Option<Range<usize>> = None;

    for row in rows.drain(..) {
        let is_top_level_heading = markdown_row_is_top_level_heading(&row);
        if let Some(source_line_range) = pending_gap_after_heading.take()
            && !is_top_level_heading
            && !matches!(row.kind, MarkdownPreviewRowKind::Spacer)
        {
            spaced_rows.push(markdown_preview_spacer_row_with_range(source_line_range));
        }

        if is_top_level_heading {
            let has_content_before_heading = matches!(
                spaced_rows.last(),
                Some(previous_row)
                    if !matches!(
                        previous_row.kind,
                        MarkdownPreviewRowKind::Spacer | MarkdownPreviewRowKind::Heading { .. }
                    )
            );

            // One spacer row is the section break. Adding a second one under
            // the heading doubles it to two blank rows, which reads as a hole
            // in the document; the heading's own vertical insets carry the
            // smaller gap beneath it instead.
            if has_content_before_heading {
                spaced_rows.push(markdown_preview_spacer_row_with_range(
                    row.source_line_range.clone(),
                ));
            } else {
                pending_gap_after_heading = Some(row.source_line_range.clone());
            }
        }

        spaced_rows.push(row);
    }

    *rows = spaced_rows;
}

fn markdown_row_is_top_level_heading(row: &MarkdownPreviewRow) -> bool {
    matches!(row.kind, MarkdownPreviewRowKind::Heading { .. })
        && row.indent_level == 0
        && row.blockquote_level == 0
}

fn current_row_kind(
    list_item_stack: &[MarkdownPreviewRowKind],
    blockquote_level: u8,
) -> MarkdownPreviewRowKind {
    if let Some(kind) = list_item_stack.last().copied() {
        kind
    } else if blockquote_level > 0 {
        MarkdownPreviewRowKind::BlockquoteLine
    } else {
        MarkdownPreviewRowKind::Paragraph
    }
}

fn markdown_alert_kind_from_blockquote_kind(
    kind: pulldown_cmark::BlockQuoteKind,
) -> Option<MarkdownAlertKind> {
    Some(match kind {
        pulldown_cmark::BlockQuoteKind::Note => MarkdownAlertKind::Note,
        pulldown_cmark::BlockQuoteKind::Tip => MarkdownAlertKind::Tip,
        pulldown_cmark::BlockQuoteKind::Important => MarkdownAlertKind::Important,
        pulldown_cmark::BlockQuoteKind::Warning => MarkdownAlertKind::Warning,
        pulldown_cmark::BlockQuoteKind::Caution => MarkdownAlertKind::Caution,
    })
}

fn html_event_should_append(
    in_paragraph: bool,
    in_heading: bool,
    in_list: bool,
    blockquote_level: u8,
    in_code_block: bool,
    in_table_row: bool,
) -> bool {
    in_paragraph || in_heading || in_list || blockquote_level > 0 || in_code_block || in_table_row
}

fn push_row_with_context(
    rows: &mut Vec<MarkdownPreviewRow>,
    mut row: MarkdownPreviewRowInput<'_>,
    footnote_context: Option<&mut MarkdownFootnoteContext>,
    row_ctx: &mut MarkdownRowContext,
) -> Option<()> {
    let pending_images = std::mem::take(&mut row_ctx.pending_images);
    let footnote_label = footnote_context.and_then(|ctx| {
        if ctx.emitted_label {
            None
        } else {
            ctx.emitted_label = true;
            Some(ctx.label.clone())
        }
    });

    let mut decoration = MarkdownPreviewRowDecoration {
        footnote_label,
        ..MarkdownPreviewRowDecoration::default()
    };
    if let Some(alert_ix) = row_ctx
        .blockquote_stack
        .iter()
        .rposition(|ctx| ctx.alert_kind.is_some())
    {
        let ctx = &mut row_ctx.blockquote_stack[alert_ix];
        decoration.alert_kind = ctx.alert_kind;
        if !ctx.emitted_row {
            ctx.emitted_row = true;
            decoration.starts_alert = true;
        }
    }

    // A picture alone in a plain paragraph reads as a block — it gets the width
    // of the document and a band of rows to itself. Everywhere else it stays
    // inline: sharing its line with text or other pictures keeps a row of
    // badges on one line and a logo beside its heading, and a row that carries
    // a bullet, a quote bar, or an indent has to keep drawing them, which a
    // block row does not.
    if let [only] = pending_images.as_slice()
        && row.text.trim().is_empty()
        && row.image.is_none()
        && row.kind == MarkdownPreviewRowKind::Paragraph
        && row.indent_level == 0
        && row.blockquote_level == 0
    {
        return push_image_block_rows(rows, only, &row, decoration);
    }

    row.inline_images = Arc::from(pending_images);
    push_row(rows, row, decoration)
}

/// Emit the band rows one block image occupies.
///
/// The preview paints into a uniform (fixed row height) list, so a picture that
/// stands on its own covers several rows and each one draws its own band.
fn push_image_block_rows(
    rows: &mut Vec<MarkdownPreviewRow>,
    inline: &MarkdownInlineImage,
    row: &MarkdownPreviewRowInput<'_>,
    decoration: MarkdownPreviewRowDecoration,
) -> Option<()> {
    let slice_count = inline.image.block_rows();
    // The alert badge and the footnote label belong to the first band only; the
    // rest continue the same picture, and only inherit its alert.
    let continuation = MarkdownPreviewRowDecoration {
        alert_kind: decoration.alert_kind,
        ..MarkdownPreviewRowDecoration::default()
    };
    let mut decoration = Some(decoration);
    for slice_ix in 0..slice_count {
        push_row(
            rows,
            MarkdownPreviewRowInput::image(
                slice_ix,
                slice_count,
                inline.alt.as_ref(),
                Arc::clone(&inline.image),
                row.source_line_range.clone(),
                row.indent_level,
                row.blockquote_level,
            ),
            decoration.take().unwrap_or_else(|| continuation.clone()),
        )?;
    }
    Some(())
}

fn push_row(
    rows: &mut Vec<MarkdownPreviewRow>,
    row: MarkdownPreviewRowInput<'_>,
    decoration: MarkdownPreviewRowDecoration,
) -> Option<()> {
    let (row_text, row_spans) = match row.kind {
        // Paragraph-like rows collapse whitespace, so remap inline spans to
        // the normalized text instead of leaving them pointed at stale bytes.
        MarkdownPreviewRowKind::Paragraph
        | MarkdownPreviewRowKind::DetailsSummary
        | MarkdownPreviewRowKind::BlockquoteLine => {
            normalize_whitespace_with_spans(row.text, row.inline_spans)
        }
        _ => (row.text.to_owned(), row.inline_spans.to_vec()),
    };
    let (row_text, row_spans, inline_images) = if row.inline_images.is_empty() {
        (row_text, row_spans, row.inline_images)
    } else {
        trim_around_inline_images(row_text, row_spans, &row.inline_images)
    };
    let spans = if row_spans.len() > MAX_INLINE_SPANS_PER_ROW {
        Arc::new(Vec::new())
    } else {
        Arc::new(row_spans)
    };

    rows.push(MarkdownPreviewRow {
        kind: row.kind,
        text: SharedString::from(row_text),
        inline_spans: spans,
        code_language: row.code_language,
        code_block_horizontal_scroll_hint: row.code_block_horizontal_scroll_hint,
        source_line_range: row.source_line_range,
        change_hint: MarkdownChangeHint::None,
        indent_level: row.indent_level,
        blockquote_level: row.blockquote_level,
        footnote_label: decoration.footnote_label,
        alert_kind: decoration.alert_kind,
        starts_alert: decoration.starts_alert,
        image: row.image,
        inline_images,
        styled_text_cache: MarkdownPreviewRowStyledTextCache::default(),
        measured_width_px: MarkdownPreviewRowWidthCache::default(),
    });

    (rows.len() <= MAX_PREVIEW_ROWS).then_some(())
}

/// Trim the whitespace a picture leaves behind when it is lifted out of the
/// line, keeping spans and picture offsets on the characters they described.
///
/// `## <img/> WorkTree` puts a space between the tag and the word; without this
/// the heading would start with that gap.
fn trim_around_inline_images(
    text: String,
    spans: Vec<MarkdownInlineSpan>,
    images: &[MarkdownInlineImage],
) -> (String, Vec<MarkdownInlineSpan>, Arc<[MarkdownInlineImage]>) {
    let start = text.len() - text.trim_start().len();
    let trimmed = text.trim().to_owned();
    let end = start + trimmed.len();
    let shift = |offset: usize| offset.clamp(start, end) - start;

    let spans = spans
        .into_iter()
        .filter_map(|span| {
            let range = shift(span.byte_range.start)..shift(span.byte_range.end);
            (range.start < range.end).then(|| span.restyled(range))
        })
        .collect();
    let images = images
        .iter()
        .map(|inline| MarkdownInlineImage {
            byte_offset: shift(inline.byte_offset),
            ..inline.clone()
        })
        .collect::<Vec<_>>();

    (trimmed, spans, Arc::from(images))
}

/// Drop or shorten spans that reach past `len`, and keep the rest.
fn clamp_inline_spans_to_len(spans: &mut Vec<MarkdownInlineSpan>, len: usize) {
    spans.retain_mut(|span| {
        span.byte_range.end = span.byte_range.end.min(len);
        span.byte_range.start < span.byte_range.end
    });
}

/// Emit unparseable content verbatim, one row per line.
///
/// This is the one row producer that does not go through
/// `push_row_with_context`, because a fallback row inherits no footnote label
/// and no alert. It still has to take the pending pictures, or they would be
/// carried past it and land on an unrelated row.
fn push_plain_fallback_rows(
    rows: &mut Vec<MarkdownPreviewRow>,
    text: &str,
    start_byte: usize,
    end_byte: usize,
    line_starts: &[usize],
    indent_level: u8,
    blockquote_level: u8,
    row_ctx: &mut MarkdownRowContext,
) -> Option<()> {
    let range = source_line_range(start_byte, end_byte, line_starts);
    let segments = if text.is_empty() {
        vec![""]
    } else {
        text.lines().collect::<Vec<_>>()
    };
    let end_line = range.end.saturating_sub(1);
    let mut pending_images = std::mem::take(&mut row_ctx.pending_images);
    let segment_count = segments.len();

    for (ix, segment) in segments.into_iter().enumerate() {
        let line_ix = (range.start + ix).min(end_line);
        let mut row = MarkdownPreviewRowInput::plain(
            MarkdownPreviewRowKind::PlainFallback,
            segment,
            &[],
            line_ix..line_ix.saturating_add(1),
            indent_level,
            blockquote_level,
        );
        // Each picture goes on the line it was written on, which is what its
        // source offset says. The last row sweeps up anything that did not
        // resolve, so nothing is dropped.
        let is_last = ix + 1 == segment_count;
        let (mine, rest) = pending_images.into_iter().partition(|inline| {
            is_last || source_line_for_byte(inline.source_byte, line_starts) == line_ix
        });
        pending_images = rest;
        row.inline_images = Arc::from(mine);
        push_row(rows, row, MarkdownPreviewRowDecoration::default())?;
    }

    Some(())
}

/// Zero-based source line containing `byte`.
pub(super) fn source_line_for_byte(byte: usize, line_starts: &[usize]) -> usize {
    line_starts.partition_point(|start| *start <= byte).max(1) - 1
}
