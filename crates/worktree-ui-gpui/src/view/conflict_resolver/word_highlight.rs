//! Word-level highlight computation over conflict sides, the aligned-row
//! highlight maps and the bounded split-row highlight cache.

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{ThreeWayAlignedMap, indexed_line_text};

#[cfg(any(test, feature = "benchmarks"))]
use super::{
    ConflictSegment, LARGE_CONFLICT_BLOCK_WORD_HIGHLIGHT_MAX_LINES, block_max_line_count,
    text_line_count,
};
#[cfg(feature = "benchmarks")]
use std::num::NonZeroU32;

#[cfg(any(test, feature = "benchmarks"))]
fn should_skip_large_block_word_highlights(block: &super::ConflictBlock) -> bool {
    block_max_line_count(block) > LARGE_CONFLICT_BLOCK_WORD_HIGHLIGHT_MAX_LINES
}

#[cfg(any(test, feature = "benchmarks"))]
pub fn compute_three_way_word_highlights(
    base_text: &str,
    base_line_starts: &[usize],
    ours_text: &str,
    ours_line_starts: &[usize],
    theirs_text: &str,
    theirs_line_starts: &[usize],
    marker_segments: &[ConflictSegment],
) -> (WordHighlights, WordHighlights, WordHighlights) {
    let mut wh_base: WordHighlights = WordHighlights::default();
    let mut wh_ours: WordHighlights = WordHighlights::default();
    let mut wh_theirs: WordHighlights = WordHighlights::default();

    fn merge_line_ranges(
        highlights: &mut WordHighlights,
        line_ix: usize,
        ranges: Vec<Range<usize>>,
    ) {
        if ranges.is_empty() {
            return;
        }
        highlights
            .entry(line_ix)
            .and_modify(|existing| {
                *existing = merge_ranges(existing, &ranges);
            })
            .or_insert(ranges);
    }

    fn line_index(start: usize, line_no: Option<u32>) -> Option<usize> {
        let local = usize::try_from(line_no?).ok()?.checked_sub(1)?;
        start.checked_add(local)
    }

    fn full_line_range(text: &str, line_starts: &[usize], line_ix: usize) -> Vec<Range<usize>> {
        let Some(line) = indexed_line_text(text, line_starts, line_ix) else {
            return Vec::new();
        };
        if line.is_empty() {
            return Vec::new();
        }
        std::iter::once(0..line.len()).collect()
    }

    struct HighlightSide<'a> {
        global_start: usize,
        text: &'a str,
        line_starts: &'a [usize],
    }

    fn apply_aligned_word_highlights(
        old_text: &str,
        new_text: &str,
        old_side: HighlightSide<'_>,
        new_side: HighlightSide<'_>,
        old_highlights: &mut WordHighlights,
        new_highlights: &mut WordHighlights,
    ) {
        use worktree_core::file_diff::PlanRowView;

        worktree_core::file_diff::for_each_side_by_side_row(
            old_text,
            new_text,
            |view| match view {
                PlanRowView::Modify {
                    old_line,
                    new_line,
                    old_text: old,
                    new_text: new,
                } => {
                    let (old_ranges, new_ranges) =
                        crate::view::word_diff::capped_word_diff_ranges(old, new);

                    if let Some(ix) = line_index(old_side.global_start, Some(old_line)) {
                        merge_line_ranges(old_highlights, ix, old_ranges);
                    }
                    if let Some(ix) = line_index(new_side.global_start, Some(new_line)) {
                        merge_line_ranges(new_highlights, ix, new_ranges);
                    }
                }
                PlanRowView::Remove { old_line, .. } => {
                    if let Some(ix) = line_index(old_side.global_start, Some(old_line)) {
                        merge_line_ranges(
                            old_highlights,
                            ix,
                            full_line_range(old_side.text, old_side.line_starts, ix),
                        );
                    }
                }
                PlanRowView::Add { new_line, .. } => {
                    if let Some(ix) = line_index(new_side.global_start, Some(new_line)) {
                        merge_line_ranges(
                            new_highlights,
                            ix,
                            full_line_range(new_side.text, new_side.line_starts, ix),
                        );
                    }
                }
                PlanRowView::Context { .. } => {}
            },
        );
    }

    let mut base_offset = 0usize;
    let mut ours_offset = 0usize;
    let mut theirs_offset = 0usize;
    for seg in marker_segments {
        match seg {
            ConflictSegment::Text(text) => {
                let n = usize::try_from(text_line_count(text)).unwrap_or(0);
                base_offset = base_offset.saturating_add(n);
                ours_offset = ours_offset.saturating_add(n);
                theirs_offset = theirs_offset.saturating_add(n);
            }
            ConflictSegment::Block(block) => {
                let base_count =
                    usize::try_from(text_line_count(block.base.as_deref().unwrap_or_default()))
                        .unwrap_or(0);
                let ours_count = usize::try_from(text_line_count(&block.ours)).unwrap_or(0);
                let theirs_count = usize::try_from(text_line_count(&block.theirs)).unwrap_or(0);
                if should_skip_large_block_word_highlights(block) {
                    base_offset = base_offset.saturating_add(base_count);
                    ours_offset = ours_offset.saturating_add(ours_count);
                    theirs_offset = theirs_offset.saturating_add(theirs_count);
                    continue;
                }

                if let Some(base) = block.base.as_deref() {
                    apply_aligned_word_highlights(
                        base,
                        &block.ours,
                        HighlightSide {
                            global_start: base_offset,
                            text: base_text,
                            line_starts: base_line_starts,
                        },
                        HighlightSide {
                            global_start: ours_offset,
                            text: ours_text,
                            line_starts: ours_line_starts,
                        },
                        &mut wh_base,
                        &mut wh_ours,
                    );
                    apply_aligned_word_highlights(
                        base,
                        &block.theirs,
                        HighlightSide {
                            global_start: base_offset,
                            text: base_text,
                            line_starts: base_line_starts,
                        },
                        HighlightSide {
                            global_start: theirs_offset,
                            text: theirs_text,
                            line_starts: theirs_line_starts,
                        },
                        &mut wh_base,
                        &mut wh_theirs,
                    );
                }
                // Local/Remote highlighting must align by diff rows, not absolute same-row index.
                apply_aligned_word_highlights(
                    &block.ours,
                    &block.theirs,
                    HighlightSide {
                        global_start: ours_offset,
                        text: ours_text,
                        line_starts: ours_line_starts,
                    },
                    HighlightSide {
                        global_start: theirs_offset,
                        text: theirs_text,
                        line_starts: theirs_line_starts,
                    },
                    &mut wh_ours,
                    &mut wh_theirs,
                );
                base_offset = base_offset.saturating_add(base_count);
                ours_offset = ours_offset.saturating_add(ours_count);
                theirs_offset = theirs_offset.saturating_add(theirs_count);
            }
        }
    }

    (wh_base, wh_ours, wh_theirs)
}

