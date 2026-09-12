use super::styled_text::{
    RESOLVED_OUTPUT_BADGE_PX, RESOLVED_OUTPUT_LINE_NO_GAP_PX, RESOLVED_OUTPUT_MARKER_LANE_PX,
    conflict_diff_query_matcher, conflict_display_text, resolved_output_gutter_width,
    resolved_output_line_no_width, three_way_input_row_menu_targets,
    two_way_split_input_row_menu_targets, whitespace_visible_text,
    whitespace_visible_text_and_highlights,
};
use super::*;

#[test]
fn whitespace_visible_text_and_highlights_remaps_highlight_ranges() {
    let style = gpui::HighlightStyle::default();
    let (display, highlights) = whitespace_visible_text_and_highlights("a b\t", &[(1..4, style)]);

    assert_eq!(display.as_ref(), "a·b→");
    assert_eq!(highlights.len(), 1);
    assert_eq!(highlights[0].0, 1..7);
}

#[test]
fn whitespace_visible_text_marks_all_whitespace_kinds() {
    let display = whitespace_visible_text(" \t\r\n");
    assert_eq!(display.as_ref(), "·→␍↵");
}

#[test]
fn conflict_display_text_reveals_implicit_line_break_marker() {
    let text: SharedString = "a b\t".into();
    let display = conflict_display_text(&text, None, true);

    assert_eq!(display.as_ref(), "a·b→↵");
}

#[test]
fn conflict_diff_query_matcher_preserves_significant_whitespace() {
    let space_matcher =
        conflict_diff_query_matcher(" ", DiffSearchOptions::default()).expect("space query");
    assert_eq!(space_matcher.query(), " ");
    assert!(space_matcher.is_match("a b"));

    let padded_matcher =
        conflict_diff_query_matcher(" foo ", DiffSearchOptions::default()).expect("padded query");
    assert_eq!(padded_matcher.query(), " foo ");
    assert!(padded_matcher.is_match("x foo y"));
    assert!(!padded_matcher.is_match("foo"));

    assert!(conflict_diff_query_matcher("", DiffSearchOptions::default()).is_none());
}

#[test]
fn three_way_input_row_targets_include_line_and_chunk_picks() {
    let (line_label, line_target, chunk_label, chunk_target) =
        three_way_input_row_menu_targets(4, 2, conflict_resolver::ConflictChoice::Theirs);

    assert_eq!(line_label.as_ref(), "Pick this line (C)");
    assert_eq!(chunk_label.as_ref(), "Pick this chunk (C)");
    assert_eq!(
        line_target,
        ResolverPickTarget::ThreeWayLine {
            line_ix: 4,
            choice: conflict_resolver::ConflictChoice::Theirs,
        }
    );
    assert_eq!(
        chunk_target,
        ResolverPickTarget::Chunk {
            conflict_ix: 2,
            choice: conflict_resolver::ConflictChoice::Theirs,
            output_line_ix: None,
        }
    );
}

#[test]
fn two_way_split_input_row_targets_map_side_to_split_line_and_chunk_choice() {
    let (line_label, line_target, chunk_label, chunk_target) =
        two_way_split_input_row_menu_targets(9, 5, ConflictPickSide::Ours);

    assert_eq!(line_label.as_ref(), "Pick this line (A)");
    assert_eq!(chunk_label.as_ref(), "Pick this chunk (A)");
    assert_eq!(
        line_target,
        ResolverPickTarget::TwoWaySplitLine {
            row_ix: 9,
            side: ConflictPickSide::Ours,
        }
    );
    assert_eq!(
        chunk_target,
        ResolverPickTarget::Chunk {
            conflict_ix: 5,
            choice: conflict_resolver::ConflictChoice::Ours,
            output_line_ix: None,
        }
    );
}

/// The gutter container hugs its content, so the width sum and the row that
/// lays that content out have to scale together -- otherwise the marker and
/// badge are clipped at anything above 100%.
#[test]
fn resolved_output_gutter_width_scales_with_ui_scale() {
    for percent in [80, 100, 150, 200] {
        let factor = percent as f32 / 100.0;

        let digits: f32 = resolved_output_line_no_width(1_234, percent).into();
        assert!(
            (digits - 4.0 * 8.0 * factor).abs() < 0.01,
            "a four-digit cell at {percent}% should be {}, got {digits}",
            4.0 * 8.0 * factor,
        );

        let with_numbers: f32 = resolved_output_gutter_width(1_234, true, percent).into();
        let expected_with = (RESOLVED_OUTPUT_MARKER_LANE_PX
            + RESOLVED_OUTPUT_BADGE_PX
            + CONFLICT_ROW_PADDING_X_PX * 2.0
            + 4.0 * 8.0
            + RESOLVED_OUTPUT_LINE_NO_GAP_PX)
            * factor;
        assert!(
            (with_numbers - expected_with).abs() < 0.01,
            "gutter width at {percent}% should be {expected_with}, got {with_numbers}",
        );

        // With numbers hidden only the number cell and its gap drop out; the
        // marker lane and badge stay, so the gutter never collapses to nothing.
        let without_numbers: f32 = resolved_output_gutter_width(1_234, false, percent).into();
        let expected_without = (RESOLVED_OUTPUT_MARKER_LANE_PX
            + RESOLVED_OUTPUT_BADGE_PX
            + CONFLICT_ROW_PADDING_X_PX * 2.0)
            * factor;
        assert!(
            (without_numbers - expected_without).abs() < 0.01,
            "gutter width without numbers at {percent}% should be {expected_without}, \
             got {without_numbers}",
        );
        assert!(without_numbers < with_numbers);
    }
}

/// A two-digit floor keeps a short file's numbers from sitting flush against the
/// marker lane, and it has to survive scaling.
#[test]
fn resolved_output_line_no_width_keeps_its_two_digit_floor_at_every_scale() {
    for percent in [80, 100, 150, 200] {
        assert_eq!(
            resolved_output_line_no_width(1, percent),
            resolved_output_line_no_width(42, percent),
            "one- and two-line files should share a cell width at {percent}%"
        );
        assert!(
            resolved_output_line_no_width(100, percent)
                > resolved_output_line_no_width(42, percent),
            "a three-digit file should widen the cell at {percent}%"
        );
    }
}
