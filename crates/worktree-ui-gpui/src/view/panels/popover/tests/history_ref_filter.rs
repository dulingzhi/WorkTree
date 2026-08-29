use super::branch::{create_tracking_store, wait_until};
use super::*;
use worktree_core::domain::{Branch, CommitId, RemoteBranch, Tag};

fn branch(name: &str) -> Branch {
    Branch {
        name: name.to_owned(),
        target: CommitId("target".into()),
        upstream: None,
        divergence: None,
    }
}

fn remote_branch(remote: &str, name: &str) -> RemoteBranch {
    RemoteBranch {
        remote: remote.to_owned(),
        name: name.to_owned(),
        target: CommitId("target".into()),
    }
}

fn tag(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        target: CommitId("target".into()),
        created_at: None,
    }
}

/// Loads the three ref lists the popover reads. The popover's fingerprint
/// repaints on these revs, so seeding before the view exists keeps the first
/// draw the one under test.
fn seed_refs(store: &AppStore) -> RepoId {
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    store.dispatch(Msg::Internal(
        worktree_state::msg::InternalMsg::BranchesLoaded {
            repo_id,
            result: Ok(vec![branch("main"), branch("dev")]),
        },
    ));
    store.dispatch(Msg::Internal(
        worktree_state::msg::InternalMsg::RemoteBranchesLoaded {
            repo_id,
            result: Ok(vec![remote_branch("origin", "main")]),
        },
    ));
    store.dispatch(Msg::Internal(
        worktree_state::msg::InternalMsg::TagsLoaded {
            repo_id,
            result: Ok(vec![tag("v1")]),
        },
    ));
    wait_until("seeded ref lists to land in the store", || {
        store.snapshot().repos.iter().any(|repo| {
            repo.id == repo_id
                && matches!(&repo.branches, worktree_state::model::Loadable::Ready(_))
                && matches!(
                    &repo.remote_branches,
                    worktree_state::model::Loadable::Ready(_)
                )
                && matches!(&repo.tags, worktree_state::model::Loadable::Ready(_))
        })
    });
    repo_id
}

fn open_ref_filter_popover(
    view: &gpui::Entity<WorkTreeView>,
    repo_id: RepoId,
    window: &mut gpui::Window,
    app: &mut gpui::App,
) {
    view.update(app, |this, cx| {
        this.popover_host.update(cx, |host, cx| {
            host.open_popover_at(
                PopoverKind::HistoryRefFilter { repo_id },
                gpui::point(gpui::px(120.0), gpui::px(72.0)),
                window,
                cx,
            );
        });
    });
}

fn popover_is_open(view: &gpui::Entity<WorkTreeView>, app: &mut gpui::App) -> bool {
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

fn history_ref_filters(store: &AppStore) -> Vec<String> {
    let snapshot = store.snapshot();
    let active = snapshot.active_repo.unwrap();
    snapshot
        .repos
        .iter()
        .find(|repo| repo.id == active)
        .map(|repo| repo.history_state.history_ref_filters.clone())
        .unwrap_or_default()
}

/// The live poller that relays store snapshots into the view is disabled under
/// the test scheduler (see `view::poller`), so tests push the snapshot by hand
/// — and every popover toggle reads the store at click time, so each click has
/// to see the previous one land first.
fn sync_and_redraw(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<WorkTreeView>) {
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            crate::view::test_support::sync_store_snapshot(this, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
}

#[gpui::test]
fn ref_filter_popover_lists_branches_remotes_and_tags(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("ref-filter-lists");
    let repo_id = seed_refs(&store);
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_ref_filter_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    for selector in [
        "history_ref_filter",
        "history_ref_filter_row_refs/heads/main",
        "history_ref_filter_row_refs/heads/dev",
        "history_ref_filter_row_refs/remotes/origin/main",
        "history_ref_filter_row_refs/tags/v1",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} in debug bounds"
        );
    }
    // Nothing is filtered yet, so there is nothing to clear.
    assert!(
        cx.debug_bounds("history_ref_filter_clear").is_none(),
        "the clear button only appears once a filter is set"
    );
}

#[gpui::test]
fn ref_filter_popover_toggles_update_the_store_and_keep_it_open(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("ref-filter-toggle");
    let repo_id = seed_refs(&store);
    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_ref_filter_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    click_debug_selector(cx, "history_ref_filter_row_refs/heads/dev");
    wait_until("the branch toggle to reach the store", || {
        history_ref_filters(&store) == vec!["refs/heads/dev".to_string()]
    });
    sync_and_redraw(cx, &view);
    cx.update(|_window, app| {
        assert!(
            popover_is_open(&view, app),
            "toggling a ref must keep the popover open for the next toggle"
        );
    });
    assert!(
        cx.debug_bounds("history_ref_filter_clear").is_some(),
        "an active filter offers Clear"
    );

    // A second toggle adds to the set rather than replacing it — the C#
    // sidebar-toggle semantics this feature mirrors.
    click_debug_selector(cx, "history_ref_filter_row_refs/tags/v1");
    wait_until("the tag toggle to reach the store", || {
        history_ref_filters(&store)
            == vec!["refs/heads/dev".to_string(), "refs/tags/v1".to_string()]
    });
    sync_and_redraw(cx, &view);

    click_debug_selector(cx, "history_ref_filter_clear");
    wait_until("clearing to reach the store", || {
        history_ref_filters(&store).is_empty()
    });
    sync_and_redraw(cx, &view);
    cx.update(|_window, app| {
        assert!(
            popover_is_open(&view, app),
            "clearing keeps the popover open too"
        );
    });
}

#[gpui::test]
fn ref_filter_popover_lists_missing_filters_for_removal(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("ref-filter-missing");
    let repo_id = seed_refs(&store);
    // A filter whose branch no longer exists — the shape a restored session
    // has after the branch was deleted elsewhere.
    store.dispatch(Msg::SetHistoryRefFilters {
        repo_id,
        refs: vec!["refs/heads/gone".to_string()],
    });
    wait_until("the restored filter to reach the store", || {
        history_ref_filters(&store) == vec!["refs/heads/gone".to_string()]
    });

    let store_for_view = store.clone();
    let (view, cx) = cx
        .add_window_view(|window, cx| WorkTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_ref_filter_popover(&view, repo_id, window, app);
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("history_ref_filter_row_refs/heads/gone")
            .is_some(),
        "a filter no listed ref explains is still shown, so it can be removed"
    );
    click_debug_selector(cx, "history_ref_filter_row_refs/heads/gone");
    wait_until("removing the stale filter to reach the store", || {
        history_ref_filters(&store).is_empty()
    });
}
