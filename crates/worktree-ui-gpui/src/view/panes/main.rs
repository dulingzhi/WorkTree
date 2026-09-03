use super::super::path_display;
use super::super::perf::{self, ViewPerfSpan};
use super::super::*;
use std::sync::atomic::{AtomicI32, Ordering};

mod actions_impl;
mod binary_conflict;
mod conflict_actions;
pub(in crate::view) mod conflict_chrome;
mod conflict_resolver_render;
mod core_impl;
mod decision_conflict;
mod diff;
pub(in crate::view) mod diff_cache;
pub(in crate::view) mod diff_search;
mod diff_stage;
mod diff_text;
mod diff_view;
mod diff_view_helpers;
mod file_editor;
mod helpers;
mod interactive_rebase;
mod keep_delete_conflict;
mod lfs_pointer;
mod preview;
mod state;
mod status_nav;

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
// The helpers surface is re-exported by name: exactly the items with
// consumers outside the `panes/main/` tree (Task 6 pruned the full list;
// the named list replaces the old glob).
pub(in crate::view) use helpers::{
    CollapsedDiffExpansionKind, CollapsedDiffHunk, CollapsedDiffVisibleRow,
    DiffHorizontalScrollColumn, DiffWrapVisibleCacheKey, DiffWrapVisualRow, PreparedSyntaxViewMode,
    RESOLVED_OUTPUT_ROW_HEIGHT_PX, VersionedCachedDiffStyledText,
    diff_file_header_height_for_ui_scale, diff_hunk_header_height_for_ui_scale,
    diff_row_height_for_ui_scale, preview_source_text_and_line_starts_from_lines,
    resolved_output_active_conflict_background, versioned_query_cached_diff_styled_text_is_current,
};

// Test-only consumers reach these helpers through this re-export: this tree's
// own `tests` module and the in-tree lib modules' `#[cfg(test)]` code.
#[cfg(test)]
pub(in crate::view) use helpers::{
    ClearDiffSelectionAction, FocusedMergetoolOutput, ResolvedOutputSourceRevision,
    ResolvedOutputUnresolvedSpans, append_choice_after_conflict_block,
    apply_conflict_choice_provenance_hints, apply_focused_mergetool_output,
    apply_resolved_output_unresolved_highlights, apply_three_way_empty_base_provenance_hints,
    build_focused_mergetool_save_payload, build_line_starts, coalesce_resolved_output_edit_deltas,
    conflict_group_indices_for_choice, conflict_group_selected_choices_for_ix,
    conflict_region_index_is_unique, preview_source_text_from_lines,
    reset_conflict_block_selection,
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
