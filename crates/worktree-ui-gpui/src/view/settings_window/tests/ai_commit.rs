use super::*;

#[gpui::test]
fn ai_commit_drafts_follow_the_provider_switch(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    // `ai_commit::current()` is a process global; serialize against the
    // panels' ✨ generation tests and restore the default on exit.
    struct RestoreAiSettings {
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for RestoreAiSettings {
        fn drop(&mut self) {
            // Still holding `_lock` (fields drop after `drop` returns), so
            // the restore stays ordered against every other test's setup —
            // an unlocked restore here could clobber a parallel
            // generation test's configured settings mid-flight.
            crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        }
    }
    let _restore = {
        let lock = crate::ai_commit::lock_test_settings();
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        RestoreAiSettings { _lock: lock }
    };

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

    // Typing an API key flows into the process global the ✨ button reads.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.ai_commit_api_key_input.update(cx, |input, cx| {
                input.set_text("sk-settings-test", cx);
            });
        });
    });
    cx.run_until_parked();
    cx.update(|_window, _app| {
        let current = crate::ai_commit::current();
        assert_eq!(current.api_key, "sk-settings-test");
        assert!(
            current.is_configured(),
            "an API key alone makes the provider usable"
        );
    });

    // Switching providers resets model and endpoint to the new defaults
    // while keeping the key.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.set_ai_commit_provider(crate::ai_commit::AiProvider::OpenAiCompatible, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let current = crate::ai_commit::current();
        assert_eq!(
            current.provider,
            crate::ai_commit::AiProvider::OpenAiCompatible
        );
        assert_eq!(current.api_key, "sk-settings-test");
        assert_eq!(
            current.model,
            crate::ai_commit::AiProvider::OpenAiCompatible.default_model()
        );
        assert_eq!(current.endpoint, "");

        let (model_text, endpoint_text) = settings_window
            .read_with(app, |settings, cx| {
                (
                    settings
                        .ai_commit_model_input
                        .read_with(cx, |input, _| input.text().to_string()),
                    settings
                        .ai_commit_endpoint_input
                        .read_with(cx, |input, _| input.text().to_string()),
                )
            })
            .expect("settings window should remain readable");
        assert_eq!(
            model_text,
            crate::ai_commit::AiProvider::OpenAiCompatible.default_model()
        );
        assert_eq!(endpoint_text, "");
    });
}

#[gpui::test]
fn ai_commit_model_list_fetch_and_pick(cx: &mut gpui::TestAppContext) {
    let _visual_guard = lock_visual_test();
    // Serialize against the other tests that touch the process global.
    struct RestoreAiSettings {
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for RestoreAiSettings {
        fn drop(&mut self) {
            // Still holding `_lock` (fields drop after `drop` returns), so
            // the restore stays ordered against every other test's setup —
            // an unlocked restore here could clobber a parallel
            // generation test's configured settings mid-flight.
            crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        }
    }
    let _restore = {
        let lock = crate::ai_commit::lock_test_settings();
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        RestoreAiSettings { _lock: lock }
    };

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

    // The fetch button enters the loading state and issues one request;
    // a second click while loading must not fire another.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.fetch_ai_commit_models(cx);
            settings.fetch_ai_commit_models(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        settings_window
            .read_with(app, |settings, _cx| {
                assert_eq!(settings.ai_commit_models, AiCommitModels::Loading);
                assert_eq!(settings.ai_commit_models_test_fetches, 1);
            })
            .expect("settings window should remain readable");
    });

    // A successful reply turns into the pickable list, and picking a row
    // lands in the input, the draft, and the process global.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.finish_ai_commit_models_fetch(
                Ok(vec!["glm-5.3".to_string(), "kimi-k3".to_string()]),
                cx,
            );
            settings.set_ai_commit_model("kimi-k3".to_string(), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        settings_window
            .read_with(app, |settings, cx| {
                assert_eq!(
                    settings.ai_commit_models,
                    AiCommitModels::Ready(
                        vec!["glm-5.3".to_string(), "kimi-k3".to_string()].into()
                    )
                );
                settings.ai_commit_model_input.read_with(cx, |input, _| {
                    assert_eq!(input.text(), "kimi-k3");
                });
            })
            .expect("settings window should remain readable");
        assert_eq!(crate::ai_commit::current().model, "kimi-k3");
    });

    // Switching providers drops the stale list — it belongs to the old
    // provider's endpoint.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.set_ai_commit_provider(crate::ai_commit::AiProvider::OpenAiCompatible, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        settings_window
            .read_with(app, |settings, _cx| {
                assert_eq!(settings.ai_commit_models, AiCommitModels::NotFetched);
            })
            .expect("settings window should remain readable");
    });
}

