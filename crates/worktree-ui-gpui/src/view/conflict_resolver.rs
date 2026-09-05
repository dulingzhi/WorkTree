//! Conflict resolver: marker parsing, resolution and the resolver projections.
//!
//! One module per domain, under `conflict_resolver/`:
//!
//! - `text` — the `ConflictText`/`ConflictBlock`/`ConflictSegment` model and
//!   the shared line-scanning helpers
//! - `parsing` — marker-text parsing and ancestor-base population
//! - `resolution` — picks, autosolve summaries and session-resolution sync
//! - `output_projection` — the editable resolved output: block maps,
//!   fragments, spans and the lazy projection
//! - `provenance` — resolved-line source classification and the gutter row
//! - `block_diff` — block-local two-way diff rows and large-block previews
//! - `three_way_map` — aligned-row maps over base/ours/theirs
//! - `visibility` — visible projections, folding and rendering-mode selection
//! - `minimap` — the quantized minimap bands
//! - `navigation` — conflict navigation targets and anchors
//! - `split_row_index` — sparse row indexing and the split-view caches
//! - `three_way` — the base/ours/theirs column vocabulary
//! - `ui_state` — the resolver UI state machine and its caches
//! - `word_highlight` — word-level highlight computation and caches
//!
//! This file is the module root: it declares the domains, keeps the shared
//! view-mode vocabulary, and re-exports the surface the renderers and panes
//! consume, so `crate::view::conflict_resolver` keeps naming the same items it
//! always did.

mod block_diff;
mod minimap;
mod navigation;
mod output_projection;
mod parsing;
mod provenance;
mod resolution;
mod split_row_index;
mod text;
mod three_way;
mod three_way_map;
mod ui_state;
mod visibility;
mod word_highlight;

pub(in crate::view) use three_way::{ThreeWayColumn, ThreeWaySides};
pub(in crate::view) use ui_state::{
    ConflictModeState, ConflictResolverImagePreviewState, ConflictResolverJoinTarget,
    ConflictResolverMarkdownPreviewState, ConflictResolverUiState, ConflictRowSelection,
    ResolvedOutlineData, ResolvedOutputConflictMarker, ResolverPickTarget, StreamedConflictState,
};

pub(in crate::view) use split_row_index::ConflictSplitStyledTextCache;
use split_row_index::SparseLineIndex;
#[cfg(test)]
use split_row_index::{CONFLICT_SPLIT_PAGE_CACHE_MAX_PAGES, CONFLICT_SPLIT_PAGE_SIZE};
#[cfg(test)]
use split_row_index::{
    CONFLICT_SPLIT_STYLE_DENSE_ROWS, CONFLICT_SPLIT_STYLE_MAX_SPARSE_PAGES,
    CONFLICT_SPLIT_STYLE_PAGE_ROWS,
};
pub use split_row_index::{ConflictSplitRowIndex, TwoWaySplitProjection, TwoWaySplitVisibleRow};

pub(in crate::view) use word_highlight::ConflictSplitWordHighlightCache;
#[cfg(any(test, feature = "benchmarks"))]
pub use word_highlight::compute_three_way_word_highlights;
pub use word_highlight::compute_word_highlights_for_row;
pub use word_highlight::{
    TwoWayWordHighlightPair, WordHighlights, compute_aligned_three_way_word_highlights,
    compute_aligned_two_way_word_highlights,
};
#[cfg(feature = "benchmarks")]
pub use word_highlight::{TwoWayWordHighlights, compute_two_way_word_highlights};

#[cfg(any(test, feature = "benchmarks"))]
pub use text::ConflictInlineRow;
#[cfg(any(test, feature = "benchmarks"))]
use text::text_line_count;
pub use text::{ConflictBlock, ConflictSegment, ConflictText};
use text::{
    ConflictTextStorage, indexed_line_count, indexed_line_text, line_text_from_starts,
    scan_text_line_stats, text_line_count_usize,
};

use parsing::append_text_segment;
pub use parsing::{
    parse_conflict_markers, parse_conflict_markers_shared_nonempty,
    populate_block_bases_from_shared_ancestor, text_contains_conflict_markers,
};
#[cfg(test)]
pub use parsing::{parse_conflict_markers_shared, populate_block_bases_from_ancestor};

#[cfg(test)]
pub(in crate::view) use navigation::fresh_conflict_nav_target_index;
pub(in crate::view) use navigation::{
    ConflictNavAnchor, ConflictNavTarget, ConflictNavTargetFilter, ConflictNavTargetId,
    build_conflict_nav_targets, conflict_nav_region_aligned_ranges, next_conflict_nav_target_index,
    next_conflict_nav_target_index_or_sole_anchor, previous_conflict_nav_target_index,
    previous_conflict_nav_target_index_or_sole_anchor, reconcile_conflict_nav_target_index,
};
#[cfg(test)]
use navigation::{ConflictNavDirection, conflict_nav_direction_for_key};

