//! Block-local two-way diff rows: tiny-block fast paths, boundary context
//! windows and head/tail previews for giant conflict blocks.

#[cfg(any(test, feature = "benchmarks"))]
use super::*;
#[cfg(any(test, feature = "benchmarks"))]
use std::sync::Arc;

/// Shared context rows kept around each block-local two-way conflict diff.
///
/// This preserves a small amount of unchanged surrounding code in the large-file
/// sparse path without regressing back to whole-file row materialization.
pub(crate) const BLOCK_LOCAL_DIFF_CONTEXT_LINES: usize = 3;
/// Above this size, one conflict block is effectively the whole document.
///
/// Bootstrap should stay bounded instead of diffing the entire block eagerly.
pub(crate) const LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES: usize = 20_000;
/// Head/tail preview rows kept for very large conflict blocks during bootstrap.
#[cfg(any(test, feature = "benchmarks"))]
pub(crate) const LARGE_CONFLICT_BLOCK_PREVIEW_LINES: usize = 128;
/// Word-diff highlighting is optional chrome, so skip giant blocks entirely.
#[cfg(any(test, feature = "benchmarks"))]
pub(crate) const LARGE_CONFLICT_BLOCK_WORD_HIGHLIGHT_MAX_LINES: usize = 4_000;

#[cfg(any(test, feature = "benchmarks"))]
pub fn build_inline_rows(rows: &[worktree_core::file_diff::FileDiffRow]) -> Vec<ConflictInlineRow> {
    use worktree_core::domain::DiffLineKind as K;
    use worktree_core::file_diff::FileDiffRowKind as RK;

    let extra = rows.iter().filter(|r| matches!(r.kind, RK::Modify)).count();
    let mut out: Vec<ConflictInlineRow> = Vec::with_capacity(rows.len() + extra);
    for row in rows {
        match row.kind {
            RK::Context => out.push(ConflictInlineRow {
                side: ConflictPickSide::Ours,
                kind: K::Context,
                old_line: row.old_line,
                new_line: row.new_line,
                content: row.old.as_deref().unwrap_or("").to_string(),
            }),
            RK::Add => out.push(ConflictInlineRow {
                side: ConflictPickSide::Theirs,
                kind: K::Add,
                old_line: None,
                new_line: row.new_line,
                content: row.new.as_deref().unwrap_or("").to_string(),
            }),
            RK::Remove => out.push(ConflictInlineRow {
                side: ConflictPickSide::Ours,
                kind: K::Remove,
                old_line: row.old_line,
                new_line: None,
                content: row.old.as_deref().unwrap_or("").to_string(),
            }),
            RK::Modify => {
                out.push(ConflictInlineRow {
                    side: ConflictPickSide::Ours,
                    kind: K::Remove,
                    old_line: row.old_line,
                    new_line: None,
                    content: row.old.as_deref().unwrap_or("").to_string(),
                });
                out.push(ConflictInlineRow {
                    side: ConflictPickSide::Theirs,
                    kind: K::Add,
                    old_line: None,
                    new_line: row.new_line,
                    content: row.new.as_deref().unwrap_or("").to_string(),
                });
            }
        }
    }
    out
}

#[cfg(any(test, feature = "benchmarks"))]
pub(super) fn block_max_line_count(block: &ConflictBlock) -> usize {
    text_line_count_usize(block.base.as_deref().unwrap_or_default())
        .max(text_line_count_usize(&block.ours))
        .max(text_line_count_usize(&block.theirs))
}

#[cfg(any(test, feature = "benchmarks"))]
fn should_use_large_conflict_block_preview(block: &ConflictBlock) -> bool {
    block_max_line_count(block) > LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES
}

#[cfg(any(test, feature = "benchmarks"))]
pub(super) fn preview_line_starts(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut starts = Vec::with_capacity(text.len().saturating_div(64).saturating_add(1));
    starts.push(0);
    for (ix, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(ix.saturating_add(1));
        }
    }
    starts
}

