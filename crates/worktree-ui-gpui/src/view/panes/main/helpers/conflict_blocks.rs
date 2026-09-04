use super::super::*;
use super::resolved_output_text::{
    ResolvedOutputSourceRevision, ResolvedOutputUnresolvedSpans, count_newlines,
    slice_text_by_line_range, source_line_count,
};
use crate::kit::text_model::TextModelSnapshot;
use rustc_hash::FxHasher;

#[cfg(test)]
use rustc_hash::FxHashSet;

pub(in crate::view) fn resolved_output_conflict_block_ranges_in_text(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
) -> Option<Vec<Range<usize>>> {
    fn is_line_boundary(
        text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
        byte_ix: usize,
    ) -> bool {
        if byte_ix == 0 || byte_ix == text.len() {
            return true;
        }
        text.byte_at(byte_ix.saturating_sub(1))
            .is_some_and(|b| b == b'\n')
    }

    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    let mut line_offset = 0usize;
    for seg in marker_segments {
        match seg {
            conflict_resolver::ConflictSegment::Text(text) => {
                if !output_text.starts_with_at(cursor, text.as_str()) {
                    return None;
                }
                cursor = cursor.saturating_add(text.len());
                line_offset = line_offset.saturating_add(count_newlines(text));
            }
            conflict_resolver::ConflictSegment::Block(block) => {
                let expected = conflict_resolver::generate_resolved_text(&[
                    conflict_resolver::ConflictSegment::Block(block.clone()),
                ]);
                if !output_text.starts_with_at(cursor, &expected) {
                    return None;
                }
                let end = cursor.saturating_add(expected.len());
                if end < cursor
                    || !is_line_boundary(output_text, cursor)
                    || !is_line_boundary(output_text, end)
                {
                    return None;
                }
                let start_line = line_offset;
                let mut end_line = line_offset.saturating_add(count_newlines(&expected));
                // A block that ends the file without a trailing newline still
                // occupies its last line, which no newline accounts for. Only
                // that case needs the extra row: when the block *is* newline
                // terminated, the outline still keeps an empty row after the
                // final newline (`resolved_output_outline_line_count`), and
                // claiming it would put this block's `?` gutter and conflict
                // bracket on a row that belongs to no conflict.
                if end == output_text.len() && !expected.is_empty() && !expected.ends_with('\n') {
                    end_line = end_line.saturating_add(1);
                }
                ranges.push(start_line..end_line);
                line_offset = line_offset.saturating_add(count_newlines(&expected));
                cursor = end;
            }
        }
    }

    Some(ranges)
}

