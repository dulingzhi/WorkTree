use super::*;

#[test]
fn settings_window_titlebar_options_match_platform_chrome_strategy() {
    let options = settings_window_titlebar_options();
    assert_eq!(
        options.appears_transparent,
        cfg!(any(target_os = "macos", target_os = "windows")),
        "settings window titlebar transparency should match the platform chrome strategy"
    );
    assert_eq!(
        options.title.as_ref().map(ToString::to_string),
        Some(SETTINGS_WINDOW_TITLE.to_string()),
        "settings window titlebar should keep the OS-visible title"
    );
}

#[test]
fn settings_window_frame_strategy_matches_platform_chrome() {
    #[cfg(target_os = "windows")]
    {
        assert_eq!(settings_window_client_inset(), px(0.0));
    }

    #[cfg(not(target_os = "windows"))]
    {
        assert_eq!(
            settings_window_client_inset(),
            chrome::CLIENT_SIDE_DECORATION_INSET
        );
    }
}

#[test]
fn settings_window_options_request_client_chrome_and_resize_behavior() {
    let bounds = Bounds::new(
        point(px(12.0), px(24.0)),
        size(
            px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX),
            px(SETTINGS_WINDOW_DEFAULT_HEIGHT_PX),
        ),
    );
    let options = settings_window_options(bounds);

    assert_eq!(
        options.window_bounds,
        Some(WindowBounds::Windowed(bounds)),
        "settings window should open at the requested bounds"
    );
    assert_eq!(
        options.window_min_size,
        Some(size(
            px(SETTINGS_WINDOW_MIN_WIDTH_PX),
            px(SETTINGS_WINDOW_MIN_HEIGHT_PX),
        )),
        "settings window should enforce its minimum size"
    );
    assert_eq!(
        options.window_decorations,
        Some(WindowDecorations::Client),
        "settings window should request client-side decorations"
    );
    assert!(
        options.is_movable,
        "settings window should remain movable with custom chrome"
    );
    assert!(
        options.is_resizable,
        "settings window should remain resizable with custom chrome"
    );
}

#[test]
fn settings_dropdown_background_is_darker_than_card_surface() {
    fn brightness(color: gpui::Rgba) -> f32 {
        color.red + color.green + color.blue
    }

    let dark = AppTheme::worktree_dark();
    assert!(
        brightness(settings_dropdown_background(dark)) < brightness(dark.colors.surface.raised),
        "dark dropdown surface should be darker than the card surface"
    );

    let light = AppTheme::worktree_light();
    assert!(
        brightness(settings_dropdown_background(light)) < brightness(light.colors.surface.raised),
        "light dropdown surface should still read darker than the card surface"
    );
}

#[test]
fn settings_theme_modes_include_automatic_and_all_available_named_themes() {
    let modes = settings_theme_modes();
    assert_eq!(modes.first(), Some(&ThemeMode::Automatic));

    let named_modes = modes.iter().skip(1).map(ThemeMode::key).collect::<Vec<_>>();
    let available_themes = crate::theme::available_themes()
        .into_iter()
        .map(|theme| theme.key.to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        named_modes,
        available_themes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
}

#[gpui::test]
fn settings_window_sets_platform_title(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();

    assert_eq!(
        settings_cx.window_title().as_deref(),
        Some(SETTINGS_WINDOW_TITLE),
        "expected settings window to expose the native OS title"
    );
}

#[gpui::test]
fn settings_dropdowns_fit_without_inner_scroll(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX), px(1200.0)));
    settings_cx.run_until_parked();

    for (section, label) in [
        (SettingsSection::Theme, "Theme"),
        (SettingsSection::DateFormat, "Date time format"),
        (SettingsSection::ChangeTracking, "Untracked files"),
        (SettingsSection::Diff, "Diff scroll sync"),
    ] {
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            settings.expanded_section = Some(section);
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });

        let max_offset = settings_window
            .update(&mut settings_cx, |settings, _window, _cx| match section {
                SettingsSection::Theme => {
                    uniform_list_vertical_scroll_metrics(&settings.theme_scroll).2
                }
                SettingsSection::DateFormat => {
                    uniform_list_vertical_scroll_metrics(&settings.date_format_scroll).2
                }
                SettingsSection::ChangeTracking => {
                    uniform_list_vertical_scroll_metrics(&settings.change_tracking_scroll).2
                }
                SettingsSection::Diff => {
                    uniform_list_vertical_scroll_metrics(&settings.diff_scroll_sync_scroll).2
                }
                _ => px(0.0),
            })
            .expect("settings window should remain readable");

        assert_eq!(
            max_offset,
            px(0.0),
            "expected the {label} dropdown to fit without inner scroll"
        );
    }
}

