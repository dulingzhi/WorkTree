use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    name: String,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;

    ConfirmDialog::new(
        crate::i18n::tr("confirm.remote_remove.title"),
        DIALOG_420_WIDTH,
    )
    .text(
        theme,
        crate::i18n::t!("confirm.remote_remove.remote_line", name = name).into_owned(),
    )
    .render(
        theme,
        dialog_cancel_button(
            "remove_remote_cancel",
            "remove_remote_cancel_hint",
            theme,
            cx,
        ),
        components::Button::new(
            "remove_remote_go",
            crate::i18n::tr("confirm.remote_remove.remove"),
        )
        .style(components::ButtonStyle::Danger)
        .on_click(theme, cx, move |this, _e, _w, cx| {
            this.store.dispatch(Msg::RemoveRemote {
                repo_id,
                name: name.clone(),
            });
            this.close_popover(cx);
        }),
        cx,
    )
}
