use super::*;
use crate::view::panels::tests::{app_state_with_repo, push_test_state};
use repositorytree_core::domain::{DiffArea, DiffLine, DiffLineKind, DiffTarget};

/// One working-tree file with a single hunk: file headers at 0..2, the hunk
/// header at src_ix 3, one removed and one added line below it — the same
/// shape the hunk-menu tests in `panels::tests::shortcuts` seed.
fn hunk_explanation_fixture_repo(repo_id: RepoId) -> RepoState {
    let workdir = std::env::temp_dir().join(format!(
        "repositorytree_ui_test_{}_hunk_explain",
        std::process::id()
    ));
    let mut repo =
        RepoState::new_opening(repo_id, repositorytree_core::domain::RepoSpec { workdir });
    let target = DiffTarget::WorkingTree {
        path: "src/lib.rs".into(),
        area: DiffArea::Unstaged,
    };
    repo.diff_state.diff_target = Some(target.clone());
    repo.diff_state.diff = Loadable::Ready(
        repositorytree_core::domain::Diff {
            target,
            lines: vec![
                DiffLine {
                    kind: DiffLineKind::Header,
                    text: "diff --git a/src/lib.rs b/src/lib.rs".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Header,
                    text: "--- a/src/lib.rs".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Header,
                    text: "+++ b/src/lib.rs".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Hunk,
                    text: "@@ -1 +1 @@".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Remove,
                    text: "-old".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Add,
                    text: "+new".into(),
                },
            ],
        }
        .into(),
    );
    repo
}

fn popover_is_open(view: &gpui::Entity<RepositoryTreeView>, app: &mut gpui::App) -> bool {
    view.read_with(app, |this, cx| this.popover_host.read(cx).popover.is_some())
}

/// What the test seam recorded: how many requests were driven, and the patch
/// snapshot the latest one carried.
fn explanation_seams(
    view: &gpui::Entity<RepositoryTreeView>,
    app: &mut gpui::App,
) -> (usize, Option<String>) {
    view.read_with(app, |this, cx| {
        let host = this.popover_host.read(cx);
        (
            host.hunk_explanation_test_requests,
            host.hunk_explanation_test_last_patch.clone(),
        )
    })
}

fn click_debug_selector(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let center = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("expected {selector} in debug bounds"))
        .center();
    cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
    cx.simulate_mouse_down(center, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(center, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

#[gpui::test]
fn hunk_menu_offers_explain_entry(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(41);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = hunk_explanation_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    let model = cx
        .update(|_window, app| {
            view.update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::DiffHunkMenu { repo_id, src_ix: 3 },
                        cx,
                    )
                })
            })
        })
        .expect("expected hunk context menu model");

    // The entry rides every hunk menu; source availability is judged at click
    // time (unconfigured sources toast), so the entry itself stays enabled.
    let (disabled, action) = model
        .items
        .iter()
        .find_map(|item| match item {
            ContextMenuItem::Entry {
                label,
                disabled,
                action,
                ..
            } if label.as_ref() == "Explain this change" => {
                Some((*disabled, (**action).clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected `Explain this change` entry"));
    assert!(
        !disabled,
        "the explain entry defers source checks to click time"
    );
    let ContextMenuAction::ExplainHunk {
        repo_id: action_repo,
        src_ix,
    } = action
    else {
        panic!("expected ExplainHunk action");
    };
    assert_eq!(action_repo, repo_id);
    assert_eq!(src_ix, 3);
}

#[gpui::test]
fn explain_hunk_popover_generates_then_lands_the_reply(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(42);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = hunk_explanation_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_hunk_explanation(repo_id, 3, window, cx)
            })
        });
        assert!(opened, "a seeded hunk must open the explanation popover");
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    // Generating state is on screen, and the request carries the hunk's patch
    // snapshot — the lines of the hunk, not a pointer at the live diff.
    assert!(
        cx.debug_bounds("hunk_explanation_generating").is_some(),
        "the popover opens in its generating state"
    );
    cx.update(|_window, app| {
        assert!(popover_is_open(&view, app));
        let (requests, patch) = explanation_seams(&view, app);
        assert_eq!(requests, 1);
        let patch = patch.expect("the request recorded its patch snapshot");
        assert!(
            patch.contains("@@ -1 +1 @@") && patch.contains("-old") && patch.contains("+new"),
            "the snapshot is the hunk's unified patch: {patch}"
        );
    });

    // The reply replaces the generating state with the explanation body.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_hunk_explanation(
                    Ok("Renames the helper.\n\nPrepares the retry path.".into()),
                    cx,
                )
            })
        })
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("hunk_explanation_generating").is_none(),
        "a landed reply retires the generating state"
    );
    assert!(
        cx.debug_bounds("hunk_explanation_text").is_some(),
        "the explanation body renders in place of the placeholder"
    );
}

#[gpui::test]
fn explain_hunk_popover_shows_errors_and_retries_the_same_snapshot(
    cx: &mut gpui::TestAppContext,
) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(43);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = hunk_explanation_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_hunk_explanation(repo_id, 3, window, cx)
            })
        });
        assert!(opened);
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_hunk_explanation(Err("provider unavailable".into()), cx)
            })
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("hunk_explanation_error").is_some(),
        "a failed request surfaces its error in the popover"
    );
    assert!(
        cx.debug_bounds("hunk_explanation_text").is_none(),
        "no explanation body while the request failed"
    );
    assert!(
        cx.debug_bounds("hunk_explanation_retry").is_some(),
        "the error state offers Retry"
    );

    // Retry drives a second request for the stored snapshot.
    click_debug_selector(cx, "hunk_explanation_retry");
    cx.update(|_window, app| {
        let (requests, patch) = explanation_seams(&view, app);
        assert_eq!(requests, 2, "retry drives a fresh request");
        assert!(
            patch
                .expect("retry resends the stored snapshot")
                .contains("-old"),
            "retry explains the snapshot the popover was opened with"
        );
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("hunk_explanation_generating").is_some(),
        "retry returns the popover to its generating state"
    );
    assert!(
        cx.debug_bounds("hunk_explanation_error").is_none(),
        "the error is cleared while the retry runs"
    );
}

#[gpui::test]
fn explain_hunk_popover_stop_cancels_and_drops_the_late_reply(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(42);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = hunk_explanation_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_hunk_explanation(repo_id, 3, window, cx)
            })
        });
        assert!(opened);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(cx.debug_bounds("hunk_explanation_stop").is_some());

    click_debug_selector(cx, "hunk_explanation_stop");

    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "stopping closes the explanation popover"
        );
        let (requests, _patch) = explanation_seams(&view, app);
        assert_eq!(requests, 1, "one request was sent before the cancel");
    });

    // The reply to the cancelled request is dropped, not written into a state
    // the user already walked away from.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_hunk_explanation(Ok("late answer".into()), cx)
            })
        })
    });
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, _| {
                assert!(
                    host.hunk_explanation.is_none(),
                    "a cancelled explanation never adopts its late reply"
                );
            })
        })
    });
}
