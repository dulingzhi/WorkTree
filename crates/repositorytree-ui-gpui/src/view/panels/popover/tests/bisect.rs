use super::branch::{create_tracking_store, wait_until, TrackingRepo};
use super::*;
use crate::view::panels::tests::{app_state_with_repo, push_test_state};
use repositorytree_core::domain::CommitId;
use repositorytree_core::services::{BisectState, BisectVerdict};

fn bisect_menu_test_repo(repo_id: RepoId, commit_id: &CommitId) -> RepoState {
    let workdir = std::env::temp_dir().join(format!(
        "repositorytree_ui_test_{}_bisect_menu",
        std::process::id()
    ));
    let mut repo = RepoState::new_opening(repo_id, repositorytree_core::domain::RepoSpec { workdir });
    repo.log = Loadable::Ready(
        repositorytree_core::domain::LogPage {
            commits: vec![repositorytree_core::domain::Commit {
                signed: false,
                id: commit_id.clone(),
                parent_ids: repositorytree_core::domain::CommitParentIds::new(),
                summary: "Hello".into(),
                author: "Alice".into(),
                time: SystemTime::UNIX_EPOCH,
            }],
            next_cursor: None,
        }
        .into(),
    );
    repo.tags = Loadable::Ready(Arc::new(vec![]));
    repo.rebase_in_progress = Loadable::Ready(false);
    repo.sequencer_state = Loadable::Ready(repositorytree_core::services::SequencerState::None);
    repo.merge_commit_message = Loadable::Ready(None);
    repo
}

/// A mid-session snapshot: bad tip known, one good anchor, git has checked out
/// a candidate between them (`current` differs from `bad`), so the strip's mark
/// buttons are armed.
fn bisect_session_mid() -> BisectState {
    BisectState {
        original_branch: Some("main".to_string()),
        bad: Some(CommitId("badbadbadbadbad01".into())),
        good: vec![CommitId("goodgoodgoodgoo01".into())],
        skipped: Vec::new(),
        current: Some(CommitId("candidatecandid1".into())),
    }
}

/// A converged snapshot: git has landed HEAD on the first bad commit, so
/// `current == bad` — there is no candidate to mark any more.
fn bisect_session_converged() -> BisectState {
    BisectState {
        original_branch: Some("main".to_string()),
        bad: Some(CommitId("badbadbadbadbad01".into())),
        good: vec![CommitId("goodgoodgoodgoo01".into())],
        skipped: Vec::new(),
        current: Some(CommitId("badbadbadbadbad01".into())),
    }
}

fn commit_menu_entry_action(model: &ContextMenuModel, label: &str) -> ContextMenuAction {
    model
        .items
        .iter()
        .find_map(|item| match item {
            ContextMenuItem::Entry {
                label: entry_label,
                action,
                ..
            } if entry_label.as_ref() == label => Some((**action).clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected `{label}` context menu entry"))
}

#[gpui::test]
fn commit_menu_idle_offers_bisect_start_at_commit(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commit_id = CommitId("deadbeefdeadbeef".into());

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let repo = bisect_menu_test_repo(repo_id, &commit_id);
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|_window, app| {
        let model = view
            .update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::CommitMenu {
                            repo_id,
                            commit_id: commit_id.clone(),
                        },
                        cx,
                    )
                })
            })
            .expect("expected commit context menu model");

        // No session running: the only bisect entry seeds one with this commit
        // as the bad end, and nothing blocks it.
        let entry = model
            .items
            .iter()
            .find_map(|item| match item {
                ContextMenuItem::Entry {
                    label,
                    disabled,
                    ..
                } if label.as_ref() == "Start bisect here as bad…" => Some(*disabled),
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected `Start bisect here as bad…` entry"));
        assert!(!entry, "idle repo must not disable bisect start");

        let ContextMenuAction::BisectStartAt {
            repo_id: action_repo,
            bad,
            goods,
        } = commit_menu_entry_action(&model, "Start bisect here as bad…")
        else {
            panic!("expected BisectStartAt action");
        };
        assert_eq!(action_repo, repo_id);
        assert_eq!(bad.as_deref(), Some(commit_id.as_ref()));
        assert!(goods.is_empty(), "the good end is marked from another commit later");
    });
}

