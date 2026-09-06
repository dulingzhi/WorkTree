use super::*;

#[gpui::test]
fn settings_window_open_source_licenses_row_switches_content(cx: &mut gpui::TestAppContext) {
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
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.select_category(SettingsCategory::Links, cx);
        // Keep the interaction test resilient as rows are added to the root links card.
        let current_x = settings.settings_window_scroll.offset().x;
        let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
        settings
            .settings_window_scroll
            .set_offset(point(current_x, -max_offset));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let row_bounds = settings_cx
        .debug_bounds("settings_window_open_source_licenses")
        .expect("expected open source licenses row bounds");
    settings_cx.simulate_click(row_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert_eq!(
            app.windows().len(),
            2,
            "expected the settings window to reuse the existing window"
        );
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.current_view)
                .expect("settings window should remain readable"),
            SettingsView::OpenSourceLicenses,
            "expected the settings window to switch to open source licenses content"
        );
    });

    assert_eq!(
        settings_cx.window_title().as_deref(),
        Some(SETTINGS_WINDOW_TITLE),
        "expected the settings window to keep its OS title"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_breadcrumb_settings")
            .is_some(),
        "expected a breadcrumb back control in the licenses view"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_open_source_licenses_columns")
            .is_some(),
        "expected open source licenses columns in debug bounds"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_open_source_licenses_scrollbar")
            .is_some(),
        "expected a visible scrollbar in the open source licenses view"
    );

    let back_bounds = settings_cx
        .debug_bounds("settings_window_breadcrumb_settings")
        .expect("expected breadcrumb back control bounds");
    settings_cx.simulate_click(back_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.current_view)
                .expect("settings window should remain readable"),
            SettingsView::Root,
            "expected the breadcrumb back control to return to the root settings view"
        );
    });
}

#[gpui::test]
fn settings_window_professional_edition_waitlist_row_opens_editions_page(
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
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.select_category(SettingsCategory::Links, cx);
        // Keep the interaction test resilient as sections are added above the links card.
        let current_x = settings.settings_window_scroll.offset().x;
        let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
        settings
            .settings_window_scroll
            .set_offset(point(current_x, -max_offset));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let row_bounds = settings_cx
        .debug_bounds("settings_window_professional_edition_waitlist")
        .expect("expected professional edition waitlist row bounds");
    settings_cx.simulate_click(row_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();

    assert_eq!(cx.opened_url(), Some(EDITIONS_URL.to_string()));
}

#[gpui::test]
fn settings_window_links_card_includes_theme_guide_row(cx: &mut gpui::TestAppContext) {
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
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.select_category(SettingsCategory::Links, cx);
        let current_x = settings.settings_window_scroll.offset().x;
        let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
        settings
            .settings_window_scroll
            .set_offset(point(current_x, -max_offset));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_links_theme_guide")
            .is_some(),
        "expected the Links card to include a Theme guide row"
    );
}
