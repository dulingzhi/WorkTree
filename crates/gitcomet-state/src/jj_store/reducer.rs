//! The jj store's reducer: pure state transitions producing effects.

use std::sync::atomic::AtomicU64;

use gitcomet_core::domain::RepoSpec;
use rustc_hash::FxHashMap;

use crate::model::RepoId;
use crate::msg::RepoWatchDegradedReason;

use super::model::{JJ_LOG_PAGE_SIZE, JjAppState};
use super::msg::{JjEffect, JjMsg, JjMutation};

/// How many operations the op-log panel shows.
const OP_LOG_LIMIT: usize = 50;

/// Reduce one message. `repos` is the worker loop's repository table (the
/// reducer is where the git store mutates it too); `id_alloc` allocates
/// repo ids for newly opened repos.
pub(crate) fn reduce(
    repos: &mut FxHashMap<RepoId, std::sync::Arc<dyn gitcomet_jj_core::JjRepository>>,
    id_alloc: &AtomicU64,
    state: &mut JjAppState,
    msg: JjMsg,
) -> Vec<JjEffect> {
    match msg {
        JjMsg::OpenRepo { workdir } => {
            // Re-opening an already-open workdir just activates it.
            if let Some(existing) = state.repos.iter().find(|repo| repo.spec.workdir == workdir) {
                let repo_id = existing.id;
                state.active_repo = Some(repo_id);
                return Vec::new();
            }
            let repo_id = RepoId(id_alloc.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            state.repos.push(super::model::JjRepoState::new(
                repo_id,
                RepoSpec {
                    workdir: workdir.clone(),
                },
            ));
            state.active_repo = Some(repo_id);
            vec![JjEffect::OpenRepo { repo_id, workdir }]
        }
        JjMsg::CloseRepo { repo_id } => {
            repos.remove(&repo_id);
            state.repos.retain(|repo| repo.id != repo_id);
            if state.active_repo == Some(repo_id) {
                state.active_repo = state.repos.first().map(|repo| repo.id);
            }
            Vec::new()
        }
        JjMsg::SetActiveRepo { repo_id } => {
            if state.repos.iter().any(|repo| repo.id == repo_id) {
                state.active_repo = Some(repo_id);
            }
            Vec::new()
        }

        JjMsg::RepoOpened { repo_id, repo } => {
            repos.insert(repo_id, repo);
            let Some(repo_state) = state.repo_mut(repo_id) else {
                repos.remove(&repo_id);
                return Vec::new();
            };
            repo_state.open_error = None;
            let epoch = repo_state.refresh_epoch;
            repo_state.log_loading = true;
            repo_state.bookmarks_loading = true;
            repo_state.ops_loading = true;
            vec![
                JjEffect::LoadLogPage {
                    repo_id,
                    epoch,
                    revset: repo_state.log_revset.clone(),
                    skip: 0,
                    limit: JJ_LOG_PAGE_SIZE,
                },
                JjEffect::LoadBookmarks { repo_id, epoch },
                JjEffect::LoadOpLog {
                    repo_id,
                    epoch,
                    limit: OP_LOG_LIMIT,
                },
                JjEffect::LoadConflicts { repo_id, epoch },
            ]
        }
        JjMsg::RepoOpenFailed { repo_id, error } => {
            repos.remove(&repo_id);
            if let Some(repo_state) = state.repo_mut(repo_id) {
                repo_state.open_error = Some(error);
            }
            Vec::new()
        }

        JjMsg::RefreshRepo { repo_id } => refresh(state, repo_id),

        JjMsg::LogPageLoaded {
            repo_id,
            epoch,
            skip,
            page,
        } => {
            let Some(repo_state) = state.repo_mut(repo_id) else {
                return Vec::new();
            };
            if epoch != repo_state.refresh_epoch {
                // A newer refresh superseded this load.
                return Vec::new();
            }
            repo_state.log_loading = false;
            repo_state.log_error = None;
            let page = *page;
            if skip == 0 {
                repo_state.changes = page.changes;
            } else {
                repo_state.changes.extend_from_slice(&page.changes);
            }
            repo_state.next_cursor = page.next_cursor;
            if let Some(working_copy) = repo_state.changes.iter().find(|c| c.is_working_copy) {
                let working_copy = working_copy.clone();
                repo_state.working_copy = Some(working_copy);
            }
            Vec::new()
        }
        JjMsg::BookmarksLoaded {
            repo_id,
            epoch,
            bookmarks,
        } => {
            let Some(repo_state) = state.repo_mut(repo_id) else {
                return Vec::new();
            };
            if epoch == repo_state.refresh_epoch {
                repo_state.bookmarks_loading = false;
                repo_state.bookmarks = bookmarks;
            }
            Vec::new()
        }
        JjMsg::OpLogLoaded {
            repo_id,
            epoch,
            ops,
        } => {
            let Some(repo_state) = state.repo_mut(repo_id) else {
                return Vec::new();
            };
            if epoch == repo_state.refresh_epoch {
                repo_state.ops_loading = false;
                repo_state.ops = ops;
            }
            Vec::new()
        }
        JjMsg::ConflictsLoaded {
            repo_id,
            epoch,
            conflicts,
        } => {
            let Some(repo_state) = state.repo_mut(repo_id) else {
                return Vec::new();
            };
            if epoch == repo_state.refresh_epoch {
                repo_state.conflicts = conflicts;
            }
            Vec::new()
        }
        JjMsg::LoadFailed {
            repo_id,
            epoch,
            what,
            error,
        } => {
            let Some(repo_state) = state.repo_mut(repo_id) else {
                return Vec::new();
            };
            if epoch != repo_state.refresh_epoch {
                return Vec::new();
            }
            match what {
                "log" => {
                    repo_state.log_loading = false;
                    repo_state.log_error = Some(error.clone());
                }
                "bookmarks" => repo_state.bookmarks_loading = false,
                "op log" => repo_state.ops_loading = false,
                _ => {}
            }
            repo_state.load_error = Some((what, error));
            Vec::new()
        }

        JjMsg::LoadMoreLog { repo_id } => {
            let Some(repo_state) = state.repo(repo_id) else {
                return Vec::new();
            };
            if repo_state.log_loading {
                return Vec::new();
            }
            let Some(skip) = repo_state.next_cursor else {
                return Vec::new();
            };
            vec![JjEffect::LoadLogPage {
                repo_id,
                epoch: repo_state.refresh_epoch,
                revset: repo_state.log_revset.clone(),
                skip,
                limit: JJ_LOG_PAGE_SIZE,
            }]
        }
        JjMsg::SetRevset { repo_id, revset } => {
            // A new revset invalidates the whole list; `refresh` bumps the
            // epoch so in-flight pages for the old revset are dropped, and
            // reloads from the top under the new revset.
            if let Some(repo_state) = state.repo_mut(repo_id) {
                repo_state.log_revset = revset.trim().to_string();
            }
            refresh(state, repo_id)
        }

        JjMsg::DescribeChange {
            repo_id,
            change,
            message,
        } => run_mutation(state, repo_id, "describe", move || JjMutation::Describe {
            change,
            message,
        }),
        JjMsg::NewChange { repo_id, message } => {
            run_mutation(state, repo_id, "new change", || JjMutation::New { message })
        }
        JjMsg::AbandonChange { repo_id, change } => {
            run_mutation(state, repo_id, "abandon", || JjMutation::Abandon { change })
        }
        JjMsg::SquashChange {
            repo_id,
            from,
            into,
        } => run_mutation(state, repo_id, "squash", || JjMutation::Squash {
            from,
            into,
        }),
        JjMsg::BookmarkCreate {
            repo_id,
            name,
            target,
        } => run_mutation(state, repo_id, "bookmark create", || {
            JjMutation::BookmarkCreate { name, target }
        }),
        JjMsg::BookmarkDelete { repo_id, name } => {
            run_mutation(state, repo_id, "bookmark delete", || {
                JjMutation::BookmarkDelete { name }
            })
        }
        JjMsg::BookmarkRename {
            repo_id,
            old_name,
            new_name,
        } => run_mutation(state, repo_id, "bookmark rename", || {
            JjMutation::BookmarkRename { old_name, new_name }
        }),
        JjMsg::BookmarkTrack {
            repo_id,
            name,
            remote,
        } => run_mutation(state, repo_id, "bookmark track", || {
            JjMutation::BookmarkTrack { name, remote }
        }),
        JjMsg::OpUndo { repo_id } => {
            run_mutation(state, repo_id, "undo operation", || JjMutation::OpUndo)
        }
        JjMsg::OpRestore { repo_id, op_id } => {
            run_mutation(state, repo_id, "restore operation", || {
                JjMutation::OpRestore { op_id }
            })
        }
        JjMsg::FetchAll { repo_id } => {
            run_mutation(state, repo_id, "fetch", || JjMutation::FetchAll)
        }
        JjMsg::Push { repo_id } => run_mutation(state, repo_id, "push", || JjMutation::Push),
        JjMsg::Snapshot { repo_id } => {
            run_mutation(state, repo_id, "snapshot", || JjMutation::Snapshot)
        }

        JjMsg::CommandFinished {
            repo_id,
            // Which operation finished does not matter: mutations are
            // serialized per repo, so any completion clears the marker.
            operation: _,
            output,
        } => {
            let mut effects = Vec::new();
            if let Some(repo_state) = state.repo_mut(repo_id) {
                repo_state.pending_command = None;
                repo_state.last_command_error = None;
                if output.is_some() {
                    repo_state.last_network_output = output;
                }
            }
            // Every mutation rewrites repo state (jj records an operation
            // per command); refresh so the panels reflect it.
            effects.extend(refresh(state, repo_id));
            effects
        }
        JjMsg::CommandFailed {
            repo_id,
            operation,
            error,
        } => {
            if let Some(repo_state) = state.repo_mut(repo_id) {
                repo_state.pending_command = None;
                repo_state.last_command_error = Some((operation.to_string(), error));
            }
            Vec::new()
        }

        JjMsg::RepoExternallyChanged { repo_id, .. } => {
            // Any external touch (another jj or git process wrote to the
            // colocated repo) refreshes everything: the watcher is
            // debounced upstream, so bursts collapse into one refresh.
            refresh(state, repo_id)
        }
        JjMsg::WatchDegraded { repo_id, reason } => {
            if let Some(repo_state) = state.repo_mut(repo_id) {
                repo_state.watch_degraded = Some(watch_degraded_text(reason));
            }
            Vec::new()
        }
    }
}

/// Reload every loadable under a fresh epoch.
fn refresh(state: &mut JjAppState, repo_id: RepoId) -> Vec<JjEffect> {
    let Some(repo_state) = state.repo_mut(repo_id) else {
        return Vec::new();
    };
    repo_state.refresh_epoch += 1;
    repo_state.log_loading = true;
    repo_state.bookmarks_loading = true;
    repo_state.ops_loading = true;
    let epoch = repo_state.refresh_epoch;
    let revset = repo_state.log_revset.clone();
    vec![
        JjEffect::LoadLogPage {
            repo_id,
            epoch,
            revset,
            skip: 0,
            limit: JJ_LOG_PAGE_SIZE,
        },
        JjEffect::LoadBookmarks { repo_id, epoch },
        JjEffect::LoadOpLog {
            repo_id,
            epoch,
            limit: OP_LOG_LIMIT,
        },
        JjEffect::LoadConflicts { repo_id, epoch },
    ]
}

/// Schedule a mutation unless one is already running for the repo. The
/// closure indirection keeps every arm a one-liner at the call sites.
fn run_mutation(
    state: &mut JjAppState,
    repo_id: RepoId,
    operation: &'static str,
    build: impl FnOnce() -> JjMutation,
) -> Vec<JjEffect> {
    let Some(repo_state) = state.repo_mut(repo_id) else {
        return Vec::new();
    };
    if repo_state.pending_command.is_some() {
        return Vec::new();
    }
    repo_state.pending_command = Some(operation);
    vec![JjEffect::RunMutation {
        repo_id,
        operation,
        mutation: build(),
    }]
}

/// Human-readable text for a degraded watch, shown by the panels.
fn watch_degraded_text(reason: RepoWatchDegradedReason) -> String {
    match reason {
        RepoWatchDegradedReason::TooManyFolders { dir_count } => {
            format!("too many folders to watch ({dir_count}); live updates paused")
        }
        RepoWatchDegradedReason::WatchLimitReached { unwatched_dirs } => format!(
            "OS watch limit reached ({unwatched_dirs} folders unwatched); live updates paused"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::runtime::worker::StoreMessage;
    use gitcomet_jj_core::{ChangeId, JjChange, JjCommitId, JjLogPage};

    fn reduce_standalone(state: &mut JjAppState, msg: JjMsg) -> Vec<JjEffect> {
        let mut repos = FxHashMap::default();
        let id_alloc = AtomicU64::new(1);
        reduce(&mut repos, &id_alloc, state, msg)
    }

    fn open_repo_state() -> JjAppState {
        let mut state = JjAppState::default();
        reduce_standalone(
            &mut state,
            JjMsg::OpenRepo {
                workdir: "/tmp/repo".into(),
            },
        );
        state
    }

    fn change(name: &str, working_copy: bool) -> JjChange {
        JjChange {
            change_id: ChangeId(name.to_string()),
            commit_id: JjCommitId(format!("c{name}")),
            divergent: false,
            conflicted: false,
            is_working_copy: working_copy,
            bookmarks: Vec::new(),
            author_name: "A".to_string(),
            author_email: "a@a".to_string(),
            committed_at_unix: 1,
            description: name.to_string(),
        }
    }

    #[test]
    fn opening_a_repo_allocates_it_activates_and_emits_open() {
        let mut state = JjAppState::default();
        let effects = reduce_standalone(
            &mut state,
            JjMsg::OpenRepo {
                workdir: "/tmp/repo".into(),
            },
        );
        assert_eq!(state.repos.len(), 1);
        assert_eq!(state.active_repo, Some(state.repos[0].id));
        assert!(matches!(effects[0], JjEffect::OpenRepo { .. }));
    }

    #[test]
    fn reopening_the_same_workdir_activates_without_duplicating() {
        let mut state = open_repo_state();
        let effects = reduce_standalone(
            &mut state,
            JjMsg::OpenRepo {
                workdir: "/tmp/repo".into(),
            },
        );
        assert_eq!(state.repos.len(), 1);
        assert!(effects.is_empty());
    }

    #[test]
    fn stale_epoch_results_are_dropped() {
        let mut state = open_repo_state();
        let repo_id = state.repos[0].id;
        // A refresh lands between the load and its result.
        reduce_standalone(&mut state, JjMsg::RefreshRepo { repo_id });
        let epoch_after_refresh = state.repos[0].refresh_epoch;
        assert_eq!(epoch_after_refresh, 1);

        let stale_page = JjLogPage {
            changes: vec![change("stale", false)],
            next_cursor: None,
        };
        reduce_standalone(
            &mut state,
            JjMsg::LogPageLoaded {
                repo_id,
                epoch: 0,
                skip: 0,
                page: Box::new(stale_page),
            },
        );
        assert!(state.repos[0].changes.is_empty());

        let fresh_page = JjLogPage {
            changes: vec![change("fresh", true), change("base", false)],
            next_cursor: None,
        };
        reduce_standalone(
            &mut state,
            JjMsg::LogPageLoaded {
                repo_id,
                epoch: epoch_after_refresh,
                skip: 0,
                page: Box::new(fresh_page),
            },
        );
        assert_eq!(state.repos[0].changes.len(), 2);
        assert_eq!(
            state.repos[0]
                .working_copy
                .as_ref()
                .map(|c| c.change_id.0.clone()),
            Some("fresh".to_string())
        );
    }

    #[test]
    fn load_more_uses_the_cursor_and_skips_while_loading() {
        let mut state = open_repo_state();
        let repo_id = state.repos[0].id;
        state.repos[0].next_cursor = Some(100);
        let effects = reduce_standalone(&mut state, JjMsg::LoadMoreLog { repo_id });
        match &effects[..] {
            [JjEffect::LoadLogPage { skip, limit, .. }] => {
                assert_eq!(*skip, 100);
                assert_eq!(*limit, JJ_LOG_PAGE_SIZE);
            }
            other => panic!("expected one LoadLogPage effect, got {other:?}"),
        }

        state.repos[0].log_loading = true;
        let effects = reduce_standalone(&mut state, JjMsg::LoadMoreLog { repo_id });
        assert!(effects.is_empty());
    }

    #[test]
    fn mutations_are_gated_on_a_pending_command() {
        let mut state = open_repo_state();
        let repo_id = state.repos[0].id;
        let effects = reduce_standalone(
            &mut state,
            JjMsg::DescribeChange {
                repo_id,
                change: ChangeId("abc".to_string()),
                message: "hello".to_string(),
            },
        );
        assert_eq!(effects.len(), 1);
        assert_eq!(state.repos[0].pending_command, Some("describe"));

        // A second gesture while the first runs is ignored.
        let effects = reduce_standalone(&mut state, JjMsg::OpUndo { repo_id });
        assert!(effects.is_empty());

        reduce_standalone(
            &mut state,
            JjMsg::CommandFinished {
                repo_id,
                operation: "describe",
                output: None,
            },
        );
        assert_eq!(state.repos[0].pending_command, None);
        // The finished command triggered a refresh (epoch bumped).
        assert_eq!(state.repos[0].refresh_epoch, 1);
    }

    #[test]
    fn command_failure_records_the_error_without_refreshing() {
        let mut state = open_repo_state();
        let repo_id = state.repos[0].id;
        reduce_standalone(
            &mut state,
            JjMsg::CommandFailed {
                repo_id,
                operation: "push",
                error: "network down".to_string(),
            },
        );
        assert_eq!(state.repos[0].refresh_epoch, 0);
        assert_eq!(
            state.repos[0]
                .last_command_error
                .as_ref()
                .map(|(_, e)| e.clone()),
            Some("network down".to_string())
        );
    }

    #[test]
    fn control_and_overtake_classification() {
        let control = JjMsg::CloseRepo { repo_id: RepoId(1) };
        assert!(control.is_control_message());
        assert!(!control.can_overtake_control_message());

        let internal = JjMsg::BookmarksLoaded {
            repo_id: RepoId(1),
            epoch: 0,
            bookmarks: Vec::new(),
        };
        assert!(!internal.is_control_message());
        assert!(internal.can_overtake_control_message());

        let gesture = JjMsg::FetchAll { repo_id: RepoId(1) };
        assert!(!gesture.is_control_message());
        assert!(!gesture.can_overtake_control_message());
    }
}
