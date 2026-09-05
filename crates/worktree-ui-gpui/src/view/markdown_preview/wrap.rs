//! Word wrap: source rows to visual rows for the uniform-row surfaces.

use super::model::{MAX_PREVIEW_ROWS, MarkdownPreviewDocument, MarkdownPreviewRow};
use gpui::SharedString;
use std::ops::Range;

/// One rendered row of a wrapped preview document.
///
/// Preview rows are painted into a uniform (fixed row height) list, so word
/// wrap works the same way it does in the text diff: a source row that does
/// not fit is split into several visual rows, each carrying the byte range of
/// `MarkdownPreviewRow::text` it paints. `wrap_ix > 0` marks a continuation,
/// which drops the list marker and alert badge so the text keeps its indent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownPreviewVisualRow {
    pub(in crate::view) row_ix: usize,
    pub(in crate::view) wrap_ix: u32,
    pub(in crate::view) byte_range: Range<usize>,
}

impl MarkdownPreviewVisualRow {
    pub(in crate::view) fn is_continuation(&self) -> bool {
        self.wrap_ix > 0
    }

    /// The portion of `row.text` this visual row paints.
    ///
    /// Hit testing, selection, and copy index rows by visual position, so they
    /// need the slice the row painted rather than the whole source row.
    pub(in crate::view) fn text_slice(&self, row: &MarkdownPreviewRow) -> SharedString {
        if self.byte_range == (0..row.text.len()) {
            return row.text.clone();
        }
        row.text
            .get(self.byte_range.clone())
            .map(SharedString::new)
            .unwrap_or_default()
    }
}

/// Source-row to visual-row mapping for one wrapped preview document.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct MarkdownPreviewWrapPlan {
    rows: Vec<MarkdownPreviewVisualRow>,
}

impl MarkdownPreviewWrapPlan {
    pub(in crate::view) fn len(&self) -> usize {
        self.rows.len()
    }

    pub(in crate::view) fn get(&self, visual_ix: usize) -> Option<&MarkdownPreviewVisualRow> {
        self.rows.get(visual_ix)
    }

    /// First visual row painted for `row_ix`, for scroll and autoscroll targets.
    pub(in crate::view) fn visual_ix_for_row(&self, row_ix: usize) -> usize {
        self.rows.partition_point(|row| row.row_ix < row_ix)
    }
}

/// Build the visual-row mapping for `document`.
///
/// `wrap_row` returns the byte ranges a row's painted text splits into at the
/// current width; an empty result (or a row that fits) yields a single visual
/// row covering the whole text, so every source row keeps at least one row.
///
/// Returns `None` when the wrapped document would exceed
/// `MAX_PREVIEW_WRAPPED_ROWS`, which the caller must treat as "do not wrap".
/// Truncating the plan instead would drop the tail of the document out of the
/// list with no way to scroll to it.
pub(in crate::view) fn build_markdown_preview_wrap_plan(
    document: &MarkdownPreviewDocument,
    mut wrap_row: impl FnMut(&MarkdownPreviewRow) -> Vec<Range<usize>>,
) -> Option<MarkdownPreviewWrapPlan> {
    let mut rows = Vec::with_capacity(document.rows.len());
    for (row_ix, row) in document.rows.iter().enumerate() {
        push_wrapped_visual_rows(&mut rows, row_ix, wrap_row(row), row, 0)?;
    }
    rows.shrink_to_fit();
    Some(MarkdownPreviewWrapPlan { rows })
}

/// Build the visual-row mappings for the two sides of a split diff preview.
///
/// `align_markdown_diff_rows` pads the two documents so source row `ix` is the
/// same diff row on both sides; wrapping each side independently would break
/// that, because a long paragraph on the left would push every later left row
/// down relative to its right-hand counterpart while the synced scroll keeps
/// the two lists at the same offset. Both sides therefore get the same number
/// of visual rows per source row, the shorter side padded with empty
/// continuations.
pub(in crate::view) fn build_markdown_preview_split_wrap_plans(
    old_doc: &MarkdownPreviewDocument,
    new_doc: &MarkdownPreviewDocument,
    mut wrap_row: impl FnMut(&MarkdownPreviewRow) -> Vec<Range<usize>>,
) -> Option<(MarkdownPreviewWrapPlan, MarkdownPreviewWrapPlan)> {
    // `align_markdown_diff_rows` pushes to both sides in lockstep, so the two
    // documents are the same length by the time they reach a split preview.
    debug_assert_eq!(old_doc.rows.len(), new_doc.rows.len());

    let row_count = old_doc.rows.len().min(new_doc.rows.len());
    let mut old_rows = Vec::with_capacity(row_count);
    let mut new_rows = Vec::with_capacity(row_count);

    for (row_ix, (old_row, new_row)) in old_doc.rows.iter().zip(new_doc.rows.iter()).enumerate() {
        let old_ranges = wrap_row(old_row);
        let new_ranges = wrap_row(new_row);
        let visual_count = old_ranges.len().max(new_ranges.len()).max(1);

        push_wrapped_visual_rows(&mut old_rows, row_ix, old_ranges, old_row, visual_count)?;
        push_wrapped_visual_rows(&mut new_rows, row_ix, new_ranges, new_row, visual_count)?;
    }

    old_rows.shrink_to_fit();
    new_rows.shrink_to_fit();
    Some((
        MarkdownPreviewWrapPlan { rows: old_rows },
        MarkdownPreviewWrapPlan { rows: new_rows },
    ))
}

/// Append the visual rows for one source row, padding up to `min_visual_rows`
/// with empty continuations so a split counterpart stays row-aligned.
fn push_wrapped_visual_rows(
    out: &mut Vec<MarkdownPreviewVisualRow>,
    row_ix: usize,
    ranges: Vec<Range<usize>>,
    row: &MarkdownPreviewRow,
    min_visual_rows: usize,
) -> Option<()> {
    let text_len = row.text.len();
    let push =
        |out: &mut Vec<MarkdownPreviewVisualRow>, wrap_ix: usize, byte_range: Range<usize>| {
            out.push(MarkdownPreviewVisualRow {
                row_ix,
                wrap_ix: u32::try_from(wrap_ix).unwrap_or(u32::MAX),
                byte_range,
            });
            (out.len() <= MAX_PREVIEW_WRAPPED_ROWS).then_some(())
        };

    // A row that fits keeps one visual row covering all of its text; building
    // a one-element Vec for that common case would allocate per source row.
    let mut wrap_ix = 0usize;
    if ranges.len() < 2 {
        push(out, wrap_ix, 0..text_len)?;
        wrap_ix += 1;
    } else {
        for byte_range in ranges {
            push(out, wrap_ix, byte_range)?;
            wrap_ix += 1;
        }
    }
    while wrap_ix < min_visual_rows {
        push(out, wrap_ix, text_len..text_len)?;
        wrap_ix += 1;
    }
    Some(())
}

/// Upper bound on visual rows in a wrapped document. A pathological window
/// width (a few pixels wide) would otherwise wrap every character onto its own
/// row and blow up the uniform list.
const MAX_PREVIEW_WRAPPED_ROWS: usize = MAX_PREVIEW_ROWS * 8;