#[cfg(any(test, feature = "benchmarks"))]
fn merge_ranges(a: &[Range<usize>], b: &[Range<usize>]) -> Vec<Range<usize>> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let mut combined: Vec<Range<usize>> = Vec::with_capacity(a.len() + b.len());
    combined.extend_from_slice(a);
    combined.extend_from_slice(b);
    combined.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::with_capacity(combined.len());
    for r in combined {
        if let Some(last) = out.last_mut().filter(|l| r.start <= l.end) {
            last.end = last.end.max(r.end);
            continue;
        }
        out.push(r);
    }
    out
}

/// Per-line pair of (old, new) word-highlight ranges for two-way diff.
#[cfg(feature = "benchmarks")]
#[derive(Clone, Debug, Default)]
pub struct TwoWayWordHighlights {
    row_to_entry: Box<[Option<NonZeroU32>]>,
    entries: Box<[TwoWayWordHighlightPair]>,
}

#[cfg(feature = "benchmarks")]
impl TwoWayWordHighlights {
    pub fn len(&self) -> usize {
        self.row_to_entry.len()
    }

    pub fn get(&self, row_ix: usize) -> Option<&TwoWayWordHighlightPair> {
        let entry_ix = self
            .row_to_entry
            .get(row_ix)?
            .map(|ix| ix.get() as usize - 1)?;
        self.entries.get(entry_ix)
    }

