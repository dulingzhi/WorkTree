//! Aligned-row maps over base/ours/theirs: per-side conflict ranges,
//! line-to-conflict maps and the kdiff3-style aligned row space.

use super::*;
use std::ops::Range;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThreeWayConflictMaps {
    /// Per-side conflict ranges indexed by [base, ours, theirs].
    pub conflict_ranges: [Vec<std::ops::Range<usize>>; 3],
    /// Per-side per-line conflict maps (populated only in eager mode).
    pub line_conflict_maps: [Vec<Option<usize>>; 3],
    pub conflict_has_base: Vec<bool>,
    pub conflict_resolved: Vec<bool>,
}

/// Project marker-region ranges into the shared aligned source-row space.
///
/// This is the legacy/current-only fallback used when a merge plan is not
/// available. Callers may retain the result before resolved regions are
/// materialized into plain text so every original region remains navigable.
pub(in crate::view) fn project_conflict_ranges_to_aligned_rows(
    segments: &[ConflictSegment],
    aligned: &ThreeWayAlignedMap,
    side_line_counts: [usize; 3],
) -> Vec<Range<usize>> {
    let maps = build_three_way_conflict_maps_without_line_maps(
        segments,
        side_line_counts[0],
        side_line_counts[1],
        side_line_counts[2],
    );
    let block_count = maps.conflict_ranges[1].len();
    let mut aligned_ranges: Vec<Range<usize>> = Vec::with_capacity(block_count);
    for block_ix in 0..block_count {
        let mut start = usize::MAX;
        let mut end = 0usize;
        for side in 0..3 {
            let side_range = &maps.conflict_ranges[side][block_ix];
            if side_range.is_empty() {
                continue;
            }
            let mapped = aligned.aligned_range_for_side_range(side, side_range.clone());
            start = start.min(mapped.start);
            end = end.max(mapped.end);
        }
        if start == usize::MAX {
            start = end;
        }
        if let Some(previous) = aligned_ranges.last() {
            start = start.max(previous.end);
            end = end.max(start);
        }
        aligned_ranges.push(start..end);
    }
    aligned_ranges
}

/// Resolve visible marker blocks back to their exact aligned merge-plan rows.
///
/// Marker text is an output projection. Text between unresolved blocks can
/// come from only one source, so advancing every source offset by that text's
/// line count can merge adjacent conflict highlights or move later highlights
/// past their real rows. Full text sessions retain the authoritative mapping
/// from marker regions to merge-plan blocks; use it whenever it is available.
pub(in crate::view) fn merge_plan_aligned_conflict_ranges(
    session: &worktree_core::conflict_session::ConflictSession,
    visible_region_indices: &[usize],
    visible_plan_block_indices: &[usize],
) -> Option<Vec<Range<usize>>> {
    let plan = session.merge_plan.as_ref()?;
    if !visible_plan_block_indices.is_empty() {
        return visible_plan_block_indices
            .iter()
            .map(|block_index| {
                plan.blocks
                    .get(*block_index)
                    .map(|block| block.rows.clone())
            })
            .collect();
    }
    visible_region_indices
        .iter()
        .map(|region_index| {
            let block_index = *session.region_plan_blocks.get(*region_index)?;
            plan.blocks.get(block_index).map(|block| block.rows.clone())
        })
        .collect()
}

