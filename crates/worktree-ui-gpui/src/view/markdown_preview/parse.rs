//! Markdown source to preview document, plus source byte-offset mapping.

use super::flatten::flatten_to_rows;
use super::model::{MAX_PREVIEW_SOURCE_BYTES, MarkdownPreviewDocument};
use std::ops::Range;

/// Build a `MarkdownPreviewDocument` from raw markdown source text.
///
/// Returns `None` if the source exceeds `MAX_PREVIEW_SOURCE_BYTES`
/// or the parsed document exceeds `MAX_PREVIEW_ROWS`.
pub(in crate::view) fn parse_markdown(source: &str) -> Option<MarkdownPreviewDocument> {
    if source.len() > MAX_PREVIEW_SOURCE_BYTES {
        return None;
    }
    build_markdown_document(source)
}

pub(super) fn build_markdown_document(source: &str) -> Option<MarkdownPreviewDocument> {
    let line_starts = build_line_starts(source);
    let rows = flatten_to_rows(source, &line_starts)?;
    Some(MarkdownPreviewDocument { rows })
}

/// Build a vec of byte offsets for the start of each line.
pub(super) fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Convert a byte offset to a 0-based line index.
pub(super) fn byte_offset_to_line(offset: usize, line_starts: &[usize]) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(ix) => ix,
        Err(ix) => ix.saturating_sub(1),
    }
}

/// Compute a source line range from byte offsets.
///
/// `start_byte` is the start of the element, `end_byte` is its exclusive end.
/// Returns a half-open `Range<usize>` of 0-based line indices.
pub(super) fn source_line_range(
    start_byte: usize,
    end_byte: usize,
    line_starts: &[usize],
) -> Range<usize> {
    let start_line = byte_offset_to_line(start_byte, line_starts);
    let end_line = byte_offset_to_line(end_byte.saturating_sub(1).max(start_byte), line_starts);
    start_line..end_line + 1
}

pub(super) fn markdown_parser_options() -> pulldown_cmark::Options {
    pulldown_cmark::Options::ENABLE_TABLES
        | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
        | pulldown_cmark::Options::ENABLE_TASKLISTS
        | pulldown_cmark::Options::ENABLE_FOOTNOTES
        | pulldown_cmark::Options::ENABLE_GFM
}