    pub fn iter(&self) -> impl Iterator<Item = Option<&TwoWayWordHighlightPair>> + '_ {
        self.row_to_entry
            .iter()
            .copied()
            .map(|entry_ix| entry_ix.and_then(|ix| self.entries.get(ix.get() as usize - 1)))
    }

    #[cfg(test)]
    pub(super) fn highlighted_rows(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(feature = "benchmarks")]
pub fn compute_two_way_word_highlights(
    diff_rows: &[worktree_core::file_diff::FileDiffRow],
) -> TwoWayWordHighlights {
    let modify_rows = diff_rows
        .iter()
        .filter(|row| row.kind == worktree_core::file_diff::FileDiffRowKind::Modify)
        .count();
    let mut row_to_entry = vec![None; diff_rows.len()];
    let mut entries = Vec::with_capacity(modify_rows);

    for (row_ix, row) in diff_rows.iter().enumerate() {
        if row.kind != worktree_core::file_diff::FileDiffRowKind::Modify {
            continue;
        }
        let old = row.old.as_deref().unwrap_or("");
        let new = row.new.as_deref().unwrap_or("");
        let (old_ranges, new_ranges) =
            crate::view::word_diff::compact_capped_word_diff_ranges(old, new);
        if old_ranges.is_empty() && new_ranges.is_empty() {
            continue;
        }

        entries.push((old_ranges, new_ranges));
        let entry_ix = NonZeroU32::new(
            u32::try_from(entries.len()).expect("two-way word highlights should fit in u32"),
        )
        .expect("stored highlight entry index should be non-zero");
        row_to_entry[row_ix] = Some(entry_ix);
    }

    TwoWayWordHighlights {
        row_to_entry: row_to_entry.into_boxed_slice(),
        entries: entries.into_boxed_slice(),
    }
}

/// Compute word-level highlights for a single `FileDiffRow` on the fly.
///
/// Used in giant/streamed mode where word highlights are not pre-computed for
/// all rows. Only produces highlights for `Modify` rows (both sides present,
/// text differs).
pub fn compute_word_highlights_for_row(
    row: &worktree_core::file_diff::FileDiffRow,
) -> Option<TwoWayWordHighlightPair> {
    if row.kind != worktree_core::file_diff::FileDiffRowKind::Modify {
        return None;
    }
    compute_word_highlights_for_texts(
        row.old.as_deref().unwrap_or(""),
        row.new.as_deref().unwrap_or(""),
    )
}

/// Compute word-level highlights for an ours/theirs line pair directly
/// (section 30 aligned two-way rows, where no `FileDiffRow` is materialized).
pub fn compute_word_highlights_for_texts(old: &str, new: &str) -> Option<TwoWayWordHighlightPair> {
    let (old_ranges, new_ranges) =
        crate::view::word_diff::compact_capped_word_diff_ranges(old, new);
    if old_ranges.is_empty() && new_ranges.is_empty() {
        None
    } else {
        Some((old_ranges, new_ranges))
    }
}

