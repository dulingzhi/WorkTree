//! The jj store's model: what the jj panels render.

use gitcomet_core::domain::RepoSpec;
use gitcomet_core::services::CommandOutput;
use gitcomet_jj_core::{JjBookmark, JjChange, JjConflict, JjOp};

use crate::model::RepoId;

/// How many changes one log page fetches.
pub const JJ_LOG_PAGE_SIZE: usize = 100;

/// One open jj repository in the store.
#[derive(Clone, Debug)]
pub struct JjRepoState {
    pub id: RepoId,
    pub spec: RepoSpec,
    /// Set when opening failed; the repo stays in the list to show the error.
    pub open_error: Option<String>,
    /// Monotonic per-repo refresh epoch. Every load result carries the epoch
    /// it was requested under; the reducer drops results whose epoch does
    /// not match, so a refresh that lands mid-load cannot be overwritten by
    /// the older load. (The git store implements the same guarantee with the
    /// runtime's `wrap_load_result` envelope; the jj store carries the epoch
    /// in each load message instead and leaves `wrap_load_result` a
    /// pass-through.)
    pub refresh_epoch: u64,
    /// The revset filter currently displayed; empty means `all()`.
    pub log_revset: String,
    /// Changes shown so far, accumulated across pages, newest first.
    pub changes: Vec<JjChange>,
    /// Cursor for the next page when more changes may exist.
    pub next_cursor: Option<usize>,
    pub log_loading: bool,
    pub log_error: Option<String>,
    /// The most recent load failure of any kind (log, bookmarks, …).
    pub load_error: Option<(&'static str, String)>,
    pub working_copy: Option<JjChange>,
    pub bookmarks: Vec<JjBookmark>,
    pub bookmarks_loading: bool,
    pub conflicts: Vec<JjConflict>,
    pub ops: Vec<JjOp>,
    pub ops_loading: bool,
    /// The in-flight mutation, if any — drives the panels' busy state and
    /// keeps gestures from stacking.
    pub pending_command: Option<&'static str>,
    pub last_command_error: Option<(String, String)>,
    /// Output of the last network command (fetch/push), for the command log.
    pub last_network_output: Option<CommandOutput>,
    /// Set while the filesystem watcher for this repo is degraded.
    pub watch_degraded: Option<String>,
}

impl JjRepoState {
    pub(crate) fn new(id: RepoId, spec: RepoSpec) -> Self {
        Self {
            id,
            spec,
            open_error: None,
            refresh_epoch: 0,
            log_revset: String::new(),
            changes: Vec::new(),
            next_cursor: None,
            log_loading: false,
            log_error: None,
            load_error: None,
            working_copy: None,
            bookmarks: Vec::new(),
            bookmarks_loading: false,
            conflicts: Vec::new(),
            ops: Vec::new(),
            ops_loading: false,
            pending_command: None,
            last_command_error: None,
            last_network_output: None,
            watch_degraded: None,
        }
    }
}

/// The jj store's whole state.
#[derive(Clone, Debug, Default)]
pub struct JjAppState {
    pub repos: Vec<JjRepoState>,
    pub active_repo: Option<RepoId>,
}

impl JjAppState {
    pub fn repo(&self, repo_id: RepoId) -> Option<&JjRepoState> {
        self.repos.iter().find(|repo| repo.id == repo_id)
    }

    pub fn repo_mut(&mut self, repo_id: RepoId) -> Option<&mut JjRepoState> {
        self.repos.iter_mut().find(|repo| repo.id == repo_id)
    }

    pub fn active_repo(&self) -> Option<&JjRepoState> {
        self.active_repo.and_then(|id| self.repo(id))
    }
}
