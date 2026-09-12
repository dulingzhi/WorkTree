//! `MainPaneView` cache lifecycle: ensure, refresh, reset and the scrollbar markers.

use super::super::helpers::{CollapsedDiffReveal, FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES};
use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild_with_patch;
use super::file_diff::file_diff_text_signature;
use super::helpers::{
    build_single_markdown_preview_document, diff_syntax_edit_from_text_change,
    file_diff_markdown_source_len, file_diff_text_is_source_backed,
    measure_markdown_preview_pictures, patch_diff_content_signature,
    read_file_diff_markdown_source, visual_line_kinds_for_patch_diff,
};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use super::patch_diff::{
    PATCH_DIFF_PAGE_SIZE, scrollbar_markers_from_visible_flags, should_hide_unified_diff_header_raw,
};
pub(in crate::view) use super::patch_diff::{
    PagedPatchDiffRows, PagedPatchSplitRows, PatchInlineVisibleMap,
};
use crate::view::diff_utils::compute_diff_yaml_block_scalar_for_src_ix;
use crate::view::markdown_preview;
use crate::view::perf::{self, ViewPerfSpan};
use crate::view::rows;
use crate::view::rows::should_hide_unified_diff_header_line;

// @split-module: impl_cache
impl MainPaneView {
    fn rebuild_patch_visual_line_kinds_from_ready_diff(
        &mut self,
        diff: &worktree_core::domain::Diff,
    ) {
        self.diff_visual_line_kind_for_src_ix =
            visual_line_kinds_for_patch_diff(diff, self.diff_whitespace_mode);
    }

    pub(in crate::view) fn rebuild_patch_visual_line_kinds_from_current_diff(&mut self) {
        let ready_diff = match self.rendered_patch_diff_loadable() {
            Some(Loadable::Ready(diff)) => Some(Arc::clone(diff)),
            _ => None,
        };
        if let Some(diff) = ready_diff {
            self.rebuild_patch_visual_line_kinds_from_ready_diff(diff.as_ref());
        } else {
            self.diff_visual_line_kind_for_src_ix = self.diff_line_kind_for_src_ix.clone();
        }
    }

    pub(in crate::view) fn ensure_patch_diff_word_highlight_for_src_ix(&mut self, src_ix: usize) {
        use worktree_core::domain::DiffLineKind as DK;

        let len = self.patch_diff_row_len();
        if src_ix >= len {
            return;
        }
        if self.diff_word_highlights.len() != len {
            self.diff_word_highlights.resize(len, None);
        }
        if self
            .diff_word_highlights
            .get(src_ix)
            .and_then(Option::as_ref)
            .is_some()
        {
            return;
        }

        if self.patch_diff_row(src_ix).is_none() {
            return;
        }
        if !matches!(self.patch_visual_line_kind(src_ix), DK::Add | DK::Remove) {
            return;
        }

        let mut group_start = src_ix;
        while group_start > 0 {
            let Some(prev) = self.patch_diff_row(group_start.saturating_sub(1)) else {
                break;
            };
            if matches!(prev.kind, DK::Remove) {
                group_start = group_start.saturating_sub(1);
            } else {
                break;
            }
        }

        let mut ix = group_start;
        let mut removed: Vec<(usize, AnnotatedDiffLine)> = Vec::new();
        while ix < len {
            let Some(line) = self.patch_diff_row(ix) else {
                break;
            };
            if !matches!(line.kind, DK::Remove) {
                break;
            }
            removed.push((ix, line));
            ix += 1;
        }

        let mut added: Vec<(usize, AnnotatedDiffLine)> = Vec::new();
        while ix < len {
            let Some(line) = self.patch_diff_row(ix) else {
                break;
            };
            if !matches!(line.kind, DK::Add) {
                break;
            }
            added.push((ix, line));
            ix += 1;
        }

        let pairs = removed.len().min(added.len());
        for i in 0..pairs {
            let (old_ix, old_line) = &removed[i];
            let (new_ix, new_line) = &added[i];
            let (old_ranges, new_ranges) =
                capped_word_diff_ranges(diff_content_text(old_line), diff_content_text(new_line));
            if matches!(self.patch_visual_line_kind(*old_ix), DK::Remove) && !old_ranges.is_empty()
            {
                self.diff_word_highlights[*old_ix] = Some(old_ranges);
            }
            if matches!(self.patch_visual_line_kind(*new_ix), DK::Add) && !new_ranges.is_empty() {
                self.diff_word_highlights[*new_ix] = Some(new_ranges);
            }
        }

        for (old_ix, old_line) in removed.into_iter().skip(pairs) {
            let text = diff_content_text(&old_line);
            if matches!(self.patch_visual_line_kind(old_ix), DK::Remove) && !text.is_empty() {
                self.diff_word_highlights[old_ix] = Some(vec![Range {
                    start: 0,
                    end: text.len(),
                }]);
            }
        }
        for (new_ix, new_line) in added.into_iter().skip(pairs) {
            let text = diff_content_text(&new_line);
            if matches!(self.patch_visual_line_kind(new_ix), DK::Add) && !text.is_empty() {
                self.diff_word_highlights[new_ix] = Some(vec![Range {
                    start: 0,
                    end: text.len(),
                }]);
            }
        }
    }

    pub(super) fn current_file_diff_line_to_row_maps(
        &self,
    ) -> (&[Option<usize>], &[Option<usize>], usize) {
        match self.diff_view {
            DiffViewMode::Inline => (
                self.file_diff_old_line_to_inline_row.as_ref(),
                self.file_diff_new_line_to_inline_row.as_ref(),
                self.file_diff_inline_row_len(),
            ),
            DiffViewMode::Split => (
                self.file_diff_old_line_to_row.as_ref(),
                self.file_diff_new_line_to_row.as_ref(),
                self.file_diff_split_row_len(),
            ),
        }
    }

    fn collapsed_hunk_row_range_for_parsed(
        &self,
        parsed: &crate::view::diff_utils::ParsedHunkHeader,
    ) -> Option<(usize, usize)> {
        let (old_line_to_row, new_line_to_row, _row_count) =
            self.current_file_diff_line_to_row_maps();

        let map_range_start = |line_to_row: &[Option<usize>], start_line: u32, line_count: u32| {
            (line_count > 0)
                .then_some(start_line)
                .filter(|line| *line > 0)
                .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                .and_then(|line_ix| line_to_row.get(line_ix).copied().flatten())
        };
        let map_range_end = |line_to_row: &[Option<usize>], start_line: u32, line_count: u32| {
            (line_count > 0)
                .then_some(start_line.saturating_add(line_count).saturating_sub(1))
                .filter(|line| *line > 0)
                .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                .and_then(|line_ix| line_to_row.get(line_ix).copied().flatten())
                .map(|row_ix| row_ix.saturating_add(1))
        };

        let start = [
            map_range_start(
                old_line_to_row,
                parsed.old_start_line,
                parsed.old_line_count,
            ),
            map_range_start(
                new_line_to_row,
                parsed.new_start_line,
                parsed.new_line_count,
            ),
        ]
        .into_iter()
        .flatten()
        .min()?;

        let end = [
            map_range_end(
                old_line_to_row,
                parsed.old_start_line,
                parsed.old_line_count,
            ),
            map_range_end(
                new_line_to_row,
                parsed.new_start_line,
                parsed.new_line_count,
            ),
        ]
        .into_iter()
        .flatten()
        .max()?;

        (start < end).then_some((start, end))
    }

