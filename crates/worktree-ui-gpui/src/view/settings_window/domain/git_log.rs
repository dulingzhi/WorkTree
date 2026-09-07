//! Git log settings: default mode, visible columns and date/chain display
//! preferences.

use super::*;

pub(in crate::view::settings_window) fn history_columns_settings_label(
    show_graph: bool,
    show_author: bool,
    show_date: bool,
    show_sha: bool,
) -> SharedString {
    let mut columns = Vec::new();
    if show_graph {
        columns.push(tr_str("settings.git_log.column_graph"));
    }
    if show_author {
        columns.push(tr_str("settings.git_log.column_author"));
    }
    if show_date {
        columns.push(tr_str("settings.git_log.column_date"));
    }
    if show_sha {
        columns.push(tr_str("settings.git_log.column_sha"));
    }

    if columns.is_empty() {
        tr_str("settings.git_log.columns_none").into()
    } else {
        columns.join(", ").into()
    }
}

impl SettingsWindowView {
    pub(in crate::view::settings_window) fn set_history_column_preferences(
        &mut self,
        show_graph: bool,
        show_author: bool,
        show_date: bool,
        show_sha: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_show_graph == show_graph
            && self.history_show_author == show_author
            && self.history_show_date == show_date
            && self.history_show_sha == show_sha
        {
            return;
        }

        self.history_show_graph = show_graph;
        self.history_show_author = show_author;
        self.history_show_date = show_date;
        self.history_show_sha = show_sha;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_history_column_preferences(show_graph, show_author, show_date, show_sha, cx);
        });
        cx.notify();
    }

    settings_setter!(
        set_history_highlight_commit_chain,
        history_highlight_commit_chain,
        bool,
        enabled,
        _window,
        set_history_highlight_commit_chain
    );

    settings_setter!(
        set_history_relative_dates,
        history_relative_dates,
        bool,
        enabled,
        _window,
        set_history_relative_dates
    );

    pub(in crate::view::settings_window) fn set_default_history_mode(
        &mut self,
        mode: HistoryMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.default_history_mode == mode {
            return;
        }

        self.default_history_mode = mode;
        self.expanded_section = None;
        self.persist_preferences(cx);
        cx.notify();
    }

    pub(in crate::view::settings_window) fn git_log_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let history_default_mode_row = self
            .summary_row(
                "settings_window_git_log_default_mode",
                tr_str("settings.row.default_history_mode"),
                crate::view::history_mode::history_mode_label(self.default_history_mode).into(),
                self.expanded_section == Some(SettingsSection::GitLogDefaultMode),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::GitLogDefaultMode, cx);
            }));

        let history_columns_row = self
            .summary_row(
                "settings_window_git_log_columns",
                tr_str("settings.row.history_columns"),
                history_columns_settings_label(
                    self.history_show_graph,
                    self.history_show_author,
                    self.history_show_date,
                    self.history_show_sha,
                ),
                self.expanded_section == Some(SettingsSection::GitLogColumns),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::GitLogColumns, cx);
            }));

        // "Lane", not "chain": what this dims is every lane but the
        // selected commit's own. A merge's second parent sits on a
        // lane of its own and washes out with the rest, so the old
        // label promised an ancestry walk the graph no longer does.
        let highlight_commit_chain_row = self
            .toggle_row(
                "settings_window_git_log_highlight_commit_chain",
                tr_str("settings.row.highlight_commit_lane"),
                self.history_highlight_commit_chain,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_history_highlight_commit_chain(!this.history_highlight_commit_chain, cx);
            }));

        let relative_dates_row = self
            .toggle_row(
                "settings_window_git_log_relative_dates",
                tr_str("settings.row.relative_dates"),
                self.history_relative_dates,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_history_relative_dates(!this.history_relative_dates, cx);
            }));

        let show_history_tags_row = self
            .toggle_row(
                "settings_window_git_log_show_tags",
                tr_str("settings.row.show_tags"),
                self.history_show_tags,
                theme,
            )
            .border_color(if self.history_show_tags {
                settings_row_separator_color(theme)
            } else {
                no_separator
            })
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_history_show_tags(!this.history_show_tags, cx);
            }));

        let auto_fetch_tags_row = self
            .summary_row(
                "settings_window_git_log_tag_fetch_mode",
                tr_str("settings.row.auto_fetch_tags"),
                git_log_tag_fetch_mode_label(self.history_tag_fetch_mode).into(),
                self.expanded_section == Some(SettingsSection::GitLogTagFetch),
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                if this.history_show_tags {
                    this.toggle_section(SettingsSection::GitLogTagFetch, cx);
                }
            }));
        let mut git_log_card = self
            .card(
                "settings_window_git_log_card",
                tr_str("settings.nav.git_log"),
                theme,
            )
            .child(history_default_mode_row);

        if self.expanded_section == Some(SettingsSection::GitLogDefaultMode) {
            let mut mode_container =
                self.detail_container("settings_window_git_log_default_mode_container", theme);
            for spec in crate::view::history_mode::history_mode_ui_specs() {
                let mode = spec.mode;
                mode_container = mode_container.child(
                    self.option_row(
                        spec.settings_row_id,
                        // The spec table carries the English source;
                        // the settings row localizes it gettext-style.
                        crate::i18n::tr_en(spec.label),
                        Some(crate::i18n::tr_en(spec.settings_description)),
                        self.default_history_mode == mode,
                        theme,
                    )
                    .on_click(cx.listener(
                        move |this, _e: &ClickEvent, _window, cx| {
                            this.set_default_history_mode(mode, cx);
                        },
                    )),
                );
            }
            git_log_card = git_log_card.child(
                mode_container.child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.git_log.default_mode_note")),
                ),
            );
        }

        git_log_card = git_log_card.child(history_columns_row);

        if self.expanded_section == Some(SettingsSection::GitLogColumns) {
            git_log_card = git_log_card.child(
                self.detail_container("settings_window_git_log_columns_container", theme)
                    .child(
                        self.toggle_row(
                            "settings_window_git_log_column_graph",
                            tr_str("settings.git_log.column_graph"),
                            self.history_show_graph,
                            theme,
                        )
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.set_history_column_preferences(
                                    !this.history_show_graph,
                                    this.history_show_author,
                                    this.history_show_date,
                                    this.history_show_sha,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        self.toggle_row(
                            "settings_window_git_log_column_author",
                            tr_str("settings.git_log.column_author"),
                            self.history_show_author,
                            theme,
                        )
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.set_history_column_preferences(
                                    this.history_show_graph,
                                    !this.history_show_author,
                                    this.history_show_date,
                                    this.history_show_sha,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        self.toggle_row(
                            "settings_window_git_log_column_date",
                            tr_str("settings.git_log.column_date"),
                            self.history_show_date,
                            theme,
                        )
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.set_history_column_preferences(
                                    this.history_show_graph,
                                    this.history_show_author,
                                    !this.history_show_date,
                                    this.history_show_sha,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        self.toggle_row(
                            "settings_window_git_log_column_sha",
                            tr_str("settings.git_log.column_sha"),
                            self.history_show_sha,
                            theme,
                        )
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.set_history_column_preferences(
                                    this.history_show_graph,
                                    this.history_show_author,
                                    this.history_show_date,
                                    !this.history_show_sha,
                                    cx,
                                );
                            },
                        )),
                    )
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.git_log.columns_note")),
                    )
                    .child(
                        self.link_row(
                            "settings_window_git_log_reset_widths",
                            tr_str("settings.git_log.reset_column_widths"),
                            tr("settings.action.reset"),
                            theme,
                        )
                        .border_color(no_separator)
                        .on_click(cx.listener(
                            |this, _e: &ClickEvent, _window, cx| {
                                this.update_main_windows(cx, |view, _window, cx| {
                                    view.reset_history_column_widths(cx);
                                });
                                cx.notify();
                            },
                        )),
                    ),
            );
        }

        git_log_card = git_log_card.child(highlight_commit_chain_row);
        git_log_card = git_log_card.child(relative_dates_row);
        git_log_card = git_log_card.child(show_history_tags_row);
        if self.history_show_tags {
            git_log_card = git_log_card.child(auto_fetch_tags_row);

            if self.expanded_section == Some(SettingsSection::GitLogTagFetch) {
                git_log_card = git_log_card.child(
                    self.detail_container("settings_window_git_log_tag_fetch_container", theme)
                        .child(
                            self.option_row(
                                "settings_window_git_log_tag_fetch_mode_activation",
                                tr_str("settings.tags.fetch_on_activation"),
                                Some(tr("settings.tags.fetch_on_activation_detail")),
                                self.history_tag_fetch_mode
                                    == GitLogTagFetchMode::OnRepositoryActivation,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_history_tag_fetch_mode(
                                        GitLogTagFetchMode::OnRepositoryActivation,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            self.option_row(
                                "settings_window_git_log_tag_fetch_mode_disabled",
                                tr_str("settings.tags.fetch_disabled"),
                                Some(tr("settings.tags.fetch_disabled_detail")),
                                self.history_tag_fetch_mode == GitLogTagFetchMode::Disabled,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _e: &ClickEvent, _window, cx| {
                                    this.set_history_tag_fetch_mode(
                                        GitLogTagFetchMode::Disabled,
                                        cx,
                                    );
                                },
                            )),
                        ),
                );
            }
        }
        git_log_card
    }
}
