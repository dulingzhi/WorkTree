use super::branch::{create_tracking_store, wait_until};
use super::*;

#[gpui::test]
fn stash_menu_has_apply_pop_and_drop_entries(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_id = RepoId(8);
    let index = 3usize;
    let message = "WIP".to_string();

    cx.update(|_window, app| {
        let model = view
            .update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::StashMenu {
                            repo_id,
                            index,
                            message: message.clone(),
                        },
                        cx,
                    )
                })
            })
            .expect("expected stash context menu model");

        let apply_action = model.items.iter().find_map(|item| match item {
            ContextMenuItem::Entry { label, action, .. } if label.as_ref() == "Apply stash" => {
                Some((**action).clone())
            }
            _ => None,
        });
        assert!(matches!(
            apply_action,
            Some(ContextMenuAction::ApplyStash {
                repo_id: rid,
                index: ix
            }) if rid == repo_id && ix == index
        ));

        let pop_action = model.items.iter().find_map(|item| match item {
            ContextMenuItem::Entry { label, action, .. } if label.as_ref() == "Pop stash" => {
                Some((**action).clone())
            }
            _ => None,
        });
        assert!(matches!(
            pop_action,
            Some(ContextMenuAction::PopStash {
                repo_id: rid,
                index: ix
            }) if rid == repo_id && ix == index
        ));

        let drop_action = model.items.iter().find_map(|item| match item {
            ContextMenuItem::Entry { label, action, .. } if label.as_ref() == "Drop stash…" => {
                Some((**action).clone())
            }
            _ => None,
        });
        assert!(matches!(
            drop_action,
            Some(ContextMenuAction::DropStashConfirm {
                repo_id: rid,
                index: ix,
                message: ref msg
            }) if rid == repo_id && ix == index && msg == &message
        ));
    });
}

#[gpui::test]
fn stash_menu_drop_action_opens_drop_confirm_popover(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_id = RepoId(9);
    let index = 1usize;
    let message = "Drop me".to_string();

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.popover_anchor = Some(PopoverAnchor::Point(point(px(32.0), px(48.0))));
                host.context_menu_activate_action(
                    ContextMenuAction::DropStashConfirm {
                        repo_id,
                        index,
                        message: message.clone(),
                    },
                    window,
                    cx,
                );

                match host.popover.as_ref() {
                    Some(PopoverKind::StashDropConfirm {
                        repo_id: rid,
                        index: ix,
                        message: msg,
                    }) => {
                        assert_eq!(*rid, repo_id);
                        assert_eq!(*ix, index);
                        assert_eq!(msg, &message);
                    }
                    _ => panic!("expected stash drop confirm popover to open"),
                }
            });
        });
    });
}

#[gpui::test]
fn stash_prompt_escape_cancels(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("stash-escape");
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        crate::app::bind_text_input_keys_for_test(app);
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.set_active_context_menu_invoker(Some("stash_btn".into()), cx);
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt { paths: Vec::new() },
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
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, Some("stash_btn"));
    });

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(!is_open, "expected Escape to close stash popover");
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, None);
    });
    cx.update(|window, app| {
        let root = view.read(app);
        let main_focus = root
            .popover_host
            .read(app)
            .main_pane
            .read(app)
            .diff_panel_focus_handle
            .clone();
        assert!(
            main_focus.is_focused(window),
            "expected Escape to move focus away from the Stash button"
        );
    });
    assert!(
        repo.actions().is_empty(),
        "expected Escape to cancel without creating a stash"
    );
}

#[gpui::test]
fn stash_prompt_renders_shortcut_hints_and_separators(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("stash-shortcuts");
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt { paths: Vec::new() },
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

    cx.debug_bounds("stash_cancel_hint")
        .expect("expected stash Cancel shortcut hint");
    cx.debug_bounds("stash_go_hint")
        .expect("expected stash Create shortcut hint");
    cx.debug_bounds("stash_cancel_end_slot_separator")
        .expect("expected stash Cancel shortcut separator");
    cx.debug_bounds("stash_go_end_slot_separator")
        .expect("expected stash Create shortcut separator");
}

#[gpui::test]
fn stash_prompt_enter_stashes_and_closes(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("stash-enter");
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

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
            this.set_active_context_menu_invoker(Some("stash_btn".into()), cx);
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt { paths: Vec::new() },
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
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, Some("stash_btn"));
    });

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.stash_message_input
                    .update(cx, |input, cx| input.set_text("wip", cx));
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(!is_open, "expected Enter to close stash popover");
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, None);
    });

    wait_until("stash action", || {
        repo.actions() == vec!["stash:wip:true:false:0".to_string()]
    });
}

