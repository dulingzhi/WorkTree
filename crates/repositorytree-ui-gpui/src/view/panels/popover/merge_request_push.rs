use super::*;

use super::stash_prompt::checkable_option_row;

/// Push HEAD carrying `git push -o merge_request.*` options so GitLab opens
/// the merge request from the push itself. `merge_request.create` is
/// implicit; the four rows below map one-to-one onto the C# client's push
/// dialog options.
pub(super) fn panel(
    this: &mut PopoverHost,
    _repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let can_push = this.can_submit_mr_push();
    let scaled_px = super::popover_scaled_px_fn(cx);

    let mut body = div()
        .flex()
        .flex_col()
        .child(
            div()
                .id("mr_push_hint")
                .debug_selector(|| "mr_push_hint".to_string())
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::tr("input.mr_push.hint")),
        )
        .child(
            div()
                .id("mr_push_target_row")
                .debug_selector(|| "mr_push_target_row".to_string())
                .px_2()
                .py_1()
                .w_full()
                .min_w(px(0.0))
                .child(this.mr_push_target_input.clone()),
        )
        .child(
            checkable_option_row(
                "mr_push_pipeline_toggle",
                crate::i18n::tr("input.mr_push.pipeline"),
                theme,
                this.mr_push_merge_when_pipeline_succeeds,
                &this.mr_push_pipeline_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_merge_when_pipeline_succeeds =
                    !this.mr_push_merge_when_pipeline_succeeds;
                cx.notify();
            })),
        )
        .child(
            checkable_option_row(
                "mr_push_remove_source_toggle",
                crate::i18n::tr("input.mr_push.remove_source"),
                theme,
                this.mr_push_remove_source_branch,
                &this.mr_push_remove_source_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_remove_source_branch = !this.mr_push_remove_source_branch;
                cx.notify();
            })),
        )
        .child(
            checkable_option_row(
                "mr_push_mr_branch_toggle",
                crate::i18n::tr("input.mr_push.mr_branch"),
                theme,
                this.mr_push_push_to_mr_branch,
                &this.mr_push_mr_branch_focus_handle,
                cx,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _w, cx| {
                this.mr_push_push_to_mr_branch = !this.mr_push_push_to_mr_branch;
                cx.notify();
            })),
        );

    body = body.child(
        div()
            .px_2()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .child(
                cancel_button("mr_push_cancel", "mr_push_cancel_hint", theme)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.dismiss_prompt_popover(window, cx);
                    }),
            )
            .child(
                components::Button::new(
                    "mr_push_go",
                    crate::i18n::tr("input.mr_push.push"),
                )
                .separated_end_slot(super::hotkey_hint(theme, "mr_push_go_hint", "Enter"))
                .style(components::ButtonStyle::Filled)
                .disabled(!can_push)
                .on_click(theme, cx, |this, _e, window, cx| {
                    this.submit_mr_push(window, cx);
                }),
            ),
    );

    components::context_menu(
        theme,
        div()
            .id("mr_push_popover")
            .debug_selector(|| "mr_push_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(440.0))
            .child(popover_title(crate::i18n::tr("input.mr_push.title")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body),
    )
}
