use super::util::push_diagnostic;
use crate::model::{
    AiCommitContext, AppState, DiagnosticKind, Loadable, RepoId, RepoLoadsInFlight,
};
use crate::msg::Effect;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::domain::{
    Commit, CommitId, LogPage, RecentCommitMessage, ReflogEntry, StashEntry,
};
use worktree_core::error::Error;

pub(in crate::store::reducer) fn file_history_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    result: std::result::Result<LogPage, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.history_state.file_history_path.as_ref() == Some(&path)
    {
        repo_state.history_state.file_history = match result {
            Ok(v) => Loadable::Ready(Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
    }
    Vec::new()
}

/// Stores the best-effort author name → email map. Cosmetic data (author
/// avatars), so failures are dropped silently rather than surfaced — the
/// initials fallback is the pre-existing behavior anyway.
pub(in crate::store::reducer) fn author_emails_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<FxHashMap<String, String>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && let Ok(emails) = result
    {
        repo_state.author_emails = emails;
    }
    Vec::new()
}

pub(in crate::store::reducer) fn blame_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    source: worktree_core::domain::BlameSource,
    result: std::result::Result<Vec<worktree_core::services::BlameLine>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.history_state.blame_path.as_ref() == Some(&path)
        && repo_state.history_state.blame_source.as_ref() == Some(&source)
    {
        let retained = repo_state.history_state.retained_blame_while_loading.take();
        repo_state.history_state.blame = match result {
            // Reuse the retained allocation when the reload produced identical
            // annotations, so the view's `Arc`-identity fingerprints and the
            // memoized blame time range stay valid and nothing repaints.
            Ok(v) => Loadable::Ready(match retained {
                Some(prev) if *prev == v => prev,
                _ => Arc::new(v),
            }),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
    }
    Vec::new()
}

pub(in crate::store::reducer) fn load_stashes(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_stashes(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::STASHES)
    {
        vec![Effect::LoadStashes { repo_id, limit: 50 }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_reflog(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    repo_state.set_reflog(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REFLOG)
    {
        vec![Effect::LoadReflog {
            repo_id,
            limit: 200,
        }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_hover_commit_message(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    // Already showing or fetching this commit: hovering the same row again must
    // not re-issue the read.
    if repo_state
        .hover_commit_message
        .as_ref()
        .is_some_and(|(id, state)| *id == commit_id && !matches!(state, Loadable::Error(_)))
    {
        return Vec::new();
    }
    repo_state.set_hover_commit_message(commit_id.clone(), Loadable::Loading);
    vec![Effect::LoadHoverCommitMessage { repo_id, commit_id }]
}

pub(in crate::store::reducer) fn hover_commit_message_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    commit_id: CommitId,
    result: std::result::Result<String, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        // A result for a commit the pointer has already left is stale.
        && repo_state
            .hover_commit_message
            .as_ref()
            .is_some_and(|(id, _)| *id == commit_id)
    {
        let value = match result {
            Ok(message) => Loadable::Ready(Arc::from(message.as_str())),
            // Deliberately not a diagnostic: a hover that loses its race with a
            // background fetch is not something to tell the user about.
            Err(e) => Loadable::Error(e.to_string()),
        };
        repo_state.set_hover_commit_message(commit_id, value);
    }
    Vec::new()
}

pub(in crate::store::reducer) fn load_recent_commit_messages(
    state: &mut AppState,
    repo_id: RepoId,
    limit: usize,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(()))
        || matches!(repo_state.recent_commit_messages, Loadable::Loading)
    {
        return Vec::new();
    }
    repo_state.set_recent_commit_messages(Loadable::Loading);
    let request_rev = repo_state.recent_commit_messages_rev;
    vec![Effect::LoadRecentCommitMessages {
        repo_id,
        limit,
        request_rev,
    }]
}

pub(in crate::store::reducer) fn recent_commit_messages_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<Vec<RecentCommitMessage>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.recent_commit_messages_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_recent_commit_messages(value);
    }
    Vec::new()
}

/// Cross-history search. A new search replaces any in-flight one — the
/// request-rev guard drops the stale reply — and an empty query just clears
/// the previous results (the picker's local rows are all it needs then).
pub(in crate::store::reducer) fn search_commits(
    state: &mut AppState,
    repo_id: RepoId,
    query: String,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    let query = query.trim().to_string();
    if query.is_empty() {
        repo_state.set_commit_search(Loadable::NotLoaded);
        repo_state.commit_search_query = None;
        return Vec::new();
    }
    repo_state.set_commit_search(Loadable::Loading);
    // The trimmed query is both what the backend searches for and what the
    // picker compares its trimmed input against when deciding whether the
    // stored results answer the typed query.
    repo_state.commit_search_query = Some(query.clone());
    let request_rev = repo_state.commit_search_rev;
    vec![Effect::SearchCommits {
        repo_id,
        query,
        request_rev,
    }]
}

pub(in crate::store::reducer) fn commits_searched(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<Vec<Commit>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.commit_search_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_commit_search(value);
    }
    Vec::new()
}

/// The ✨ button's data fetch. Every click starts a fresh load — the staged
/// diff changes between clicks, so unlike `recent_commit_messages` the result
/// is never reused.
pub(in crate::store::reducer) fn load_ai_commit_context(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_ai_commit_context(Loadable::Loading);
    let request_rev = repo_state.ai_commit_context_rev;
    vec![Effect::LoadAiCommitContext {
        repo_id,
        request_rev,
    }]
}

pub(in crate::store::reducer) fn ai_commit_context_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    request_rev: u64,
    result: std::result::Result<AiCommitContext, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id)
        && repo_state.ai_commit_context_rev == request_rev
    {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_ai_commit_context(value);
    }
    Vec::new()
}

