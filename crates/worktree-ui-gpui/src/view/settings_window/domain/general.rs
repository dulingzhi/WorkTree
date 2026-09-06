//! General settings: theme, language, avatar, scale/density, fonts,
//! date/time and file-editing preferences.

use super::*;

/// Display label for a density tier, shared by the summary row and the option
/// rows so the two can never drift apart.
pub(in crate::view::settings_window) fn ui_density_label(
    density: crate::density::Density,
) -> SharedString {
    tr(match density {
        crate::density::Density::Comfortable => "settings.density.comfortable",
        crate::density::Density::Compact => "settings.density.compact",
    })
}

/// The theme rows, labels included, from a single pass over the theme list.
///
/// `ThemeMode::label` resolves a key by re-reading the user theme directory --
/// a `create_dir_all`, a `read_dir`, and a `metadata` per file, all of it ahead
/// of the memo that is supposed to make it cheap -- and the row processor below
/// runs on every layout pass while the dropdown is open. Taking the label off
/// the same `ThemeOption` the mode is built from spends that once per render
/// instead of once per visible row per frame.
pub(in crate::view::settings_window) fn settings_theme_mode_options()
-> Vec<(ThemeMode, SharedString)> {
    let themes = crate::theme::available_themes();
    let mut options = Vec::with_capacity(themes.len() + 1);
    options.push((
        ThemeMode::Automatic,
        SharedString::from(ThemeMode::Automatic.label()),
    ));
    options.extend(
        themes
            .into_iter()
            .map(|theme| (ThemeMode::Named(theme.key), SharedString::from(theme.label))),
    );
    options
}

pub(in crate::view::settings_window) fn settings_theme_modes() -> Vec<ThemeMode> {
    settings_theme_mode_options()
        .into_iter()
        .map(|(mode, _)| mode)
        .collect()
}

impl SettingsWindowView {
    /// Refills the font dropdown lists after the background system font scan
    /// completes. Construction may have captured the bundled-only fallback;
    /// this swaps in the scanned families once they exist.
    pub(in crate::view::settings_window) fn refresh_font_options(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if crate::font_preferences::system_font_catalog_ready() {
            self.ui_font_options = crate::font_preferences::ui_font_options();
            self.editor_font_options = crate::font_preferences::editor_font_options();
        }
        cx.notify();
    }

    pub(in crate::view::settings_window) fn custom_theme_folder_detail(&self) -> SharedString {
        session::user_themes_dir()
            .map(|path| path.display().to_string().into())
            .unwrap_or_else(|| tr("settings.common.unavailable"))
    }

    pub(in crate::view::settings_window) fn open_custom_theme_folder(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(path) = crate::theme::ensure_user_themes_dir_exists() else {
            self.push_main_window_toast(
                components::ToastKind::Error,
                t!("settings.theme.folder_toast_unavailable").into_owned(),
                cx,
            );
            return;
        };

        if let Err(err) = super::platform_open::open_path(&path) {
            self.push_main_window_toast(
                components::ToastKind::Error,
                t!("settings.theme.folder_open_failed", err = err).into_owned(),
                cx,
            );
        }
    }

