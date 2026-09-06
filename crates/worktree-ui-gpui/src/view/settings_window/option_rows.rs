//! Option rows: the `option_row` family of row builders and the per-setting
//! `render_*_option_rows` dropdown row sources.

use super::*;
use gpui::Stateful;

impl SettingsWindowView {
    fn font_option_detail(&self, family: &str) -> Option<SharedString> {
        match family {
            crate::font_preferences::UI_SYSTEM_FONT_FAMILY => {
                Some(tr("settings.fonts.system_detail"))
            }
            _ => None,
        }
    }

    pub(super) fn font_options_hint(&self, family: &str) -> SharedString {
        self.font_option_detail(family)
            .unwrap_or_else(|| tr("settings.fonts.choose_hint"))
    }

    fn font_option_row_for_family(
        &self,
        id_prefix: &'static str,
        ix: usize,
        family: &str,
        selected: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        self.option_row(
            format!("{id_prefix}_{ix}"),
            crate::font_preferences::display_label(family),
            None,
            selected,
            theme,
        )
    }

    pub(super) fn option_row(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        detail: Option<SharedString>,
        selected: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let id: SharedString = id.into();
        let debug_id = id.clone();
        let text_color = if selected {
            theme.colors.foreground.primary
        } else {
            theme.colors.foreground.secondary
        };
        let selected_bg = with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.16 } else { 0.10 },
        );
        let hover_bg = theme.hover_overlay();
        let active_bg = theme.active_overlay();

        div()
            .id(id)
            .debug_selector(move || debug_id.to_string())
            .w_full()
            .px_2()
            .py_1()
            .flex()
            .items_start()
            .gap_2()
            .rounded(px(theme.radii.row))
            .cursor(CursorStyle::PointingHand)
            .bg(if selected {
                selected_bg
            } else {
                gpui::rgba(0x00000000)
            })
            .hover(move |s| {
                if selected {
                    s.bg(selected_bg)
                } else {
                    s.bg(hover_bg)
                }
            })
            .active(move |s| {
                if selected {
                    s.bg(selected_bg)
                } else {
                    s.bg(active_bg)
                }
            })
            .child(
                div()
                    .w(px(16.0))
                    // Match the label's line box so the check mark centers on
                    // the first text line instead of hugging the row's top.
                    .h(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |d| {
                        d.child(svg_icon(
                            "icons/check.svg",
                            theme.colors.accent.foreground,
                            px(12.0),
                        ))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .line_height(px(20.0))
                            .text_color(text_color)
                            .child(label.into()),
                    )
                    .when_some(detail, |this, detail| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .line_clamp(1)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .child(detail),
                        )
                    }),
            )
    }

    pub(super) fn setting_option_row(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        detail: Option<SharedString>,
        selected: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        self.option_row(id, label, detail, selected, theme)
            .rounded(px(0.0))
            .pb_3()
            .border_b_1()
            .border_color(settings_row_separator_color(theme))
    }

    fn dense_detail_option_row(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        selected: bool,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        let id: SharedString = id.into();
        let debug_id = id.clone();
        let text_color = if selected {
            theme.colors.foreground.primary
        } else {
            theme.colors.foreground.secondary
        };
        let selected_bg = with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.16 } else { 0.10 },
        );
        let hover_bg = theme.hover_overlay();
        let active_bg = theme.active_overlay();

        div()
            .id(id)
            .debug_selector(move || debug_id.to_string())
            .w_full()
            .min_h(px(SETTINGS_DROPDOWN_DENSE_DETAIL_ROW_HEIGHT_PX))
            .px_2()
            .py(px(2.0))
            .flex()
            .items_center()
            .gap_2()
            .rounded(px(theme.radii.row))
            .cursor(CursorStyle::PointingHand)
            .bg(if selected {
                selected_bg
            } else {
                gpui::rgba(0x00000000)
            })
            .hover(move |s| {
                if selected {
                    s.bg(selected_bg)
                } else {
                    s.bg(hover_bg)
                }
            })
            .active(move |s| {
                if selected {
                    s.bg(selected_bg)
                } else {
                    s.bg(active_bg)
                }
            })
            .child(
                div()
                    .w(px(16.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(selected, |d| {
                        d.child(svg_icon(
                            "icons/check.svg",
                            theme.colors.accent.foreground,
                            px(12.0),
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(text_color)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(label.into()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(detail.into()),
                    ),
            )
    }

    pub(super) fn render_ui_font_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| {
                this.ui_font_options
                    .get(ix)
                    .cloned()
                    .map(|family| (ix, family))
            })
            .map(|(ix, family)| {
                this.font_option_row_for_family(
                    "settings_window_ui_font",
                    ix,
                    family.as_str(),
                    this.ui_font_family == family,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_ui_font_family(family.clone(), cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_theme_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        let modes = settings_theme_mode_options();
        range
            .filter_map(|ix| modes.get(ix).cloned())
            .map(|(mode, label)| {
                this.option_row(
                    format!("settings_window_theme_{}", mode.key()),
                    label,
                    None,
                    this.theme_mode == mode,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, window, cx| {
                    this.set_theme_mode(mode.clone(), window, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_language_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| crate::i18n::Language::ALL.get(ix).copied())
            .map(|language| {
                this.option_row(
                    format!("settings_window_language_{}", language.key()),
                    this.language_option_label(language),
                    None,
                    this.language == language,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_language(language, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_avatar_source_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| crate::avatar_source::AvatarSource::ALL.get(ix).copied())
            .map(|source| {
                this.option_row(
                    format!("settings_window_avatar_source_{}", source.key()),
                    source.label(),
                    None,
                    this.avatar_source == source,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_avatar_source(source, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_ai_commit_source_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| crate::ai_commit_sources::AiSource::ALL.get(ix).copied())
            .map(|source| {
                this.option_row(
                    format!("settings_window_ai_commit_source_{}", source.key()),
                    source.label(),
                    None,
                    this.ai_commit_source == source,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_ai_commit_source(source, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_merge_tool_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| merge_tool_options().into_iter().nth(ix))
            .map(|option| {
                let (row_id, label, selected, next_selection) = match option {
                    MergeToolOption::FromGitConfig => (
                        "settings_window_merge_tool_option_from_git_config".to_string(),
                        tr_str("settings.merge_tool.from_git_config"),
                        matches!(
                            this.merge_tool_selection,
                            ExternalMergeToolSelection::FromGitConfig
                        ),
                        ExternalMergeToolSelection::FromGitConfig,
                    ),
                    MergeToolOption::Preset(preset) => {
                        // Re-clicking the selected preset must not wipe the
                        // manual executable path; a different preset starts
                        // fresh — its old path names the wrong executable.
                        let preserved_path = match &this.merge_tool_selection {
                            ExternalMergeToolSelection::Builtin { id, path } if id == preset.id => {
                                path.clone()
                            }
                            _ => None,
                        };
                        (
                            format!("settings_window_merge_tool_option_{}", preset.id),
                            tr_str(preset.label_key),
                            matches!(
                                &this.merge_tool_selection,
                                ExternalMergeToolSelection::Builtin { id, .. } if id == preset.id
                            ),
                            ExternalMergeToolSelection::Builtin {
                                id: preset.id.to_string(),
                                path: preserved_path,
                            },
                        )
                    }
                    MergeToolOption::Custom => (
                        "settings_window_merge_tool_option_custom".to_string(),
                        tr_str("settings.merge_tool.custom"),
                        matches!(
                            this.merge_tool_selection,
                            ExternalMergeToolSelection::Custom { .. }
                        ),
                        ExternalMergeToolSelection::Custom {
                            command: this.merge_tool_custom_command_draft.clone(),
                            trust_exit_code: false,
                        },
                    ),
                };
                this.option_row(row_id, label, None, selected, theme)
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.set_merge_tool_selection(next_selection.clone(), cx);
                    }))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_ai_commit_provider_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| crate::ai_commit::AiProvider::ALL.get(ix).copied())
            .map(|provider| {
                this.option_row(
                    format!("settings_window_ai_commit_provider_{}", provider.key()),
                    provider.label(),
                    None,
                    this.ai_commit_provider == provider,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_ai_commit_provider(provider, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_ai_commit_model_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        let AiCommitModels::Ready(models) = &this.ai_commit_models else {
            return Vec::new();
        };
        range
            .filter_map(|ix| models.get(ix).cloned())
            .map(|model| {
                let selected = this.ai_commit_model_draft == *model;
                this.option_row(
                    format!("settings_window_ai_commit_model_{}", model),
                    model.as_str(),
                    None,
                    selected,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_ai_commit_model(model.clone(), cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_editor_font_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| {
                this.editor_font_options
                    .get(ix)
                    .cloned()
                    .map(|family| (ix, family))
            })
            .map(|(ix, family)| {
                this.font_option_row_for_family(
                    "settings_window_editor_font",
                    ix,
                    family.as_str(),
                    this.editor_font_family == family,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_editor_font_family(family.clone(), cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_external_editor_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| this.external_editor_options.get(ix).cloned())
            .map(|option| {
                let selected = match &option.kind {
                    crate::external_editor::ExternalEditorOptionKind::None => {
                        this.external_editor_setting.is_none()
                    }
                    crate::external_editor::ExternalEditorOptionKind::Detected(setting) => {
                        this.external_editor_setting.as_ref() == Some(setting)
                    }
                    crate::external_editor::ExternalEditorOptionKind::Custom => {
                        this.external_editor_is_custom()
                    }
                };
                let row = this.option_row(
                    option.id.clone(),
                    option.label.clone(),
                    option.detail.clone().map(Into::into),
                    selected,
                    theme,
                );
                match option.kind {
                    crate::external_editor::ExternalEditorOptionKind::None => row
                        .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                            this.set_external_editor_setting(None, cx);
                        }))
                        .into_any_element(),
                    crate::external_editor::ExternalEditorOptionKind::Detected(setting) => row
                        .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                            this.set_external_editor_setting(Some(setting.clone()), cx);
                        }))
                        .into_any_element(),
                    crate::external_editor::ExternalEditorOptionKind::Custom => row
                        .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                            this.select_custom_external_editor(cx);
                        }))
                        .into_any_element(),
                }
            })
            .collect()
    }

    pub(super) fn render_date_format_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| {
                DateTimeFormat::all()
                    .get(ix)
                    .copied()
                    .map(|format| (ix, format))
            })
            .map(|(_ix, format)| {
                this.option_row(
                    match format {
                        DateTimeFormat::YmdHm => "settings_window_date_format_ymd_hm",
                        DateTimeFormat::YmdHms => "settings_window_date_format_ymd_hms",
                        DateTimeFormat::DmyHm => "settings_window_date_format_dmy_hm",
                        DateTimeFormat::MdyHm => "settings_window_date_format_mdy_hm",
                    },
                    format.label(),
                    None,
                    this.date_time_format == format,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_date_time_format(format, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_timezone_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| {
                Timezone::all()
                    .get(ix)
                    .copied()
                    .map(|timezone| (ix, timezone))
            })
            .map(|(_ix, timezone)| {
                this.dense_detail_option_row(
                    format!("settings_window_timezone_{}", timezone.key()),
                    timezone.label(),
                    timezone.cities(),
                    this.timezone == timezone,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_timezone(timezone, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_change_tracking_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| CHANGE_TRACKING_OPTIONS.get(ix).copied())
            .map(|(id, option, detail)| {
                this.option_row(
                    id,
                    option.label(),
                    Some(tr(detail)),
                    this.change_tracking_view == option,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_change_tracking_view(option, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_diff_scroll_sync_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| DIFF_SCROLL_SYNC_OPTIONS.get(ix).copied())
            .map(|(id, option, detail)| {
                this.option_row(
                    id,
                    option.label(),
                    Some(tr(detail)),
                    this.diff_scroll_sync == option,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_diff_scroll_sync(option, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_diff_view_mode_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| DIFF_VIEW_MODE_OPTIONS.get(ix).copied())
            .map(|(id, option, detail)| {
                this.option_row(
                    id,
                    option.settings_label(),
                    Some(tr(detail)),
                    this.diff_view_mode == option,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_diff_view_mode(option, cx);
                }))
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_diff_content_mode_option_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        range
            .filter_map(|ix| DIFF_CONTENT_MODE_OPTIONS.get(ix).copied())
            .map(|(id, option, detail)| {
                this.option_row(
                    id,
                    option.label(),
                    Some(tr(detail)),
                    this.diff_content_mode == option,
                    theme,
                )
                .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                    this.set_diff_content_mode(option, cx);
                }))
                .into_any_element()
            })
            .collect()
    }
}
