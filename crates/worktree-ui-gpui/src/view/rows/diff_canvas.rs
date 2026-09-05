use super::canvas::keyed_canvas;
use super::diff_text::{
    PreparedDocumentByteRangeHighlights, build_cached_diff_query_overlay_styled_text,
    build_cached_diff_styled_text, build_cached_diff_styled_text_from_relative_highlights,
    hash_rgba_bits as hash_rgba, slice_cached_diff_styled_text,
    syntax_highlights_for_streamed_line_slice_heuristic, whitespace_visible_line_styled_text,
    whitespace_visible_line_styled_text_for_raw, whitespace_visible_line_text,
    whitespace_visible_styled_text,
};
use super::*;
use crate::kit::diff_text_metrics::{
    DIFF_FONT_SCALE, LineMetrics, center_text_y, diff_text_style, line_metrics,
    line_metrics_annot_when, px_2,
};
use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};
use crate::view::panes::main::DiffHorizontalScrollColumn;
use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, HighlightStyle, Hitbox, HitboxBehavior, Pixels,
    Styled, TextRun, TextStyle, TransformationMatrix, TruncateFrom, Window, fill, point, px, size,
};
use palette::IntoColor;
use rustc_hash::{FxHashMap, FxHasher};
use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::sync::OnceLock;
use worktree_core::domain::{DiffArea, DiffLineKind};

mod blame;
mod geometry;
mod rows;
mod stage_gutter;
mod streamed;

// Re-exported at the original `diff_canvas::…` paths (the `rows` module's
// re-export surface and in-tree `diff_canvas::` consumers depend on them).
pub(super) use self::blame::paint_centered_svg_icon;
pub(in crate::view) use self::blame::{
    AnnotArea, DIFF_ANNOTATION_COLUMN_WIDTH_PX, DIFF_ANNOTATION_MAX_WIDTH_PX,
    DIFF_ANNOTATION_MIN_WIDTH_PX, RowBlamePaint, blame_gutter_row_canvas,
};
pub(in crate::view) use self::geometry::{
    diff_change_bar_width, diff_inline_text_start, diff_row_horizontal_padding,
    diff_single_column_text_start, diff_text_wrap_char_width,
};
#[cfg(test)]
pub(in crate::view) use self::rows::{
    DiffPaintRecord, clear_diff_paint_log_for_tests, diff_paint_log_for_tests,
};
pub(super) use self::rows::{
    inline_diff_line_row_canvas, patch_split_column_row_canvas, split_diff_line_row_canvas,
    worktree_preview_row_canvas,
};
pub(in crate::view) use self::stage_gutter::{DiffStageHover, DiffStageSlot, StageGutterSpec};
pub(in crate::view) use self::streamed::{
    DiffTextWrapSlice, DiffWrapByteRange, is_streamable_diff_text,
    whitespace_visible_diff_offset_map,
};
pub(super) use self::streamed::{StreamedDiffTextPaintSpec, StreamedDiffTextSyntaxSource};

#[cfg(test)]
mod tests {
    use super::geometry::{DIFF_ROW_BACKGROUND_OVERDRAW_PX, row_bg_fill_bounds};
    use super::rows::{
        DiffRowTextRegions, compute_runs, inline_row_canvas_revision_key,
        patch_split_row_canvas_revision_key, split_row_canvas_revision_key,
    };
    use super::streamed::{
        STREAMED_DIFF_TEXT_MIN_BYTES, build_streamed_diff_slice_styled_text,
        diff_text_paint_payload, should_stream_diff_text,
    };
    use super::*;

    fn rgba(r: f32, g: f32, b: f32) -> gpui::Rgba {
        gpui::Rgba::new(r, g, b, 1.0)
    }

    /// `gpui` shapes a line by splitting the text at each run boundary, so a
    /// run that ends inside a multi-byte character aborts the process in
    /// `str::split_at`. This pins the diff canvas to the shared guard: without
    /// it, a highlight pointing into a character reaches `shape_line` as a
    /// run length that splits it.
    #[test]
    fn compute_runs_never_splits_a_multibyte_char() {
        let text = "— dash — end";
        let style = TextStyle::default();
        let bold = HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..HighlightStyle::default()
        };

