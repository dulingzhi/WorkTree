//! Tests for the markdown preview engine, split by domain.
//!
//! Shared helpers live here; each domain's tests are a child module that pulls
//! this module's namespace — and through it the facade's — with
//! `use super::*`.

pub(super) use super::*;

// Domain internals these tests exercise directly, re-privated here because
// they are not part of the surface the renderers consume.
pub(super) use super::diff::{annotate_change_hints, line_range_change_hint, parse_markdown_diff};
pub(super) use super::flatten::source_line_for_byte;
pub(super) use super::inline::normalize_whitespace_with_spans;
pub(super) use super::model::{MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS, MAX_INLINE_SPANS_PER_ROW};
pub(super) use super::parse::{build_line_starts, byte_offset_to_line, source_line_range};

mod blocks;
mod diff;
mod html;
mod images;
mod inline;
mod parse;
mod tables;
mod wrap;

use gpui::SharedString;
use std::ops::Range;

fn parse(src: &str) -> MarkdownPreviewDocument {
    parse_markdown(src).expect("parse should succeed")
}

fn thematic_break_rows(count: usize) -> String {
    "---\n".repeat(count)
}

fn row_kinds(doc: &MarkdownPreviewDocument) -> Vec<&MarkdownPreviewRowKind> {
    doc.rows.iter().map(|r| &r.kind).collect()
}

fn row_texts(doc: &MarkdownPreviewDocument) -> Vec<&str> {
    doc.rows.iter().map(|r| r.text.as_ref()).collect()
}

fn code_rows(doc: &MarkdownPreviewDocument) -> Vec<&MarkdownPreviewRow> {
    doc.rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::CodeLine { .. }))
        .collect()
}

fn spans_with_style(
    row: &MarkdownPreviewRow,
    style: MarkdownInlineStyle,
) -> Vec<&MarkdownInlineSpan> {
    row.inline_spans
        .iter()
        .filter(|s| s.style == style)
        .collect()
}

fn image_rows(doc: &MarkdownPreviewDocument) -> Vec<&MarkdownPreviewRow> {
    doc.rows.iter().filter(|row| row.kind.is_image()).collect()
}

fn link_spans(row: &MarkdownPreviewRow) -> Vec<(&str, &str)> {
    row.inline_spans
        .iter()
        .filter_map(|span| {
            let url = span.link_url.as_ref()?;
            let text = row.text.get(span.byte_range.clone())?;
            Some((text, url.as_ref()))
        })
        .collect()
}

/// Inline spans become `gpui` text runs, and `gpui` shapes a line by
/// splitting the text at each run boundary. A span that lands inside a
/// multi-byte character aborts the process in `str::split_at`, so the
/// parser must never emit one — see [`crate::text_runs`] for the guard on
/// the render side.
fn assert_rows_span_aligned(source: &str, doc: &MarkdownPreviewDocument) {
    for (row_ix, row) in doc.rows.iter().enumerate() {
        let text = row.text.as_ref();
        let mut prev_end = 0usize;
        for span in row.inline_spans.iter() {
            assert!(
                span.byte_range.start <= span.byte_range.end,
                "src {source:?} row {row_ix} text {text:?} span {span:?} inverted"
            );
            assert!(
                span.byte_range.end <= text.len(),
                "src {source:?} row {row_ix} text {text:?} span {span:?} out of bounds"
            );
            assert!(
                text.is_char_boundary(span.byte_range.start),
                "src {source:?} row {row_ix} text {text:?} span {span:?} start not boundary"
            );
            assert!(
                text.is_char_boundary(span.byte_range.end),
                "src {source:?} row {row_ix} text {text:?} span {span:?} end not boundary"
            );
            assert!(
                span.byte_range.start >= prev_end,
                "src {source:?} row {row_ix} text {text:?} span {span:?} overlaps previous"
            );
            prev_end = span.byte_range.end;
        }
    }
}
