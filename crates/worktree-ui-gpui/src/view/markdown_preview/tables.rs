//! Column alignment for flattened markdown table rows.

use super::model::{MarkdownInlineSpan, MarkdownPreviewRow, MarkdownPreviewRowKind};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
struct MarkdownTableCell {
    text: String,
    spans: Vec<MarkdownInlineSpan>,
}

pub(super) fn align_table_columns(rows: &mut [MarkdownPreviewRow]) {
    let mut start = 0usize;
    while start < rows.len() {
        if !matches!(rows[start].kind, MarkdownPreviewRowKind::TableRow { .. }) {
            start += 1;
            continue;
        }

        // A header row opens a table, so it also closes the one before it —
        // two tables that touch must not have their columns padded to each
        // other's widths.
        let mut end = start + 1;
        while end < rows.len()
            && matches!(
                rows[end].kind,
                MarkdownPreviewRowKind::TableRow { is_header: false }
            )
        {
            end += 1;
        }

        align_table_block_rows(&mut rows[start..end]);
        start = end;
    }
}

fn align_table_block_rows(rows: &mut [MarkdownPreviewRow]) {
    // Most preview tables are plain text, so avoid per-cell owned buffers when
    // there are no inline spans to remap.
    if rows.iter().all(|row| row.inline_spans.is_empty()) {
        align_table_block_rows_without_inline_spans(rows);
        return;
    }

    let split_rows = rows
        .iter()
        .map(|row| split_markdown_table_cells(row.text.as_ref(), row.inline_spans.as_ref()))
        .collect::<Vec<_>>();
    let column_count = split_rows.iter().map(Vec::len).max().unwrap_or(0);
    if column_count == 0 {
        return;
    }

    let mut column_widths = vec![0usize; column_count];
    for cells in &split_rows {
        for (ix, cell) in cells.iter().enumerate() {
            column_widths[ix] = column_widths[ix].max(cell.text.chars().count());
        }
    }

    for (row, cells) in rows.iter_mut().zip(split_rows) {
        let (text, spans) = build_aligned_table_row_text(cells, &column_widths);
        row.text = text.into();
        row.inline_spans = Arc::new(spans);
    }
}

fn align_table_block_rows_without_inline_spans(rows: &mut [MarkdownPreviewRow]) {
    let mut column_widths: Vec<usize> = Vec::new();

    for row in rows.iter() {
        for (column_ix, (_, cell_width)) in
            MarkdownTableCellIter::new(row.text.as_ref()).enumerate()
        {
            if let Some(width) = column_widths.get_mut(column_ix) {
                *width = (*width).max(cell_width);
            } else {
                column_widths.push(cell_width);
            }
        }
    }

    if column_widths.is_empty() {
        return;
    }

    for row in rows.iter_mut() {
        row.text =
            build_aligned_table_row_text_without_spans(row.text.as_ref(), column_widths.as_slice())
                .into();
    }
}

struct MarkdownTableCellIter<'a> {
    text: &'a str,
    next_start: usize,
    finished: bool,
}

impl<'a> MarkdownTableCellIter<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            next_start: 0,
            finished: false,
        }
    }
}

impl<'a> Iterator for MarkdownTableCellIter<'a> {
    type Item = (&'a str, usize);

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        let start = self.next_start;
        let mut char_width = 0usize;
        for (relative_byte_ix, ch) in self.text[start..].char_indices() {
            if ch == '\t' {
                let end = start + relative_byte_ix;
                self.next_start = end + ch.len_utf8();
                return Some((&self.text[start..end], char_width));
            }
            char_width += 1;
        }

        self.finished = true;
        (start < self.text.len() || start == 0).then_some((&self.text[start..], char_width))
    }
}

fn split_markdown_table_cells(
    text: &str,
    inline_spans: &[MarkdownInlineSpan],
) -> Vec<MarkdownTableCell> {
    let mut cell_ranges = Vec::new();
    let mut cell_start = 0usize;
    for (byte_ix, ch) in text.char_indices() {
        if ch == '\t' {
            cell_ranges.push(cell_start..byte_ix);
            cell_start = byte_ix + ch.len_utf8();
        }
    }
    if cell_ranges.is_empty() || cell_start < text.len() {
        cell_ranges.push(cell_start..text.len());
    }

    cell_ranges
        .into_iter()
        .map(|range| {
            let cell_text = text
                .get(range.clone())
                .map(str::to_owned)
                .unwrap_or_default();
            let spans = inline_spans
                .iter()
                .filter_map(|span| {
                    let start = span.byte_range.start.max(range.start);
                    let end = span.byte_range.end.min(range.end);
                    if start < end {
                        Some(span.restyled((start - range.start)..(end - range.start)))
                    } else {
                        None
                    }
                })
                .collect();
            MarkdownTableCell {
                text: cell_text,
                spans,
            }
        })
        .collect()
}

fn build_aligned_table_row_text(
    cells: Vec<MarkdownTableCell>,
    column_widths: &[usize],
) -> (String, Vec<MarkdownInlineSpan>) {
    const TABLE_COLUMN_SEPARATOR: &str = " | ";

    let text_capacity = column_widths
        .iter()
        .copied()
        .fold(0usize, usize::saturating_add)
        .saturating_add(
            cells
                .iter()
                .fold(0usize, |bytes, cell| bytes.saturating_add(cell.text.len())),
        )
        .saturating_add(
            TABLE_COLUMN_SEPARATOR
                .len()
                .saturating_mul(column_widths.len().saturating_sub(1)),
        );
    let span_capacity = cells
        .iter()
        .fold(0usize, |len, cell| len.saturating_add(cell.spans.len()));
    let mut text = String::with_capacity(text_capacity);
    let mut spans = Vec::with_capacity(span_capacity);
    let mut cells = cells.into_iter();

    for (ix, width) in column_widths.iter().copied().enumerate() {
        let cell = cells.next();
        let cell_width = cell
            .as_ref()
            .map(|cell| cell.text.chars().count())
            .unwrap_or(0);
        let cell_start = text.len();
        if let Some(cell) = cell {
            text.push_str(&cell.text);
            spans.extend(cell.spans.into_iter().map(|span| {
                span.restyled(
                    (cell_start + span.byte_range.start)..(cell_start + span.byte_range.end),
                )
            }));
        }

        if ix + 1 < column_widths.len() {
            let pad = width.saturating_sub(cell_width);
            for _ in 0..pad {
                text.push(' ');
            }
            text.push_str(TABLE_COLUMN_SEPARATOR);
        }
    }

    (text, spans)
}

fn build_aligned_table_row_text_without_spans(text: &str, column_widths: &[usize]) -> String {
    const TABLE_COLUMN_SEPARATOR: &str = " | ";

    let mut aligned = String::with_capacity(
        text.len().saturating_add(
            column_widths
                .len()
                .saturating_sub(1)
                .saturating_mul(TABLE_COLUMN_SEPARATOR.len().saturating_sub(1)),
        ),
    );
    let mut cells = MarkdownTableCellIter::new(text);

    for (column_ix, width) in column_widths.iter().copied().enumerate() {
        let Some((cell_text, cell_width)) = cells.next() else {
            if column_ix + 1 < column_widths.len() {
                for _ in 0..width {
                    aligned.push(' ');
                }
                aligned.push_str(TABLE_COLUMN_SEPARATOR);
            }
            continue;
        };

        aligned.push_str(cell_text);
        if column_ix + 1 < column_widths.len() {
            for _ in 0..width.saturating_sub(cell_width) {
                aligned.push(' ');
            }
            aligned.push_str(TABLE_COLUMN_SEPARATOR);
        }
    }

    aligned
}
