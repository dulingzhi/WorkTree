//! Effect execution for the jj store: each effect spawns on one of the
//! shared executors and posts its result back through the worker channel.

use std::sync::Arc;

use gitcomet_jj_core::{JjLogQuery, JjRepository};

use crate::model::RepoId;
use crate::store::runtime::executor::TaskExecutor;
use crate::store::runtime::worker::WorkerSender;

use super::backend::JjBackend;
use super::msg::{JjEffect, JjMsg, JjMutation};

/// Everything an effect needs at spawn time. The repository handles come
/// from the worker loop's table; a handle that is missing (repo closed
/// while the effect was queued) makes the effect a no-op — the reducer has
/// already forgotten the repo, so there is nothing to report into.
pub(crate) struct JjEffectContext<'a> {
    pub backend: &'a Arc<dyn JjBackend>,
    pub repos: &'a rustc_hash::FxHashMap<RepoId, Arc<dyn JjRepository>>,
    /// Mutations and opens run on the primary pool.
    pub executor: &'a TaskExecutor,
    /// Bulk log walks run on the load pool, keeping them off the hot path
    /// like the git store.
    pub load_executor: &'a TaskExecutor,
    /// Light metadata reads (bookmarks, op log, conflicts) run on the
    /// metadata pool, mirroring the git store's split.
    pub metadata_executor: &'a TaskExecutor,
    pub msg_tx: &'a WorkerSender<JjMsg>,
}

pub(crate) fn schedule_effect(ctx: &JjEffectContext<'_>, effect: JjEffect) {
    match effect {
        JjEffect::OpenRepo { repo_id, workdir } => {
            let backend = Arc::clone(ctx.backend);
            let msg_tx = ctx.msg_tx.clone();
            ctx.executor.spawn(move || {
                let result = backend.open(&workdir);
                let msg = match result {
                    Ok(repo) => JjMsg::RepoOpened { repo_id, repo },
                    Err(err) => JjMsg::RepoOpenFailed {
                        repo_id,
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj open repo");
            });
        }
        JjEffect::LoadLogPage {
            repo_id,
            epoch,
            revset,
            skip,
            limit,
        } => {
            let repo = match ctx.repos.get(&repo_id) {
                Some(repo) => Arc::clone(repo),
                None => return,
            };
            let msg_tx = ctx.msg_tx.clone();
            ctx.load_executor.spawn(move || {
                let query = JjLogQuery {
                    revset,
                    limit,
                    skip,
                };
                let msg = match repo.log(&query) {
                    Ok(page) => JjMsg::LogPageLoaded {
                        repo_id,
                        epoch,
                        skip,
                        page: Box::new(page),
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "log",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj log");
            });
        }
        JjEffect::LoadBookmarks { repo_id, epoch } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.metadata_executor.spawn(move || {
                let msg = match repo.bookmarks() {
                    Ok(bookmarks) => JjMsg::BookmarksLoaded {
                        repo_id,
                        epoch,
                        bookmarks,
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "bookmarks",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj bookmark list");
            });
        }
        JjEffect::LoadOpLog {
            repo_id,
            epoch,
            limit,
        } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.metadata_executor.spawn(move || {
                let msg = match repo.op_log(limit) {
                    Ok(ops) => JjMsg::OpLogLoaded {
                        repo_id,
                        epoch,
                        ops,
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "op log",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj op log");
            });
        }
        JjEffect::LoadConflicts { repo_id, epoch } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.metadata_executor.spawn(move || {
                let msg = match repo.conflicts() {
                    Ok(conflicts) => JjMsg::ConflictsLoaded {
                        repo_id,
                        epoch,
                        conflicts,
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "conflicts",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj resolve --list");
            });
        }
        JjEffect::LoadChangeFiles {
            repo_id,
            epoch,
            change,
        } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.metadata_executor.spawn(move || {
                let msg = match repo.change_files(&change) {
                    Ok(files) => JjMsg::ChangeFilesLoaded {
                        repo_id,
                        epoch,
                        change,
                        files,
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "change files",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj diff --summary");
            });
        }
        JjEffect::LoadFileDiff {
            repo_id,
            epoch,
            change,
            path,
        } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.metadata_executor.spawn(move || {
                let msg = match repo.file_diff_text(&change, &path) {
                    Ok(text) => JjMsg::FileDiffLoaded {
                        repo_id,
                        epoch,
                        change,
                        path,
                        text,
                    },
                    Err(err) => JjMsg::LoadFailed {
                        repo_id,
                        epoch,
                        what: "file diff",
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, "jj diff --git");
            });
        }
        JjEffect::RunMutation {
            repo_id,
            operation,
            mutation,
        } => {
            let Some(repo) = ctx.repos.get(&repo_id) else {
                return;
            };
            let repo = Arc::clone(repo);
            let msg_tx = ctx.msg_tx.clone();
            ctx.executor.spawn(move || {
                let result = run_mutation(&*repo, &mutation);
                let msg = match result {
                    Ok(output) => JjMsg::CommandFinished {
                        repo_id,
                        operation,
                        output,
                    },
                    Err(err) => JjMsg::CommandFailed {
                        repo_id,
                        operation,
                        error: err.to_string(),
                    },
                };
                msg_tx.send_effect_or_log(msg, operation);
            });
        }
    }
}

/// Execute one mutation. Only the network operations produce output worth
/// keeping for the command log.
fn run_mutation(
    repo: &dyn JjRepository,
    mutation: &JjMutation,
) -> gitcomet_core::services::Result<Option<gitcomet_core::services::CommandOutput>> {
    use gitcomet_core::services::{CommandOutput, Result};
    fn plain(value: Result<()>) -> Result<Option<CommandOutput>> {
        value.map(|_| None)
    }
    match mutation {
        JjMutation::Describe { change, message } => plain(repo.describe(change, message)),
        JjMutation::New { message } => plain(repo.new_change(message.as_deref()).map(|_| ())),
        JjMutation::NewAt { onto } => plain(repo.new_change_at(onto).map(|_| ())),
        JjMutation::Abandon { change } => plain(repo.abandon(change)),
        JjMutation::Squash { from, into } => plain(repo.squash(from, into.as_ref())),
        JjMutation::BookmarkCreate { name, target } => plain(repo.bookmark_create(name, target)),
        JjMutation::BookmarkDelete { name } => plain(repo.bookmark_delete(name)),
        JjMutation::BookmarkRename { old_name, new_name } => {
            plain(repo.bookmark_rename(old_name, new_name))
        }
        JjMutation::BookmarkTrack { name, remote } => {
            plain(repo.bookmark_track(name, remote.as_deref()))
        }
        JjMutation::OpUndo => plain(repo.op_undo()),
        JjMutation::OpRestore { op_id } => plain(repo.op_restore(op_id)),
        JjMutation::FetchAll => repo.fetch_all_with_output().map(Some),
        JjMutation::Push => repo.push_tracked_with_output().map(Some),
        JjMutation::Snapshot => plain(repo.snapshot()),
    }
}
