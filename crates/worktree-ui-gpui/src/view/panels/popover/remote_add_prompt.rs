use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_submit = this.can_submit_remote_add(cx);
    let scaled_px = super::popover_scaled_px_fn(cx);

    div()
        .flex()
        .flex_col()
        .w(scaled_px(640.0))
        .child(popover_title(crate::i18n::tr("input.remote_add.title")))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(input_label(
            theme,
            crate::i18n::tr_str("input.remote_add.name_label"),
        ))
        .child(
            div()
                .px_2()
                .pb_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.remote_prompts.remote_name_input.clone()),
        )
        .child(input_label(
            theme,
            crate::i18n::tr_str("prompts.submodule_add.url_label"),
        ))
        .child(
            div()
                .px_2()
                .pb_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.remote_prompts.remote_url_input.clone()),
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
                    cancel_button("add_remote_cancel", "add_remote_cancel_hint", theme)
                        .focus_handle(this.remote_prompts.remote_add_focus.cancel.clone())
                        .on_click(theme, cx, |this, _e, window, cx| {
                            this.dismiss_prompt_popover(window, cx);
                        }),
                )
                .child(
                    components::Button::new(
                        "add_remote_go",
                        crate::i18n::tr("prompts.submodule_add.add"),
                    )
                    .focus_handle(this.remote_prompts.remote_add_focus.submit.clone())
                    .disabled(!can_submit)
                    .separated_end_slot(super::hotkey_hint(theme, "add_remote_go_hint", "Enter"))
                    .style(components::ButtonStyle::Filled)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.submit_remote_add(cx);
                    }),
                ),
        )
}