/// Line ranges for the displayed conflict blocks, tolerating manual edits.
///
/// The walk above only reports ranges while the buffer still reads back exactly
/// as the segments render, so one keystroke anywhere in the output drops every
/// marker at once — placeholders lose their conflict color, their bracket and
/// their chunk menu. `ResolvedOutputBlockMap` carries block byte ownership
/// through edits, so fall back to it and convert its ranges into line space.
pub(in crate::view) fn resolved_output_conflict_block_line_ranges(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> Option<Vec<Range<usize>>> {
    resolved_output_conflict_block_ranges_in_text(marker_segments, output_text).or_else(|| {
        conflict_block_line_ranges_from_block_map(marker_segments, output_text, block_map)
    })
}

fn conflict_block_line_ranges_from_block_map(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> Option<Vec<Range<usize>>> {
    if !block_map.is_valid_for(marker_segments, output_text) {
        return None;
    }

    let byte_ranges = block_map.ranges();
    let mut line_ranges = Vec::with_capacity(byte_ranges.len());
    // The map keeps its ranges sorted and disjoint, so one forward pass counts
    // every newline exactly once instead of rescanning the prefix per block.
    let mut cursor = 0usize;
    let mut line = 0usize;
    for range in byte_ranges {
        let start_line = line.saturating_add(output_text.count_newlines_in(cursor..range.start));
        let body_newlines = output_text.count_newlines_in(range.clone());
        let mut end_line = start_line.saturating_add(body_newlines);
        // A block that ends the file without a trailing newline still occupies
        // its last row, which no newline accounts for — matching the strict
        // walk. The carried `line` below must not include this adjustment: it
        // counts newlines actually seen, and the next block's start is measured
        // from those.
        let body_is_empty = range.start == range.end;
        let body_ends_with_newline = range
            .end
            .checked_sub(1)
            .and_then(|last| output_text.byte_at(last))
            .is_some_and(|byte| byte == b'\n');
        if range.end == output_text.len() && !body_is_empty && !body_ends_with_newline {
            end_line = end_line.saturating_add(1);
        }
        line_ranges.push(start_line..end_line);
        line = start_line.saturating_add(body_newlines);
        cursor = range.end;
    }

    Some(line_ranges)
}

pub(in crate::view) fn conflict_marker_ranges_for_block(
    block: &conflict_resolver::ConflictBlock,
    line_range: Range<usize>,
) -> Vec<Range<usize>> {
    if !block.resolved && block.choice.is_empty() {
        return vec![line_range];
    }

    let mut marker_ranges = Vec::new();
    if !block.resolved
        && let Some(relative_subranges) = unresolved_decision_ranges_for_block(block)
            .or_else(|| unresolved_subchunk_conflict_ranges_for_block(block))
    {
        for relative in relative_subranges {
            let start = line_range
                .start
                .saturating_add(relative.start)
                .min(line_range.end);
            let end = line_range
                .start
                .saturating_add(relative.end)
                .min(line_range.end);
            marker_ranges.push(start..end);
        }
    }
    if marker_ranges.is_empty() {
        marker_ranges.push(line_range);
    }
    marker_ranges
}

pub(in crate::view) fn write_conflict_markers_for_ranges(
    markers: &mut [Option<ResolvedOutputConflictMarker>],
    conflict_ix: usize,
    unresolved: bool,
    marker_ranges: &[Range<usize>],
) {
    let output_line_count = markers.len();
    if output_line_count == 0 {
        return;
    }

    for marker_range in marker_ranges {
        if marker_range.start < marker_range.end {
            let end = marker_range.end.min(output_line_count);
            for (line_ix, marker_slot) in markers
                .iter_mut()
                .enumerate()
                .take(end)
                .skip(marker_range.start)
            {
                *marker_slot = Some(ResolvedOutputConflictMarker {
                    conflict_ix,
                    range_start: marker_range.start,
                    range_end: marker_range.end,
                    is_start: line_ix == marker_range.start,
                    is_end: line_ix + 1 == marker_range.end,
                    unresolved,
                });
            }
            continue;
        }

        let anchor = marker_range.start.min(output_line_count.saturating_sub(1));
        markers[anchor] = Some(ResolvedOutputConflictMarker {
            conflict_ix,
            range_start: marker_range.start,
            range_end: marker_range.end,
            is_start: true,
            is_end: true,
            unresolved,
        });
    }
}

pub(in crate::view) fn output_line_range_for_conflict_block_in_text(
    segments: &[conflict_resolver::ConflictSegment],
    output_text: &str,
    conflict_ix: usize,
) -> Option<Range<usize>> {
    resolved_output_conflict_block_ranges_in_text(segments, output_text)
        .and_then(|ranges| ranges.get(conflict_ix).cloned())
}

pub(in crate::view) fn conflict_fragment_text_for_choice(
    base: &str,
    ours: &str,
    theirs: &str,
    choice: conflict_resolver::ConflictChoice,
) -> String {
    use worktree_core::conflict_output::ConflictOutputSource;

    let mut out = String::new();
    for source in choice.iter() {
        match source {
            ConflictOutputSource::Base => out.push_str(base),
            ConflictOutputSource::Ours => out.push_str(ours),
            ConflictOutputSource::Theirs => out.push_str(theirs),
        }
    }
    out
}

pub(in crate::view) fn unresolved_subchunk_conflict_ranges_for_block(
    block: &conflict_resolver::ConflictBlock,
) -> Option<Vec<Range<usize>>> {
    use worktree_core::conflict_session::Subchunk;

    let base = block.base.as_deref()?;
    let subchunks = worktree_core::conflict_session::split_conflict_into_subchunks(
        base,
        &block.ours,
        &block.theirs,
    )?;
    let mut ranges = Vec::new();
    let mut line_offset = 0usize;
    for subchunk in subchunks {
        let (fragment, is_conflict) = match subchunk {
            Subchunk::Resolved(text) => (text, false),
            Subchunk::Conflict { base, ours, theirs } => (
                conflict_fragment_text_for_choice(&base, &ours, &theirs, block.choice),
                true,
            ),
        };
        let start = line_offset;
        line_offset = line_offset.saturating_add(count_newlines(&fragment));
        if is_conflict {
            ranges.push(start..line_offset);
        }
    }
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

#[derive(Clone, Debug)]
pub(in crate::view) struct UnresolvedDecisionRegion {
    pub(super) row_range: Range<usize>,
    pub(super) selected_line_range: Range<usize>,
    pub(super) alternate_line_range: Range<usize>,
    pub(super) has_non_emitting_rows: bool,
}

pub(in crate::view) fn unresolved_decision_regions_for_block(
    block: &conflict_resolver::ConflictBlock,
) -> Option<Vec<UnresolvedDecisionRegion>> {
    let (left, right, choose_left) = match block.choice {
        conflict_resolver::ConflictChoice::Ours => (&block.ours, &block.theirs, true),
        conflict_resolver::ConflictChoice::Theirs => (&block.theirs, &block.ours, false),
        _ => return None,
    };
    let plan = worktree_core::file_diff::side_by_side_plan(left, right);
    if plan.row_count == 0 {
        return None;
    }
    let regions = worktree_core::file_diff::plan_row_region_anchors(&plan).region_anchors;
    if regions.is_empty() {
        return None;
    }
    let (old_prefix, new_prefix) = worktree_core::file_diff::plan_emitted_line_prefix_counts(&plan);
    let (selected_prefix, alternate_prefix) = if choose_left {
        (&old_prefix, &new_prefix)
    } else {
        (&new_prefix, &old_prefix)
    };

    let mut decision_regions: Vec<UnresolvedDecisionRegion> = Vec::with_capacity(regions.len());
    for region in regions {
        let row_start = region.row_start.min(plan.row_count);
        let row_end = region.row_end_exclusive.min(plan.row_count).max(row_start);
        let selected_line_range = selected_prefix[row_start]..selected_prefix[row_end];
        let alternate_line_range = alternate_prefix[row_start]..alternate_prefix[row_end];
        let emitted_rows = selected_line_range
            .end
            .saturating_sub(selected_line_range.start);
        let has_non_emitting_rows = emitted_rows < row_end.saturating_sub(row_start);

        if let Some(last) = decision_regions.last_mut()
            && last.selected_line_range == selected_line_range
        {
            last.row_range.end = row_end;
            last.alternate_line_range.end =
                last.alternate_line_range.end.max(alternate_line_range.end);
            last.has_non_emitting_rows |= has_non_emitting_rows;
            continue;
        }

        decision_regions.push(UnresolvedDecisionRegion {
            row_range: row_start..row_end,
            selected_line_range,
            alternate_line_range,
            has_non_emitting_rows,
        });
    }
    if decision_regions.is_empty() {
        return None;
    }

    // Merge nearby non-zero ranges into one logical decision chunk while
    // preserving insertion anchors as independent picks.
    const MERGE_GAP_LINES: usize = 1;
    let mut merged: Vec<UnresolvedDecisionRegion> = Vec::with_capacity(decision_regions.len());
    for next in decision_regions {
        if let Some(prev) = merged.last_mut() {
            let prev_zero = prev.selected_line_range.start == prev.selected_line_range.end;
            let next_zero = next.selected_line_range.start == next.selected_line_range.end;
            let can_merge = if prev_zero || next_zero {
                prev_zero
                    && next_zero
                    && next.selected_line_range.start
                        <= prev.selected_line_range.end.saturating_add(MERGE_GAP_LINES)
            } else {
                // Keep ranges with insertion/deletion-only rows separate so
                // structural additions (e.g. trailing inserted methods) don't
                // collapse into preceding modification chunks.
                !prev.has_non_emitting_rows
                    && !next.has_non_emitting_rows
                    && next.selected_line_range.start
                        <= prev.selected_line_range.end.saturating_add(MERGE_GAP_LINES)
            };
            if can_merge {
                prev.row_range.end = next.row_range.end;
                prev.selected_line_range.end = prev
                    .selected_line_range
                    .end
                    .max(next.selected_line_range.end);
                prev.alternate_line_range.end = prev
                    .alternate_line_range
                    .end
                    .max(next.alternate_line_range.end);
                prev.has_non_emitting_rows |= next.has_non_emitting_rows;
                continue;
            }
        }
        merged.push(next);
    }

    Some(merged)
}

pub(in crate::view) fn unresolved_decision_ranges_for_block(
    block: &conflict_resolver::ConflictBlock,
) -> Option<Vec<Range<usize>>> {
    unresolved_decision_regions_for_block(block).map(|regions| {
        regions
            .into_iter()
            .map(|region| region.selected_line_range)
            .collect()
    })
}

pub(in crate::view) fn build_resolved_output_conflict_markers(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    output_line_count: usize,
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> Vec<Option<ResolvedOutputConflictMarker>> {
    let Some(block_ranges) =
        resolved_output_conflict_block_line_ranges(marker_segments, output_text, block_map)
    else {
        return vec![None; output_line_count];
    };

    build_resolved_output_conflict_markers_from_ranges(
        marker_segments,
        block_ranges.as_slice(),
        output_line_count,
    )
}

pub(in crate::view) fn build_resolved_output_conflict_markers_from_ranges(
    marker_segments: &[conflict_resolver::ConflictSegment],
    block_ranges: &[Range<usize>],
    output_line_count: usize,
) -> Vec<Option<ResolvedOutputConflictMarker>> {
    let mut markers = vec![None; output_line_count];
    if output_line_count == 0 {
        return markers;
    }

    for (conflict_ix, (block, range)) in marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .zip(block_ranges.iter().cloned())
        .enumerate()
    {
        let marker_ranges = conflict_marker_ranges_for_block(block, range);
        write_conflict_markers_for_ranges(
            &mut markers,
            conflict_ix,
            !block.resolved,
            marker_ranges.as_slice(),
        );
    }

    markers
}

pub(in crate::view) fn build_resolved_output_conflict_markers_from_block_ranges(
    marker_segments: &[conflict_resolver::ConflictSegment],
    block_ranges: &[Range<usize>],
    output_line_count: usize,
) -> Vec<Option<ResolvedOutputConflictMarker>> {
    let mut markers = vec![None; output_line_count];
    if output_line_count == 0 {
        return markers;
    }

    for (conflict_ix, (block, range)) in marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .zip(block_ranges.iter().cloned())
        .enumerate()
    {
        write_conflict_markers_for_ranges(
            &mut markers,
            conflict_ix,
            !block.resolved,
            std::slice::from_ref(&range),
        );
    }

    markers
}

pub(in crate::view) fn push_conflict_text_segment(
    segments: &mut Vec<conflict_resolver::ConflictSegment>,
    text: impl Into<conflict_resolver::ConflictText>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(conflict_resolver::ConflictSegment::Text(prev)) = segments.last_mut() {
        prev.push_str(text.as_str());
        return;
    }
    segments.push(conflict_resolver::ConflictSegment::Text(text));
}

pub(in crate::view) fn resolved_output_markers_for_text(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> Vec<Option<ResolvedOutputConflictMarker>> {
    let output_line_count = output_text.row_count();
    build_resolved_output_conflict_markers(
        marker_segments,
        output_text,
        output_line_count,
        block_map,
    )
}

/// Byte ranges whose output rows are still unresolved, and the subset of them
/// owned by `active_conflict`. Derive these from the current segments instead of
/// the asynchronously refreshed outline so syntax styling never briefly wins
/// while outline metadata catches up.
/// Unresolved output rows and the conflict each belongs to.
pub(in crate::view) type UnresolvedRows = Arc<[(Range<usize>, usize)]>;

/// [`UnresolvedRows`] paired with the state they were scanned from.
pub(in crate::view) type CachedUnresolvedRows = (ResolvedOutputKey, UnresolvedRows);

/// What the unresolved rows actually depend on: the buffer *and* which blocks
/// are still open.
///
/// The revision alone is not enough. A pick can leave the output byte-identical
/// — choosing the side already displayed, or resolving a whitespace-only block —
/// so the buffer never bumps its revision while the answer changes. Keying on
/// the revision alone leaves the yellow wash painted on a block the user just
/// resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct ResolvedOutputKey {
    pub(in crate::view) revision: ResolvedOutputSourceRevision,
    pub(in crate::view) resolution: u64,
    pub(in crate::view) block_map: u64,
}

impl ResolvedOutputKey {
    pub(in crate::view) fn new(
        snapshot: &TextModelSnapshot,
        marker_segments: &[conflict_resolver::ConflictSegment],
        block_map: &conflict_resolver::ResolvedOutputBlockMap,
    ) -> Self {
        Self {
            revision: ResolvedOutputSourceRevision::from_snapshot(snapshot),
            resolution: resolution_fingerprint(marker_segments),
            block_map: block_map_fingerprint(block_map),
        }
    }
}

/// O(conflicts) digest of the block map's byte ranges.
///
/// The rows fall back to the map for block geometry whenever the strict walk
/// fails — which is exactly once the user has edited the buffer. The map can be
/// rebuilt or reset without the text revision or any block's resolution moving,
/// and rows computed against the old geometry then land on the wrong lines.
fn block_map_fingerprint(block_map: &conflict_resolver::ResolvedOutputBlockMap) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = FxHasher::default();
    let ranges = block_map.ranges();
    ranges.len().hash(&mut hasher);
    for range in ranges {
        range.start.hash(&mut hasher);
        range.end.hash(&mut hasher);
    }
    hasher.finish()
}

/// O(conflicts) digest of which blocks are resolved. Not a hash of the text —
/// the revision already covers that.
fn resolution_fingerprint(marker_segments: &[conflict_resolver::ConflictSegment]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = FxHasher::default();
    for segment in marker_segments {
        match segment {
            conflict_resolver::ConflictSegment::Block(block) => {
                block.resolved.hash(&mut hasher);
                block.choice.hash(&mut hasher);
            }
            conflict_resolver::ConflictSegment::Text(_) => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Every still-unresolved output row, tagged with the conflict it belongs to.
///
/// Depends only on the *text*. Selecting a different conflict does not change
/// it, which is what lets the caller cache it across navigation.
///
/// Takes the rope rather than a materialized document plus a line-start array:
/// the rows wanted are the marker rows of unresolved blocks, and the rope
/// answers "byte range of row N" in O(log n), so this never has to build an
/// index proportional to the document.
pub(in crate::view) fn resolved_output_unresolved_rows(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &crate::kit::rope::Rope,
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> UnresolvedRows {
    if !marker_segments.iter().any(|segment| {
        matches!(segment, conflict_resolver::ConflictSegment::Block(block) if !block.resolved)
    }) {
        return Arc::default();
    }
    let Some(block_ranges) =
        resolved_output_conflict_block_line_ranges(marker_segments, output_text, block_map)
    else {
        return Arc::default();
    };

    // Walk the unresolved blocks rather than building a marker entry for every
    // row and filtering it. The per-line array is proportional to the document;
    // this is proportional to the conflicts, which is what the caller actually
    // asked about.
    let mut rows = Vec::new();
    for (conflict_ix, (block, line_range)) in marker_segments
        .iter()
        .filter_map(|segment| match segment {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            conflict_resolver::ConflictSegment::Text(_) => None,
        })
        .zip(block_ranges.iter().cloned())
        .enumerate()
    {
        if block.resolved {
            continue;
        }
        for marker_range in conflict_marker_ranges_for_block(block, line_range) {
            for line_ix in marker_range.start..marker_range.end {
                let Ok(row) = u32::try_from(line_ix) else {
                    continue;
                };
                if row >= output_text.line_count() {
                    continue;
                }
                let mut range = output_text.line_range(row);
                while range.end > range.start
                    && conflict_resolver::ResolvedOutputSource::byte_at(output_text, range.end - 1)
                        == Some(b'\r')
                {
                    range.end -= 1;
                }
                if !range.is_empty() {
                    rows.push((range, conflict_ix));
                }
            }
        }
    }
    rows.sort_by_key(|(range, _)| (range.start, range.end));
    rows.into()
}

/// Split cached rows into "every unresolved row" and "the selected conflict's".
///
/// O(unresolved rows), so moving the wash between conflicts costs nothing that
/// scales with the document.
pub(in crate::view) fn resolved_output_unresolved_spans_for_active(
    rows: &[(Range<usize>, usize)],
    active_conflict: Option<usize>,
) -> ResolvedOutputUnresolvedSpans {
    let mut all = Vec::with_capacity(rows.len());
    let mut active = Vec::new();
    for (range, conflict_ix) in rows {
        if active_conflict == Some(*conflict_ix) {
            active.push(range.clone());
        }
        all.push(range.clone());
    }
    ResolvedOutputUnresolvedSpans {
        all: all.into(),
        active: active.into(),
    }
}

/// Scan and select in one call. Production splits the two so navigation can
/// reuse the scan; this stays for tests that only care about the result.
#[cfg(test)]
pub(in crate::view) fn resolved_output_unresolved_byte_ranges(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &str,
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
    active_conflict: Option<usize>,
) -> ResolvedOutputUnresolvedSpans {
    let rope = crate::kit::rope::Rope::from_str(output_text);
    let rows = resolved_output_unresolved_rows(marker_segments, &rope, block_map);
    resolved_output_unresolved_spans_for_active(rows.as_ref(), active_conflict)
}

/// Byte spans of the unresolved-conflict placeholder rows, terminator included.
///
/// A `<Merge Conflict>` row is a drawing of an open decision, not text the file
/// will ever contain, so the buffer refuses to edit these spans however the
/// rest of the output has been rewritten by hand. Rows are identified by their
/// own content, which keeps the protection standing even once the marker
/// segments no longer line up with the buffer.
pub(in crate::view) fn resolved_output_placeholder_protected_ranges(
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
) -> Arc<[Range<usize>]> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    output_text.for_each_row_with_terminator(&mut |range, line| {
        if conflict_resolver::line_is_unresolved_conflict_placeholder(line) {
            ranges.push(range);
        }
    });
    ranges.into()
}

/// The placeholder spans as tree-sitter should see them: the protected rows
/// minus their line terminator.
///
/// Keeping the `\n` real is deliberate. It guarantees the lines either side of a
/// masked row cannot lex as one token, and it keeps every row index — and so
/// every `Point` the incremental edit path computes — aligned with the text.
///
/// Derived from the same spans the buffer protects from editing, so the mask and
/// the protection can never drift apart.
pub(in crate::view) fn resolved_output_live_syntax_mask(
    protected_ranges: &[Range<usize>],
    output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
) -> Arc<[Range<usize>]> {
    if protected_ranges.is_empty() {
        return Arc::default();
    }
    let mut mask = Vec::with_capacity(protected_ranges.len());
    for range in protected_ranges {
        let mut end = range.end.min(output_text.len());
        if end > range.start && output_text.byte_at(end - 1) == Some(b'\n') {
            end -= 1;
        }
        if end > range.start && output_text.byte_at(end - 1) == Some(b'\r') {
            end -= 1;
        }
        if end > range.start {
            mask.push(range.start..end);
        }
    }
    mask.into()
}

pub(in crate::view) fn resolved_output_marker_for_line(
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &str,
    output_line_ix: usize,
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
) -> Option<ResolvedOutputConflictMarker> {
    resolved_output_markers_for_text(marker_segments, output_text, block_map)
        .get(output_line_ix)
        .copied()
        .flatten()
}

pub(in crate::view) fn first_output_marker_line_for_conflict(
    markers: &[Option<ResolvedOutputConflictMarker>],
    conflict_ix: usize,
) -> Option<usize> {
    markers.iter().enumerate().find_map(|(line_ix, marker)| {
        marker
            .as_ref()
            .and_then(|m| (m.conflict_ix == conflict_ix && m.is_start).then_some(line_ix))
    })
}

#[cfg(test)]
pub(in crate::view) fn conflict_marker_nav_entries_from_markers(
    markers: &[Option<ResolvedOutputConflictMarker>],
) -> Vec<usize> {
    let mut seen_conflicts = FxHashSet::default();
    markers
        .iter()
        .enumerate()
        .filter_map(|(line_ix, marker)| {
            marker.as_ref().and_then(|m| {
                (m.is_start && seen_conflicts.insert(m.conflict_ix)).then_some(line_ix)
            })
        })
        .collect()
}

pub(in crate::view) fn split_target_conflict_block_into_subchunks(
    marker_segments: &mut Vec<conflict_resolver::ConflictSegment>,
    conflict_region_indices: &mut Vec<usize>,
    target_conflict_ix: usize,
) -> bool {
    use worktree_core::conflict_session::{Subchunk, split_conflict_into_subchunks};

    let Some(target_block) = marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .nth(target_conflict_ix)
        .cloned()
    else {
        return false;
    };
    if target_block.resolved {
        return false;
    }

    enum SplitMode {
        Subchunks(Vec<Subchunk>),
        DecisionRanges {
            regions: Vec<UnresolvedDecisionRegion>,
            choice_is_ours: bool,
        },
    }
    let split_mode = if let Some(base) = target_block.base.as_deref() {
        split_conflict_into_subchunks(base, &target_block.ours, &target_block.theirs).and_then(
            |subchunks| {
                let split_conflict_count = subchunks
                    .iter()
                    .filter(|subchunk| matches!(subchunk, Subchunk::Conflict { .. }))
                    .count();
                (split_conflict_count > 1).then_some(SplitMode::Subchunks(subchunks))
            },
        )
    } else {
        None
    }
    .or_else(|| {
        let (analysis_block, choice_is_ours) =
            if target_block.choice == conflict_resolver::ConflictChoice::Ours {
                (target_block.clone(), true)
            } else if target_block.choice == conflict_resolver::ConflictChoice::Theirs {
                (target_block.clone(), false)
            } else if target_block.choice.is_empty() {
                let mut analysis_block = target_block.clone();
                analysis_block.choice = conflict_resolver::ConflictChoice::Ours;
                (analysis_block, true)
            } else {
                return None;
            };
        unresolved_decision_regions_for_block(&analysis_block).and_then(|regions| {
            (regions.len() > 1).then_some(SplitMode::DecisionRanges {
                regions,
                choice_is_ours,
            })
        })
    });
    let Some(split_mode) = split_mode else {
        return false;
    };

    let mut next_segments = Vec::with_capacity(marker_segments.len().saturating_add(4));
    let mut next_region_indices =
        Vec::with_capacity(conflict_region_indices.len().saturating_add(4));
    let mut seen_conflict_ix = 0usize;
    for seg in marker_segments.drain(..) {
        match seg {
            conflict_resolver::ConflictSegment::Block(block) => {
                let region_ix = conflict_region_indices
                    .get(seen_conflict_ix)
                    .copied()
                    .unwrap_or(seen_conflict_ix);
                if seen_conflict_ix == target_conflict_ix {
                    match &split_mode {
                        SplitMode::Subchunks(subchunks) => {
                            for subchunk in subchunks {
                                match subchunk {
                                    Subchunk::Resolved(text) => {
                                        push_conflict_text_segment(
                                            &mut next_segments,
                                            text.clone(),
                                        );
                                    }
                                    Subchunk::Conflict { base, ours, theirs } => {
                                        next_segments.push(
                                            conflict_resolver::ConflictSegment::Block(
                                                conflict_resolver::ConflictBlock {
                                                    base: Some(base.clone().into()),
                                                    ours: ours.clone().into(),
                                                    theirs: theirs.clone().into(),
                                                    choice: target_block.choice,
                                                    resolved: false,
                                                    // Subchunks of a whitespace-only
                                                    // block are whitespace-only too.
                                                    whitespace_only: target_block.whitespace_only,
                                                },
                                            ),
                                        );
                                        next_region_indices.push(region_ix);
                                    }
                                }
                            }
                        }
                        SplitMode::DecisionRanges {
                            regions,
                            choice_is_ours,
                        } => {
                            let (selected_text, alternate_text) = if *choice_is_ours {
                                (&target_block.ours, &target_block.theirs)
                            } else {
                                (&target_block.theirs, &target_block.ours)
                            };
                            let selected_total_lines = source_line_count(selected_text);
                            let mut selected_cursor = 0usize;
                            for region in regions {
                                let prefix = slice_text_by_line_range(
                                    selected_text,
                                    selected_cursor..region.selected_line_range.start,
                                );
                                push_conflict_text_segment(&mut next_segments, prefix);

                                let selected_fragment = slice_text_by_line_range(
                                    selected_text,
                                    region.selected_line_range.clone(),
                                );
                                let alternate_fragment = slice_text_by_line_range(
                                    alternate_text,
                                    region.alternate_line_range.clone(),
                                );
                                let (ours, theirs) = if *choice_is_ours {
                                    (selected_fragment, alternate_fragment)
                                } else {
                                    (alternate_fragment, selected_fragment)
                                };
                                next_segments.push(conflict_resolver::ConflictSegment::Block(
                                    conflict_resolver::ConflictBlock {
                                        base: None,
                                        ours: ours.into(),
                                        theirs: theirs.into(),
                                        choice: target_block.choice,
                                        resolved: false,
                                        whitespace_only: target_block.whitespace_only,
                                    },
                                ));
                                next_region_indices.push(region_ix);
                                selected_cursor = region.selected_line_range.end;
                            }
                            let suffix = slice_text_by_line_range(
                                selected_text,
                                selected_cursor..selected_total_lines,
                            );
                            push_conflict_text_segment(&mut next_segments, suffix);
                        }
                    }
                } else {
                    next_segments.push(conflict_resolver::ConflictSegment::Block(block));
                    next_region_indices.push(region_ix);
                }
                seen_conflict_ix = seen_conflict_ix.saturating_add(1);
            }
            conflict_resolver::ConflictSegment::Text(text) => {
                push_conflict_text_segment(&mut next_segments, text);
            }
        }
    }

    *marker_segments = next_segments;
    *conflict_region_indices = next_region_indices;
    true
}

pub(in crate::view) fn conflict_region_index_is_unique(
    conflict_region_indices: &[usize],
    region_ix: usize,
) -> bool {
    conflict_region_indices
        .iter()
        .filter(|&&ix| ix == region_ix)
        .take(2)
        .count()
        <= 1
}

pub(in crate::view) fn conflict_block_matches_group(
    block: &conflict_resolver::ConflictBlock,
    region_ix: usize,
    target_block: &conflict_resolver::ConflictBlock,
    target_region_ix: usize,
) -> bool {
    region_ix == target_region_ix
        && block.base == target_block.base
        && block.ours == target_block.ours
        && block.theirs == target_block.theirs
}

pub(in crate::view) fn conflict_group_member_indices_for_ix(
    marker_segments: &[conflict_resolver::ConflictSegment],
    conflict_region_indices: &[usize],
    conflict_ix: usize,
) -> Vec<usize> {
    let mut blocks: Vec<&conflict_resolver::ConflictBlock> = Vec::new();
    // True when a block has non-empty text between it and the previous block.
    let mut separated_before: Vec<bool> = Vec::new();
    let mut saw_text_since_prev_block = false;
    for seg in marker_segments {
        match seg {
            conflict_resolver::ConflictSegment::Text(text) => {
                if !text.is_empty() {
                    saw_text_since_prev_block = true;
                }
            }
            conflict_resolver::ConflictSegment::Block(block) => {
                separated_before.push(saw_text_since_prev_block);
                blocks.push(block);
                saw_text_since_prev_block = false;
            }
        }
    }
    let Some(target_block) = blocks.get(conflict_ix).copied() else {
        return Vec::new();
    };
    let target_region_ix = conflict_region_indices
        .get(conflict_ix)
        .copied()
        .unwrap_or(conflict_ix);

    let mut start = conflict_ix;
    while start > 0 {
        if separated_before[start] {
            break;
        }
        let prev_ix = start - 1;
        let prev_block = blocks[prev_ix];
        let prev_region_ix = conflict_region_indices
            .get(prev_ix)
            .copied()
            .unwrap_or(prev_ix);
        if conflict_block_matches_group(prev_block, prev_region_ix, target_block, target_region_ix)
        {
            start = prev_ix;
        } else {
            break;
        }
    }

    let mut end_exclusive = conflict_ix + 1;
    while end_exclusive < blocks.len() {
        let next_ix = end_exclusive;
        if separated_before[next_ix] {
            break;
        }
        let next_block = blocks[next_ix];
        let next_region_ix = conflict_region_indices
            .get(next_ix)
            .copied()
            .unwrap_or(next_ix);
        if conflict_block_matches_group(next_block, next_region_ix, target_block, target_region_ix)
        {
            end_exclusive += 1;
        } else {
            break;
        }
    }

    (start..end_exclusive).collect()
}

pub(in crate::view) fn conflict_group_selected_choices_for_ix(
    marker_segments: &[conflict_resolver::ConflictSegment],
    conflict_region_indices: &[usize],
    conflict_ix: usize,
) -> Vec<conflict_resolver::ConflictChoice> {
    let group_indices =
        conflict_group_member_indices_for_ix(marker_segments, conflict_region_indices, conflict_ix);
    if group_indices.is_empty() {
        return Vec::new();
    }
    let blocks: Vec<&conflict_resolver::ConflictBlock> = marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .collect();

    let mut has_base = false;
    let mut has_ours = false;
    let mut has_theirs = false;
    for ix in group_indices {
        let Some(block) = blocks.get(ix).copied() else {
            continue;
        };
        if !block.resolved {
            continue;
        }
        use worktree_core::conflict_output::ConflictOutputSource;
        has_base |= block.choice.contains(ConflictOutputSource::Base);
        has_ours |= block.choice.contains(ConflictOutputSource::Ours);
        has_theirs |= block.choice.contains(ConflictOutputSource::Theirs);
    }

    let mut selected = Vec::with_capacity(3);
    if has_base {
        selected.push(conflict_resolver::ConflictChoice::Base);
    }
    if has_ours {
        selected.push(conflict_resolver::ConflictChoice::Ours);
    }
    if has_theirs {
        selected.push(conflict_resolver::ConflictChoice::Theirs);
    }
    selected
}

pub(in crate::view) fn conflict_group_indices_for_choice(
    marker_segments: &[conflict_resolver::ConflictSegment],
    conflict_region_indices: &[usize],
    conflict_ix: usize,
    choice: conflict_resolver::ConflictChoice,
) -> Vec<usize> {
    let group_indices =
        conflict_group_member_indices_for_ix(marker_segments, conflict_region_indices, conflict_ix);
    if group_indices.is_empty() {
        return Vec::new();
    }
    let blocks: Vec<&conflict_resolver::ConflictBlock> = marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .collect();

    group_indices
        .into_iter()
        .filter(|&ix| {
            let Some(block) = blocks.get(ix).copied() else {
                return false;
            };
            if !block.resolved {
                return false;
            }
            match choice {
                conflict_resolver::ConflictChoice::Base => block
                    .choice
                    .contains(worktree_core::conflict_output::ConflictOutputSource::Base),
                conflict_resolver::ConflictChoice::Ours => block
                    .choice
                    .contains(worktree_core::conflict_output::ConflictOutputSource::Ours),
                conflict_resolver::ConflictChoice::Theirs => block
                    .choice
                    .contains(worktree_core::conflict_output::ConflictOutputSource::Theirs),
                conflict_resolver::ConflictChoice::Both => {
                    block.choice == conflict_resolver::ConflictChoice::Both
                }
                _ => block.choice == choice,
            }
        })
        .collect()
}

pub(in crate::view) fn should_remove_conflict_block_on_reset(
    marker_segments: &[conflict_resolver::ConflictSegment],
    conflict_region_indices: &[usize],
    conflict_ix: usize,
) -> bool {
    let group_indices =
        conflict_group_member_indices_for_ix(marker_segments, conflict_region_indices, conflict_ix);
    group_indices.len() > 1
}

pub(in crate::view) fn remove_conflict_block_at(
    marker_segments: &mut Vec<conflict_resolver::ConflictSegment>,
    conflict_region_indices: &mut Vec<usize>,
    conflict_ix: usize,
) -> bool {
    let mut next_segments = Vec::with_capacity(marker_segments.len());
    let mut seen_conflict_ix = 0usize;
    let mut removed = false;
    for seg in marker_segments.drain(..) {
        match seg {
            conflict_resolver::ConflictSegment::Block(block) => {
                if seen_conflict_ix == conflict_ix {
                    removed = true;
                } else {
                    next_segments.push(conflict_resolver::ConflictSegment::Block(block));
                }
                seen_conflict_ix = seen_conflict_ix.saturating_add(1);
            }
            conflict_resolver::ConflictSegment::Text(text) => {
                push_conflict_text_segment(&mut next_segments, text);
            }
        }
    }
    *marker_segments = next_segments;
    if removed && conflict_ix < conflict_region_indices.len() {
        conflict_region_indices.remove(conflict_ix);
    }
    removed
}

pub(in crate::view) fn reset_conflict_block_selection(
    marker_segments: &mut Vec<conflict_resolver::ConflictSegment>,
    conflict_region_indices: &mut Vec<usize>,
    conflict_ix: usize,
) -> bool {
    if should_remove_conflict_block_on_reset(marker_segments, conflict_region_indices, conflict_ix)
    {
        return remove_conflict_block_at(marker_segments, conflict_region_indices, conflict_ix);
    }

    let mut seen_conflict_ix = 0usize;
    for seg in marker_segments.iter_mut() {
        let conflict_resolver::ConflictSegment::Block(block) = seg else {
            continue;
        };
        if seen_conflict_ix == conflict_ix {
            if !block.resolved {
                return false;
            }
            block.resolved = false;
            // A genuinely unpicked block has no implicit source. The output
            // projection renders its dedicated merge-conflict placeholder.
            block.choice = conflict_resolver::ConflictChoice::empty();
            return true;
        }
        seen_conflict_ix = seen_conflict_ix.saturating_add(1);
    }
    false
}

pub(in crate::view) fn append_choice_after_conflict_block(
    marker_segments: &mut Vec<conflict_resolver::ConflictSegment>,
    conflict_region_indices: &mut Vec<usize>,
    conflict_ix: usize,
    choice: conflict_resolver::ConflictChoice,
) -> Option<usize> {
    let target_block = marker_segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block),
            _ => None,
        })
        .nth(conflict_ix)?
        .clone();
    let group_indices =
        conflict_group_member_indices_for_ix(marker_segments, conflict_region_indices, conflict_ix);
    let &group_end_ix = group_indices.last()?;
    let target_region_ix = conflict_region_indices
        .get(conflict_ix)
        .copied()
        .unwrap_or(conflict_ix);
    if !target_block.resolved {
        return None;
    }
    if matches!(choice, conflict_resolver::ConflictChoice::Base) && target_block.base.is_none() {
        return None;
    }
    if conflict_group_selected_choices_for_ix(marker_segments, conflict_region_indices, conflict_ix)
        .contains(&choice)
    {
        return None;
    }

    let mut next_segments = Vec::with_capacity(marker_segments.len().saturating_add(1));
    let mut next_region_indices =
        Vec::with_capacity(conflict_region_indices.len().saturating_add(1));
    let mut seen_conflict_ix = 0usize;
    let mut next_conflict_ix = 0usize;
    let mut inserted_conflict_ix = None;

    let push_appended = |next_segments: &mut Vec<conflict_resolver::ConflictSegment>,
                         next_region_indices: &mut Vec<usize>,
                         next_conflict_ix: &mut usize,
                         inserted_conflict_ix: &mut Option<usize>| {
        if inserted_conflict_ix.is_some() {
            return;
        }
        let mut appended = target_block.clone();
        appended.choice = choice;
        appended.resolved = true;
        next_segments.push(conflict_resolver::ConflictSegment::Block(appended));
        next_region_indices.push(target_region_ix);
        *inserted_conflict_ix = Some(*next_conflict_ix);
        *next_conflict_ix = next_conflict_ix.saturating_add(1);
    };

    for seg in marker_segments.drain(..) {
        if seen_conflict_ix == group_end_ix.saturating_add(1) {
            push_appended(
                &mut next_segments,
                &mut next_region_indices,
                &mut next_conflict_ix,
                &mut inserted_conflict_ix,
            );
        }
        match seg {
            conflict_resolver::ConflictSegment::Block(block) => {
                let region_ix = conflict_region_indices
                    .get(seen_conflict_ix)
                    .copied()
                    .unwrap_or(seen_conflict_ix);
                next_segments.push(conflict_resolver::ConflictSegment::Block(block));
                next_region_indices.push(region_ix);
                next_conflict_ix = next_conflict_ix.saturating_add(1);
                seen_conflict_ix = seen_conflict_ix.saturating_add(1);
            }
            conflict_resolver::ConflictSegment::Text(text) => {
                push_conflict_text_segment(&mut next_segments, text);
            }
        }
    }
    push_appended(
        &mut next_segments,
        &mut next_region_indices,
        &mut next_conflict_ix,
        &mut inserted_conflict_ix,
    );

    *marker_segments = next_segments;
    *conflict_region_indices = next_region_indices;
    inserted_conflict_ix
}

#[cfg(test)]
pub(in crate::view) fn apply_three_way_empty_base_provenance_hints(
    meta: &mut [conflict_resolver::ResolvedLineMeta],
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &str,
) {
    let generated = conflict_resolver::generate_resolved_text(marker_segments);
    if generated != output_text || meta.is_empty() {
        return;
    }

    let mut block_ix = 0usize;
    let mut a_line = 1u32;
    let mut b_line = 1u32;
    let mut c_line = 1u32;

    for seg in marker_segments {
        match seg {
            conflict_resolver::ConflictSegment::Text(text) => {
                let n = u32::try_from(source_line_count(text)).unwrap_or(0);
                a_line = a_line.saturating_add(n);
                b_line = b_line.saturating_add(n);
                c_line = c_line.saturating_add(n);
            }
            conflict_resolver::ConflictSegment::Block(block) => {
                let a_count =
                    u32::try_from(source_line_count(block.base.as_deref().unwrap_or_default()))
                        .unwrap_or(0);
                let b_count = u32::try_from(source_line_count(&block.ours)).unwrap_or(0);
                let c_count = u32::try_from(source_line_count(&block.theirs)).unwrap_or(0);

                let base_empty = block.base.as_ref().is_none_or(|s| s.is_empty());
                if base_empty
                    && let Some(range) = output_line_range_for_conflict_block_in_text(
                        marker_segments,
                        output_text,
                        block_ix,
                    )
                {
                    let mut output_offset = 0usize;
                    for source in block.choice.iter() {
                        let (source_count, resolved_source, input_line) = match source {
                            worktree_core::conflict_output::ConflictOutputSource::Base => {
                                (a_count, conflict_resolver::ResolvedLineSource::A, a_line)
                            }
                            worktree_core::conflict_output::ConflictOutputSource::Ours => {
                                (b_count, conflict_resolver::ResolvedLineSource::B, b_line)
                            }
                            worktree_core::conflict_output::ConflictOutputSource::Theirs => {
                                (c_count, conflict_resolver::ResolvedLineSource::C, c_line)
                            }
                        };
                        let remaining = range
                            .end
                            .saturating_sub(range.start.saturating_add(output_offset));
                        let take =
                            usize::min(remaining, usize::try_from(source_count).unwrap_or(0));
                        for off in 0..take {
                            if let Some(m) = meta.get_mut(range.start + output_offset + off)
                                && matches!(
                                    m.source,
                                    conflict_resolver::ResolvedLineSource::A
                                        | conflict_resolver::ResolvedLineSource::Manual
                                )
                            {
                                m.source = resolved_source;
                                m.input_line = Some(
                                    input_line.saturating_add(u32::try_from(off).unwrap_or(0)),
                                );
                            }
                        }
                        output_offset = output_offset.saturating_add(take);
                    }
                }

                a_line = a_line.saturating_add(a_count);
                b_line = b_line.saturating_add(b_count);
                c_line = c_line.saturating_add(c_count);
                block_ix = block_ix.saturating_add(1);
            }
        }
    }
}

pub(in crate::view) fn apply_conflict_choice_provenance_hints_for_ranges(
    meta: &mut [conflict_resolver::ResolvedLineMeta],
    marker_segments: &[conflict_resolver::ConflictSegment],
    block_ranges: &[Range<usize>],
    view_mode: ConflictResolverViewMode,
) {
    if meta.is_empty() {
        return;
    }

    let assign_range = |meta: &mut [conflict_resolver::ResolvedLineMeta],
                        range: Range<usize>,
                        source: conflict_resolver::ResolvedLineSource,
                        start_line: u32,
                        line_count: u32| {
        let len = range.end.saturating_sub(range.start);
        for off in 0..len {
            if let Some(m) = meta.get_mut(range.start + off) {
                m.source = source;
                let off_u32 = u32::try_from(off).unwrap_or(u32::MAX);
                m.input_line = (off_u32 < line_count).then_some(start_line.saturating_add(off_u32));
            }
        }
    };

    let assign_both_range = |meta: &mut [conflict_resolver::ResolvedLineMeta],
                             range: Range<usize>,
                             first_source: conflict_resolver::ResolvedLineSource,
                             first_start: u32,
                             first_count: u32,
                             second_source: conflict_resolver::ResolvedLineSource,
                             second_start: u32,
                             second_count: u32| {
        let len = range.end.saturating_sub(range.start);
        let first_count_usize = usize::try_from(first_count).unwrap_or(0);
        let first_take = len.min(first_count_usize);
        assign_range(
            meta,
            range.start..range.start.saturating_add(first_take),
            first_source,
            first_start,
            first_count,
        );
        assign_range(
            meta,
            range.start.saturating_add(first_take)..range.end,
            second_source,
            second_start,
            second_count,
        );
    };

    let mut block_ix = 0usize;
    let mut a_line = 1u32;
    let mut b_line = 1u32;
    let mut c_line = 1u32;

    for seg in marker_segments {
        match seg {
            conflict_resolver::ConflictSegment::Text(text) => {
                let n = u32::try_from(source_line_count(text)).unwrap_or(0);
                a_line = a_line.saturating_add(n);
                b_line = b_line.saturating_add(n);
                if view_mode == ConflictResolverViewMode::ThreeWay {
                    c_line = c_line.saturating_add(n);
                }
            }
            conflict_resolver::ConflictSegment::Block(block) => {
                let (a_count, b_count, c_count) = match view_mode {
                    ConflictResolverViewMode::ThreeWay => (
                        u32::try_from(source_line_count(block.base.as_deref().unwrap_or_default()))
                            .unwrap_or(0),
                        u32::try_from(source_line_count(&block.ours)).unwrap_or(0),
                        u32::try_from(source_line_count(&block.theirs)).unwrap_or(0),
                    ),
                    ConflictResolverViewMode::TwoWayDiff => (
                        u32::try_from(source_line_count(&block.ours)).unwrap_or(0),
                        u32::try_from(source_line_count(&block.theirs)).unwrap_or(0),
                        0,
                    ),
                };

                if let Some(range) = block_ranges.get(block_ix).cloned() {
                    if !block.resolved && block.choice.is_empty() {
                        assign_range(
                            meta,
                            range,
                            conflict_resolver::ResolvedLineSource::Manual,
                            0,
                            0,
                        );
                    } else {
                        match (view_mode, block.choice) {
                            (
                                ConflictResolverViewMode::ThreeWay,
                                conflict_resolver::ConflictChoice::Base,
                            ) => {
                                assign_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::A,
                                    a_line,
                                    a_count,
                                );
                            }
                            (
                                ConflictResolverViewMode::ThreeWay,
                                conflict_resolver::ConflictChoice::Ours,
                            ) => {
                                assign_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::B,
                                    b_line,
                                    b_count,
                                );
                            }
                            (
                                ConflictResolverViewMode::ThreeWay,
                                conflict_resolver::ConflictChoice::Theirs,
                            ) => {
                                assign_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::C,
                                    c_line,
                                    c_count,
                                );
                            }
                            (
                                ConflictResolverViewMode::ThreeWay,
                                conflict_resolver::ConflictChoice::Both,
                            ) => {
                                assign_both_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::B,
                                    b_line,
                                    b_count,
                                    conflict_resolver::ResolvedLineSource::C,
                                    c_line,
                                    c_count,
                                );
                            }
                            (
                                ConflictResolverViewMode::TwoWayDiff,
                                conflict_resolver::ConflictChoice::Theirs,
                            ) => {
                                assign_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::B,
                                    b_line,
                                    b_count,
                                );
                            }
                            (
                                ConflictResolverViewMode::TwoWayDiff,
                                conflict_resolver::ConflictChoice::Both,
                            ) => {
                                assign_both_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::A,
                                    a_line,
                                    a_count,
                                    conflict_resolver::ResolvedLineSource::B,
                                    b_line,
                                    b_count,
                                );
                            }
                            // In two-way mode, Base falls back to local-side semantics.
                            (
                                ConflictResolverViewMode::TwoWayDiff,
                                conflict_resolver::ConflictChoice::Base,
                            )
                            | (
                                ConflictResolverViewMode::TwoWayDiff,
                                conflict_resolver::ConflictChoice::Ours,
                            ) => {
                                assign_range(
                                    meta,
                                    range,
                                    conflict_resolver::ResolvedLineSource::A,
                                    a_line,
                                    a_count,
                                );
                            }
                            _ => {
                                // Arbitrary ordered combinations are rendered
                                // correctly; this compact hint table treats their
                                // mixed provenance as manual.
                            }
                        }
                    }
                }

                a_line = a_line.saturating_add(a_count);
                b_line = b_line.saturating_add(b_count);
                c_line = c_line.saturating_add(c_count);
                block_ix = block_ix.saturating_add(1);
            }
        }
    }
}