    pub(in crate::view) fn collapsed_hunk_change_summary(&self, src_ix: usize) -> (bool, bool) {
        let mut has_additions = false;
        let mut has_removals = false;

        for candidate_ix in src_ix.saturating_add(1)..self.patch_diff_row_len() {
            let click_kind = self
                .diff_click_kinds
                .get(candidate_ix)
                .copied()
                .unwrap_or(DiffClickKind::Line);
            if click_kind != DiffClickKind::Line {
                break;
            }

            match self.patch_visual_line_kind(candidate_ix) {
                worktree_core::domain::DiffLineKind::Add => has_additions = true,
                worktree_core::domain::DiffLineKind::Remove => has_removals = true,
                worktree_core::domain::DiffLineKind::Context
                | worktree_core::domain::DiffLineKind::Header
                | worktree_core::domain::DiffLineKind::Hunk => {}
            }

            if has_additions && has_removals {
                break;
            }
        }

        (has_additions, has_removals)
    }

    fn reindex_collapsed_diff_hunks(&mut self) {
        self.collapsed_diff_hunk_ix_by_src_ix.clear();
        for (hunk_ix, hunk) in self.collapsed_diff_hunks.iter().enumerate() {
            let previous = self
                .collapsed_diff_hunk_ix_by_src_ix
                .insert(hunk.src_ix, hunk_ix);
            debug_assert!(previous.is_none());
        }
    }

    fn ensure_collapsed_diff_hunk_index(&mut self) {
        if self.collapsed_diff_hunk_ix_by_src_ix.len() != self.collapsed_diff_hunks.len() {
            self.reindex_collapsed_diff_hunks();
        }
    }

    fn ensure_collapsed_diff_hunks_initialized(&mut self) {
        if !self.collapsed_diff_hunks.is_empty() {
            self.ensure_collapsed_diff_hunk_index();
            return;
        }

        for src_ix in 0..self.patch_diff_row_len() {
            let click_kind = self
                .diff_click_kinds
                .get(src_ix)
                .copied()
                .unwrap_or(DiffClickKind::Line);
            if click_kind != DiffClickKind::HunkHeader {
                continue;
            }

            let Some(line) = self.patch_diff_row(src_ix) else {
                continue;
            };
            let Some(parsed) =
                crate::view::diff_utils::parse_unified_hunk_header_for_display(line.text.as_ref())
            else {
                continue;
            };
            let Some((base_row_start, base_row_end_exclusive)) =
                self.collapsed_hunk_row_range_for_parsed(&parsed)
            else {
                continue;
            };
            let (has_additions, has_removals) = self.collapsed_hunk_change_summary(src_ix);
            let reveal = self
                .collapsed_diff_reveals
                .get(&src_ix)
                .copied()
                .unwrap_or_default();
            self.collapsed_diff_hunks.push(CollapsedDiffHunk {
                src_ix,
                base_row_start,
                base_row_end_exclusive,
                has_additions,
                has_removals,
                reveal_up_lines: reveal.up_lines,
                reveal_down_lines: reveal.down_lines,
            });
        }
        self.reindex_collapsed_diff_hunks();
    }

    pub(super) fn persist_collapsed_diff_hunk_reveal(&mut self, hunk_ix: usize) {
        let Some(hunk) = self.collapsed_diff_hunks.get(hunk_ix).copied() else {
            return;
        };
        let reveal = CollapsedDiffReveal {
            up_lines: hunk.reveal_up_lines,
            down_lines: hunk.reveal_down_lines,
        };
        if reveal == CollapsedDiffReveal::default() {
            self.collapsed_diff_reveals.remove(&hunk.src_ix);
        } else {
            self.collapsed_diff_reveals.insert(hunk.src_ix, reveal);
        }
    }

    pub(super) fn merge_collapsed_diff_hunks_up(&mut self, hunk_ix: usize) {
        if hunk_ix == 0 || hunk_ix >= self.collapsed_diff_hunks.len() {
            return;
        }

        let previous = self.collapsed_diff_hunks[hunk_ix - 1];
        let current = self.collapsed_diff_hunks[hunk_ix];
        self.collapsed_diff_hunks[hunk_ix - 1] = CollapsedDiffHunk {
            src_ix: previous.src_ix,
            base_row_start: previous.base_row_start,
            base_row_end_exclusive: current.base_row_end_exclusive,
            has_additions: previous.has_additions || current.has_additions,
            has_removals: previous.has_removals || current.has_removals,
            reveal_up_lines: previous.reveal_up_lines,
            reveal_down_lines: current.reveal_down_lines,
        };
        self.collapsed_diff_hunks.remove(hunk_ix);
        self.reindex_collapsed_diff_hunks();
        self.collapsed_diff_header_display_cache.clear();
    }

    pub(super) fn merge_collapsed_diff_hunks_down(&mut self, hunk_ix: usize) {
        if hunk_ix + 1 >= self.collapsed_diff_hunks.len() {
            return;
        }

        let current = self.collapsed_diff_hunks[hunk_ix];
        let next = self.collapsed_diff_hunks[hunk_ix + 1];
        self.collapsed_diff_hunks[hunk_ix] = CollapsedDiffHunk {
            src_ix: current.src_ix,
            base_row_start: current.base_row_start,
            base_row_end_exclusive: next.base_row_end_exclusive,
            has_additions: current.has_additions || next.has_additions,
            has_removals: current.has_removals || next.has_removals,
            reveal_up_lines: current.reveal_up_lines,
            reveal_down_lines: next.reveal_down_lines,
        };
        self.collapsed_diff_hunks.remove(hunk_ix + 1);
        self.reindex_collapsed_diff_hunks();
        self.collapsed_diff_header_display_cache.clear();
    }

    fn normalize_collapsed_diff_hunks_after_rebuild(&mut self) {
        let mut hunk_ix = 0;
        while hunk_ix + 1 < self.collapsed_diff_hunks.len() {
            if self.collapsed_diff_gap_fully_revealed_after_rebuild(hunk_ix) {
                self.merge_collapsed_diff_hunks_down(hunk_ix);
            } else {
                hunk_ix += 1;
            }
        }
    }

    fn rebuild_collapsed_diff_header_display_cache(&mut self) {
        self.collapsed_diff_header_display_cache.clear();
        let src_ixs = self
            .collapsed_diff_hunks
            .iter()
            .map(|hunk| hunk.src_ix)
            .collect::<Vec<_>>();
        for src_ix in src_ixs {
            if let Some(display) = self.collapsed_diff_dynamic_hunk_range_display(src_ix) {
                self.collapsed_diff_header_display_cache
                    .insert(src_ix, display);
            }
        }
    }

