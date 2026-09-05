//! Visible projections for the resolver lists: span-based hide-resolved,
//! collapsed-context folding, two-way visible indices and rendering-mode
//! selection.

use super::*;
use rustc_hash::FxHashMap;

/// Whether computing the three-way alignment is practical for these sides
/// (section 30 aligned row space).
///
/// The alignment diff is O(size × dissimilarity): a whole-file conflict on a
/// large file makes Myers effectively quadratic. Small files always align;
/// large ones only when each side still shares a reasonable fraction of its
/// lines with base.
pub fn three_way_alignment_is_practical(base: &str, ours: &str, theirs: &str) -> bool {
    worktree_core::merge::interactive_merge_plan_is_practical(
        Some(base),
        ours,
        theirs,
        worktree_core::merge::InteractiveMergePlanBudget::default(),
    )
}

/// Whether computing the direct two-way alignment is practical (section 30 aligned
/// row space, no-base fallback). Same rationale as
/// [`three_way_alignment_is_practical`], with ours standing in for the base
/// as the similarity anchor.
pub fn two_way_alignment_is_practical(ours: &str, theirs: &str) -> bool {
    worktree_core::merge::interactive_merge_plan_is_practical(
        None,
        ours,
        theirs,
        worktree_core::merge::InteractiveMergePlanBudget::default(),
    )
}

pub fn select_conflict_rendering_mode(
    segments: &[ConflictSegment],
    combined_line_count: usize,
) -> ConflictRenderingMode {
    let _ = combined_line_count;
    if !segments.is_empty() {
        ConflictRenderingMode::StreamedLargeFile
    } else {
        ConflictRenderingMode::EagerSmallFile
    }
}

#[cfg(any(test, feature = "benchmarks"))]
fn build_two_way_conflict_line_ranges(
    segments: &[ConflictSegment],
) -> Vec<(std::ops::Range<u32>, std::ops::Range<u32>)> {
    let mut ranges = Vec::new();
    let mut ours_line = 1u32;
    let mut theirs_line = 1u32;

    for seg in segments {
        match seg {
            ConflictSegment::Text(text) => {
                let count = text_line_count(text);
                ours_line = ours_line.saturating_add(count);
                theirs_line = theirs_line.saturating_add(count);
            }
            ConflictSegment::Block(block) => {
                let ours_count = text_line_count(&block.ours);
                let theirs_count = text_line_count(&block.theirs);
                let ours_end = ours_line.saturating_add(ours_count);
                let theirs_end = theirs_line.saturating_add(theirs_count);
                ranges.push((ours_line..ours_end, theirs_line..theirs_end));
                ours_line = ours_end;
                theirs_line = theirs_end;
            }
        }
    }

    ranges
}

#[cfg(any(test, feature = "benchmarks"))]
fn row_conflict_index_for_lines(
    old_line: Option<u32>,
    new_line: Option<u32>,
    ranges: &[(std::ops::Range<u32>, std::ops::Range<u32>)],
) -> Option<usize> {
    ranges.iter().position(|(ours, theirs)| {
        old_line.is_some_and(|line| ours.contains(&line))
            || new_line.is_some_and(|line| theirs.contains(&line))
    })
}

/// Build conflict-index maps for two-way split and inline rows.
///
/// Each output entry is `Some(conflict_index)` when the row belongs to a marker
/// conflict block, or `None` for non-conflict context rows.
#[cfg(any(test, feature = "benchmarks"))]
pub fn map_two_way_rows_to_conflicts(
    segments: &[ConflictSegment],
    diff_rows: &[worktree_core::file_diff::FileDiffRow],
    inline_rows: &[ConflictInlineRow],
) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let ranges = build_two_way_conflict_line_ranges(segments);
    let split = diff_rows
        .iter()
        .map(|row| row_conflict_index_for_lines(row.old_line, row.new_line, &ranges))
        .collect();
    let inline = inline_rows
        .iter()
        .map(|row| row_conflict_index_for_lines(row.old_line, row.new_line, &ranges))
        .collect();
    (split, inline)
}

