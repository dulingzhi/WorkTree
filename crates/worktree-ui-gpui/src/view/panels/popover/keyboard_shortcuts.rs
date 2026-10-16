//! 08-5: in-app keyboard-shortcut cheatsheet. A centered, repo-less dialog that
//! lists every command carrying a shortcut, so the bindings are discoverable
//! without leaving the app. The contents are static (`COMMANDS`), so the panel
//! needs no repo id, no lazy data load, and no fingerprint repo binding.

use super::*;

pub(super) fn panel(this: &mut PopoverHost, cx: &mut gpui::Context<PopoverHost>) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);
    let percent = crate::ui_scale::current(cx).percent;
    let tooltip_host = this.tooltip_host.clone();

    let title = components::context_menu_header(
        theme,
        percent,
        crate::i18n::tr("ui.shortcuts.title"),
        Some(tooltip_host.clone()),
        cx,
    );
    let intro = components::context_menu_description(
        theme,
        percent,
        crate::i18n::tr("ui.shortcuts.intro"),
        Some(tooltip_host.clone()),
        cx,
    );

    // Every command that advertises a shortcut becomes one cheatsheet row: the
    // translated label on the left, the keycap chips on the right. `COMMANDS`
    // already carries the platform-resolved `Shortcut`, so no per-platform work
    // is needed here.
    let rows: Vec<gpui::Div> = crate::view::command_palette::COMMANDS
        .iter()
        .filter_map(|cmd| {
            let shortcut_label = cmd.shortcut.label()?;
            let label = div()
                .flex_1()
                .min_w_0()
                .text_color(theme.colors.foreground.primary)
                .text_size(scaled_px(13.0))
                .line_height(scaled_px(18.0))
                .child(crate::i18n::tr_str(cmd.label).to_string());
            let keycaps = components::shortcut_keys(&shortcut_label, theme, percent);
            Some(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(scaled_px(16.0))
                    .py(scaled_px(5.0))
                    .child(label)
                    .child(keycaps),
            )
        })
        .collect();

    // The binding count can outgrow the visible area, so the row list scrolls
    // inside the crate's scroll-container helper (visible scrollbar, vertical
    // only, wheel restricted to the vertical axis).
    let list: gpui::AnyElement = components::ScrollContainer::vertical(
        "keyboard_shortcuts_scroll_surface",
        "keyboard_shortcuts_scrollbar",
        gpui::ScrollHandle::new(),
        scaled_px(440.0),
    )
    .render(theme, div().flex().flex_col().children(rows));

    components::context_menu(
        theme,
        div()
            .debug_selector(|| "keyboard_shortcuts_panel".to_string())
            .w(scaled_px(540.0))
            .flex()
            .flex_col()
            .child(title)
            .child(intro)
            .child(list),
    )
}
