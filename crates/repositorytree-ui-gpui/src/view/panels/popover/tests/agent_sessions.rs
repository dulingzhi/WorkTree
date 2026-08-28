use super::*;

use super::branch::create_tracking_store;
use crate::view::agent_workbench::{AgentKind, AgentSessionState};
use repositorytree_core::domain::CommitId;

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

fn open_roster(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<RepositoryTreeView>,
    repo_id: RepoId,
    seed_session: bool,
) {
    cx.update(|window, app| {
        view.update(app, |this, cx| {
            if seed_session {
                this.agent_sessions.insert(
                    repo_id,
                    AgentSessionState {
                        kind: AgentKind::ClaudeCode,
                        baseline: CommitId("aaaa111122223333444455556666777788889999".into()),
                        worktree_path: std::path::PathBuf::from("/repos/agent-1770000000"),
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
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, true);

    assert!(
        cx.debug_bounds("agent_sessions_popover").is_some(),
        "the roster should render"
    );
    assert!(
        cx.debug_bounds("agent_session_card").is_some(),
        "the running session's card should render"
    );
    assert!(
        cx.debug_bounds("agent_view_changes").is_some()
            && cx.debug_bounds("agent_stop_session").is_some(),
        "the card carries view-changes and stop actions"
    );
    assert!(
        cx.debug_bounds("agent_session_empty").is_none(),
        "no empty hint while a session runs"
    );
    assert!(
        cx.debug_bounds("agent_start_claude").is_some()
            && cx.debug_bounds("agent_start_codex").is_some(),
        "start entries are always offered (a second session replaces the first)"
    );
}

#[gpui::test]
fn agent_sessions_roster_without_a_session_shows_the_hint(cx: &mut gpui::TestAppContext) {
    let (store, events, _repo, _workdir) = create_tracking_store("agent-roster-empty");
    let repo_id = store.snapshot().active_repo.expect("expected active repo");
    let (view, cx) =
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, false);

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
        cx.add_window_view(|window, cx| RepositoryTreeView::new(store, events, None, window, cx));

    open_roster(cx, &view, repo_id, true);

    click(cx, "agent_stop_session");

    let session_gone =
        cx.update(|_window, app| view.read(app).agent_sessions.get(&repo_id).is_none());
    assert!(
        session_gone,
        "stopping ends the session record along with the terminal session"
    );
    let popover_closed = cx.update(|_window, app| {
        view.read(app).popover_host.read(app).popover_kind_for_tests()
            != Some(PopoverKind::AgentSessions { repo_id })
    });
    assert!(popover_closed, "the roster closes after its action");
}