#[gpui::test]
fn commit_menu_during_session_marks_specific_commit(cx: &mut gpui::TestAppContext) {
    let (store, events) = AppStore::new(Arc::new(TestBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commit_id = CommitId("deadbeefdeadbeef".into());

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            let mut repo = bisect_menu_test_repo(repo_id, &commit_id);
            repo.bisect = Loadable::Ready(Some(bisect_session_mid()));
            push_test_state(this, app_state_with_repo(repo, repo_id), cx);
        });
    });

    cx.update(|_window, app| {
        let model = view
            .update(app, |this, cx| {
                this.popover_host.update(cx, |host, cx| {
                    host.context_menu_model(
                        &PopoverKind::CommitMenu {
                            repo_id,
                            commit_id: commit_id.clone(),
                        },
                        cx,
                    )
                })
            })
            .expect("expected commit context menu model");

        assert!(
            !context_menu_has_entry(&model, "Start bisect here as bad…"),
            "an active session must not offer a second start"
        );
        for (label, verdict) in [
            ("Bisect: mark this commit good", BisectVerdict::Good),
            ("Bisect: mark this commit bad", BisectVerdict::Bad),
            ("Bisect: mark this commit skip", BisectVerdict::Skip),
        ] {
            let ContextMenuAction::BisectMarkCommit {
                repo_id: action_repo,
                verdict: action_verdict,
                commit,
            } = commit_menu_entry_action(&model, label)
            else {
                panic!("expected BisectMarkCommit action for `{label}`");
            };
            assert_eq!(action_repo, repo_id);
            assert_eq!(action_verdict, verdict);
            assert_eq!(commit, commit_id.as_ref().to_string());
        }
    });
}

fn context_menu_has_entry(model: &ContextMenuModel, label: &str) -> bool {
    model.items.iter().any(|item| {
        matches!(
            item,
            ContextMenuItem::Entry {
                label: entry_label,
                ..
            } if entry_label.as_ref() == label
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

fn seed_bisect_session(
    store: &AppStore,
    repo: &TrackingRepo,
    state: BisectState,
) -> RepoId {
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    // The tracking repo keeps reporting the session on every refresh, so no
    // later load can clear it. Dispatch is async (a channel into the reducer
    // task), so wait for the session to land in the store before the caller
    // builds the view on top of the settled snapshot.
    repo.set_bisect_state(Some(state.clone()));
    store.dispatch(Msg::Internal(
        repositorytree_state::msg::InternalMsg::BisectStateLoaded {
            repo_id,
            result: Ok(Some(state)),
        },
    ));
    wait_until("seeded bisect session to land in the store", || {
        store.snapshot().repos.iter().any(|repo| {
            repo.id == repo_id && matches!(&repo.bisect, Loadable::Ready(Some(_)))
        })
    });
    repo_id
}

#[gpui::test]
fn bisect_strip_marks_candidate_and_resets(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) =
        create_tracking_store("bisect-strip-mark-reset");
    seed_bisect_session(&store, &repo, bisect_session_mid());
    let store_for_view = store.clone();
    let (_view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    for selector in [
        "bisect_strip",
        "bisect_bad_button",
        "bisect_good_button",
        "bisect_skip_button",
        "bisect_reset_button",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} in debug bounds while a bisect session is active"
        );
    }

    click_debug_selector(cx, "bisect_bad_button");
    wait_until("bad mark to reach the tracking repo", || {
        repo.actions().iter().any(|a| a == "bisect-mark:bad:none")
    });
}

#[gpui::test]
fn bisect_strip_converged_disables_marks(cx: &mut gpui::TestAppContext) {
    let (store, events, repo, _workdir) =
        create_tracking_store("bisect-strip-converged");
    seed_bisect_session(&store, &repo, bisect_session_converged());
    let store_for_view = store.clone();
    let (_view, cx) = cx
        .add_window_view(|window, cx| RepositoryTreeView::new(store_for_view, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(
        cx.debug_bounds("bisect_strip").is_some(),
        "a converged session still shows the strip (first bad commit found)"
    );

    // current == bad means no candidate is checked out: every mark button is
    // disabled, so clicking must not dispatch anything.
    for selector in ["bisect_bad_button", "bisect_good_button", "bisect_skip_button"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} in debug bounds"
        );
        click_debug_selector(cx, selector);
    }
    assert!(
        !repo.actions().iter().any(|a| a.starts_with("bisect-mark")),
        "disabled mark buttons must not dispatch bisect marks, got {:?}",
        repo.actions()
    );

    // Reset is the one button that stays live on a converged session.
    click_debug_selector(cx, "bisect_reset_button");
    wait_until("reset to reach the tracking repo", || {
        repo.actions().iter().any(|a| a == "bisect-reset")
    });
}
