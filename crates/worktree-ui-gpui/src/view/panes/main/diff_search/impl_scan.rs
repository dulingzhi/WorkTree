//! `MainPaneView` scan backends for the file editor, the patch diff, the
//! worktree preview and the markdown preview.

use super::conflict::{
    ConflictResolverSearchContext, conflict_resolver_visible_match_indices_with_matcher,
    conflict_resolver_visible_match_indices_with_needle,
};
use super::consts::FILE_EDITOR_SEARCH_MAX_MATCHES;
use super::inline_patch::{
    collect_inline_patch_diff_visible_matches_with_needle, inline_patch_diff_search_text,
    inline_patch_diff_visible_ix_matches_query,
};
use super::row_text::{
    DiffSearchFinalizeMode, DiffSearchVisibleCandidates, diff_search_resume_match_ix,
    file_editor_search_ranges, resolved_output_line_ix_matches_query,
};
use super::stream::{
    collect_file_diff_line_text_stream_match_visible_rows, collect_split_stream_match_visible_rows,
    collect_stream_match_row_offsets, collect_stream_match_visible_rows,
};
use super::trigram::{
    DiffSearchVisibleTrigramIndex, diff_search_inline_patch_query_uses_trigram_index,
};

use crate::kit::text_expand::maybe_expand_tabs;
use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::kit::text_search::DiffSearchMatcher;
use crate::view::DiffTextRegion;
use crate::view::caches::MarkdownSearchSurface;
use crate::view::conflict_resolver::ThreeWayColumn;
use crate::view::panes::main::state::MainPaneView;
use crate::view::preview_kind::DiffViewMode;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::sync::Arc;
use worktree_state::model::Loadable;
// @split-module: impl_scan
impl MainPaneView {
    pub(super) fn diff_search_scan_current_view_with_matcher(
        &mut self,
        matcher: &DiffSearchMatcher,
    ) {
        // Ahead of everything else: the arms below dispatch on
        // `is_file_preview_active()`, which stays true in edit mode, and that is
        // what had the search counting the stale pre-edit preview text.
        if self.is_file_editor_active() {
            self.file_editor_search_scan(matcher);
            return;
        }

        // Ahead of the preview and conflict arms: a rendered markdown preview
        // is also a file preview / a conflict target, and those arms would scan
        // the markdown *source* under it rather than what is on screen.
        if self.rendered_markdown_preview_owns_view() {
            match self.markdown_search_surface() {
                Some(surface) => self.markdown_preview_search_scan(surface, matcher),
                None => self.diff_search_matches.clear(),
            }
            return;
        }

        // Wrapped diffs need source-row scanning so literals can cross soft-wrap boundaries.
        if !self.diff_word_wrap
            && matcher.can_use_ascii_case_insensitive_fast_path()
            && let Some(query) = AsciiCaseInsensitiveNeedle::new(matcher.query())
        {
            self.diff_search_scan_current_view_with_needle(query);
            return;
        }

        self.diff_search_scan_current_view_general(matcher);
    }

    /// Collect every occurrence in the editor buffer.
    ///
    /// Counted per *occurrence*, not per row as everywhere else: three hits on
    /// one line are three stops. `diff_search_matches` carries the line each one
    /// sits on, in the same order, so the shared match cursor and the `n/N` label
    /// keep working over this list unchanged.
    fn file_editor_search_scan(&mut self, matcher: &DiffSearchMatcher) {
        self.diff_search_matches.clear();
        self.file_editor_search_matches.clear();
        self.file_editor_search_rev = self.file_editor_search_rev.wrapping_add(1);

        let Some(snapshot) = self.file_editor_search_source.clone() else {
            return;
        };
        if matcher.is_empty() || matcher.regex_error().is_some() {
            return;
        }

        let found = file_editor_search_ranges(&snapshot, matcher, FILE_EDITOR_SEARCH_MAX_MATCHES);
        self.diff_search_matches.extend(
            found
                .iter()
                .map(|range| snapshot.row_for_offset(range.start)),
        );
        self.file_editor_search_matches = found;
    }

