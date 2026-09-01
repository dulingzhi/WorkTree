use super::*;

pub(in crate::view) fn absolute_scroll_y(handle: &ScrollHandle) -> Pixels {
    let raw = handle.offset().y;
    if raw < px(0.0) { -raw } else { raw }
}

pub(in crate::view) fn scroll_is_near_bottom(handle: &ScrollHandle, threshold: Pixels) -> bool {
    let max_offset = handle.max_offset().y.max(px(0.0));
    if max_offset <= px(0.0) {
        return true;
    }

    let scroll_y = absolute_scroll_y(handle).max(px(0.0)).min(max_offset);
    (max_offset - scroll_y) <= threshold
}
