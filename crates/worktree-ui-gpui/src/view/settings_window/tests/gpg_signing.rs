use super::*;

#[gpui::test]
fn gpg_signing_category_renders_and_records_config_writes(cx: &mut gpui::TestAppContext) {
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
        settings.select_category(SettingsCategory::GpgSigning, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_gpg_signing")
            .is_some(),
        "GPG signing card should render for its category"
    );

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        // Test builds start from an empty config: the toggle writes an
        // explicit value, and empty drafts clear their keys.
        settings.set_gpg_commit_signing(true, cx);
        settings.apply_gpg_signing_key(cx);
        settings.gpg_program_draft = "/opt/gnupg/bin/gpg".to_string();
        settings.apply_gpg_program(cx);

        assert_eq!(
            settings.gpg_config_test_writes,
            vec![
                ("commit.gpgsign".to_string(), Some("true".to_string())),
                ("user.signingkey".to_string(), None),
                (
                    "gpg.program".to_string(),
                    Some("/opt/gnupg/bin/gpg".to_string())
                ),
            ],
            "each control should write its own global git-config key"
        );
        assert!(settings.gpg_config.commit_signing_enabled);
        assert_eq!(settings.gpg_config.user_signing_key, "");
        assert_eq!(settings.gpg_config.gpg_program, "/opt/gnupg/bin/gpg");
        assert!(settings.gpg_save_error.is_none());

        // Toggling back off stays explicit — a bare `commit.gpgsign`
        // key would read as true in git.
        settings.set_gpg_commit_signing(false, cx);
        assert_eq!(
            settings.gpg_config_test_writes.last(),
            Some(&("commit.gpgsign".to_string(), Some("false".to_string())))
        );
        assert!(!settings.gpg_config.commit_signing_enabled);
    });
}
