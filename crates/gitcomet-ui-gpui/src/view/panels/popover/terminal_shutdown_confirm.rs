use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    prompt: TerminalShutdownPrompt,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let terminal_label = if prompt.summary.terminal_count == 1 {
        crate::i18n::t!("confirm.terminal_shutdown.terminal_one").into_owned()
    } else {
        crate::i18n::t!(
            "confirm.terminal_shutdown.terminal_many",
            count = prompt.summary.terminal_count
        )
        .into_owned()
    };
    let title = match prompt.action {
        TerminalShutdownAction::QuitApp => crate::i18n::tr("confirm.common.quit_title"),
        TerminalShutdownAction::CloseWindow => crate::i18n::tr("confirm.common.close_window_title"),
        TerminalShutdownAction::CloseRepo { .. }
        | TerminalShutdownAction::CloseTerminalForRepo { .. }
        | TerminalShutdownAction::CloseTerminalTab { .. } => {
            crate::i18n::tr("confirm.terminal_shutdown.title")
        }
    };
    let confirm_label = match prompt.action {
        TerminalShutdownAction::QuitApp => {
            crate::i18n::tr("confirm.terminal_shutdown.terminate_and_quit")
        }
        TerminalShutdownAction::CloseWindow => {
            crate::i18n::tr("confirm.terminal_shutdown.terminate_and_close")
        }
        TerminalShutdownAction::CloseRepo { .. }
        | TerminalShutdownAction::CloseTerminalForRepo { .. }
        | TerminalShutdownAction::CloseTerminalTab { .. } => {
            crate::i18n::tr("confirm.terminal_shutdown.terminate_and_close")
        }
    };
    let detail = if prompt.summary.running_command_count == 1 {
        crate::i18n::t!("confirm.terminal_shutdown.text_one", label = terminal_label).into_owned()
    } else {
        crate::i18n::t!(
            "confirm.terminal_shutdown.text_many",
            count = prompt.summary.running_command_count,
            label = terminal_label
        )
        .into_owned()
    };

    let repo_names = &prompt.summary.repo_names;
    let show_repo_list = !repo_names.is_empty()
        && matches!(
            prompt.action,
            TerminalShutdownAction::CloseWindow | TerminalShutdownAction::QuitApp
        );

    let mut dialog = ConfirmDialog::new(title, DIALOG_440_WIDTH).text(theme, detail);
    if show_repo_list {
        dialog = dialog.section(
            div()
                .px_2()
                .pb_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .children(repo_names.iter().map(|name| {
                            div()
                                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                                .ml_2()
                                .child(name.clone())
                        })),
                ),
        );
    }

    dialog.render(
        theme,
        cancel_button(
            "terminal_shutdown_cancel",
            "terminal_shutdown_cancel_hint",
            theme,
        )
        .on_click(theme, cx, |this, _e, _window, cx| {
            let root_view = this.root_view.clone();
            let _ = root_view.update(cx, |root, cx| {
                root.clear_pending_terminal_shutdown_prompt(cx);
            });
            this.close_popover(cx);
        }),
        components::Button::new("terminal_shutdown_confirm", confirm_label)
            .style(components::ButtonStyle::Danger)
            .on_click(theme, cx, move |this, _e, window, cx| {
                let root_view = this.root_view.clone();
                let _ = root_view.update(cx, |root, cx| {
                    root.confirm_terminal_shutdown(prompt.clone(), window, cx);
                });
                this.close_popover(cx);
            }),
        cx,
    )
}
