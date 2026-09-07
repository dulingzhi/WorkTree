//! Terminal settings: external-terminal draft inputs, launch testing and
//! action-bar target selection.

use super::*;

#[derive(Clone, Debug)]
pub(in crate::view::settings_window) struct TerminalSettingsStatus {
    pub(in crate::view::settings_window) is_error: bool,
    pub(in crate::view::settings_window) text: SharedString,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view::settings_window) enum TerminalProgramInputTarget {
    ExternalTerminal,
}

impl SettingsWindowView {
    fn apply_terminal_preferences_change(
        &mut self,
        next: TerminalPreferences,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.terminal_preferences == next {
            return;
        }

        self.terminal_preferences = next.clone();
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.apply_terminal_preferences(next.clone(), cx);
        });
        cx.notify();
    }

    fn set_terminal_status(
        &mut self,
        is_error: bool,
        text: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.terminal_status = Some(TerminalSettingsStatus {
            is_error,
            text: text.into(),
        });
        cx.notify();
    }

    fn external_terminal_preferences_with_drafts(
        &self,
        cx: &gpui::Context<Self>,
    ) -> TerminalPreferences {
        let mut preferences = self.terminal_preferences.clone();
        preferences.external_terminal_program = self
            .terminal_external_program_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        let args_raw = self
            .terminal_external_args_input
            .read_with(cx, |input, _| input.text().to_string());
        preferences.external_terminal_args = parse_terminal_args_multiline(&args_raw);
        preferences
    }

    pub(in crate::view::settings_window) fn set_external_terminal_mode(
        &mut self,
        mode: ExternalTerminalMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.terminal_preferences.external_terminal_mode == mode {
            return;
        }

        let mut next = self.terminal_preferences.clone();
        next.external_terminal_mode = mode;
        self.terminal_status = None;
        self.apply_terminal_preferences_change(next, cx);
    }

    pub(in crate::view::settings_window) fn set_action_bar_terminal_target(
        &mut self,
        target: ActionBarTerminalTarget,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.terminal_preferences.action_bar_terminal_target == target {
            return;
        }

        let mut next = self.terminal_preferences.clone();
        next.action_bar_terminal_target = target;
        self.terminal_status = None;
        self.apply_terminal_preferences_change(next, cx);
    }

    pub(in crate::view::settings_window) fn save_terminal_external_draft(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = self.external_terminal_preferences_with_drafts(cx);
        self.apply_terminal_preferences_change(next, cx);
        self.set_terminal_status(false, tr("settings.terminal.status_saved"), cx);
    }

    pub(in crate::view::settings_window) fn reset_terminal_external_draft(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let program = self.terminal_preferences.external_terminal_program.clone();
        self.terminal_external_program_input
            .update(cx, |input, cx| input.set_text(program, cx));
        let args = self.terminal_preferences.external_args_multiline();
        self.terminal_external_args_input
            .update(cx, |input, cx| input.set_text(args, cx));
        self.set_terminal_status(false, tr("settings.terminal.status_reset"), cx);
    }

    pub(in crate::view::settings_window) fn browse_terminal_program_input(
        &mut self,
        target: TerminalProgramInputTarget,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let prompt = match target {
            TerminalProgramInputTarget::ExternalTerminal => {
                tr_str("settings.terminal.prompt_select_launcher")
            }
        };
        let allow_directories = cfg!(target_os = "macos");
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: allow_directories,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let view = cx.weak_entity();

        window
            .spawn(cx, async move |cx| {
                let result = rx.await;
                let paths = match result {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    Ok(Err(_)) | Err(_) => return,
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let rendered = path.display().to_string();
                let _ = view.update(cx, |this, cx| {
                    match target {
                        TerminalProgramInputTarget::ExternalTerminal => {
                            this.terminal_external_program_input
                                .update(cx, |input, cx| input.set_text(rendered.clone(), cx));
                        }
                    }
                    this.terminal_status = None;
                    cx.notify();
                });
            })
            .detach();
    }

    fn preferred_terminal_launch_context(
        &self,
        cx: &gpui::Context<Self>,
    ) -> ExternalTerminalLaunchContext {
        for handle in cx
            .windows()
            .into_iter()
            .filter_map(|window| window.downcast::<WorkTreeView>())
        {
            if let Ok(Some(context)) = handle.read_with(cx, |view, _cx| {
                view.terminal_launch_context_for_active_repo()
            }) {
                return context;
            }
        }

        ExternalTerminalLaunchContext {
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            repo_name: None,
        }
    }

    pub(in crate::view::settings_window) fn test_terminal_launch_from_draft(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let preferences = self.external_terminal_preferences_with_drafts(cx);
        let context = self.preferred_terminal_launch_context(cx);
        match launch_external_terminal_from_preferences(&preferences, &context) {
            Ok(()) => {
                self.set_terminal_status(false, tr("settings.terminal.status_launch_sent"), cx)
            }
            Err(err) => self.set_terminal_status(
                true,
                t!("settings.terminal.status_launch_failed", err = err).into_owned(),
                cx,
            ),
        }
    }

    pub(in crate::view::settings_window) fn terminal_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let terminal_external_row = self
            .summary_row(
                "settings_window_terminal_external",
                tr_str("settings.row.external_terminal"),
                self.terminal_preferences.external_summary().into(),
                self.expanded_section == Some(SettingsSection::TerminalExternal),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::TerminalExternal, cx);
            }));

        let terminal_action_bar_row = self
            .summary_row(
                "settings_window_terminal_action_bar",
                tr_str("settings.row.action_bar_terminal"),
                self.terminal_preferences
                    .action_bar_terminal_target
                    .label()
                    .into(),
                self.expanded_section == Some(SettingsSection::TerminalActionBar),
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::TerminalActionBar, cx);
            }));
        let mut terminal_card = self.card(
            "settings_window_terminal_card",
            tr_str("settings.nav.terminal"),
            theme,
        );

        terminal_card = terminal_card.child(terminal_external_row);
        if self.expanded_section == Some(SettingsSection::TerminalExternal) {
            terminal_card = terminal_card
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.terminal.note_best_effort")),
                )
                .child(
                    div()
                        .px_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            self.option_row(
                                "settings_window_terminal_external_default",
                                ExternalTerminalMode::SystemDefault.label(),
                                Some(tr("settings.terminal.default_detail")),
                                self.terminal_preferences.external_terminal_mode
                                    == ExternalTerminalMode::SystemDefault,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_external_terminal_mode(
                                        ExternalTerminalMode::SystemDefault,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            self.option_row(
                                "settings_window_terminal_external_custom",
                                ExternalTerminalMode::CustomProgram.label(),
                                Some(tr("settings.terminal.custom_detail")),
                                self.terminal_preferences.external_terminal_mode
                                    == ExternalTerminalMode::CustomProgram,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_external_terminal_mode(
                                        ExternalTerminalMode::CustomProgram,
                                        cx,
                                    );
                                },
                            )),
                        ),
                );

            if self.terminal_preferences.external_terminal_mode
                == ExternalTerminalMode::CustomProgram
            {
                terminal_card = terminal_card
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.terminal.program")),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .w_full()
                            .min_w(px(0.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(self.terminal_external_program_input.clone()),
                            )
                            .child(
                                components::Button::new(
                                    "settings_window_terminal_external_browse",
                                    tr("settings.action.browse"),
                                )
                                .style(components::ButtonStyle::Outlined)
                                .on_click(
                                    theme,
                                    cx,
                                    |this, _e, window, cx| {
                                        this.browse_terminal_program_input(
                                            TerminalProgramInputTarget::ExternalTerminal,
                                            window,
                                            cx,
                                        );
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.terminal.arguments")),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .w_full()
                            .min_w(px(0.0))
                            .child(self.terminal_external_args_input.clone()),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.terminal.args_hint")),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                components::Button::new(
                                    "settings_window_terminal_external_save",
                                    tr("settings.action.save"),
                                )
                                .style(components::ButtonStyle::Filled)
                                .on_click(
                                    theme,
                                    cx,
                                    |this, _e, _w, cx| {
                                        this.save_terminal_external_draft(cx);
                                    },
                                ),
                            )
                            .child(
                                components::Button::new(
                                    "settings_window_terminal_external_reset",
                                    tr("settings.action.reset"),
                                )
                                .style(components::ButtonStyle::Outlined)
                                .on_click(
                                    theme,
                                    cx,
                                    |this, _e, _w, cx| {
                                        this.reset_terminal_external_draft(cx);
                                    },
                                ),
                            )
                            .child(
                                components::Button::new(
                                    "settings_window_terminal_external_test",
                                    tr("settings.action.test_launch"),
                                )
                                .style(components::ButtonStyle::Outlined)
                                .on_click(
                                    theme,
                                    cx,
                                    |this, _e, _w, cx| {
                                        this.test_terminal_launch_from_draft(cx);
                                    },
                                ),
                            ),
                    );
            }
        }

        terminal_card = terminal_card.child(terminal_action_bar_row);
        if self.expanded_section == Some(SettingsSection::TerminalActionBar) {
            terminal_card = terminal_card
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.action_bar_terminal.note")),
                )
                .child(
                    div()
                        .px_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            self.option_row(
                                "settings_window_terminal_action_bar_embedded",
                                ActionBarTerminalTarget::Embedded.label(),
                                Some(tr("settings.action_bar_terminal.embedded_detail")),
                                self.terminal_preferences.action_bar_terminal_target
                                    == ActionBarTerminalTarget::Embedded,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_action_bar_terminal_target(
                                        ActionBarTerminalTarget::Embedded,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            self.option_row(
                                "settings_window_terminal_action_bar_external",
                                ActionBarTerminalTarget::External.label(),
                                Some(tr("settings.action_bar_terminal.external_detail")),
                                self.terminal_preferences.action_bar_terminal_target
                                    == ActionBarTerminalTarget::External,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_action_bar_terminal_target(
                                        ActionBarTerminalTarget::External,
                                        cx,
                                    );
                                },
                            )),
                        ),
                );
        }

        if let Some(status) = self.terminal_status.clone() {
            terminal_card = terminal_card.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(if status.is_error {
                        theme.colors.status.danger.foreground
                    } else {
                        theme.colors.status.success.foreground
                    })
                    .child(status.text),
            );
        }
        terminal_card
    }
}
