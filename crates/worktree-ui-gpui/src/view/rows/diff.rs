use super::diff_canvas;
use super::diff_text::*;
use super::*;
use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};
use crate::view::panes::main::{
    CollapsedDiffExpansionKind, CollapsedDiffHunk, CollapsedDiffVisibleRow,
    DiffHorizontalScrollColumn,
};
use crate::view::panes::main::{
    VersionedCachedDiffStyledText, versioned_query_cached_diff_styled_text_is_current,
};
use worktree_core::domain::DiffLineKind;
use worktree_core::file_diff::FileDiffRowKind;

mod blame;
mod collapsed_hunk;
mod rows;

pub(in crate::view) use self::blame::{BlameRenderCtx, build_row_blame_paint};
pub(in crate::view) use self::rows::should_hide_unified_diff_header_line;
// Re-exported at the original `rows::diff::PatchSplitColumn` path and scope
// (`pub(in rows)`) for the `super::diff::PatchSplitColumn` consumer in
// `diff_canvas.rs`.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PatchSplitColumn {
    Left,
    Right,
}

fn diff_row_height(ui_scale_percent: u32) -> Pixels {
    crate::view::panes::main::diff_row_height_for_ui_scale(ui_scale_percent)
}

fn diff_file_header_height(ui_scale_percent: u32) -> Pixels {
    crate::view::panes::main::diff_file_header_height_for_ui_scale(ui_scale_percent)
}

fn diff_hunk_header_height(ui_scale_percent: u32) -> Pixels {
    crate::view::panes::main::diff_hunk_header_height_for_ui_scale(ui_scale_percent)
}

fn focused_diff_neutral_row_bg(theme: AppTheme) -> gpui::Rgba {
    with_alpha(
        theme.colors.foreground.secondary,
        if theme.is_dark { 0.26 } else { 0.16 },
    )
}