pub(in crate::store::reducer) fn load_file_history(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    limit: usize,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    repo_state.history_state.file_history_path = Some(path.clone());
    repo_state.history_state.file_history = Loadable::Loading;
    vec![Effect::LoadFileHistory {
        repo_id,
        path,
        limit,
    }]
}

pub(in crate::store::reducer) fn load_blame(
    state: &mut AppState,
    repo_id: RepoId,
    path: PathBuf,
    source: worktree_core::domain::BlameSource,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // The view dispatches this from `MainPaneView::render` against an
    // asynchronously pushed `AppState` snapshot, and `AppStore::dispatch` is a
    // channel send, so several frames can ask for the same blame before the
    // `Loading` snapshot reaches the view. Without this guard each of those
    // frames forks another `git blame --line-porcelain` for the same file.
    // `blame_path` + `blame_source` identify the request exactly, which a
    // repo-wide `RepoLoadsInFlight` bit could not.
    let same_target = repo_state.history_state.blame_path.as_ref() == Some(&path)
        && repo_state.history_state.blame_source.as_ref() == Some(&source);
    if same_target && repo_state.history_state.blame.is_loading() {
        return Vec::new();
    }
    if same_target {
        // Reloading the same file: keep the current annotations painted until
        // the new ones land.
        repo_state.retain_blame_while_loading();
    } else {
        // Re-targeting: anything held over describes a different file.
        repo_state.clear_retained_blame();
    }
    repo_state.history_state.blame_path = Some(path.clone());
    repo_state.history_state.blame_source = Some(source.clone());
    repo_state.history_state.blame = Loadable::Loading;
    vec![Effect::LoadBlame {
        repo_id,
        path,
        source,
    }]
}

pub(in crate::store::reducer) fn stashes_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<StashEntry>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let stashes = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_stashes(stashes);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::STASHES)
        {
            effects.push(Effect::LoadStashes { repo_id, limit: 50 });
        }
    }
    effects
}

pub(in crate::store::reducer) fn reflog_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<ReflogEntry>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let next = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_reflog(next);
        if repo_state.loads_in_flight.finish(RepoLoadsInFlight::REFLOG) {
            effects.push(Effect::LoadReflog {
                repo_id,
                limit: 200,
            });
        }
    }
    effects
}