    fn rebuild_collapsed_diff_projection(&mut self) {
        self.collapsed_diff_visible_rows.clear();
        self.collapsed_diff_hunk_visible_indices.clear();
        self.collapsed_diff_header_display_cache.clear();

        if !self.is_collapsed_diff_projection_active() {
            return;
        }

        let next_identity = self.current_collapsed_diff_projection_identity();
        if self.collapsed_diff_projection_identity != next_identity {
            self.collapsed_diff_hunks.clear();
            self.collapsed_diff_hunk_ix_by_src_ix.clear();
            self.collapsed_diff_reveals.clear();
        }
        self.collapsed_diff_projection_identity = next_identity;
        if self.collapsed_diff_projection_identity.is_none() {
            return;
        }

        let (_, _, total_rows) = self.current_file_diff_line_to_row_maps();
        if total_rows == 0 {
            return;
        }

        self.ensure_collapsed_diff_hunks_initialized();
        self.normalize_collapsed_diff_hunks_after_rebuild();
        self.reindex_collapsed_diff_hunks();

        if self.collapsed_diff_hunks.is_empty() {
            return;
        }

        for hunk_ix in 0..self.collapsed_diff_hunks.len() {
            let hunk = self.collapsed_diff_hunks[hunk_ix];
            let expansion_kind = self.collapsed_diff_expansion_kind(hunk_ix);
            let has_expansion_header =
                expansion_kind != crate::view::panes::main::CollapsedDiffExpansionKind::None;

            let up_revealed_rows = if hunk_ix == 0 {
                let leading_start = hunk
                    .base_row_start
                    .saturating_sub(hunk.reveal_up_lines.min(hunk.base_row_start));
                leading_start..hunk.base_row_start
            } else {
                let previous = self.collapsed_diff_hunks[hunk_ix - 1];
                let gap_start = previous.base_row_end_exclusive;
                let gap_end = hunk.base_row_start.max(gap_start);
                let gap_len = gap_end.saturating_sub(gap_start);
                let top_end = gap_start.saturating_add(previous.reveal_down_lines.min(gap_len));
                let bottom_start = gap_end.saturating_sub(hunk.reveal_up_lines.min(gap_len));

                for row_ix in gap_start..top_end {
                    self.collapsed_diff_visible_rows
                        .push(CollapsedDiffVisibleRow::FileRow { row_ix });
                }
                bottom_start.max(top_end)..gap_end
            };

            if !has_expansion_header {
                for row_ix in up_revealed_rows.clone() {
                    self.collapsed_diff_visible_rows
                        .push(CollapsedDiffVisibleRow::FileRow { row_ix });
                }
            }

            self.collapsed_diff_hunk_visible_indices
                .push(self.collapsed_diff_visible_rows.len());
            if has_expansion_header {
                let hidden_rows =
                    self.collapsed_diff_hidden_rows_for_expansion_kind(hunk.src_ix, expansion_kind);
                self.collapsed_diff_visible_rows
                    .push(CollapsedDiffVisibleRow::HunkHeader {
                        src_ix: hunk.src_ix,
                        expansion_kind,
                        display_src_ix: Some(hunk.src_ix),
                        hidden_rows,
                    });
                for row_ix in up_revealed_rows {
                    self.collapsed_diff_visible_rows
                        .push(CollapsedDiffVisibleRow::FileRow { row_ix });
                }
            }
            for row_ix in hunk.base_row_start..hunk.base_row_end_exclusive {
                self.collapsed_diff_visible_rows
                    .push(CollapsedDiffVisibleRow::FileRow { row_ix });
            }
        }

        if let Some(last_hunk) = self.collapsed_diff_hunks.last().copied() {
            let trailing_end = last_hunk
                .base_row_end_exclusive
                .saturating_add(
                    last_hunk
                        .reveal_down_lines
                        .min(total_rows.saturating_sub(last_hunk.base_row_end_exclusive)),
                )
                .min(total_rows);
            for row_ix in last_hunk.base_row_end_exclusive..trailing_end {
                self.collapsed_diff_visible_rows
                    .push(CollapsedDiffVisibleRow::FileRow { row_ix });
            }

            let hidden_rows = self.collapsed_diff_hidden_down_rows(last_hunk.src_ix);
            if hidden_rows > 0 {
                self.collapsed_diff_visible_rows
                    .push(CollapsedDiffVisibleRow::HunkHeader {
                        src_ix: last_hunk.src_ix,
                        expansion_kind: crate::view::panes::main::CollapsedDiffExpansionKind::Down,
                        display_src_ix: None,
                        hidden_rows,
                    });
            }
        }
        self.rebuild_collapsed_diff_header_display_cache();
    }

    pub(in super::super::super::super) fn ensure_single_markdown_preview_cache(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(path) = self.worktree_preview_path.clone() else {
            return;
        };
        let source_rev = self.worktree_preview_content_rev;
        if !matches!(self.worktree_preview, Loadable::Ready(_)) {
            return;
        }

        let cache_matches = self.worktree_markdown_preview_path.as_ref() == Some(&path)
            && self.worktree_markdown_preview_source_rev == source_rev;
        if cache_matches {
            match &self.worktree_markdown_preview {
                Loadable::Ready(_) | Loadable::Error(_) => return,
                Loadable::Loading if self.worktree_markdown_preview_inflight.is_some() => return,
                _ => {}
            }
        }

        self.worktree_markdown_preview_path = Some(path.clone());
        self.worktree_markdown_preview_source_rev = source_rev;

        let source_len = if self.worktree_preview_text.is_empty() {
            self.worktree_preview_source_len
        } else {
            self.worktree_preview_text.len()
        };
        if source_len > markdown_preview::MAX_PREVIEW_SOURCE_BYTES {
            self.worktree_markdown_preview = Loadable::Error(
                markdown_preview::single_preview_unavailable_reason(source_len).to_string(),
            );
            self.worktree_markdown_preview_inflight = None;
            return;
        }

        self.worktree_markdown_preview = Loadable::Loading;
        self.worktree_markdown_preview_seq = self.worktree_markdown_preview_seq.wrapping_add(1);
        let seq = self.worktree_markdown_preview_seq;
        self.worktree_markdown_preview_inflight = Some(seq);
        let source_text =
            (!self.worktree_preview_text.is_empty()).then_some(self.worktree_preview_text.clone());
        let source_path = self.worktree_preview_source_path.clone();
        let image_base_dir = self.markdown_preview_image_base_dir();

        cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                type BuiltPreview = (
                    Arc<markdown_preview::MarkdownPreviewDocument>,
                    rows::MarkdownPreviewPictureSizes,
                );
                let build_preview =
                    move || -> Result<BuiltPreview, markdown_preview::MarkdownPreviewRefusal> {
                        let _perf_scope = perf::span(ViewPerfSpan::MarkdownPreviewParse);
                        let source_text = match source_text {
                            Some(source_text) => source_text,
                            None => {
                                let source_path = source_path.ok_or_else(|| {
                                    "Preview source path is unavailable.".to_string()
                                })?;
                                std::fs::read_to_string(&source_path)
                                .map(SharedString::from)
                                .map_err(|e| {
                                    if e.kind() == std::io::ErrorKind::InvalidData {
                                        "File is not valid UTF-8; binary preview is not supported."
                                            .to_string()
                                    } else {
                                        e.to_string()
                                    }
                                })?
                            }
                        };
                        let document =
                            build_single_markdown_preview_document(source_text.as_ref())?;
                        // Measured here rather than on the first frame: it reads
                        // files, and this is already the thread that does that.
                        let picture_sizes = measure_markdown_preview_pictures(
                            document.as_ref(),
                            image_base_dir.as_deref(),
                        );
                        Ok((document, picture_sizes))
                    };
                let result = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(build_preview).await
                } else {
                    build_preview()
                };

