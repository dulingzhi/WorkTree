//! `MainPaneView` cache invalidation for diff text and query overlays.

use crate::view::panes::main::state::MainPaneView;
use gpui::SharedString;
// @split-module: impl_caches
impl MainPaneView {
    pub(in crate::view) fn clear_diff_text_query_overlay_cache(&mut self) {
        self.diff_text_query_segments_cache.clear();
        self.diff_text_query_cache_query = SharedString::default();
        self.diff_text_query_cache_options = Default::default();
        self.diff_text_query_cache_matcher = None;
        self.diff_text_query_cache_generation =
            self.diff_text_query_cache_generation.wrapping_add(1);
    }

    pub(in crate::view) fn invalidate_diff_text_query_overlay_cache(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        if self.diff_text_query_cache_query.as_ref() != query
            || self.diff_text_query_cache_options != options
        {
            self.diff_text_query_cache_query = query.to_string().into();
            self.diff_text_query_cache_options = options;
            self.diff_text_query_cache_matcher = (!query.is_empty())
                .then(|| super::diff_search::DiffSearchMatcher::new(query, options));
            self.diff_text_query_cache_generation =
                self.diff_text_query_cache_generation.wrapping_add(1);
        }
    }

    pub(in crate::view) fn sync_diff_text_query_overlay_cache(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        self.invalidate_diff_text_query_overlay_cache(query, options);
    }

    pub(in crate::view) fn clear_diff_text_style_caches(&mut self) {
        self.diff_text_segments_cache.clear();
        self.clear_diff_text_query_overlay_cache();
    }

    pub(in crate::view) fn clear_worktree_preview_segments_cache(&mut self) {
        self.worktree_preview_segments_cache.clear();
        self.worktree_preview_cache_write_blocked_until_rev = None;
    }

    pub(in crate::view) fn clear_conflict_diff_query_overlay_caches(&mut self) {
        self.conflict_diff_query_segments_cache_split.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.conflict_diff_query_cache_query = SharedString::default();
        self.conflict_diff_query_cache_options = Default::default();
    }

    pub(in crate::view) fn clear_conflict_diff_style_caches_preserving_query(&mut self) {
        self.conflict_diff_segments_cache_split.clear();
        self.conflict_diff_query_segments_cache_split.clear();
        self.conflict_three_way_query_segments_cache.clear();
    }

    pub(in crate::view) fn sync_conflict_diff_query_overlay_caches(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        if self.conflict_diff_query_cache_query.as_ref() != query
            || self.conflict_diff_query_cache_options != options
        {
            self.conflict_diff_query_cache_query = query.to_string().into();
            self.conflict_diff_query_cache_options = options;
            self.conflict_diff_query_segments_cache_split.clear();
            self.conflict_three_way_query_segments_cache.clear();
        }
    }

    pub(in crate::view) fn clear_conflict_diff_style_caches(&mut self) {
        self.clear_conflict_diff_style_caches_preserving_query();
        self.conflict_diff_query_cache_query = SharedString::default();
        self.conflict_diff_query_cache_options = Default::default();
    }
}