#[cfg(any(test, feature = "benchmarks"))]
fn line_slice_text<'a>(
    text: &'a str,
    line_starts: &[usize],
    line_count: usize,
    start_line_ix: usize,
    end_line_ix: usize,
) -> &'a str {
    if text.is_empty() || line_count == 0 {
        return "";
    }

    let start = start_line_ix.min(line_count);
    let end = end_line_ix.min(line_count);
    if start >= end {
        return "";
    }

    let text_len = text.len();
    let start_byte = line_starts
        .get(start)
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    let end_byte = if end >= line_count {
        text_len
    } else {
        line_starts
            .get(end)
            .copied()
            .unwrap_or(text_len)
            .min(text_len)
    };
    if start_byte >= end_byte {
        return "";
    }
    text.get(start_byte..end_byte).unwrap_or("")
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_renumbered_block_diff_rows(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    old_text: &str,
    new_text: &str,
    old_line_offset: u32,
    new_line_offset: u32,
) -> bool {
    let old_line_count = text_line_count_usize(old_text);
    let new_line_count = text_line_count_usize(new_text);
    let whole_block_diff_ran = old_line_count > LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES
        || new_line_count > LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES;
    debug_assert!(
        !whole_block_diff_ran,
        "bootstrap should not call side_by_side_rows on a giant conflict block"
    );
    if push_tiny_block_diff_rows(rows, old_text, new_text, old_line_offset, new_line_offset) {
        return false;
    }
    worktree_core::file_diff::append_side_by_side_rows_with_offsets(
        rows,
        old_text,
        new_text,
        old_line_offset,
        new_line_offset,
    );
    whole_block_diff_ran
}

#[cfg(any(test, feature = "benchmarks"))]
fn collect_tiny_block_lines(text: &str) -> Option<([&str; 2], usize)> {
    let mut lines = ["", ""];
    let mut count = 0usize;
    for line in text.lines() {
        if count == lines.len() {
            return None;
        }
        lines[count] = line;
        count += 1;
    }
    Some((lines, count))
}

