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
        view.read(app).popover_host.read(app).popover_kind_for_tests()
            != Some(PopoverKind::RepoSettingsPrompt { repo_id })
    });
    assert!(closed, "an empty plan closes the prompt");

}
