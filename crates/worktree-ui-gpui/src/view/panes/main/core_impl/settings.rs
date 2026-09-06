//! Settings setters: pane-local application plus, where marked, persistence
//! through the root view's UI settings store.
use super::*;

impl MainPaneView {
    pub(in crate::view) fn set_active_context_menu_invoker(
        &mut self,
        next: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.active_context_menu_invoker == next {
            return;
        }
        self.active_context_menu_invoker = next.clone();
        self.history_view.update(cx, |view, cx| {
            view.set_active_context_menu_invoker(next, cx)
        });
        cx.notify();
    }

    pub(in crate::view) fn set_date_time_format(
        &mut self,
        next: DateTimeFormat,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.date_time_format == next {
            return;
        }
        self.date_time_format = next;
        self.history_view
            .update(cx, |view, cx| view.set_date_time_format(next, cx));
        cx.notify();
    }

    pub(in crate::view) fn set_history_highlight_commit_chain(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view.update(cx, |view, cx| {
            view.set_history_highlight_commit_chain(enabled, cx)
        });
        cx.notify();
    }

    pub(in crate::view) fn history_highlight_commit_chain(&self, cx: &App) -> bool {
        self.history_view.read(cx).history_highlight_commit_chain
    }

    pub(in crate::view) fn set_history_relative_dates(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view
            .update(cx, |view, cx| view.set_history_relative_dates(enabled, cx));
        cx.notify();
    }

    pub(in crate::view) fn history_relative_dates(&self, cx: &App) -> bool {
        self.history_view.read(cx).history_relative_dates
    }

    pub(in crate::view) fn set_timezone(&mut self, next: Timezone, cx: &mut gpui::Context<Self>) {
        self.history_view
            .update(cx, |view, cx| view.set_timezone(next, cx));
        cx.notify();
    }

    pub(in crate::view) fn set_show_timezone(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view
            .update(cx, |view, cx| view.set_show_timezone(enabled, cx));
        cx.notify();
    }

    pub(in crate::view) fn set_diff_scroll_sync(
        &mut self,
        next: DiffScrollSync,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_scroll_sync == next {
            return;
        }

        self.diff_scroll_sync = next;
        self.sync_diff_split_scroll();
        self.sync_conflict_preview_scroll();
        cx.notify();
    }