                let _ = view.update(cx, |this, cx| {
                    if this.worktree_markdown_preview_inflight != Some(seq) {
                        return;
                    }
                    if this.worktree_preview_path.as_ref() != Some(&path)
                        || this.worktree_preview_content_rev != source_rev
                    {
                        return;
                    }

                    this.worktree_markdown_preview_inflight = None;
                    match result {
                        Ok((document, picture_sizes)) => {
                            this.worktree_markdown_preview_picture_sizes = picture_sizes;
                            // The blocks these positions belonged to are gone
                            // with the document that described them.
                            this.worktree_markdown_preview_block_scrolls.clear();
                            this.worktree_markdown_preview = Loadable::Ready(document);
                            // An open search scanned nothing while this was
                            // parsing, so without a rescan it would keep
                            // reporting "no matches" over a document that
                            // plainly holds the term.
                            this.diff_search_recompute_matches();
                        }
                        Err(refusal) => {
                            // The document these described is gone too, so they
                            // are cleared here for the same reason as above.
                            this.worktree_markdown_preview_picture_sizes = Default::default();
                            this.worktree_markdown_preview_block_scrolls.clear();
                            let prefers_source = refusal.prefers_source();
                            this.worktree_markdown_preview =
                                Loadable::Error(refusal.into_message());
                            // A document that parsed but is too big to lay out
                            // still reads fine as source, so the reader is
                            // taken there rather than left on an empty pane
                            // with a message and a toggle to find.
                            if prefers_source {
                                this.rendered_preview_modes.set(
                                    RenderedPreviewKind::Markdown,
                                    RenderedPreviewMode::Source,
                                );
                            }
                        }
                    }
                    cx.notify();
                });
            },
        )
        .detach();
    }

    /// Resets file-diff data fields (syntax, rows, text, highlights) without
    /// touching the identity fields (repo_id, target, rev).
    pub(in crate::view) fn reset_file_diff_cache_data(&mut self) {
        self.reset_collapsed_diff_projection(false);
        self.file_diff_cache_content_signature = None;
        self.file_diff_cache_inflight = None;
        self.file_diff_cache_error = None;
        self.file_diff_syntax_generation = self.file_diff_syntax_generation.wrapping_add(1);
        self.file_diff_style_cache_epochs.bump_both();
        self.file_diff_cache_path = None;
        self.file_diff_cache_language = None;
        self.file_diff_cache_rows.clear();
        self.file_diff_row_provider = None;
        self.file_diff_old_text = SharedString::default();
        self.file_diff_old_line_starts = Arc::default();
        self.file_diff_old_line_to_row = Arc::default();
        self.file_diff_old_line_to_inline_row = Arc::default();
        self.file_diff_new_text = SharedString::default();
        self.file_diff_new_line_starts = Arc::default();
        self.file_diff_new_line_to_row = Arc::default();
        self.file_diff_new_line_to_inline_row = Arc::default();
        self.file_diff_inline_cache.clear();
        self.file_diff_inline_row_provider = None;
        self.file_diff_inline_text = SharedString::default();
        self.reset_file_diff_word_highlight_caches();
    }

    /// Drop the memoized intra-line word-diff ranges. They are keyed by bare row
    /// index, so they only describe the rows they were computed from: anything
    /// that changes what row *n* holds has to clear them or row *n* keeps ranges
    /// measured against text it no longer shows.
    pub(in crate::view) fn reset_file_diff_word_highlight_caches(&mut self) {
        self.file_diff_inline_word_highlights =
            rows::new_lru_cache(FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES);
        self.file_diff_split_word_highlights =
            rows::new_lru_cache(FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES);
    }

    pub(in super::super::super::super) fn ensure_file_diff_cache(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some((
            repo_id,
            diff_file_rev,
            diff_target,
            workdir,
            expected_abs_path,
            file,
            patch_diff,
            patch_diff_loading,
        )) = (|| {
            let (repo_id, diff_file_rev, diff_target, workdir, expected_abs_path) =
                self.rendered_file_diff_identity()?;
            let file: Option<Arc<worktree_core::domain::FileDiffText>> =
                match self.rendered_file_diff_loadable()? {
                    Loadable::Ready(Some(file)) => Some(Arc::clone(file)),
                    _ => None,
                };
            let patch_diff_loadable = self.rendered_patch_diff_loadable()?;
            let patch_diff: Option<Arc<worktree_core::domain::Diff>> = match patch_diff_loadable {
                Loadable::Ready(diff) => Some(Arc::clone(diff)),
                _ => None,
            };
            let patch_diff_loading = matches!(patch_diff_loadable, Loadable::Loading);

            Some((
                repo_id,
                diff_file_rev,
                diff_target,
                workdir,
                expected_abs_path,
                file,
                patch_diff,
                patch_diff_loading,
            ))
        })()
        else {
            self.file_diff_cache_repo_id = None;
            self.file_diff_cache_target = None;
            self.file_diff_cache_rev = 0;
            self.reset_file_diff_cache_data();
            return;
        };

        let diff_target_for_task = diff_target.clone();
        let file_content_signature = file.as_ref().map(|file| {
            let mut signature = file_diff_text_signature(file.as_ref());
            if let Some(patch_diff) = patch_diff.as_ref() {
                signature ^= patch_diff_content_signature(patch_diff.as_ref()).rotate_left(1);
            }
            signature ^= (self.diff_whitespace_mode.key().len() as u64).rotate_left(7);
            signature
        });
        let same_repo_and_target = self.file_diff_cache_repo_id == Some(repo_id)
            && self.file_diff_cache_target == Some(diff_target.clone())
            && self.file_diff_cache_whitespace_mode == self.diff_whitespace_mode
            && self.file_diff_cache_path.as_ref() == Some(&expected_abs_path);
        let previous_split_left_reparse_seed = same_repo_and_target
            .then(|| self.file_diff_split_prepared_syntax_document(DiffTextRegion::SplitLeft))
            .flatten();
        let previous_split_right_reparse_seed = same_repo_and_target
            .then(|| self.file_diff_split_prepared_syntax_document(DiffTextRegion::SplitRight))
            .flatten();
        let previous_old_text = same_repo_and_target.then(|| self.file_diff_old_text.clone());
        let previous_new_text = same_repo_and_target.then(|| self.file_diff_new_text.clone());

        if patch_diff_loading
            && patch_diff.is_none()
            && file
                .as_ref()
                .is_some_and(|file| file_diff_text_is_source_backed(file.as_ref()))
        {
            if same_repo_and_target {
                self.file_diff_cache_inflight = None;
                self.rekey_file_diff_prepared_syntax_documents_for_rev(diff_file_rev);
                self.file_diff_cache_rev = diff_file_rev;
            } else {
                self.file_diff_cache_repo_id = Some(repo_id);
                self.file_diff_cache_rev = diff_file_rev;
                self.file_diff_cache_whitespace_mode = self.diff_whitespace_mode;
                self.file_diff_cache_target = Some(diff_target);
                self.reset_file_diff_cache_data();
                self.clear_diff_text_style_caches();
            }
            return;
        }

        if same_repo_and_target
            && file.is_none()
            && self.file_diff_cache_content_signature.is_some()
        {
            // Keep the current same-target rows visible while a refresh is pending.
            // Dropping them would create a zero-width frame, and GPUI would clamp
            // any horizontal offset back to the start before the ready payload returns.
            self.rekey_file_diff_prepared_syntax_documents_for_rev(diff_file_rev);
            self.file_diff_cache_rev = diff_file_rev;
            return;
        }

        if same_repo_and_target && self.file_diff_cache_rev == diff_file_rev {
            // Reselecting the same file enters Loading with an unchanged file rev; keep the
            // current cache until a ready file payload proves the effective content changed.
            let content_changed_without_rev_bump = file_content_signature
                .is_some_and(|signature| self.file_diff_cache_content_signature != Some(signature));
            if !content_changed_without_rev_bump {
                return;
            }
        }

        if same_repo_and_target
            && let Some(signature) = file_content_signature
            && self.file_diff_cache_content_signature == Some(signature)
        {
            // Store-side refreshes can bump diff_file_rev with identical file payloads.
            // Keep the row cache and prepared syntax documents alive across rev-only refreshes.
            // Any older row rebuild is now redundant because the current rows already match
            // the active content signature.
            self.file_diff_cache_inflight = None;
            self.rekey_file_diff_prepared_syntax_documents_for_rev(diff_file_rev);
            self.file_diff_cache_rev = diff_file_rev;
            self.refresh_file_diff_syntax_documents(cx, None, None, None, None);
            return;
        }

        self.file_diff_cache_repo_id = Some(repo_id);
        self.file_diff_cache_rev = diff_file_rev;
        self.file_diff_cache_whitespace_mode = self.diff_whitespace_mode;
        self.file_diff_cache_target = Some(diff_target);

        // Rebuilding the same file keeps the rows that are already on screen:
        // they are a complete, self-consistent generation, the completion below
        // swaps every field of the next one in atomically, and dropping them
        // first is what makes the pane flash "Processing file…" on every staged
        // line. A different file — or one with no content to rebuild from — must
        // still clear immediately, since its rows are not this file's.
        let keep_rows_while_rebuilding = same_repo_and_target && file.is_some();
        if !keep_rows_while_rebuilding {
            self.reset_file_diff_cache_data();

            // Reset the segment cache to avoid mixing patch/file indices.
            self.clear_diff_text_style_caches();
        }

        let Some(file) = file else {
            return;
        };
        let content_signature =
            file_content_signature.unwrap_or_else(|| file_diff_text_signature(file.as_ref()));

        self.file_diff_cache_seq = self.file_diff_cache_seq.wrapping_add(1);
        let seq = self.file_diff_cache_seq;
        self.file_diff_cache_inflight = Some(seq);
        self.file_diff_syntax_generation = seq;
        let whitespace_mode = self.diff_whitespace_mode;

        cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                let rebuild_cache = move || {
                    build_file_diff_cache_rebuild_with_patch(
                        file.as_ref(),
                        &workdir,
                        patch_diff.as_deref(),
                        whitespace_mode,
                    )
                };
                let rebuild_result = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(rebuild_cache).await
                } else {
                    rebuild_cache()
                };

                let _ = view.update(cx, |this, cx| {
                    if this.file_diff_cache_inflight != Some(seq) {
                        return;
                    }
                    if this.file_diff_cache_repo_id != Some(repo_id)
                        || this.file_diff_cache_rev != diff_file_rev
                        || this.file_diff_cache_whitespace_mode != whitespace_mode
                        || this.file_diff_cache_target != Some(diff_target_for_task.clone())
                    {
                        return;
                    }

                    this.file_diff_cache_inflight = None;
                    let rebuild = match rebuild_result {
                        Ok(rebuild) => rebuild,
                        Err(error) => {
                            this.reset_file_diff_cache_data();
                            this.file_diff_cache_repo_id = Some(repo_id);
                            this.file_diff_cache_rev = diff_file_rev;
                            this.file_diff_cache_whitespace_mode = whitespace_mode;
                            this.file_diff_cache_target = Some(diff_target_for_task.clone());
                            this.file_diff_cache_path = Some(expected_abs_path.clone());
                            this.file_diff_cache_content_signature = Some(content_signature);
                            this.file_diff_cache_error = Some(error);
                            cx.notify();
                            return;
                        }
                    };
                    this.file_diff_cache_error = None;
                    this.file_diff_cache_path = rebuild.file_path;
                    this.file_diff_cache_language = rebuild.language;
                    this.file_diff_row_provider = Some(rebuild.row_provider);
                    this.file_diff_old_text = rebuild.old_text;
                    this.file_diff_old_line_starts = rebuild.old_line_starts;
                    this.file_diff_old_line_to_row = rebuild.old_line_to_row;
                    this.file_diff_old_line_to_inline_row = rebuild.old_line_to_inline_row;
                    this.file_diff_new_text = rebuild.new_text;
                    this.file_diff_new_line_starts = rebuild.new_line_starts;
                    this.file_diff_new_line_to_row = rebuild.new_line_to_row;
                    this.file_diff_new_line_to_inline_row = rebuild.new_line_to_inline_row;
                    this.file_diff_inline_row_provider = Some(rebuild.inline_row_provider);
                    this.file_diff_inline_text = rebuild.inline_text;
                    this.file_diff_cache_content_signature = Some(content_signature);
                    // The rows just changed under their own indices. On the
                    // clearing path `reset_file_diff_cache_data` already did
                    // this; the kept-rows path deliberately skips it, so without
                    // this a staged line leaves every row holding the previous
                    // generation's word ranges.
                    this.reset_file_diff_word_highlight_caches();
                    #[cfg(test)]
                    {
                        this.file_diff_cache_rows = rebuild.rows;
                        this.file_diff_inline_cache = rebuild.inline_rows;
                    }
                    let split_left_edit_hint = previous_old_text.as_ref().and_then(|previous| {
                        diff_syntax_edit_from_text_change(
                            previous.as_ref(),
                            this.file_diff_old_text.as_ref(),
                        )
                    });
                    let split_right_edit_hint = previous_new_text.as_ref().and_then(|previous| {
                        diff_syntax_edit_from_text_change(
                            previous.as_ref(),
                            this.file_diff_new_text.as_ref(),
                        )
                    });
                    this.refresh_file_diff_syntax_documents(
                        cx,
                        previous_split_left_reparse_seed,
                        previous_split_right_reparse_seed,
                        split_left_edit_hint,
                        split_right_edit_hint,
                    );

                    // Reset the segment cache to avoid mixing patch/file indices.
                    this.clear_diff_text_style_caches();
                    cx.notify();
                });
            },
        )
        .detach();
    }

    pub(in super::super::super::super) fn ensure_file_markdown_preview_cache(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let clear_cache = |this: &mut Self| {
            this.file_markdown_preview_cache_repo_id = None;
            this.file_markdown_preview_cache_target = None;
            this.file_markdown_preview_cache_rev = 0;
            this.file_markdown_preview_cache_content_signature = None;
            this.file_markdown_preview = Loadable::NotLoaded;
            this.file_markdown_preview_inflight = None;
        };

        let Some((repo_id, diff_file_rev, diff_target, expected_abs_path, file)) = (|| {
            let (repo_id, diff_file_rev, diff_target, _workdir, expected_abs_path) =
                self.rendered_file_diff_identity()?;
            let file: Option<Arc<worktree_core::domain::FileDiffText>> =
                match self.rendered_file_diff_loadable()? {
                    Loadable::Ready(Some(file)) => Some(Arc::clone(file)),
                    _ => None,
                };

            Some((repo_id, diff_file_rev, diff_target, expected_abs_path, file))
        })() else {
            clear_cache(self);
            return;
        };

        let diff_target_for_task = diff_target.clone();
        let file_content_signature = file
            .as_ref()
            .map(|file| file_diff_text_signature(file.as_ref()));
        let same_repo_and_target = self.file_markdown_preview_cache_repo_id == Some(repo_id)
            && self.file_markdown_preview_cache_target == Some(diff_target.clone())
            && self.file_diff_cache_path.as_ref() == Some(&expected_abs_path);

        if same_repo_and_target && self.file_markdown_preview_cache_rev == diff_file_rev {
            return;
        }

        if same_repo_and_target
            && let Some(signature) = file_content_signature
            && self.file_markdown_preview_cache_content_signature == Some(signature)
        {
            if self.file_markdown_preview_inflight.is_none() {
                self.file_markdown_preview_cache_rev = diff_file_rev;
            }
            return;
        }

        self.file_markdown_preview_cache_repo_id = Some(repo_id);
        self.file_markdown_preview_cache_rev = diff_file_rev;
        self.file_markdown_preview_cache_content_signature = None;
        self.file_markdown_preview_cache_target = Some(diff_target);
        self.file_markdown_preview = Loadable::NotLoaded;
        self.file_markdown_preview_inflight = None;

        let Some(file) = file else {
            return;
        };
        // `file` was `Some` when `file_content_signature` was computed, so unwrap is safe.
        let content_signature = file_content_signature.unwrap();
        let old_source = file.old_source.clone();
        let new_source = file.new_source.clone();
        let old_legacy_text = file.old.clone();
        let new_legacy_text = file.new.clone();

        let combined_len =
            file_diff_markdown_source_len(old_source.as_ref(), old_legacy_text.as_ref())
                + file_diff_markdown_source_len(new_source.as_ref(), new_legacy_text.as_ref());
        if combined_len > markdown_preview::MAX_DIFF_PREVIEW_SOURCE_BYTES {
            self.file_markdown_preview = Loadable::Error(
                markdown_preview::diff_preview_unavailable_reason(combined_len).to_string(),
            );
            self.file_markdown_preview_cache_content_signature = Some(content_signature);
            return;
        }

        self.file_markdown_preview = Loadable::Loading;
        self.file_markdown_preview_seq = self.file_markdown_preview_seq.wrapping_add(1);
        let seq = self.file_markdown_preview_seq;
        self.file_markdown_preview_inflight = Some(seq);

        cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                let build_preview = move || {
                    let _perf_scope = perf::span(ViewPerfSpan::MarkdownPreviewParse);
                    let old_source = read_file_diff_markdown_source(
                        old_source.as_ref(),
                        old_legacy_text.as_ref(),
                    )?;
                    let new_source = read_file_diff_markdown_source(
                        new_source.as_ref(),
                        new_legacy_text.as_ref(),
                    )?;
                    markdown_preview::build_markdown_diff_preview(
                        old_source.as_ref(),
                        new_source.as_ref(),
                    )
                    .map(Arc::new)
                    .ok_or_else(|| {
                        markdown_preview::diff_preview_unavailable_reason(
                            old_source.len() + new_source.len(),
                        )
                        .to_string()
                    })
                };
                let result = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(build_preview).await
                } else {
                    build_preview()
                };

                let _ = view.update(cx, |this, cx| {
                    if this.file_markdown_preview_inflight != Some(seq) {
                        return;
                    }
                    if this.file_markdown_preview_cache_repo_id != Some(repo_id)
                        || this.file_markdown_preview_cache_rev != diff_file_rev
                        || this.file_markdown_preview_cache_target
                            != Some(diff_target_for_task.clone())
                    {
                        return;
                    }

                    this.file_markdown_preview_inflight = None;
                    this.file_markdown_preview_cache_content_signature = Some(content_signature);
                    match result {
                        Ok(preview) => this.file_markdown_preview = Loadable::Ready(preview),
                        Err(error) => this.file_markdown_preview = Loadable::Error(error),
                    }
                    // See the single-document preview: a search opened while
                    // this was parsing found nothing and needs to rescan.
                    this.diff_search_recompute_matches();
                    cx.notify();
                });
            },
        )
        .detach();
    }

    pub(in super::super::super::super) fn ensure_rendered_patch_diff_cache(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let ready_diff = match self.rendered_patch_diff_loadable() {
            Some(Loadable::Ready(diff)) => Some(Arc::clone(diff)),
            _ => None,
        };
        let metadata_current = self.diff_cache_repo_id == self.active_repo_id()
            && self.diff_cache_rev == self.rendered_patch_diff_rev()
            && self.diff_cache_target == self.rendered_diff_target().cloned();
        let ready_content_changed = metadata_current
            && ready_diff.as_ref().is_some_and(|diff| {
                self.patch_diff_row_len() != diff.lines.len()
                    || self.diff_cache_content_signature
                        != Some(patch_diff_content_signature(diff.as_ref()))
            });
        let should_rebuild = !metadata_current || ready_content_changed;
        if should_rebuild {
            self.rebuild_diff_cache(cx);
        }
    }

    pub(in super::super::super::super) fn rebuild_diff_cache(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let next_cache_state = self.active_repo().map(|repo| {
            let workdir: Option<std::path::PathBuf> = self
                .rendered_diff_workdir()
                .map(std::path::Path::to_path_buf);
            let diff = match self.rendered_patch_diff_loadable() {
                Some(Loadable::Ready(diff)) => Some(Arc::clone(diff)),
                _ => None,
            };
            (
                repo.id,
                self.rendered_patch_diff_rev(),
                self.rendered_diff_target().cloned(),
                workdir,
                diff,
            )
        });
        let next_content_signature = next_cache_state
            .as_ref()
            .and_then(|(_, _, _, _, diff)| diff.as_ref())
            .map(|diff| patch_diff_content_signature(diff.as_ref()));
        if let Some((repo_id, diff_rev, diff_target, _, diff)) = next_cache_state.as_ref() {
            let same_repo_and_target = self.diff_cache_repo_id == Some(*repo_id)
                && self.diff_cache_target.as_ref() == diff_target.as_ref();
            if same_repo_and_target {
                if diff.is_none() && self.diff_cache_content_signature.is_some() {
                    // Preserve the last ready same-target patch rows through transient Loading.
                    self.diff_cache_rev = *diff_rev;
                    return;
                }

                if diff.is_some()
                    && next_content_signature.is_some()
                    && self.diff_cache_content_signature == next_content_signature
                {
                    // Store-side refreshes can bump diff_rev without changing the rendered patch.
                    // Keep visible rows and horizontal width hints intact across those rev-only
                    // redraws.
                    self.diff_cache_rev = *diff_rev;
                    return;
                }
            }
        }
        let clear_reveals = match next_cache_state.as_ref() {
            Some((repo_id, _, diff_target, _, Some(_))) if diff_target.is_some() => {
                self.diff_cache_repo_id != Some(*repo_id)
                    || self.diff_cache_target.as_ref() != diff_target.as_ref()
                    || self.diff_cache_content_signature != next_content_signature
            }
            _ => true,
        };

        self.reset_collapsed_diff_projection(clear_reveals);
        self.diff_cache.clear();
        self.diff_row_provider = None;
        self.diff_split_row_provider = None;
        self.diff_cache_repo_id = None;
        self.diff_cache_rev = 0;
        self.diff_cache_content_signature = None;
        self.diff_cache_target = None;
        self.diff_file_for_src_ix.clear();
        self.diff_language_for_src_ix.clear();
        self.diff_yaml_block_scalar_for_src_ix.clear();
        self.diff_click_kinds.clear();
        self.diff_line_kind_for_src_ix.clear();
        self.diff_visual_line_kind_for_src_ix.clear();
        self.diff_hide_unified_header_for_src_ix.clear();
        self.diff_header_display_cache.clear();
        self.diff_split_cache.clear();
        self.diff_split_cache_len = 0;
        self.diff_visible_indices.clear();
        self.diff_visible_inline_map = None;
        self.diff_visible_cache_len = 0;
        self.diff_visible_is_file_view = false;
        self.diff_scrollbar_markers_cache.clear();
        self.diff_word_highlights.clear();
        self.diff_word_highlights_inflight = None;
        self.diff_file_stats.clear();
        self.clear_diff_text_style_caches();
        self.clear_diff_selection_state();
        self.diff_preview_is_new_file = false;

        let Some((repo_id, diff_rev, diff_target, workdir, diff)) = next_cache_state else {
            return;
        };

        self.diff_cache_repo_id = Some(repo_id);
        self.diff_cache_rev = diff_rev;
        self.diff_cache_content_signature = next_content_signature;
        self.diff_cache_target = diff_target;

        let Some(diff) = diff else {
            return;
        };
        let Some(workdir) = workdir else {
            return;
        };

        let row_provider = Arc::new(PagedPatchDiffRows::new(
            Arc::clone(&diff),
            PATCH_DIFF_PAGE_SIZE,
        ));
        let mut split_row_count = 0usize;
        let mut pending_split_removes = 0usize;
        let mut pending_split_adds = 0usize;
        self.diff_row_provider = Some(row_provider);

        self.diff_file_for_src_ix = compute_diff_file_for_src_ix(diff.lines.as_slice());
        self.diff_line_kind_for_src_ix = diff
            .lines
            .iter()
            .map(|line| {
                match line.kind {
                    worktree_core::domain::DiffLineKind::Remove => pending_split_removes += 1,
                    worktree_core::domain::DiffLineKind::Add => pending_split_adds += 1,
                    worktree_core::domain::DiffLineKind::Context
                    | worktree_core::domain::DiffLineKind::Header
                    | worktree_core::domain::DiffLineKind::Hunk => {
                        split_row_count += pending_split_removes.max(pending_split_adds) + 1;
                        pending_split_removes = 0;
                        pending_split_adds = 0;
                    }
                }
                line.kind
            })
            .collect();
        self.rebuild_patch_visual_line_kinds_from_ready_diff(diff.as_ref());
        split_row_count += pending_split_removes.max(pending_split_adds);
        self.diff_split_row_provider = Some(Arc::new(PagedPatchSplitRows::new_with_len_hint(
            Arc::clone(self.diff_row_provider.as_ref().expect("set just above")),
            split_row_count,
        )));
        self.diff_hide_unified_header_for_src_ix = diff
            .lines
            .iter()
            .map(|line| should_hide_unified_diff_header_raw(line.kind, line.text.as_ref()))
            .collect();
        self.diff_click_kinds = diff
            .lines
            .iter()
            .map(|line| {
                if matches!(line.kind, worktree_core::domain::DiffLineKind::Hunk) {
                    DiffClickKind::HunkHeader
                } else if matches!(line.kind, worktree_core::domain::DiffLineKind::Header)
                    && line.text.starts_with("diff --git ")
                {
                    DiffClickKind::FileHeader
                } else {
                    DiffClickKind::Line
                }
            })
            .collect();
        for (src_ix, click_kind) in self.diff_click_kinds.iter().enumerate() {
            match click_kind {
                DiffClickKind::FileHeader => {
                    let Some(line) = diff.lines.get(src_ix) else {
                        continue;
                    };
                    // The header row opens its own file section, so the path
                    // resolved for it above is exactly what to show.
                    let display: SharedString = self
                        .diff_file_for_src_ix
                        .get(src_ix)
                        .and_then(|path| path.as_ref())
                        .map(|path| SharedString::new(Arc::clone(path)))
                        .unwrap_or_else(|| SharedString::from(line.text.as_ref().to_string()));
                    self.diff_header_display_cache.insert(src_ix, display);
                }
                DiffClickKind::HunkHeader => {
                    let Some(line) = diff.lines.get(src_ix) else {
                        continue;
                    };
                    let display = parse_unified_hunk_header_for_display(line.text.as_ref())
                        .map(|p| {
                            let heading = p.heading.unwrap_or_default();
                            if heading.is_empty() {
                                format!("{} {}", p.old, p.new)
                            } else {
                                format!("{} {}  {heading}", p.old, p.new)
                            }
                        })
                        .unwrap_or_else(|| line.text.as_ref().to_string());
                    self.diff_header_display_cache
                        .insert(src_ix, display.into());
                }
                DiffClickKind::Line => {}
            }
        }
        self.diff_file_stats = compute_diff_file_stats(diff.lines.as_slice());
        self.diff_word_highlights = vec![None; self.patch_diff_row_len()];
        self.diff_word_highlights_inflight = None;

        let mut current_file: Option<Arc<str>> = None;
        let mut current_language: Option<rows::DiffSyntaxLanguage> = None;
        for (src_ix, line) in diff.lines.iter().enumerate() {
            let file = self
                .diff_file_for_src_ix
                .get(src_ix)
                .and_then(|p| p.as_ref());
            let file_changed = match (&current_file, file) {
                (Some(cur), Some(next)) => !Arc::ptr_eq(cur, next),
                (None, None) => false,
                _ => true,
            };
            if file_changed {
                current_file = file.cloned();
                current_language =
                    file.and_then(|p| rows::diff_syntax_language_for_path(p.as_ref()));
            }

            let language = match line.kind {
                worktree_core::domain::DiffLineKind::Add
                | worktree_core::domain::DiffLineKind::Remove
                | worktree_core::domain::DiffLineKind::Context => current_language,
                worktree_core::domain::DiffLineKind::Header
                | worktree_core::domain::DiffLineKind::Hunk => None,
            };
            self.diff_language_for_src_ix.push(language);
        }
        self.diff_yaml_block_scalar_for_src_ix = compute_diff_yaml_block_scalar_for_src_ix(
            diff.lines.as_slice(),
            self.diff_file_for_src_ix.as_slice(),
            self.diff_language_for_src_ix.as_slice(),
        );
        if let Some(preview) = build_new_file_preview_from_diff(
            diff.lines.as_slice(),
            &workdir,
            self.diff_cache_target.as_ref(),
        ) {
            self.diff_preview_is_new_file = true;
            self.set_worktree_preview_ready_rows(
                preview.abs_path,
                preview.lines.as_slice(),
                preview.source_len,
                cx,
            );
            self.worktree_preview_scroll
                .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
        }
    }

    fn ensure_diff_split_cache(&mut self) {
        if self.diff_split_row_provider.is_some() {
            return;
        }
        if self.diff_split_cache_len == self.diff_cache.len() && !self.diff_split_cache.is_empty() {
            return;
        }
        self.diff_split_cache_len = self.diff_cache.len();
        self.diff_split_cache = build_patch_split_rows(&self.diff_cache);
    }

    fn diff_scrollbar_markers_patch(&self) -> Vec<components::ScrollbarMarker> {
        match self.diff_view {
            DiffViewMode::Inline => {
                scrollbar_markers_from_flags(self.diff_visible_len(), |visible_ix| {
                    let Some(src_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return 0;
                    };
                    match self.patch_visual_line_kind(src_ix) {
                        worktree_core::domain::DiffLineKind::Add => 1,
                        worktree_core::domain::DiffLineKind::Remove => 2,
                        _ => 0,
                    }
                })
            }
            DiffViewMode::Split => {
                if self.diff_split_row_provider.is_some() && !self.diff_word_wrap {
                    let meta = self.patch_split_visible_meta_from_source();
                    debug_assert_eq!(meta.visible_indices.as_slice(), self.diff_visible_indices);
                    return scrollbar_markers_from_visible_flags(meta.visible_flags.as_slice());
                }
                scrollbar_markers_from_flags(self.diff_visible_len(), |visible_ix| {
                    let Some(row_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return 0;
                    };
                    let Some(row) = self.patch_diff_split_row(row_ix) else {
                        return 0;
                    };
                    match &row {
                        PatchSplitRow::Aligned { .. } => {
                            match self.patch_split_visual_row_kind(&row) {
                                worktree_core::file_diff::FileDiffRowKind::Add => 1,
                                worktree_core::file_diff::FileDiffRowKind::Remove => 2,
                                worktree_core::file_diff::FileDiffRowKind::Modify => 3,
                                worktree_core::file_diff::FileDiffRowKind::Context => 0,
                            }
                        }
                        PatchSplitRow::Raw { .. } => 0,
                    }
                })
            }
        }
    }

    fn diff_scrollbar_markers_collapsed(&self) -> Vec<components::ScrollbarMarker> {
        let ranges = self
            .collapsed_diff_hunks
            .iter()
            .enumerate()
            .filter_map(|(hunk_ix, hunk)| {
                let flag = Self::collapsed_diff_hunk_marker_flag(*hunk);
                let (start, end) = self.collapsed_diff_hunk_visible_file_bounds(hunk_ix, *hunk)?;
                Some((start, end, flag))
            })
            .collect::<Vec<_>>();
        if self.diff_word_wrap {
            return scrollbar_markers_from_flags(self.diff_visible_len(), |visible_ix| {
                let source_visible_ix = self
                    .diff_source_visible_ix_for_visible_ix(visible_ix)
                    .unwrap_or(visible_ix);
                ranges
                    .iter()
                    .find_map(|(start, end, flag)| {
                        (source_visible_ix >= *start && source_visible_ix < *end).then_some(*flag)
                    })
                    .unwrap_or(0)
            });
        }
        scrollbar_markers_from_visible_ranges(self.diff_visible_len(), ranges)
    }

    pub(in crate::view) fn compute_diff_scrollbar_markers(
        &self,
    ) -> Vec<components::ScrollbarMarker> {
        if self.is_collapsed_diff_projection_active() {
            return self.diff_scrollbar_markers_collapsed();
        }

        if !self.is_file_diff_view_active() {
            return self.diff_scrollbar_markers_patch();
        }

        match self.diff_view {
            DiffViewMode::Inline => {
                if let Some(provider) = self.file_diff_inline_row_provider.as_ref()
                    && !self.diff_word_wrap
                {
                    return provider.scrollbar_markers();
                }
                scrollbar_markers_from_flags(self.diff_visible_len(), |visible_ix| {
                    let Some(inline_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return 0;
                    };
                    match self.file_diff_inline_visual_kind(inline_ix) {
                        worktree_core::domain::DiffLineKind::Add => 1,
                        worktree_core::domain::DiffLineKind::Remove => 2,
                        _ => 0,
                    }
                })
            }
            DiffViewMode::Split => {
                if let Some(provider) = self.file_diff_row_provider.as_ref()
                    && !self.diff_word_wrap
                {
                    return provider.scrollbar_markers();
                }
                scrollbar_markers_from_flags(self.diff_visible_len(), |visible_ix| {
                    let Some(row_ix) = self.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return 0;
                    };
                    match self.file_diff_split_visual_kind(row_ix) {
                        worktree_core::file_diff::FileDiffRowKind::Add => 1,
                        worktree_core::file_diff::FileDiffRowKind::Remove => 2,
                        worktree_core::file_diff::FileDiffRowKind::Modify => 3,
                        worktree_core::file_diff::FileDiffRowKind::Context => 0,
                    }
                })
            }
        }
    }

    pub(in super::super::super::super) fn ensure_diff_visible_indices(&mut self) {
        let is_file_view = self.is_file_diff_view_active();
        let collapsed_projection_active = self.is_collapsed_diff_projection_active();
        let projection_rev = if collapsed_projection_active {
            self.diff_visible_projection_rev
        } else {
            0
        };
        let needs_collapsed_rebuild = collapsed_projection_active
            && (self.diff_visible_cache_projection_rev != projection_rev
                || self.diff_visible_view != self.diff_view
                || self.diff_visible_is_file_view != is_file_view);
        if needs_collapsed_rebuild {
            self.rebuild_collapsed_diff_projection();
        }

        let current_len = if collapsed_projection_active {
            self.collapsed_diff_visible_rows.len()
        } else if is_file_view {
            match self.diff_view {
                DiffViewMode::Inline => self.file_diff_inline_row_len(),
                DiffViewMode::Split => self.file_diff_split_row_len(),
            }
        } else {
            match self.diff_view {
                DiffViewMode::Inline => self.patch_diff_row_len(),
                DiffViewMode::Split => self.patch_diff_split_row_len(),
            }
        };

        if self.diff_visible_cache_len == current_len
            && self.diff_visible_view == self.diff_view
            && self.diff_visible_is_file_view == is_file_view
            && self.diff_visible_cache_projection_rev == projection_rev
        {
            return;
        }

        let preserve_horizontal_width = collapsed_projection_active
            && self.diff_visible_cache_projection_rev != u64::MAX
            && self.diff_visible_view == self.diff_view
            && self.diff_visible_is_file_view == is_file_view;

        self.diff_visible_cache_len = current_len;
        self.diff_visible_view = self.diff_view;
        self.diff_visible_is_file_view = is_file_view;
        self.diff_visible_cache_projection_rev = projection_rev;
        self.diff_wrap_visible_rows.clear();
        self.diff_wrap_visible_cache_key = None;
        if !preserve_horizontal_width {
            self.reset_diff_horizontal_scroll_state();
        }
        self.diff_visible_inline_map = None;
        self.diff_search_inline_patch_trigram_index = None;

        if collapsed_projection_active {
            self.diff_visible_indices.clear();
            self.diff_scrollbar_markers_cache = self.compute_diff_scrollbar_markers();
            if self.diff_search_has_query() {
                self.diff_search_recompute_matches_for_current_view_preserving_current();
            }
            return;
        }

        if is_file_view {
            self.diff_visible_indices = (0..current_len).collect();
            self.diff_scrollbar_markers_cache = self.compute_diff_scrollbar_markers();
            if self.diff_search_has_query() {
                self.diff_search_recompute_matches_for_current_view_preserving_current();
            }
            return;
        }

        let mut split_visible_flags: Option<Vec<u8>> = None;
        match self.diff_view {
            DiffViewMode::Inline => {
                if self.diff_hide_unified_header_for_src_ix.len() == current_len {
                    self.diff_visible_inline_map = Some(PatchInlineVisibleMap::from_hidden_flags(
                        self.diff_hide_unified_header_for_src_ix.as_slice(),
                    ));
                    self.diff_visible_indices = Vec::new();
                } else {
                    self.diff_visible_indices = self
                        .patch_diff_rows_slice(0, current_len)
                        .into_iter()
                        .enumerate()
                        .filter_map(|(ix, line)| {
                            (!should_hide_unified_diff_header_line(&line)).then_some(ix)
                        })
                        .collect();
                }
            }
            DiffViewMode::Split => {
                if self.diff_split_row_provider.is_some() {
                    let meta = self.patch_split_visible_meta_from_source();
                    debug_assert_eq!(meta.total_rows, current_len);
                    self.diff_visible_indices = meta.visible_indices;
                    split_visible_flags = Some(meta.visible_flags);
                } else {
                    self.ensure_diff_split_cache();

                    self.diff_visible_indices = self
                        .diff_split_cache
                        .iter()
                        .enumerate()
                        .filter_map(|(ix, row)| match row {
                            PatchSplitRow::Raw { src_ix, .. } => self
                                .diff_cache
                                .get(*src_ix)
                                .is_some_and(|line| !should_hide_unified_diff_header_line(line))
                                .then_some(ix),
                            PatchSplitRow::Aligned { .. } => Some(ix),
                        })
                        .collect();
                }
            }
        }

        self.diff_scrollbar_markers_cache = split_visible_flags
            .map(|flags| scrollbar_markers_from_visible_flags(flags.as_slice()))
            .unwrap_or_else(|| self.compute_diff_scrollbar_markers());

        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_for_current_view_preserving_current();
        }
    }
}
