//! `MainPaneView` match recomputation, in all its variants.

use super::row_text::DiffSearchFinalizeMode;

use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::kit::text_search::DiffSearchQueryReuse;
use crate::kit::text_search::diff_search_query_reuse;
use crate::view::panes::main::state::MainPaneView;
use crate::view::preview_kind::DiffViewMode;
// @split-module: impl_recompute
impl MainPaneView {
    pub(in super::super::super::super) fn diff_search_recompute_matches(&mut self) {
        self.diff_search_cancel_pending_query_recompute();
        if !self.diff_search_active {
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        if !self.is_file_preview_active() && self.active_conflict_target().is_none() {
            self.ensure_diff_visible_indices();
        }

        self.diff_search_recompute_matches_for_current_view();
    }

    pub(in super::super::super::super) fn diff_search_recompute_matches_and_scroll_to_first(
        &mut self,
    ) {
        self.diff_search_cancel_pending_query_recompute();
        self.diff_search_recompute_matches_with_finalize(DiffSearchFinalizeMode::ScrollToFirst);
    }

    fn diff_search_recompute_matches_with_finalize(&mut self, finalize: DiffSearchFinalizeMode) {
        if !self.diff_search_active {
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        if !self.is_file_preview_active() && self.active_conflict_target().is_none() {
            self.ensure_diff_visible_indices();
        }

        self.diff_search_recompute_matches_for_current_view_with_finalize(finalize);
    }

    pub(in super::super::super::super) fn diff_search_recompute_matches_preserving_current(
        &mut self,
    ) {
        self.diff_search_cancel_pending_query_recompute();
        if !self.diff_search_active {
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        if !self.is_file_preview_active() && self.active_conflict_target().is_none() {
            self.ensure_diff_visible_indices();
        }

        self.diff_search_recompute_matches_for_current_view_preserving_current();
    }

    pub(in crate::view::panes::main) fn diff_search_recompute_matches_for_query_change(
        &mut self,
        previous_query: &str,
    ) {
        if !self.diff_search_active {
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        self.diff_search_match_ix = None;
        let matcher = self.diff_search_current_matcher();

        if matcher.is_empty() || matcher.regex_error().is_some() {
            self.diff_search_matches.clear();
            self.diff_search_finalize_matches(DiffSearchFinalizeMode::ScrollToFirst);
            return;
        }

        // Wrapped diffs need source-row scanning so literals can cross soft-wrap boundaries.
        let can_refine = !self.diff_word_wrap
            && matcher.can_use_ascii_case_insensitive_fast_path()
            && self.diff_search_can_refine_current_matches();

        match diff_search_query_reuse(previous_query, matcher.query()) {
            DiffSearchQueryReuse::SameSemantics
                if matcher.can_use_ascii_case_insensitive_fast_path() => {}
            DiffSearchQueryReuse::Refinement if can_refine => {
                let Some(query) = AsciiCaseInsensitiveNeedle::new(matcher.query()) else {
                    self.diff_search_matches.clear();
                    self.diff_search_finalize_matches(DiffSearchFinalizeMode::ScrollToFirst);
                    return;
                };
                let mut previous_matches = std::mem::take(&mut self.diff_search_matches);
                if !(self
                    .diff_search_try_refine_worktree_preview_matches(query, &mut previous_matches)
                    || self
                        .diff_search_try_refine_inline_patch_matches(query, &mut previous_matches))
                {
                    if self.is_file_diff_view_active() && self.diff_view == DiffViewMode::Split {
                        let mut expanded_tabs = String::new();
                        previous_matches.retain(|&visible_ix| {
                            self.diff_search_file_diff_split_visible_row_matches_query(
                                query,
                                visible_ix,
                                &mut expanded_tabs,
                            )
                        });
                    } else {
                        previous_matches.retain(|&visible_ix| {
                            self.diff_search_visible_row_matches_query(query, visible_ix)
                        });
                    }
                }
                self.diff_search_matches = previous_matches;
            }
            DiffSearchQueryReuse::SameSemantics
            | DiffSearchQueryReuse::None
            | DiffSearchQueryReuse::Refinement => {
                self.diff_search_scan_current_view_with_matcher(&matcher);
            }
        }

        self.diff_search_finalize_matches(DiffSearchFinalizeMode::ScrollToFirst);
    }

    pub(in crate::view::panes::main) fn diff_search_recompute_matches_for_current_view(&mut self) {
        let previous_match_ix = self.diff_search_match_ix;
        let previous_visible_ix =
            previous_match_ix.and_then(|ix| self.diff_search_matches.get(ix).copied());
        self.diff_search_recompute_matches_for_current_view_with_finalize(
            DiffSearchFinalizeMode::preserve_current(previous_match_ix, previous_visible_ix),
        );
    }

    pub(in crate::view::panes::main) fn diff_search_recompute_matches_for_current_view_preserving_current(
        &mut self,
    ) {
        let previous_match_ix = self.diff_search_match_ix;
        let previous_visible_ix = self.diff_search_current_match_visible_ix();
        self.diff_search_recompute_matches_for_current_view_with_finalize(
            DiffSearchFinalizeMode::preserve_current(previous_match_ix, previous_visible_ix),
        );
    }

    fn diff_search_recompute_matches_for_current_view_with_finalize(
        &mut self,
        finalize: DiffSearchFinalizeMode,
    ) {
        let matcher = self.diff_search_current_matcher();

        if matcher.is_empty() || matcher.regex_error().is_some() {
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        self.diff_search_scan_current_view_with_matcher(&matcher);
        self.diff_search_finalize_matches(finalize);
    }
}
