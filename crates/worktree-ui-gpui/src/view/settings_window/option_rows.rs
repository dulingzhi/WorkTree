//! Option rows: the `option_row` family of row builders and the per-setting
//! `render_*_option_rows` dropdown row sources.

use super::*;
use gpui::Stateful;

// ---------------------------------------------------------------------------
// Option-row convergence (P3/T13-13b): the macros below generate the
// isomorphic `render_*_option_rows` uniform-list row sources.
// Expansion-equivalence evidence: every generated function is token-equal
// (modulo rustfmt) to the handwritten body it replaced; the per-function
// proof table lives in the task evidence (D:\tmp\p3t13-13b2-*).
//
// Kept handwritten (divergence points):
//   render_theme_option_rows        - (mode, label) tuple source; the click
//                                     forwards the real `window` (theme
//                                     switch re-resolves per window)
//   render_merge_tool_option_rows   - three-way option-kind match building
//                                     (id, label, selected, next_selection)
//   render_ai_commit_model_option_rows - let-else early return on the model
//                                     fetch state; models are plain strings
//   render_external_editor_option_rows - per-kind selected logic and three
//                                     different click handlers
//   render_date_format_option_rows  - row id comes from a match on the format
//   render_timezone_option_rows     - dense detail row with a cities list
// ---------------------------------------------------------------------------

/// Row source over an `Enum::ALL` table: one `option_row` per variant.
/// The default arm labels via `$var.label()`; the `this_label` arm labels via
/// `this.$label_fn($var)` (the language row translates its label).
macro_rules! all_options_rows {
    ($name:ident, $all:path, $var:ident, $id:literal, $field:ident, $setter:ident) => {
        pub(super) fn $name(
            this: &mut Self,
            range: Range<usize>,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> Vec<AnyElement> {
            let theme = this.theme;
            range
                .filter_map(|ix| $all.get(ix).copied())
                .map(|$var| {
                    this.option_row(
                        format!($id, $var.key()),
                        $var.label(),
                        None,
                        this.$field == $var,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.$setter($var, cx);
                    }))
                    .into_any_element()
                })
                .collect()
        }
    };
    ($name:ident, $all:path, $var:ident, $id:literal, $field:ident, $setter:ident, this_label: $label_fn:ident) => {
        pub(super) fn $name(
            this: &mut Self,
            range: Range<usize>,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> Vec<AnyElement> {
            let theme = this.theme;
            range
                .filter_map(|ix| $all.get(ix).copied())
                .map(|$var| {
                    this.option_row(
                        format!($id, $var.key()),
                        this.$label_fn($var),
                        None,
                        this.$field == $var,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.$setter($var, cx);
                    }))
                    .into_any_element()
                })
                .collect()
        }
    };
}

/// Row source over a `(id, option, detail_key)` constant table. The default
/// arm labels via `option.label()`; the `settings_label` arm uses the
/// settings-specific label (diff view mode).
macro_rules! table_options_rows {
    ($name:ident, $table:ident, $field:ident, $setter:ident) => {
        pub(super) fn $name(
            this: &mut Self,
            range: Range<usize>,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> Vec<AnyElement> {
            let theme = this.theme;
            range
                .filter_map(|ix| $table.get(ix).copied())
                .map(|(id, option, detail)| {
                    this.option_row(
                        id,
                        option.label(),
                        Some(tr(detail)),
                        this.$field == option,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.$setter(option, cx);
                    }))
                    .into_any_element()
                })
                .collect()
        }
    };
    ($name:ident, $table:ident, $field:ident, $setter:ident, settings_label) => {
        pub(super) fn $name(
            this: &mut Self,
            range: Range<usize>,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> Vec<AnyElement> {
            let theme = this.theme;
            range
                .filter_map(|ix| $table.get(ix).copied())
                .map(|(id, option, detail)| {
                    this.option_row(
                        id,
                        option.settings_label(),
                        Some(tr(detail)),
                        this.$field == option,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.$setter(option, cx);
                    }))
                    .into_any_element()
                })
                .collect()
        }
    };
}

/// Row source over a cached font-family list: one `font_option_row_for_family`
/// per entry, indexed by position.
macro_rules! font_options_rows {
    ($name:ident, $options:ident, $id:literal, $field:ident, $setter:ident) => {
        pub(super) fn $name(
            this: &mut Self,
            range: Range<usize>,
            _window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> Vec<AnyElement> {
            let theme = this.theme;
            range
                .filter_map(|ix| this.$options.get(ix).cloned().map(|family| (ix, family)))
                .map(|(ix, family)| {
                    this.font_option_row_for_family(
                        $id,
                        ix,
                        family.as_str(),
                        this.$field == family,
                        theme,
                    )
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                        this.$setter(family.clone(), cx);
                    }))
                    .into_any_element()
                })
                .collect()
        }
    };
}

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

    font_options_rows!(
        render_ui_font_option_rows,
        ui_font_options,
        "settings_window_ui_font",
        ui_font_family,
        set_ui_font_family
    );

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

    all_options_rows!(render_language_option_rows, crate::i18n::Language::ALL, language, "settings_window_language_{}", language, set_language, this_label: language_option_label);

    all_options_rows!(
        render_avatar_source_option_rows,
        crate::avatar_source::AvatarSource::ALL,
        source,
        "settings_window_avatar_source_{}",
        avatar_source,
        set_avatar_source
    );

    all_options_rows!(
        render_ai_commit_source_option_rows,
        crate::ai_commit_sources::AiSource::ALL,
        source,
        "settings_window_ai_commit_source_{}",
        ai_commit_source,
        set_ai_commit_source
    );

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

    all_options_rows!(
        render_ai_commit_provider_option_rows,
        crate::ai_commit::AiProvider::ALL,
        provider,
        "settings_window_ai_commit_provider_{}",
        ai_commit_provider,
        set_ai_commit_provider
    );

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

    font_options_rows!(
        render_editor_font_option_rows,
        editor_font_options,
        "settings_window_editor_font",
        editor_font_family,
        set_editor_font_family
    );

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

    table_options_rows!(
        render_change_tracking_option_rows,
        CHANGE_TRACKING_OPTIONS,
        change_tracking_view,
        set_change_tracking_view
    );

    table_options_rows!(
        render_diff_scroll_sync_option_rows,
        DIFF_SCROLL_SYNC_OPTIONS,
        diff_scroll_sync,
        set_diff_scroll_sync
    );

    table_options_rows!(
        render_diff_view_mode_option_rows,
        DIFF_VIEW_MODE_OPTIONS,
        diff_view_mode,
        set_diff_view_mode,
        settings_label
    );

    table_options_rows!(
        render_diff_content_mode_option_rows,
        DIFF_CONTENT_MODE_OPTIONS,
        diff_content_mode,
        set_diff_content_mode
    );
}
