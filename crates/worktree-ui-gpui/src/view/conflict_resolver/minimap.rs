//! The minimap column: band quantization over the visible projection and
//! the aligned runs it hides.

use super::*;
use std::ops::Range;

/// Blank rows appended below the last line of the source diff lists so the
/// tail of the file can be scrolled up into a comfortable reading position.
pub const CONFLICT_BOTTOM_OVERSCROLL_ROWS: usize = 10;

/// Number of bands the minimap column is quantized into.
///
/// kdiff3 paints one band per line; bounding the band count keeps paint cost
/// independent of file size while staying far finer than any column height.
pub const MINIMAP_BAND_COUNT: usize = 2048;

/// Build the minimap column's bands for the current three-way projection.
///
/// The result is in *visible* row space so the painted column lines up with
/// what the panes actually show: rows hidden by hide-resolved or collapsed
/// context are folded into their summary row's band, exactly as they are in
/// the lists. Returns an empty vector when the map carries no classification
/// (the identity fallback used for unaligned/giant files), which callers treat
/// as "no minimap available".
///
/// `conflict_ranges` and `conflict_resolved` are the aligned conflict ranges
/// and their resolution state, in step: a conflict the user has settled is
/// repainted in the resolved color so the bands that stay red are the work
/// that is left.
///
/// `trailing_rows` are the blank overscroll rows the lists append below the
/// last line. They carry no changes but do take up scroll range, so the bands
/// have to cover them for the viewport frame to line up with the panes.
pub fn build_minimap_bands(
    aligned: &ThreeWayAlignedMap,
    projection: &ThreeWayVisibleProjection,
    conflict_ranges: &[Range<usize>],
    conflict_resolved: &[bool],
    trailing_rows: usize,
) -> Vec<worktree_core::merge::MinimapRowKind> {
    use worktree_core::merge::MinimapRowKind;

    if aligned.is_identity() || projection.len() == 0 {
        return Vec::new();
    }
    let visible_len = projection.len() + trailing_rows;

    let band_count = MINIMAP_BAND_COUNT.min(visible_len);
    let mut bands = vec![MinimapRowKind::Unchanged; band_count];
    let band_span = |visible: std::ops::Range<usize>| {
        let first = visible.start.min(visible_len - 1) * band_count / visible_len;
        let last = (visible.end - 1).min(visible_len - 1) * band_count / visible_len;
        first..=last.min(band_count - 1)
    };
    let mut paint = |visible: std::ops::Range<usize>, kind: MinimapRowKind| {
        if kind == MinimapRowKind::Unchanged || visible.is_empty() {
            return;
        }
        for band in &mut bands[band_span(visible)] {
            *band = band.merge(kind);
        }
    };

    // Walk the visible spans and, for each, the aligned runs it covers. A
    // collapsed span shows several aligned rows on one visible row, so every
    // run it hides merges into that row's band.
    let spans = projection.spans();
    let runs = &aligned.runs;
    let aligned_len = aligned.aligned_len();
    let span_source_start = |span: &ThreeWayVisibleSpan| match *span {
        ThreeWayVisibleSpan::Lines {
            source_line_start, ..
        }
        | ThreeWayVisibleSpan::CollapsedContext {
            source_line_start, ..
        } => Some(source_line_start),
        // A collapsed conflict block carries no source range of its own.
        ThreeWayVisibleSpan::CollapsedResolvedBlock { .. } => None,
    };

    let mut covered = 0usize;
    for (span_ix, span) in spans.iter().enumerate() {
        let (source, visible_start, collapsed) = match *span {
            ThreeWayVisibleSpan::Lines {
                visible_start,
                source_line_start,
                len,
            } => (
                source_line_start..source_line_start + len,
                visible_start,
                false,
            ),
            ThreeWayVisibleSpan::CollapsedContext {
                visible_index,
                source_line_start,
                len,
                ..
            } => (
                source_line_start..source_line_start + len,
                visible_index,
                true,
            ),
            ThreeWayVisibleSpan::CollapsedResolvedBlock { visible_index, .. } => {
                // The hidden rows are everything between the previous span and
                // the next one that names a source line.
                let next = spans[span_ix + 1..]
                    .iter()
                    .find_map(span_source_start)
                    .unwrap_or(aligned_len);
                (covered..next.max(covered), visible_index, true)
            }
        };
        covered = covered.max(source.end);
        if source.is_empty() {
            continue;
        }

        let mut run_ix = runs.partition_point(|run| run.aligned_start + run.rows <= source.start);
        while let Some(run) = runs
            .get(run_ix)
            .filter(|run| run.aligned_start < source.end)
        {
            run_ix += 1;
            let kind = worktree_core::merge::minimap_row_kind(run.kind);
            if kind == MinimapRowKind::Unchanged {
                continue;
            }
            if collapsed {
                paint(visible_start..visible_start + 1, kind);
                continue;
            }
            let start = run.aligned_start.max(source.start);
            let end = (run.aligned_start + run.rows).min(source.end);
            paint(
                visible_start + (start - source.start)..visible_start + (end - source.start),
                kind,
            );
        }
    }

    // Second pass: a conflict the user has settled recedes to the resolved
    // color. Only bands the first pass classified as an open conflict change,
    // so a one-sided change sharing a band keeps its own side's color.
    for (range_ix, range) in conflict_ranges.iter().enumerate() {
        if range.is_empty() || !conflict_resolved.get(range_ix).copied().unwrap_or(false) {
            continue;
        }
        // A block hidden behind hide-resolved has no visible rows of its own;
        // its summary row is the one to repaint.
        let visible = match (
            projection.visible_index_for_source_line(range.start),
            projection.visible_index_for_source_line(range.end - 1),
        ) {
            (Some(first), Some(last)) if last >= first => first..last + 1,
            _ => match projection.visible_index_for_conflict(conflict_ranges, range_ix) {
                Some(row) => row..row + 1,
                None => continue,
            },
        };
        for band in &mut bands[band_span(visible)] {
            if *band == MinimapRowKind::Conflict {
                *band = band.resolved();
            }
        }
    }

    bands
}
