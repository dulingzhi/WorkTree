//! Store runtime machinery that is independent of any one store's
//! `Msg`/`Effect` vocabulary, extracted strangler-style so a second consumer
//! (the jj store) can instantiate it without inheriting the git model.
//!
//! What lives here:
//! * [`executor`] — the thread-pool `TaskExecutor` and its thread-count
//!   policies (already type-erased: tasks are `FnOnce` closures).
//! * [`worker`] — the command channel, the sender with its alive/shutdown
//!   and load-guard mechanics, the control-priority drain loop, and
//!   [`worker::RepoTaskToken`]. Generic over the store's message type via
//!   the [`worker::StoreMessage`] policy trait; the git store's
//!   implementation of that trait lives in `crate::store::worker_channel`.
//! * [`repo_monitor`] — filesystem watching for external changes. Only the
//!   per-store message construction is generic; the watcher, ignore-rule
//!   cache, debouncing, and degradation logic are shared as-is.
//!
//! The runtime half of the loads-in-flight merge pattern is the load guard:
//! `load_epoch` tagging plus [`worker::StoreMessage::wrap_load_result`],
//! which lets a store drop results from superseded repository loads. The
//! model half — the per-`RepoState` in-flight bitset that coalesces
//! duplicate load requests — encodes each store's own load kinds and stays
//! with that store's model (`crate::model::RepoLoadsInFlight` for git).
//!
//! Two git-specific details ride along here temporarily and move back out
//! when a second store lands: the `InsertRepoForTest` command variant (it
//! carries an `Arc<dyn GitRepository>`), and `repo_monitor`'s gitdir /
//! gitignore discovery, which opens the repository through gix. Everything
//! else in this subtree is VCS-neutral.

pub(super) mod executor;
pub(super) mod repo_monitor;
pub(super) mod worker;
