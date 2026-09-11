//! `MainPaneView` navigation and the visible-row match predicates.

use super::conflict::diff_search_split_row_texts_match_query;
use super::inline_patch::inline_patch_diff_visible_ix_matches_query;
use super::row_text::{resolved_output_line_ix_matches_query, retain_refined_visible_matches};

use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::view::DiffTextRegion;
use crate::view::caches::MarkdownSearchSurface;
use crate::view::panes::main::helpers::centered_reveal_scroll_y;
use crate::view::panes::main::state::MainPaneView;
use crate::view::preview_kind::DiffViewMode;
use gpui::point;
use std::ops::Range;
use worktree_core::domain::DiffArea;
use worktree_core::domain::DiffTarget;
use worktree_core::domain::FileStatusKind;
use worktree_state::model::Loadable;
// @split-module: impl_navigate
impl MainPaneView {
    pub(in crate::view) fn active_conflict_target(
        &self,
    ) -> Option<(
        std::path::PathBuf,
        Option<worktree_core::domain::FileConflictKind>,
    )> {
        if self.is_inline_submodule_diff_active() {
            return None;
        }
        let repo = self.active_repo()?;
        let DiffTarget::WorkingTree { path, area } = repo.diff_state.diff_target.as_ref()? else {
            return None;
        };
        if *area != DiffArea::Unstaged {
            return None;
        }
        let conflict = repo
            .status_entry_for_path(DiffArea::Unstaged, path.as_path())
            .filter(|entry| entry.kind == FileStatusKind::Conflicted)?;

        Some((path.clone(), conflict.conflict))
    }

    pub(in crate::view) fn file_editor_search_current_range(&self) -> Option<Range<usize>> {
        let ix = self.diff_search_match_ix?;
        self.file_editor_search_matches.get(ix).cloned()
    }

    /// Drop the editor's match list and ask the next render to un-paint it.
    pub(in crate::view) fn file_editor_search_clear(&mut self) {
        if self.file_editor_search_matches.is_empty() {
            return;
        }
        self.file_editor_search_matches.clear();
        self.file_editor_search_rev = self.file_editor_search_rev.wrapping_add(1);
    }

    pub(super) fn diff_search_file_diff_split_visible_row_matches_query(
        &self,
        query: AsciiCaseInsensitiveNeedle<'_>,
        visible_ix: usize,
        expanded_tabs: &mut String,
    ) -> bool {
        if !self.is_file_diff_view_active() || self.diff_view != DiffViewMode::Split {
            return false;
        }
        if self.diff_word_wrap {
            let left = self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
            let right = self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
            return query.is_match(left.as_ref()) || query.is_match(right.as_ref());
        }
        let Some(mapped_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
            return false;
        };
        let Some(provider) = self.file_diff_row_provider.as_ref() else {
            return false;
        };
        let Some((left, right)) = provider.split_row_texts(mapped_ix) else {
            return false;
        };
        diff_search_split_row_texts_match_query(
            query,
            left.as_ref().map(|text| text.as_ref()),
            right.as_ref().map(|text| text.as_ref()),
            expanded_tabs,
        )
    }

