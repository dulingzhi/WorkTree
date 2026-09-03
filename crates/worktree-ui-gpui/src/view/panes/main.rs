use super::super::path_display;
use super::super::perf::{self, ViewPerfSpan};
use super::super::*;
use std::sync::atomic::{AtomicI32, Ordering};

mod actions_impl;
mod conflict_actions;
mod core_impl;
pub(in crate::view) mod diff_cache;
pub(in crate::view) mod diff_search;
mod diff_stage;
mod diff_text;
mod file_editor;
mod helpers;
mod interactive_rebase;
mod preview;
mod state;

#[cfg(feature = "benchmarks")]
#[allow(unused_imports)]
pub(in crate::view) use diff_search::{
    AsciiCaseInsensitiveNeedle, DiffSearchQueryReuse, diff_search_query_reuse,
};
// The editor's free functions are exercised directly by the panel tests; the
// pane itself reaches them through `impl MainPaneView`.
#[cfg(test)]
pub(in crate::view) use file_editor::*;
pub(crate) use state::MainPaneView;
// The helpers surface is re-exported by name: every item helpers.rs declares
// `pub(super)` or `pub(in crate::view)` is listed here (Task 6 prunes this to
// the tree-external consumers; the named list replaces the old glob).
#[allow(unused_imports)]
pub(in crate::view) use helpers::{
    BlameTimeRangeCache, CachedUnresolvedRows, ClearDiffSelectionAction,
    CollapsedDiffExpansionKind, CollapsedDiffHunk, CollapsedDiffProjectionIdentity,
    CollapsedDiffReveal, CollapsedDiffVisibleRow, DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS,
    DiffHorizontalScrollColumn, DiffHorizontalScrollState, DiffTextAutoscrollTarget,
    DiffWrapVisibleCacheKey, DiffWrapVisualRow, FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES,
    FileDiffSplitWordHighlights, FileDiffStyleCacheEpochs, FocusedMergetoolOutput,
    FocusedMergetoolSavePayload, ICommitEditorMode, IRebaseDragState, IRebaseViewState,
    LARGE_RESOLVED_OUTLINE_THREE_WAY_PROVENANCE_MAX_LINES,
    LARGE_RESOLVED_OUTLINE_TWO_WAY_PROVENANCE_MAX_LINES, PreparedSyntaxDocumentKey,
    PreparedSyntaxViewMode, RESOLVED_OUTPUT_ROW_HEIGHT_PX, ResolvedOutlineDelta, ResolvedOutputKey,
    ResolvedOutputSourceRevision, ResolvedOutputUnresolvedSpans, SEARCH_REVEAL_MARGIN_PX,
    StashedResolvedOutlineState, UnresolvedDecisionRegion, UnresolvedRows,
    VersionedCachedDiffStyledText, append_choice_after_conflict_block, append_line_insertion_text,
    apply_conflict_choice_provenance_hints, apply_conflict_choice_provenance_hints_for_ranges,
    apply_focused_mergetool_output, apply_resolved_output_unresolved_highlights,
    build_focused_mergetool_save_payload, build_line_starts, build_line_starts_with_count,
    build_resolved_output_conflict_markers,
    build_resolved_output_conflict_markers_from_block_ranges,
    build_resolved_output_conflict_markers_from_ranges, centered_reveal_scroll_y,
    clear_diff_selection_action, coalesce_resolved_output_edit_deltas,
    conflict_block_matches_group, conflict_canvas_rows_enabled_from_env,
    conflict_fragment_text_for_choice, conflict_group_indices_for_choice,
    conflict_group_member_indices_for_ix, conflict_group_selected_choices_for_ix,
    conflict_marker_ranges_for_block, conflict_region_index_is_unique,
    conflict_resolver_output_context_line, conflict_strategy_needs_full_side_payloads,
    count_newlines, diff_file_header_height_for_ui_scale, diff_hunk_header_height_for_ui_scale,
    diff_row_height_for_ui_scale, dirty_byte_range_to_line_range,
    first_output_marker_line_for_conflict, focused_mergetool_save_exit_code,
    historical_browse_content, indexed_line_byte_range, indexed_line_count,
    indexed_line_count_from_len, line_content_byte_range_for_index, line_index_for_offset,
    line_start_offset_for_index, output_line_range_for_conflict_block_in_text,
    parse_conflict_canvas_rows_env, preview_line_flags_for_text, preview_line_flags_from_bools,
    preview_line_flags_from_source, preview_line_has_tabs_without_loading,
    preview_line_is_ascii_without_loading, preview_source_text_and_line_starts_from_lines,
    push_conflict_text_segment, remap_resolved_output_conflict_block_ranges_for_delta,
    remove_conflict_block_at, reset_conflict_block_selection, resolved_outline_delta_between_texts,
    resolved_outline_delta_for_snapshot_transition, resolved_output_active_conflict_background,
    resolved_output_active_unresolved_highlight_style, resolved_output_conflict_block_line_ranges,
    resolved_output_conflict_block_ranges_in_text, resolved_output_heuristic_highlight_provider,
    resolved_output_heuristic_highlights_for_range, resolved_output_heuristic_provider_binding_key,
    resolved_output_live_highlight_provider, resolved_output_live_provider_binding_key,
    resolved_output_live_syntax_mask, resolved_output_marker_for_line,
    resolved_output_markers_for_text, resolved_output_placeholder_protected_ranges,
    resolved_output_snapshot_is_modified, resolved_output_unresolved_highlight_style,
    resolved_output_unresolved_rows, resolved_output_unresolved_spans_for_active, reveal_scroll_x,
    shifted_line_index, should_remove_conflict_block_on_reset,
    should_skip_resolved_outline_provenance, slice_text_by_line_range, source_line_count,
    split_line_count, split_target_conflict_block_into_subchunks,
    unresolved_decision_ranges_for_block, unresolved_decision_regions_for_block,
    unresolved_subchunk_conflict_ranges_for_block, versioned_cached_diff_styled_text_is_current,
    versioned_query_cached_diff_styled_text_is_current, worktree_output_requires_protection,
    write_conflict_markers_for_ranges,
};
#[cfg(test)]
pub(in crate::view) use helpers::{
    apply_three_way_empty_base_provenance_hints, conflict_marker_nav_entries_from_markers,
    preview_source_text_from_lines, resolved_output_unresolved_byte_ranges,
};

