//! `MainPaneView` collapsed-hunk projection: indexing, expansion and header display.

use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
use super::helpers::COLLAPSED_DIFF_REVEAL_STEP;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;

// @split-module: impl_collapsed
impl MainPaneView {
    pub(super) fn collapsed_diff_expansion_kind(
        &self,
        hunk_ix: usize,
    ) -> crate::view::panes::main::CollapsedDiffExpansionKind {
        use crate::view::panes::main::CollapsedDiffExpansionKind;

        let Some(hunk) = self.collapsed_diff_hunks.get(hunk_ix).copied() else {
            return CollapsedDiffExpansionKind::None;
        };

        let hidden_up = self.collapsed_diff_hidden_up_rows(hunk.src_ix);
        if hunk_ix == 0 {
            if hidden_up > 0 {
                CollapsedDiffExpansionKind::Up
            } else {
                CollapsedDiffExpansionKind::None
            }
        } else if hidden_up == 0 {
            CollapsedDiffExpansionKind::None
        } else if hidden_up <= COLLAPSED_DIFF_REVEAL_STEP {
            CollapsedDiffExpansionKind::Short
        } else {
            CollapsedDiffExpansionKind::Both
        }
    }

    pub(super) fn collapsed_diff_hidden_rows_for_expansion_kind(
        &self,
        src_ix: usize,
        expansion_kind: crate::view::panes::main::CollapsedDiffExpansionKind,
    ) -> usize {
        match expansion_kind {
            crate::view::panes::main::CollapsedDiffExpansionKind::Down => {
                self.collapsed_diff_hidden_down_rows(src_ix)
            }
            crate::view::panes::main::CollapsedDiffExpansionKind::Up
            | crate::view::panes::main::CollapsedDiffExpansionKind::Both
            | crate::view::panes::main::CollapsedDiffExpansionKind::Short => {
                self.collapsed_diff_hidden_up_rows(src_ix)
            }
            crate::view::panes::main::CollapsedDiffExpansionKind::None => 0,
        }
    }

    pub(super) fn collapsed_diff_gap_fully_revealed_after_rebuild(&self, hunk_ix: usize) -> bool {
        let Some(current) = self.collapsed_diff_hunks.get(hunk_ix).copied() else {
            return false;
        };
        let Some(next) = self.collapsed_diff_hunks.get(hunk_ix + 1).copied() else {
            return false;
        };

        let gap_len = next
            .base_row_start
            .saturating_sub(current.base_row_end_exclusive);
        if gap_len == 0 {
            return false;
        }

        current
            .reveal_down_lines
            .min(gap_len)
            .saturating_add(next.reveal_up_lines.min(gap_len))
            >= gap_len
    }

    fn collapsed_diff_hunk_index_for_src_ix(&self, src_ix: usize) -> Option<usize> {
        self.collapsed_diff_hunk_ix_by_src_ix.get(&src_ix).copied()
    }

    pub(in crate::view) fn collapsed_diff_hunk_for_src_ix(
        &self,
        src_ix: usize,
    ) -> Option<CollapsedDiffHunk> {
        self.collapsed_diff_hunk_index_for_src_ix(src_ix)
            .and_then(|hunk_ix| self.collapsed_diff_hunks.get(hunk_ix).copied())
    }

    pub(in crate::view) fn collapsed_diff_hidden_up_rows(&self, src_ix: usize) -> usize {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return 0;
        };
        let hunk = self.collapsed_diff_hunks[hunk_ix];
        if hunk_ix == 0 {
            return hunk
                .base_row_start
                .saturating_sub(hunk.reveal_up_lines.min(hunk.base_row_start));
        }