pub(in crate::view) fn apply_conflict_choice_provenance_hints(
    meta: &mut [conflict_resolver::ResolvedLineMeta],
    marker_segments: &[conflict_resolver::ConflictSegment],
    output_text: &str,
    view_mode: ConflictResolverViewMode,
) {
    let generated = conflict_resolver::generate_resolved_text(marker_segments);
    if generated != output_text {
        return;
    }

    let Some(block_ranges) =
        resolved_output_conflict_block_ranges_in_text(marker_segments, output_text)
    else {
        return;
    };

    apply_conflict_choice_provenance_hints_for_ranges(
        meta,
        marker_segments,
        block_ranges.as_slice(),
        view_mode,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_decision_regions_track_non_emitting_selected_rows() {
        let block = conflict_resolver::ConflictBlock {
            base: None,
            ours: "".into(),
            theirs: "added line\n".into(),
            choice: conflict_resolver::ConflictChoice::Ours,
            resolved: false,
            whitespace_only: false,
        };

        let regions =
            unresolved_decision_regions_for_block(&block).expect("expected one decision region");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].row_range, 0..1);
        assert_eq!(regions[0].selected_line_range, 0..0);
        assert_eq!(regions[0].alternate_line_range, 0..1);
        assert!(regions[0].has_non_emitting_rows);
    }
}
