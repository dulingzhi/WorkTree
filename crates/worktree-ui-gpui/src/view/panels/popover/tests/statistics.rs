use super::*;

use worktree_core::domain::ContributorCommit;

/// Seed a repo with `statistics` and open the statistics popover on it,
/// centered like the palette entry does. The seeded state also goes into the
/// store so popover dispatches reduce against it (the lazy-load test reads
/// the transition back out of the snapshot).
fn open_statistics(
    cx: &mut gpui::TestAppContext,
    statistics: Loadable<Arc<Vec<ContributorCommit>>>,
) -> (Entity<WorkTreeView>, &mut gpui::VisualTestContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));
    let repo_id = RepoId(7);
    let workdir = std::env::temp_dir().join(format!(
        "worktree_ui_test_{}_statistics",
        std::process::id()
    ));

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let mut repo = RepoState::new_opening(
                repo_id,
                worktree_core::domain::RepoSpec {
                    workdir: workdir.clone(),
                },
            );
            repo.open = Loadable::Ready(());
            repo.statistics = statistics;
            let state = Arc::new(AppState {
                repos: vec![repo],
                active_repo: Some(repo_id),
                ..Default::default()
            });
            this.store.replace_snapshot_for_test(Arc::clone(&state));
            this.state = Arc::clone(&state);
            // In the running app the host mirrors the UI model through an
            // observer; seed it directly so the panel sees the repo.
            this.popover_host.update(cx, |host, _cx| {
                host.state = Arc::clone(&state);
            });
            cx.notify();
        });
    });

    cx.update(|window, app| {
        view.update(app, |this, cx| {
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_centered(PopoverKind::Statistics { repo_id }, window, cx);
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    (view, cx)
}

/// A commit `hours_ago` hours back, so it lands inside the current week,
/// month, and year regardless of when the test runs.
fn recent_commit(author: &str, hours_ago: u64) -> ContributorCommit {
    ContributorCommit {
        author: Arc::from(author),
        time: SystemTime::now() - std::time::Duration::from_secs(hours_ago * 3600),
    }
}

fn click(cx: &mut gpui::VisualTestContext, _view: &Entity<WorkTreeView>, selector: &'static str) {
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
fn statistics_popover_renders_chart_and_contributors(cx: &mut gpui::TestAppContext) {
    let commits = vec![
        recent_commit("Alice", 1),
        recent_commit("Alice", 2),
        recent_commit("Bob", 3),
    ];
    let (_view, cx) = open_statistics(cx, Loadable::Ready(Arc::new(commits)));

    assert!(
        cx.debug_bounds("statistics_popover").is_some(),
        "the statistics panel should render"
    );
    assert!(
        cx.debug_bounds("statistics_chart_week").is_some(),
        "the popover opens on the week tab"
    );
    assert!(
        cx.debug_bounds("statistics_contributor_0").is_some()
            && cx.debug_bounds("statistics_contributor_1").is_some(),
        "one row per contributor"
    );
    assert!(
        cx.debug_bounds("statistics_contributor_2").is_none(),
        "no row beyond the contributors"
    );
}

#[gpui::test]
fn statistics_tab_click_switches_the_charted_period(cx: &mut gpui::TestAppContext) {
    let commits = vec![recent_commit("Alice", 1)];
    let (view, cx) = open_statistics(cx, Loadable::Ready(Arc::new(commits)));

    assert!(
        cx.debug_bounds("statistics_chart_week").is_some(),
        "the popover opens on the week tab"
    );

    click(cx, &view, "statistics_period_tab_year");

    assert!(
        cx.debug_bounds("statistics_chart_year").is_some(),
        "clicking the year tab re-charts the year"
    );
    assert!(
        cx.debug_bounds("statistics_chart_week").is_none(),
        "the week chart is replaced"
    );
}

#[gpui::test]
fn statistics_loading_state_hides_the_chart(cx: &mut gpui::TestAppContext) {
    let (_view, cx) = open_statistics(cx, Loadable::Loading);

    assert!(
        cx.debug_bounds("statistics_popover").is_some(),
        "the statistics panel should render"
    );
    assert!(
        cx.debug_bounds("statistics_body").is_none(),
        "the chart and rankings wait for the commit list"
    );
}

#[gpui::test]
fn opening_statistics_with_no_data_requests_the_load(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open_statistics(cx, Loadable::NotLoaded);

    // The open path dispatches `Msg::LoadRepoStatistics`; with the repo open,
    // the reducer transitions the field away from NotLoaded. TestBackend
    // cannot open repositories, so the spawned load usually fails fast and
    // lands on Error — either way, NotLoaded is the only state that means the
    // dispatch never happened.
    let statistics = view.update(cx, |this, _cx| {
        this.store
            .snapshot()
            .repos
            .iter()
            .find(|repo| repo.id == RepoId(7))
            .expect("seeded repo should stay in the snapshot")
            .statistics
            .clone()
    });
    assert!(
        matches!(statistics, Loadable::Loading | Loadable::Error(_)),
        "opening without data should start the load, got {statistics:?}"
    );
}

#[gpui::test]
fn an_empty_window_keeps_the_chart_axis(cx: &mut gpui::TestAppContext) {
    let (_view, cx) = open_statistics(cx, Loadable::Ready(Arc::new(Vec::new())));

    assert!(
        cx.debug_bounds("statistics_chart_week").is_some(),
        "the chart renders its axis even with no commits"
    );
    assert!(
        cx.debug_bounds("statistics_contributor_0").is_none(),
        "no contributor rows without commits"
    );
}
