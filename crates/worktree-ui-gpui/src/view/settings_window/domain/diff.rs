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
}
