use super::*;

/// One checkable option row. Same visual language as the create-branch
/// checkout toggle: a 16px box that fills with a check when enabled.
/// Shared with the merge-request push prompt.
pub(super) fn checkable_option_row(
    id: &'static str,
    label: SharedString,
    theme: AppTheme,
    enabled: bool,
    focus_handle: &FocusHandle,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Stateful<gpui::Div> {
    let scaled_px = super::popover_scaled_px_fn(cx);
    let border = if enabled {
        theme.colors.status.success.foreground
    } else {
        theme.colors.stroke.default
    };
    let background = if enabled {
        with_alpha(
            theme.colors.status.success.foreground,
            if theme.is_dark { 0.18 } else { 0.12 },
        )
    } else {
        gpui::rgba(0x00000000)
    };

    focusable_toggle_row(id, id, theme, focus_handle, cx)
        .flex()
        .gap_2()
        .justify_start()
        .child(
            div()
                .size(scaled_px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(border)
                .rounded(scaled_px(theme.radii.control * 0.5))
                .bg(background)
                .when(enabled, |this| {
                    this.child(crate::view::icons::svg_icon(
                        "icons/check.svg",
                        theme.colors.status.success.foreground,
                        scaled_px(10.0),
                    ))
                }),
        )
        .child(div().text_sm().child(label))
}

pub(super) fn panel(
    this: &mut PopoverHost,
    paths: Vec<std::path::PathBuf>,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_stash = this.can_submit_stash(cx);
    let scaled_px = super::popover_scaled_px_fn(cx);

    let mut panel = div()
        .flex()
        .flex_col()
        .w(scaled_px(420.0))
        .child(popover_title(crate::i18n::tr("input.stash.title")))
        .child(div().border_t_1().border_color(theme.colors.stroke.default));

    if !paths.is_empty() {
        panel = panel.child(
            div()
                .id("stash_paths_selected")
                .debug_selector(move || "stash_paths_selected".to_string())
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(
                    crate::i18n::t!("input.stash.paths_selected", count = paths.len()).into_owned(),
                ),
        );
    }

    panel = panel
        .child(
            div()
                .px_2()
                .py_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.stash_message_input.clone()),
        )
        .child(
            checkable_option_row(
                "stash_include_untracked_toggle",
                crate::i18n::tr("input.stash.include_untracked"),
                theme,
                this.stash_include_untracked,
                &this.stash_include_untracked_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.stash_include_untracked = !this.stash_include_untracked;
                cx.notify();
            })),
        )
        .child(
            checkable_option_row(
                "stash_keep_index_toggle",
                crate::i18n::tr("input.stash.keep_index"),
                theme,
                this.stash_keep_index,
                &this.stash_keep_index_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.stash_keep_index = !this.stash_keep_index;
                cx.notify();
            })),
        );

    panel.child(
        div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .child(
                cancel_button("stash_cancel", "stash_cancel_hint", theme)
                    .focus_handle(this.stash_focus.cancel.clone())
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.dismiss_prompt_popover(window, cx);
                    }),
            )
            .child(
                components::Button::new("stash_go", crate::i18n::tr("input.stash.stash"))
                    .focus_handle(this.stash_focus.submit.clone())
                    .separated_end_slot(super::hotkey_hint(theme, "stash_go_hint", "Enter"))
                    .style(components::ButtonStyle::Filled)
                    .disabled(!can_stash)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.submit_stash(window, cx);
                    }),
            ),
    )
}