#[gpui::test]
fn stash_prompt_enter_with_empty_input_does_not_close_or_stash(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("stash-empty-enter");
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

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
            this.set_active_context_menu_invoker(Some("stash_btn".into()), cx);
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt { paths: Vec::new() },
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
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, Some("stash_btn"));
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(
        is_open,
        "expected Enter to respect the disabled Stash action when the message is empty"
    );
    cx.update(|_window, app| {
        let active_invoker = view
            .read(app)
            .active_context_menu_invoker
            .as_ref()
            .map(|id| id.as_ref());
        assert_eq!(active_invoker, Some("stash_btn"));
    });

    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        repo.actions().is_empty(),
        "expected empty input to avoid stash actions"
    );
}

#[gpui::test]
fn stash_prompt_with_paths_renders_count_and_option_toggles(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("stash-paths-render");
    let store_for_view = store.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        RepositoryTreeView::new(store_for_view, events, None, window, cx)
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt {
                        paths: vec![
                            std::path::PathBuf::from("src/a.rs"),
                            std::path::PathBuf::from("src/b.rs"),
                        ],
                    },
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

    cx.debug_bounds("stash_paths_selected")
        .expect("expected selected-paths row in stash prompt");
    cx.debug_bounds("stash_include_untracked_toggle")
        .expect("expected include-untracked toggle in stash prompt");
    cx.debug_bounds("stash_keep_index_toggle")
        .expect("expected keep-index toggle in stash prompt");

    // The full-worktree prompt shows no paths row.
    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::StashPrompt { paths: Vec::new() },
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
        cx.debug_bounds("stash_paths_selected").is_none(),
        "full-worktree stash prompt must not show the paths row"
    );
}

#[gpui::test]
fn stash_prompt_submit_carries_options_and_paths(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("stash-options-submit");
    let store_for_view = store.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        RepositoryTreeView::new(store_for_view, events, None, window, cx)
    });

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
                    PopoverKind::StashPrompt {
                        paths: vec![
                            std::path::PathBuf::from("src/a.rs"),
                            std::path::PathBuf::from("src/b.rs"),
                        ],
                    },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
                // The option rows are click-driven; the submit path reads
                // these fields, so flip them the way the click handler would.
                host.stash_include_untracked = false;
                host.stash_keep_index = true;
                host.stash_message_input
                    .update(cx, |input, cx| input.set_text("partial", cx));
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    wait_until("stash action with options", || {
        repo.actions() == vec!["stash:partial:false:true:2".to_string()]
    });
}

#[gpui::test]
fn stash_menu_branch_entry_opens_prefilled_prompt_and_submits(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("stash-branch-prompt");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let store_for_view = store.clone();
    let (view, cx) = cx.add_window_view(|window, cx| {
        RepositoryTreeView::new(store_for_view, events, None, window, cx)
    });

    cx.update(|window, app| {
        app.bind_keys([gpui::KeyBinding::new(
            "enter",
            crate::kit::Enter,
            Some("TextInput"),
        )]);
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        let model = view
            .update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::StashMenu {
                            repo_id,
                            index: 3,
                            message: "WIP".to_string(),
                        },
                        cx,
                    )
                })
            })
            .expect("expected stash context menu model");

        let branch_action = model.items.iter().find_map(|item| match item {
            ContextMenuItem::Entry { label, action, .. } if label.as_ref() == "Branch stash…" => {
                Some((**action).clone())
            }
            _ => None,
        });
        assert!(
            matches!(
                branch_action,
                Some(ContextMenuAction::OpenPopover {
                    kind: PopoverKind::StashBranchPrompt { repo_id: rid, index: 3 }
                }) if rid == repo_id
            ),
            "expected Branch stash… entry to open the stash-branch prompt"
        );
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_centered(
                    PopoverKind::StashBranchPrompt {
                        repo_id,
                        index: 3,
                    },
                    window,
                    cx,
                );
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    // The prompt opens with a suggested branch name derived from the index.
    cx.update(|_window, app| {
        let prefilled = view.update(app, |this, cx| {
            this.popover_host
                .read_with(cx, |host, _| host.create_branch_input.read(cx).text().to_string())
        });
        assert_eq!(prefilled, "stash-3");
    });

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.create_branch_input
                    .update(cx, |input, cx| input.set_text("recover-wip", cx));
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    let is_open = cx.update(|_window, app| view.read(app).popover_host.read(app).is_open());
    assert!(!is_open, "expected Enter to close the stash-branch prompt");

    wait_until("stash branch action", || {
        repo.actions() == vec!["stash-branch:recover-wip:3".to_string()]
    });
}
