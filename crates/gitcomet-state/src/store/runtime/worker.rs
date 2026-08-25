//! The worker-loop machinery shared by every store flavor: the command
//! channel, the control-priority drain loop, and the load guard that lets a
//! store drop results from superseded loads.
//!
//! Generic over the store's message type `M`; [`StoreMessage`] is the policy
//! surface each store implements for its own message enum (the git store's
//! implementation lives in `crate::store::worker_channel`).

use crate::model::RepoId;
use crate::msg::{RepoExternalChange, RepoWatchDegradedReason};
use gitcomet_core::services::CancellationToken;
#[cfg(any(test, feature = "test-support"))]
use gitcomet_core::services::GitRepository;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use crate::store::repo_load_trace;
use crate::store::send_diagnostics::{self, SendFailureKind};

/// How a store's message type participates in the shared worker machinery.
/// Every method corresponds to a real call site here; the mechanics around
/// each call (channel sends, guards, priority) stay in this module.
pub(in crate::store) trait StoreMessage: Send + 'static {
    /// Whether this message changes which repository the store is bound to
    /// (open/close/activate/reorder) and must not sit behind queued work.
    fn is_control_message(&self) -> bool;

    /// Whether this message may overtake queued control commands in the
    /// drain loop. Store-internal replies overtake; user-initiated work
    /// does not, to preserve dispatch order.
    fn can_overtake_control_message(&self) -> bool;

    /// Stable name for repo-load tracing.
    fn trace_name(&self) -> &'static str;

    /// Wrap a background load's result message so the worker loop can
    /// attribute it to a load epoch and drop results from superseded
    /// loads. Messages that are already wrapped pass through unchanged.
    fn wrap_load_result(repo_id: RepoId, load_epoch: u64, msg: Self) -> Self;

    /// Message emitted when the filesystem watcher observes external
    /// changes to a repository.
    fn repo_externally_changed(repo_id: RepoId, change: RepoExternalChange) -> Self;

    /// Message emitted when a repository's filesystem watch degrades (or
    /// recovers), so the UI can explain why live updates stopped.
    fn repo_watch_degraded(repo_id: RepoId, reason: RepoWatchDegradedReason) -> Self;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::store) struct StoreInstanceId(u64);

impl StoreInstanceId {
    pub(in crate::store) fn next() -> Self {
        static NEXT_STORE_ID: AtomicU64 = AtomicU64::new(1);
        Self(NEXT_STORE_ID.fetch_add(1, Ordering::Relaxed))
    }

    pub(in crate::store) fn get(self) -> u64 {
        self.0
    }
}

pub(in crate::store) enum WorkerCommand<M> {
    Msg(Box<M>),
    Shutdown,
    #[cfg(any(test, feature = "test-support"))]
    InsertRepoForTest {
        repo_id: RepoId,
        repo: Arc<dyn GitRepository>,
    },
}

enum WorkerSenderInner<M> {
    Command(mpsc::Sender<WorkerCommand<M>>),
    #[cfg(test)]
    MsgForTest(mpsc::Sender<M>),
}

/// Hand-written so the generic sender stays `Clone` without requiring
/// `M: Clone` — none of the fields store an `M` by value.
impl<M> Clone for WorkerSenderInner<M> {
    fn clone(&self) -> Self {
        match self {
            WorkerSenderInner::Command(tx) => WorkerSenderInner::Command(tx.clone()),
            #[cfg(test)]
            WorkerSenderInner::MsgForTest(tx) => WorkerSenderInner::MsgForTest(tx.clone()),
        }
    }
}

pub(in crate::store) struct WorkerSender<M> {
    inner: WorkerSenderInner<M>,
    alive: Arc<AtomicBool>,
    store_id: StoreInstanceId,
    repo_load_guard: Option<RepoLoadGuard>,
    cancellation: Option<CancellationToken>,
}

impl<M> Clone for WorkerSender<M> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            alive: Arc::clone(&self.alive),
            store_id: self.store_id,
            repo_load_guard: self.repo_load_guard.clone(),
            cancellation: self.cancellation.clone(),
        }
    }
}