/// Binary search on sorted, non-overlapping ranges to find which conflict a line belongs to.
///
/// Returns `Some(conflict_index)` if the line falls within a range, `None` otherwise.
/// Ranges must be sorted by start and non-overlapping for correct results.
pub fn conflict_index_for_line(ranges: &[std::ops::Range<usize>], line: usize) -> Option<usize> {
    ranges
        .binary_search_by(|range| {
            if line < range.start {
                std::cmp::Ordering::Greater
            } else if line >= range.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()
}

/// Build per-column line-to-conflict maps for three-way conflict rendering.
///
/// The returned `conflict_ranges` follow the legacy behavior and are expressed
/// in the ours-column line space. The line maps provide O(1) conflict lookup
/// for each column at render/navigation time.
fn build_three_way_conflict_maps_impl(
    segments: &[ConflictSegment],
    base_line_count: usize,
    ours_line_count: usize,
    theirs_line_count: usize,
    include_line_conflict_maps: bool,
) -> ThreeWayConflictMaps {
    if segments.is_empty() {
        return ThreeWayConflictMaps {
            conflict_ranges: Default::default(),
            line_conflict_maps: if include_line_conflict_maps {
                [
                    vec![None; base_line_count],
                    vec![None; ours_line_count],
                    vec![None; theirs_line_count],
                ]
            } else {
                Default::default()
            },
            conflict_has_base: Vec::new(),
            conflict_resolved: Vec::new(),
        };
    }

    let block_count = segments
        .iter()
        .filter(|segment| matches!(segment, ConflictSegment::Block(_)))
        .count();
    let mut maps = ThreeWayConflictMaps {
        conflict_ranges: [
            Vec::with_capacity(block_count),
            Vec::with_capacity(block_count),
            Vec::with_capacity(block_count),
        ],
        line_conflict_maps: if include_line_conflict_maps {
            [
                vec![None; base_line_count],
                vec![None; ours_line_count],
                vec![None; theirs_line_count],
            ]
        } else {
            Default::default()
        },
        conflict_has_base: Vec::with_capacity(block_count),
        conflict_resolved: Vec::with_capacity(block_count),
    };

    fn mark_range(map: &mut [Option<usize>], start: usize, end: usize, conflict_ix: usize) {
        if map.is_empty() {
            return;
        }
        let from = start.min(map.len());
        let to = end.min(map.len());
        for slot in &mut map[from..to] {
            *slot = Some(conflict_ix);
        }
    }

    let mut base_offset = 0usize;
    let mut ours_offset = 0usize;
    let mut theirs_offset = 0usize;
    let mut conflict_ix = 0usize;
    for segment in segments {
        match segment {
            ConflictSegment::Text(text) => {
                let line_count = text_line_count_usize(text);
                base_offset = base_offset.saturating_add(line_count);
                ours_offset = ours_offset.saturating_add(line_count);
                theirs_offset = theirs_offset.saturating_add(line_count);
            }
            ConflictSegment::Block(block) => {
                let base_count = text_line_count_usize(block.base.as_deref().unwrap_or_default());
                let ours_count = text_line_count_usize(&block.ours);
                let theirs_count = text_line_count_usize(&block.theirs);

                let base_end = base_offset.saturating_add(base_count);
                let ours_end = ours_offset.saturating_add(ours_count);
                let theirs_end = theirs_offset.saturating_add(theirs_count);

                maps.conflict_ranges[0].push(base_offset..base_end);
                maps.conflict_ranges[1].push(ours_offset..ours_end);
                maps.conflict_ranges[2].push(theirs_offset..theirs_end);
                maps.conflict_has_base.push(block.base.is_some());
                maps.conflict_resolved.push(block.resolved);

                mark_range(
                    &mut maps.line_conflict_maps[0],
                    base_offset,
                    base_end,
                    conflict_ix,
                );
                mark_range(
                    &mut maps.line_conflict_maps[1],
                    ours_offset,
                    ours_end,
                    conflict_ix,
                );
                mark_range(
                    &mut maps.line_conflict_maps[2],
                    theirs_offset,
                    theirs_end,
                    conflict_ix,
                );

                base_offset = base_end;
                ours_offset = ours_end;
                theirs_offset = theirs_end;
                conflict_ix = conflict_ix.saturating_add(1);
            }
        }
    }

    maps
}

#[cfg(any(test, feature = "benchmarks"))]
pub fn build_three_way_conflict_maps(
    segments: &[ConflictSegment],
    base_line_count: usize,
    ours_line_count: usize,
    theirs_line_count: usize,
) -> ThreeWayConflictMaps {
    build_three_way_conflict_maps_impl(
        segments,
        base_line_count,
        ours_line_count,
        theirs_line_count,
        true,
    )
}

/// Build compact three-way conflict metadata without eager per-line side maps.
pub fn build_three_way_conflict_maps_without_line_maps(
    segments: &[ConflictSegment],
    base_line_count: usize,
    ours_line_count: usize,
    theirs_line_count: usize,
) -> ThreeWayConflictMaps {
    build_three_way_conflict_maps_impl(
        segments,
        base_line_count,
        ours_line_count,
        theirs_line_count,
        false,
    )
}

/// kdiff3-style aligned row space over base/ours/theirs (section 30).
///
/// Maps between visual rows (shared by all columns) and per-side line
/// indices. Sides shorter than a run are padded: their rows map to `None`.
/// The default value is an unbounded identity map (row == line on every
/// side), which is also the fallback when alignment is unavailable
/// (missing/binary sides, giant files).
#[derive(Clone, Debug, Default)]
pub struct ThreeWayAlignedMap {
    pub(super) runs: Vec<AlignedMapRun>,
    aligned_len: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AlignedMapRun {
    pub(super) aligned_start: usize,
    pub(super) rows: usize,
    starts: [usize; 3],
    lens: [usize; 3],
    pub(super) kind: worktree_core::merge::AlignedRunKind,
}

impl ThreeWayAlignedMap {
    /// Build from the merge engine's alignment runs.
    pub fn from_alignment(alignment: &[worktree_core::merge::AlignedRun]) -> Self {
        let mut runs = Vec::with_capacity(alignment.len());
        let mut aligned_start = 0usize;
        for run in alignment {
            let rows = run.visual_rows();
            runs.push(AlignedMapRun {
                aligned_start,
                rows,
                starts: [run.base.start, run.ours.start, run.theirs.start],
                lens: [run.base.len(), run.ours.len(), run.theirs.len()],
                kind: run.kind,
            });
            aligned_start += rows;
        }
        Self {
            runs,
            aligned_len: aligned_start,
        }
    }

    /// The identity map behaves as if every side had `row == line`.
    pub fn is_identity(&self) -> bool {
        self.runs.is_empty()
    }

    /// Total aligned rows. Zero for the identity map (callers keep their own
    /// length in that case).
    pub fn aligned_len(&self) -> usize {
        self.aligned_len
    }

    fn run_for_row(&self, row: usize) -> Option<&AlignedMapRun> {
        if row >= self.aligned_len {
            return None;
        }
        let ix = self
            .runs
            .partition_point(|run| run.aligned_start + run.rows <= row);
        self.runs.get(ix)
    }

    /// The side line rendered at `row`, or `None` for padding rows (and rows
    /// past the aligned end).
    pub fn side_line_for_row(&self, side: usize, row: usize) -> Option<usize> {
        if self.is_identity() {
            return Some(row);
        }
        let run = self.run_for_row(row)?;
        let offset = row - run.aligned_start;
        (offset < run.lens[side]).then(|| run.starts[side] + offset)
    }

    /// The row at which a side line renders. Lines past the side's end clamp
    /// to the end of the aligned space.
    pub fn row_for_side_line(&self, side: usize, line: usize) -> usize {
        if self.is_identity() {
            return line;
        }
        let ix = self
            .runs
            .partition_point(|run| run.starts[side] + run.lens[side] <= line);
        match self.runs.get(ix) {
            Some(run) => run.aligned_start + line.saturating_sub(run.starts[side]),
            None => self.aligned_len,
        }
    }

    /// section 30 split: the side line index a split boundary at aligned `row` maps
    /// to — i.e. the first side line at or after `row` (padding rows round up
    /// to the next real line; rows past the aligned end clamp to the side
    /// length). Use with `row` and `row_end + 1` to bracket a selection.
    pub fn side_line_lower_bound(&self, side: usize, row: usize) -> usize {
        if self.is_identity() {
            return row;
        }
        match self.run_for_row(row) {
            Some(run) => {
                let offset = row - run.aligned_start;
                run.starts[side] + offset.min(run.lens[side])
            }
            None => self
                .runs
                .last()
                .map(|run| run.starts[side] + run.lens[side])
                .unwrap_or(0),
        }
    }

    /// Map a per-side line range to the aligned row range covering it.
    pub fn aligned_range_for_side_range(
        &self,
        side: usize,
        range: std::ops::Range<usize>,
    ) -> std::ops::Range<usize> {
        if self.is_identity() {
            return range;
        }
        if range.is_empty() {
            let boundary = self.row_for_side_line(side, range.start);
            return boundary..boundary;
        }
        let start = self.row_for_side_line(side, range.start);
        let end = self.row_for_side_line(side, range.end.saturating_sub(1)) + 1;
        start..end.max(start)
    }
}
