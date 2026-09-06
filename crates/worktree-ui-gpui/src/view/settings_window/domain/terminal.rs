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
}
