//! Git-store policy for the shared worker machinery (see
//! `super::runtime::worker`): the concrete command and sender types bound to
//! `Msg`, plus the [`StoreMessage`] implementation that decides control
//! priority, load-result wrapping, and monitor message construction for the
//! git store's message vocabulary.

use crate::msg::{InternalMsg, Msg, RepoExternalChange, RepoWatchDegradedReason};

use super::RepoId;
use super::repo_load_trace;
use super::runtime::worker::{StoreMessage, WorkerCommand, WorkerSender};

pub(super) type StoreWorkerCommand = WorkerCommand<Msg>;
pub(super) type StoreWorkerSender = WorkerSender<Msg>;
pub(super) use super::runtime::worker::StoreInstanceId;

impl StoreMessage for Msg {
    fn is_control_message(&self) -> bool {
        matches!(
            self,
            Msg::OpenRepo(_)
                | Msg::CloseRepo { .. }
                | Msg::CloseRepos { .. }
                | Msg::SetActiveRepo { .. }
                | Msg::ReorderRepoTabs { .. }
        )
    }

    fn can_overtake_control_message(&self) -> bool {
        matches!(self, Msg::Internal(_))
    }

    fn trace_name(&self) -> &'static str {
        repo_load_trace::msg_name(self)
    }

    fn wrap_load_result(repo_id: RepoId, load_epoch: u64, msg: Msg) -> Msg {
        match msg {
            Msg::Internal(message) => match message {
                InternalMsg::RepoLoadFinished { .. } => Msg::Internal(message),
                message => {
                    repo_load_trace::trace!(
                        "wrap_repo_load_message repo_id={:?} load_epoch={} inner={}",
                        repo_id,
                        load_epoch,
                        repo_load_trace::internal_msg_name(&message)
                    );
                    Msg::Internal(InternalMsg::RepoLoadFinished {
                        repo_id,
                        load_epoch,
                        message: Box::new(message),
                    })
                }
            },
            msg => msg,
        }
    }

    fn repo_externally_changed(repo_id: RepoId, change: RepoExternalChange) -> Self {
        Msg::RepoExternallyChanged { repo_id, change }
    }

    fn repo_watch_degraded(repo_id: RepoId, reason: RepoWatchDegradedReason) -> Self {
        Msg::RepoWatchDegraded { repo_id, reason }
    }
}
