use super::*;

#[gpui::test]
fn expanded_settings_sections_render_scrollable_list_containers(cx: &mut gpui::TestAppContext) {
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
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX), px(1800.0)));
    settings_cx.run_until_parked();

    for (section, selector) in [
        (
            SettingsSection::Theme,
            "settings_window_theme_list_container",
        ),
        (
            SettingsSection::DateFormat,
            "settings_window_date_format_list_container",
        ),
        (
            SettingsSection::UiFont,
            "settings_window_ui_font_list_container",
        ),
        (
            SettingsSection::EditorFont,
            "settings_window_editor_font_list_container",
        ),
        (
            SettingsSection::ExternalCodeEditor,
            "settings_window_external_code_editor_list_container",
        ),
        (
            SettingsSection::Timezone,
            "settings_window_timezone_list_container",
        ),
        (
            SettingsSection::ChangeTracking,
            "settings_window_change_tracking_list_container",
        ),
        (
            SettingsSection::Diff,
            "settings_window_diff_scroll_sync_list_container",
        ),
        (
            SettingsSection::DiffContentMode,
            "settings_window_diff_content_mode_list_container",
        ),
    ] {
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            settings.expanded_section = Some(section);
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });

        assert!(
            settings_cx.debug_bounds(selector).is_some(),
            "expected `{selector}` to be rendered for the expanded section"
        );
    }
}

#[gpui::test]
fn expanded_theme_section_renders_theme_utilities_and_opens_theme_guide(
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
        settings.expanded_section = Some(SettingsSection::Theme);
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_theme_links_container")
            .is_some(),
        "expected the expanded theme section to render theme utility links"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_theme_custom_folder")
            .is_some(),
        "expected the expanded theme section to render the custom folder action"
    );

    let guide_bounds = settings_cx
        .debug_bounds("settings_window_theme_guide")
        .expect("expected theme guide row bounds");
    settings_cx.simulate_click(guide_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();

    assert_eq!(cx.opened_url(), Some(THEMES_GUIDE_URL.to_string()));
}

#[gpui::test]
fn density_setting_updates_preference_and_main_window(cx: &mut gpui::TestAppContext) {
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
    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX), px(1200.0)));
    settings_cx.run_until_parked();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.select_category(SettingsCategory::General, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_ui_density")
            .is_some(),
        "density row should render in the General card"
    );

    let row_bounds = settings_cx
        .debug_bounds("settings_window_ui_density")
        .expect("density summary row bounds");
    settings_cx.simulate_click(row_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_ui_density_container")
            .is_some(),
        "density options should render once expanded"
    );

    let compact_bounds = settings_cx
        .debug_bounds("settings_window_ui_density_compact")
        .expect("compact option row should be laid out");
    settings_cx.simulate_click(compact_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        assert_eq!(settings.ui_density, crate::density::Density::Compact);
        assert_eq!(
            settings.preference_settings().ui_density,
            Some("compact".to_string()),
            "the density must ride along every settings persist"
        );
        assert_eq!(
            crate::density::current(cx).density,
            crate::density::Density::Compact
        );
    });
    settings_cx.update(|_window, app| {
        assert_eq!(
            main_view.read(app).ui_density,
            crate::density::Density::Compact,
            "the main window should follow the density selection"
        );
    });

    // Switching back has to re-expand the section first: selecting an
    // option collapses it, exactly like every other settings dropdown.
    let row_bounds = settings_cx
        .debug_bounds("settings_window_ui_density")
        .expect("density summary row bounds");
    settings_cx.simulate_click(row_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let comfortable_bounds = settings_cx
        .debug_bounds("settings_window_ui_density_comfortable")
        .expect("comfortable option row should still be laid out");
    settings_cx.simulate_click(comfortable_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(settings.ui_density, crate::density::Density::Comfortable);
    });
    settings_cx.update(|_window, app| {
        assert_eq!(
            main_view.read(app).ui_density,
            crate::density::Density::Comfortable
        );
    });
}

#[gpui::test]
fn show_timezone_toggle_defers_main_window_update(cx: &mut gpui::TestAppContext) {
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

    let next_show_timezone = cx.update(|_window, app| {
        !settings_window
            .read_with(app, |settings, _cx| settings.show_timezone)
            .expect("settings window should be readable")
    });

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_show_timezone(next_show_timezone, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "settings window toggle should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::show_timezone(main_view.read(app)),
            next_show_timezone
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.show_timezone)
                .expect("settings window should remain readable"),
            next_show_timezone
        );
    });
}

