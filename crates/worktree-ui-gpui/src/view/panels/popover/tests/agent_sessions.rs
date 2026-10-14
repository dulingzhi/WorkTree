use super::*;

use super::branch::create_tracking_store;
use crate::view::agent_workbench::{AgentKind, AgentSessionId, AgentSessionState};
use worktree_core::domain::CommitId;

fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
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

/// Open the roster with the given sessions seeded (each is `(id, worktree_path)`).
/// Seeding only inserts — it never clears other sessions already in state.
fn open_roster(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<WorkTreeView>,
    repo_id: RepoId,
    seeds: &[(u64, &str)],
) {
    cx.update(|window, app| {
        view.update(app, |this, cx| {
            let inner = this.agent_sessions.entry(repo_id).or_default();
            for (id, path) in seeds {
                inner.insert(
                    AgentSessionId(*id),
                    AgentSessionState {
                        id: AgentSessionId(*id),
                        kind: AgentKind::ClaudeCode,
                        baseline: CommitId("aaaa111122223333444455556666777788889999".into()),
                        worktree_path: std::path::PathBuf::from(path),
                        worktree_repo_id: None,
                    },
                );
            }
            this.popover_host.update(cx, |host, cx| {
                host.open_popover_at(
                    PopoverKind::AgentSessions { repo_id },
                    gpui::point(gpui::px(120.0), gpui::px(72.0)),
                    window,
                    cx,
                );
            });
        });
        let _ = window.draw(app);
    });
}

#[gpui::test]
fn agent_sessions_roster_renders_the_running_session(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("agent-roster");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, &[(1, "/repos/agent-1770000000")]);

    assert!(
        cx.debug_bounds("agent_sessions_popover").is_some(),
        "the roster should render"
    );
    assert!(
        cx.debug_bounds("agent_session_card_1").is_some(),
        "the running session's card should render"
    );
    assert!(
        cx.debug_bounds("agent_view_changes_1").is_some()
            && cx.debug_bounds("agent_stop_session_1").is_some(),
        "the card carries view-changes and stop actions"
    );
    assert!(
        cx.debug_bounds("agent_session_empty").is_none(),
        "no empty hint while a session runs"
    );
    assert!(
        cx.debug_bounds("agent_start_claude").is_some()
            && cx.debug_bounds("agent_start_codex").is_some(),
        "start entries are always offered (a second session joins the first)"
    );
}

#[gpui::test]
fn agent_sessions_roster_without_a_session_shows_the_hint(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("agent-roster-empty");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, &[]);

    assert!(
        cx.debug_bounds("agent_session_empty").is_some(),
        "without a session the roster explains what starting one does"
    );
    assert!(cx.debug_bounds("agent_session_card").is_none());
}

#[gpui::test]
fn agent_sessions_stop_ends_the_session(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("agent-roster-stop");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, &[(1, "/repos/agent-1770000000")]);

    click(cx, "agent_stop_session_1");

    let session_gone =
        cx.update(|_window, app| !view.read(app).agent_sessions.contains_key(&repo_id));
    assert!(
        session_gone,
        "stopping ends the session record along with the terminal session"
    );
    let popover_closed = cx.update(|_window, app| {
        view.read(app)
            .popover_host
            .read(app)
            .popover_kind_for_tests()
            != Some(PopoverKind::AgentSessions { repo_id })
    });
    assert!(popover_closed, "the roster closes after its action");
}

#[gpui::test]
fn agent_sessions_roster_lists_each_session(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("agent-roster-multi");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    open_roster(
        cx,
        &view,
        repo_id,
        &[(1, "/repos/agent-1"), (2, "/repos/agent-2")],
    );

    assert!(
        cx.debug_bounds("agent_session_card_1").is_some()
            && cx.debug_bounds("agent_session_card_2").is_some(),
        "every running session gets its own card"
    );

    click(cx, "agent_stop_session_1");

    let (first_gone, second_survives, still_tracked) = cx.update(|_window, app| {
        let sessions = view.read(app).agent_sessions.get(&repo_id);
        (
            sessions.map_or(true, |m| !m.contains_key(&AgentSessionId(1))),
            sessions.map_or(false, |m| m.contains_key(&AgentSessionId(2))),
            view.read(app).agent_sessions.contains_key(&repo_id),
        )
    });
    assert!(first_gone, "stopping one session removes only that record");
    assert!(second_survives, "the other session keeps running");
    assert!(still_tracked, "the repo still hosts the surviving session");
}