    pub(in crate::view) fn set_diff_view_mode(
        &mut self,
        next: DiffViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_view == next {
            return;
        }

        self.diff_view = next;
        // Inline keys styled segments by `row_ix` while split keys them by
        // `row_ix * 2` / `row_ix * 2 + 1` (`file_diff_split_cache_key`) against
        // the same `split_left`/`split_right` epochs, so the two key spaces
        // alias. Clear on every mode change, not just the toolbar/hotkey ones.
        self.clear_diff_text_style_caches();
        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_preserving_current();
        }
        cx.notify();
    }

    pub(in crate::view) fn set_annotate_enabled(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.annotate_enabled == next {
            return;
        }

        self.annotate_enabled = next;
        // The annotation column changes the available text width, so word-wrap
        // column counts and wrapped-row projection must be recomputed.
        self.invalidate_diff_wrap_visible_cache();
        if next {
            // An explicit toggle on: retry a previously failed blame for the same
            // target (force = true). The per-frame Render path never forces.
            self.request_blame_for_current_target(true, cx);
        }
        cx.notify();
    }

    pub(in crate::view) fn set_diff_content_mode(
        &mut self,
        next: DiffContentMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_content_mode == next {
            return;
        }

        self.diff_content_mode = next;
        self.diff_selection_anchor = None;
        self.diff_selection_range = None;
        self.clear_diff_text_style_caches();
        self.clear_diff_text_query_overlay_cache();
        self.clear_conflict_diff_style_caches();
        self.clear_conflict_diff_query_overlay_caches();
        self.clear_worktree_preview_segments_cache();
        self.reset_collapsed_diff_projection(false);
        self.ensure_rendered_patch_diff_cache(cx);
        if self.current_main_diff_supports_diff_content_toggle() {
            self.ensure_file_diff_cache(cx);
        }
        if self.current_main_diff_wants_file_diff() {
            self.ensure_file_image_diff_cache(cx);
        }
        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_preserving_current();
        }
        cx.notify();
    }

    pub(in crate::view) fn set_diff_whitespace_mode(
        &mut self,
        next: DiffWhitespaceMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_whitespace_mode == next {
            return;
        }

        self.diff_whitespace_mode = next;
        self.diff_selection_anchor = None;
        self.diff_selection_range = None;
        self.rebuild_patch_visual_line_kinds_from_current_diff();
        self.diff_word_highlights.clear();
        self.diff_word_highlights_inflight = None;
        self.reset_file_diff_word_highlight_caches();
        self.clear_diff_text_style_caches();
        self.clear_diff_text_query_overlay_cache();
        self.clear_conflict_diff_style_caches();
        self.clear_conflict_diff_query_overlay_caches();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.clear_worktree_preview_segments_cache();
        self.reset_collapsed_diff_projection(false);
        self.diff_visible_cache_len = 0;
        self.diff_visible_cache_projection_rev = u64::MAX;
        self.diff_scrollbar_markers_cache.clear();
        if self.current_main_diff_supports_diff_content_toggle() {
            self.reset_file_diff_cache_data();
            self.ensure_file_diff_cache(cx);
        }
        if self.diff_search_active && !self.diff_search_query.is_empty() {
            self.diff_search_recompute_matches_preserving_current();
        }
        cx.notify();
    }

    pub(in crate::view) fn set_diff_reveal_whitespace_chars(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.reveal_whitespace_chars == next {
            return;
        }

        self.reveal_whitespace_chars = next;
        self.clear_diff_text_style_caches();
        self.clear_conflict_diff_style_caches();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.diff_wrap_visible_cache_key = None;
        self.diff_wrap_visible_rows.clear();
        cx.notify();
    }

    pub(in crate::view) fn set_diff_word_wrap(&mut self, next: bool, cx: &mut gpui::Context<Self>) {
        if self.diff_word_wrap == next {
            return;
        }

        self.diff_word_wrap = next;
        self.diff_wrap_visible_cache_key = None;
        self.diff_wrap_visible_rows.clear();
        self.reset_diff_horizontal_scroll_state();
        cx.notify();
    }

    pub(in crate::view) fn set_diff_show_line_numbers(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_show_line_numbers == next {
            return;
        }

        self.diff_show_line_numbers = next;
        self.diff_wrap_visible_cache_key = None;
        self.reset_diff_horizontal_scroll_state();
        cx.notify();
    }

    // Apply the mode inside the pane first, then sync the root preference
    // without re-entering `main_pane.update(...)`.
    pub(in crate::view) fn set_diff_content_mode_and_persist(
        &mut self,
        next: DiffContentMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_content_mode != next {
            self.set_diff_content_mode(next, cx);
        }
        let root_view = self.root_view.clone();
        let _ = root_view.update(cx, |root, cx| {
            root.sync_diff_content_mode_from_pane(next, cx);
        });
    }

    pub(in crate::view) fn set_diff_whitespace_mode_and_persist(
        &mut self,
        next: DiffWhitespaceMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_whitespace_mode != next {
            self.set_diff_whitespace_mode(next, cx);
        }
        let root_view = self.root_view.clone();
        let _ = root_view.update(cx, |root, cx| {
            root.sync_diff_whitespace_mode_from_pane(next, cx);
        });
    }

    pub(in crate::view) fn set_diff_reveal_whitespace_chars_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.reveal_whitespace_chars != next {
            self.set_diff_reveal_whitespace_chars(next, cx);
        }
        let root_view = self.root_view.clone();
        let _ = root_view.update(cx, |root, cx| {
            root.sync_diff_reveal_whitespace_chars_from_pane(next, cx);
        });
    }

    pub(in crate::view) fn set_diff_word_wrap_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap != next {
            self.set_diff_word_wrap(next, cx);
        }
        let root_view = self.root_view.clone();
        let _ = root_view.update(cx, |root, cx| {
            root.sync_diff_word_wrap_from_pane(next, cx);
        });
    }

    pub(in crate::view) fn set_diff_show_line_numbers_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_show_line_numbers != next {
            self.set_diff_show_line_numbers(next, cx);
        }
        let root_view = self.root_view.clone();
        let _ = root_view.update(cx, |root, cx| {
            root.sync_diff_show_line_numbers_from_pane(next, cx);
        });
    }

    pub(in crate::view) fn history_visible_column_preferences(
        &self,
        cx: &gpui::App,
    ) -> (bool, bool, bool, bool) {
        self.history_view
            .read(cx)
            .history_visible_column_preferences()
    }

    /// Persisted merge tool preferences: (auto-advance, collapse-unchanged
    /// default, output scroll sync, show line numbers). Read by the root view's
    /// UI settings persist.
    pub(in crate::view) fn mergetool_preferences(&self) -> (bool, bool, bool, bool) {
        (
            self.mergetool_auto_advance,
            self.mergetool_collapse_unchanged,
            self.mergetool_output_scroll_sync,
            self.mergetool_show_line_numbers,
        )
    }

    pub(in crate::view) fn schedule_ui_settings_persist(&mut self, cx: &mut gpui::Context<Self>) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.schedule_ui_settings_persist(cx);
        });
    }

    pub(in crate::view) fn set_mergetool_auto_advance_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.mergetool_auto_advance == next {
            return;
        }
        self.mergetool_auto_advance = next;
        self.schedule_ui_settings_persist(cx);
        cx.notify();
    }

    pub(in crate::view) fn set_mergetool_output_scroll_sync_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.mergetool_output_scroll_sync == next {
            return;
        }
        self.mergetool_output_scroll_sync = next;
        self.schedule_ui_settings_persist(cx);
        cx.notify();
    }

    pub(in crate::view) fn set_mergetool_view_three_way_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.mergetool_view_three_way == next {
            return;
        }
        self.mergetool_view_three_way = next;
        // Unlike the cog-menu setters this can run while the root view is
        // already being updated (view-mode toggles), so schedule the persist
        // after the current update flush.
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.schedule_ui_settings_persist(cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view) fn set_mergetool_show_line_numbers_and_persist(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.mergetool_show_line_numbers == next {
            return;
        }
        self.mergetool_show_line_numbers = next;
        self.schedule_ui_settings_persist(cx);
        cx.notify();
    }

    pub(in crate::view) fn history_tag_preferences(&self, cx: &gpui::App) -> (bool, bool) {
        self.history_view.read(cx).history_tag_preferences()
    }

    pub(in crate::view) fn set_history_column_preferences(
        &mut self,
        show_graph: bool,
        show_author: bool,
        show_date: bool,
        show_sha: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view.update(cx, |view, cx| {
            view.set_history_column_preferences(show_graph, show_author, show_date, show_sha, cx);
        });
        cx.notify();
    }

    pub(in crate::view) fn set_history_tag_preferences(
        &mut self,
        show_tags: bool,
        auto_fetch_tags_on_repo_activation: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view.update(cx, |view, cx| {
            view.set_history_tag_preferences(show_tags, auto_fetch_tags_on_repo_activation, cx);
        });
        cx.notify();
    }

    pub(in crate::view) fn reset_history_column_widths(&mut self, cx: &mut gpui::Context<Self>) {
        self.history_view.update(cx, |view, cx| {
            view.reset_history_column_widths();
            cx.notify();
        });
        cx.notify();
    }
}