#[cfg(all(test, feature = "benchmarks"))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use worktree_core::file_diff::{FileDiffLineText, FileDiffRow, FileDiffRowKind};

    fn modify_row(old: &'static str, new: &'static str) -> FileDiffRow {
        FileDiffRow {
            kind: FileDiffRowKind::Modify,
            old_line: Some(1),
            new_line: Some(1),
            old: Some(FileDiffLineText::shared(Arc::<str>::from(old))),
            new: Some(FileDiffLineText::shared(Arc::<str>::from(new))),
            eof_newline: None,
        }
    }

    #[test]
    fn two_way_word_highlights_store_only_rows_with_ranges() {
        let rows = vec![
            FileDiffRow {
                kind: FileDiffRowKind::Context,
                old_line: Some(1),
                new_line: Some(1),
                old: Some(FileDiffLineText::shared(Arc::<str>::from("same"))),
                new: Some(FileDiffLineText::shared(Arc::<str>::from("same"))),
                eof_newline: None,
            },
            modify_row(
                "let value = compute_local(1);",
                "let value = compute_remote(1);",
            ),
            modify_row(
                "let shared_alpha = compute_local(2);",
                "let shared_alpha_tail = compute_remote(2);",
            ),
        ];

        let highlights = compute_two_way_word_highlights(&rows);

        assert_eq!(highlights.len(), rows.len());
        assert_eq!(highlights.highlighted_rows(), 2);
        assert!(highlights.get(0).is_none());
        assert!(highlights.get(1).is_some());
        assert!(highlights.get(2).is_some());
        assert_eq!(highlights.iter().filter(|entry| entry.is_some()).count(), 2);
    }
}

/// Per-line word-highlight ranges. `None` means no highlights for that line.
pub type WordHighlights = FxHashMap<usize, Vec<Range<usize>>>;

/// Per-line pair of `(old, new)` word-highlight ranges for a two-way diff row.
pub type TwoWayWordHighlightPair = (
    crate::view::word_diff::WordDiffRanges,
    crate::view::word_diff::WordDiffRanges,
);

const CONFLICT_SPLIT_WORD_HIGHLIGHT_CACHE_ROWS: usize = 4_096;

/// Bounded render cache for giant two-way conflicts. The same row is rendered
/// independently by the left and right lists, so sharing the computed pair here
/// avoids running the word diff twice per frame without retaining the whole file.
#[derive(Clone, Debug, Default)]
pub(in crate::view) struct ConflictSplitWordHighlightCache {
    rows: FxHashMap<usize, Arc<TwoWayWordHighlightPair>>,
    insertion_order: VecDeque<usize>,
}

impl ConflictSplitWordHighlightCache {
    pub(in crate::view) fn get(&self, row_ix: usize) -> Option<Arc<TwoWayWordHighlightPair>> {
        self.rows.get(&row_ix).cloned()
    }

    pub(in crate::view) fn insert(
        &mut self,
        row_ix: usize,
        highlights: TwoWayWordHighlightPair,
    ) -> Arc<TwoWayWordHighlightPair> {
        if let Some(existing) = self.rows.get(&row_ix) {
            return Arc::clone(existing);
        }
        while self.rows.len() >= CONFLICT_SPLIT_WORD_HIGHLIGHT_CACHE_ROWS {
            let Some(evicted) = self.insertion_order.pop_front() else {
                break;
            };
            self.rows.remove(&evicted);
        }
        let highlights = Arc::new(highlights);
        self.rows.insert(row_ix, Arc::clone(&highlights));
        self.insertion_order.push_back(row_ix);
        highlights
    }

    pub(in crate::view) fn clear(&mut self) {
        self.rows.clear();
        self.insertion_order.clear();
    }
}

/// section 30 R11: cap on aligned rows that receive word-level highlights, bounding
/// the per-row word-diff work on files with huge change counts.
pub const ALIGNED_WORD_HIGHLIGHT_MAX_ROWS: usize = 4_000;

fn merge_word_highlight_ranges(
    highlights: &mut WordHighlights,
    line_ix: usize,
    ranges: Vec<Range<usize>>,
) {
    if ranges.is_empty() {
        return;
    }
    let entry = highlights.entry(line_ix).or_default();
    entry.extend(ranges);
    entry.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(entry.len());
    for r in entry.drain(..) {
        if let Some(last) = merged.last_mut().filter(|l| r.start <= l.end) {
            last.end = last.end.max(r.end);
            continue;
        }
        merged.push(r);
    }
    *entry = merged;
}

