//! Diff settings: change-tracking and diff view/content/scroll-sync
//! preferences with their dropdown option tables.

use super::*;

// The third tuple element is a translation key for the row's detail text.
pub(in crate::view::settings_window) const CHANGE_TRACKING_OPTIONS: &[(
    &str,
    ChangeTrackingView,
    &str,
)] = &[
    (
        "settings_window_change_tracking_combined",
        ChangeTrackingView::Combined,
        "settings.change_tracking.combined_detail",
    ),
    (
        "settings_window_change_tracking_split_untracked",
        ChangeTrackingView::SplitUntracked,
        "settings.change_tracking.split_untracked_detail",
    ),
];

pub(in crate::view::settings_window) const DIFF_SCROLL_SYNC_OPTIONS: &[(
    &str,
    DiffScrollSync,
    &str,
)] = &[
    (
        "settings_window_diff_scroll_sync_vertical",
        DiffScrollSync::Vertical,
        "settings.diff.scroll_sync_vertical_detail",
    ),
    (
        "settings_window_diff_scroll_sync_horizontal",
        DiffScrollSync::Horizontal,
        "settings.diff.scroll_sync_horizontal_detail",
    ),
    (
        "settings_window_diff_scroll_sync_none",
        DiffScrollSync::None,
        "settings.diff.scroll_sync_none_detail",
    ),
    (
        "settings_window_diff_scroll_sync_both",
        DiffScrollSync::Both,
        "settings.diff.scroll_sync_both_detail",
    ),
];

pub(in crate::view::settings_window) const DIFF_CONTENT_MODE_OPTIONS: &[(
    &str,
    DiffContentMode,
    &str,
)] = &[
    (
        "settings_window_diff_content_mode_collapsed",
        DiffContentMode::Collapsed,
        "settings.diff.content_collapsed_detail",
    ),
    (
        "settings_window_diff_content_mode_full",
        DiffContentMode::Full,
        "settings.diff.content_full_detail",
    ),
];

pub(in crate::view::settings_window) const DIFF_VIEW_MODE_OPTIONS: &[(&str, DiffViewMode, &str)] =
    &[
        (
            "settings_window_diff_view_mode_inline",
            DiffViewMode::Inline,
            "settings.diff.view_inline_detail",
        ),
        (
            "settings_window_diff_view_mode_split",
            DiffViewMode::Split,
            "settings.diff.view_split_detail",
        ),
    ];

impl SettingsWindowView {
    settings_setter!(
        set_change_tracking_view,
        change_tracking_view,
        ChangeTrackingView,
        next,
        _window,
        set_change_tracking_view,
        reset_section
    );

    settings_setter!(
        set_diff_scroll_sync,
        diff_scroll_sync,
        DiffScrollSync,
        next,
        _window,
        set_diff_scroll_sync,
        reset_section
    );

    settings_setter!(
        set_diff_content_mode,
        diff_content_mode,
        DiffContentMode,
        next,
        _window,
        set_diff_content_mode,
        reset_section
    );

    settings_setter!(
        set_diff_whitespace_mode,
        diff_whitespace_mode,
        DiffWhitespaceMode,
        next,
        _window,
        set_diff_whitespace_mode
    );

    settings_setter!(
        set_diff_view_mode,
        diff_view_mode,
        DiffViewMode,
        next,
        _window,
        set_diff_view_mode,
        reset_section
    );

    settings_setter!(
        set_diff_reveal_whitespace_chars,
        diff_reveal_whitespace_chars,
        bool,
        next,
        _window,
        set_diff_reveal_whitespace_chars
    );

    settings_setter!(
        set_diff_word_wrap,
        diff_word_wrap,
        bool,
        next,
        _window,
        set_diff_word_wrap
    );

    settings_setter!(
        set_diff_show_line_numbers,
        diff_show_line_numbers,
        bool,
        next,
        _window,
        set_diff_show_line_numbers
    );

