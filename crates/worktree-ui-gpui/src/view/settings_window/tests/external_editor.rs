use super::*;

#[gpui::test]
fn custom_external_editor_renders_detail_container(cx: &mut gpui::TestAppContext) {
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
    assert!(
        settings_cx
            .debug_bounds("settings_window_external_code_editor_custom_container")
            .is_none(),
        "expected external editor custom details to stay hidden for the default None setting"
    );

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.external_editor_setting = Some(ExternalCodeEditorSetting::Custom {
            executable: PathBuf::new(),
            arguments: None,
        });
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_external_code_editor_custom_container")
            .is_some(),
        "expected custom external editor mode to render its detail container"
    );
}

#[gpui::test]
fn browsed_external_editor_path_updates_custom_setting_and_notifies(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let _external_editor_guard = crate::external_editor::configured_setting_override_test_guard();
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

    let editor_path = PathBuf::from("/tmp/worktree-custom-editor");
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.apply_browsed_external_editor_path(editor_path.clone(), cx);

        assert_eq!(
            settings.external_editor_setting,
            Some(ExternalCodeEditorSetting::Custom {
                executable: editor_path.clone(),
                arguments: None,
            })
        );
        assert_eq!(
            settings.external_editor_custom_path_draft,
            editor_path.display().to_string()
        );
        assert_eq!(
            settings
                .external_editor_custom_path_input
                .read(cx)
                .text()
                .to_string(),
            editor_path.display().to_string()
        );
        assert_eq!(settings.external_editor_browse_notify_count, 1);
    });
}

#[test]
fn custom_external_editor_browse_prompt_allows_app_bundle_directories() {
    let options = custom_external_editor_path_prompt_options();

    assert!(
        options.files,
        "custom external editor browsing should still allow executable files"
    );
    assert!(
        options.directories,
        "custom external editor browsing should allow macOS .app bundle directories"
    );
    assert!(
        !options.multiple,
        "custom external editor browsing should remain a single-selection prompt"
    );
    assert_eq!(
        options.prompt.as_ref().map(ToString::to_string),
        Some("Select external code editor".to_string())
    );
}

#[gpui::test]
fn external_editor_setting_seeds_from_pending_override_and_can_clear(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = lock_visual_test();
    let _external_editor_guard = crate::external_editor::configured_setting_override_test_guard();
    let pending_setting = ExternalCodeEditorSetting::Custom {
        executable: PathBuf::from("/tmp/worktree-pending-editor"),
        arguments: Some("--reuse-window {path}".to_string()),
    };
    crate::external_editor::set_configured_setting_override(Some(pending_setting.clone()));

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

    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            assert_eq!(
                settings.external_editor_setting,
                Some(pending_setting.clone()),
                "settings should use the pending in-memory editor preference before session persistence finishes"
            );

            settings.set_external_editor_setting(None, cx);

            assert_eq!(settings.external_editor_setting, None);
            assert_eq!(
                crate::external_editor::configured_setting_preference_override(),
                Some(None),
                "clearing the reopened settings window should replace the pending editor preference"
            );
        });
    });
}

#[test]
fn external_editor_preference_persist_queue_skips_stale_custom_draft_writes() {
    let session_file = unique_session_file("external-editor-draft-sequence");
    let queue = ExternalEditorPreferencePersistQueue::default();
    let stale_setting = Some(ExternalCodeEditorSetting::Custom {
        executable: PathBuf::from("/tmp/editor"),
        arguments: Some("--reuse".to_string()),
    });
    let latest_setting = Some(ExternalCodeEditorSetting::Custom {
        executable: PathBuf::from("/tmp/editor-final"),
        arguments: Some("--reuse-window {path}".to_string()),
    });

    let stale_sequence = queue.next_sequence();
    let latest_sequence = queue.next_sequence();

    assert!(
        queue
            .persist_to_path_if_latest(latest_sequence, latest_setting.clone(), &session_file)
            .expect("persist latest custom editor draft")
    );
    assert!(
        !queue
            .persist_to_path_if_latest(stale_sequence, stale_setting, &session_file)
            .expect("skip stale custom editor draft")
    );

    let loaded = worktree_state::session::load_from_path(&session_file);
    assert_eq!(loaded.external_code_editor, latest_setting);
}

#[gpui::test]
fn generic_preference_persistence_omits_external_editor_snapshot(cx: &mut gpui::TestAppContext) {
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

    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, _cx| {
            settings.external_editor_setting = Some(ExternalCodeEditorSetting::Custom {
                executable: PathBuf::from("/tmp/editor-before-theme-change"),
                arguments: Some("--reuse-window {path}".to_string()),
            });
            let persisted = settings.preference_settings();
            assert_eq!(persisted.external_code_editor, None);
        });
    });
}