    /// Scroll the merge tool's rendered columns to a match.
    ///
    /// Unlike the text columns, which share one aligned row space, the three
    /// rendered documents are parsed independently from three different
    /// sources: row 300 of Base is not row 300 of Theirs. Scrolling all three to
    /// the same index would drag two of them to unrelated prose, so only the
    /// columns that actually hold the match at that row move.
    pub(super) fn conflict_markdown_preview_reveal(&mut self, visible_ix: usize) {
        let matcher = self.diff_search_current_matcher();
        if matcher.is_empty() || matcher.regex_error().is_some() {
            return;
        }
        for column in [
            ThreeWayColumn::Base,
            ThreeWayColumn::Ours,
            ThreeWayColumn::Theirs,
        ] {
            let matches = match self.conflict_resolver.markdown_preview.document(column) {
                Loadable::Ready(document) => document
                    .rows
                    .get(visible_ix)
                    .is_some_and(|row| matcher.is_match(row.text.as_ref())),
                _ => false,
            };
            if !matches {
                continue;
            }
            match column {
                ThreeWayColumn::Base => &self.conflict_resolver_diff_scroll,
                ThreeWayColumn::Ours => &self.conflict_preview_ours_scroll,
                ThreeWayColumn::Theirs => &self.conflict_preview_theirs_scroll,
            }
            .scroll_to_item_strict(visible_ix, gpui::ScrollStrategy::Center);
        }
    }

