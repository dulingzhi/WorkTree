use super::*;
use crate::view::panels::tests::{app_state_with_repo, push_test_state};
use worktree_core::domain::{DiffArea, DiffLine, DiffLineKind, DiffTarget};

/// The path the fixture's diff describes — what the review entry is keyed on.
fn reviewed_path() -> std::path::PathBuf {
    "src/lib.rs".into()
}

/// One working-tree file with **two** hunks, so a test can tell "the whole
/// file" apart from "the hunk under the cursor": a review that only saw the
/// first hunk would not contain the second hunk's added line.
fn file_review_fixture_repo(repo_id: RepoId) -> RepoState {
    let workdir = std::env::temp_dir().join(format!(
        "worktree_ui_test_{}_file_review",
        std::process::id()
    ));
    let mut repo = RepoState::new_opening(repo_id, worktree_core::domain::RepoSpec { workdir });
    let target = DiffTarget::WorkingTree {
        path: reviewed_path(),
        area: DiffArea::Unstaged,
    };
    repo.diff_state.diff_target = Some(target.clone());
    repo.diff_state.diff = Loadable::Ready(
        worktree_core::domain::Diff {
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
                DiffLine {
                    kind: DiffLineKind::Hunk,
                    text: "@@ -9 +9 @@".into(),
                },
                DiffLine {
                    kind: DiffLineKind::Add,
                    text: "+second_hunk".into(),
                },
            ],
        }
        .into(),
    );
    repo
}

fn popover_is_open(view: &gpui::Entity<WorkTreeView>, app: &mut gpui::App) -> bool {
    view.read_with(app, |this, cx| this.popover_host.read(cx).popover.is_some())
}

/// What the test seam recorded: how many requests were driven, and the patch
/// snapshot the latest one carried.
fn review_seams(view: &gpui::Entity<WorkTreeView>, app: &mut gpui::App) -> (usize, Option<String>) {
    view.read_with(app, |this, cx| {
        let host = this.popover_host.read(cx);
        (
            host.file_review_test_requests,
            host.file_review_test_last_patch.clone(),
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

fn one_finding() -> Vec<crate::ai_commit::ReviewFinding> {
    vec![crate::ai_commit::ReviewFinding {
        severity: crate::ai_commit::ReviewSeverity::Error,
        line: Some(3),
        title: "Drops the error".into(),
        suggestion: "Propagate it with `?`".into(),
    }]
}

#[gpui::test]
fn diff_editor_menu_offers_the_review_entry(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(51);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    let model = cx
        .update(|_window, app| {
            view.update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::DiffEditorMenu {
                            repo_id,
                            area: DiffArea::Unstaged,
                            path: Some(reviewed_path()),
                            hunk_patch: None,
                            hunks_count: 2,
                            lines_patch: None,
                            discard_lines_patch: None,
                            lines_count: 0,
                            copy_text: None,
                            copy_target: None,
                        },
                        cx,
                    )
                })
            })
        })
        .expect("expected diff editor menu model");

    // Like the hunk menu's explain entry: source availability is judged at
    // click time (an unconfigured source toasts), so the entry stays enabled.
    let (disabled, action) = model
        .items
        .iter()
        .find_map(|item| match item {
            ContextMenuItem::Entry {
                label,
                disabled,
                action,
                ..
            } if label.as_ref() == "Review this file" => Some((*disabled, (**action).clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected `Review this file` entry"));
    assert!(
        !disabled,
        "the review entry defers source checks to click time"
    );
    let ContextMenuAction::ReviewFile {
        repo_id: action_repo,
        path,
    } = action
    else {
        panic!("expected ReviewFile action");
    };
    assert_eq!(action_repo, repo_id);
    assert_eq!(path, reviewed_path());
}

#[gpui::test]
fn review_file_popover_generates_then_lands_the_findings(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(52);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_file_review(repo_id, &reviewed_path(), window, cx)
            })
        });
        assert!(opened, "a seeded file must open the review popover");
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("file_review_generating").is_some(),
        "the popover opens in its generating state"
    );
    cx.update(|_window, app| {
        assert!(popover_is_open(&view, app));
        let (requests, patch) = review_seams(&view, app);
        assert_eq!(requests, 1);
        let patch = patch.expect("the request recorded its patch snapshot");
        // The snapshot is the *file*, not the hunk: both hunks are in it.
        assert!(
            patch.contains("diff --git a/src/lib.rs b/src/lib.rs")
                && patch.contains("@@ -1 +1 @@")
                && patch.contains("+new")
                && patch.contains("@@ -9 +9 @@")
                && patch.contains("+second_hunk"),
            "the snapshot is the whole file's unified patch: {patch}"
        );
    });

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_file_review(Ok(one_finding()), cx)
            })
        })
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("file_review_generating").is_none(),
        "a landed reply retires the generating state"
    );
    assert!(
        cx.debug_bounds("file_review_findings").is_some(),
        "the findings body renders in place of the placeholder"
    );
}

#[gpui::test]
fn review_file_popover_renders_a_clean_result_as_its_own_state(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(54);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_file_review(repo_id, &reviewed_path(), window, cx)
            })
        });
        assert!(opened);
        // A file with nothing to report is a result, not an empty panel.
        view.update(app, |this, cx| {
            this.popover_host
                .update(cx, |host, cx| host.finish_file_review(Ok(Vec::new()), cx))
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("file_review_clean").is_some(),
        "a clean review says so instead of rendering an empty list"
    );
    assert!(
        cx.debug_bounds("file_review_findings").is_none(),
        "no findings list when there are no findings"
    );
}

#[gpui::test]
fn review_file_popover_shows_errors_and_retries_the_same_snapshot(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(53);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_file_review(repo_id, &reviewed_path(), window, cx)
            })
        });
        assert!(opened);
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_file_review(Err("provider unavailable".into()), cx)
            })
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("file_review_error").is_some(),
        "a failed request surfaces its error in the popover"
    );
    assert!(
        cx.debug_bounds("file_review_findings").is_none(),
        "no findings body while the request failed"
    );
    assert!(
        cx.debug_bounds("file_review_retry").is_some(),
        "the error state offers Retry"
    );

    click_debug_selector(cx, "file_review_retry");
    cx.update(|_window, app| {
        let (requests, patch) = review_seams(&view, app);
        assert_eq!(requests, 2, "retry drives a fresh request");
        assert!(
            patch
                .expect("retry resends the stored snapshot")
                .contains("+second_hunk"),
            "retry reviews the snapshot the popover was opened with"
        );
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("file_review_generating").is_some(),
        "retry returns the popover to its generating state"
    );
    assert!(
        cx.debug_bounds("file_review_error").is_none(),
        "the error is cleared while the retry runs"
    );
}

#[gpui::test]
fn review_file_popover_stop_cancels_and_drops_the_late_reply(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(52);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_file_review(repo_id, &reviewed_path(), window, cx)
            })
        });
        assert!(opened);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(cx.debug_bounds("file_review_stop").is_some());

    click_debug_selector(cx, "file_review_stop");

    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "stopping closes the review popover"
        );
        let (requests, _patch) = review_seams(&view, app);
        assert_eq!(requests, 1, "one request was sent before the cancel");
    });

    // The reply to the cancelled request is dropped, not written into a state
    // the user already walked away from.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_file_review(Ok(one_finding()), cx)
            })
        })
    });
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, _| {
                assert!(
                    host.file_review.is_none(),
                    "a cancelled review never adopts its late reply"
                );
            })
        })
    });
}
