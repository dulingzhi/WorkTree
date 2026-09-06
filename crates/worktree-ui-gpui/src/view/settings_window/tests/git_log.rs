use super::*;

#[gpui::test]
fn expanded_history_columns_section_renders_detail_container(cx: &mut gpui::TestAppContext) {
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
        settings.expanded_section = Some(SettingsSection::GitLogColumns);
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_git_log_columns_container")
            .is_some(),
        "expected the history columns section to render its detail container when expanded"
    );
}

#[gpui::test]
fn expanded_git_log_default_mode_section_renders_modes_in_order_and_updates_selection(
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
        settings.expanded_section = Some(SettingsSection::GitLogDefaultMode);
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let mut previous_top = None;
    for spec in crate::view::history_mode::history_mode_ui_specs() {
        let bounds = settings_cx
            .debug_bounds(spec.settings_row_id)
            .unwrap_or_else(|| panic!("expected `{}` bounds", spec.settings_row_id));
        if let Some(previous_top) = previous_top {
            assert!(
                bounds.top() > previous_top,
                "expected `{}` to appear below the previous history mode row",
                spec.settings_row_id
            );
        }
        previous_top = Some(bounds.top());
    }

    let selected = crate::view::history_mode::history_mode_ui_specs()
        .last()
        .copied()
        .expect("history modes");
    let initial_selected_bounds = settings_cx
        .debug_bounds(selected.settings_row_id)
        .expect("expected selected row bounds");
    let scroll_bounds = settings_cx
        .debug_bounds("settings_window_scroll")
        .expect("expected settings scroll bounds");
    let selected_center = initial_selected_bounds.center();
    if selected_center.y >= scroll_bounds.bottom() {
        let scroll_delta = selected_center.y - scroll_bounds.bottom() + px(24.0);
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            let current = settings.settings_window_scroll.offset();
            settings
                .settings_window_scroll
                .set_offset(point(current.x, current.y - scroll_delta));
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });
    }
    let selected_bounds = settings_cx
        .debug_bounds(selected.settings_row_id)
        .expect("expected selected row bounds");
    settings_cx.simulate_click(selected_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();

    cx.update(|_window, app| {
        assert_eq!(
            settings_window
                .read_with(app, |settings, _cx| settings.default_history_mode)
                .expect("settings window should remain readable"),
            selected.mode
        );
    });
}

#[gpui::test]
fn expanded_git_log_default_mode_section_renders_before_history_columns_row(
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
        settings.expanded_section = Some(SettingsSection::GitLogDefaultMode);
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let default_mode_container = settings_cx
        .debug_bounds("settings_window_git_log_default_mode_container")
        .expect("expected default history mode container bounds");
    let history_columns_row = settings_cx
        .debug_bounds("settings_window_git_log_columns")
        .expect("expected history columns row bounds");

    assert!(
        default_mode_container.bottom() <= history_columns_row.top(),
        "expected the default history mode container to appear before the history columns row"
    );
}