#[derive(Clone)]
struct RepoLoadGuard {
    repo_id: RepoId,
    load_epoch: u64,
}

impl<M: StoreMessage> WorkerSender<M> {
    pub(in crate::store) fn new(
        tx: mpsc::Sender<WorkerCommand<M>>,
        alive: Arc<AtomicBool>,
        store_id: StoreInstanceId,
    ) -> Self {
        Self {
            inner: WorkerSenderInner::Command(tx),
            alive,
            store_id,
            repo_load_guard: None,
            cancellation: None,
        }
    }

    #[cfg(test)]
    pub(in crate::store) fn for_test_msg_sender(tx: mpsc::Sender<M>) -> Self {
        Self {
            inner: WorkerSenderInner::MsgForTest(tx),
            alive: Arc::new(AtomicBool::new(true)),
            store_id: StoreInstanceId(0),
            repo_load_guard: None,
            cancellation: None,
        }
    }

    pub(in crate::store) fn store_id(&self) -> StoreInstanceId {
        self.store_id
    }

    pub(in crate::store) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub(in crate::store) fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub(in crate::store) fn with_repo_load_guard(
        &self,
        repo_id: RepoId,
        load_epoch: u64,
        cancellation: CancellationToken,
    ) -> Self {
        let mut guarded = self.clone();
        guarded.repo_load_guard = Some(RepoLoadGuard {
            repo_id,
            load_epoch,
        });
        guarded.cancellation = Some(cancellation);
        guarded
    }

    pub(in crate::store) fn dispatch(&self, msg: M) {
        self.send_or_log(
            msg,
            SendFailureKind::StoreDispatch,
            "AppStore::dispatch",
            false,
        );
    }

    pub(in crate::store) fn send_effect_or_log(&self, msg: M, context: &'static str) {
        if self.is_cancelled() {
            repo_load_trace::trace!(
                "suppress_effect_message_cancelled msg={} context={}",
                msg.trace_name(),
                context
            );
            return;
        }
        repo_load_trace::trace!(
            "send_effect_message msg={} context={}",
            msg.trace_name(),
            context
        );
        let msg = self.wrap_effect_message(msg);
        self.send_or_log(msg, SendFailureKind::EffectMessage, context, true);
    }

    pub(in crate::store) fn send_repo_monitor_or_log(&self, msg: M, context: &'static str) {
        self.send_or_log(msg, SendFailureKind::RepoMonitorMessage, context, true);
    }

    fn wrap_effect_message(&self, msg: M) -> M {
        #[cfg(test)]
        if matches!(&self.inner, WorkerSenderInner::MsgForTest(_)) {
            return msg;
        }

        let Some(guard) = &self.repo_load_guard else {
            return msg;
        };
        M::wrap_load_result(guard.repo_id, guard.load_epoch, msg)
    }

    fn send_or_log(
        &self,
        msg: M,
        kind: SendFailureKind,
        context: &'static str,
        suppress_after_shutdown: bool,
    ) {
        if suppress_after_shutdown && !self.is_alive() {
            return;
        }

        match &self.inner {
            WorkerSenderInner::Command(tx) => {
                send_diagnostics::send_or_log(tx, WorkerCommand::Msg(Box::new(msg)), kind, context)
            }
            #[cfg(test)]
            WorkerSenderInner::MsgForTest(tx) => {
                send_diagnostics::send_or_log(tx, msg, kind, context)
            }
        }
    }

