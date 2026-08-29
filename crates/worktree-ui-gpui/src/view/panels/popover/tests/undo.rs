use super::branch::{create_tracking_store, wait_until, TrackingRepo};
use super::*;
use worktree_core::domain::{CommitId, ReflogEntry};
use worktree_state::model::Loadable;

fn entry(index: usize, sha: &str, message: &str) -> ReflogEntry {
    ReflogEntry {
        index,
        new_id: CommitId(sha.into()),
        message: message.into(),
        time: None,
        selector: format!("HEAD@{{{index}}}").into(),
        author: "Jane Doe".into(),
    }
}

/// A reflog whose newest entry is a completed merge — the canonical
/// "undo the merge I just ran" case, defaulting to `--mixed`.
fn merged_reflog() -> Vec<ReflogEntry> {
    vec![
        entry(0, "feedfacedeadbeef", "merge main: Fast-forward"),
        entry(1, "abcdef0123456789", "commit: base"),
    ]
}

/// Seeds the store's reflog directly (the `ReflogLoaded` path) while the
/// tracking repo keeps serving the same entries on every later load, so no
/// refresh can swap the dialog out from under the test. Mirrors the bisect
/// session seeding: dispatch is async, so wait for the snapshot before the
/// caller builds a view on top of it.
fn seed_reflog(store: &AppStore, repo: &TrackingRepo, entries: Vec<ReflogEntry>) -> RepoId {
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    repo.set_reflog(entries.clone());
    store.dispatch(Msg::Internal(
        worktree_state::msg::InternalMsg::ReflogLoaded {
            repo_id,
            result: Ok(entries),
        },
    ));
    wait_until("seeded reflog to land in the store", || {
        store.snapshot().repos.iter().any(|repo| {
            repo.id == repo_id && matches!(&repo.reflog, Loadable::Ready(_))
        })
    });
    repo_id
}

fn open_undo_popover(
    view: &gpui::Entity<WorkTreeView>,
    repo_id: RepoId,
    window: &mut gpui::Window,
    app: &mut gpui::App,
) {
    view.update(app, |this, cx| {
        this.popover_host.update(cx, |host, cx| {
            host.open_popover_at(
                PopoverKind::UndoLastActionPrompt { repo_id },
                gpui::point(gpui::px(120.0), gpui::px(72.0)),
                window,
                cx,
            );
        });
    });
}

fn popover_is_open(
    view: &gpui::Entity<WorkTreeView>,
    app: &mut gpui::App,
) -> bool {
    view.read_with(app, |this, cx| this.popover_host.read(cx).popover.is_some())
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
fn undo_popover_resets_a_completed_merge_in_default_mixed_mode(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("undo-reset-default");
    let repo_id = seed_reflog(&store, &repo, merged_reflog());
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        open_undo_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    // The reset-back dialog renders: mode chips (mixed preselected, per the
    // merge plan) and a Go button. No abort button — nothing is in flight.
    for selector in ["undo_mode_chip_soft", "undo_mode_chip_mixed", "undo_mode_chip_hard", "undo_go"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} in debug bounds for a completed merge"
        );
    }
    assert!(
        cx.debug_bounds("undo_abort_merge_go").is_none(),
        "a completed merge must not offer an abort"
    );

    // Go without touching the chips: the plan's default mode (mixed) is used.
    click_debug_selector(cx, "undo_go");
    wait_until("reset to reach the tracking repo", || {
        repo.actions().iter().any(|a| a == "reset:Mixed:abcdef0123456789")
    });
    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "confirming the reset should close the popover"
        );
    });
}

#[gpui::test]
fn undo_popover_mode_chip_switches_to_hard(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("undo-mode-chip");
    let repo_id = seed_reflog(&store, &repo, merged_reflog());
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        open_undo_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    click_debug_selector(cx, "undo_mode_chip_hard");
    click_debug_selector(cx, "undo_go");
    wait_until("hard reset to reach the tracking repo", || {
        repo.actions().iter().any(|a| a == "reset:Hard:abcdef0123456789")
    });
}

#[gpui::test]
fn undo_popover_aborts_a_merge_waiting_to_conclude(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("undo-abort-merge");
    let repo_id = seed_reflog(&store, &repo, merged_reflog());
    // A merge commit message pending in the store means git is mid-merge:
    // that wins over any reflog classification.
    store.dispatch(Msg::Internal(
        worktree_state::msg::InternalMsg::MergeCommitMessageLoaded {
            repo_id,
            result: Ok(Some("Merge branch 'topic'".to_string())),
        },
    ));
    wait_until("pending merge message to land in the store", || {
        store.snapshot().repos.iter().any(|repo| {
            repo.id == repo_id
                && matches!(&repo.merge_commit_message, Loadable::Ready(Some(_)))
        })
    });

    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        open_undo_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("undo_abort_merge_go").is_some(),
        "a pending merge should offer abort"
    );
    assert!(
        cx.debug_bounds("undo_go").is_none(),
        "a pending merge must not offer a reset"
    );

    click_debug_selector(cx, "undo_abort_merge_go");
    wait_until("merge abort to reach the tracking repo", || {
        repo.actions().iter().any(|a| a == "merge-abort")
    });
    cx.update(|_window, app| {
        assert!(
            !popover_is_open(&view, app),
            "confirming the abort should close the popover"
        );
    });
}

#[gpui::test]
fn undo_popover_with_nothing_undoable_shows_the_notice(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) = create_tracking_store("undo-nothing");
    // One entry: below the two-entry minimum every classification needs.
    let repo_id = seed_reflog(&store, &repo, vec![entry(0, "abcdef0123456789", "commit: only")]);
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        open_undo_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("undo_nothing_ok").is_some(),
        "an undoable-less reflog should show the notice dialog"
    );
    for selector in ["undo_go", "undo_mode_chip_mixed", "undo_abort_merge_go"] {
        assert!(
            cx.debug_bounds(selector).is_none(),
            "expected no {selector} when there is nothing to undo"
        );
    }
}
