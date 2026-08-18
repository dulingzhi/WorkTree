use super::*;

/// Shown when closing the window or quitting would throw away buffers the file
/// editor is still holding.
///
/// Only reachable with auto-save off — with it on the buffers are already on
/// disk by the time anything can close.
pub(super) fn panel(
    this: &mut PopoverHost,
    prompt: UnsavedFileEditsPrompt,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let (title, discard_label) = match prompt.action {
        UnsavedFileEditsAction::CloseWindow(_) => (
            crate::i18n::tr("confirm.common.close_window_title"),
            crate::i18n::tr("confirm.unsaved_file_edits.discard_and_close"),
        ),
        UnsavedFileEditsAction::QuitApp => (
            crate::i18n::tr("confirm.common.quit_title"),
            crate::i18n::tr("confirm.unsaved_file_edits.discard_and_quit"),
        ),
    };
    let detail = if prompt.files.len() == 1 {
        crate::i18n::t!("confirm.unsaved_file_edits.text_one").into_owned()
    } else {
        crate::i18n::t!(
            "confirm.unsaved_file_edits.text_many",
            count = prompt.files.len()
        )
        .into_owned()
    };

    let action = prompt.action;
    let dialog = ConfirmDialog::new(title, DIALOG_440_WIDTH)
        .text(theme, detail)
        .section(
            div()
                .px_2()
                .pb_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        // Capped: a long list would push the buttons out of the
                        // dialog, and the count above already says how many.
                        .children(prompt.files.iter().take(8).map(|label| {
                            div()
                                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                                .ml_2()
                                .child(label.clone())
                        }))
                        .when(prompt.files.len() > 8, |d| {
                            d.child(
                                div().ml_2().child(
                                    crate::i18n::t!(
                                        "prompts.gitignore.more_note",
                                        count = prompt.files.len() - 8
                                    )
                                    .into_owned(),
                                ),
                            )
                        }),
                ),
        );

    dialog.render(
        theme,
        cancel_button(
            "unsaved_file_edits_cancel",
            "unsaved_file_edits_cancel_hint",
            theme,
        )
        .on_click(theme, cx, |this, _e, _window, cx| {
            let root_view = this.root_view.clone();
            let _ = root_view.update(cx, |root, cx| {
                root.clear_pending_unsaved_file_edits_prompt(cx);
            });
            this.close_popover(cx);
        }),
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                components::Button::new("unsaved_file_edits_discard", discard_label)
                    .style(components::ButtonStyle::Danger)
                    .on_click(theme, cx, move |this, _e, _window, cx| {
                        let root_view = this.root_view.clone();
                        let _ = root_view.update(cx, |root, cx| {
                            root.resolve_unsaved_file_edits(action, false, cx);
                        });
                        this.close_popover(cx);
                    }),
            )
            .child(
                components::Button::new(
                    "unsaved_file_edits_save",
                    crate::i18n::tr("confirm.unsaved_file_edits.save_all"),
                )
                .style(components::ButtonStyle::Filled)
                .on_click(theme, cx, move |this, _e, _window, cx| {
                    let root_view = this.root_view.clone();
                    let _ = root_view.update(cx, |root, cx| {
                        root.resolve_unsaved_file_edits(action, true, cx);
                    });
                    this.close_popover(cx);
                }),
            ),
        cx,
    )
}
