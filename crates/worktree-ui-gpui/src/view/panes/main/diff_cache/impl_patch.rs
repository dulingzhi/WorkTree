//! `MainPaneView` patch diff rows and their visual line kinds.

use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use super::patch_diff::{PatchSplitVisibleMeta, build_patch_split_visible_meta_from_src};
use worktree_core::domain::DiffRowProvider;

// @split-module: impl_patch
impl MainPaneView {
    pub(in crate::view) fn patch_diff_row_len(&self) -> usize {
        self.diff_row_provider
            .as_ref()
            .map(|provider| provider.len_hint())
            .unwrap_or_else(|| self.diff_cache.len())
    }

    pub(in crate::view) fn patch_diff_row(&self, src_ix: usize) -> Option<AnnotatedDiffLine> {
        if let Some(provider) = self.diff_row_provider.as_ref() {
            provider.row(src_ix)
        } else {
            self.diff_cache.get(src_ix).cloned()
        }
    }

    pub(in crate::view) fn patch_visual_line_kind(
        &self,
        src_ix: usize,
    ) -> worktree_core::domain::DiffLineKind {
        self.diff_visual_line_kind_for_src_ix
            .get(src_ix)
            .copied()
            .or_else(|| self.diff_line_kind_for_src_ix.get(src_ix).copied())
            .or_else(|| self.patch_diff_row(src_ix).map(|line| line.kind))
            .unwrap_or(worktree_core::domain::DiffLineKind::Context)
    }

    pub(in crate::view) fn patch_split_visual_row_kind(
        &self,
        row: &PatchSplitRow,
    ) -> worktree_core::file_diff::FileDiffRowKind {
        use worktree_core::domain::DiffLineKind as DK;
        use worktree_core::file_diff::FileDiffRowKind as RK;

        let PatchSplitRow::Aligned {
            row,
            old_src_ix,
            new_src_ix,
        } = row
        else {
            return RK::Context;
        };

        let old_changed = old_src_ix
            .is_some_and(|src_ix| matches!(self.patch_visual_line_kind(src_ix), DK::Remove));
        let new_changed =
            new_src_ix.is_some_and(|src_ix| matches!(self.patch_visual_line_kind(src_ix), DK::Add));

        match (old_changed, new_changed) {
            (true, true) => RK::Modify,
            (true, false) => RK::Remove,
            (false, true) => RK::Add,
            (false, false) => {
                if matches!(row.kind, RK::Add | RK::Remove | RK::Modify) {
                    RK::Context
                } else {
                    row.kind
                }
            }
        }
    }

    pub(in crate::view) fn patch_diff_rows_slice(
        &self,
        start: usize,
        end: usize,
    ) -> Vec<AnnotatedDiffLine> {
        if let Some(provider) = self.diff_row_provider.as_ref() {
            provider.slice(start, end).collect()
        } else {
            let end = end.min(self.diff_cache.len());
            if start >= end {
                Vec::new()
            } else {
                self.diff_cache[start..end].to_vec()
            }
        }
    }

    pub(in crate::view) fn patch_diff_split_row_len(&self) -> usize {
        self.diff_split_row_provider
            .as_ref()
            .map(|provider| provider.len_hint())
            .unwrap_or_else(|| self.diff_split_cache.len())
    }

    pub(in crate::view) fn patch_diff_split_row(&self, row_ix: usize) -> Option<PatchSplitRow> {
        if let Some(provider) = self.diff_split_row_provider.as_ref() {
            provider.row(row_ix)
        } else {
            self.diff_split_cache.get(row_ix).cloned()
        }
    }

    pub(super) fn patch_split_visible_meta_from_source(&self) -> PatchSplitVisibleMeta {
        build_patch_split_visible_meta_from_src(
            self.diff_line_kind_for_src_ix.as_slice(),
            self.diff_visual_line_kind_for_src_ix.as_slice(),
            self.diff_click_kinds.as_slice(),
            self.diff_hide_unified_header_for_src_ix.as_slice(),
        )
    }
}
