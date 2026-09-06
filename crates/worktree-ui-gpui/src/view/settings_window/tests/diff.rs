use super::*;

#[gpui::test]
fn expanded_diff_content_mode_section_renders_before_scroll_sync_row(
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

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX), px(1200.0)));
    settings_cx.run_until_parked();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.expanded_section = Some(SettingsSection::DiffContentMode);
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let diff_mode_row = settings_cx
        .debug_bounds("settings_window_diff_content_mode")
        .expect("expected diff mode row bounds");
    let diff_mode_container = settings_cx
        .debug_bounds("settings_window_diff_content_mode_list_container")
        .expect("expected diff mode list container bounds");
    let scroll_sync_row = settings_cx
        .debug_bounds("settings_window_diff_scroll_sync")
        .expect("expected scroll sync row bounds");

    assert!(
        diff_mode_row.bottom() <= diff_mode_container.top()
            && diff_mode_container.bottom() <= scroll_sync_row.top(),
        "expected the diff mode selector to expand directly below the diff mode row"
    );
}

#[gpui::test]
fn change_tracking_setting_defers_main_window_update(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
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

    let next_view = cx.update(|_window, app| {
        let current = settings_window
            .read_with(app, |settings, _cx| settings.change_tracking_view)
            .expect("settings window should be readable");
        match current {
            ChangeTrackingView::Combined => ChangeTrackingView::SplitUntracked,
            ChangeTrackingView::SplitUntracked => ChangeTrackingView::Combined,
        }
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_change_tracking_view(next_view, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "change tracking update should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::change_tracking_view(main_view.read(app)),
            next_view
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.change_tracking_view)
                .expect("settings window should remain readable"),
            next_view
        );
    });
}

#[gpui::test]
fn diff_scroll_sync_setting_defers_main_window_update(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
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

    let next_mode = cx.update(|_window, app| {
        let current = settings_window
            .read_with(app, |settings, _cx| settings.diff_scroll_sync)
            .expect("settings window should be readable");
        match current {
            DiffScrollSync::Both => DiffScrollSync::Vertical,
            DiffScrollSync::Vertical => DiffScrollSync::Horizontal,
            DiffScrollSync::Horizontal => DiffScrollSync::None,
            DiffScrollSync::None => DiffScrollSync::Both,
        }
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_diff_scroll_sync(next_mode, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "diff scroll sync update should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::diff_scroll_sync(main_view.read(app)),
            next_mode
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_scroll_sync)
                .expect("settings window should remain readable"),
            next_mode
        );
    });
}

#[gpui::test]
fn diff_content_mode_setting_defers_main_window_update(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
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

    let next_mode = cx.update(|_window, app| {
        let current = settings_window
            .read_with(app, |settings, _cx| settings.diff_content_mode)
            .expect("settings window should be readable");
        match current {
            DiffContentMode::Full => DiffContentMode::Collapsed,
            DiffContentMode::Collapsed => DiffContentMode::Full,
        }
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_diff_content_mode(next_mode, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "diff content mode update should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::diff_content_mode(main_view.read(app)),
            next_mode
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_content_mode)
                .expect("settings window should remain readable"),
            next_mode
        );
    });
}

#[gpui::test]
fn diff_whitespace_mode_setting_defers_main_window_update(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
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

    let next_mode = cx.update(|_window, app| {
        let current = settings_window
            .read_with(app, |settings, _cx| settings.diff_whitespace_mode)
            .expect("settings window should be readable");
        current.toggled()
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_diff_whitespace_mode(next_mode, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "diff whitespace mode update should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::diff_whitespace_mode(main_view.read(app)),
            next_mode
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_whitespace_mode)
                .expect("settings window should remain readable"),
            next_mode
        );
    });
}

#[gpui::test]
fn diff_render_settings_update_main_window(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
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

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_diff_reveal_whitespace_chars(true, cx);
                    settings.set_diff_word_wrap(true, cx);
                    settings.set_diff_show_line_numbers(false, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "diff render setting updates should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert!(crate::view::test_support::diff_reveal_whitespace_chars(
            main_view.read(app)
        ));
        assert!(crate::view::test_support::diff_word_wrap(
            main_view.read(app)
        ));
        assert!(!crate::view::test_support::diff_show_line_numbers(
            main_view.read(app)
        ));
        assert!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_reveal_whitespace_chars)
                .expect("settings window should remain readable")
        );
        assert!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_word_wrap)
                .expect("settings window should remain readable")
        );
        assert!(
            !settings_window
                .read_with(app, |settings, _cx| settings.diff_show_line_numbers)
                .expect("settings window should remain readable")
        );
    });
}

#[test]
fn diff_render_defaults_from_session_wrapper() {
    let session_file = unique_session_file("diff-defaults");
    worktree_state::session::persist_ui_settings_to_path(
        worktree_state::session::UiSettings {
            diff_reveal_whitespace_chars: Some(true),
            diff_word_wrap: Some(true),
            diff_show_line_numbers: Some(false),
            ..Default::default()
        },
        &session_file,
    )
    .expect("seed diff defaults session");

    run_subtest_with_session_env(
        "diff_render_defaults_from_session_subprocess",
        &session_file,
    );
}

#[gpui::test]
fn diff_render_defaults_from_session_subprocess(cx: &mut gpui::TestAppContext) {
    if std::env::var_os(DIFF_DEFAULTS_SESSION_SUBTEST_ENV).is_none() {
        return;
    }

    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|_window, app| {
        let view = main_view.read(app);
        assert!(crate::view::test_support::diff_reveal_whitespace_chars(
            view
        ));
        assert!(crate::view::test_support::diff_word_wrap(view));
        assert!(!crate::view::test_support::diff_show_line_numbers(view));
        assert!(view.main_pane.read(app).reveal_whitespace_chars);
        assert!(view.main_pane.read(app).diff_word_wrap);
        assert!(!view.main_pane.read(app).diff_show_line_numbers);
    });

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

    cx.update(|_window, app| {
        assert!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_reveal_whitespace_chars)
                .expect("settings window should remain readable")
        );
        assert!(
            settings_window
                .read_with(app, |settings, _cx| settings.diff_word_wrap)
                .expect("settings window should remain readable")
        );
        assert!(
            !settings_window
                .read_with(app, |settings, _cx| settings.diff_show_line_numbers)
                .expect("settings window should remain readable")
        );
    });
}
