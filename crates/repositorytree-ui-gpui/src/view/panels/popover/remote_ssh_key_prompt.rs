use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    name: String,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_submit = this.can_submit_remote_ssh_key(cx);
    let scaled_px = super::popover_scaled_px_fn(cx);

    div()
        .flex()
        .flex_col()
        .w(scaled_px(640.0))
        .child(popover_title(
            crate::i18n::t!("input.remote_ssh_key.title").into_owned(),
        ))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(
                    crate::i18n::t!("input.remote_ssh_key.remote_line", name = name).into_owned(),
                ),
        )
        .child(
            div()
                .px_2()
                .pb_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.remote_ssh_key_input.clone()),
        )
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    cancel_button(
                        "remote_ssh_key_cancel",
                        "remote_ssh_key_cancel_hint",
                        theme,
                    )
                    .focus_handle(this.remote_ssh_key_focus.cancel.clone())
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.dismiss_prompt_popover(window, cx);
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            components::Button::new(
                                "remote_ssh_key_clear",
                                crate::i18n::tr("input.remote_ssh_key.clear"),
                            )
                            .focus_handle(this.remote_ssh_key_clear_focus.clone())
                            .style(components::ButtonStyle::Subtle)
                            .on_click(theme, cx, |this, _e, _w, cx| {
                                this.clear_remote_ssh_key(cx);
                            }),
                        )
                        .child(
                            components::Button::new(
                                "remote_ssh_key_go",
                                crate::i18n::tr("input.remote_ssh_key.save"),
                            )
                            .focus_handle(this.remote_ssh_key_focus.submit.clone())
                            .disabled(!can_submit)
                            .separated_end_slot(super::hotkey_hint(
                                theme,
                                "remote_ssh_key_go_hint",
                                "Enter",
                            ))
                            .style(components::ButtonStyle::Filled)
                            .on_click(theme, cx, |this, _e, _w, cx| {
                                this.submit_remote_ssh_key(cx);
                            }),
                        ),
                ),
        )
}