/// section 30 R11 (kdiff3 change colours): word highlights over the aligned row
/// space. For each aligned row where a side's line differs from the base line
/// paired at the same row, word-diff the pair and record ranges keyed by each
/// side's own line index (the renderer's cache key space). Padding rows
/// (added/removed lines) get no word ranges — the per-side row tint already
/// marks them whole. Requires a real base; both-added (two-way) maps use the
/// two-way highlight path instead.
pub fn compute_aligned_three_way_word_highlights(
    aligned: &ThreeWayAlignedMap,
    base_text: &str,
    base_line_starts: &[usize],
    ours_text: &str,
    ours_line_starts: &[usize],
    theirs_text: &str,
    theirs_line_starts: &[usize],
) -> (WordHighlights, WordHighlights, WordHighlights) {
    let mut wh_base = WordHighlights::default();
    let mut wh_ours = WordHighlights::default();
    let mut wh_theirs = WordHighlights::default();
    if aligned.is_identity() || base_text.is_empty() {
        return (wh_base, wh_ours, wh_theirs);
    }

    let mut budget = ALIGNED_WORD_HIGHLIGHT_MAX_ROWS;
    for row in 0..aligned.aligned_len() {
        if budget == 0 {
            break;
        }
        let Some(base_ix) = aligned.side_line_for_row(0, row) else {
            continue;
        };
        let Some(base_line) = indexed_line_text(base_text, base_line_starts, base_ix) else {
            continue;
        };
        let mut row_diffed = false;
        for (side, side_text, side_starts, side_highlights) in [
            (1usize, ours_text, ours_line_starts, &mut wh_ours),
            (2usize, theirs_text, theirs_line_starts, &mut wh_theirs),
        ] {
            let Some(side_ix) = aligned.side_line_for_row(side, row) else {
                continue;
            };
            let Some(side_line) = indexed_line_text(side_text, side_starts, side_ix) else {
                continue;
            };
            if side_line == base_line {
                continue;
            }
            let (base_ranges, side_ranges) =
                crate::view::word_diff::capped_word_diff_ranges(base_line, side_line);
            merge_word_highlight_ranges(&mut wh_base, base_ix, base_ranges);
            merge_word_highlight_ranges(side_highlights, side_ix, side_ranges);
            row_diffed = true;
        }
        if row_diffed {
            budget -= 1;
        }
    }

    (wh_base, wh_ours, wh_theirs)
}

/// section 30 R11: aligned two-way (ours↔theirs) word highlights, precomputed
/// once per conflict-source rebuild and shared by both diff columns (Ours and
/// Theirs). Keyed by aligned row — the renderer's row space. Only rows where
/// both sides have a line and the two lines differ byte-wise get an entry; the
/// render-time whitespace mode still decides whether to *apply* them
/// (whitespace-equal rows render as context), so this stays independent of that
/// toggle. Replaces the previous per-render, per-column inline word diff.
pub fn compute_aligned_two_way_word_highlights(
    aligned: &ThreeWayAlignedMap,
    ours_text: &str,
    ours_line_starts: &[usize],
    theirs_text: &str,
    theirs_line_starts: &[usize],
) -> FxHashMap<usize, TwoWayWordHighlightPair> {
    let mut highlights = FxHashMap::default();
    if aligned.is_identity() {
        return highlights;
    }

    let mut budget = ALIGNED_WORD_HIGHLIGHT_MAX_ROWS;
    for row in 0..aligned.aligned_len() {
        if budget == 0 {
            break;
        }
        let (Some(ours_ix), Some(theirs_ix)) = (
            aligned.side_line_for_row(1, row),
            aligned.side_line_for_row(2, row),
        ) else {
            continue;
        };
        let (Some(ours_line), Some(theirs_line)) = (
            indexed_line_text(ours_text, ours_line_starts, ours_ix),
            indexed_line_text(theirs_text, theirs_line_starts, theirs_ix),
        ) else {
            continue;
        };
        if ours_line == theirs_line {
            continue;
        }
        if let Some(pair) = compute_word_highlights_for_texts(ours_line, theirs_line) {
            highlights.insert(row, pair);
            budget -= 1;
        }
    }

    highlights
}
