use super::*;

use super::branch::create_tracking_store;

fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} should be on screen"));
    let center = bounds.center();
    cx.simulate_event(gpui::MouseDownEvent {
        position: center,
        modifiers: Default::default(),
        button: gpui::MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: center,
        modifiers: Default::default(),
        button: gpui::MouseButton::Left,
        click_count: 1,
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
}

#[gpui::test]
fn repo_settings_prompt_renders_fields_and_cycles_the_sign_tri_state(
    cx: &mut gpui::TestAppContext,
) {
    let (store, events, _repo, _workdir) = create_tracking_store("repo-settings");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::RepoSettingsPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });

    let opened = cx.update(|_window, app| {
        view.read(app)
            .popover_host
            .read(app)
            .popover_kind_for_tests()
    });
    assert_eq!(
        opened,
        Some(PopoverKind::RepoSettingsPrompt { repo_id }),
        "the popover must be open before asserting its body"
    );
    assert!(
        cx.debug_bounds("repo_settings_popover").is_some(),
        "the prompt should render"
    );
    assert!(
        cx.debug_bounds("repo_settings_user_input").is_some()
            && cx.debug_bounds("repo_settings_email_input").is_some(),
        "both identity fields render"
    );

    // The signing override cycles inherit → on → off → inherit, one click
    // per state, all reachable from one row.
    let state_after = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_window, app| {
            view.read(app)
                .popover_host
                .read(app)
                .repo_settings
                .repo_settings_sign_commits
        })
    };
    click(cx, "repo_settings_sign_toggle");
    assert_eq!(state_after(cx), Some(true));
    click(cx, "repo_settings_sign_toggle");
    assert_eq!(state_after(cx), Some(false));
    click(cx, "repo_settings_sign_toggle");
    assert_eq!(state_after(cx), None, "three clicks return to inherit");

    // An unchanged draft applies nothing: Apply closes without touching git.
    click(cx, "repo_settings_apply");
    let closed = cx.update(|_window, app| {
        view.read(app)
            .popover_host
            .read(app)
            .popover_kind_for_tests()
            != Some(PopoverKind::RepoSettingsPrompt { repo_id })
    });
    assert!(closed, "an empty plan closes the prompt");
}

fn open_repo_settings_prompt(
    view: &gpui::Entity<WorkTreeView>,
    repo_id: RepoId,
    cx: &mut gpui::VisualTestContext,
) {
    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::RepoSettingsPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });
}

fn repo_settings_prompt_is_open(view: &gpui::Entity<WorkTreeView>, app: &gpui::App) -> bool {
    view.read(app)
        .popover_host
        .read(app)
        .popover_kind_for_tests()
        .is_some_and(|kind| matches!(kind, PopoverKind::RepoSettingsPrompt { .. }))
}

#[gpui::test]
fn repo_settings_cancel_click_closes(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("repo-settings-cancel");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, mut cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    open_repo_settings_prompt(&view, repo_id, &mut cx);
    cx.update(|_window, app| {
        assert!(
            repo_settings_prompt_is_open(&view, app),
            "the prompt must be open before cancelling"
        );
    });

    click(cx, "repo_settings_cancel");
    cx.update(|_window, app| {
        assert!(
            !repo_settings_prompt_is_open(&view, app),
            "Cancel must close the prompt"
        );
    });
}

#[gpui::test]
fn repo_settings_escape_closes(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("repo-settings-escape");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, mut cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));
    cx.update(|window, app| {
        crate::app::bind_text_input_keys_for_test(app);
        let _ = window.draw(app);
    });

    open_repo_settings_prompt(&view, repo_id, &mut cx);
    cx.update(|_window, app| {
        assert!(
            repo_settings_prompt_is_open(&view, app),
            "the prompt must be open before escaping"
        );
    });

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
        assert!(
            !repo_settings_prompt_is_open(&view, app),
            "Escape must close the prompt — its Cancel button carries an Esc hint"
        );
    });
}

#[gpui::test]
fn repo_settings_renders_never_reread_the_config_snapshot(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("repo-settings-reread");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, mut cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));
    cx.update(|window, app| {
        crate::app::bind_text_input_keys_for_test(app);
        let _ = window.draw(app);
    });

    open_repo_settings_prompt(&view, repo_id, &mut cx);
    let loads_after_open = cx.update(|_window, app| {
        view.read(app)
            .popover_host
            .read(app)
            .repo_settings_test_loads_for_tests()
    });
    assert_eq!(loads_after_open, 1, "opening reads the snapshot once");

    // Every keystroke re-renders the host; none of those renders may re-read
    // the config (a read is five git process spawns on the UI thread — the
    // input-lag regression this test guards against).
    cx.simulate_keystrokes("a");
    cx.simulate_keystrokes("b");
    for _ in 0..3 {
        cx.update(|window, app| {
            let _ = window.draw(app);
        });
    }
    let loads_after_typing = cx.update(|_window, app| {
        view.read(app)
            .popover_host
            .read(app)
            .repo_settings_test_loads_for_tests()
    });
    assert_eq!(
        loads_after_typing, loads_after_open,
        "renders between open and apply must never re-read the config snapshot"
    );
}
