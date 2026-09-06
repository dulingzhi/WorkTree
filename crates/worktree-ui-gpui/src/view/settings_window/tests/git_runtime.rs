use super::*;

#[test]
fn git_executable_mode_tracks_runtime_preference() {
    assert_eq!(
        GitExecutableMode::from_preference(&GitExecutablePreference::SystemPath),
        GitExecutableMode::SystemPath
    );
    assert_eq!(
        GitExecutableMode::from_preference(&GitExecutablePreference::Custom(PathBuf::from(
            "/opt/git/bin/git"
        ),)),
        GitExecutableMode::Custom
    );
}

#[test]
fn git_runtime_info_from_state_surfaces_unavailable_detail() {
    let runtime = GitRuntimeState {
        preference: GitExecutablePreference::Custom(PathBuf::new()),
        availability: GitExecutableAvailability::Unavailable {
            detail: "Custom Git executable is not configured. Choose an executable or switch back to System PATH.".to_string(),
        },
    };

    let info = git_runtime_info_from_state(runtime.clone());
    assert_eq!(info.runtime, runtime);
    assert_eq!(info.compatibility, GitCompatibility::Unavailable);
    assert_eq!(info.version_display.as_ref(), "Unavailable");
    assert_eq!(
        info.detail.as_ref().map(|detail| detail.as_ref()),
        Some(
            "Custom Git executable is not configured. Choose an executable or switch back to System PATH."
        )
    );
}

#[test]
fn applied_git_executable_path_tracks_runtime_preference() {
    assert_eq!(
        applied_git_executable_path(&GitRuntimeState {
            preference: GitExecutablePreference::SystemPath,
            availability: GitExecutableAvailability::Available {
                version_output: "git version 2.51.0".to_string(),
            },
        }),
        None
    );
    assert_eq!(
        applied_git_executable_path(&GitRuntimeState {
            preference: GitExecutablePreference::Custom(PathBuf::from("/opt/git/bin/git")),
            availability: GitExecutableAvailability::Available {
                version_output: "git version 2.51.0".to_string(),
            },
        }),
        Some(PathBuf::from("/opt/git/bin/git"))
    );
    assert_eq!(
        applied_git_executable_path(&GitRuntimeState {
            preference: GitExecutablePreference::Custom(PathBuf::new()),
            availability: GitExecutableAvailability::Unavailable {
                detail: "missing".to_string(),
            },
        }),
        Some(PathBuf::new())
    );
}

#[test]
fn git_executable_scope_note_mentions_browser_only_scope() {
    let note = git_executable_scope_note();
    assert!(
        note.contains("browser window"),
        "expected browser-only scope note, got: {note}"
    );
    assert!(
        note.contains("System PATH"),
        "expected command-mode fallback note, got: {note}"
    );
}

#[test]
fn parse_git_version_extracts_first_version_token() {
    assert_eq!(
        parse_git_version("git version 2.50.7"),
        Some(GitVersion {
            major: 2,
            minor: 50
        })
    );
}

#[test]
fn parse_git_version_token_accepts_numeric_prefixes_and_rejects_non_numeric_prefixes() {
    assert_eq!(
        parse_git_version_token("2.45.1.windows.1"),
        Some(GitVersion {
            major: 2,
            minor: 45
        })
    );
    assert_eq!(parse_git_version_token("v2.45.1"), None);
    assert_eq!(parse_u32_prefix("53rc1"), Some(53));
    assert_eq!(parse_u32_prefix("rc53"), None);
}

#[test]
fn supported_version_requires_minimum_2_50() {
    assert!(is_supported_git_version(GitVersion {
        major: MIN_GIT_MAJOR,
        minor: MIN_GIT_MINOR,
    }));
    assert!(is_supported_git_version(GitVersion {
        major: MIN_GIT_MAJOR,
        minor: MIN_GIT_MINOR + 1,
    }));
    assert!(!is_supported_git_version(GitVersion {
        major: MIN_GIT_MAJOR,
        minor: MIN_GIT_MINOR - 1,
    }));
    assert!(is_supported_git_version(GitVersion {
        major: MIN_GIT_MAJOR + 1,
        minor: 0,
    }));
}

#[gpui::test]
fn custom_git_executable_mode_renders_detail_container(cx: &mut gpui::TestAppContext) {
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
        settings.select_category(SettingsCategory::GitExecutable, cx);
        settings.git_executable_mode = GitExecutableMode::Custom;
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        settings_cx
            .debug_bounds("settings_window_git_executable_custom_container")
            .is_some(),
        "expected custom git executable mode to render its detail container"
    );
}