#[gpui::test]
fn settings_window_root_view_renders_visible_scrollbar(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let synthetic_fonts: Arc<[String]> = (0..200)
        .map(|ix| format!("Test UI Font {ix:03}"))
        .collect::<Vec<_>>()
        .into();

    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.ui_font_options = synthetic_fonts.clone();
            settings.ui_font_family = synthetic_fonts[0].clone();
            settings.expanded_section = Some(SettingsSection::UiFont);
            settings.settings_window_scroll = ScrollHandle::default();
            settings.ui_font_scroll = UniformListScrollHandle::default();
            cx.notify();
        });
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(
        px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX),
        px(SETTINGS_WINDOW_MIN_HEIGHT_PX),
    ));
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let max_offset = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            settings.settings_window_scroll.max_offset().y.max(px(0.0))
        })
        .expect("settings window should remain readable");
    assert!(
        max_offset > px(0.0),
        "expected the root settings page to be scrollable during the test"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_scrollbar")
            .is_some(),
        "expected a visible scrollbar in the root settings view"
    );
}

#[gpui::test]
fn settings_window_rows_clamp_under_lilex_at_minimum_width(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.ui_font_family = crate::bundled_fonts::LILEX_FONT_FAMILY.to_string();
        settings.runtime_info.app_version_display =
            "WorkTree v0.0.0-overflow-regression-build".into();
        settings.runtime_info.operating_system =
            "linux (gnu-linux-overflow-regression-platform, x86_64-extra-build-metadata)".into();
        settings.runtime_info.git.version_display =
            "git version 2.51.0 (overflow-regression-build-with-very-long-metadata)".into();
        settings.runtime_info.git.compatibility = GitCompatibility::Supported;
        settings.overflow_probe = true;
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(
        px(SETTINGS_WINDOW_MIN_WIDTH_PX),
        px(SETTINGS_WINDOW_DEFAULT_HEIGHT_PX),
    ));
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    for (row_selector, label_selector, value_selector) in [
        (
            "settings_window_overflow_summary",
            "settings_window_overflow_summary_label",
            "settings_window_overflow_summary_value",
        ),
        (
            "settings_window_overflow_toggle",
            "settings_window_overflow_toggle_label",
            "settings_window_overflow_toggle_value",
        ),
        (
            "settings_window_overflow_info",
            "settings_window_overflow_info_label",
            "settings_window_overflow_info_value",
        ),
        (
            "settings_window_overflow_link",
            "settings_window_overflow_link_label",
            "settings_window_overflow_link_value",
        ),
        (
            "settings_window_git_runtime",
            "settings_window_git_runtime_label",
            "settings_window_git_runtime_value",
        ),
    ] {
        assert_debug_bounds_within(&mut settings_cx, row_selector, label_selector);
        assert_debug_bounds_within(&mut settings_cx, row_selector, value_selector);
    }
}