#[cfg(not(test))]
const CONFLICT_RESOLVED_OUTLINE_DEBOUNCE_MS: u64 = 140;
const FOCUSED_MERGETOOL_EXIT_SUCCESS: i32 = 0;
const FOCUSED_MERGETOOL_EXIT_CANCELED: i32 = 1;
const FOCUSED_MERGETOOL_EXIT_ERROR: i32 = 2;

#[inline]
pub(in crate::view) fn pane_non_main_width_for_layout(
    sidebar_w: Pixels,
    details_w: Pixels,
    _sidebar_collapsed: bool,
    _details_collapsed: bool,
) -> Pixels {
    // Resize handles overlay pane boundaries and therefore consume no layout width.
    sidebar_w + details_w
}

#[inline]
pub(in crate::view) fn pane_content_width_for_layout_from_non_main_width(
    total_w: Pixels,
    non_main_w: Pixels,
) -> Pixels {
    (total_w - non_main_w).max(px(0.0))
}

pub(in crate::view) fn pane_content_width_for_layout(
    total_w: Pixels,
    sidebar_w: Pixels,
    details_w: Pixels,
    sidebar_collapsed: bool,
    details_collapsed: bool,
) -> Pixels {
    pane_content_width_for_layout_from_non_main_width(
        total_w,
        pane_non_main_width_for_layout(sidebar_w, details_w, sidebar_collapsed, details_collapsed),
    )
}

impl Render for MainPaneView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        debug_assert!(matches!(
            self.view_mode,
            WorkTreeViewMode::Normal | WorkTreeViewMode::FocusedMergetool
        ));
        self.last_window_size = window.viewport_size();
        self.sync_root_layout_snapshot(cx);
        // The file explorer marks and pins files with unsaved buffers, and those
        // buffers live here rather than in the store, so nothing else can notice
        // them changing.
        self.sync_unsaved_file_edits_rev(cx);
        let history_content_width = self.main_pane_content_width(cx);
        self.history_view.update(cx, |v, _| {
            v.set_last_window_size(self.last_window_size);
            v.set_history_content_width(history_content_width);
        });

        let show_diff = self
            .active_repo()
            .and_then(|r| r.diff_state.diff_target.as_ref())
            .is_some();
        let in_rebase = self.active_repo().is_some_and(|r| {
            r.interactive_rebase_setup.is_some() || r.interactive_cherry_pick_setup.is_some()
        });
        // Keep blame in sync with the displayed file/revision while annotate is
        // on; the request is a no-op when the target is unchanged. Render must not
        // force a retry — a persistent error would re-dispatch every frame.
        if self.annotate_enabled && show_diff {
            self.request_blame_for_current_target(false, cx);
        }
        let inner = if show_diff {
            self.diff_view(window, cx).into_any_element()
        } else if in_rebase {
            self.interactive_rebase_view(window, cx).into_any_element()
        } else {
            self.history_view.clone().into_any_element()
        };
        // The historical-browse treatment lives inside `diff_view` now — as a
        // tint on the file header and the content surface, see
        // `historical_browse_content_active`.
        div().size_full().relative().child(inner)
    }
}

#[cfg(test)]
mod tests;
