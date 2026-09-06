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
}