/// Build visible row indices for two-way views.
///
/// When `hide_resolved` is true, rows belonging to resolved conflict blocks are
/// removed from the visible list. Non-conflict rows are always kept visible.
#[cfg(any(test, feature = "benchmarks"))]
pub fn build_two_way_visible_indices(
    row_conflict_map: &[Option<usize>],
    segments: &[ConflictSegment],
    hide_resolved: bool,
) -> Vec<usize> {
    if !hide_resolved {
        return (0..row_conflict_map.len()).collect();
    }

    let resolved_blocks: Vec<bool> = segments
        .iter()
        .filter_map(|s| match s {
            ConflictSegment::Block(b) => Some(b.resolved),
            _ => None,
        })
        .collect();

    row_conflict_map
        .iter()
        .enumerate()
        .filter_map(|(ix, conflict_ix)| match conflict_ix {
            Some(ci) if resolved_blocks.get(*ci).copied().unwrap_or(false) => None,
            _ => Some(ix),
        })
        .collect()
}

/// Find the visible list index for the first row that belongs to `conflict_ix`.
///
/// `visible_row_indices` maps visible list rows to source row indices. This helper
/// resolves conflict index -> visible row index so callers can scroll/focus a
/// specific conflict in two-way resolver modes.
#[cfg(test)]
pub fn visible_index_for_two_way_conflict(
    row_conflict_map: &[Option<usize>],
    visible_row_indices: &[usize],
    conflict_ix: usize,
) -> Option<usize> {
    visible_row_indices.iter().position(|&row_ix| {
        row_conflict_map
            .get(row_ix)
            .copied()
            .flatten()
            .is_some_and(|ix| ix == conflict_ix)
    })
}

/// Build unresolved-only visible navigation entries for two-way views.
///
/// Returns visible list indices (not source row indices) in unresolved queue
/// order so callers can feed them directly into shared diff navigation helpers.
#[cfg(test)]
pub fn unresolved_visible_nav_entries_for_two_way(
    segments: &[ConflictSegment],
    row_conflict_map: &[Option<usize>],
    visible_row_indices: &[usize],
) -> Vec<usize> {
    unresolved_conflict_indices(segments)
        .into_iter()
        .filter_map(|conflict_ix| {
            visible_index_for_two_way_conflict(row_conflict_map, visible_row_indices, conflict_ix)
        })
        .collect()
}

/// Map a two-way visible index back to its conflict index.
#[cfg(test)]
pub fn two_way_conflict_index_for_visible_row(
    row_conflict_map: &[Option<usize>],
    visible_row_indices: &[usize],
    visible_ix: usize,
) -> Option<usize> {
    let row_ix = *visible_row_indices.get(visible_ix)?;
    row_conflict_map.get(row_ix).copied().flatten()
}

/// Represents a visible row in the three-way view when hide-resolved is active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreeWayVisibleItem {
    /// A normal line at the given index in the three-way data.
    Line(usize),
    /// A collapsed summary row for a resolved conflict block (by conflict index).
    CollapsedBlock(usize),
    /// A folded run of unchanged context lines (section 30 collapsed context mode).
    CollapsedContext {
        source_line_start: usize,
        len: usize,
        /// Stable fold identity (the fold's start line before any reveals),
        /// used to key partial-reveal state.
        fold_id: usize,
    },
}

/// Span-based replacement for `Vec<ThreeWayVisibleItem>` that uses O(spans) memory
/// instead of O(visible lines). Each span covers a contiguous run of source lines
/// or a single synthetic row (collapsed block / preview gap).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreeWayVisibleSpan {
    /// A contiguous run of source lines mapped 1:1 to visible indices.
    Lines {
        visible_start: usize,
        source_line_start: usize,
        len: usize,
    },
    /// A single collapsed-block row at the given visible index.
    CollapsedResolvedBlock {
        visible_index: usize,
        conflict_ix: usize,
    },
    /// A single fold row hiding `len` unchanged context lines.
    CollapsedContext {
        visible_index: usize,
        source_line_start: usize,
        len: usize,
        /// Stable fold identity (start line before any reveals).
        fold_id: usize,
    },
}

impl ThreeWayVisibleSpan {
    fn visible_start(&self) -> usize {
        match *self {
            Self::Lines { visible_start, .. } => visible_start,
            Self::CollapsedResolvedBlock { visible_index, .. } => visible_index,
            Self::CollapsedContext { visible_index, .. } => visible_index,
        }
    }

