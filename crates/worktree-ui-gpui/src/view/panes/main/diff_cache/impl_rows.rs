//! `MainPaneView` file-diff row construction for the split and inline bodies.

use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use crate::view::panes::main::diff_cache::file_diff;
use crate::view::rows;
use worktree_core::domain::DiffRowProvider;

// @split-module: impl_rows
impl MainPaneView {
    pub(in crate::view) fn file_diff_split_row_len(&self) -> usize {
        self.file_diff_row_provider
            .as_ref()
            .map(|provider| provider.len_hint())
            .unwrap_or_else(|| self.file_diff_cache_rows.len())
    }

    pub(in crate::view) fn file_diff_split_row(&self, row_ix: usize) -> Option<FileDiffRow> {
        if let Some(provider) = self.file_diff_row_provider.as_ref() {
            provider.row(row_ix)
        } else {
            self.file_diff_cache_rows.get(row_ix).cloned()
        }
    }

    pub(in crate::view) fn file_diff_split_render_data(
        &self,
        row_ix: usize,
    ) -> Option<FileDiffRow> {
        if let Some(provider) = self.file_diff_row_provider.as_ref() {
            provider.render_data(row_ix)
        } else {
            self.file_diff_cache_rows.get(row_ix).cloned()
        }
    }

    pub(in crate::view) fn file_diff_split_visual_kind(
        &self,
        row_ix: usize,
    ) -> worktree_core::file_diff::FileDiffRowKind {
        self.file_diff_row_provider
            .as_ref()
            .and_then(|provider| provider.visual_kind(row_ix))
            .or_else(|| self.file_diff_cache_rows.get(row_ix).map(|row| row.kind))
            .unwrap_or(worktree_core::file_diff::FileDiffRowKind::Context)
    }

    pub(in crate::view) fn file_diff_inline_row_len(&self) -> usize {
        self.file_diff_inline_row_provider
            .as_ref()
            .map(|provider| provider.len_hint())
            .unwrap_or_else(|| self.file_diff_inline_cache.len())
    }

    pub(in crate::view) fn file_diff_inline_row(
        &self,
        inline_ix: usize,
    ) -> Option<AnnotatedDiffLine> {
        if let Some(provider) = self.file_diff_inline_row_provider.as_ref() {
            provider.row(inline_ix)
        } else {
            self.file_diff_inline_cache.get(inline_ix).cloned()
        }
    }

    pub(in crate::view) fn file_diff_inline_render_data(
        &self,
        inline_ix: usize,
    ) -> Option<self::file_diff::InlineFileDiffRowRenderData> {
        if let Some(provider) = self.file_diff_inline_row_provider.as_ref() {
            provider.render_data(inline_ix)
        } else {
            let line = self.file_diff_inline_cache.get(inline_ix)?.clone();
            Some(self::file_diff::InlineFileDiffRowRenderData {
                kind: line.kind,
                old_line: line.old_line,
                new_line: line.new_line,
                text: crate::view::diff_utils::diff_content_line_text(&line),
            })
        }
    }

    pub(in crate::view) fn file_diff_inline_visual_kind(
        &self,
        inline_ix: usize,
    ) -> worktree_core::domain::DiffLineKind {
        self.file_diff_inline_row_provider
            .as_ref()
            .and_then(|provider| provider.visual_kind(inline_ix))
            .or_else(|| {
                self.file_diff_inline_cache
                    .get(inline_ix)
                    .map(|row| row.kind)
            })
            .unwrap_or(worktree_core::domain::DiffLineKind::Context)
    }

    pub(in crate::view) fn file_diff_split_modify_pair_texts(
        &self,
        row_ix: usize,
    ) -> Option<(
        worktree_core::file_diff::FileDiffLineText,
        worktree_core::file_diff::FileDiffLineText,
    )> {
        self.file_diff_row_provider
            .as_ref()
            .and_then(|provider| provider.modify_pair_texts(row_ix))
    }

    pub(in crate::view) fn file_diff_inline_modify_pair_texts(
        &self,
        inline_ix: usize,
    ) -> Option<(
        worktree_core::file_diff::FileDiffLineText,
        worktree_core::file_diff::FileDiffLineText,
        worktree_core::domain::DiffLineKind,
    )> {
        self.file_diff_inline_row_provider
            .as_ref()
            .and_then(|provider| provider.modify_pair_texts(inline_ix))
    }

    pub(in crate::view) fn file_diff_split_style_cache_epoch(&self, region: DiffTextRegion) -> u64 {
        self.file_diff_style_cache_epochs.split_epoch(region)
    }

    pub(in crate::view) fn file_diff_inline_style_cache_epoch(
        &self,
        line: &AnnotatedDiffLine,
    ) -> u64 {
        self.file_diff_style_cache_epochs.inline_epoch(line.kind)
    }

    /// Project inline-diff syntax from the real old/new (split) documents.
    ///
    /// Instead of parsing the synthetic mixed inline stream, project each row into
    /// the correct real old/new document using its 1-based diff line numbers.
    pub(in crate::view) fn file_diff_inline_projected_syntax(
        &self,
        line: &AnnotatedDiffLine,
    ) -> rows::PreparedDiffSyntaxLine {
        rows::prepared_diff_syntax_line_for_inline_diff_row(
            self.file_diff_split_prepared_syntax_document(DiffTextRegion::SplitLeft),
            self.file_diff_split_prepared_syntax_document(DiffTextRegion::SplitRight),
            line,
        )
    }

    pub(in crate::view) fn file_diff_split_prepared_syntax_document(
        &self,
        region: DiffTextRegion,
    ) -> Option<rows::PreparedDiffSyntaxDocument> {
        let view_mode = match region {
            DiffTextRegion::SplitLeft => PreparedSyntaxViewMode::FileDiffSplitLeft,
            DiffTextRegion::SplitRight | DiffTextRegion::Inline => {
                PreparedSyntaxViewMode::FileDiffSplitRight
            }
        };
        self.file_diff_prepared_syntax_document(view_mode)
    }
}
