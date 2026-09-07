//! Shared helpers for the main pane, split into domain modules.
//!
//! The named re-exports below flatten the domains back into the `helpers`
//! path so existing `use super::helpers::{...}` consumers stay unchanged.
//! Names consumed only by `#[cfg(test)]` code sit in `#[cfg(test)]` re-export
//! blocks; names with no consumer were pruned rather than allowed.

mod conflict_blocks;
mod diff_metrics;
mod focused_mergetool;
mod resolved_output_text;
mod scroll_reveal;

pub(in crate::view) use conflict_blocks::{
    CachedUnresolvedRows, ResolvedOutputKey, UnresolvedRows, append_choice_after_conflict_block,
    apply_conflict_choice_provenance_hints, apply_conflict_choice_provenance_hints_for_ranges,
    build_resolved_output_conflict_markers,
    build_resolved_output_conflict_markers_from_block_ranges, conflict_group_indices_for_choice,
    conflict_group_member_indices_for_ix, conflict_group_selected_choices_for_ix,
    conflict_marker_ranges_for_block, conflict_region_index_is_unique,
    first_output_marker_line_for_conflict, output_line_range_for_conflict_block_in_text,
    reset_conflict_block_selection, resolved_output_conflict_block_line_ranges,
    resolved_output_conflict_block_ranges_in_text, resolved_output_live_syntax_mask,
    resolved_output_marker_for_line, resolved_output_markers_for_text,
    resolved_output_placeholder_protected_ranges, resolved_output_unresolved_rows,
    resolved_output_unresolved_spans_for_active, split_target_conflict_block_into_subchunks,
    write_conflict_markers_for_ranges,
};
#[cfg(test)]
pub(in crate::view) use conflict_blocks::{
    apply_three_way_empty_base_provenance_hints, conflict_marker_nav_entries_from_markers,
    resolved_output_unresolved_byte_ranges,
};

#[cfg(test)]
pub(in crate::view) use diff_metrics::parse_conflict_canvas_rows_env;
pub(in crate::view) use diff_metrics::{
    BlameTimeRangeCache, CollapsedDiffExpansionKind, CollapsedDiffHunk,
    CollapsedDiffProjectionIdentity, CollapsedDiffReveal, CollapsedDiffVisibleRow,
    DiffHorizontalScrollColumn, DiffHorizontalScrollState, DiffTextAutoscrollTarget,
    DiffWrapVisibleCacheKey, DiffWrapVisualRow, FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES,
    FileDiffSplitWordHighlights, FileDiffStyleCacheEpochs, ICommitEditorMode, IRebaseDragState,
    IRebaseViewState, PreparedSyntaxDocumentKey, PreparedSyntaxViewMode,
    VersionedCachedDiffStyledText, conflict_canvas_rows_enabled_from_env,
    historical_browse_content, versioned_cached_diff_styled_text_is_current,
    versioned_query_cached_diff_styled_text_is_current,
};

pub(in crate::view) use focused_mergetool::{
    ClearDiffSelectionAction, FocusedMergetoolOutput, apply_focused_mergetool_output,
    build_focused_mergetool_save_payload, clear_diff_selection_action,
    conflict_strategy_needs_full_side_payloads, focused_mergetool_save_exit_code,
};

pub(in crate::view) use resolved_output_text::{
    ResolvedOutlineDelta, ResolvedOutputSourceRevision, StashedResolvedOutlineState,
    append_line_insertion_text, build_line_starts, coalesce_resolved_output_edit_deltas,
    conflict_resolver_output_context_line, count_newlines, dirty_byte_range_to_line_range,
    indexed_line_byte_range, indexed_line_count, indexed_line_count_from_len,
    line_content_byte_range_for_index, line_start_offset_for_index, preview_line_flags_for_text,
    preview_line_flags_from_bools, preview_line_flags_from_source,
    preview_line_has_tabs_without_loading, preview_line_is_ascii_without_loading,
    preview_source_text_and_line_starts_from_lines,
    remap_resolved_output_conflict_block_ranges_for_delta, resolved_outline_delta_between_texts,
    resolved_outline_delta_for_snapshot_transition, resolved_output_active_conflict_background,
    resolved_output_heuristic_highlight_provider, resolved_output_heuristic_highlights_for_range,
    resolved_output_heuristic_provider_binding_key, resolved_output_live_highlight_provider,
    resolved_output_live_provider_binding_key, resolved_output_snapshot_is_modified,
    shifted_line_index, should_skip_resolved_outline_provenance, source_line_count,
    split_line_count, worktree_output_requires_protection,
};
#[cfg(test)]
pub(in crate::view) use resolved_output_text::{
    ResolvedOutputUnresolvedSpans, apply_resolved_output_unresolved_highlights,
    preview_source_text_from_lines, resolved_output_active_unresolved_highlight_style,
    resolved_output_unresolved_highlight_style,
};

pub(in crate::view) use scroll_reveal::{
    DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS, RESOLVED_OUTPUT_ROW_HEIGHT_PX,
    centered_reveal_scroll_y, diff_file_header_height_for_ui_scale,
    diff_hunk_header_height_for_ui_scale, diff_row_height_for_ui_scale, reveal_scroll_x,
};
