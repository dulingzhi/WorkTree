//! Tag settings: history tag display, fetch mode and the default tag type.

use super::*;

pub(in crate::view::settings_window) fn git_log_tag_fetch_mode_label(
    mode: GitLogTagFetchMode,
) -> &'static str {
    match mode {
        GitLogTagFetchMode::OnRepositoryActivation => tr_str("settings.tags.fetch_on_activation"),
        GitLogTagFetchMode::Disabled => tr_str("settings.tags.fetch_disabled"),
    }
}

impl SettingsWindowView {
    pub(in crate::view::settings_window) fn set_history_show_tags(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_show_tags == enabled {
            return;
        }

        self.history_show_tags = enabled;
        if !enabled && self.expanded_section == Some(SettingsSection::GitLogTagFetch) {
            self.expanded_section = None;
        }
        let tag_fetch_mode = self.history_tag_fetch_mode;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_history_tag_preferences(enabled, tag_fetch_mode, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_history_tag_fetch_mode(
        &mut self,
        mode: GitLogTagFetchMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_tag_fetch_mode == mode {
            return;
        }

        self.history_tag_fetch_mode = mode;
        self.expanded_section = None;
        let show_tags = self.history_show_tags;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_history_tag_preferences(show_tags, mode, cx);
        });
        cx.notify();
    }

    settings_setter!(
        set_default_tag_type,
        default_tag_type,
        DefaultTagType,
        tag_type,
        _window,
        set_default_tag_type_preference
    );
}