#[gpui::test]
fn ai_commit_source_selection_gates_fields_and_reports_availability(cx: &mut gpui::TestAppContext) {
    use crate::ai_commit_sources::AiSource;

    let _visual_guard = lock_visual_test();
    // `ai_commit::current()` is a process global; serialize against the
    // other AI settings tests and restore the default on exit.
    struct RestoreAiSettings {
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for RestoreAiSettings {
        fn drop(&mut self) {
            // Still holding `_lock` (fields drop after `drop` returns), so
            // the restore stays ordered against every other test's setup —
            // an unlocked restore here could clobber a parallel
            // generation test's configured settings mid-flight.
            crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        }
    }
    let _restore = {
        let lock = crate::ai_commit::lock_test_settings();
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
        RestoreAiSettings { _lock: lock }
    };

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

    // The fresh window sits on manual, with no availability in flight.
    cx.update(|_window, app| {
        let _ = settings_window.read_with(app, |settings, _cx| {
            assert_eq!(settings.ai_commit_source, AiSource::Manual);
            assert!(settings.ai_commit_availability.is_none());
        });
    });

    // Switching to an external source persists the choice, keeps the
    // manual drafts for a later switch back, and produces an
    // availability verdict in the background. The verdict itself depends
    // on the machine's real ~/.claude — only its shape is asserted.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.set_ai_commit_source(AiSource::ClaudeCode, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        assert_eq!(crate::ai_commit::current().source, AiSource::ClaudeCode);
        let _ = settings_window.read_with(app, |settings, _cx| {
            let availability = settings
                .ai_commit_availability
                .clone()
                .expect("availability should settle after the background check");
            if !availability.detected {
                assert_eq!(
                    availability.message.map(|(key, _)| key),
                    Some("settings.ai_commit.missing.claude_code")
                );
            }
            // The summary now names the source, not the manual provider.
            assert_eq!(settings.ai_commit_summary(), AiSource::ClaudeCode.label());
        });
    });

    // Switching back to manual clears the verdict and restores the
    // manual drafts untouched.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.set_ai_commit_source(AiSource::Manual, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let current = crate::ai_commit::current();
        assert_eq!(current.source, AiSource::Manual);
        assert_eq!(current.provider, crate::ai_commit::AiProvider::Anthropic);
        let _ = settings_window.read_with(app, |settings, _cx| {
            assert!(settings.ai_commit_availability.is_none());
        });
    });

    // A custom command is required for the custom source: empty reports
    // missing, typing one flips the verdict to detected.
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings.set_ai_commit_source(AiSource::Custom, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let _ = settings_window.read_with(app, |settings, _cx| {
            let availability = settings
                .ai_commit_availability
                .clone()
                .expect("custom availability should settle");
            assert!(!availability.detected);
            assert_eq!(
                availability.message.map(|(key, _)| key),
                Some("settings.ai_commit.missing.custom_empty")
            );
        });
    });
    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            settings
                .ai_commit_custom_command_input
                .update(cx, |input, cx| {
                    input.set_text("my-tool --flag {PROMPT}", cx);
                });
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        assert_eq!(
            crate::ai_commit::current().custom_command,
            "my-tool --flag {PROMPT}"
        );
        let _ = settings_window.read_with(app, |settings, _cx| {
            let availability = settings
                .ai_commit_availability
                .clone()
                .expect("custom availability should re-settle after typing");
            assert!(availability.detected);
        });
    });
}

#[gpui::test]
fn ai_commit_source_dropdown_wheel_scrolls_inner_list_before_outer_window(
    cx: &mut gpui::TestAppContext,
) {
    // 10 sources overflow the dropdown cap; before the fix, the wheel also
    // scrolled the settings page behind the open list.
    assert_dropdown_wheel_stops_at_list(
        cx,
        "settings_window_ai_commit_source_list_container",
        560.0,
        |settings| &settings.ai_commit_source_scroll,
        |settings, _cx| {
            settings.expanded_section = Some(SettingsSection::AiCommitMessage);
        },
    );
}

#[gpui::test]
fn ai_commit_model_dropdown_wheel_scrolls_inner_list_before_outer_window(
    cx: &mut gpui::TestAppContext,
) {
    let synthetic_models: Arc<[String]> = (0..200)
        .map(|ix| format!("Test Model {ix:03}"))
        .collect::<Vec<_>>()
        .into();
    assert_dropdown_wheel_stops_at_list(
        cx,
        "settings_window_ai_commit_model_list_container",
        560.0,
        |settings| &settings.ai_commit_models_scroll,
        |settings, _cx| {
            settings.ai_commit_models = AiCommitModels::Ready(synthetic_models.clone());
            settings.expanded_section = Some(SettingsSection::AiCommitMessage);
        },
    );
}

#[gpui::test]
fn ai_commit_provider_dropdown_wheel_chains_to_outer_page_when_list_cannot_scroll(
    cx: &mut gpui::TestAppContext,
) {
    assert_dropdown_wheel_chains_to_outer_page_when_list_cannot_scroll(
        cx,
        "settings_window_ai_commit_provider_list_container",
        560.0,
        |settings| &settings.ai_commit_provider_scroll,
        |settings, _cx| {
            settings.expanded_section = Some(SettingsSection::AiCommitMessage);
        },
    );
}