    pub(in crate::view::settings_window) fn set_ui_scale_percent(
        &mut self,
        percent: u32,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let percent = ui_scale::set_current(cx, percent).percent;
        if self.ui_scale_percent == percent {
            return;
        }

        self.expanded_section = None;
        self.apply_ui_scale_percent(percent, window, cx);
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, root_window, cx| {
            view.apply_ui_scale_percent(percent, root_window, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_ui_density(
        &mut self,
        density: crate::density::Density,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_density == density {
            return;
        }

        self.ui_density = density;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _root_window, cx| {
            view.apply_ui_density(density, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_theme_mode(
        &mut self,
        mode: ThemeMode,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.theme_mode == mode {
            return;
        }

        self.theme_mode = mode.clone();
        self.theme = mode.resolve_theme(window.appearance());
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, root_window, cx| {
            view.popover_host.update(cx, |host, cx| {
                host.set_theme_mode(mode.clone(), root_window.appearance(), cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_language(
        &mut self,
        language: crate::i18n::Language,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.language == language {
            return;
        }

        self.language = language;
        self.expanded_section = None;
        crate::i18n::set_current(cx, language);
        // Rebuild the macOS native menu bar so its labels follow the new
        // language without a restart.
        #[cfg(target_os = "macos")]
        crate::app::refresh_macos_app_menus(cx);
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |_view, _window, cx| {
            cx.notify();
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_avatar_source(
        &mut self,
        source: crate::avatar_source::AvatarSource,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.avatar_source == source {
            return;
        }

        self.avatar_source = source;
        self.expanded_section = None;
        crate::avatar_source::set_current(source);
        self.persist_preferences(cx);
        // History rows, hover cards and the details pane all key off the
        // process-global source; one notify re-render swaps every URL.
        self.update_main_windows(cx, move |_view, _window, cx| {
            cx.notify();
        });
        cx.notify();
    }

    /// Summary value for the language row: the language's own name, with the
    /// resolved language appended when following the system.
    pub(in crate::view::settings_window) fn language_summary(&self) -> gpui::SharedString {
        self.language_option_label(self.language)
    }

    pub(in crate::view::settings_window) fn language_option_label(
        &self,
        language: crate::i18n::Language,
    ) -> gpui::SharedString {
        match language {
            crate::i18n::Language::System => {
                let resolved = crate::i18n::Language::from_key(language.resolved_locale())
                    .unwrap_or(crate::i18n::Language::English)
                    .native_label();
                crate::i18n::t!("app.language.system_with_resolved", language = resolved).into()
            }
            other => other.native_label().into(),
        }
    }

    pub(in crate::view::settings_window) fn set_ui_font_family(
        &mut self,
        family: String,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_font_family == family {
            return;
        }

        self.ui_font_family = family;
        self.expanded_section = None;
        crate::font_preferences::set_current(
            cx,
            self.ui_font_family.clone(),
            self.editor_font_family.clone(),
            self.use_font_ligatures,
        );
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.notify_font_preferences_changed(cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_editor_font_family(
        &mut self,
        family: String,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.editor_font_family == family {
            return;
        }

        self.editor_font_family = family;
        self.expanded_section = None;
        crate::font_preferences::set_current(
            cx,
            self.ui_font_family.clone(),
            self.editor_font_family.clone(),
            self.use_font_ligatures,
        );
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.notify_font_preferences_changed(cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_use_font_ligatures(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.use_font_ligatures == enabled {
            return;
        }

        self.use_font_ligatures = enabled;
        crate::font_preferences::set_current(
            cx,
            self.ui_font_family.clone(),
            self.editor_font_family.clone(),
            self.use_font_ligatures,
        );
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.notify_font_preferences_changed(cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_date_time_format(
        &mut self,
        format: DateTimeFormat,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.date_time_format == format {
            return;
        }

        self.date_time_format = format;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.popover_host.update(cx, |host, cx| {
                host.set_date_time_format(format, cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_timezone(
        &mut self,
        timezone: Timezone,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.timezone == timezone {
            return;
        }

        self.timezone = timezone;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.popover_host.update(cx, |host, cx| {
                host.set_timezone(timezone, cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_show_timezone(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.show_timezone == enabled {
            return;
        }

        self.show_timezone = enabled;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.popover_host.update(cx, |host, cx| {
                host.set_show_timezone(enabled, cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_auto_save_file_edits(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.auto_save_file_edits == next {
            return;
        }

        self.auto_save_file_edits = next;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_auto_save_file_edits(next, cx);
        });
        cx.notify();
    }

    /// One row per theme file the loader refused, named and with its reason.
    ///
    /// A rejected file is otherwise silent: it simply is not in the picker, the
    /// app falls back to a bundled theme, and the account of why only ever
    /// reaches stderr. After a schema break every custom theme in the folder is
    /// rejected at once, and "my theme is gone" has to be answerable from here.
    pub(in crate::view::settings_window) fn rejected_theme_rows(
        &self,
        theme: AppTheme,
    ) -> Vec<AnyElement> {
        crate::theme::runtime_theme_issues()
            .iter()
            .enumerate()
            .map(|(ix, issue)| {
                let name: SharedString = issue
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| issue.path.display().to_string())
                    .into();
                let message: SharedString = issue.message.clone().into();
                div()
                    .debug_selector(move || format!("settings_window_theme_rejected_{ix}"))
                    .w_full()
                    .min_w(px(0.0))
                    .px_2()
                    .pt_1()
                    .pb_3()
                    .flex()
                    .flex_col()
                    .items_stretch()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(settings_row_separator_color(theme))
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .child(svg_icon(
                                "icons/warning.svg",
                                theme.colors.status.warning.foreground,
                                px(13.0),
                            ))
                            .child(div().flex_1().min_w(px(0.0)).child(name)),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.0))
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(message),
                    )
                    .into_any_element()
            })
            .collect()
    }
}