    pub(in crate::view::settings_window) fn change_tracking_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let change_tracking_row = self
            .summary_row(
                "settings_window_change_tracking",
                tr_str("settings.row.untracked_files"),
                self.change_tracking_view.settings_label().into(),
                self.expanded_section == Some(SettingsSection::ChangeTracking),
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::ChangeTracking, cx);
            }));
        let mut change_tracking_card = self
            .card(
                "settings_window_change_tracking_card",
                tr_str("settings.nav.change_tracking"),
                theme,
            )
            .child(change_tracking_row);

        if self.expanded_section == Some(SettingsSection::ChangeTracking) {
            let list = uniform_list(
                "settings_window_change_tracking_list",
                CHANGE_TRACKING_OPTIONS.len(),
                cx.processor(Self::render_change_tracking_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.change_tracking_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.change_tracking_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            change_tracking_card = change_tracking_card.child(self.dropdown_list_container(
                "settings_window_change_tracking_list_container",
                "settings_window_change_tracking_scrollbar",
                self.change_tracking_scroll.clone(),
                CHANGE_TRACKING_OPTIONS.len(),
                SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }
        change_tracking_card
    }

    pub(in crate::view::settings_window) fn diff_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let diff_scroll_sync_row = self
            .summary_row(
                "settings_window_diff_scroll_sync",
                tr_str("settings.row.scroll_sync"),
                self.diff_scroll_sync.label().into(),
                self.expanded_section == Some(SettingsSection::Diff),
                theme,
            )
            .border_color(no_separator)
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::Diff, cx);
            }));

        let diff_content_mode_row = self
            .summary_row(
                "settings_window_diff_content_mode",
                tr_str("settings.row.diff_mode"),
                self.diff_content_mode.settings_label().into(),
                self.expanded_section == Some(SettingsSection::DiffContentMode),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::DiffContentMode, cx);
            }));

        let diff_whitespace_mode_row = self
            .toggle_row(
                "settings_window_diff_whitespace_mode",
                tr_str("settings.row.show_whitespace_changes"),
                self.diff_whitespace_mode == DiffWhitespaceMode::Show,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_diff_whitespace_mode(this.diff_whitespace_mode.toggled(), cx);
            }));

        let diff_reveal_whitespace_chars_row = self
            .toggle_row(
                "settings_window_diff_reveal_whitespace_chars",
                tr_str("settings.row.reveal_whitespace_characters"),
                self.diff_reveal_whitespace_chars,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_diff_reveal_whitespace_chars(!this.diff_reveal_whitespace_chars, cx);
            }));

        let diff_word_wrap_row = self
            .toggle_row(
                "settings_window_diff_word_wrap",
                tr_str("settings.row.word_wrap"),
                self.diff_word_wrap,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_diff_word_wrap(!this.diff_word_wrap, cx);
            }));

        let diff_show_line_numbers_row = self
            .toggle_row(
                "settings_window_diff_show_line_numbers",
                tr_str("settings.row.show_line_numbers"),
                self.diff_show_line_numbers,
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.set_diff_show_line_numbers(!this.diff_show_line_numbers, cx);
            }));
        let mut diff_card = self
            .card(
                "settings_window_diff_card",
                tr_str("settings.nav.diff"),
                theme,
            )
            .child(diff_content_mode_row);

        if self.expanded_section == Some(SettingsSection::DiffContentMode) {
            let list = uniform_list(
                "settings_window_diff_content_mode_list",
                DIFF_CONTENT_MODE_OPTIONS.len(),
                cx.processor(Self::render_diff_content_mode_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.diff_content_mode_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.diff_content_mode_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            diff_card = diff_card.child(self.dropdown_list_container(
                "settings_window_diff_content_mode_list_container",
                "settings_window_diff_content_mode_scrollbar",
                self.diff_content_mode_scroll.clone(),
                DIFF_CONTENT_MODE_OPTIONS.len(),
                SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        let diff_view_mode_row = self
            .summary_row(
                "settings_window_diff_view_mode",
                tr_str("settings.row.view_mode"),
                self.diff_view_mode.settings_label().into(),
                self.expanded_section == Some(SettingsSection::DiffViewMode),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::DiffViewMode, cx);
            }));

        diff_card = diff_card.child(diff_view_mode_row);

        if self.expanded_section == Some(SettingsSection::DiffViewMode) {
            let list = uniform_list(
                "settings_window_diff_view_mode_list",
                DIFF_VIEW_MODE_OPTIONS.len(),
                cx.processor(Self::render_diff_view_mode_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.diff_view_mode_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.diff_view_mode_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            diff_card = diff_card.child(self.dropdown_list_container(
                "settings_window_diff_view_mode_list_container",
                "settings_window_diff_view_mode_scrollbar",
                self.diff_view_mode_scroll.clone(),
                DIFF_VIEW_MODE_OPTIONS.len(),
                SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }

        diff_card = diff_card
            .child(diff_whitespace_mode_row)
            .child(diff_reveal_whitespace_chars_row)
            .child(diff_word_wrap_row)
            .child(diff_show_line_numbers_row);

        diff_card = diff_card.child(diff_scroll_sync_row);

        if self.expanded_section == Some(SettingsSection::Diff) {
            let list = uniform_list(
                "settings_window_diff_scroll_sync_list",
                DIFF_SCROLL_SYNC_OPTIONS.len(),
                cx.processor(Self::render_diff_scroll_sync_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.diff_scroll_sync_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(
                self.diff_scroll_sync_scroll.clone(),
            ));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            diff_card = diff_card.child(self.dropdown_list_container(
                "settings_window_diff_scroll_sync_list_container",
                "settings_window_diff_scroll_sync_scrollbar",
                self.diff_scroll_sync_scroll.clone(),
                DIFF_SCROLL_SYNC_OPTIONS.len(),
                SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX + 18.0,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));
        }
        diff_card
    }
}
