use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    path: &std::path::Path,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_submit = this.can_submit_submodule_change_pointer(cx);
    let scaled_px = super::popover_scaled_px_fn(cx);

    div()
        .flex()
        .flex_col()
        .w(scaled_px(420.0))
        .child(popover_title(crate::i18n::tr(
            "input.submodule_change_pointer.title",
        )))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(
                    crate::i18n::t!(
                        "prompts.submodule_trust.submodule_line",
                        path = path.display()
                    )
                    .into_owned(),
                ),
        )
        .child(input_label(
            theme,
            crate::i18n::tr_str("input.submodule_change_pointer.target_label"),
        ))
        .child(
            div()
                .px_2()
                .pb_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.submodule_ref_input.clone()),
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
                        "submodule_change_pointer_cancel",
                        "submodule_change_pointer_cancel_hint",
                        theme,
                    )
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.dismiss_inline_popover(window, cx);
                    }),
                )
                .child(
                    components::Button::new(
                        "submodule_change_pointer_go",
                        crate::i18n::tr("input.submodule_change_pointer.change"),
                    )
                    .separated_end_slot(super::hotkey_hint(
                        theme,
                        "submodule_change_pointer_go_hint",
                        "Enter",
                    ))
                    .style(components::ButtonStyle::Filled)
                    .disabled(!can_submit)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.submit_submodule_change_pointer(window, cx);
                    }),
                ),
        )
}