    fn visible_len(&self) -> usize {
        match *self {
            Self::Lines { len, .. } => len,
            Self::CollapsedResolvedBlock { .. } | Self::CollapsedContext { .. } => 1,
        }
    }
}

/// Compact visible-index projection for three-way views.
///
/// Replaces `Vec<ThreeWayVisibleItem>` for giant mode. Stores spans instead of
/// per-row entries, keeping memory proportional to the number of conflict blocks
/// rather than the number of file lines.
#[derive(Clone, Debug, Default)]
pub struct ThreeWayVisibleProjection {
    spans: Vec<ThreeWayVisibleSpan>,
    visible_len: usize,
}

enum ThreeWayVisibleRun {
    Lines { start: usize, end: usize },
    Collapsed { conflict_ix: usize },
}

fn for_each_three_way_visible_run(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    conflict_resolved: &[bool],
    hide_resolved: bool,
    mut visit: impl FnMut(ThreeWayVisibleRun),
) {
    if total_lines == 0 {
        return;
    }

    if !hide_resolved {
        visit(ThreeWayVisibleRun::Lines {
            start: 0,
            end: total_lines,
        });
        return;
    }

    let mut line_ix = 0usize;

    for (range_ix, range) in conflict_ranges.iter().enumerate() {
        if line_ix >= total_lines {
            break;
        }

        let range_start = range.start.min(total_lines);
        let range_end = range.end.min(total_lines);

        if line_ix < range_start {
            visit(ThreeWayVisibleRun::Lines {
                start: line_ix,
                end: range_start,
            });
            line_ix = range_start;
        }

        let resolved = conflict_resolved.get(range_ix).copied().unwrap_or(false);
        if resolved && range_start < range_end && line_ix < range_end {
            visit(ThreeWayVisibleRun::Collapsed {
                conflict_ix: range_ix,
            });
            line_ix = range_end;
            continue;
        }

        if line_ix < range_end {
            visit(ThreeWayVisibleRun::Lines {
                start: line_ix,
                end: range_end,
            });
            line_ix = range_end;
        }
    }

    if line_ix < total_lines {
        visit(ThreeWayVisibleRun::Lines {
            start: line_ix,
            end: total_lines,
        });
    }
}

impl ThreeWayVisibleProjection {
    /// Total number of visible rows.
    pub fn len(&self) -> usize {
        self.visible_len
    }

    /// Look up the visible item at the given visible index. O(log spans).
    pub fn get(&self, visible_ix: usize) -> Option<ThreeWayVisibleItem> {
        if visible_ix >= self.visible_len {
            return None;
        }
        let span_ix = self
            .spans
            .partition_point(|s| s.visible_start() + s.visible_len() <= visible_ix);
        let span = self.spans.get(span_ix)?;
        match *span {
            ThreeWayVisibleSpan::Lines {
                visible_start,
                source_line_start,
                len,
            } => {
                let offset = visible_ix.checked_sub(visible_start)?;
                if offset >= len {
                    return None;
                }
                Some(ThreeWayVisibleItem::Line(source_line_start + offset))
            }
            ThreeWayVisibleSpan::CollapsedResolvedBlock {
                visible_index,
                conflict_ix,
            } => {
                if visible_ix != visible_index {
                    return None;
                }
                Some(ThreeWayVisibleItem::CollapsedBlock(conflict_ix))
            }
            ThreeWayVisibleSpan::CollapsedContext {
                visible_index,
                source_line_start,
                len,
                fold_id,
            } => {
                if visible_ix != visible_index {
                    return None;
                }
                Some(ThreeWayVisibleItem::CollapsedContext {
                    source_line_start,
                    len,
                    fold_id,
                })
            }
        }
    }

    /// Find the visible index for the first line of a conflict range, or its
    /// collapsed entry. Returns `None` if the range is not visible.
    /// O(log spans).
    pub fn visible_index_for_conflict(
        &self,
        conflict_ranges: &[std::ops::Range<usize>],
        range_ix: usize,
    ) -> Option<usize> {
        let range = conflict_ranges.get(range_ix)?;
        for span in &self.spans {
            match *span {
                ThreeWayVisibleSpan::Lines {
                    visible_start,
                    source_line_start,
                    len,
                } => {
                    let source_end = source_line_start + len;
                    if range.start >= source_line_start && range.start < source_end {
                        return Some(visible_start + (range.start - source_line_start));
                    }
                }
                ThreeWayVisibleSpan::CollapsedResolvedBlock {
                    visible_index,
                    conflict_ix,
                } if conflict_ix == range_ix => {
                    return Some(visible_index);
                }
                _ => {}
            }
        }
        None
    }

