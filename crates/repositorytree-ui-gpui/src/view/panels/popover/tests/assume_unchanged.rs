use super::*;

/// Seed a repo whose assume-unchanged list holds `paths` and open the manager
/// on it, centered like the palette entry does.
fn open_manager(
    cx: &mut gpui::TestAppContext,
    paths: Vec<std::path::PathBuf>,
) -> (Entity<RepositoryTreeView>, &mut gpui::VisualTestContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_id = RepoId(7);
    let workdir = std::env::temp_dir().join(format!(
        "repositorytree_ui_test_{}_assume_unchanged",
        std::process::id()
    ));

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let mut repo = RepoState::new_opening(
                repo_id,
                repositorytree_core::domain::RepoSpec {
                    workdir: workdir.clone(),
                },
            );
            repo.assume_unchanged = Loadable::Ready(Arc::new(paths));
            let state = Arc::new(AppState {
                repos: vec![repo],
                active_repo: Some(repo_id),
                ..Default::default()
            });
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
                host.open_popover_centered(
                    PopoverKind::AssumeUnchangedManager { repo_id },
                    window,
                    cx,
                );
            });
        });
    });
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    (view, cx)
}

#[gpui::test]
fn assume_unchanged_manager_lists_marked_paths(cx: &mut gpui::TestAppContext) {
    let (_view, cx) = open_manager(
        cx,
        vec![
            std::path::PathBuf::from("src/big.bin"),
            std::path::PathBuf::from("assets/large.png"),
        ],
    );

    assert!(
        cx.debug_bounds("assume_unchanged_manager").is_some(),
        "the manager panel should render"
    );
    assert!(
        cx.debug_bounds("assume_unchanged_row_0").is_some()
            && cx.debug_bounds("assume_unchanged_row_1").is_some(),
        "one row per marked path"
    );
    assert!(
        cx.debug_bounds("assume_unchanged_row_2").is_none(),
        "no row beyond the marked paths"
    );
}

#[gpui::test]
fn assume_unchanged_manager_empty_state_has_no_rows(cx: &mut gpui::TestAppContext) {
    let (_view, cx) = open_manager(cx, vec![]);

    assert!(
        cx.debug_bounds("assume_unchanged_manager").is_some(),
        "the manager panel should render"
    );
    assert!(
        cx.debug_bounds("assume_unchanged_row_0").is_none(),
        "an empty list shows the empty state, not rows"
    );
}
