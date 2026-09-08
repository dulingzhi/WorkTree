use super::util::push_diagnostic;
use super::worktrees::resync_working_tree_details_if_selected;
use crate::model::{
    AppState, ConflictFileLoadMode, DiagnosticKind, Loadable, RepoId, RepoLoadsInFlight,
};
use crate::msg::Effect;
use crate::store::repo_load_trace;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use worktree_core::domain::{
    Branch, ContributorCommit, FileStatus, FileStatusKind, RefMetadata, Remote, RemoteBranch,
    RemoteTag, RepoStatus, Submodule, Tag, UpstreamDivergence,
};
use worktree_core::error::Error;

pub(in crate::store::reducer) fn ref_metadata_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<(String, RefMetadata)>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let ref_metadata = match result {
            Ok(entries) => Loadable::Ready(entries.into_iter().collect()),
            // A backend that does not implement this will never implement it,
            // so latch an empty map rather than `Error` — callers retry on
            // `Error`, which would re-schedule a doomed load on every open.
            Err(e) if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) => {
                Loadable::Ready(FxHashMap::default())
            }
            // Deliberately no diagnostic: this data only decorates picker rows,
            // which fall back to name-only. A transient failure must not raise
            // an error banner on every picker open.
            Err(e) => Loadable::Error(e.to_string()),
        };
        repo_state.set_ref_metadata(ref_metadata);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REF_METADATA)
        {
            effects.push(Effect::LoadRefMetadata { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn submodules_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Submodule>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let submodules = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_submodules(submodules);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::SUBMODULES)
        {
            effects.push(Effect::LoadSubmodules { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn refresh_branches(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::BRANCHES)
    {
        vec![Effect::LoadBranches { repo_id }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_tags(state: &mut AppState, repo_id: RepoId) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_tags(Loadable::Loading);
    if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
        vec![Effect::LoadTags { repo_id }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_remote_tags(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_remote_tags(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REMOTE_TAGS)
    {
        vec![Effect::LoadRemoteTags { repo_id }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_ref_metadata(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_ref_metadata(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::REF_METADATA)
    {
        vec![Effect::LoadRefMetadata { repo_id }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn load_submodules(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.set_submodules(Loadable::Loading);
    if repo_state
        .loads_in_flight
        .request(RepoLoadsInFlight::SUBMODULES)
    {
        vec![Effect::LoadSubmodules { repo_id }]
    } else {
        Vec::new()
    }
}

pub(in crate::store::reducer) fn branches_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Branch>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let branches = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_branches(branches);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::BRANCHES)
        {
            effects.push(Effect::LoadBranches { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn remotes_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Remote>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let remotes = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_remotes(remotes);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTES)
        {
            effects.push(Effect::LoadRemotes { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn remote_branches_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<RemoteBranch>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let branches = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_remote_branches(branches);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTE_BRANCHES)
        {
            effects.push(Effect::LoadRemoteBranches { repo_id });
        }
    }
    effects
}

/// Condensed status-lane payload for the repo-load trace: enough of the path
/// list to recognize a wrongful staged-list clear in one reproduction without
/// flooding the log.
fn trace_status_paths(entries: &[FileStatus]) -> String {
    const MAX_PATHS: usize = 4;
    let mut names: Vec<String> = entries
        .iter()
        .take(MAX_PATHS)
        .map(|entry| entry.path.display().to_string())
        .collect();
    if entries.len() > MAX_PATHS {
        names.push(format!("+{}", entries.len() - MAX_PATHS));
    }
    format!("[{}]", names.join(", "))
}

pub(in crate::store::reducer) fn status_for_paths_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    paths: std::sync::Arc<[std::path::PathBuf]>,
    result: std::result::Result<worktree_core::services::StatusForPaths, Error>,
) -> Vec<Effect> {
    use worktree_core::services::StatusForPaths;
    let mut effects = Vec::new();
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return effects;
    };
    let fallback_error = result
        .as_ref()
        .err()
        .map(|error| error.to_string())
        .unwrap_or_else(|| "<unmergeable payload>".to_string());
    match result {
        // The targeted lane only ever merges onto a settled full snapshot;
        // anything else (or an unmergeable shape) falls back to a full scan,
        // whose landing finishes this lane's in-flight bit — no retry loop.
        Ok(StatusForPaths::Lists { unstaged, staged })
            if matches!(repo_state.status, Loadable::Ready(_)) =>
        {
            repo_load_trace::trace!(
                "status_for_paths_merge repo_id={:?} covered={:?} staged_scan={} unstaged_scan={}",
                repo_id,
                paths.iter().collect::<Vec<_>>(),
                trace_status_paths(&staged),
                trace_status_paths(&unstaged)
            );
            repo_state.patch_status_for_paths(&paths, unstaged, staged);
            repo_load_trace::trace!(
                "status_for_paths_merged repo_id={:?} staged_now={}",
                repo_id,
                repo_state
                    .staged_status_entries()
                    .map_or(0, |entries| entries.len())
            );
            resync_working_tree_details_if_selected(repo_state);
            // A change folded while the targeted scan was in flight re-runs
            // the lane coarsely: the folded burst's paths are unknown here.
            finish_status_lane_replay(
                repo_state,
                RepoLoadsInFlight::WORKTREE_STATUS,
                Effect::LoadWorktreeStatus { repo_id },
                &mut effects,
            );
            // The merge replaces a covered path's staged half too, so it also
            // finishes the staged lane. Dispatches that hold only the
            // worktree flag (the external watcher's) finish a lane they never
            // took — a no-op — while the action-completion refresh, which
            // holds both, would strand its staged flag here otherwise.
            finish_status_lane_replay(
                repo_state,
                RepoLoadsInFlight::STAGED_STATUS,
                Effect::LoadStagedStatus { repo_id },
                &mut effects,
            );
        }
        Ok(_) | Err(_) => {
            repo_load_trace::trace!(
                "status_for_paths_fallback_full_scan repo_id={:?} covered={:?} reason={}",
                repo_id,
                paths.iter().collect::<Vec<_>>(),
                fallback_error
            );
            effects.push(Effect::LoadStatus { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<RepoStatus, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(
                    &repo_state.status,
                    Loadable::Ready(prev) if prev.as_ref() == &next
                );
                repo_load_trace::trace!(
                    "status_loaded repo_id={:?} staged={} unstaged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next.staged),
                    trace_status_paths(&next.unstaged),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_status(Loadable::Ready(Arc::new(next)));
                }
                clear_resolved_conflict_context(repo_state);
            }
            Err(e) => {
                repo_load_trace::trace!("status_loaded_error repo_id={:?} error={}", repo_id, e);
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::WORKTREE_STATUS,
            Effect::LoadWorktreeStatus { repo_id },
            &mut effects,
        );
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::STAGED_STATUS,
            Effect::LoadStagedStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

pub(in crate::store::reducer) fn worktree_status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<worktree_core::domain::FileStatus>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(&repo_state.worktree_status, Loadable::Ready(prev) if prev.as_slice() == next.as_slice());
                repo_load_trace::trace!(
                    "worktree_status_loaded repo_id={:?} unstaged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_worktree_status(Loadable::Ready(next));
                }
                clear_resolved_conflict_context(repo_state);
            }
            Err(e) => {
                repo_load_trace::trace!(
                    "worktree_status_loaded_error repo_id={:?} error={}",
                    repo_id,
                    e
                );
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_worktree_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::WORKTREE_STATUS,
            Effect::LoadWorktreeStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

pub(in crate::store::reducer) fn staged_status_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<worktree_core::domain::FileStatus>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match result {
            Ok(next) => {
                let status_unchanged = matches!(&repo_state.staged_status, Loadable::Ready(prev) if prev.as_slice() == next.as_slice());
                repo_load_trace::trace!(
                    "staged_status_loaded repo_id={:?} staged={} unchanged={}",
                    repo_id,
                    trace_status_paths(&next),
                    status_unchanged
                );
                if !status_unchanged {
                    repo_state.set_staged_status(Loadable::Ready(next));
                }
            }
            Err(e) => {
                repo_load_trace::trace!(
                    "staged_status_loaded_error repo_id={:?} error={}",
                    repo_id,
                    e
                );
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_staged_status(Loadable::Error(e.to_string()));
            }
        }
        resync_working_tree_details_if_selected(repo_state);
        finish_status_lane_replay(
            repo_state,
            RepoLoadsInFlight::STAGED_STATUS,
            Effect::LoadStagedStatus { repo_id },
            &mut effects,
        );
    }
    effects
}

fn finish_status_lane_replay(
    repo_state: &mut crate::model::RepoState,
    flag: u32,
    replay_effect: Effect,
    effects: &mut Vec<Effect>,
) {
    // A pending request means a refresh was coalesced while this load was in flight — a genuine
    // external change or a just-completed action. Always replay it, even when the loaded payload
    // matches what is currently displayed: the in-flight load may have read the working tree or
    // index just *before* the change landed, so the coalesced refresh is the only chance to
    // observe it. Suppressing it on an unchanged payload (as a previous revision did) drops real
    // external changes and leaves stale entries in the uncommitted view.
    //
    // This cannot self-sustain a refresh loop: status reads are read-only (the gix backend's
    // `maybe_persist_*` helpers never rewrite `.git/index`, and worktree reads emit only ignored
    // `Access` events), so a completed status load never manufactures the filesystem event that
    // would set `pending` again.
    if repo_state.loads_in_flight.finish(flag) {
        effects.push(replay_effect);
    }
}

/// Clear conflict-file/session state when the tracked conflict path is no longer
/// present as an unresolved conflict in status.
fn clear_resolved_conflict_context(repo_state: &mut crate::model::RepoState) {
    let Some(conflict_path) = repo_state.conflict_state.conflict_file_path.as_ref() else {
        return;
    };
    let still_conflicted = repo_state.worktree_status_entries().is_none_or(|status| {
        status
            .iter()
            .any(|entry| entry.path == *conflict_path && entry.kind == FileStatusKind::Conflicted)
    });
    if still_conflicted {
        return;
    }

    repo_state.set_conflict_file_path(None);
    repo_state.set_conflict_file_load_mode(ConflictFileLoadMode::CurrentOnly);
    repo_state.set_conflict_file(Loadable::NotLoaded);
    repo_state.conflict_state.session_pending_restore = None;
    repo_state.set_conflict_session(None);
    repo_state.set_conflict_hide_resolved(false);
}

pub(in crate::store::reducer) fn head_branch_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<String, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let head_branch = match result {
            Ok(v) => {
                if v == "HEAD" {
                    if repo_state.detached_head_commit.is_none()
                        && repo_state
                            .history_state
                            .history_scope
                            .guarantees_head_visibility()
                        && let Loadable::Ready(page) = &repo_state.log
                    {
                        repo_state
                            .set_detached_head_commit(page.commits.first().map(|c| c.id.clone()));
                    }
                } else {
                    repo_state.set_detached_head_commit(None);
                }
                Loadable::Ready(v)
            }
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_head_branch(head_branch);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::HEAD_BRANCH)
        {
            effects.push(Effect::LoadHeadBranch { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn upstream_divergence_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Option<UpstreamDivergence>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let value = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.set_upstream_divergence(value);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::UPSTREAM_DIVERGENCE)
        {
            effects.push(Effect::LoadUpstreamDivergence { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn tags_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<Tag>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let tags = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) {
                    Loadable::Ready(Vec::new())
                } else if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_tags(tags);
        if repo_state.loads_in_flight.finish(RepoLoadsInFlight::TAGS) {
            effects.push(Effect::LoadTags { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn remote_tags_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<RemoteTag>, Error>,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let remote_tags = match result {
            Ok(v) => Loadable::Ready(v),
            Err(e) => {
                if matches!(e.kind(), worktree_core::error::ErrorKind::Unsupported(_)) {
                    Loadable::Ready(Vec::new())
                } else if matches!(e.kind(), worktree_core::error::ErrorKind::Cancelled) {
                    Loadable::NotLoaded
                } else {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            }
        };
        repo_state.set_remote_tags(remote_tags);
        if repo_state
            .loads_in_flight
            .finish(RepoLoadsInFlight::REMOTE_TAGS)
        {
            effects.push(Effect::LoadRemoteTags { repo_id });
        }
    }
    effects
}

pub(in crate::store::reducer) fn assume_unchanged_list_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<std::path::PathBuf>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let list = match result {
            Ok(v) => Loadable::Ready(std::sync::Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.assume_unchanged = list;
        repo_state.assume_unchanged_rev = repo_state.assume_unchanged_rev.wrapping_add(1);
        repo_state.bump_ops_rev();
    }
    Vec::new()
}

/// On-demand like the stash list: nothing asks until the statistics dialog
/// opens, and while it loads the dialog shows the same pending state every
/// other load does.
pub(in crate::store::reducer) fn load_repo_statistics(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    if !matches!(repo_state.open, Loadable::Ready(())) {
        return Vec::new();
    }
    repo_state.statistics = Loadable::Loading;
    repo_state.statistics_rev = repo_state.statistics_rev.wrapping_add(1);
    repo_state.bump_ops_rev();
    vec![Effect::LoadRepoStatistics { repo_id }]
}

pub(in crate::store::reducer) fn repo_statistics_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<Vec<ContributorCommit>, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        let statistics = match result {
            Ok(v) => Loadable::Ready(std::sync::Arc::new(v)),
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                Loadable::Error(e.to_string())
            }
        };
        repo_state.statistics = statistics;
        repo_state.statistics_rev = repo_state.statistics_rev.wrapping_add(1);
        repo_state.bump_ops_rev();
    }
    Vec::new()
}
