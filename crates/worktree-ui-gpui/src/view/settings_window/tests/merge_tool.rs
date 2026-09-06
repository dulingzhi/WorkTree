use super::*;

#[gpui::test]
fn merge_tool_category_renders_and_persists_selection(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let _preference_lock = worktree_core::external_merge_tool::lock_external_merge_tool_test();
    let _preference_guard =
        worktree_core::external_merge_tool::ExternalMergeToolResetGuard::install(
            ExternalMergeToolSelection::FromGitConfig,
        );

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
        settings.select_category(SettingsCategory::MergeTool, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool")
            .is_some(),
        "merge tool card should render for its category"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_list_container")
            .is_none(),
        "dropdown stays collapsed until the row is expanded"
    );

    let row_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_selection")
        .expect("merge tool summary row bounds");
    settings_cx.simulate_click(row_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_list_container")
            .is_some(),
        "dropdown should render once expanded"
    );

    let vscode_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_option_vscode")
        .expect("vscode preset row should be laid out");
    settings_cx.simulate_click(vscode_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();

    let expected = ExternalMergeToolSelection::Builtin {
        id: "vscode".to_string(),
        path: None,
    };
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(settings.merge_tool_selection, expected);
        assert_eq!(
            settings.preference_settings().external_merge_tool,
            Some(expected.clone()),
            "the preference must ride along every settings persist"
        );
    });
    assert_eq!(
        worktree_core::external_merge_tool::current_external_merge_tool(),
        expected,
        "selecting a tool installs it for the next conflicted right-click"
    );

    let default_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_option_from_git_config")
        .expect("from-git-config row should be laid out");
    settings_cx.simulate_click(default_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::FromGitConfig
        );
    });
}

#[gpui::test]
fn merge_tool_custom_command_and_trust_toggle_only_for_custom(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let _preference_lock = worktree_core::external_merge_tool::lock_external_merge_tool_test();
    let _preference_guard =
        worktree_core::external_merge_tool::ExternalMergeToolResetGuard::install(
            ExternalMergeToolSelection::FromGitConfig,
        );

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
        settings.select_category(SettingsCategory::MergeTool, cx);
        settings.toggle_section(SettingsSection::MergeTool, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_custom_container")
            .is_none()
            && settings_cx
                .debug_bounds("settings_window_merge_tool_trust_exit_code")
                .is_none(),
        "custom command and trust toggle stay hidden for non-custom selections"
    );

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.set_merge_tool_selection(
            ExternalMergeToolSelection::Custom {
                command: "code --wait --merge $REMOTE $LOCAL $BASE $MERGED".to_string(),
                trust_exit_code: false,
            },
            cx,
        );
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_custom_container")
            .is_some()
            && settings_cx
                .debug_bounds("settings_window_merge_tool_trust_exit_code")
                .is_some(),
        "custom command and trust toggle appear for the custom selection"
    );

    // The trust row sits below the fold of the page scroller; bring it
    // into view before clicking, like the links-page row tests do.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
        settings.settings_window_scroll.set_offset(point(
            settings.settings_window_scroll.offset().x,
            -max_offset,
        ));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let trust_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_trust_exit_code")
        .expect("trust exit code row bounds after scrolling");
    settings_cx.simulate_click(trust_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Custom {
                command: "code --wait --merge $REMOTE $LOCAL $BASE $MERGED".to_string(),
                trust_exit_code: true,
            },
            "clicking the toggle flips trust-exit-code on the selection"
        );
    });

    // Typing in the command input updates the live selection.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings
            .merge_tool_custom_command_input
            .update(cx, |input, cx| {
                input.set_text("meld $LOCAL $BASE $REMOTE $MERGED", cx);
            });
    });
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Custom {
                command: "meld $LOCAL $BASE $REMOTE $MERGED".to_string(),
                trust_exit_code: true,
            },
            "typing in the custom command input updates the selection"
        );
    });
    assert_eq!(
        worktree_core::external_merge_tool::current_external_merge_tool(),
        ExternalMergeToolSelection::Custom {
            command: "meld $LOCAL $BASE $REMOTE $MERGED".to_string(),
            trust_exit_code: true,
        },
        "edits install immediately for the next conflicted right-click"
    );
}

#[gpui::test]
fn merge_tool_availability_row_reports_status(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let _preference_lock = worktree_core::external_merge_tool::lock_external_merge_tool_test();
    let _preference_guard =
        worktree_core::external_merge_tool::ExternalMergeToolResetGuard::install(
            ExternalMergeToolSelection::FromGitConfig,
        );

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
        settings.select_category(SettingsCategory::MergeTool, cx);
        settings.set_merge_tool_selection(
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: None,
            },
            cx,
        );
        settings.toggle_section(SettingsSection::MergeTool, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    // Whether `code` is on PATH depends on the machine; what must hold is
    // that the background probe completes and renders a status row.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert!(
            settings.merge_tool_availability.is_some(),
            "the PATH probe should have completed after expanding"
        );
    });
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_availability")
            .is_some(),
        "availability status row should render for a built-in preset"
    );

    // An id the preset table does not know gets an honest warning instead
    // of a silent reset.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.set_merge_tool_selection(
            ExternalMergeToolSelection::Builtin {
                id: "not-a-real-tool".to_string(),
                path: None,
            },
            cx,
        );
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert!(
            settings.merge_tool_availability.is_none(),
            "no probe runs for a preset the table does not know"
        );
    });
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_availability")
            .is_some(),
        "the unknown-id warning row should render in place of the probe"
    );
}