fn diff_placeholder_row(
    id: impl Into<gpui::ElementId>,
    theme: AppTheme,
    ui_scale_percent: u32,
) -> AnyElement {
    div()
        .id(id)
        .h(diff_row_height(ui_scale_percent))
        .px_2()
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child("")
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::blame::{
        BlamePrev, LocalChange, build_row_blame_paint_inner, build_row_blame_paint_tracked,
        classify_local_change,
    };
    use super::collapsed_hunk::{
        collapsed_hunk_header_bg, collapsed_hunk_header_row_height, collapsed_inline_hunk_bg,
        collapsed_inline_hunk_fg, collapsed_split_hunk_bg, collapsed_split_hunk_fg,
        focused_collapsed_hunk_bg,
    };
    use super::rows::{coverage_gutter_color, focused_diff_line_bg};

    fn collapsed_hunk(has_removals: bool, has_additions: bool) -> CollapsedDiffHunk {
        CollapsedDiffHunk {
            src_ix: 0,
            base_row_start: 0,
            base_row_end_exclusive: 1,
            has_additions,
            has_removals,
            reveal_up_lines: 0,
            reveal_down_lines: 0,
        }
    }

    fn uncommitted_blame_line() -> worktree_core::services::BlameLine {
        worktree_core::services::BlameLine {
            commit_id: std::sync::Arc::from("0000000000000000000000000000000000000000"),
            author: std::sync::Arc::from("Not Committed Yet"),
            author_time_unix: None,
            summary: std::sync::Arc::from("Not Committed Yet"),
            body: None,
            line: String::new(),
            prior_exists: false,
            source_path: None,
            prior_commit: None,
        }
    }

    fn committed_blame_line(sha: &str) -> worktree_core::services::BlameLine {
        worktree_core::services::BlameLine {
            commit_id: std::sync::Arc::from(sha),
            author: std::sync::Arc::from("Jane Doe"),
            author_time_unix: Some(1_700_000_000),
            summary: std::sync::Arc::from("a commit"),
            body: None,
            line: String::new(),
            prior_exists: true,
            source_path: None,
            prior_commit: None,
        }
    }

    fn blame_ctx(
        lines: Vec<worktree_core::services::BlameLine>,
        area: Option<worktree_core::domain::DiffArea>,
    ) -> BlameRenderCtx {
        BlameRenderCtx {
            lines: std::sync::Arc::new(lines),
            range: None,
            now: std::time::SystemTime::now(),
            path: std::sync::Arc::from(std::path::Path::new("file.rs")),
            viewed_commit: None,
            area,
        }
    }

    #[test]
    fn classify_local_change_matrix() {
        use worktree_core::domain::DiffArea;
        // Staged area: every local line is staged regardless of context-ness.
        assert_eq!(
            classify_local_change(DiffArea::Staged, true),
            LocalChange::Staged
        );
        assert_eq!(
            classify_local_change(DiffArea::Staged, false),
            LocalChange::Staged
        );
        // Unstaged area: an unchanged context line is a staged change; any actual
        // change (add / modify / remove) is an unstaged change.
        assert_eq!(
            classify_local_change(DiffArea::Unstaged, true),
            LocalChange::Staged
        );
        assert_eq!(
            classify_local_change(DiffArea::Unstaged, false),
            LocalChange::Unstaged
        );
    }

    #[test]
    fn unstaged_diff_separates_staged_context_from_unstaged_add() {
        use worktree_core::domain::DiffArea;
        let theme = AppTheme::worktree_dark();
        let ctx = blame_ctx(
            vec![uncommitted_blame_line(), uncommitted_blame_line()],
            Some(DiffArea::Unstaged),
        );
        // Uncommitted context line -> staged: green diff-add bar.
        let (staged, _) =
            build_row_blame_paint_inner(&ctx, true, Some(1), Some(1), BlamePrev::default(), theme)
                .unwrap();
        assert_eq!(staged.border, theme.colors.diff.added.foreground);
        assert_eq!(staged.when.as_ref(), "Staged");
        // Added line -> unstaged: red diff-remove bar.
        let (unstaged, _) =
            build_row_blame_paint_inner(&ctx, false, None, Some(2), BlamePrev::default(), theme)
                .unwrap();
        assert_eq!(unstaged.border, theme.colors.diff.removed.foreground);
        assert_eq!(unstaged.when.as_ref(), "Unstaged");
    }

    #[test]
    fn modified_line_with_both_sides_is_unstaged_not_staged() {
        use worktree_core::domain::DiffArea;
        let theme = AppTheme::worktree_dark();
        let ctx = blame_ctx(vec![uncommitted_blame_line()], Some(DiffArea::Unstaged));
        // A split `Modify` row has both old and new line numbers, but it is a
        // change, not unchanged context. Unstaged must override staged here.
        let (paint, _) =
            build_row_blame_paint_inner(&ctx, false, Some(1), Some(1), BlamePrev::default(), theme)
                .unwrap();
        assert_eq!(paint.border, theme.colors.diff.removed.foreground);
        assert_eq!(paint.when.as_ref(), "Unstaged");
    }

    #[test]
    fn staged_area_labels_all_local_as_staged() {
        use worktree_core::domain::DiffArea;
        let theme = AppTheme::worktree_dark();
        let ctx = blame_ctx(vec![uncommitted_blame_line()], Some(DiffArea::Staged));
        // Even an added line is "Staged" when viewing the staged area.
        let (paint, _) =
            build_row_blame_paint_inner(&ctx, false, None, Some(1), BlamePrev::default(), theme)
                .unwrap();
        assert_eq!(paint.border, theme.colors.diff.added.foreground);
        assert_eq!(paint.when.as_ref(), "Staged");
    }

    #[test]
    fn revision_blame_keeps_now_label() {
        let theme = AppTheme::worktree_dark();
        // No working-tree area (revision blame) -> legacy generic local change.
        let ctx = blame_ctx(vec![uncommitted_blame_line()], None);
        let (paint, _) =
            build_row_blame_paint_inner(&ctx, true, Some(1), Some(1), BlamePrev::default(), theme)
                .unwrap();
        assert_eq!(
            paint.border,
            crate::theme::blame_local_change_color(theme.is_dark)
        );
        assert_eq!(paint.when.as_ref(), "Now");
    }

    #[test]
    fn run_breaks_at_staged_unstaged_boundary() {
        use worktree_core::domain::DiffArea;
        let theme = AppTheme::worktree_dark();
        let ctx = blame_ctx(
            vec![
                uncommitted_blame_line(),
                uncommitted_blame_line(),
                uncommitted_blame_line(),
            ],
            Some(DiffArea::Unstaged),
        );
        let prev = std::cell::Cell::new(BlamePrev::default());
        // Line 1: staged context -> run start, label shown.
        let p1 = build_row_blame_paint_tracked(&ctx, true, Some(1), Some(1), &prev, None, theme)
            .unwrap();
        assert!(p1.show_text);
        assert_eq!(p1.when.as_ref(), "Staged");
        // Line 2: staged context, contiguous -> not a run start (label not repeated).
        let p2 = build_row_blame_paint_tracked(&ctx, true, Some(2), Some(2), &prev, None, theme)
            .unwrap();
        assert!(!p2.show_text);
        // Line 3: unstaged add. The new-side line is still contiguous, but the
        // staged->unstaged group change must start a new run with its label.
        let p3 =
            build_row_blame_paint_tracked(&ctx, false, None, Some(3), &prev, None, theme).unwrap();
        assert!(p3.show_text);
        assert_eq!(p3.when.as_ref(), "Unstaged");
    }

    #[test]
    fn wrapped_continuation_rows_do_not_repeat_annotation_text() {
        // Regression: when a long line wraps, each wrapped visual row carries the
        // same logical new-side line, so `is_run_start` is recomputed as true for
        // the continuation rows. Without the wrap_ix gate the time/author/summary
        // label is duplicated down every wrapped line. Continuation rows must keep
        // their recency border but suppress the repeated text.
        let theme = AppTheme::worktree_dark();
        let sha = "1111111111111111111111111111111111111111";
        let ctx = blame_ctx(
            vec![committed_blame_line(sha)],
            Some(worktree_core::domain::DiffArea::Unstaged),
        );
        let prev = std::cell::Cell::new(BlamePrev::default());
        let wrap = |wrap_ix: usize| {
            Some(diff_canvas::DiffTextWrapSlice {
                wrap_ix,
                wrap_columns: 80,
                primary_range: diff_canvas::DiffWrapByteRange::default(),
                secondary_range: diff_canvas::DiffWrapByteRange::default(),
            })
        };

        // First visual row of the wrapped line (wrap_ix == 0): run start, label shown.
        let first =
            build_row_blame_paint_tracked(&ctx, false, None, Some(1), &prev, wrap(0), theme)
                .unwrap();
        assert!(first.show_text, "the wrap_ix == 0 row shows the annotation");
        let border = first.border;

        // Continuation rows (wrap_ix > 0) of the SAME line: border kept, text hidden.
        for wrap_ix in 1..=2 {
            let cont = build_row_blame_paint_tracked(
                &ctx,
                false,
                None,
                Some(1),
                &prev,
                wrap(wrap_ix),
                theme,
            )
            .unwrap();
            assert!(
                !cont.show_text,
                "wrap_ix == {wrap_ix} continuation row must not repeat the annotation text"
            );
            assert_eq!(cont.border, border, "the recency bar stays continuous");
        }
    }

    #[test]
    fn untracked_content_view_collapses_consecutive_same_commit_lines() {
        // The full file-content view uses the untracked `build_row_blame_paint`
        // (no threaded group). Consecutive lines of the same commit must collapse
        // into one run: the message shows on the first line only, not repeated.
        let theme = AppTheme::worktree_dark();
        let sha = "1111111111111111111111111111111111111111";
        let ctx = blame_ctx(
            vec![committed_blame_line(sha), committed_blame_line(sha)],
            Some(worktree_core::domain::DiffArea::Unstaged),
        );
        // Line 1 (no previous rendered line) starts the run -> shows the summary.
        let first = build_row_blame_paint(&ctx, false, None, Some(1), None, theme).unwrap();
        assert!(first.show_text);
        assert_eq!(first.summary.as_ref(), "a commit");
        // Line 2 is contiguous and same commit -> not a run start, no repeated text.
        let second = build_row_blame_paint(&ctx, false, None, Some(2), Some(1), theme).unwrap();
        assert!(!second.show_text);
        assert!(second.when.as_ref().is_empty());
        assert!(second.summary.as_ref().is_empty());
    }

    #[test]
    fn removal_rows_get_a_local_bar_only_in_working_tree_blame() {
        use worktree_core::domain::DiffArea;
        let theme = AppTheme::worktree_dark();
        // Pure removal (old side only) in the unstaged area -> unstaged bar + label,
        // even though there is no `BlameLine` for a deleted line.
        let ctx = blame_ctx(Vec::new(), Some(DiffArea::Unstaged));
        let prev = std::cell::Cell::new(BlamePrev::default());
        let removal =
            build_row_blame_paint_tracked(&ctx, false, Some(5), None, &prev, None, theme).unwrap();
        assert_eq!(removal.border, theme.colors.diff.removed.foreground);
        assert_eq!(removal.when.as_ref(), "Unstaged");
        // Revision blame has no staged/unstaged concept, so removals get no bar.
        let ctx_rev = blame_ctx(Vec::new(), None);
        let prev_rev = std::cell::Cell::new(BlamePrev::default());
        assert!(
            build_row_blame_paint_tracked(&ctx_rev, false, Some(5), None, &prev_rev, None, theme)
                .is_none()
        );
    }

    #[test]
    fn focused_diff_row_backgrounds_are_semantic_and_not_text_selection() {
        for theme in [AppTheme::worktree_dark(), AppTheme::worktree_light()] {
            let text_selection_bg = with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.28 } else { 0.18 },
            );
            let add_focus = focused_diff_line_bg(theme, DiffLineKind::Add);
            let remove_focus = focused_diff_line_bg(theme, DiffLineKind::Remove);
            let neutral_focus = focused_diff_line_bg(theme, DiffLineKind::Context);
            let (add_bg, _, _) = diff_line_colors(theme, DiffLineKind::Add);
            let (remove_bg, _, _) = diff_line_colors(theme, DiffLineKind::Remove);
            let (context_bg, _, _) = diff_line_colors(theme, DiffLineKind::Context);

            // The focused row is the diff palette's own token, not a tint mixed
            // from the status palette: those greens and reds differ in every
            // bundled theme, so deriving it there shifted the row's hue the
            // moment it took focus.
            assert_eq!(add_focus, theme.colors.diff.added.focused_background);
            assert_eq!(remove_focus, theme.colors.diff.removed.focused_background);

            assert_ne!(add_focus, text_selection_bg);
            assert_ne!(remove_focus, text_selection_bg);
            assert_ne!(neutral_focus, text_selection_bg);
            assert_ne!(neutral_focus, theme.colors.diff.modified.focused_background);
            assert_ne!(add_focus, add_bg);
            assert_ne!(remove_focus, remove_bg);
            assert_ne!(neutral_focus, context_bg);
            assert_ne!(add_focus, remove_focus);
            assert_ne!(add_focus, neutral_focus);
            assert_ne!(remove_focus, neutral_focus);

            let collapsed_focus = focused_collapsed_hunk_bg(theme, None);
            let expected_collapsed_focus = with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.22 } else { 0.16 },
            );
            assert_eq!(collapsed_focus, expected_collapsed_focus);
            assert_ne!(collapsed_focus, add_focus);
            assert_ne!(collapsed_focus, remove_focus);
            assert_ne!(collapsed_focus, neutral_focus);
            assert_ne!(collapsed_focus, text_selection_bg);
            assert_eq!(
                focused_collapsed_hunk_bg(theme, Some(collapsed_hunk(false, true))),
                collapsed_focus
            );
            assert_eq!(
                focused_collapsed_hunk_bg(theme, Some(collapsed_hunk(true, false))),
                collapsed_focus
            );
            assert_eq!(
                focused_collapsed_hunk_bg(theme, Some(collapsed_hunk(true, true))),
                collapsed_focus
            );
            assert_eq!(focused_collapsed_hunk_bg(theme, None), collapsed_focus);
        }
    }

    #[test]
    fn collapsed_hunk_headers_use_uniform_diff_row_height() {
        for ui_scale_percent in [75, 100, 125, 150, 200] {
            assert_eq!(
                collapsed_hunk_header_row_height(ui_scale_percent),
                diff_row_height(ui_scale_percent)
            );
            assert_ne!(
                collapsed_hunk_header_row_height(ui_scale_percent),
                diff_hunk_header_height(ui_scale_percent)
            );
        }
    }

    #[test]
    fn collapsed_inline_hunk_headers_use_neutral_colors() {
        for theme in [AppTheme::worktree_dark(), AppTheme::worktree_light()] {
            let neutral = collapsed_hunk_header_bg(theme);

            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, false)),
                    CollapsedDiffExpansionKind::Up,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(false, true)),
                    CollapsedDiffExpansionKind::Up,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    CollapsedDiffExpansionKind::Up,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    CollapsedDiffExpansionKind::Both,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    CollapsedDiffExpansionKind::Short,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    CollapsedDiffExpansionKind::Down,
                ),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_bg(theme, None, CollapsedDiffExpansionKind::Up),
                neutral
            );
            assert_eq!(
                collapsed_inline_hunk_fg(theme, Some(collapsed_hunk(true, false))),
                theme.colors.foreground.secondary
            );
            assert_eq!(
                collapsed_inline_hunk_fg(theme, Some(collapsed_hunk(false, true))),
                theme.colors.foreground.secondary
            );
            assert_eq!(
                collapsed_inline_hunk_fg(theme, Some(collapsed_hunk(true, true))),
                theme.colors.foreground.secondary
            );
            assert_eq!(
                collapsed_inline_hunk_fg(theme, None),
                theme.colors.foreground.secondary
            );
        }
    }

    #[test]
    fn collapsed_split_hunk_headers_use_neutral_colors_for_both_sides() {
        for theme in [AppTheme::worktree_dark(), AppTheme::worktree_light()] {
            let neutral = collapsed_hunk_header_bg(theme);

            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, false)),
                    PatchSplitColumn::Left,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, false)),
                    PatchSplitColumn::Right,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(false, true)),
                    PatchSplitColumn::Left,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(false, true)),
                    PatchSplitColumn::Right,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    PatchSplitColumn::Left,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(
                    theme,
                    Some(collapsed_hunk(true, true)),
                    PatchSplitColumn::Right,
                ),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(theme, None, PatchSplitColumn::Left),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_bg(theme, None, PatchSplitColumn::Right),
                neutral
            );
            assert_eq!(
                collapsed_split_hunk_fg(theme, PatchSplitColumn::Left),
                theme.colors.foreground.secondary
            );
            assert_eq!(
                collapsed_split_hunk_fg(theme, PatchSplitColumn::Right),
                theme.colors.foreground.secondary
            );
        }
    }

    #[test]
    fn coverage_gutter_color_marks_hit_and_miss_lines_only() {
        let theme = AppTheme::worktree_dark();
        let report = worktree_core::coverage::CoverageReport::parse_lcov(
            "SF:src/lib.rs\nDA:1,2\nDA:2,0\nend_of_record\n",
        )
        .unwrap();

        assert_eq!(
            coverage_gutter_color(theme, &report, "src/lib.rs", Some(1)),
            Some(theme.colors.status.success.foreground),
            "a covered line reads green in the gutter"
        );
        assert_eq!(
            coverage_gutter_color(theme, &report, "src/lib.rs", Some(2)),
            Some(theme.colors.status.danger.foreground),
            "a missed line reads red in the gutter"
        );
        assert_eq!(
            coverage_gutter_color(theme, &report, "src/lib.rs", Some(99)),
            None,
            "a line without data leaves the diff's own gutter color"
        );
        assert_eq!(
            coverage_gutter_color(theme, &report, "src/lib.rs", None),
            None,
            "rows without a new-side number (removed lines) stay unannotated"
        );
    }
}