    /// Collect matches in a rendered markdown preview.
    ///
    /// Scans the text the preview *shows*, not the markdown behind it: Ctrl+F
    /// for `bold` finds a bolded word and does not match the `**` that made it
    /// bold. Wrapped lists report the first visual row of the matching source
    /// row, which is the row the reveal scrolls to.
    fn markdown_preview_search_scan(
        &mut self,
        surface: MarkdownSearchSurface,
        matcher: &DiffSearchMatcher,
    ) {
        // Collected before assigning: the documents are borrowed out of `self`.
        let mut matches = Vec::new();
        for (list, document) in self.markdown_search_documents(surface) {
            let plan = list.and_then(|list| self.markdown_preview_wrap_plan(list));
            matches.extend(
                document
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| matcher.is_match(row.text.as_ref()))
                    .map(|(row_ix, _)| plan.map_or(row_ix, |plan| plan.visual_ix_for_row(row_ix))),
            );
        }
        matches.sort_unstable();
        matches.dedup();
        self.diff_search_matches = matches;
    }

    fn diff_search_visual_ix_for_source_match(
        &self,
        source_visible_ix: usize,
        region: DiffTextRegion,
        offset: usize,
    ) -> usize {
        if !(self.diff_word_wrap && self.diff_wrap_visible_cache_key.is_some()) {
            return source_visible_ix;
        }

        let first_visible_ix = self
            .diff_wrap_visible_rows
            .partition_point(|row| row.source_visible_ix < source_visible_ix);
        let mut boundary_candidate = None;
        let mut fallback = None;
        for visible_ix in first_visible_ix..self.diff_wrap_visible_rows.len() {
            let Some(row) = self.diff_wrap_visible_rows.get(visible_ix) else {
                break;
            };
            if row.source_visible_ix != source_visible_ix {
                break;
            }
            fallback.get_or_insert(visible_ix);
            let (_, range) = self.diff_text_visual_source_range_for_region(visible_ix, region);
            if range.is_empty() {
                if offset == range.start {
                    return visible_ix;
                }
                continue;
            }
            if range.start <= offset && offset < range.end {
                return visible_ix;
            }
            if offset == range.end {
                boundary_candidate = Some(visible_ix);
            }
        }

        boundary_candidate.or(fallback).unwrap_or(source_visible_ix)
    }

    fn diff_search_collect_wrapped_source_matches(
        &self,
        source_len: usize,
        region: DiffTextRegion,
        matcher: &DiffSearchMatcher,
        out: &mut Vec<usize>,
    ) {
        let rows = (0..source_len).map(|source_visible_ix| {
            let text = self.diff_text_full_line_for_region(source_visible_ix, region);
            (source_visible_ix, text)
        });
        let mut row_offsets = Vec::new();
        collect_stream_match_row_offsets(rows, matcher, &mut row_offsets);
        out.extend(row_offsets.into_iter().map(|(source_visible_ix, offset)| {
            self.diff_search_visual_ix_for_source_match(source_visible_ix, region, offset)
        }));
    }

    fn diff_search_scan_current_view_with_needle(&mut self, query: AsciiCaseInsensitiveNeedle<'_>) {
        self.diff_search_matches.clear();

        if self.is_file_preview_active() {
            let Some(line_count) = self.worktree_preview_line_count() else {
                return;
            };
            if let Some(index) = self.worktree_preview_search_trigram_index.as_ref() {
                match index.candidates(query.as_bytes()) {
                    DiffSearchVisibleCandidates::None => {}
                    DiffSearchVisibleCandidates::All => {
                        for ix in 0..line_count {
                            if self.worktree_preview_line_raw_text(ix).is_some_and(|line| {
                                resolved_output_line_ix_matches_query(&line, query)
                            }) {
                                self.diff_search_matches.push(ix);
                            }
                        }
                    }
                    DiffSearchVisibleCandidates::Indexed(candidate_rows) => {
                        for &ix in candidate_rows {
                            let ix = ix as usize;
                            if self.worktree_preview_line_raw_text(ix).is_some_and(|line| {
                                resolved_output_line_ix_matches_query(&line, query)
                            }) {
                                self.diff_search_matches.push(ix);
                            }
                        }
                    }
                }
            } else {
                for ix in 0..line_count {
                    if self
                        .worktree_preview_line_raw_text(ix)
                        .is_some_and(|line| resolved_output_line_ix_matches_query(&line, query))
                    {
                        self.diff_search_matches.push(ix);
                    }
                }
            }
        } else if let Some((_path, conflict_kind)) = self.active_conflict_target() {
            if conflict_kind.is_some() || self.conflict_resolver.path.is_some() {
                let ctx =
                    ConflictResolverSearchContext::from_conflict_resolver(&self.conflict_resolver);
                self.diff_search_matches =
                    conflict_resolver_visible_match_indices_with_needle(query, &ctx);
            }
        } else {
            if self.diff_view == DiffViewMode::Inline
                && !self.is_file_diff_view_active()
                && !self.is_collapsed_diff_projection_active()
                && !self.diff_word_wrap
                && self.diff_search_scan_inline_patch_diff_with_needle(query)
            {
                return;
            }

            let total = self.diff_visible_len();
            if self.diff_word_wrap {
                for visible_ix in 0..total {
                    match self.diff_view {
                        DiffViewMode::Inline => {
                            let text =
                                self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline);
                            if query.is_match(text.as_ref()) {
                                self.diff_search_matches.push(visible_ix);
                            }
                        }
                        DiffViewMode::Split => {
                            let left = self
                                .diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
                            let right = self
                                .diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
                            if query.is_match(left.as_ref()) || query.is_match(right.as_ref()) {
                                self.diff_search_matches.push(visible_ix);
                            }
                        }
                    }
                }
                return;
            }
            if self.diff_view == DiffViewMode::Inline && self.is_file_diff_view_active() {
                for visible_ix in 0..total {
                    if self
                        .diff_search_file_diff_inline_visible_row_matches_query(query, visible_ix)
                    {
                        self.diff_search_matches.push(visible_ix);
                    }
                }
                return;
            }
            if self.diff_view == DiffViewMode::Split && self.is_file_diff_view_active() {
                let mut expanded_tabs = String::new();
                for visible_ix in 0..total {
                    if self.diff_search_file_diff_split_visible_row_matches_query(
                        query,
                        visible_ix,
                        &mut expanded_tabs,
                    ) {
                        self.diff_search_matches.push(visible_ix);
                    }
                }
                return;
            }

            for visible_ix in 0..total {
                match self.diff_view {
                    DiffViewMode::Inline => {
                        let text =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline);
                        if query.is_match(text.as_ref()) {
                            self.diff_search_matches.push(visible_ix);
                        }
                    }
                    DiffViewMode::Split => {
                        let left =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
                        let right =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
                        if query.is_match(left.as_ref()) || query.is_match(right.as_ref()) {
                            self.diff_search_matches.push(visible_ix);
                        }
                    }
                }
            }
        }
    }

    fn diff_search_scan_current_view_general(&mut self, matcher: &DiffSearchMatcher) {
        self.diff_search_matches.clear();

        if self.is_file_preview_active() {
            let Some(line_count) = self.worktree_preview_line_count() else {
                return;
            };
            if self.worktree_preview_source_len > 0 && self.worktree_preview_text.is_empty() {
                let mut matches = Vec::new();
                collect_file_diff_line_text_stream_match_visible_rows(
                    (0..line_count).filter_map(|ix| {
                        self.worktree_preview_line_raw_text(ix)
                            .map(|line| (ix, line))
                    }),
                    matcher,
                    &mut matches,
                );
                self.diff_search_matches = matches;
            } else {
                let rows: Vec<_> = (0..line_count)
                    .filter_map(|ix| {
                        self.worktree_preview_line_raw_text(ix)
                            .map(|line| (ix, Cow::Owned(line.as_ref().to_string())))
                    })
                    .collect();
                collect_stream_match_visible_rows(rows, matcher, &mut self.diff_search_matches);
            }
            self.diff_search_matches.sort_unstable();
            self.diff_search_matches.dedup();
            return;
        }

        if let Some((_path, conflict_kind)) = self.active_conflict_target() {
            if conflict_kind.is_some() || self.conflict_resolver.path.is_some() {
                let ctx =
                    ConflictResolverSearchContext::from_conflict_resolver(&self.conflict_resolver);
                self.diff_search_matches =
                    conflict_resolver_visible_match_indices_with_matcher(matcher, &ctx);
            }
            return;
        }

        if self.diff_view == DiffViewMode::Inline
            && !self.is_file_diff_view_active()
            && !self.is_collapsed_diff_projection_active()
            && !self.diff_word_wrap
            && self.diff_search_scan_inline_patch_diff_general(matcher)
        {
            return;
        }

        let total = self.diff_visible_len();
        if self.diff_word_wrap {
            let source_len = self
                .diff_wrap_visible_cache_key
                .map(|key| key.source_len)
                .unwrap_or(total);
            match self.diff_view {
                DiffViewMode::Inline => {
                    let mut matches = Vec::new();
                    self.diff_search_collect_wrapped_source_matches(
                        source_len,
                        DiffTextRegion::Inline,
                        matcher,
                        &mut matches,
                    );
                    self.diff_search_matches.extend(matches);
                }
                DiffViewMode::Split => {
                    let mut matches = Vec::new();
                    self.diff_search_collect_wrapped_source_matches(
                        source_len,
                        DiffTextRegion::SplitLeft,
                        matcher,
                        &mut matches,
                    );
                    self.diff_search_collect_wrapped_source_matches(
                        source_len,
                        DiffTextRegion::SplitRight,
                        matcher,
                        &mut matches,
                    );
                    self.diff_search_matches.extend(matches);
                }
            }
            self.diff_search_matches.sort_unstable();
            self.diff_search_matches.dedup();
            return;
        }
        if self.diff_view == DiffViewMode::Inline && self.is_file_diff_view_active() {
            let rows = (0..total).filter_map(|visible_ix| {
                self.diff_mapped_ix_for_visible_ix(visible_ix)
                    .and_then(|mapped_ix| self.file_diff_inline_render_data(mapped_ix))
                    .map(|row| (visible_ix, row.text))
            });
            let mut matches = Vec::new();
            collect_file_diff_line_text_stream_match_visible_rows(rows, matcher, &mut matches);
            self.diff_search_matches = matches;
            self.diff_search_matches.sort_unstable();
            self.diff_search_matches.dedup();
            return;
        }

        if self.diff_view == DiffViewMode::Split && self.is_file_diff_view_active() {
            let Some(provider) = self.file_diff_row_provider.as_ref() else {
                return;
            };
            let rows: Vec<_> = (0..total)
                .filter_map(|visible_ix| {
                    let mapped_ix = self.diff_mapped_ix_for_visible_ix(visible_ix)?;
                    let (left, right) = provider.split_row_texts(mapped_ix)?;
                    Some((
                        visible_ix,
                        left.map(|left| Cow::Owned(maybe_expand_tabs(left.as_ref()).to_string())),
                        right
                            .map(|right| Cow::Owned(maybe_expand_tabs(right.as_ref()).to_string())),
                    ))
                })
                .collect();
            collect_split_stream_match_visible_rows(rows, matcher, &mut self.diff_search_matches);
            self.diff_search_matches.sort_unstable();
            self.diff_search_matches.dedup();
            return;
        }

        match self.diff_view {
            DiffViewMode::Inline => {
                let rows: Vec<_> = (0..total)
                    .map(|visible_ix| {
                        let text =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline);
                        (visible_ix, Cow::Owned(text.as_ref().to_string()))
                    })
                    .collect();
                collect_stream_match_visible_rows(rows, matcher, &mut self.diff_search_matches);
            }
            DiffViewMode::Split => {
                let left_rows: Vec<_> = (0..total)
                    .map(|visible_ix| {
                        let text =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitLeft);
                        (visible_ix, Cow::Owned(text.as_ref().to_string()))
                    })
                    .collect();
                collect_stream_match_visible_rows(
                    left_rows,
                    matcher,
                    &mut self.diff_search_matches,
                );

                let right_rows: Vec<_> = (0..total)
                    .map(|visible_ix| {
                        let text =
                            self.diff_text_line_for_region(visible_ix, DiffTextRegion::SplitRight);
                        (visible_ix, Cow::Owned(text.as_ref().to_string()))
                    })
                    .collect();
                collect_stream_match_visible_rows(
                    right_rows,
                    matcher,
                    &mut self.diff_search_matches,
                );
            }
        }
        self.diff_search_matches.sort_unstable();
        self.diff_search_matches.dedup();
    }

    fn diff_search_scan_inline_patch_diff_with_needle(
        &mut self,
        query: AsciiCaseInsensitiveNeedle<'_>,
    ) -> bool {
        let diff = match self.rendered_patch_diff_loadable() {
            Some(Loadable::Ready(diff)) => Arc::clone(diff),
            _ => return false,
        };

        let diff_click_kinds = &self.diff_click_kinds;
        let diff_header_display_cache = &self.diff_header_display_cache;
        let diff_visible_inline_map = self.diff_visible_inline_map.as_ref();
        let diff_visible_indices = &self.diff_visible_indices;
        let matches = &mut self.diff_search_matches;

        if !diff_search_inline_patch_query_uses_trigram_index(query) {
            collect_inline_patch_diff_visible_matches_with_needle(
                diff.as_ref(),
                diff_click_kinds,
                diff_header_display_cache,
                diff_visible_inline_map,
                diff_visible_indices,
                query,
                matches,
            );
            return true;
        }

        if self.diff_search_inline_patch_trigram_index.is_none() {
            let mut index = DiffSearchVisibleTrigramIndex::default();
            let mut trigrams = SmallVec::<[u32; 64]>::new();
            if let Some(map) = self.diff_visible_inline_map.as_ref() {
                map.for_each_visible_src_ix(|visible_ix, src_ix| {
                    if let Some(text) = inline_patch_diff_search_text(
                        diff.as_ref(),
                        &self.diff_click_kinds,
                        &self.diff_header_display_cache,
                        src_ix,
                    ) {
                        index.insert_text(visible_ix as u32, text.as_ref(), &mut trigrams);
                    }
                });
            } else {
                for (visible_ix, &src_ix) in self.diff_visible_indices.iter().enumerate() {
                    if let Some(text) = inline_patch_diff_search_text(
                        diff.as_ref(),
                        &self.diff_click_kinds,
                        &self.diff_header_display_cache,
                        src_ix,
                    ) {
                        index.insert_text(visible_ix as u32, text.as_ref(), &mut trigrams);
                    }
                }
            }
            self.diff_search_inline_patch_trigram_index = Some(index.finish());
        }

        let index = self
            .diff_search_inline_patch_trigram_index
            .as_ref()
            .expect("inline patch diff trigram index initialized");

        match index.candidates(query.as_bytes()) {
            DiffSearchVisibleCandidates::None => {}
            DiffSearchVisibleCandidates::All => {
                collect_inline_patch_diff_visible_matches_with_needle(
                    diff.as_ref(),
                    diff_click_kinds,
                    diff_header_display_cache,
                    diff_visible_inline_map,
                    diff_visible_indices,
                    query,
                    matches,
                );
            }
            DiffSearchVisibleCandidates::Indexed(candidate_visible_rows) => {
                for &visible_ix in candidate_visible_rows {
                    let visible_ix = visible_ix as usize;
                    if inline_patch_diff_visible_ix_matches_query(
                        diff.as_ref(),
                        diff_click_kinds,
                        diff_header_display_cache,
                        diff_visible_inline_map,
                        diff_visible_indices,
                        query,
                        visible_ix,
                    ) {
                        matches.push(visible_ix);
                    }
                }
            }
        }

        true
    }

    fn diff_search_scan_inline_patch_diff_general(&mut self, matcher: &DiffSearchMatcher) -> bool {
        let diff = match self.rendered_patch_diff_loadable() {
            Some(Loadable::Ready(diff)) => Arc::clone(diff),
            _ => return false,
        };

        if let Some(map) = self.diff_visible_inline_map.as_ref() {
            let mut rows = Vec::new();
            map.for_each_visible_src_ix(|visible_ix, src_ix| {
                if let Some(text) = inline_patch_diff_search_text(
                    diff.as_ref(),
                    &self.diff_click_kinds,
                    &self.diff_header_display_cache,
                    src_ix,
                ) {
                    rows.push((visible_ix, Cow::Owned(text.as_ref().to_string())));
                }
            });
            collect_stream_match_visible_rows(rows, matcher, &mut self.diff_search_matches);
        } else {
            let rows: Vec<_> = self
                .diff_visible_indices
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(visible_ix, src_ix)| {
                    inline_patch_diff_search_text(
                        diff.as_ref(),
                        &self.diff_click_kinds,
                        &self.diff_header_display_cache,
                        src_ix,
                    )
                    .map(|text| (visible_ix, Cow::Owned(text.as_ref().to_string())))
                })
                .collect();
            collect_stream_match_visible_rows(rows, matcher, &mut self.diff_search_matches);
        }

        self.diff_search_matches.sort_unstable();
        self.diff_search_matches.dedup();
        true
    }

    pub(super) fn diff_search_finalize_matches(&mut self, mode: DiffSearchFinalizeMode) {
        if self.diff_search_matches.is_empty() {
            self.diff_search_match_ix = None;
            return;
        }

        match mode {
            DiffSearchFinalizeMode::ScrollToFirst => {
                self.diff_search_match_ix = Some(0);
                let first = self.diff_search_matches[0];
                self.diff_search_scroll_to_visible_ix(first);
            }
            DiffSearchFinalizeMode::PreserveCurrent {
                previous_match_ix,
                previous_visible_ix,
            } => {
                let had_previous_match =
                    previous_match_ix.is_some() || previous_visible_ix.is_some();
                let next_ix = previous_visible_ix
                    .and_then(|visible_ix| {
                        diff_search_resume_match_ix(Some(visible_ix), &self.diff_search_matches)
                    })
                    .or_else(|| {
                        previous_match_ix
                            .map(|ix| ix.min(self.diff_search_matches.len().saturating_sub(1)))
                    })
                    .unwrap_or(0);
                self.diff_search_match_ix = Some(next_ix);
                if !had_previous_match {
                    let first = self.diff_search_matches[next_ix];
                    self.diff_search_scroll_to_visible_ix(first);
                }
            }
        }
    }
}
