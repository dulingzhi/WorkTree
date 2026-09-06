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
    pub(in crate::view::settings_window) fn set_change_tracking_view(
        &mut self,
        next: ChangeTrackingView,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.change_tracking_view == next {
            return;
        }

        self.change_tracking_view = next;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_change_tracking_view(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_scroll_sync(
        &mut self,
        next: DiffScrollSync,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_scroll_sync == next {
            return;
        }

        self.diff_scroll_sync = next;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_scroll_sync(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_content_mode(
        &mut self,
        next: DiffContentMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_content_mode == next {
            return;
        }

        self.diff_content_mode = next;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_content_mode(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_whitespace_mode(
        &mut self,
        next: DiffWhitespaceMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_whitespace_mode == next {
            return;
        }

        self.diff_whitespace_mode = next;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_whitespace_mode(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_view_mode(
        &mut self,
        next: DiffViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_view_mode == next {
            return;
        }

        self.diff_view_mode = next;
        self.expanded_section = None;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_view_mode(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_reveal_whitespace_chars(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_reveal_whitespace_chars == next {
            return;
        }

        self.diff_reveal_whitespace_chars = next;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_reveal_whitespace_chars(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_word_wrap(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap == next {
            return;
        }

        self.diff_word_wrap = next;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_word_wrap(next, cx);
        });
        cx.notify();
    }

    pub(in crate::view::settings_window) fn set_diff_show_line_numbers(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_show_line_numbers == next {
            return;
        }

        self.diff_show_line_numbers = next;
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, cx| {
            view.set_diff_show_line_numbers(next, cx);
        });
        cx.notify();
    }
}