    /// Access the underlying spans for direct iteration (avoids per-item O(log n) lookup).
    pub fn spans(&self) -> &[ThreeWayVisibleSpan] {
        &self.spans
    }

    /// Find the visible index showing the given source line. Lines hidden
    /// inside a collapsed context fold map to the fold's row.
    pub fn visible_index_for_source_line(&self, line: usize) -> Option<usize> {
        for span in &self.spans {
            match *span {
                ThreeWayVisibleSpan::Lines {
                    visible_start,
                    source_line_start,
                    len,
                } => {
                    if line >= source_line_start && line < source_line_start + len {
                        return Some(visible_start + (line - source_line_start));
                    }
                }
                ThreeWayVisibleSpan::CollapsedContext {
                    visible_index,
                    source_line_start,
                    len,
                    ..
                } => {
                    if line >= source_line_start && line < source_line_start + len {
                        return Some(visible_index);
                    }
                }
                ThreeWayVisibleSpan::CollapsedResolvedBlock { .. } => {}
            }
        }
        None
    }
}

/// One `resolved` flag per marker block, in display order — the state the
/// minimap and the visible projection classify conflicts with.
pub(in crate::view) fn resolved_conflict_flags_from_segments(
    segments: &[ConflictSegment],
) -> Vec<bool> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            ConflictSegment::Block(block) => Some(block.resolved),
            ConflictSegment::Text(_) => None,
        })
        .collect()
}

/// Build a span-based visible projection for three-way views.
///
/// All lines in every conflict block are included (no preview gaps).
/// Resolved blocks collapse to a single summary row when `hide_resolved` is true.
/// Context lines kept visible on each side of a conflict when collapsed
/// context mode is active (section 30).
pub(crate) const CONFLICT_COLLAPSED_CONTEXT_LINES: usize = 3;

/// Runs shorter than this stay expanded — a fold row would not be
/// meaningfully shorter than the lines it hides.
const MIN_CONTEXT_FOLD_LINES: usize = 2;

/// Lines revealed per click of a fold's reveal arrows (matches the diff
/// view's collapsed-hunk reveal step).
pub(crate) const CONFLICT_FOLD_REVEAL_STEP: usize = 20;

/// Per-fold partial-reveal state, keyed by the fold's stable identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct ConflictFoldReveal {
    /// Lines revealed from the top edge of the fold.
    pub top: usize,
    /// Lines revealed from the bottom edge of the fold.
    pub bottom: usize,
    /// The user expanded the whole fold.
    pub expand_all: bool,
}

/// Visibility options for the three-way projection.
#[derive(Clone, Copy, Default)]
pub(in crate::view) struct ThreeWayVisibleOptions<'a> {
    pub hide_resolved: bool,
    /// section 30 collapsed context mode: fold unchanged runs beyond
    /// [`CONFLICT_COLLAPSED_CONTEXT_LINES`] around each conflict.
    pub collapse_context: bool,
    /// Per-fold reveal state, keyed by fold identity (pre-reveal start line).
    pub context_fold_reveals: Option<&'a FxHashMap<usize, ConflictFoldReveal>>,
}

