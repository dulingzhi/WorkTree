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

    pub(in crate::view::settings_window) fn tags_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.card(
            "settings_window_tags_card",
            tr_str("settings.nav.tags"),
            theme,
        )
        .child(
            self.setting_option_row(
                "settings_window_tags_default_lightweight",
                tr_str("settings.tags.lightweight"),
                Some(tr("settings.tags.lightweight_detail")),
                self.default_tag_type == DefaultTagType::Lightweight,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_default_tag_type(DefaultTagType::Lightweight, cx);
            })),
        )
        .child(
            self.setting_option_row(
                "settings_window_tags_default_annotated",
                tr_str("settings.tags.annotated"),
                Some(tr("settings.tags.annotated_detail")),
                self.default_tag_type == DefaultTagType::Annotated,
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_default_tag_type(DefaultTagType::Annotated, cx);
            })),
        )
    }
}
