//! The jj store: a second instantiation of the shared store runtime
//! (`crate::store::runtime`) with a jj-native vocabulary.
//!
//! It owns the same shape of worker loop as the git store — a command
//! channel with control-priority draining, a reducer over an
//! `Arc<RwLock<Arc<…>>>` state cell, effects spawned on the shared
//! executors, and filesystem monitoring for the active repo — but the
//! model, messages, and effects all speak jj: changes and revsets instead
//! of commits and branches, the operation log instead of reflogs. The
//! strangler constraint from the plan holds: this store consumes the
//! runtime exactly as extracted, and where the git store leans on
//! load-epoch envelopes this one carries the epoch in its load messages —
//! both are allowed by the `StoreMessage` policy.
//!
//! Not wired into the app yet; the flavor switch (#76) picks this up.

pub mod backend;
mod effects;
mod model;
mod msg;
mod reducer;

pub use backend::{CliJjBackend, JjBackend};
pub use model::{JJ_LOG_PAGE_SIZE, JjAppState, JjRepoState};
pub use msg::{JjEffect, JjMsg, JjMutation};

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use gitcomet_jj_core::JjRepository;
use rustc_hash::FxHashMap;

use crate::model::RepoId;
use crate::msg::StoreEvent;
#[cfg(any(test, feature = "test-support"))]
use crate::store::runtime::executor::StoreExecutorPool;
use crate::store::runtime::executor::{
    TaskExecutor, default_worker_threads, metadata_worker_threads, repo_load_worker_threads,
};
use crate::store::runtime::repo_monitor::RepoMonitorManager;
use crate::store::runtime::worker::{
    StoreInstanceId, WorkerCommand, WorkerSender, recv_next_worker_command,
};

use effects::{JjEffectContext, schedule_effect};

/// The jj-flavored store handle. Cloning shares the worker loop; the last
/// `JjStorePublicLifetime` drops on store shutdown.
pub struct JjStore {
    state: Arc<RwLock<Arc<JjAppState>>>,
    msg_tx: WorkerSender<JjMsg>,
    public_lifetime: Arc<JjStorePublicLifetime>,
}

struct JjStorePublicLifetime {
    msg_tx: WorkerSender<JjMsg>,
}

impl Drop for JjStorePublicLifetime {
    fn drop(&mut self) {
        self.msg_tx.shutdown();
    }
}

impl Clone for JjStore {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            msg_tx: self.msg_tx.clone(),
            public_lifetime: Arc::clone(&self.public_lifetime),
        }
    }
}

