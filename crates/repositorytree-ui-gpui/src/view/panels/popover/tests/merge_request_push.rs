use super::*;

use super::branch::{create_tracking_store, wait_until};

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
fn push_menu_offers_merge_request_entry(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("mr-push-menu");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let store_for_view = store.clone();
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let entry = cx.update(|_window, app| {
        let model = view
            .update(app, |this, cx| {
                this.popover_host
                    .update(cx, |host, cx| host.context_menu_model(&PopoverKind::PushPicker, cx))
            })
            .expect("expected push menu model");

        model.items.iter().find_map(|item| match item {
            ContextMenuItem::Entry { label, action, .. }
                if label.as_ref() == "Push with merge request…" =>
            {
                Some((**action).clone())
            }
            _ => None,
        })
    });
    assert!(
        matches!(
            entry,
            Some(ContextMenuAction::OpenPopover {
                kind: PopoverKind::MergeRequestPushPrompt { repo_id: rid }
            }) if rid == repo_id
        ),
        "expected the push menu's MR entry to open the merge-request prompt"
    );
}

#[gpui::test]
fn mr_push_prompt_submits_enter_carried_options(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("mr-push-submit");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let store_for_view = store.clone();
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        app.bind_keys([gpui::KeyBinding::new(
            "enter",
            crate::kit::Enter,
            Some("TextInput"),
        )]);
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::MergeRequestPushPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
                // Defaults: only remove-source is on. Flip the option rows
                // the way their click handlers would, then target main.
                host.mr_push_merge_when_pipeline_succeeds = true;
                host.mr_push_push_to_mr_branch = true;
                host.mr_push_target_input
                    .update(cx, |input, cx| input.set_text("main", cx));
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    // The whole option surface is reachable in the rendered prompt.
    for selector in [
        "mr_push_popover",
        "mr_push_target_row",
        "mr_push_pipeline_toggle",
        "mr_push_remove_source_toggle",
        "mr_push_mr_branch_toggle",
        "mr_push_go_hint",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} in the merge-request prompt"
        );
    }

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    // create / target / pipeline / remove-source / MR-branch, in that order.
    wait_until("merge-request push", || {
        repo.actions() == vec!["push-mr:true:main:true:true:true".to_string()]
    });

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(!is_open, "expected the prompt to close after submitting");
}

#[gpui::test]
fn mr_push_toggle_clicks_flip_options_without_closing(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("mr-push-toggle");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let store_for_view = store.clone();
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::MergeRequestPushPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("mr_push_popover").is_some(),
        "the merge-request prompt should render"
    );

    // Defaults on open: remove-source on, MR-branch off.
    let defaults = cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.read_with(cx, |host, _| {
                (host.mr_push_remove_source_branch, host.mr_push_push_to_mr_branch)
            })
        })
    });
    assert_eq!(defaults, (true, false));

    click(cx, "mr_push_remove_source_toggle");
    click(cx, "mr_push_mr_branch_toggle");

    let flipped = cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.read_with(cx, |host, _| {
                (host.mr_push_remove_source_branch, host.mr_push_push_to_mr_branch)
            })
        })
    });
    assert_eq!(flipped, (false, true));

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(is_open, "toggling options keeps the prompt open");
}

#[gpui::test]
fn mr_push_prompt_description_generation_lands_and_resets(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("mr-push-description");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::MergeRequestPushPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("mr_push_description_input").is_some(),
        "the description field should render under the push options"
    );
    assert!(
        cx.debug_bounds("mr_push_generate_description").is_some(),
        "the AI generate action should be offered"
    );
    assert!(
        cx.debug_bounds("mr_push_description_error").is_none(),
        "no error row before anything failed"
    );

    // The generation itself is network work; the landing seam is the test
    // surface, same contract as the AI commit path.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.mr_push_description_generating = true;
                host.finish_mr_description_generation(
                    Ok("## Changes\n- Fix the focus ring".to_string()),
                    cx,
                );
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let landed = cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.read_with(cx, |host, _| {
                (
                    host.mr_push_description_generating,
                    host.mr_push_description_error.is_none(),
                    host.mr_push_description_input
                        .read_with(cx, |input, _| input.text().to_string()),
                )
            })
        })
    });
    assert_eq!(
        landed,
        (false, true, "## Changes\n- Fix the focus ring".to_string()),
        "a successful generation fills the editable field and clears the guard"
    );

    // Reopening starts clean: the description is per-push, not sticky.
    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::MergeRequestPushPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });
    let reopened = cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.read_with(cx, |host, _| {
                host.mr_push_description_input
                    .read_with(cx, |input, _| input.text().to_string())
            })
        })
    });
    assert_eq!(reopened, "", "the description resets on reopen");
}

#[gpui::test]
fn mr_push_prompt_description_failure_lands_inline(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("mr-push-description-err");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::MergeRequestPushPrompt { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
                host.mr_push_description_generating = true;
                host.finish_mr_description_generation(
                    Err("no credentials for the source".to_string()),
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("mr_push_description_error").is_some(),
        "a failed generation shows its error inline"
    );
    let state = cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.read_with(cx, |host, _| {
                (
                    host.mr_push_description_generating,
                    host.mr_push_description_error
                        .as_ref()
                        .map(|error| error.to_string()),
                )
            })
        })
    });
    assert_eq!(
        state,
        (false, Some("no credentials for the source".to_string())),
        "the guard clears while the error stays for the retry"
    );
}