    pub(super) fn diff_search_file_diff_inline_visible_row_matches_query(
        &self,
        query: AsciiCaseInsensitiveNeedle<'_>,
        visible_ix: usize,
    ) -> bool {
        if !self.is_file_diff_view_active() || self.diff_view != DiffViewMode::Inline {
            return false;
        }
        if self.diff_word_wrap {
            return query.is_match(
                self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline)
                    .as_ref(),
            );
        }
        let Some(mapped_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
            return false;
        };
        self.file_diff_inline_render_data(mapped_ix)
            .is_some_and(|row| query.is_match(row.text.as_ref()))
    }

    pub(super) fn diff_search_try_refine_inline_patch_matches(
        &self,
        query: AsciiCaseInsensitiveNeedle<'_>,
        previous_matches: &mut Vec<usize>,
    ) -> bool {
        if self.is_file_editor_active()
            || self.is_file_preview_active()
            || self.active_conflict_target().is_some()
            || self.diff_view != DiffViewMode::Inline
            || self.is_file_diff_view_active()
            || self.is_collapsed_diff_projection_active()
            || self.diff_word_wrap
        {
            return false;
        }

        let Some(diff) = self.rendered_patch_diff_loadable() else {
            return false;
        };
        let Loadable::Ready(diff) = diff else {
            return false;
        };
        let Some(index) = self.diff_search_inline_patch_trigram_index.as_ref() else {
            return false;
        };

        let diff_click_kinds = &self.diff_click_kinds;
        let diff_header_display_cache = &self.diff_header_display_cache;
        let diff_visible_inline_map = self.diff_visible_inline_map.as_ref();
        let diff_visible_indices = &self.diff_visible_indices;
        retain_refined_visible_matches(
            previous_matches,
            index.candidates(query.as_bytes()),
            |visible_ix| {
                inline_patch_diff_visible_ix_matches_query(
                    diff.as_ref(),
                    diff_click_kinds,
                    diff_header_display_cache,
                    diff_visible_inline_map,
                    diff_visible_indices,
                    query,
                    visible_ix,
                )
            },
        );
        true
    }

    pub(super) fn diff_search_try_refine_worktree_preview_matches(
        &self,
        query: AsciiCaseInsensitiveNeedle<'_>,
        previous_matches: &mut Vec<usize>,
    ) -> bool {
        if self.is_file_editor_active() || !self.is_file_preview_active() {
            return false;
        }
        let Some(index) = self.worktree_preview_search_trigram_index.as_ref() else {
            return false;
        };

        retain_refined_visible_matches(
            previous_matches,
            index.candidates(query.as_bytes()),
            |line_ix| {
                self.worktree_preview_line_raw_text(line_ix)
                    .is_some_and(|line| resolved_output_line_ix_matches_query(&line, query))
            },
        );
        true
    }

    pub(super) fn diff_search_visible_row_matches_query(
        &self,
        query: AsciiCaseInsensitiveNeedle<'_>,
        visible_ix: usize,
    ) -> bool {
        if self.is_file_editor_active() {
            return self
                .file_editor_search_source
                .as_ref()
                .filter(|snapshot| visible_ix < snapshot.line_count())
                .is_some_and(|snapshot| {
                    query.is_match(snapshot.slice(snapshot.line_range(visible_ix)).as_ref())
                });
        }

        if self.is_file_preview_active() {
            return self
                .worktree_preview_line_raw_text(visible_ix)
                .is_some_and(|line| resolved_output_line_ix_matches_query(&line, query));
        }

        match self.diff_view {
            DiffViewMode::Inline => {
                if self.diff_word_wrap {
                    return query.is_match(
                        self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline)
                            .as_ref(),
                    );
                }
                if self.is_file_diff_view_active() {
                    return self
                        .diff_search_file_diff_inline_visible_row_matches_query(query, visible_ix);
                }
                query.is_match(
                    self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline)
                        .as_ref(),
                )
            }
            DiffViewMode::Split => {
                if self.diff_word_wrap {
                    let left =
                        self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
                    let right =
                        self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
                    return query.is_match(left.as_ref()) || query.is_match(right.as_ref());
                }
                if self.is_file_diff_view_active() {
                    let mut expanded_tabs = String::new();
                    return self.diff_search_file_diff_split_visible_row_matches_query(
                        query,
                        visible_ix,
                        &mut expanded_tabs,
                    );
                }
                let left = self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
                let right = self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
                query.is_match(left.as_ref()) || query.is_match(right.as_ref())
            }
        }
    }

    pub(in crate::view) fn diff_search_current_match_row(&self) -> Option<usize> {
        if !self.diff_search_active || !self.diff_search_has_query() {
            return None;
        }
        self.diff_search_current_match_visible_ix()
    }

    pub(super) fn diff_search_current_match_visible_ix(&self) -> Option<usize> {
        let len = self.diff_search_matches.len();
        if len == 0 {
            return None;
        }
        self.diff_search_match_ix
            .map(|ix| self.diff_search_matches[ix.min(len - 1)])
    }

    pub(in super::super::super::super) fn diff_search_prev_match(&mut self) {
        if !self.diff_search_active {
            return;
        }

        self.diff_search_flush_pending_query_recompute();
        if self.diff_search_matches.is_empty() {
            self.diff_search_recompute_matches();
        }
        let len = self.diff_search_matches.len();
        if len == 0 {
            return;
        }

        let current = self
            .diff_search_match_ix
            .unwrap_or(0)
            .min(len.saturating_sub(1));
        let next_ix = if current == 0 { len - 1 } else { current - 1 };
        self.diff_search_match_ix = Some(next_ix);
        let target = self.diff_search_matches[next_ix];
        self.diff_search_scroll_to_visible_ix(target);
    }

    pub(in super::super::super::super) fn diff_search_next_match(&mut self) {
        if !self.diff_search_active {
            return;
        }

        self.diff_search_flush_pending_query_recompute();
        if self.diff_search_matches.is_empty() {
            self.diff_search_recompute_matches();
        }
        let len = self.diff_search_matches.len();
        if len == 0 {
            return;
        }

        let current = self
            .diff_search_match_ix
            .unwrap_or(0)
            .min(len.saturating_sub(1));
        let next_ix = (current + 1) % len;
        self.diff_search_match_ix = Some(next_ix);
        let target = self.diff_search_matches[next_ix];
        self.diff_search_scroll_to_visible_ix(target);
    }

    pub(super) fn diff_search_scroll_to_visible_ix(&mut self, visible_ix: usize) {
        self.clear_diff_text_selection();
        self.diff_selection_range = None;
        // Only the hitbox-backed canvases below arm this; clearing it here
        // keeps a request from an earlier view alive into one that cannot serve
        // it.
        self.diff_search_horizontal_reveal = None;

        if self.is_file_editor_active() {
            self.file_editor_search_reveal_current(visible_ix);
            return;
        }

        if self.rendered_markdown_preview_owns_view() {
            match self.markdown_search_surface() {
                None => {}
                // No fixed row height and no `scroll_to_item` to hand this to,
                // so the renderer measures the row and scrolls during prepaint.
                Some(MarkdownSearchSurface::Worktree) => {
                    self.markdown_preview_reveal.request(visible_ix)
                }
                Some(MarkdownSearchSurface::DiffInline) => self
                    .diff_scroll
                    .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center),
                // Both sides share one visual row space, so one index moves both.
                Some(MarkdownSearchSurface::DiffSplit) => {
                    self.diff_scroll
                        .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
                    self.diff_split_right_scroll
                        .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
                }
                Some(MarkdownSearchSurface::Conflict) => {
                    self.conflict_markdown_preview_reveal(visible_ix)
                }
            }
            return;
        }

        if self.is_file_preview_active() {
            self.worktree_preview_scroll
                .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
            // Vertical here; the sideways half needs the row's painted geometry
            // and runs from the render pass once it has it.
            self.diff_search_horizontal_reveal = Some((
                visible_ix,
                super::helpers::DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS,
            ));
            return;
        }

        if let Some((_path, conflict_kind)) = self.active_conflict_target() {
            if Self::conflict_resolver_strategy(conflict_kind, false).is_some() {
                self.conflict_resolver_scroll_all_columns(visible_ix, gpui::ScrollStrategy::Center);
                // The columns are only half the merge tool. Without this the
                // resolved output stays wherever it was while the inputs jump.
                self.conflict_resolver_reveal_search_match_in_output(visible_ix);
                self.diff_search_horizontal_reveal = Some((
                    visible_ix,
                    super::helpers::DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS,
                ));
            } else {
                self.diff_scroll
                    .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
            }
            return;
        }

        self.diff_scroll
            .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
        self.diff_search_horizontal_reveal = Some((
            visible_ix,
            super::helpers::DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS,
        ));
        self.diff_selection_anchor = Some(visible_ix);
        self.diff_selection_range = Some((visible_ix, visible_ix));
    }

    /// Bring the current match into view in the editor.
    ///
    /// Runs without a `cx`, so it does only the scroll and leaves the selection
    /// to `render_file_editor` via the rev bumped here. The editor is a
    /// `TextInput`, not a `uniform_list`, so there is no deferred
    /// `scroll_to_item` to hand the work to and the offset is computed the way a
    /// list would — as `place_conflict_resolved_output_editor_at_row` does for
    /// the conflict resolver's editable output.
    fn file_editor_search_reveal_current(&mut self, line_ix: usize) {
        // A wrapped line owns several rows; land on the first of them and let the
        // input's own caret autoscroll close the gap once the selection lands.
        let visual_row = self
            .file_editor_wrap_row_starts
            .get(line_ix)
            .copied()
            .unwrap_or(line_ix);
        if let Some(y) = centered_reveal_scroll_y(
            visual_row,
            self.file_editor_gutter_row_height,
            self.file_editor_scroll.bounds().size.height,
            self.file_editor_scroll.max_offset().y,
            self.file_editor_scroll.offset().y,
        ) {
            let offset = self.file_editor_scroll.offset();
            self.file_editor_scroll.set_offset(point(offset.x, y));
        }
        // The wash moves too: the current match is the one hit the overlay leaves
        // out.
        self.file_editor_search_rev = self.file_editor_search_rev.wrapping_add(1);
        self.file_editor_search_reveal_rev = self.file_editor_search_reveal_rev.wrapping_add(1);
    }
}
