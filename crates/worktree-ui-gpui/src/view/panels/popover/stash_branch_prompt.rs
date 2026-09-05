use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    index: usize,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_submit = this.can_submit_stash_branch(cx);
    let ui_scale_percent = super::popover_ui_scale_percent(cx);
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);

    // The kind carries only the index; the quoted message comes from the
    // loaded stash list. Missing here means the list reloaded under us — the
    // label simply degrades to the bare reference.
    let stash_message = this
        .state
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .and_then(|repo| match &repo.stashes {
            Loadable::Ready(stashes) => stashes
                .iter()
                .find(|stash| stash.index == index)
                .map(|stash| stash.message.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    let summary_line = if stash_message.is_empty() {
        format!("stash@{{{index}}}")
    } else {
        format!("stash@{{{index}}} {stash_message}")
    };

    div()
        .flex()
        .flex_col()
        .w(scaled_px(420.0))
        .child(popover_title(crate::i18n::tr("input.stash_branch.title")))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(summary_line),
        )
        .child(input_label(
            theme,
            crate::i18n::tr_str("prompts.create_branch.name_label"),
        ))
        .child(
            div()
                .px_2()
                .pb_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.create_branch.create_branch_input.clone()),
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
                    cancel_button("stash_branch_cancel", "stash_branch_cancel_hint", theme)
                        .focus_handle(this.stash_branch_focus.cancel.clone())
                        .on_click(theme, cx, |this, _e, window, cx| {
                            this.dismiss_prompt_popover(window, cx);
                        }),
                )
                .child(
                    components::Button::new(
                        "stash_branch_go",
                        crate::i18n::tr("input.stash_branch.create"),
                    )
                    .focus_handle(this.stash_branch_focus.submit.clone())
                    .separated_end_slot(hotkey_hint(theme, "stash_branch_go_hint", "Enter"))
                    .style(components::ButtonStyle::Filled)
                    .disabled(!can_submit)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.submit_stash_branch(window, cx);
                    }),
                ),
        )
}