#[cfg(test)]
pub use resolution::{
    AutosolveTraceMode, apply_choice_to_unresolved_segments, apply_session_region_resolutions,
    auto_resolve_segments, auto_resolve_segments_history,
    auto_resolve_segments_history_with_region_indices, auto_resolve_segments_pass2,
    auto_resolve_segments_pass2_with_region_indices, auto_resolve_segments_regex,
    format_autosolve_trace_summary, next_unresolved_conflict_index, prev_unresolved_conflict_index,
    unresolved_conflict_indices,
};
pub use resolution::{
    ConflictSummaryCounts, active_conflict_autosolve_trace_label,
    apply_plan_session_region_resolutions_with_index_map,
    apply_session_region_resolutions_with_index_map, conflict_count,
    conflict_ctrl_pick_choice_for_key, conflict_quick_pick_choice_for_key,
    conflict_session_summary_counts, conflict_stage_safety_check,
    derive_region_resolution_updates_from_output, derive_region_resolution_updates_from_segments,
    effective_conflict_counts, format_conflict_summary, format_open_summary_toast,
    on_open_autosolve_summary, resolved_conflict_count, sequential_conflict_region_indices,
};
pub(in crate::view) use resolution::{apply_ordered_region_resolutions, choice_for_selection};

pub(crate) use output_projection::ResolvedOutputBlockMap;
pub use output_projection::{
    ResolvedOutputProjection, ResolvedOutputText, bootstrap_resolved_output_text,
    generate_resolved_text, generate_resolved_text_with_options,
};
pub(in crate::view) use output_projection::{
    ResolvedOutputSource, UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER,
    line_is_unresolved_conflict_placeholder,
};

pub(in crate::view) use provenance::ResolvedOutputGutterRow;
pub use provenance::{
    ResolvedLineMeta, ResolvedLineSource, SourceLineKey,
    build_resolved_output_line_sources_index_from_text,
    compute_resolved_line_provenance_from_text_two_way_indexed_sources,
    compute_resolved_line_provenance_from_text_with_indexed_sources,
};
#[cfg(any(test, feature = "benchmarks"))]
pub use provenance::{
    SourceLines, compute_resolved_line_provenance, resolved_output_outline_line_count,
    split_output_lines_for_outline,
};
#[cfg(test)]
pub use provenance::{
    append_lines_to_output, build_resolved_output_line_sources_index, is_source_line_in_output,
};

#[cfg(test)]
use block_diff::block_local_two_way_diff_rows_with_context;
pub(crate) use block_diff::{BLOCK_LOCAL_DIFF_CONTEXT_LINES, LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES};
#[cfg(any(test, feature = "benchmarks"))]
pub(crate) use block_diff::{
    LARGE_CONFLICT_BLOCK_PREVIEW_LINES, LARGE_CONFLICT_BLOCK_WORD_HIGHLIGHT_MAX_LINES,
    block_local_two_way_diff_rows_with_stats,
};
#[cfg(any(test, feature = "benchmarks"))]
pub use block_diff::{block_local_two_way_diff_rows, build_inline_rows};
#[cfg(any(test, feature = "benchmarks"))]
use block_diff::{block_max_line_count, preview_line_starts};

#[cfg(any(test, feature = "benchmarks"))]
pub use three_way_map::build_three_way_conflict_maps;
pub(in crate::view) use three_way_map::merge_plan_aligned_conflict_ranges;
pub(in crate::view) use three_way_map::project_conflict_ranges_to_aligned_rows;
pub use three_way_map::{
    ThreeWayAlignedMap, ThreeWayConflictMaps, build_three_way_conflict_maps_without_line_maps,
    conflict_index_for_line,
};

pub(crate) use visibility::CONFLICT_FOLD_REVEAL_STEP;
#[cfg(test)]
pub(in crate::view) use visibility::build_three_way_visible_projection_with_resolved_flags;
pub(in crate::view) use visibility::{
    ConflictFoldReveal, ThreeWayVisibleOptions, build_three_way_visible_projection_with_options,
    resolved_conflict_flags_from_segments,
};
pub use visibility::{
    ThreeWayVisibleItem, ThreeWayVisibleProjection, ThreeWayVisibleSpan,
    select_conflict_rendering_mode, three_way_alignment_is_practical,
    two_way_alignment_is_practical,
};
#[cfg(any(test, feature = "benchmarks"))]
pub use visibility::{
    build_three_way_visible_map, build_three_way_visible_projection, build_two_way_visible_indices,
    map_two_way_rows_to_conflicts,
};
#[cfg(test)]
pub use visibility::{
    two_way_conflict_index_for_visible_row, unresolved_visible_nav_entries_for_two_way,
    visible_index_for_conflict, visible_index_for_two_way_conflict,
};

#[cfg(test)]
pub use minimap::MINIMAP_BAND_COUNT;
pub use minimap::{CONFLICT_BOTTOM_OVERSCROLL_ROWS, build_minimap_bands};

pub use worktree_core::conflict_output::ConflictOutputChoice as ConflictChoice;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConflictResolverViewMode {
    ThreeWay,
    TwoWayDiff,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConflictRenderingMode {
    EagerSmallFile,
    StreamedLargeFile,
}

impl ConflictRenderingMode {
    pub fn is_streamed_large_file(self) -> bool {
        matches!(self, Self::StreamedLargeFile)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub enum ConflictPickSide {
    Ours,
    Theirs,
}

// ── Tests ───────────────────────────────────────────────────────────────
#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests;