#[gpui::test]
fn settings_window_containers_fill_available_width_when_content_wraps(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let synthetic_fonts: Arc<[String]> = (0..24)
        .map(|ix| format!("Overflow Regression UI Font {ix:02} With Extended Width Coverage"))
        .collect::<Vec<_>>()
        .into();

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.ui_font_options = synthetic_fonts.clone();
        settings.ui_font_family = synthetic_fonts[0].clone();
        settings.expanded_section = Some(SettingsSection::UiFont);
        settings.git_executable_mode = GitExecutableMode::Custom;
        settings.runtime_info.app_version_display =
            "WorkTree v0.0.0-overflow-regression-build-with-extra-layout-metadata".into();
        settings.runtime_info.operating_system =
            "linux (gnu-linux-overflow-regression-platform with verbose wrapping metadata, x86_64)"
                .into();
        settings.runtime_info.git.version_display =
            "git version 2.51.0 (overflow-regression-build-with-very-long-metadata)".into();
        settings.runtime_info.git.compatibility = GitCompatibility::Unknown;
        settings.runtime_info.git.detail = Some(
            "This deliberately long compatibility detail must wrap inside the Git executable card without shrinking the settings containers into narrow blocks."
                .into(),
        );
        settings.settings_window_scroll = ScrollHandle::default();
        settings.ui_font_scroll = UniformListScrollHandle::default();
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_MIN_WIDTH_PX), px(1200.0)));
    settings_cx.run_until_parked();

    // Each category renders its card on its own page now, so visit every
    // category and verify the visible card fills the content-pane width.
    for (category, card_selector) in [
        (SettingsCategory::General, "settings_window_general"),
        (
            SettingsCategory::ChangeTracking,
            "settings_window_change_tracking_card",
        ),
        (SettingsCategory::Diff, "settings_window_diff_card"),
        (
            SettingsCategory::FileEditing,
            "settings_window_file_editing_card",
        ),
        (SettingsCategory::GitLog, "settings_window_git_log_card"),
        (
            SettingsCategory::GitExecutable,
            "settings_window_git_executable",
        ),
        (SettingsCategory::Environment, "settings_window_environment"),
        (SettingsCategory::Links, "settings_window_links"),
    ] {
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            settings.select_category(category, cx);
            // The General page keeps a dropdown expanded to exercise wrapping.
            if category == SettingsCategory::General {
                settings.expanded_section = Some(SettingsSection::UiFont);
            }
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });

        assert_debug_matching_horizontal_insets(
            &mut settings_cx,
            "settings_window_scroll",
            card_selector,
        );

        if category == SettingsCategory::General {
            assert_debug_matching_horizontal_insets(
                &mut settings_cx,
                "settings_window_general",
                "settings_window_ui_font_list_container",
            );
        }
        if category == SettingsCategory::GitExecutable {
            assert_debug_matching_horizontal_insets(
                &mut settings_cx,
                "settings_window_git_executable",
                "settings_window_git_executable_custom_container",
            );
        }
    }
}

#[gpui::test]
fn non_macos_settings_window_renders_custom_chrome_controls(cx: &mut gpui::TestAppContext) {
    if cfg!(target_os = "macos") {
        return;
    }

    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    for selector in [
        "settings_window_header_drag",
        "settings_window_min",
        "settings_window_max",
        "settings_window_close",
    ] {
        assert!(
            settings_cx.debug_bounds(selector).is_some(),
            "expected `{selector}` in debug bounds"
        );
    }
}

#[gpui::test]
fn linux_settings_window_close_button_closes_only_the_settings_window(
    cx: &mut gpui::TestAppContext,
) {
    if !cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return;
    }

    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        assert_eq!(app.windows().len(), 2, "expected main + settings windows");
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let close_bounds = settings_cx
        .debug_bounds("settings_window_close")
        .expect("expected settings window close control bounds");
    settings_cx.simulate_mouse_move(close_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_mouse_down(
        close_bounds.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    settings_cx.simulate_mouse_up(
        close_bounds.center(),
        MouseButton::Left,
        Modifiers::default(),
    );
    settings_cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            app.windows().len(),
            1,
            "expected the settings close control to close only the settings window"
        );
        assert!(
            app.windows()
                .into_iter()
                .all(|window| window.downcast::<SettingsWindowView>().is_none()),
            "expected the settings window to be removed"
        );
    });
}