impl JjStore {
    pub fn new(backend: Arc<dyn JjBackend>) -> (Self, smol::channel::Receiver<StoreEvent>) {
        let state = Arc::new(RwLock::new(Arc::new(JjAppState::default())));
        let (command_tx, command_rx) = mpsc::channel::<WorkerCommand<JjMsg>>();
        let store_id = StoreInstanceId::next();
        let store_alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let msg_tx = WorkerSender::new(command_tx, Arc::clone(&store_alive), store_id);
        let (event_tx, event_rx) = smol::channel::bounded::<StoreEvent>(1);

        let thread_state = Arc::clone(&state);
        let thread_msg_tx = msg_tx.clone();

        thread::spawn(move || {
            #[cfg(any(test, feature = "test-support"))]
            let executor = TaskExecutor::shared_for_store(
                StoreExecutorPool::Primary,
                default_worker_threads(),
            );
            #[cfg(not(any(test, feature = "test-support")))]
            let executor = TaskExecutor::new(default_worker_threads());

            #[cfg(any(test, feature = "test-support"))]
            let repo_load_executor = TaskExecutor::shared_for_store(
                StoreExecutorPool::RepoLoad,
                repo_load_worker_threads(),
            );
            #[cfg(not(any(test, feature = "test-support")))]
            let repo_load_executor = TaskExecutor::new(repo_load_worker_threads());

            #[cfg(any(test, feature = "test-support"))]
            let metadata_executor = TaskExecutor::shared_for_store(
                StoreExecutorPool::Metadata,
                metadata_worker_threads(),
            );
            #[cfg(not(any(test, feature = "test-support")))]
            let metadata_executor = TaskExecutor::new(metadata_worker_threads());

            let mut repos: FxHashMap<RepoId, Arc<dyn JjRepository>> = FxHashMap::default();
            let mut repo_monitors = RepoMonitorManager::new();
            let id_alloc = AtomicU64::new(1);
            let active_repo_id = Arc::new(AtomicU64::new(0));
            let mut deferred_commands = VecDeque::new();

            while let Ok(command) = recv_next_worker_command(&command_rx, &mut deferred_commands) {
                let msg = match command {
                    WorkerCommand::Msg(msg) => *msg,
                    WorkerCommand::Shutdown => break,
                    // The test-repo insertion variant is git-specific and
                    // unused here; jj tests drive a fake backend instead.
                    #[cfg(any(test, feature = "test-support"))]
                    WorkerCommand::InsertRepoForTest { .. } => continue,
                };

                if !thread_msg_tx.is_alive() {
                    continue;
                }

                // A closed repo's watcher stops before the reducer drops
                // it from the state.
                if let JjMsg::CloseRepo { repo_id } = &msg {
                    repo_monitors.stop(*repo_id);
                }

                // Reduce under the write lock, then apply effects.
                let effects = {
                    let mut app_state = thread_state.write().unwrap_or_else(|e| e.into_inner());
                    let app_state = Arc::make_mut(&mut app_state);
                    reducer::reduce(&mut repos, &id_alloc, app_state, msg)
                };

                // Coalesced state-changed notification (at most one
                // pending), mirroring the git store's semantics.
                let active_value = thread_state
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .active_repo
                    .map(|id| id.0)
                    .unwrap_or(0);
                active_repo_id.store(active_value, Ordering::Relaxed);
                if !thread_msg_tx.is_alive() {
                    continue;
                }
                let _ = event_tx.try_send(StoreEvent::StateChanged);

                // Keep monitoring scoped to the active repo, like the git
                // store: colocated jj repos have a `.git` dir, which is
                // what the watcher keys on — and every jj command syncs
                // refs through it, so external jj activity is caught too.
                let (active_repo, active_workdir) = {
                    let state = thread_state.read().unwrap_or_else(|e| e.into_inner());
                    let active_repo = state.active_repo;
                    let active_workdir = active_repo.and_then(|repo_id| {
                        state
                            .repos
                            .iter()
                            .find(|repo| repo.id == repo_id)
                            .map(|repo| repo.spec.workdir.clone())
                    });
                    (active_repo, active_workdir)
                };
                for repo_id in repo_monitors.running_repo_ids() {
                    if Some(repo_id) != active_repo {
                        repo_monitors.stop(repo_id);
                    }
                }
                if let Some(repo_id) = active_repo
                    && let Some(workdir) = active_workdir
                    && repos.contains_key(&repo_id)
                {
                    repo_monitors.start(
                        repo_id,
                        workdir,
                        thread_msg_tx.clone(),
                        Arc::clone(&active_repo_id),
                    );
                }

                for effect in effects {
                    schedule_effect(
                        &JjEffectContext {
                            backend: &backend,
                            repos: &repos,
                            executor: &executor,
                            load_executor: &repo_load_executor,
                            metadata_executor: &metadata_executor,
                            msg_tx: &thread_msg_tx,
                        },
                        effect,
                    );
                }
            }

            repo_monitors.stop_all();
        });

        (
            Self {
                state,
                msg_tx: msg_tx.clone(),
                public_lifetime: Arc::new(JjStorePublicLifetime { msg_tx }),
            },
            event_rx,
        )
    }

    pub fn dispatch(&self, msg: JjMsg) {
        self.msg_tx.dispatch(msg);
    }

    pub fn snapshot(&self) -> Arc<JjAppState> {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests;