    pub(in crate::store) fn shutdown(&self) {
        if !self.alive.swap(false, Ordering::AcqRel) {
            return;
        }

        match &self.inner {
            WorkerSenderInner::Command(tx) => {
                let _ = tx.send(WorkerCommand::Shutdown);
            }
            #[cfg(test)]
            WorkerSenderInner::MsgForTest(_) => {}
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::store) fn insert_repo_for_test(
        &self,
        repo_id: RepoId,
        repo: Arc<dyn GitRepository>,
    ) {
        if !self.is_alive() {
            return;
        }

        match &self.inner {
            WorkerSenderInner::Command(tx) => {
                let _ = tx.send(WorkerCommand::InsertRepoForTest { repo_id, repo });
            }
            #[cfg(test)]
            WorkerSenderInner::MsgForTest(_) => {}
        }
    }
}

/// Bookkeeping for one repository's in-flight background loads: the load
/// epoch that tags result messages, plus the cancellation tokens that stop
/// superseded work. Shared by every store flavor; the git store keeps the
/// per-repo table in its worker loop.
#[derive(Clone)]
pub(in crate::store) struct RepoTaskToken {
    pub(in crate::store) load_epoch: u64,
    pub(in crate::store) cancellation: CancellationToken,
    /// Cancellation for the *current* log walk alone. An author-filtered walk
    /// on a large repository runs for tens of seconds and the repo-load pool
    /// has one or two threads, so a superseded walk has to be stopped for its
    /// replacement to start at all — but stopping it must not disturb the
    /// repository's other loads, which share [`Self::cancellation`].
    log_cancellation: Arc<Mutex<CancellationToken>>,
}

impl RepoTaskToken {
    pub(in crate::store) fn new(load_epoch: u64) -> Self {
        Self {
            load_epoch,
            cancellation: CancellationToken::new(),
            log_cancellation: Arc::new(Mutex::new(CancellationToken::new())),
        }
    }

    /// Cancels the log walk in flight, if any, and hands out the token for the
    /// walk that replaces it.
    pub(in crate::store) fn take_over_log(&self) -> CancellationToken {
        let mut slot = self
            .log_cancellation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        slot.cancel();
        let next = CancellationToken::new();
        *slot = next.clone();
        next
    }

    /// Cancels every task running under this token, log walks included.
    pub(in crate::store) fn cancel(&self) {
        self.cancellation.cancel();
        self.log_cancellation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel();
    }
}

pub(in crate::store) fn is_control_command<M: StoreMessage>(command: &WorkerCommand<M>) -> bool {
    match command {
        WorkerCommand::Msg(msg) => msg.is_control_message(),
        WorkerCommand::Shutdown => true,
        #[cfg(any(test, feature = "test-support"))]
        WorkerCommand::InsertRepoForTest { .. } => true,
    }
}

pub(in crate::store) fn can_control_command_overtake<M: StoreMessage>(
    command: &WorkerCommand<M>,
) -> bool {
    matches!(
        command,
        WorkerCommand::Msg(msg) if msg.can_overtake_control_message()
    )
}

fn first_control_command_before_order_barrier<M: StoreMessage>(
    deferred: &VecDeque<WorkerCommand<M>>,
) -> Option<usize> {
    for (ix, command) in deferred.iter().enumerate() {
        if is_control_command(command) {
            return Some(ix);
        }
        if !can_control_command_overtake(command) {
            return None;
        }
    }
    None
}

fn has_order_barrier_before_control<M: StoreMessage>(
    deferred: &VecDeque<WorkerCommand<M>>,
) -> bool {
    for command in deferred {
        if is_control_command(command) {
            return false;
        }
        if !can_control_command_overtake(command) {
            return true;
        }
    }
    false
}

pub(in crate::store) fn recv_next_worker_command<M: StoreMessage>(
    command_rx: &mpsc::Receiver<WorkerCommand<M>>,
    deferred: &mut VecDeque<WorkerCommand<M>>,
) -> Result<WorkerCommand<M>, mpsc::RecvError> {
    if let Some(ix) = first_control_command_before_order_barrier(deferred) {
        return Ok(deferred.remove(ix).expect("deferred command exists"));
    }

    let first = match deferred.pop_front() {
        Some(command) => command,
        None => command_rx.recv()?,
    };
    if is_control_command(&first) {
        return Ok(first);
    }
    if !can_control_command_overtake(&first) {
        return Ok(first);
    }
    if has_order_barrier_before_control(deferred) {
        return Ok(first);
    }

    while let Ok(command) = command_rx.try_recv() {
        if is_control_command(&command) {
            deferred.push_front(first);
            return Ok(command);
        }
        if !can_control_command_overtake(&command) {
            deferred.push_back(command);
            break;
        }
        deferred.push_back(command);
    }

    Ok(first)
}
