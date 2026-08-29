use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;

    ConfirmDialog::new(
        crate::i18n::tr("confirm.pull_reconcile.title"),
        DIALOG_440_WIDTH,
    )
    .text(theme, crate::i18n::tr("confirm.pull_reconcile.body"))
    .command(
        theme,
        crate::i18n::tr("confirm.pull_reconcile.command_merge"),
    )
    .command(
        theme,
        crate::i18n::tr("confirm.pull_reconcile.command_rebase"),
    )
    .render(
        theme,
        dialog_cancel_button(
            "pull_reconcile_cancel",
            "pull_reconcile_cancel_hint",
            theme,
            cx,
        ),
        div()
            .flex()
            .gap_1()
            .child(
                components::Button::new(
                    "pull_reconcile_merge",
                    crate::i18n::tr("confirm.pull_reconcile.merge"),
                )
                .style(components::ButtonStyle::Filled)
                .on_click(theme, cx, move |this, _e, _w, cx| {
                    this.store.dispatch(Msg::Pull {
                        repo_id,
                        mode: PullMode::Merge,
                    });
                    this.close_popover(cx);
                }),
            )
            .child(
                components::Button::new(
                    "pull_reconcile_rebase",
                    crate::i18n::tr("confirm.pull_reconcile.rebase"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, cx| {
                    this.store.dispatch(Msg::Pull {
                        repo_id,
                        mode: PullMode::Rebase,
                    });
                    this.close_popover(cx);
                }),
            ),
        cx,
    )
}