        for highlights in [
            // Inside the leading em dash, from both sides.
            vec![(0..1, bold)],
            vec![(1..3, bold)],
            vec![(2..4, bold)],
            // Inside the second em dash, after valid text.
            vec![(0..2, bold), (7..9, bold)],
            // Past the end, overlapping, and out of order.
            vec![(5..99, bold)],
            vec![(0..6, bold), (2..4, bold)],
            vec![(6..9, bold), (0..3, bold)],
            vec![],
        ] {
            let runs = compute_runs(text, &style, &highlights);
            let total: usize = runs.iter().map(|run| run.len).sum();
            assert_eq!(
                total,
                text.len(),
                "runs must tile the text for {highlights:?}"
            );

            let mut rest = text;
            for run in &runs {
                assert!(
                    rest.is_char_boundary(run.len),
                    "run of {} bytes splits a character in {rest:?} for {highlights:?}",
                    run.len
                );
                rest = &rest[run.len..];
            }
        }
    }

    fn test_bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn streamed_query_spec(
        raw_text: &str,
        query: &str,
        query_options: DiffSearchOptions,
    ) -> StreamedDiffTextPaintSpec {
        StreamedDiffTextPaintSpec {
            raw_text: worktree_core::file_diff::FileDiffLineText::from(raw_text),
            query: query.to_owned().into(),
            query_options,
            query_matcher: (!query.is_empty())
                .then(|| Arc::new(DiffSearchMatcher::new(query, query_options))),
            query_emphasis: DiffSearchMatchEmphasis::Other,
            word_ranges: Arc::from(Vec::<Range<usize>>::new()),
            word_kind: None,
            syntax: StreamedDiffTextSyntaxSource::None,
        }
    }

    fn highlight_ranges(styled: &CachedDiffStyledText) -> Vec<Range<usize>> {
        styled
            .highlights
            .iter()
            .map(|(range, _)| range.clone())
            .collect()
    }

    #[test]
    fn diff_text_paint_payload_reveals_whitespace_markers() {
        let style = HighlightStyle::default();
        let styled = CachedDiffStyledText {
            text: "a b\t".into(),
            highlights: Arc::from(vec![(1..4, style)]),
            highlights_hash: 7,
            text_hash: 11,
        };

        let payload = diff_text_paint_payload(
            Some(&styled),
            None,
            Some("a b\t"),
            true,
            DiffTextRegion::Inline,
            None,
        );

        assert_eq!(payload.text.as_ref(), "a·b→↵");
        assert_eq!(payload.highlights[0].0, 1..7);
        assert_ne!(payload.text_hash, styled.text_hash);

        let offset_map = payload.offset_map.expect("reveal whitespace offset map");
        assert_eq!(offset_map.source_offset_for_display(0), 0);
        assert_eq!(offset_map.source_offset_for_display("a·".len()), 2);
        assert_eq!(offset_map.source_offset_for_display("a·b→".len()), 7);
        assert_eq!(offset_map.display_offset_for_source(2), "a·".len());
        assert_eq!(offset_map.display_offset_for_source(7), "a·b→".len());
    }

    #[test]
    fn diff_text_paint_payload_keeps_streamed_whitespace_rows_unmaterialized() {
        let raw = "a ".repeat((STREAMED_DIFF_TEXT_MIN_BYTES / 2).saturating_add(1));
        let spec = streamed_query_spec(raw.as_str(), "", DiffSearchOptions::default());

        assert!(should_stream_diff_text(Some(&spec)));

        let payload =
            diff_text_paint_payload(None, Some(&spec), None, true, DiffTextRegion::Inline, None);

        assert!(payload.text.is_empty());
        assert!(payload.highlights.is_empty());
        assert_ne!(payload.text_hash, 0);
        assert!(payload.offset_map.is_none());
    }

    #[test]
    fn row_bg_fill_bounds_overdraws_bottom_without_changing_origin_or_width() {
        let bounds = test_bounds(4.0, 8.0, 120.0, 20.0);
        let painted = row_bg_fill_bounds(bounds);

        assert_eq!(painted.origin, bounds.origin);
        assert_eq!(painted.size.width, bounds.size.width);
        assert_eq!(
            painted.size.height,
            bounds.size.height + px(DIFF_ROW_BACKGROUND_OVERDRAW_PX)
        );
    }

    #[test]
    fn diff_row_text_regions_single_only_hits_inside_text() {
        let regions =
            DiffRowTextRegions::single(DiffTextRegion::Inline, test_bounds(5.0, 5.0, 20.0, 10.0));

        assert_eq!(
            regions.region_at(point(px(10.0), px(10.0))),
            Some(DiffTextRegion::Inline)
        );
        assert_eq!(regions.region_at(point(px(1.0), px(10.0))), None);
    }

    #[test]
    fn diff_row_text_regions_split_maps_left_and_right_regions() {
        let regions = DiffRowTextRegions::split(
            test_bounds(0.0, 0.0, 40.0, 20.0),
            test_bounds(41.0, 0.0, 40.0, 20.0),
        );

        assert_eq!(
            regions.region_at(point(px(10.0), px(10.0))),
            Some(DiffTextRegion::SplitLeft)
        );
        assert_eq!(
            regions.region_at(point(px(60.0), px(10.0))),
            Some(DiffTextRegion::SplitRight)
        );
        assert_eq!(regions.region_at(point(px(40.5), px(10.0))), None);
    }

    #[test]
    fn inline_row_canvas_revision_key_tracks_rendered_payload() {
        let base = inline_row_canvas_revision_key(
            &"1".into(),
            &"2".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            11,
            17,
        );

        assert_eq!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"3".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(1.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                11,
                17,
            )
        );
        assert_ne!(
            base,
            inline_row_canvas_revision_key(
                &"1".into(),
                &"2".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                12,
                17,
            )
        );
    }

    #[test]
    fn split_row_canvas_revision_key_tracks_both_sides() {
        let base = split_row_canvas_revision_key(
            &"10".into(),
            &"20".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            3,
            5,
            7,
            11,
        );

        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"20".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                4,
                5,
                7,
                11,
            )
        );
        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"20".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                3,
                5,
                7,
                11,
            )
        );
        assert_ne!(
            base,
            split_row_canvas_revision_key(
                &"10".into(),
                &"21".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                3,
                5,
                7,
                11,
            )
        );
    }

    #[test]
    fn patch_split_row_canvas_revision_key_tracks_line_number_and_style() {
        let base = patch_split_row_canvas_revision_key(
            &"42".into(),
            rgba(0.0, 0.0, 0.0),
            rgba(1.0, 1.0, 1.0),
            rgba(1.0, 1.0, 1.0),
            13,
            17,
        );

        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"43".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                13,
                17,
            )
        );
        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"42".into(),
                rgba(0.0, 1.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                13,
                17,
            )
        );
        assert_ne!(
            base,
            patch_split_row_canvas_revision_key(
                &"42".into(),
                rgba(0.0, 0.0, 0.0),
                rgba(1.0, 1.0, 1.0),
                rgba(1.0, 1.0, 1.0),
                14,
                17,
            )
        );
    }

    #[test]
    fn streamed_query_overlay_skips_whole_word_on_partial_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "foo_suffix",
            "foo",
            DiffSearchOptions {
                whole_word: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) = build_streamed_diff_slice_styled_text(theme, &spec, &(0..3));

        assert_eq!(resolved, 0..3);
        assert_eq!(styled.text.as_ref(), "foo");
        assert!(styled.highlights.is_empty());
    }

    #[test]
    fn streamed_query_overlay_skips_regex_anchor_on_partial_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "prefixfoo suffix",
            r"^foo",
            DiffSearchOptions {
                regex: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) = build_streamed_diff_slice_styled_text(theme, &spec, &(6..9));

        assert_eq!(resolved, 6..9);
        assert_eq!(styled.text.as_ref(), "foo");
        assert!(styled.highlights.is_empty());
    }

    #[test]
    fn streamed_query_overlay_keeps_boundary_sensitive_matches_on_full_slice() {
        let theme = AppTheme::worktree_dark();
        let spec = streamed_query_spec(
            "foo suffix",
            r"^foo",
            DiffSearchOptions {
                regex: true,
                ..DiffSearchOptions::default()
            },
        );

        let (styled, _, resolved) =
            build_streamed_diff_slice_styled_text(theme, &spec, &(0.."foo suffix".len()));

        assert_eq!(resolved, 0.."foo suffix".len());
        assert_eq!(highlight_ranges(&styled), vec![0..3]);
    }
}