/// Build the three-way visible projection with hide-resolved and collapsed
/// context folding applied.
pub(in crate::view) fn build_three_way_visible_projection_with_options(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    conflict_resolved: &[bool],
    options: ThreeWayVisibleOptions<'_>,
) -> ThreeWayVisibleProjection {
    if !options.collapse_context || conflict_ranges.is_empty() {
        return build_three_way_visible_projection_with_resolved_flags(
            total_lines,
            conflict_ranges,
            conflict_resolved,
            options.hide_resolved,
        );
    }
    if total_lines == 0 {
        return ThreeWayVisibleProjection::default();
    }

    let fold_reveal = |fold_id: usize| {
        options
            .context_fold_reveals
            .and_then(|reveals| reveals.get(&fold_id).copied())
            .unwrap_or_default()
    };

    let mut spans: Vec<ThreeWayVisibleSpan> = Vec::new();
    let mut visible_ix = 0usize;
    let push_lines =
        |spans: &mut Vec<ThreeWayVisibleSpan>, visible_ix: &mut usize, start: usize, len: usize| {
            if len == 0 {
                return;
            }
            spans.push(ThreeWayVisibleSpan::Lines {
                visible_start: *visible_ix,
                source_line_start: start,
                len,
            });
            *visible_ix += len;
        };
    // Emit an unchanged gap, keeping `leading_keep` lines adjacent to the
    // previous conflict and `trailing_keep` lines before the next one;
    // anything beyond that folds unless the user expanded it.
    let push_gap = |spans: &mut Vec<ThreeWayVisibleSpan>,
                    visible_ix: &mut usize,
                    start: usize,
                    end: usize,
                    leading_keep: usize,
                    trailing_keep: usize| {
        let len = end.saturating_sub(start);
        if len == 0 {
            return;
        }
        let keep = leading_keep.saturating_add(trailing_keep);
        let fold_len = len.saturating_sub(keep);
        let fold_start = start + leading_keep;
        // The fold identity is its pre-reveal start line, so partial reveals
        // keep addressing the same fold.
        let fold_id = fold_start;
        let reveal = fold_reveal(fold_id);
        let revealed_top = reveal.top.min(fold_len);
        let revealed_bottom = reveal.bottom.min(fold_len.saturating_sub(revealed_top));
        let remaining = fold_len - revealed_top - revealed_bottom;
        if reveal.expand_all
            || fold_len < MIN_CONTEXT_FOLD_LINES
            || remaining < MIN_CONTEXT_FOLD_LINES
        {
            push_lines(spans, visible_ix, start, len);
            return;
        }
        push_lines(spans, visible_ix, start, leading_keep + revealed_top);
        spans.push(ThreeWayVisibleSpan::CollapsedContext {
            visible_index: *visible_ix,
            source_line_start: fold_start + revealed_top,
            len: remaining,
            fold_id,
        });
        *visible_ix += 1;
        push_lines(
            spans,
            visible_ix,
            fold_start + revealed_top + remaining,
            revealed_bottom + trailing_keep,
        );
    };

    let ctx = CONFLICT_COLLAPSED_CONTEXT_LINES;
    let mut line_ix = 0usize;
    for (range_ix, range) in conflict_ranges.iter().enumerate() {
        if line_ix >= total_lines {
            break;
        }
        let range_start = range.start.min(total_lines).max(line_ix);
        let range_end = range.end.min(total_lines).max(range_start);

        let leading_keep = if range_ix == 0 { 0 } else { ctx };
        push_gap(
            &mut spans,
            &mut visible_ix,
            line_ix,
            range_start,
            leading_keep,
            ctx,
        );

        let resolved = conflict_resolved.get(range_ix).copied().unwrap_or(false);
        if options.hide_resolved && resolved && range_start < range_end {
            spans.push(ThreeWayVisibleSpan::CollapsedResolvedBlock {
                visible_index: visible_ix,
                conflict_ix: range_ix,
            });
            visible_ix += 1;
        } else {
            push_lines(
                &mut spans,
                &mut visible_ix,
                range_start,
                range_end - range_start,
            );
        }
        line_ix = range_end;
    }
    if line_ix < total_lines {
        push_gap(&mut spans, &mut visible_ix, line_ix, total_lines, ctx, 0);
    }

    ThreeWayVisibleProjection {
        spans,
        visible_len: visible_ix,
    }
}

