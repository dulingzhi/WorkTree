//! `MainPaneView` query state and its debounced recompute scheduling.

use super::consts::DIFF_SEARCH_QUERY_DEBOUNCE_MS;

use crate::kit::text_search::DiffSearchMatcher;
use crate::kit::text_search::DiffSearchOptions;
use crate::view::panes::main::state::MainPaneView;
use gpui::SharedString;
use std::time::Duration;
// @split-module: impl_query
impl MainPaneView {
    pub(in crate::view) fn diff_search_options_or_default(&self) -> DiffSearchOptions {
        if self.diff_search_active {
            self.diff_search_options
        } else {
            DiffSearchOptions::default()
        }
    }

    pub(in crate::view) fn diff_search_has_query(&self) -> bool {
        self.diff_search_active && !self.diff_search_query.as_ref().is_empty()
    }

    pub(in crate::view::panes::main) fn diff_search_current_matcher(
        &mut self,
    ) -> DiffSearchMatcher {
        let matcher = DiffSearchMatcher::new(
            self.diff_search_query.as_ref(),
            self.diff_search_options_or_default(),
        );
        self.diff_search_regex_error = matcher.regex_error().map(|err| err.to_string().into());
        matcher
    }

    pub(in super::super::super::super) fn diff_search_cancel_pending_query_recompute(&mut self) {
        self.diff_search_debounce_seq = self.diff_search_debounce_seq.wrapping_add(1);
        self.diff_search_pending_previous_query = None;
    }

    pub(in crate::view::panes::main) fn diff_search_schedule_query_recompute(
        &mut self,
        previous_query: SharedString,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.diff_search_active {
            self.diff_search_cancel_pending_query_recompute();
            self.diff_search_matches.clear();
            self.diff_search_match_ix = None;
            return;
        }

        if self.diff_search_pending_previous_query.is_none() {
            self.diff_search_pending_previous_query = Some(previous_query);
        }
        self.diff_search_debounce_seq = self.diff_search_debounce_seq.wrapping_add(1);
        let seq = self.diff_search_debounce_seq;

        cx.spawn(
            async move |view: gpui::WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                cx.background_executor()
                    .timer(Duration::from_millis(DIFF_SEARCH_QUERY_DEBOUNCE_MS))
                    .await;
                let _ = view.update(cx, |this, cx| {
                    if this.diff_search_debounce_seq != seq {
                        return;
                    }
                    if this.diff_search_flush_pending_query_recompute() {
                        cx.notify();
                    }
                });
            },
        )
        .detach();
    }

    pub(in crate::view::panes::main) fn diff_search_flush_pending_query_recompute(
        &mut self,
    ) -> bool {
        let Some(previous_query) = self.diff_search_pending_previous_query.take() else {
            return false;
        };

        self.diff_search_debounce_seq = self.diff_search_debounce_seq.wrapping_add(1);
        self.diff_search_recompute_matches_for_query_change(previous_query.as_ref());
        true
    }

    pub(super) fn diff_search_can_refine_current_matches(&self) -> bool {
        // The editor's list is occurrence-indexed, so the row-wise refinement
        // below cannot narrow it — it always rescans.
        if self.is_file_editor_active() {
            return false;
        }
        // A rendered markdown preview holds indices into its own rendered rows;
        // every refiner below tests the *source* text at that index instead, so
        // narrowing would quietly drop the wrong rows.
        if self.rendered_markdown_preview_owns_view() {
            return false;
        }
        self.is_file_preview_active() || self.active_conflict_target().is_none()
    }
}
