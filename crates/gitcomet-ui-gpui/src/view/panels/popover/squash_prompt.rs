use super::*;

fn short_sha(id: &gitcomet_core::domain::CommitId) -> &str {
    id.as_ref().get(0..8).unwrap_or(id.as_ref())
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);

    // Recompute eligibility from live state each render: the selection or the
    // log may have changed while the prompt is open. Prefill of the inputs is
    // handled outside the render path (see `sync_squash_prompt_prefill`).
    let repo = this.state.repos.iter().find(|r| r.id == repo_id);
    let plan = this.squash_plan_for_repo_id(repo_id);
    let preview = repo
        .map(|repo| repo.history_state.squash_preview.clone())
        .unwrap_or(Loadable::NotLoaded);

    let cancel_button = |this: &PopoverHost, cx: &mut gpui::Context<PopoverHost>| {
        super::cancel_button("squash_cancel", "squash_cancel_hint", theme)
            .focus_handle(this.squash_cancel_focus_handle.clone())
            .on_click(theme, cx, |this, _e, window, cx| {
                this.dismiss_prompt_popover(window, cx);
            })
    };

    let Some(plan) = plan else {
        return div()
            .flex()
            .flex_col()
            .w(scaled_px(420.0))
            .child(popover_title(crate::i18n::tr("prompts.squash.title_empty")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr("prompts.squash.not_squashable")),
            )
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(cancel_button(this, cx)),
            );
    };

    let message_empty = this
        .squash_message_input
        .read_with(cx, |input, _| input.text().trim().is_empty());

    let summary = if plan.head == plan.actual_head {
        crate::i18n::t!(
            "prompts.squash.summary_head",
            oldest = short_sha(&plan.oldest),
            head = short_sha(&plan.head)
        )
        .into_owned()
    } else {
        crate::i18n::t!(
            "prompts.squash.summary_rewrite",
            oldest = short_sha(&plan.oldest),
            head = short_sha(&plan.head)
        )
        .into_owned()
    };
    let message_hint: Option<SharedString> = match &preview {
        Loadable::Loading | Loadable::NotLoaded => {
            Some(crate::i18n::tr("prompts.squash.building_message"))
        }
        Loadable::Error(e) => Some(
            crate::i18n::t!("prompts.squash.build_error", error = e)
                .into_owned()
                .into(),
        ),
        Loadable::Ready(_) => None,
    };

    let description_scroll = this.squash_description_scroll.clone();
    let count = plan.commit_count;

    div()
        .flex()
        .flex_col()
        .w(scaled_px(420.0))
        .child(popover_title(
            crate::i18n::t!("prompts.squash.title_count", count = count).into_owned(),
        ))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(summary),
        )
        .child(
            div()
                .px_2()
                .pt_1()
                .pb_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("prompts.squash.commit_message")),
                )
                .child(
                    div()
                        .w_full()
                        .min_w(px(0.0))
                        .child(this.squash_message_input.clone()),
                ),
        )
        .child(
            div()
                .px_2()
                .pt_1()
                .pb_2()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr("prompts.squash.description")),
                )
                .child(
                    components::ScrollContainer::vertical(
                        "squash_description_scroll_surface",
                        "squash_description_scrollbar",
                        description_scroll,
                        scaled_px(180.0),
                    )
                    .render(theme, this.squash_description_input.clone()),
                ),
        )
        .when_some(message_hint, |el, hint| {
            el.child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(hint),
            )
        })
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            div()
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .justify_between()
                .child(cancel_button(this, cx))
                .child(
                    components::Button::new("squash_go", crate::i18n::tr("prompts.squash.submit"))
                        .focus_handle(this.squash_submit_focus_handle.clone())
                        .style(components::ButtonStyle::Filled)
                        .disabled(message_empty)
                        .on_click(theme, cx, move |this, _e, _w, cx| {
                            this.submit_squash(cx);
                        }),
                ),
        )
}
