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
    // The switch is a process global and tests run in parallel: hold it on for
    // the duration rather than relying on the default.
    let _guard = crate::ai_commit::SettingsGuard::configured_with_file_review();
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

#[gpui::test]
fn review_file_popover_copies_the_findings_as_markdown(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(55);
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
                host.finish_file_review(Ok(one_finding()), cx)
            })
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("file_review_findings").is_some(),
        "the findings render before a copy is offered"
    );

    click_debug_selector(cx, "file_review_copy");

    let copied = cx.update(|_window, app| app.read_from_clipboard().and_then(|item| item.text()));
    let copied = copied.expect("copy put the review on the clipboard");
    assert!(
        copied.contains("src/lib.rs"),
        "the export names the reviewed file: {copied}"
    );
    assert!(
        copied.contains("Drops the error"),
        "the export carries the finding: {copied}"
    );
    assert!(
        copied.contains("Propagate it with `?`"),
        "the export carries the suggestion: {copied}"
    );
}

/// A batch run is one file at a time, and a file it cannot build a patch for is
/// recorded and skipped rather than stalling the whole change.
#[gpui::test]
fn batch_review_drives_one_file_at_a_time_and_records_failures(cx: &mut gpui::TestAppContext) {
    let _guard = crate::ai_commit::SettingsGuard::configured_with_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(61);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    // Only `src/lib.rs` has a diff in the fixture; the second path stands in
    // for a file whose patch cannot be built.
    let paths = vec![reviewed_path(), "src/without-a-diff.rs".into()];
    cx.update(|window, app| {
        let opened = view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.start_file_review_batch(repo_id, paths.clone(), window, cx)
            })
        });
        assert!(opened, "a non-empty batch opens the popover");
    });

    cx.update(|_window, app| {
        let (requests, patch) = review_batch_seams(&view, app);
        assert_eq!(requests, 1, "only the first file is asked for at a time");
        assert!(
            patch
                .expect("the first request carries a patch")
                .contains("diff --git a/src/lib.rs b/src/lib.rs"),
            "and it is the first file's"
        );
    });

    // Land the first file: the run must move on to the second by itself.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.finish_file_review_batch(Ok(one_finding()), cx);
            });
        });
    });

    cx.update(|_window, app| {
        let (requests, _) = review_batch_seams(&view, app);
        assert_eq!(
            requests, 1,
            "the unpatchable file is skipped, not requested: {requests}"
        );
        let results = batch_results(&view, app);
        assert_eq!(
            results.len(),
            2,
            "and it is still recorded, so the summary cannot overclaim"
        );
        assert!(!results[0].1, "the reviewed file succeeded");
        assert!(results[1].1, "the unpatchable one is recorded as failed");
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(
        cx.debug_bounds("file_review_batch_results").is_some(),
        "a finished run renders its per-file results"
    );
}

/// The batch's requests and the patch the last one carried.
fn review_batch_seams(
    view: &gpui::Entity<WorkTreeView>,
    app: &mut gpui::App,
) -> (usize, Option<String>) {
    view.read_with(app, |this, cx| {
        let host = this.popover_host.read(cx);
        (
            host.file_review_batch_test_requests,
            host.file_review_batch_test_last_patch.clone(),
        )
    })
}

/// One `(path, failed)` per finished file, in review order.
fn batch_results(view: &gpui::Entity<WorkTreeView>, app: &mut gpui::App) -> Vec<(String, bool)> {
    view.read_with(app, |this, cx| {
        let host = this.popover_host.read(cx);
        host.file_review_batch
            .as_ref()
            .map(|batch| {
                batch
                    .results
                    .iter()
                    .map(|(path, outcome)| {
                        let failed = matches!(
                            outcome,
                            super::file_review_batch::FileReviewOutcome::Error(_)
                        );
                        (path.to_string_lossy().replace('\\', "/"), failed)
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The palette reaches the review the right-click menu does, without the menu:
/// "this file" is the one the active diff view is showing.
#[gpui::test]
fn command_palette_review_file_reviews_the_file_in_the_diff_view(cx: &mut gpui::TestAppContext) {
    let _guard = crate::ai_commit::SettingsGuard::configured_with_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(56);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.execute_command("review-file", Some(window), cx)
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert!(
            popover_is_open(&view, app),
            "the palette command opens the review popover"
        );
        let (requests, patch) = review_seams(&view, app);
        assert_eq!(requests, 1, "the command drives the review request");
        assert!(
            patch
                .expect("the request carries its snapshot")
                .contains("diff --git a/src/lib.rs b/src/lib.rs"),
            "it reviews the file the diff view is showing"
        );
    });
}

/// The switch is offered as "off", not "on but useless": with the feature
/// switched off the menu does not carry the entry at all, so there is nothing
/// to click and nothing for a click to warn about.
#[gpui::test]
fn diff_editor_menu_drops_the_review_entry_when_the_feature_is_off(cx: &mut gpui::TestAppContext) {
    let _guard = crate::ai_commit::SettingsGuard::configured_without_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(57);
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

    let labels: Vec<&str> = model
        .items
        .iter()
        .filter_map(|item| match item {
            ContextMenuItem::Entry { label, .. } => Some(label.as_ref()),
            _ => None,
        })
        .collect();
    assert!(
        !labels.contains(&"Review this file"),
        "a switched-off feature is not offered, rather than offered dead: {labels:?}"
    );
}

/// The command still exists in the registry, so it can be invoked without the
/// palette (a test, or a future keybinding). The switch is enforced where the
/// command lands, not only where it is listed.
#[gpui::test]
fn command_palette_review_file_stays_shut_when_the_feature_is_off(cx: &mut gpui::TestAppContext) {
    let _guard = crate::ai_commit::SettingsGuard::configured_without_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(58);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.execute_command("review-file", Some(window), cx)
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "a switched-off feature opens no popover"
        );
        let (requests, patch) = review_seams(&view, app);
        assert_eq!(requests, 0, "and sends no request");
        assert!(patch.is_none(), "and captures no snapshot");
    });
}

/// With auto switched on, opening a file's diff reviews it without a menu
/// click — and only once: a redraw of the same file must not re-send.
#[gpui::test]
fn auto_review_starts_when_a_file_diff_opens_and_not_again_on_redraw(
    cx: &mut gpui::TestAppContext,
) {
    let _guard = crate::ai_commit::SettingsGuard::configured_with_auto_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(59);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert!(
            popover_is_open(&view, app),
            "auto-review opens the popover without a click"
        );
        let (requests, patch) = review_seams(&view, app);
        assert_eq!(requests, 1, "and drives exactly one request");
        assert!(
            patch
                .expect("the auto review captures a snapshot")
                .contains("diff --git a/src/lib.rs b/src/lib.rs"),
            "of the file the diff view is showing"
        );
    });

    // The same file, re-rendered: `diff_target_rev` has not moved, so the
    // review must not fire again.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    cx.update(|_window, app| {
        let (requests, _) = review_seams(&view, app);
        assert_eq!(requests, 1, "a redraw of the same file sends nothing new");
    });
}

/// The auto switch is what turns it on — not merely having a configured
/// source. Without it, opening a diff stays quiet.
#[gpui::test]
fn auto_review_stays_quiet_while_the_auto_switch_is_off(cx: &mut gpui::TestAppContext) {
    let _guard = crate::ai_commit::SettingsGuard::configured_with_file_review();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(60);
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = file_review_fixture_repo(repo_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "a configured source alone opens nothing"
        );
        let (requests, _) = review_seams(&view, app);
        assert_eq!(requests, 0, "and sends nothing");
    });
}