#[gpui::test]
fn merge_tool_manual_executable_path_browse_clear_and_persist(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    let _preference_lock = worktree_core::external_merge_tool::lock_external_merge_tool_test();
    let _preference_guard =
        worktree_core::external_merge_tool::ExternalMergeToolResetGuard::install(
            ExternalMergeToolSelection::FromGitConfig,
        );

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

    // A real file on disk so the availability probe resolves it.
    let tool_dir = tempfile::tempdir().unwrap();
    let tool_exe = tool_dir.path().join("kdiff3.exe");
    std::fs::write(&tool_exe, b"").unwrap();
    let rendered = tool_exe.display().to_string();

    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.select_category(SettingsCategory::MergeTool, cx);
        settings.set_merge_tool_selection(
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: None,
            },
            cx,
        );
        settings.toggle_section(SettingsSection::MergeTool, cx);
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_executable_path_container")
            .is_some(),
        "the executable path detail should render for a built-in preset"
    );
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_executable_path_clear")
            .is_none(),
        "clear should be hidden while no manual path is set"
    );

    // Browse picks a file: the selection, input text and installed
    // preference all adopt it, and the probe reports it as effective.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.apply_browsed_merge_tool_path(tool_exe.clone(), cx);
    });
    settings_cx.run_until_parked();
    // The path detail sits below the fold of the page scroller; bring it
    // into view before asserting, like the trust-row test does.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
        settings.settings_window_scroll.set_offset(point(
            settings.settings_window_scroll.offset().x,
            -max_offset,
        ));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(rendered.clone()),
            }
        );
        assert_eq!(settings.merge_tool_executable_path_draft, rendered);
        assert_eq!(
            settings.preference_settings().external_merge_tool,
            Some(ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(rendered.clone()),
            })
        );
        assert_eq!(
            worktree_core::external_merge_tool::current_external_merge_tool(),
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(rendered.clone()),
            }
        );
        assert_eq!(
            settings.merge_tool_availability,
            Some(MergeToolAvailability::Available {
                resolved: rendered.clone(),
                via_override: true,
            })
        );
    });
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_executable_path_clear")
            .is_some(),
        "clear should render once a manual path is set"
    );

    // Re-clicking the selected preset keeps the manual path; a different
    // preset starts fresh because the old path names the wrong tool.
    let vscode_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_option_vscode")
        .expect("vscode preset row should be laid out");
    settings_cx.simulate_click(vscode_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(rendered.clone()),
            },
            "re-clicking the selected preset must keep the manual path"
        );
    });
    let kdiff3_bounds = settings_cx
        .debug_bounds("settings_window_merge_tool_option_kdiff3")
        .expect("kdiff3 preset row should be laid out");
    settings_cx.simulate_click(kdiff3_bounds.center(), Modifiers::default());
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Builtin {
                id: "kdiff3".to_string(),
                path: None,
            },
            "switching presets must drop the previous manual path"
        );
    });

    // Switch back and clear: the manual path is gone from selection,
    // preference and field, handing resolution back to PATH.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        settings.set_merge_tool_selection(
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(rendered.clone()),
            },
            cx,
        );
        settings.clear_merge_tool_manual_path(cx);
    });
    settings_cx.run_until_parked();
    let _ = settings_window.update(&mut settings_cx, |settings, _window, _cx| {
        assert_eq!(
            settings.merge_tool_selection,
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: None,
            }
        );
        // Clearing hands the field back to the PATH echo: the draft shows
        // the resolved executable when PATH has one, empty otherwise.
        let expected_echo = match &settings.merge_tool_availability {
            Some(MergeToolAvailability::Available {
                resolved,
                via_override: false,
            }) => resolved.clone(),
            _ => String::new(),
        };
        assert_eq!(settings.merge_tool_executable_path_draft, expected_echo);
        assert_eq!(
            settings.preference_settings().external_merge_tool,
            Some(ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: None,
            })
        );
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        settings_cx
            .debug_bounds("settings_window_merge_tool_executable_path_clear")
            .is_none(),
        "clear hides again once the manual path is dropped"
    );
}

#[gpui::test]
fn merge_tool_dropdown_wheel_scrolls_inner_list_before_outer_window(cx: &mut gpui::TestAppContext) {
    assert_dropdown_wheel_stops_at_list(
        cx,
        "settings_window_merge_tool_list_container",
        320.0,
        |settings| &settings.merge_tool_scroll,
        |settings, _cx| {
            settings.expanded_section = Some(SettingsSection::MergeTool);
        },
    );
}
