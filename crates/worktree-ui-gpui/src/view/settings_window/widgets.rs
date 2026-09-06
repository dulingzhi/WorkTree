//! Shared widgets: dropdown geometry constants, scroll-welding helpers and
//! the row/card builders every settings card composes.

use super::*;
use gpui::Stateful;

pub(super) const SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX: f32 = 224.0;

// The theme list is the one bundled dropdown that outgrew the standard cap:
// Automatic plus every embedded theme at the compact row height. It gets its
// own bound so it expands fully instead of growing an inner scrollbar, while
// every other dropdown keeps the standard-cap geometry.
pub(super) const SETTINGS_THEME_DROPDOWN_LIST_MAX_HEIGHT_PX: f32 = 448.0;

pub(super) const SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX: f32 = 28.0;

pub(super) const SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX: f32 = 20.0;

pub(super) const SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX: f32 = 42.0;

pub(super) const SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX: f32 = 24.0;

pub(super) const SETTINGS_DROPDOWN_DENSE_DETAIL_ROW_HEIGHT_PX: f32 = 28.0;

fn uniform_list_vertical_wheel_delta(event: &gpui::ScrollWheelEvent, window: &Window) -> Pixels {
    event.delta.pixel_delta(window.line_height()).y
}

fn normalize_scroll_offset(raw_offset: Pixels, max_offset: Pixels) -> Pixels {
    if max_offset <= px(0.0) {
        return px(0.0);
    }

    if raw_offset < px(0.0) {
        (-raw_offset).max(px(0.0)).min(max_offset)
    } else {
        raw_offset.max(px(0.0)).min(max_offset)
    }
}

pub(super) fn uniform_list_vertical_scroll_metrics(
    handle: &UniformListScrollHandle,
) -> (Pixels, Pixels, Pixels) {
    let state = handle.0.borrow();
    let max_offset = state
        .last_item_size
        .map(|size| (size.contents.height - size.item.height).max(px(0.0)))
        .unwrap_or_else(|| state.base_handle.max_offset().y.max(px(0.0)));
    let raw_offset = state.base_handle.offset().y;
    let scroll_offset = normalize_scroll_offset(raw_offset, max_offset);
    (raw_offset, scroll_offset, max_offset)
}

fn uniform_list_should_stop_scroll_propagation(
    handle: &UniformListScrollHandle,
    event: &gpui::ScrollWheelEvent,
    window: &Window,
) -> bool {
    let delta_y = uniform_list_vertical_wheel_delta(event, window);
    if delta_y.is_zero() {
        return false;
    }

    let (raw_offset_after, _scroll_offset_after, max_offset) =
        uniform_list_vertical_scroll_metrics(handle);
    if max_offset <= px(0.0) {
        return false;
    }

    // This runs after the list's built-in wheel scroll listener, so reconstruct the pre-scroll
    // position before deciding whether to keep the event inside the dropdown.
    let raw_offset_before = raw_offset_after - delta_y;
    let scroll_offset_before = normalize_scroll_offset(raw_offset_before, max_offset);
    if delta_y < px(0.0) {
        scroll_offset_before < max_offset
    } else {
        scroll_offset_before > px(0.0)
    }
}

/// The `.on_scroll_wheel` handler every settings dropdown list uses, so a
/// wheel over the list stops at the list and only chains to the page scroller
/// once the list has hit its edge. Every new dropdown must use this — without
/// it the page behind the open list scrolls in lockstep (the exact bug the
/// four lists below were bitten by).
pub(super) fn stop_dropdown_wheel_chaining(
    scroll: UniformListScrollHandle,
) -> impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) {
    move |event, window, cx| {
        if uniform_list_should_stop_scroll_propagation(&scroll, event, window) {
            cx.stop_propagation();
        }
    }
}

fn mix_color(a: gpui::Rgba, b: gpui::Rgba, t: f32) -> gpui::Rgba {
    let t = t.clamp(0.0, 1.0);
    gpui::Rgba::new(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    )
}

pub(super) fn settings_row_separator_color(theme: AppTheme) -> gpui::Rgba {
    mix_color(
        theme.colors.surface.canvas,
        theme.colors.stroke.subtle,
        if theme.is_dark { 0.14 } else { 0.10 },
    )
}

pub(super) fn settings_dropdown_background(theme: AppTheme) -> gpui::Rgba {
    if theme.is_dark {
        mix_color(
            theme.colors.surface.raised,
            theme.colors.surface.canvas,
            0.58,
        )
    } else {
        mix_color(
            theme.colors.surface.raised,
            theme.colors.stroke.default,
            0.55,
        )
    }
}