        let prev = self.collapsed_diff_hunks[hunk_ix - 1];
        let gap_len = hunk
            .base_row_start
            .saturating_sub(prev.base_row_end_exclusive);
        let visible = prev
            .reveal_down_lines
            .min(gap_len)
            .saturating_add(hunk.reveal_up_lines.min(gap_len));
        gap_len.saturating_sub(visible.min(gap_len))
    }

    pub(in crate::view) fn collapsed_diff_hidden_down_rows(&self, src_ix: usize) -> usize {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return 0;
        };
        let hunk = self.collapsed_diff_hunks[hunk_ix];
        let (_, _, total_rows) = self.current_file_diff_line_to_row_maps();
        if hunk_ix + 1 >= self.collapsed_diff_hunks.len() {
            return total_rows
                .saturating_sub(hunk.base_row_end_exclusive)
                .saturating_sub(
                    hunk.reveal_down_lines
                        .min(total_rows.saturating_sub(hunk.base_row_end_exclusive)),
                );
        }

        let next = self.collapsed_diff_hunks[hunk_ix + 1];
        let gap_len = next
            .base_row_start
            .saturating_sub(hunk.base_row_end_exclusive);
        let visible = hunk
            .reveal_down_lines
            .min(gap_len)
            .saturating_add(next.reveal_up_lines.min(gap_len));
        gap_len.saturating_sub(visible.min(gap_len))
    }

    fn collapsed_diff_file_row_line_numbers(
        &self,
        row_ix: usize,
    ) -> Option<(Option<u32>, Option<u32>)> {
        match self.diff_view {
            DiffViewMode::Inline => self
                .file_diff_inline_render_data(row_ix)
                .map(|row| (row.old_line, row.new_line)),
            DiffViewMode::Split => self
                .file_diff_split_row(row_ix)
                .map(|row| (row.old_line, row.new_line)),
        }
    }

    pub(super) fn collapsed_diff_dynamic_hunk_range_display(
        &self,
        src_ix: usize,
    ) -> Option<SharedString> {
        fn update_bounds(min: &mut Option<u32>, max: &mut Option<u32>, line: Option<u32>) {
            let Some(line) = line else {
                return;
            };
            *min = Some(min.map_or(line, |current| current.min(line)));
            *max = Some(max.map_or(line, |current| current.max(line)));
        }

        fn format_range(
            prefix: char,
            fallback_start: u32,
            min: Option<u32>,
            max: Option<u32>,
        ) -> String {
            let (start, count) = match (min, max) {
                (Some(min), Some(max)) if max >= min => (min, max.saturating_sub(min) + 1),
                _ => (fallback_start, 0),
            };
            if count == 1 {
                format!("{prefix}{start}")
            } else {
                format!("{prefix}{start},{count}")
            }
        }

        let (_, _, total_rows) = self.current_file_diff_line_to_row_maps();
        let hunk_ix = self.collapsed_diff_hunk_index_for_src_ix(src_ix)?;
        let hunk = self.collapsed_diff_hunks[hunk_ix];
        let has_revealed_above = if hunk_ix == 0 {
            hunk.reveal_up_lines.min(hunk.base_row_start) > 0
        } else {
            let previous = self.collapsed_diff_hunks[hunk_ix - 1];
            let gap_len = hunk
                .base_row_start
                .saturating_sub(previous.base_row_end_exclusive);
            previous.reveal_down_lines.min(gap_len) > 0 || hunk.reveal_up_lines.min(gap_len) > 0
        };
        let has_revealed_below = if hunk_ix + 1 < self.collapsed_diff_hunks.len() {
            let next = self.collapsed_diff_hunks[hunk_ix + 1];
            let gap_len = next
                .base_row_start
                .saturating_sub(hunk.base_row_end_exclusive);
            hunk.reveal_down_lines.min(gap_len) > 0
        } else {
            hunk.reveal_down_lines
                .min(total_rows.saturating_sub(hunk.base_row_end_exclusive))
                > 0
        };
        if !has_revealed_above && !has_revealed_below {
            return None;
        }

        let parsed = self.patch_diff_row(src_ix).and_then(|line| {
            crate::view::diff_utils::parse_unified_hunk_header_for_display(line.text.as_ref())
        })?;
        let mut old_min = None;
        let mut old_max = None;
        let mut new_min = None;
        let mut new_max = None;
        let mut has_revealed_context = false;

        let mut visit_rows = |range: std::ops::Range<usize>,
                              revealed_context: bool,
                              this: &Self| {
            if range.is_empty() {
                return;
            }
            has_revealed_context |= revealed_context;
            for row_ix in range {
                let Some((old_line, new_line)) = this.collapsed_diff_file_row_line_numbers(row_ix)
                else {
                    continue;
                };
                update_bounds(&mut old_min, &mut old_max, old_line);
                update_bounds(&mut new_min, &mut new_max, new_line);
            }
        };

        if hunk_ix == 0 {
            let leading_start = hunk
                .base_row_start
                .saturating_sub(hunk.reveal_up_lines.min(hunk.base_row_start));
            visit_rows(leading_start..hunk.base_row_start, true, self);
        } else {
            let previous = self.collapsed_diff_hunks[hunk_ix - 1];
            let gap_start = previous.base_row_end_exclusive;
            let gap_end = hunk.base_row_start.max(gap_start);
            let gap_len = gap_end.saturating_sub(gap_start);
            let top_end = gap_start.saturating_add(previous.reveal_down_lines.min(gap_len));
            let bottom_start = gap_end.saturating_sub(hunk.reveal_up_lines.min(gap_len));

            visit_rows(gap_start..top_end, true, self);
            visit_rows(bottom_start.max(top_end)..gap_end, true, self);
        }

        visit_rows(
            hunk.base_row_start..hunk.base_row_end_exclusive,
            false,
            self,
        );

        let trailing_end = if hunk_ix + 1 < self.collapsed_diff_hunks.len() {
            let next = self.collapsed_diff_hunks[hunk_ix + 1];
            let gap_len = next
                .base_row_start
                .saturating_sub(hunk.base_row_end_exclusive);
            hunk.base_row_end_exclusive
                .saturating_add(hunk.reveal_down_lines.min(gap_len))
        } else {
            hunk.base_row_end_exclusive
                .saturating_add(
                    hunk.reveal_down_lines
                        .min(total_rows.saturating_sub(hunk.base_row_end_exclusive)),
                )
                .min(total_rows)
        };
        visit_rows(hunk.base_row_end_exclusive..trailing_end, true, self);

        has_revealed_context.then(|| {
            format!(
                "{} {}",
                format_range('-', parsed.old_start_line, old_min, old_max),
                format_range('+', parsed.new_start_line, new_min, new_max)
            )
            .into()
        })
    }

    pub(in crate::view) fn collapsed_diff_hunk_header_display(
        &self,
        src_ix: usize,
    ) -> Option<SharedString> {
        self.collapsed_diff_header_display_cache
            .get(&src_ix)
            .cloned()
            .or_else(|| self.diff_header_display_cache.get(&src_ix).cloned())
            .or_else(|| {
                self.patch_diff_row(src_ix)
                    .map(|line| SharedString::from(line.text.as_ref().to_owned()))
            })
    }

    pub(in crate::view) fn collapsed_diff_reveal_hunk_up(
        &mut self,
        src_ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return;
        };
        let delta = self
            .collapsed_diff_hidden_up_rows(src_ix)
            .min(COLLAPSED_DIFF_REVEAL_STEP);
        if delta == 0 {
            return;
        }
        self.collapsed_diff_hunks[hunk_ix].reveal_up_lines = self.collapsed_diff_hunks[hunk_ix]
            .reveal_up_lines
            .saturating_add(delta);
        self.persist_collapsed_diff_hunk_reveal(hunk_ix);
        if self.collapsed_diff_hidden_up_rows(src_ix) == 0 && hunk_ix > 0 {
            self.merge_collapsed_diff_hunks_up(hunk_ix);
        }
        self.invalidate_collapsed_diff_visible_projection();
        self.ensure_diff_visible_indices();
        cx.notify();
    }

    pub(in crate::view) fn collapsed_diff_reveal_hunk_down(
        &mut self,
        src_ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return;
        };
        let delta = self
            .collapsed_diff_hidden_down_rows(src_ix)
            .min(COLLAPSED_DIFF_REVEAL_STEP);
        if delta == 0 {
            return;
        }
        self.collapsed_diff_hunks[hunk_ix].reveal_down_lines = self.collapsed_diff_hunks[hunk_ix]
            .reveal_down_lines
            .saturating_add(delta);
        self.persist_collapsed_diff_hunk_reveal(hunk_ix);
        if hunk_ix + 1 < self.collapsed_diff_hunks.len()
            && self.collapsed_diff_hidden_down_rows(src_ix) == 0
        {
            self.merge_collapsed_diff_hunks_down(hunk_ix);
        }
        self.invalidate_collapsed_diff_visible_projection();
        self.ensure_diff_visible_indices();
        cx.notify();
    }

    pub(in crate::view) fn collapsed_diff_reveal_hunk_down_before(
        &mut self,
        src_ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return;
        };
        if hunk_ix == 0 {
            return;
        }
        let previous_hunk_ix = hunk_ix - 1;
        let previous_src_ix = self.collapsed_diff_hunks[previous_hunk_ix].src_ix;
        let delta = self
            .collapsed_diff_hidden_down_rows(previous_src_ix)
            .min(COLLAPSED_DIFF_REVEAL_STEP);
        if delta == 0 {
            return;
        }
        self.collapsed_diff_hunks[previous_hunk_ix].reveal_down_lines = self.collapsed_diff_hunks
            [previous_hunk_ix]
            .reveal_down_lines
            .saturating_add(delta);
        self.persist_collapsed_diff_hunk_reveal(previous_hunk_ix);
        if previous_hunk_ix + 1 < self.collapsed_diff_hunks.len()
            && self.collapsed_diff_hidden_down_rows(previous_src_ix) == 0
        {
            self.merge_collapsed_diff_hunks_down(previous_hunk_ix);
        }
        self.invalidate_collapsed_diff_visible_projection();
        self.ensure_diff_visible_indices();
        cx.notify();
    }

    pub(in crate::view) fn collapsed_diff_reveal_hunk_short(
        &mut self,
        src_ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(hunk_ix) = self.collapsed_diff_hunk_index_for_src_ix(src_ix) else {
            return;
        };
        if hunk_ix == 0 {
            return;
        }
        let delta = self.collapsed_diff_hidden_up_rows(src_ix);
        if delta == 0 {
            return;
        }
        self.collapsed_diff_hunks[hunk_ix].reveal_up_lines = self.collapsed_diff_hunks[hunk_ix]
            .reveal_up_lines
            .saturating_add(delta);
        self.persist_collapsed_diff_hunk_reveal(hunk_ix);
        self.merge_collapsed_diff_hunks_up(hunk_ix);
        self.invalidate_collapsed_diff_visible_projection();
        self.ensure_diff_visible_indices();
        cx.notify();
    }

    pub(super) fn collapsed_diff_hunk_marker_flag(hunk: CollapsedDiffHunk) -> u8 {
        match (hunk.has_additions, hunk.has_removals) {
            (true, true) => 3,
            (true, false) => 1,
            (false, true) => 2,
            (false, false) => 0,
        }
    }

    pub(super) fn collapsed_diff_hunk_visible_file_bounds(
        &self,
        hunk_ix: usize,
        hunk: CollapsedDiffHunk,
    ) -> Option<(usize, usize)> {
        let mut visible_ix = *self.collapsed_diff_hunk_visible_indices.get(hunk_ix)?;
        while let Some(row) = self.collapsed_diff_visible_rows.get(visible_ix).copied() {
            match row {
                CollapsedDiffVisibleRow::HunkHeader { .. } => visible_ix += 1,
                CollapsedDiffVisibleRow::FileRow { row_ix } if row_ix < hunk.base_row_start => {
                    visible_ix += 1;
                }
                CollapsedDiffVisibleRow::FileRow { row_ix } if row_ix == hunk.base_row_start => {
                    let end_ix = visible_ix
                        .saturating_add(
                            hunk.base_row_end_exclusive
                                .saturating_sub(hunk.base_row_start),
                        )
                        .min(self.collapsed_diff_visible_rows.len());
                    return (visible_ix < end_ix).then_some((visible_ix, end_ix));
                }
                CollapsedDiffVisibleRow::FileRow { .. } => return None,
            }
        }
        None
    }
}