#[cfg(any(test, feature = "benchmarks"))]
fn tiny_block_line_number(start: u32, offset: usize) -> u32 {
    start.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX))
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_tiny_block_context_row(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    old_line: u32,
    new_line: u32,
    text: &str,
) {
    use worktree_core::file_diff::{FileDiffRow, FileDiffRowKind};

    rows.push(FileDiffRow {
        kind: FileDiffRowKind::Context,
        old_line: Some(old_line),
        new_line: Some(new_line),
        old: Some(text.into()),
        new: Some(text.into()),
        eof_newline: None,
    });
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_tiny_block_modify_row(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    old_line: u32,
    new_line: u32,
    old_text: &str,
    new_text: &str,
) {
    use worktree_core::file_diff::{FileDiffRow, FileDiffRowKind};

    rows.push(FileDiffRow {
        kind: FileDiffRowKind::Modify,
        old_line: Some(old_line),
        new_line: Some(new_line),
        old: Some(old_text.into()),
        new: Some(new_text.into()),
        eof_newline: None,
    });
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_tiny_block_remove_row(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    old_line: u32,
    text: &str,
) {
    use worktree_core::file_diff::{FileDiffRow, FileDiffRowKind};

    rows.push(FileDiffRow {
        kind: FileDiffRowKind::Remove,
        old_line: Some(old_line),
        new_line: None,
        old: Some(text.into()),
        new: None,
        eof_newline: None,
    });
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_tiny_block_add_row(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    new_line: u32,
    text: &str,
) {
    use worktree_core::file_diff::{FileDiffRow, FileDiffRowKind};

    rows.push(FileDiffRow {
        kind: FileDiffRowKind::Add,
        old_line: None,
        new_line: Some(new_line),
        old: None,
        new: Some(text.into()),
        eof_newline: None,
    });
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_tiny_block_diff_rows(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    old_text: &str,
    new_text: &str,
    old_line_offset: u32,
    new_line_offset: u32,
) -> bool {
    if (!old_text.is_empty() && !old_text.ends_with('\n'))
        || (!new_text.is_empty() && !new_text.ends_with('\n'))
    {
        return false;
    }

    let Some((old_lines, old_len)) = collect_tiny_block_lines(old_text) else {
        return false;
    };
    let Some((new_lines, new_len)) = collect_tiny_block_lines(new_text) else {
        return false;
    };
    if old_len > 1 && new_len > 1 {
        return false;
    }

    let mut prefix = 0usize;
    while prefix < old_len && prefix < new_len && old_lines[prefix] == new_lines[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while prefix + suffix < old_len
        && prefix + suffix < new_len
        && old_lines[old_len - 1 - suffix] == new_lines[new_len - 1 - suffix]
    {
        suffix += 1;
    }

    for (offset, line) in old_lines.iter().enumerate().take(prefix) {
        push_tiny_block_context_row(
            rows,
            tiny_block_line_number(old_line_offset, offset),
            tiny_block_line_number(new_line_offset, offset),
            line,
        );
    }

    let old_mid_start = prefix;
    let new_mid_start = prefix;
    let old_mid_len = old_len.saturating_sub(prefix + suffix);
    let new_mid_len = new_len.saturating_sub(prefix + suffix);
    let paired_len = old_mid_len.min(new_mid_len);

    for offset in 0..paired_len {
        let old_ix = old_mid_start + offset;
        let new_ix = new_mid_start + offset;
        push_tiny_block_modify_row(
            rows,
            tiny_block_line_number(old_line_offset, old_ix),
            tiny_block_line_number(new_line_offset, new_ix),
            old_lines[old_ix],
            new_lines[new_ix],
        );
    }

    for offset in paired_len..old_mid_len {
        let old_ix = old_mid_start + offset;
        push_tiny_block_remove_row(
            rows,
            tiny_block_line_number(old_line_offset, old_ix),
            old_lines[old_ix],
        );
    }

    for offset in paired_len..new_mid_len {
        let new_ix = new_mid_start + offset;
        push_tiny_block_add_row(
            rows,
            tiny_block_line_number(new_line_offset, new_ix),
            new_lines[new_ix],
        );
    }

    for offset in 0..suffix {
        let old_ix = old_len - suffix + offset;
        let new_ix = new_len - suffix + offset;
        push_tiny_block_context_row(
            rows,
            tiny_block_line_number(old_line_offset, old_ix),
            tiny_block_line_number(new_line_offset, new_ix),
            old_lines[old_ix],
        );
    }

    true
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_large_conflict_block_preview_rows(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    block: &ConflictBlock,
    ours_offset: u32,
    theirs_offset: u32,
) {
    let ours_count = text_line_count_usize(&block.ours);
    let theirs_count = text_line_count_usize(&block.theirs);
    let ours_line_starts = preview_line_starts(&block.ours);
    let theirs_line_starts = preview_line_starts(&block.theirs);

    let head_ours_end = ours_count.min(LARGE_CONFLICT_BLOCK_PREVIEW_LINES);
    let head_theirs_end = theirs_count.min(LARGE_CONFLICT_BLOCK_PREVIEW_LINES);
    let _ = push_renumbered_block_diff_rows(
        rows,
        line_slice_text(&block.ours, &ours_line_starts, ours_count, 0, head_ours_end),
        line_slice_text(
            &block.theirs,
            &theirs_line_starts,
            theirs_count,
            0,
            head_theirs_end,
        ),
        ours_offset,
        theirs_offset,
    );

    let tail_ours_start = ours_count.saturating_sub(LARGE_CONFLICT_BLOCK_PREVIEW_LINES);
    let tail_theirs_start = theirs_count.saturating_sub(LARGE_CONFLICT_BLOCK_PREVIEW_LINES);
    let omitted_ours = tail_ours_start.saturating_sub(head_ours_end);
    let omitted_theirs = tail_theirs_start.saturating_sub(head_theirs_end);
    let can_show_tail = omitted_ours > 0 && omitted_theirs > 0;

    if omitted_ours > 0 || omitted_theirs > 0 {
        let summary: Arc<str> = format!(
            "... large conflict block preview omitted {omitted_ours} ours lines and {omitted_theirs} theirs lines ..."
        )
        .into();
        rows.push(worktree_core::file_diff::FileDiffRow {
            kind: worktree_core::file_diff::FileDiffRowKind::Context,
            old_line: (omitted_ours > 0).then(|| {
                ours_offset.saturating_add(u32::try_from(head_ours_end).unwrap_or(u32::MAX))
            }),
            new_line: (omitted_theirs > 0).then(|| {
                theirs_offset.saturating_add(u32::try_from(head_theirs_end).unwrap_or(u32::MAX))
            }),
            old: Some(Arc::clone(&summary).into()),
            new: Some(summary.into()),
            eof_newline: None,
        });
    }

    if can_show_tail {
        let _ = push_renumbered_block_diff_rows(
            rows,
            line_slice_text(
                &block.ours,
                &ours_line_starts,
                ours_count,
                tail_ours_start,
                ours_count,
            ),
            line_slice_text(
                &block.theirs,
                &theirs_line_starts,
                theirs_count,
                tail_theirs_start,
                theirs_count,
            ),
            ours_offset.saturating_add(u32::try_from(tail_ours_start).unwrap_or(u32::MAX)),
            theirs_offset.saturating_add(u32::try_from(tail_theirs_start).unwrap_or(u32::MAX)),
        );
    }
}

/// Build two-way diff rows using block-local diffs instead of a full-file Myers diff.
///
/// For each `Block` segment, a block-local `side_by_side_rows` is run on just
/// the block's ours vs theirs text, and the resulting rows are re-numbered to
/// global line positions. Surrounding `Text` segments contribute only a small
/// boundary context window, so unchanged file regions are not materialized in
/// full.
///
/// The output is proportional to total conflict-block size plus a fixed amount
/// of context per block, making it suitable for very large files where running
/// Myers on the entire ours/theirs content would be prohibitively expensive.
#[cfg(any(test, feature = "benchmarks"))]
pub fn block_local_two_way_diff_rows(
    segments: &[ConflictSegment],
) -> Vec<worktree_core::file_diff::FileDiffRow> {
    block_local_two_way_diff_rows_with_stats(segments).0
}

#[cfg(any(test, feature = "benchmarks"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BlockLocalTwoWayDiffStats {
    pub(crate) whole_block_diff_ran: bool,
}

#[cfg(any(test, feature = "benchmarks"))]
pub(crate) fn block_local_two_way_diff_rows_with_stats(
    segments: &[ConflictSegment],
) -> (
    Vec<worktree_core::file_diff::FileDiffRow>,
    BlockLocalTwoWayDiffStats,
) {
    block_local_two_way_diff_rows_with_context_and_stats(segments, BLOCK_LOCAL_DIFF_CONTEXT_LINES)
}

#[cfg(test)]
pub(super) fn block_local_two_way_diff_rows_with_context(
    segments: &[ConflictSegment],
    context_lines: usize,
) -> Vec<worktree_core::file_diff::FileDiffRow> {
    block_local_two_way_diff_rows_with_context_and_stats(segments, context_lines).0
}

#[cfg(any(test, feature = "benchmarks"))]
fn block_local_two_way_diff_rows_with_context_and_stats(
    segments: &[ConflictSegment],
    context_lines: usize,
) -> (
    Vec<worktree_core::file_diff::FileDiffRow>,
    BlockLocalTwoWayDiffStats,
) {
    let mut rows = Vec::with_capacity(estimate_block_local_two_way_row_capacity(
        segments,
        context_lines,
    ));
    let mut stats = BlockLocalTwoWayDiffStats::default();
    let mut ours_line = 1u32;
    let mut theirs_line = 1u32;

    for (segment_ix, segment) in segments.iter().enumerate() {
        match segment {
            ConflictSegment::Text(text) => {
                let count = push_block_local_boundary_context_rows(
                    &mut rows,
                    segments,
                    segment_ix,
                    text,
                    ours_line,
                    theirs_line,
                    context_lines,
                );
                ours_line = ours_line.saturating_add(count);
                theirs_line = theirs_line.saturating_add(count);
            }
            ConflictSegment::Block(block) => {
                let ours_offset = ours_line;
                let theirs_offset = theirs_line;
                if should_use_large_conflict_block_preview(block) {
                    push_large_conflict_block_preview_rows(
                        &mut rows,
                        block,
                        ours_offset,
                        theirs_offset,
                    );
                } else {
                    stats.whole_block_diff_ran |= push_renumbered_block_diff_rows(
                        &mut rows,
                        &block.ours,
                        &block.theirs,
                        ours_offset,
                        theirs_offset,
                    );
                }
                let ours_count = text_line_count(&block.ours);
                let theirs_count = text_line_count(&block.theirs);
                ours_line = ours_line.saturating_add(ours_count);
                theirs_line = theirs_line.saturating_add(theirs_count);
            }
        }
    }
    (rows, stats)
}

#[cfg(any(test, feature = "benchmarks"))]
fn estimate_block_local_two_way_row_capacity(
    segments: &[ConflictSegment],
    context_lines: usize,
) -> usize {
    segments
        .len()
        .saturating_mul(context_lines.saturating_add(2))
        .max(1)
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_block_local_boundary_context_rows(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    segments: &[ConflictSegment],
    segment_ix: usize,
    text: &ConflictText,
    old_line_start: u32,
    new_line_start: u32,
    context_lines: usize,
) -> u32 {
    let text_str = text.as_str();
    let line_count = text_line_count(text_str);
    if text_str.is_empty() || context_lines == 0 {
        return line_count;
    }

    let has_prev_block = segment_ix > 0
        && matches!(
            segments.get(segment_ix - 1),
            Some(ConflictSegment::Block(_))
        );
    let has_next_block = matches!(
        segments.get(segment_ix + 1),
        Some(ConflictSegment::Block(_))
    );
    if !has_prev_block && !has_next_block {
        return line_count;
    }

    let line_count_usize = usize::try_from(line_count).unwrap_or(usize::MAX);

    let leading_count = if has_prev_block {
        context_lines.min(line_count_usize)
    } else {
        0
    };
    let trailing_count = if has_next_block {
        context_lines.min(line_count_usize)
    } else {
        0
    };
    let trailing_start = line_count_usize.saturating_sub(trailing_count);

    // Leading context: scan forward for the first `leading_count` lines.
    push_block_local_context_lines(
        rows,
        text_str.lines().enumerate().take(leading_count),
        old_line_start,
        new_line_start,
    );

    // Trailing context: find the byte offset of the trailing_start-th line
    // by scanning backwards from the end, avoiding a full-text forward scan.
    let effective_trailing_start = leading_count.max(trailing_start);
    if trailing_count > 0 && effective_trailing_start < line_count_usize {
        let bytes = text_str.as_bytes();
        // Find byte offset of the effective_trailing_start-th line by
        // reverse-scanning for the (line_count - effective_trailing_start)
        // newlines from the end.
        let lines_from_end = line_count_usize - effective_trailing_start;
        let byte_offset = byte_offset_of_nth_line_from_end(bytes, lines_from_end);
        push_block_local_context_lines(
            rows,
            text_str[byte_offset..]
                .lines()
                .enumerate()
                .map(move |(ix, line)| (effective_trailing_start + ix, line)),
            old_line_start,
            new_line_start,
        );
    }

    line_count
}

#[cfg(any(test, feature = "benchmarks"))]
/// Find the byte offset of the `n`-th line from the end of the text.
/// Returns the byte offset where the `n`-th-from-end line starts.
#[cfg(any(test, feature = "benchmarks"))]
fn byte_offset_of_nth_line_from_end(bytes: &[u8], n: usize) -> usize {
    if n == 0 {
        return bytes.len();
    }
    // Count newlines from the end. We need to find `n` line-start positions.
    // A line starts either at the beginning of the text or after a newline.
    // If the text ends with \n, the last newline does NOT start a new line
    // (text_line_count_usize treats trailing \n as the last line's terminator).
    let mut remaining = n;
    let mut pos = bytes.len();
    // Skip trailing newline if present (it terminates the last counted line,
    // not a new line after it).
    if pos > 0 && bytes[pos - 1] == b'\n' {
        pos -= 1;
    }
    while remaining > 0 && pos > 0 {
        if let Some(nl) = memchr::memrchr(b'\n', &bytes[..pos]) {
            pos = nl;
            remaining -= 1;
        } else {
            // No more newlines; the first line starts at offset 0.
            return 0;
        }
    }
    if remaining > 0 {
        0
    } else {
        // `pos` points at the newline; the line starts after it.
        pos + 1
    }
}

#[cfg(any(test, feature = "benchmarks"))]
fn push_block_local_context_lines<'a>(
    rows: &mut Vec<worktree_core::file_diff::FileDiffRow>,
    lines: impl Iterator<Item = (usize, &'a str)>,
    old_line_start: u32,
    new_line_start: u32,
) {
    use worktree_core::file_diff::{FileDiffRow, FileDiffRowKind};

    for (line_ix, text) in lines {
        let line_offset = u32::try_from(line_ix).unwrap_or(u32::MAX);
        let content: Arc<str> = text.into();
        rows.push(FileDiffRow {
            kind: FileDiffRowKind::Context,
            old_line: Some(old_line_start.saturating_add(line_offset)),
            new_line: Some(new_line_start.saturating_add(line_offset)),
            old: Some(Arc::clone(&content).into()),
            new: Some(content.into()),
            eof_newline: None,
        });
    }
}