fn settings_dropdown_border_color(theme: AppTheme) -> gpui::Rgba {
    if theme.is_dark {
        with_alpha(theme.colors.stroke.default, 0.98)
    } else {
        theme.colors.stroke.default
    }
}

fn settings_dropdown_height(
    item_count: usize,
    estimated_row_height_px: f32,
    extra_height_px: f32,
    max_height_px: f32,
    ui_scale_percent: u32,
) -> Pixels {
    ui_scale::design_px_from_percent(
        (((item_count.max(1) as f32) * estimated_row_height_px) + extra_height_px)
            .min(max_height_px),
        ui_scale_percent,
    )
}

impl SettingsWindowView {
    pub(super) fn push_main_window_toast(
        &self,
        kind: components::ToastKind,
        message: String,
        cx: &mut gpui::Context<Self>,
    ) {
        self.update_main_windows(cx, move |view, _window, cx| {
            view.push_toast(kind, message.clone(), cx);
        });
    }

    pub(super) fn empty_dropdown_list(&self, message: &'static str, theme: AppTheme) -> AnyElement {
        div()
            .w_full()
            .h_full()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.colors.foreground.secondary)
            .child(message)
            .into_any_element()
    }

    pub(super) fn dropdown_list_container(
        &self,
        container_id: &'static str,
        scrollbar_id: &'static str,
        scroll: UniformListScrollHandle,
        item_count: usize,
        estimated_row_height_px: f32,
        extra_height_px: f32,
        max_list_height_px: f32,
        list: AnyElement,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let height = settings_dropdown_height(
            item_count,
            estimated_row_height_px,
            extra_height_px,
            max_list_height_px,
            self.ui_scale_percent,
        );
        // `h` includes the 1px border on each edge, so keep the requested
        // dropdown height available to the inner list viewport.
        let outer_height = height + px(2.0);

        div()
            .id(container_id)
            .debug_selector(move || container_id.to_string())
            .w_full()
            .min_w(px(0.0))
            .relative()
            .h(outer_height)
            .min_h(outer_height)
            .rounded(px(theme.radii.row))
            .border_1()
            .border_color(settings_dropdown_border_color(theme))
            .bg(settings_dropdown_background(theme))
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .h_full()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list),
            )
            .child(
                components::Scrollbar::new(scrollbar_id, scroll)
                    .always_visible()
                    .render(theme),
            )
    }

    pub(super) fn detail_container(
        &self,
        container_id: &'static str,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        div()
            .id(container_id)
            .debug_selector(move || container_id.to_string())
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .rounded(px(theme.radii.row))
            .border_1()
            .border_color(settings_dropdown_border_color(theme))
            .bg(settings_dropdown_background(theme))
            .overflow_hidden()
    }

    pub(super) fn summary_row(
        &self,
        id: &'static str,
        label: &'static str,
        value: SharedString,
        expanded: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let label_debug_id = format!("{id}_label");
        let value_debug_id = format!("{id}_value");
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .px_2()
            .pt_1()
            .pb_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(theme.radii.row))
            .border_b_1()
            .border_color(settings_row_separator_color(theme))
            .cursor(CursorStyle::PointingHand)
            .overflow_hidden()
            .hover(move |s| s.bg(theme.colors.interaction.hover_background))
            .active(move |s| s.bg(theme.colors.interaction.pressed_background))
            .child(
                div()
                    .debug_selector(move || label_debug_id.clone())
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(label),
                    ),
            )
            .child(
                div()
                    .debug_selector(move || value_debug_id.clone())
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.colors.foreground.secondary)
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(value),
                    )
                    .child(div().flex_shrink_0().child(svg_icon(
                        if expanded {
                            "icons/chevron_down.svg"
                        } else {
                            "icons/arrow_right.svg"
                        },
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))),
            )
    }

    pub(super) fn toggle_row(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let label_debug_id = format!("{id}_label");
        let value_debug_id = format!("{id}_value");
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .px_2()
            .pt_1()
            .pb_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(theme.radii.row))
            .border_b_1()
            .border_color(settings_row_separator_color(theme))
            .cursor(CursorStyle::PointingHand)
            .overflow_hidden()
            .hover(move |s| s.bg(theme.colors.interaction.hover_background))
            .active(move |s| s.bg(theme.colors.interaction.pressed_background))
            .child(
                div()
                    .debug_selector(move || label_debug_id.clone())
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(label),
                    ),
            )
            .child(
                div()
                    .debug_selector(move || value_debug_id.clone())
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(
                        // Toggle-switch visual; the whole row stays the click
                        // target, so this carries no handlers of its own.
                        div()
                            .w(px(28.0))
                            .h(px(16.0))
                            .rounded(px(theme.radii.pill))
                            .flex()
                            .items_center()
                            .p(px(2.0))
                            .when(enabled, |track| {
                                track.justify_end().bg(theme.colors.accent.foreground)
                            })
                            .when(!enabled, |track| {
                                track.justify_start().bg(with_alpha(
                                    theme.colors.foreground.secondary,
                                    if theme.is_dark { 0.35 } else { 0.30 },
                                ))
                            })
                            .child(
                                div()
                                    .size(px(12.0))
                                    .rounded(px(theme.radii.pill))
                                    .bg(gpui::rgba(0xFFFFFFF2)),
                            ),
                    ),
            )
    }

    pub(super) fn info_row(
        &self,
        id: &'static str,
        label: &'static str,
        value: SharedString,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let label_debug_id = format!("{id}_label");
        let value_debug_id = format!("{id}_value");
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .px_2()
            .pt_1()
            .pb_3()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(settings_row_separator_color(theme))
            .overflow_hidden()
            .child(
                div()
                    .debug_selector(move || label_debug_id.clone())
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(label),
                    ),
            )
            .child(
                div()
                    .debug_selector(move || value_debug_id.clone())
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .text_sm()
                            .font_family(UI_MONOSPACE_FONT_FAMILY)
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(value),
                    ),
            )
    }

    pub(super) fn link_row(
        &self,
        id: &'static str,
        label: &'static str,
        value: SharedString,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let label_debug_id = format!("{id}_label");
        let value_debug_id = format!("{id}_value");
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .px_2()
            .pt_1()
            .pb_3()
            .flex()
            .flex_col()
            .items_stretch()
            .gap_0p5()
            .rounded(px(theme.radii.row))
            .border_b_1()
            .border_color(settings_row_separator_color(theme))
            .cursor(CursorStyle::PointingHand)
            .hover(move |s| s.bg(theme.colors.interaction.hover_background))
            .active(move |s| s.bg(theme.colors.interaction.pressed_background))
            .child(
                div()
                    .debug_selector(move || label_debug_id.clone())
                    .min_w(px(0.0))
                    .text_sm()
                    .child(label),
            )
            .child(
                div()
                    .debug_selector(move || value_debug_id.clone())
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .items_start()
                    .gap_2()
                    .text_sm()
                    .text_color(theme.colors.accent.foreground)
                    .child(div().flex_1().min_w(px(0.0)).child(value))
                    .child(div().flex_shrink_0().child(svg_icon(
                        "icons/open_external.svg",
                        theme.colors.accent.foreground,
                        px(13.0),
                    ))),
            )
    }

    pub(super) fn overflow_probe_content(&self, theme: AppTheme) -> Stateful<gpui::Div> {
        div()
            .id("settings_window_overflow_probe_view")
            .w_full()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child(
                self.card("settings_window_overflow_probe_card", "Overflow probe", theme)
                    .child(self.summary_row(
                        "settings_window_overflow_summary",
                        "Deliberately long summary label for overflow coverage",
                        "Extraordinarily long monospace-friendly summary value used to verify clipping"
                            .into(),
                        false,
                        theme,
                    ))
                    .child(self.toggle_row(
                        "settings_window_overflow_toggle",
                        "Deliberately long toggle label for overflow coverage",
                        true,
                        theme,
                    ))
                    .child(self.info_row(
                        "settings_window_overflow_info",
                        "Deliberately long info label for overflow coverage",
                        self.runtime_info.operating_system.clone(),
                        theme,
                    ))
                    .child(self.link_row(
                        "settings_window_overflow_link",
                        "Deliberately long link label for overflow coverage",
                        "https://github.com/dulingzhi/WorkTree/releases/tag/settings-overflow-regression"
                            .into(),
                        theme,
                    ))
                    .child(self.git_runtime_row(theme)),
            )
    }

    pub(super) fn card(
        &self,
        id: &'static str,
        title: &'static str,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .min_w(px(0.0))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .px_2()
                    .pb_2()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.colors.foreground.primary)
                    .child(title),
            )
    }

    pub(super) fn subsection_heading(
        &self,
        id: &'static str,
        title: &'static str,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.to_string())
            .w_full()
            .px_2()
            .pt(px(24.0))
            .pb_2()
            .text_sm()
            .font_weight(FontWeight::BOLD)
            .text_color(theme.colors.foreground.primary)
            .child(title)
    }
}
