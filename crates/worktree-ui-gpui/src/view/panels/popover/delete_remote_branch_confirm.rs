use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    remote: String,
    branch: String,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let full = format!("{remote}/{branch}");

    ConfirmDialog::new(
        crate::i18n::tr("confirm.delete_remote_branch.title"),
        DIALOG_420_WIDTH,
    )
    .mono_value(theme, full)
    .text(theme, crate::i18n::tr("confirm.delete_remote_branch.body"))
    .command(theme, format!("git push {remote} --delete {branch}"))
    .render(
        theme,
        dialog_cancel_button(
            "delete_remote_branch_cancel",
            "delete_remote_branch_cancel_hint",
            theme,
            cx,
        ),
        components::Button::new(
            "delete_remote_branch_go",
            crate::i18n::tr("confirm.delete_remote_branch.delete"),
        )
        .style(components::ButtonStyle::Danger)
        .on_click(theme, cx, move |this, _e, _w, cx| {
            this.store.dispatch(Msg::DeleteRemoteBranch {
                repo_id,
                remote: remote.clone(),
                branch: branch.clone(),
            });
            this.close_popover(cx);
        }),
        cx,
    )
}