#[gpui::test]
fn auto_save_file_edits_toggle_reaches_the_main_window(cx: &mut gpui::TestAppContext) {
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

    cx.update(|_window, app| {
        assert!(
            !main_view.read(app).main_pane.read(app).auto_save_file_edits,
            "auto-save is off until it is turned on"
        );
    });

    // Nested inside a `WorkTreeView` update, as the deferral regression
    // tests do: the settings window must not re-enter the main view.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        cx.update(|_window, app| {
            main_view.update(app, |_view, cx| {
                let _ = settings_window.update(cx, |settings, _window, cx| {
                    settings.set_auto_save_file_edits(true, cx);
                });
            });
        });
    }));
    assert!(
        result.is_ok(),
        "the auto-save toggle should not re-enter WorkTreeView updates"
    );

    cx.run_until_parked();

    cx.update(|_window, app| {
        assert!(
            main_view.read(app).main_pane.read(app).auto_save_file_edits,
            "the pane that owns the editor must see the new value"
        );
        assert!(
            settings_window
                .read_with(app, |settings, _cx| settings.auto_save_file_edits)
                .expect("settings window should remain readable")
        );
    });
}

#[gpui::test]
fn ui_font_dropdown_wheel_scrolls_inner_list_before_outer_window(cx: &mut gpui::TestAppContext) {
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
    // The General page grew by the Language, Avatar, and Density rows,
    // which pushed the UI-font dropdown's hit area below the old 460px
    // window (and then below 560px); 600px keeps it in view while the page
    // still overflows (outer scroll stays active).
    settings_cx.simulate_resize(size(px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX), px(600.0)));
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let list_bounds = settings_cx
        .debug_bounds("settings_window_ui_font_list_container")
        .expect("expected UI font list bounds");

    let (outer_before, inner_before, outer_max, inner_max) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(&settings.ui_font_scroll).1,
                settings.settings_window_scroll.max_offset().y.max(px(0.0)),
                uniform_list_vertical_scroll_metrics(&settings.ui_font_scroll).2,
            )
        })
        .expect("settings window should remain readable");
    assert!(
        outer_max > px(0.0),
        "expected the settings page to be scrollable during the test"
    );
    assert!(
        inner_max > px(0.0),
        "expected the UI font list to be scrollable during the test"
    );

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(-120.0), px(0.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();

    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let (outer_after_horizontal_scroll, inner_after_horizontal_scroll) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(&settings.ui_font_scroll).1,
            )
        })
        .expect("settings window should remain readable");

    assert!(
        (inner_after_horizontal_scroll - inner_before).abs() <= px(0.5),
        "expected horizontal-only wheel scroll not to move the UI font list vertically"
    );
    assert!(
        (outer_after_horizontal_scroll - outer_before).abs() <= px(0.5),
        "expected horizontal-only wheel scroll not to move the outer settings page vertically"
    );

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();

    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let (outer_after_inner_scroll, inner_after_inner_scroll) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(&settings.ui_font_scroll).1,
            )
        })
        .expect("settings window should remain readable");

    assert!(
        inner_after_inner_scroll > inner_before + px(0.5),
        "expected the UI font list to consume wheel scroll first"
    );
    assert!(
        (outer_after_inner_scroll - outer_before).abs() <= px(0.5),
        "expected the outer settings page to stay still while the UI font list can still scroll"
    );

    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        let (raw_offset, _scroll_offset, max_offset) =
            uniform_list_vertical_scroll_metrics(&settings.ui_font_scroll);
        let current_x = settings.ui_font_scroll.0.borrow().base_handle.offset().x;
        let target_y = if raw_offset > px(0.0) {
            max_offset
        } else {
            -max_offset
        };
        settings
            .ui_font_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(current_x, target_y));
        cx.notify();
    });
    settings_cx.run_until_parked();

    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let outer_before_boundary_handoff = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            absolute_scroll_y(&settings.settings_window_scroll)
        })
        .expect("settings window should remain readable");

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();

    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let outer_after_boundary_handoff = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            absolute_scroll_y(&settings.settings_window_scroll)
        })
        .expect("settings window should remain readable");

    assert!(
        outer_after_boundary_handoff > outer_before_boundary_handoff + px(0.5),
        "expected wheel scrolling to bubble to the outer settings page once the UI font list reaches its boundary"
    );
}

#[gpui::test]
fn avatar_source_dropdown_wheel_chains_to_outer_page_when_list_cannot_scroll(
    cx: &mut gpui::TestAppContext,
) {
    // Three sources fit without scrolling, so chaining to the page is the
    // correct behavior — and what the handler must preserve.
    assert_dropdown_wheel_chains_to_outer_page_when_list_cannot_scroll(
        cx,
        "settings_window_avatar_source_list_container",
        420.0,
        |settings| &settings.avatar_source_scroll,
        |settings, _cx| {
            settings.expanded_section = Some(SettingsSection::AvatarSource);
        },
    );
}
