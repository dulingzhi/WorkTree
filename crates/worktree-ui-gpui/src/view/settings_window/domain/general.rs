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

    settings_setter!(
        set_ui_density,
        ui_density,
        crate::density::Density,
        density,
        _root_window,
        apply_ui_density,
        reset_section
    );

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

    settings_font_setter!(
        set_ui_font_family,
        ui_font_family,
        String,
        family,
        reset_section
    );

    settings_font_setter!(
        set_editor_font_family,
        editor_font_family,
        String,
        family,
        reset_section
    );

    settings_font_setter!(set_use_font_ligatures, use_font_ligatures, bool, enabled);

    settings_setter!(
        set_date_time_format,
        date_time_format,
        DateTimeFormat,
        format,
        _window,
        set_date_time_format,
        popover,
        reset_section
    );

    settings_setter!(
        set_timezone,
        timezone,
        Timezone,
        timezone,
        _window,
        set_timezone,
        popover,
        reset_section
    );

    settings_setter!(
        set_show_timezone,
        show_timezone,
        bool,
        enabled,
        _window,
        set_show_timezone,
        popover
    );

    settings_setter!(
        set_auto_save_file_edits,
        auto_save_file_edits,
        bool,
        next,
        _window,
        set_auto_save_file_edits
    );

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

    pub(in crate::view::settings_window) fn general_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme_row = self
            .summary_row(
                "settings_window_theme",
                tr_str("settings.row.theme"),
                self.theme_mode.label().into(),
                self.expanded_section == Some(SettingsSection::Theme),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::Theme, cx);
            }));

        let language_row = self
            .summary_row(
                "settings_window_language",
                crate::i18n::tr_str("app.language.title"),
                self.language_summary(),
                self.expanded_section == Some(SettingsSection::Language),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::Language, cx);
            }));

        let avatar_source_row = self
            .summary_row(
                "settings_window_avatar_source",
                tr_str("settings.row.avatar_source"),
                self.avatar_source.label(),
                self.expanded_section == Some(SettingsSection::AvatarSource),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::AvatarSource, cx);
            }));

        let date_format_row = self
            .summary_row(
                "settings_window_date_format",
                tr_str("settings.row.date_format"),
                self.date_time_format.label().into(),
                self.expanded_section == Some(SettingsSection::DateFormat),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::DateFormat, cx);
            }));

        let ui_scale_row = self
            .summary_row(
                "settings_window_ui_scale",
                tr_str("settings.row.ui_scale"),
                ui_scale::label(self.ui_scale_percent).into(),
                self.expanded_section == Some(SettingsSection::UiScale),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::UiScale, cx);
            }));

        let ui_density_row = self
            .summary_row(
                "settings_window_ui_density",
                tr_str("settings.row.ui_density"),
                ui_density_label(self.ui_density),
                self.expanded_section == Some(SettingsSection::UiDensity),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::UiDensity, cx);
            }));

        let ui_font_row = self
            .summary_row(
                "settings_window_ui_font",
                tr_str("settings.row.ui_font"),
                crate::font_preferences::display_label(&self.ui_font_family).into(),
                self.expanded_section == Some(SettingsSection::UiFont),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::UiFont, cx);
            }));

        let editor_font_row = self
            .summary_row(
                "settings_window_editor_font",
                tr_str("settings.row.editor_font"),
                crate::font_preferences::display_label(&self.editor_font_family).into(),
                self.expanded_section == Some(SettingsSection::EditorFont),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::EditorFont, cx);
            }));

        let font_ligatures_row = self
            .toggle_row(
                "settings_window_use_font_ligatures",
                tr_str("settings.row.font_ligatures"),
                self.use_font_ligatures,
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_use_font_ligatures(!this.use_font_ligatures, cx);
            }));

        let external_editor_row = self
            .summary_row(
                "settings_window_external_code_editor",
                tr_str("settings.row.external_code_editor"),
                crate::external_editor::label_for_setting(self.external_editor_setting.as_ref())
                    .into(),
                self.expanded_section == Some(SettingsSection::ExternalCodeEditor),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::ExternalCodeEditor, cx);
            }));

        let ai_commit_row = self
            .summary_row(
                "settings_window_ai_commit",
                tr_str("settings.row.ai_commit"),
                self.ai_commit_summary(),
                self.expanded_section == Some(SettingsSection::AiCommitMessage),
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::AiCommitMessage, cx);
            }));

        let timezone_row = self
            .summary_row(
                "settings_window_timezone",
                tr_str("settings.row.date_timezone"),
                self.timezone.label().into(),
                self.expanded_section == Some(SettingsSection::Timezone),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::Timezone, cx);
            }));

        let show_timezone_row = self
            .toggle_row(
                "settings_window_show_timezone",
                tr_str("settings.row.show_timezone"),
                self.show_timezone,
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_show_timezone(!this.show_timezone, cx);
            }));
        let mut general_card = self
            .card(
                "settings_window_general",
                tr_str("settings.nav.general"),
                theme,
            )
            .child(self.subsection_heading(
                "settings_window_general_appearance",
                tr_str("settings.section.appearance"),
                theme,
            ))
            .child(theme_row);

        if self.expanded_section == Some(SettingsSection::Theme) {
            let theme_mode_count = settings_theme_modes().len();
            let list = uniform_list(
                "settings_window_theme_list",
                theme_mode_count,
                cx.processor(Self::render_theme_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.theme_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(self.theme_scroll.clone()));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_theme_list_container",
                "settings_window_theme_scrollbar",
                self.theme_scroll.clone(),
                theme_mode_count,
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_THEME_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
            general_card = general_card.child(
                self.detail_container("settings_window_theme_links_container", theme)
                    // Above the folder link, so a theme that is
                    // missing from the list above is explained right
                    // next to the way to go and fix it.
                    .children(self.rejected_theme_rows(theme))
                    .child(
                        self.link_row(
                            "settings_window_theme_custom_folder",
                            tr_str("settings.row.open_theme_folder"),
                            self.custom_theme_folder_detail(),
                            theme,
                        )
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.open_custom_theme_folder(cx);
                            },
                        )),
                    )
                    .child(
                        self.link_row(
                            "settings_window_theme_guide",
                            tr_str("settings.row.theme_guide"),
                            THEMES_GUIDE_URL.into(),
                            theme,
                        )
                        .border_color(no_separator)
                        .on_click(|_, _, cx| {
                            cx.open_url(THEMES_GUIDE_URL);
                        }),
                    ),
            );
        }

        general_card = general_card.child(language_row);
        if self.expanded_section == Some(SettingsSection::Language) {
            let language_count = crate::i18n::Language::ALL.len();
            let list = uniform_list(
                "settings_window_language_list",
                language_count,
                cx.processor(Self::render_language_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.language_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(self.language_scroll.clone()));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_language_list_container",
                "settings_window_language_scrollbar",
                self.language_scroll.clone(),
                language_count,
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        general_card = general_card.child(avatar_source_row);
        if self.expanded_section == Some(SettingsSection::AvatarSource) {
            let source_count = crate::avatar_source::AvatarSource::ALL.len();
            let list = uniform_list(
                "settings_window_avatar_source_list",
                source_count,
                cx.processor(Self::render_avatar_source_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.avatar_source_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.avatar_source_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_avatar_source_list_container",
                "settings_window_avatar_source_scrollbar",
                self.avatar_source_scroll.clone(),
                source_count,
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        general_card = general_card.child(ui_scale_row);
        if self.expanded_section == Some(SettingsSection::UiScale) {
            let mut detail = self.detail_container("settings_window_ui_scale_container", theme);
            for percent in ui_scale::UI_SCALE_PRESETS.iter().copied() {
                let detail_text = match percent {
                    ui_scale::DEFAULT_UI_SCALE_PERCENT => {
                        Some(tr("settings.ui_scale.detail_default"))
                    }
                    80 | 90 => Some(tr("settings.ui_scale.detail_fit_more")),
                    110 | 125 | 150 => Some(tr("settings.ui_scale.detail_larger")),
                    _ => None,
                };
                detail = detail.child(
                    self.option_row(
                        format!("settings_window_ui_scale_{percent}"),
                        ui_scale::label(percent),
                        detail_text,
                        self.ui_scale_percent == percent,
                        theme,
                    )
                    .on_click(cx.listener(
                        move |this, _e: &ClickEvent, window, cx| {
                            this.set_ui_scale_percent(percent, window, cx);
                        },
                    )),
                );
            }
            general_card = general_card.child(
                detail.child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.ui_scale.shortcut")),
                ),
            );
        }

        general_card = general_card.child(ui_density_row);
        if self.expanded_section == Some(SettingsSection::UiDensity) {
            let mut detail = self.detail_container("settings_window_ui_density_container", theme);
            for (density, detail_text) in [
                (
                    crate::density::Density::Comfortable,
                    tr("settings.density.detail_comfortable"),
                ),
                (
                    crate::density::Density::Compact,
                    tr("settings.density.detail_compact"),
                ),
            ] {
                detail = detail.child(
                    self.option_row(
                        format!("settings_window_ui_density_{}", density.key()),
                        ui_density_label(density),
                        Some(detail_text),
                        self.ui_density == density,
                        theme,
                    )
                    .on_click(cx.listener(
                        move |this, _e: &ClickEvent, _window, cx| {
                            this.set_ui_density(density, cx);
                        },
                    )),
                );
            }
            general_card = general_card.child(detail);
        }

        general_card = general_card.child(ui_font_row);
        if self.expanded_section == Some(SettingsSection::UiFont) {
            let list = if self.ui_font_options.is_empty() {
                self.empty_dropdown_list(tr_str("settings.fonts.empty"), theme)
            } else {
                restrict_scroll_to_vertical_axis(
                    uniform_list(
                        "settings_window_ui_font_list",
                        self.ui_font_options.len(),
                        cx.processor(Self::render_ui_font_option_rows),
                    )
                    .w_full()
                    .min_w(px(0.0))
                    .h_full()
                    .min_h(px(0.0))
                    .track_scroll(&self.ui_font_scroll)
                    .on_scroll_wheel(stop_dropdown_wheel_chaining(self.ui_font_scroll.clone())),
                )
                .into_any_element()
            };
            general_card = general_card
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(self.font_options_hint(self.ui_font_family.as_str())),
                )
                .child(self.dropdown_list_container(
                    "settings_window_ui_font_list_container",
                    "settings_window_ui_font_scrollbar",
                    self.ui_font_scroll.clone(),
                    self.ui_font_options.len(),
                    SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                    0.0,
                    SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                    list,
                    theme,
                ));
        }

        general_card = general_card.child(editor_font_row);
        if self.expanded_section == Some(SettingsSection::EditorFont) {
            let list = if self.editor_font_options.is_empty() {
                self.empty_dropdown_list(tr_str("settings.fonts.empty"), theme)
            } else {
                restrict_scroll_to_vertical_axis(
                    uniform_list(
                        "settings_window_editor_font_list",
                        self.editor_font_options.len(),
                        cx.processor(Self::render_editor_font_option_rows),
                    )
                    .w_full()
                    .min_w(px(0.0))
                    .h_full()
                    .min_h(px(0.0))
                    .track_scroll(&self.editor_font_scroll)
                    .on_scroll_wheel(stop_dropdown_wheel_chaining(
                        self.editor_font_scroll.clone(),
                    )),
                )
                .into_any_element()
            };
            general_card = general_card
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(self.font_options_hint(self.editor_font_family.as_str())),
                )
                .child(self.dropdown_list_container(
                    "settings_window_editor_font_list_container",
                    "settings_window_editor_font_scrollbar",
                    self.editor_font_scroll.clone(),
                    self.editor_font_options.len(),
                    SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                    0.0,
                    SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                    list,
                    theme,
                ));
        }

        general_card = general_card.child(font_ligatures_row);

        general_card = general_card
            .child(self.subsection_heading(
                "settings_window_general_integrations",
                tr_str("settings.section.integrations"),
                theme,
            ))
            .child(external_editor_row);
        if self.expanded_section == Some(SettingsSection::ExternalCodeEditor) {
            let list = uniform_list(
                "settings_window_external_code_editor_list",
                self.external_editor_options.len(),
                cx.processor(Self::render_external_editor_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.external_editor_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.external_editor_scroll.clone(),
            ))
            .into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_external_code_editor_list_container",
                "settings_window_external_code_editor_scrollbar",
                self.external_editor_scroll.clone(),
                self.external_editor_options.len(),
                SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        if self.external_editor_is_custom() {
            let browse_button = components::Button::new(
                "settings_window_external_code_editor_browse",
                tr("settings.action.browse"),
            )
            .style(components::ButtonStyle::Outlined)
            .on_click(theme, cx, |_this, _e, window, cx| {
                let view = cx.weak_entity();
                let rx = cx.prompt_for_paths(custom_external_editor_path_prompt_options());

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
                        let _ = view.update(cx, |this, cx| {
                            this.apply_browsed_external_editor_path(path, cx);
                        });
                    })
                    .detach();
            });

            general_card = general_card.child(
                self.detail_container(
                    "settings_window_external_code_editor_custom_container",
                    theme,
                )
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.external_editor.custom_executable")),
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
                                .child(self.external_editor_custom_path_input.clone()),
                        )
                        .child(browse_button),
                )
                .child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.external_editor.arguments")),
                )
                .child(
                    div()
                        .px_2()
                        .pb_1()
                        .w_full()
                        .min_w(px(0.0))
                        .child(self.external_editor_custom_arguments_input.clone()),
                ),
            );
        }

        general_card = general_card.child(ai_commit_row);
        if self.expanded_section == Some(SettingsSection::AiCommitMessage) {
            use crate::ai_commit_sources::AiSource;

            // The source dropdown leads — it decides which of the
            // sections below appear at all.
            general_card = general_card.child(
                div()
                    .px_2()
                    .pt_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(tr_str("settings.ai_commit.source_heading")),
            );
            let source_count = AiSource::ALL.len();
            let list = uniform_list(
                "settings_window_ai_commit_source_list",
                source_count,
                cx.processor(Self::render_ai_commit_source_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.ai_commit_source_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.ai_commit_source_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_ai_commit_source_list_container",
                "settings_window_ai_commit_source_scrollbar",
                self.ai_commit_source_scroll.clone(),
                source_count,
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));

            if self.ai_commit_source != AiSource::Manual {
                // Availability of the selected source, computed in
                // the background; blank while the check is in
                // flight.
                let (status_text, status_color) = match &self.ai_commit_availability {
                    None => (
                        tr("settings.ai_commit.checking"),
                        theme.colors.foreground.secondary,
                    ),
                    Some(availability) if availability.detected => (
                        tr("settings.ai_commit.available"),
                        theme.colors.status.success.foreground,
                    ),
                    Some(availability) => {
                        let text = match &availability.message {
                            Some((key, Some(detail))) => {
                                crate::i18n::t!(*key, detail = detail).into_owned()
                            }
                            Some((key, None)) => crate::i18n::t!(*key).into_owned(),
                            None => String::new(),
                        };
                        (SharedString::from(text), theme.colors.foreground.secondary)
                    }
                };
                general_card = general_card.child(
                    div()
                        .id("settings_window_ai_commit_availability")
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_text),
                );
                let hint_key = if self.ai_commit_source.is_cli() {
                    "settings.ai_commit.privacy_hint_cli"
                } else {
                    "settings.ai_commit.privacy_hint_external"
                };
                general_card = general_card.child(
                    div()
                        .id("settings_window_ai_commit_source_hint")
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr(hint_key)),
                );
            }

            if self.ai_commit_source == AiSource::Custom {
                general_card = general_card.child(
                    self.detail_container("settings_window_ai_commit_custom_container", theme)
                        .child(
                            div()
                                .px_2()
                                .pt_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.ai_commit.custom_command")),
                        )
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .w_full()
                                .min_w(px(0.0))
                                .child(self.ai_commit_custom_command_input.clone()),
                        )
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.ai_commit.custom_command_hint")),
                        ),
                );
            }

            if self.ai_commit_source == AiSource::Manual {
                general_card = general_card.child(
                    div()
                        .px_2()
                        .pt_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.ai_commit.provider_heading")),
                );
                let provider_count = crate::ai_commit::AiProvider::ALL.len();
                let list = uniform_list(
                    "settings_window_ai_commit_provider_list",
                    provider_count,
                    cx.processor(Self::render_ai_commit_provider_option_rows),
                )
                .w_full()
                .min_w(px(0.0))
                .h_full()
                .min_h(px(0.0))
                .track_scroll(&self.ai_commit_provider_scroll)
                .on_scroll_wheel(stop_dropdown_wheel_chaining(
                    self.ai_commit_provider_scroll.clone(),
                ));
                let list = restrict_scroll_to_vertical_axis(list).into_any_element();
                general_card = general_card.child(self.dropdown_list_container(
                    "settings_window_ai_commit_provider_list_container",
                    "settings_window_ai_commit_provider_scrollbar",
                    self.ai_commit_provider_scroll.clone(),
                    provider_count,
                    SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                    SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                    SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                    list,
                    theme,
                ));
                general_card = general_card.child(
                    self.detail_container("settings_window_ai_commit_fields_container", theme)
                        .child(
                            div()
                                .px_2()
                                .pt_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.ai_commit.model")),
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
                                        .child(self.ai_commit_model_input.clone()),
                                )
                                .child(
                                    components::Button::new(
                                        "settings_window_ai_commit_models_fetch",
                                        tr("settings.ai_commit.models_fetch"),
                                    )
                                    .style(components::ButtonStyle::Outlined)
                                    .disabled(matches!(
                                        self.ai_commit_models,
                                        AiCommitModels::Loading
                                    ))
                                    .on_click(
                                        theme,
                                        cx,
                                        |this, _e, _window, cx| {
                                            this.fetch_ai_commit_models(cx);
                                        },
                                    ),
                                ),
                        ),
                );

                match &self.ai_commit_models {
                    AiCommitModels::NotFetched => {}
                    AiCommitModels::Loading => {
                        general_card = general_card.child(
                            div()
                                .id("settings_window_ai_commit_models_loading")
                                .px_2()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.ai_commit.models_loading")),
                        );
                    }
                    AiCommitModels::Error(error) => {
                        general_card = general_card.child(
                            div()
                                .id("settings_window_ai_commit_models_error")
                                .px_2()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(t!("settings.ai_commit.models_failed", error = error)),
                        );
                    }
                    AiCommitModels::Ready(models) => {
                        let list = uniform_list(
                            "settings_window_ai_commit_model_list",
                            models.len(),
                            cx.processor(Self::render_ai_commit_model_option_rows),
                        )
                        .w_full()
                        .min_w(px(0.0))
                        .h_full()
                        .min_h(px(0.0))
                        .track_scroll(&self.ai_commit_models_scroll)
                        .on_scroll_wheel(stop_dropdown_wheel_chaining(
                            self.ai_commit_models_scroll.clone(),
                        ));
                        let list = restrict_scroll_to_vertical_axis(list).into_any_element();
                        general_card = general_card.child(self.dropdown_list_container(
                            "settings_window_ai_commit_model_list_container",
                            "settings_window_ai_commit_model_scrollbar",
                            self.ai_commit_models_scroll.clone(),
                            models.len(),
                            SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                            SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                            SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                            list,
                            theme,
                        ));
                    }
                }

                general_card = general_card
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.ai_commit.api_key")),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .w_full()
                            .min_w(px(0.0))
                            .child(self.ai_commit_api_key_input.clone()),
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.ai_commit.endpoint")),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .w_full()
                            .min_w(px(0.0))
                            .child(self.ai_commit_endpoint_input.clone()),
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.ai_commit.privacy_hint")),
                    );
            }
        }

        general_card = general_card
            .child(self.subsection_heading(
                "settings_window_general_date_time",
                tr_str("settings.section.date_time"),
                theme,
            ))
            .child(date_format_row);
        if self.expanded_section == Some(SettingsSection::DateFormat) {
            let list = uniform_list(
                "settings_window_date_format_list",
                DateTimeFormat::all().len(),
                cx.processor(Self::render_date_format_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.date_format_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.date_format_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_date_format_list_container",
                "settings_window_date_format_scrollbar",
                self.date_format_scroll.clone(),
                DateTimeFormat::all().len(),
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        general_card = general_card.child(timezone_row);
        if self.expanded_section == Some(SettingsSection::Timezone) {
            let list = uniform_list(
                "settings_window_timezone_list",
                Timezone::all().len(),
                cx.processor(Self::render_timezone_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.timezone_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(self.timezone_scroll.clone()));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            general_card = general_card.child(self.dropdown_list_container(
                "settings_window_timezone_list_container",
                "settings_window_timezone_scrollbar",
                self.timezone_scroll.clone(),
                Timezone::all().len(),
                SETTINGS_DROPDOWN_DENSE_DETAIL_ROW_HEIGHT_PX,
                0.0,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        general_card = general_card.child(show_timezone_row);
        general_card
    }

    pub(in crate::view::settings_window) fn file_editing_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.card(
            "settings_window_file_editing_card",
            tr_str("settings.nav.file_editing"),
            theme,
        )
        .child(
            self.toggle_row(
                "settings_window_auto_save_file_edits",
                tr_str("settings.row.auto_save"),
                self.auto_save_file_edits,
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_auto_save_file_edits(!this.auto_save_file_edits, cx);
            })),
        )
    }
}
