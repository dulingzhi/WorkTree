use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    target: String,
    mode: ResetMode,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let mode_label = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };

    ConfirmDialog::new(crate::i18n::tr("confirm.reset.title"), DIALOG_380_WIDTH)
        .text(theme, format!("{mode_label} → {target}"))
        .note(
            theme,
            match mode {
                ResetMode::Hard => crate::i18n::tr("confirm.reset.note_hard"),
                ResetMode::Mixed => crate::i18n::tr("confirm.reset.note_mixed"),
                ResetMode::Soft => crate::i18n::tr("confirm.reset.note_soft"),
            },
        )
        .render(
            theme,
            dialog_cancel_button("reset_cancel", "reset_cancel_hint", theme, cx),
            components::Button::new("reset_go", crate::i18n::tr("confirm.reset.go"))
                .style(components::ButtonStyle::Filled)
                .on_click(theme, cx, move |this, _e, _w, cx| {
                    this.store.dispatch(Msg::Reset {
                        repo_id,
                        target: target.clone(),
                        mode,
                    });
                    this.close_popover(cx);
                }),
            cx,
        )
}
