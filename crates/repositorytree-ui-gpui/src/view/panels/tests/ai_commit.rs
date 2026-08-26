use super::*;
use crate::view::panes::AiCommitGeneration;

/// The ✨ generation state machine is driven through the same
/// `apply_state_snapshot` path production uses: `push_test_state` publishes
/// into the UI model, the observer applies the snapshot, and
/// `drive_ai_commit_generation` advances the phase.
///
/// `ai_commit::current()` is a process global and tests run in parallel, so
/// every test here serializes on `ai_commit::TEST_SETTINGS_LOCK` (shared with
/// the settings window's AI section test) and restores the default after.
struct AiSettingsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl AiSettingsGuard {
    fn lock_configured() -> Self {
        let lock = crate::ai_commit::lock_test_settings();
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings {
            provider: crate::ai_commit::AiProvider::Anthropic,
            api_key: "sk-test".to_string(),
            ..Default::default()
        });
        Self { _lock: lock }
    }
}

impl Drop for AiSettingsGuard {
    fn drop(&mut self) {
        // Still holding `_lock` here (fields drop after `drop` returns), so
        // the restore and every other test's setup stay ordered.
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
    }
}

fn repo_with_staged_file(
    repo_id: repositorytree_state::model::RepoId,
    staged: bool,
) -> repositorytree_state::model::RepoState {
    let workdir = std::env::temp_dir().join(format!(
        "repositorytree_ui_test_ai_commit_{}_{}",
        std::process::id(),
        repo_id.0
    ));
    let mut repo = opening_repo_state(repo_id, &workdir);
    if staged {
        set_test_file_status(
            &mut repo,
            "a.txt",
            repositorytree_core::domain::FileStatusKind::Modified,
            repositorytree_core::domain::DiffArea::Staged,
        );
    }
    repo
}

#[gpui::test]
fn ai_generate_without_configuration_or_staged_changes_never_starts(cx: &mut gpui::TestAppContext) {
    let _settings = AiSettingsGuard::lock_configured();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_id = repositorytree_state::model::RepoId(61);

    // Configured but nothing staged: the click must refuse.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            push_test_state(
                this,
                app_state_with_repo(repo_with_staged_file(repo_id, false), repo_id),
                cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, cx| {
            pane.start_ai_commit_message_generation(cx);
            assert!(
                pane.ai_commit_generation.is_none(),
                "nothing staged means no generation"
            );
            assert_eq!(pane.ai_commit_test_generations, 0);
        });
    });

    // Unconfigured with staged changes: still no generation.
    crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings::default());
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            push_test_state(
                this,
                app_state_with_repo(repo_with_staged_file(repo_id, true), repo_id),
                cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, cx| {
            pane.start_ai_commit_message_generation(cx);
            assert!(
                pane.ai_commit_generation.is_none(),
                "an unconfigured provider means no generation"
            );
            assert_eq!(pane.ai_commit_test_generations, 0);
        });
    });
}

#[gpui::test]
fn ai_generate_fetches_context_then_runs_the_provider_request(cx: &mut gpui::TestAppContext) {
    let _settings = AiSettingsGuard::lock_configured();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_id = repositorytree_state::model::RepoId(62);

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            push_test_state(
                this,
                app_state_with_repo(repo_with_staged_file(repo_id, true), repo_id),
                cx,
            );
        });
    });
    cx.run_until_parked();

    // The click enters the fetch phase.
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, cx| {
            pane.start_ai_commit_message_generation(cx);
        });
    });
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, _cx| {
            let generation = pane
                .ai_commit_generation
                .clone()
                .expect("generation in flight");
            assert_eq!(
                generation,
                AiCommitGeneration::FetchingContext {
                    repo_id,
                    seen_rev: 0,
                },
                "expected the context fetch to be in flight"
            );
        });
    });

    // Publishing a ready context advances to the provider request with the
    // fetched payload — in test builds that means the recorded counter and
    // context rather than a network call.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let mut repo = repo_with_staged_file(repo_id, true);
            repo.ai_commit_context = repositorytree_state::model::Loadable::Ready(Arc::new(
                repositorytree_state::model::AiCommitContext {
                    diff: "diff --git a/a.txt b/a.txt".to_string(),
                    recent_subjects: vec!["feat: prior".to_string()],
                },
            ));
            repo.ai_commit_context_rev = repo.ai_commit_context_rev.wrapping_add(1);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, _cx| {
            assert_eq!(
                pane.ai_commit_test_generations, 1,
                "the ready context must hand the provider request its payload"
            );
            let context = pane
                .ai_commit_test_last_context
                .as_ref()
                .expect("generation context recorded");
            assert_eq!(context.diff, "diff --git a/a.txt b/a.txt");
            assert_eq!(context.recent_subjects, vec!["feat: prior".to_string()]);
            assert_eq!(
                pane.ai_commit_generation,
                Some(AiCommitGeneration::Generating { repo_id }),
                "the pane stays in the generating phase until a reply lands"
            );
        });
    });
}

#[gpui::test]
fn switching_repos_abandons_an_in_flight_generation(cx: &mut gpui::TestAppContext) {
    let _settings = AiSettingsGuard::lock_configured();
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));
    let repo_a = repositorytree_state::model::RepoId(63);
    let repo_b = repositorytree_state::model::RepoId(64);

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            push_test_state(
                this,
                app_state_with_repo(repo_with_staged_file(repo_a, true), repo_a),
                cx,
            );
        });
    });
    cx.run_until_parked();
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, cx| {
            pane.start_ai_commit_message_generation(cx);
            assert!(pane.ai_commit_generation.is_some());
        });
    });

    // Activate another repo: the pending generation must not survive the
    // switch, or its reply would clobber the other repo's message box.
    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let mut state = repositorytree_state::model::AppState::default();
            state.repos.push(repo_with_staged_file(repo_b, true));
            state.active_repo = Some(repo_b);
            push_test_state(this, Arc::new(state), cx);
        });
    });
    cx.update(|_window, app| {
        let pane = view.read(app).details_pane.clone();
        pane.update(app, |pane, _cx| {
            assert!(
                pane.ai_commit_generation.is_none(),
                "a generation started for another repo must be abandoned"
            );
        });
    });
}