pub(in crate::view) fn build_three_way_visible_projection_with_resolved_flags(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    conflict_resolved: &[bool],
    hide_resolved: bool,
) -> ThreeWayVisibleProjection {
    if total_lines == 0 {
        return ThreeWayVisibleProjection::default();
    }

    if !hide_resolved {
        return ThreeWayVisibleProjection {
            spans: vec![ThreeWayVisibleSpan::Lines {
                visible_start: 0,
                source_line_start: 0,
                len: total_lines,
            }],
            visible_len: total_lines,
        };
    }

    let mut spans: Vec<ThreeWayVisibleSpan> =
        Vec::with_capacity(conflict_ranges.len().saturating_mul(2).saturating_add(1));
    let mut visible_ix = 0usize;
    for_each_three_way_visible_run(
        total_lines,
        conflict_ranges,
        conflict_resolved,
        true,
        |run| match run {
            ThreeWayVisibleRun::Lines { start, end } => {
                let len = end.saturating_sub(start);
                if len == 0 {
                    return;
                }
                spans.push(ThreeWayVisibleSpan::Lines {
                    visible_start: visible_ix,
                    source_line_start: start,
                    len,
                });
                visible_ix += len;
            }
            ThreeWayVisibleRun::Collapsed { conflict_ix } => {
                spans.push(ThreeWayVisibleSpan::CollapsedResolvedBlock {
                    visible_index: visible_ix,
                    conflict_ix,
                });
                visible_ix += 1;
            }
        },
    );

    ThreeWayVisibleProjection {
        spans,
        visible_len: visible_ix,
    }
}

#[cfg(any(test, feature = "benchmarks"))]
pub fn build_three_way_visible_projection(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    segments: &[ConflictSegment],
    hide_resolved: bool,
) -> ThreeWayVisibleProjection {
    let conflict_resolved = resolved_conflict_flags_from_segments(segments);
    build_three_way_visible_projection_with_resolved_flags(
        total_lines,
        conflict_ranges,
        &conflict_resolved,
        hide_resolved,
    )
}

/// Build the mapping from visible row indices to actual three-way data items.
///
/// When `hide_resolved` is false, every line maps directly.
/// When true, resolved conflict ranges are collapsed to a single summary row.
#[cfg(any(test, feature = "benchmarks"))]
pub(in crate::view) fn build_three_way_visible_map_with_resolved_flags(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    conflict_resolved: &[bool],
    hide_resolved: bool,
) -> Vec<ThreeWayVisibleItem> {
    if total_lines == 0 {
        return Vec::new();
    }

    if !hide_resolved {
        return (0..total_lines).map(ThreeWayVisibleItem::Line).collect();
    }

    let mut visible_len = 0usize;
    for_each_three_way_visible_run(
        total_lines,
        conflict_ranges,
        conflict_resolved,
        true,
        |run| match run {
            ThreeWayVisibleRun::Lines { start, end } => {
                visible_len += end.saturating_sub(start);
            }
            ThreeWayVisibleRun::Collapsed { .. } => {
                visible_len += 1;
            }
        },
    );

    let mut visible = Vec::with_capacity(visible_len);
    for_each_three_way_visible_run(
        total_lines,
        conflict_ranges,
        conflict_resolved,
        true,
        |run| match run {
            ThreeWayVisibleRun::Lines { start, end } => {
                for line_ix in start..end {
                    visible.push(ThreeWayVisibleItem::Line(line_ix));
                }
            }
            ThreeWayVisibleRun::Collapsed { conflict_ix } => {
                visible.push(ThreeWayVisibleItem::CollapsedBlock(conflict_ix));
            }
        },
    );
    visible
}

#[cfg(any(test, feature = "benchmarks"))]
pub fn build_three_way_visible_map(
    total_lines: usize,
    conflict_ranges: &[std::ops::Range<usize>],
    segments: &[ConflictSegment],
    hide_resolved: bool,
) -> Vec<ThreeWayVisibleItem> {
    let conflict_resolved = resolved_conflict_flags_from_segments(segments);
    build_three_way_visible_map_with_resolved_flags(
        total_lines,
        conflict_ranges,
        &conflict_resolved,
        hide_resolved,
    )
}

/// Find the visible index for the first line of a conflict range, or the
/// collapsed block entry. Returns `None` if the range is not visible.
#[cfg(test)]
pub fn visible_index_for_conflict(
    visible_map: &[ThreeWayVisibleItem],
    conflict_ranges: &[std::ops::Range<usize>],
    range_ix: usize,
) -> Option<usize> {
    let range = conflict_ranges.get(range_ix)?;
    visible_map.iter().position(|item| match item {
        ThreeWayVisibleItem::Line(ix) => range.contains(ix),
        ThreeWayVisibleItem::CollapsedBlock(ci) => *ci == range_ix,
        ThreeWayVisibleItem::CollapsedContext { .. } => false,
    })
}
