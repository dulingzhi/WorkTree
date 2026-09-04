use super::super::*;

const DIFF_ROW_HEIGHT_PX: f32 = 20.0;
const DIFF_FILE_HEADER_HEIGHT_PX: f32 = 28.0;
const DIFF_HUNK_HEADER_HEIGHT_PX: f32 = 24.0;
/// Height of one resolved-output gutter row. The row space navigation scrolls
/// through is measured in these, so anything computing an output scroll offset
/// by hand has to agree with what the gutter actually lays out.
pub(in crate::view) const RESOLVED_OUTPUT_ROW_HEIGHT_PX: f32 = 20.0;

/// Frames a sideways search reveal waits for its row to paint before giving up.
pub(in crate::view) const DIFF_SEARCH_HORIZONTAL_REVEAL_ATTEMPTS: u8 = 4;

/// The scroll offset a `uniform_list` would land on to reveal `row_ix`, or
/// `None` when the row is already fully visible and the list would not move.
///
/// Mirrors `uniform_list`'s own non-strict `ScrollStrategy::Center` arithmetic —
/// centre the row's midpoint in the viewport, clamp into the scrollable range,
/// and leave an already-visible row alone. Kept here so the editable resolved
/// output, which is a `TextInput` rather than a list, can be placed on exactly
/// the offset its gutter list is about to compute.
pub(in crate::view) fn centered_reveal_scroll_y(
    row_ix: usize,
    row_height: Pixels,
    viewport_height: Pixels,
    max_offset_y: Pixels,
    current_y: Pixels,
) -> Option<Pixels> {
    if row_height <= px(0.0) || viewport_height <= px(0.0) {
        return None;
    }
    let row_top = row_height * row_ix as f32;
    let row_bottom = row_top + row_height;
    let scroll_top = -current_y;
    let above = row_top < scroll_top;
    let below = row_bottom > scroll_top + viewport_height;
    if !above && !below {
        return None;
    }
    let target_top = (row_top + row_height / 2.0) - viewport_height / 2.0;
    Some(-target_top.clamp(px(0.0), max_offset_y.max(px(0.0))))
}

/// Margin kept between a revealed search match and the edge it was scrolled
/// past, so the hit does not sit flush against the pane border.
pub(in crate::view) const SEARCH_REVEAL_MARGIN_PX: f32 = 24.0;

/// The horizontal scroll offset that brings `[match_left, match_right]` into
/// view, or `None` when it already is and the pane should not move.
///
/// Unlike the vertical reveal this scrolls the *least* it can rather than
/// centring: a long line jumping sideways on every match is disorienting, and
/// the surrounding text is what makes a hit readable. A match too wide for the
/// viewport is anchored by its start, which is where reading resumes.
///
/// `match_left`/`match_right` are in content space; offsets run negative as the
/// view scrolls right, matching `ScrollHandle`.
pub(in crate::view) fn reveal_scroll_x(
    match_left: Pixels,
    match_right: Pixels,
    viewport_width: Pixels,
    max_offset_x: Pixels,
    current_x: Pixels,
) -> Option<Pixels> {
    if viewport_width <= px(0.0) {
        return None;
    }
    let margin = px(SEARCH_REVEAL_MARGIN_PX).min(viewport_width / 4.0);
    let view_left = -current_x;
    let view_right = view_left + viewport_width;

    let target_left = if match_left < view_left + margin {
        match_left - margin
    } else if match_right > view_right - margin {
        // Anchor the start when the match cannot fit, so reading begins at the
        // hit rather than at its tail.
        (match_right + margin - viewport_width).min(match_left - margin)
    } else {
        return None;
    };

    let target = -target_left.clamp(px(0.0), max_offset_x.max(px(0.0)));
    (target != current_x).then_some(target)
}

#[inline]
fn scaled_diff_px(value: f32, ui_scale_percent: u32) -> Pixels {
    crate::ui_scale::design_px_from_percent(value, ui_scale_percent)
}

#[inline]
pub(in crate::view) fn diff_row_height_for_ui_scale(ui_scale_percent: u32) -> Pixels {
    scaled_diff_px(DIFF_ROW_HEIGHT_PX, ui_scale_percent)
}

#[inline]
pub(in crate::view) fn diff_file_header_height_for_ui_scale(ui_scale_percent: u32) -> Pixels {
    scaled_diff_px(DIFF_FILE_HEADER_HEIGHT_PX, ui_scale_percent)
}

#[inline]
pub(in crate::view) fn diff_hunk_header_height_for_ui_scale(ui_scale_percent: u32) -> Pixels {
    scaled_diff_px(DIFF_HUNK_HEADER_HEIGHT_PX, ui_scale_percent)
}

#[cfg(test)]
mod search_reveal_x_tests {
    use super::{SEARCH_REVEAL_MARGIN_PX, reveal_scroll_x};
    use gpui::px;

    fn viewport() -> gpui::Pixels {
        px(800.0)
    }

    fn max_offset() -> gpui::Pixels {
        px(4000.0)
    }

    #[test]
    fn a_match_already_on_screen_does_not_move_the_view() {
        assert_eq!(
            reveal_scroll_x(px(200.0), px(260.0), viewport(), max_offset(), px(0.0)),
            None
        );
    }

    #[test]
    fn a_match_off_the_right_edge_scrolls_just_far_enough_to_show_it() {
        // Right edge at 800; the match ends at 900, so the view slides by the
        // overshoot plus the margin and no further.
        let target = reveal_scroll_x(px(840.0), px(900.0), viewport(), max_offset(), px(0.0))
            .expect("expected the view to scroll right");
        assert_eq!(target, px(-(900.0 + SEARCH_REVEAL_MARGIN_PX - 800.0)));
    }

    #[test]
    fn a_match_off_the_left_edge_scrolls_back_to_it() {
        // Scrolled 1000 right, with the match at 300 behind the left edge.
        let target = reveal_scroll_x(px(300.0), px(360.0), viewport(), max_offset(), px(-1000.0))
            .expect("expected the view to scroll left");
        assert_eq!(target, px(-(300.0 - SEARCH_REVEAL_MARGIN_PX)));
    }

    #[test]
    fn a_match_wider_than_the_viewport_is_anchored_by_its_start() {
        let target = reveal_scroll_x(px(1000.0), px(3000.0), viewport(), max_offset(), px(0.0))
            .expect("expected the view to scroll right");
        assert_eq!(target, px(-(1000.0 - SEARCH_REVEAL_MARGIN_PX)));
    }

    #[test]
    fn the_target_is_clamped_into_the_scrollable_range() {
        // Never past the end of the content...
        assert_eq!(
            reveal_scroll_x(px(9000.0), px(9060.0), viewport(), px(500.0), px(0.0)),
            Some(px(-500.0))
        );
        // ...and never before its start.
        assert_eq!(
            reveal_scroll_x(px(0.0), px(10.0), viewport(), max_offset(), px(-40.0)),
            Some(px(0.0))
        );
    }

    #[test]
    fn an_unmeasured_viewport_has_no_reveal_to_compute() {
        assert_eq!(
            reveal_scroll_x(px(1000.0), px(1060.0), px(0.0), max_offset(), px(0.0)),
            None
        );
    }
}
