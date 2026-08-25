//! The jj store's message and effect vocabulary, plus its implementation of
//! the shared runtime's [`StoreMessage`] policy.

use std::path::PathBuf;
use std::sync::Arc;

use gitcomet_jj_core::{
    ChangeId, JjBookmark, JjConflict, JjFileStat, JjLogPage, JjOp, JjRepository,
};

use crate::model::RepoId;
use crate::msg::{RepoExternalChange, RepoWatchDegradedReason};
use crate::store::runtime::worker::StoreMessage;

/// One message into the jj store's worker loop.
pub enum JjMsg {
    // --- control (must not sit behind queued work) ---
    OpenRepo {
        workdir: PathBuf,
    },
    CloseRepo {
        repo_id: RepoId,
    },
    SetActiveRepo {
        repo_id: RepoId,
    },

    // --- open results (worker-local repo table) ---
    RepoOpened {
        repo_id: RepoId,
        repo: Arc<dyn JjRepository>,
    },
    RepoOpenFailed {
        repo_id: RepoId,
        error: String,
    },

    // --- refresh and loads ---
    /// Reload every loadable from scratch (bumps the refresh epoch).
    RefreshRepo {
        repo_id: RepoId,
    },
    LogPageLoaded {
        repo_id: RepoId,
        epoch: u64,
        /// The skip this page was requested under: `0` replaces the list,
        /// anything else appends.
        skip: usize,
        page: Box<JjLogPage>,
    },
    BookmarksLoaded {
        repo_id: RepoId,
        epoch: u64,
        bookmarks: Vec<JjBookmark>,
    },
    OpLogLoaded {
        repo_id: RepoId,
        epoch: u64,
        ops: Vec<JjOp>,
    },
    ConflictsLoaded {
        repo_id: RepoId,
        epoch: u64,
        conflicts: Vec<JjConflict>,
    },
    ChangeFilesLoaded {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
        files: Vec<JjFileStat>,
    },
    FileDiffLoaded {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
        path: String,
        text: String,
    },
    LoadFailed {
        repo_id: RepoId,
        epoch: u64,
        what: &'static str,
        error: String,
    },

    // --- user gestures ---
    LoadMoreLog {
        repo_id: RepoId,
    },
    /// Request the selected change's file list (change-detail panel).
    LoadChangeFiles {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
    },
    /// Request one file's diff text within a change.
    LoadFileDiff {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
        path: String,
    },
    SetRevset {
        repo_id: RepoId,
        revset: String,
    },
    DescribeChange {
        repo_id: RepoId,
        change: ChangeId,
        message: String,
    },
    NewChange {
        repo_id: RepoId,
        message: Option<String>,
    },
    /// Open a fresh change on top of the selected row and move @ there.
    NewChangeAt {
        repo_id: RepoId,
        change: ChangeId,
    },
    AbandonChange {
        repo_id: RepoId,
        change: ChangeId,
    },
    SquashChange {
        repo_id: RepoId,
        from: ChangeId,
        into: Option<ChangeId>,
    },
    BookmarkCreate {
        repo_id: RepoId,
        name: String,
        target: ChangeId,
    },
    BookmarkDelete {
        repo_id: RepoId,
        name: String,
    },
    BookmarkRename {
        repo_id: RepoId,
        old_name: String,
        new_name: String,
    },
    BookmarkTrack {
        repo_id: RepoId,
        name: String,
        remote: Option<String>,
    },
    OpUndo {
        repo_id: RepoId,
    },
    OpRestore {
        repo_id: RepoId,
        op_id: String,
    },
    FetchAll {
        repo_id: RepoId,
    },
    Push {
        repo_id: RepoId,
    },
    Snapshot {
        repo_id: RepoId,
    },

    // --- mutation results ---
    CommandFinished {
        repo_id: RepoId,
        operation: &'static str,
        output: Option<gitcomet_core::services::CommandOutput>,
    },
    CommandFailed {
        repo_id: RepoId,
        operation: &'static str,
        error: String,
    },

    // --- watcher ---
    RepoExternallyChanged {
        repo_id: RepoId,
        change: RepoExternalChange,
    },
    WatchDegraded {
        repo_id: RepoId,
        reason: RepoWatchDegradedReason,
    },
}

impl std::fmt::Debug for JjMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use JjMsg::*;
        match self {
            OpenRepo { workdir } => f
                .debug_struct("OpenRepo")
                .field("workdir", workdir)
                .finish(),
            CloseRepo { repo_id } => f
                .debug_struct("CloseRepo")
                .field("repo_id", repo_id)
                .finish(),
            SetActiveRepo { repo_id } => f
                .debug_struct("SetActiveRepo")
                .field("repo_id", repo_id)
                .finish(),
            // The repository handle is not `Debug`; keep the record useful.
            RepoOpened { repo_id, .. } => f
                .debug_struct("RepoOpened")
                .field("repo_id", repo_id)
                .field("repo", &"<handle>")
                .finish(),
            RepoOpenFailed { repo_id, error } => f
                .debug_struct("RepoOpenFailed")
                .field("repo_id", repo_id)
                .field("error", error)
                .finish(),
            RefreshRepo { repo_id } => f
                .debug_struct("RefreshRepo")
                .field("repo_id", repo_id)
                .finish(),
            LogPageLoaded {
                repo_id,
                epoch,
                skip,
                page,
            } => f
                .debug_struct("LogPageLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("skip", skip)
                .field("len", &page.changes.len())
                .finish(),
            BookmarksLoaded {
                repo_id,
                epoch,
                bookmarks,
            } => f
                .debug_struct("BookmarksLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("len", &bookmarks.len())
                .finish(),
            OpLogLoaded {
                repo_id,
                epoch,
                ops,
            } => f
                .debug_struct("OpLogLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("len", &ops.len())
                .finish(),
            ConflictsLoaded {
                repo_id,
                epoch,
                conflicts,
            } => f
                .debug_struct("ConflictsLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("len", &conflicts.len())
                .finish(),
            ChangeFilesLoaded {
                repo_id,
                epoch,
                change,
                files,
            } => f
                .debug_struct("ChangeFilesLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .field("len", &files.len())
                .finish(),
            FileDiffLoaded {
                repo_id,
                epoch,
                change,
                path,
                text,
            } => f
                .debug_struct("FileDiffLoaded")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .field("path", path)
                .field("len", &text.len())
                .finish(),
            LoadFailed {
                repo_id,
                epoch,
                what,
                error,
            } => f
                .debug_struct("LoadFailed")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("what", what)
                .field("error", error)
                .finish(),
            LoadMoreLog { repo_id } => f
                .debug_struct("LoadMoreLog")
                .field("repo_id", repo_id)
                .finish(),
            LoadChangeFiles {
                repo_id,
                epoch,
                change,
            } => f
                .debug_struct("LoadChangeFiles")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .finish(),
            LoadFileDiff {
                repo_id,
                epoch,
                change,
                path,
            } => f
                .debug_struct("LoadFileDiff")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .field("path", path)
                .finish(),
            SetRevset { repo_id, revset } => f
                .debug_struct("SetRevset")
                .field("repo_id", repo_id)
                .field("revset", revset)
                .finish(),
            DescribeChange {
                repo_id,
                change,
                message,
            } => f
                .debug_struct("DescribeChange")
                .field("repo_id", repo_id)
                .field("change", change)
                .field("message", message)
                .finish(),
            NewChange { repo_id, message } => f
                .debug_struct("NewChange")
                .field("repo_id", repo_id)
                .field("message", message)
                .finish(),
            NewChangeAt { repo_id, change } => f
                .debug_struct("NewChangeAt")
                .field("repo_id", repo_id)
                .field("change", change)
                .finish(),
            AbandonChange { repo_id, change } => f
                .debug_struct("AbandonChange")
                .field("repo_id", repo_id)
                .field("change", change)
                .finish(),
            SquashChange {
                repo_id,
                from,
                into,
            } => f
                .debug_struct("SquashChange")
                .field("repo_id", repo_id)
                .field("from", from)
                .field("into", into)
                .finish(),
            BookmarkCreate {
                repo_id,
                name,
                target,
            } => f
                .debug_struct("BookmarkCreate")
                .field("repo_id", repo_id)
                .field("name", name)
                .field("target", target)
                .finish(),
            BookmarkDelete { repo_id, name } => f
                .debug_struct("BookmarkDelete")
                .field("repo_id", repo_id)
                .field("name", name)
                .finish(),
            BookmarkRename {
                repo_id,
                old_name,
                new_name,
            } => f
                .debug_struct("BookmarkRename")
                .field("repo_id", repo_id)
                .field("old_name", old_name)
                .field("new_name", new_name)
                .finish(),
            BookmarkTrack {
                repo_id,
                name,
                remote,
            } => f
                .debug_struct("BookmarkTrack")
                .field("repo_id", repo_id)
                .field("name", name)
                .field("remote", remote)
                .finish(),
            OpUndo { repo_id } => f.debug_struct("OpUndo").field("repo_id", repo_id).finish(),
            OpRestore { repo_id, op_id } => f
                .debug_struct("OpRestore")
                .field("repo_id", repo_id)
                .field("op_id", op_id)
                .finish(),
            FetchAll { repo_id } => f
                .debug_struct("FetchAll")
                .field("repo_id", repo_id)
                .finish(),
            Push { repo_id } => f.debug_struct("Push").field("repo_id", repo_id).finish(),
            Snapshot { repo_id } => f
                .debug_struct("Snapshot")
                .field("repo_id", repo_id)
                .finish(),
            CommandFinished {
                repo_id,
                operation,
                output,
            } => f
                .debug_struct("CommandFinished")
                .field("repo_id", repo_id)
                .field("operation", operation)
                .field("has_output", &output.is_some())
                .finish(),
            CommandFailed {
                repo_id,
                operation,
                error,
            } => f
                .debug_struct("CommandFailed")
                .field("repo_id", repo_id)
                .field("operation", operation)
                .field("error", error)
                .finish(),
            RepoExternallyChanged { repo_id, change } => f
                .debug_struct("RepoExternallyChanged")
                .field("repo_id", repo_id)
                .field("change", change)
                .finish(),
            WatchDegraded { repo_id, reason } => f
                .debug_struct("WatchDegraded")
                .field("repo_id", repo_id)
                .field("reason", reason)
                .finish(),
        }
    }
}

impl JjMsg {
    pub fn repo_id(&self) -> Option<RepoId> {
        use JjMsg::*;
        match self {
            OpenRepo { .. } => None,
            CloseRepo { repo_id }
            | SetActiveRepo { repo_id }
            | RepoOpened { repo_id, .. }
            | RepoOpenFailed { repo_id, .. }
            | RefreshRepo { repo_id }
            | LogPageLoaded { repo_id, .. }
            | BookmarksLoaded { repo_id, .. }
            | OpLogLoaded { repo_id, .. }
            | ConflictsLoaded { repo_id, .. }
            | ChangeFilesLoaded { repo_id, .. }
            | FileDiffLoaded { repo_id, .. }
            | LoadFailed { repo_id, .. }
            | LoadMoreLog { repo_id }
            | LoadChangeFiles { repo_id, .. }
            | LoadFileDiff { repo_id, .. }
            | SetRevset { repo_id, .. }
            | DescribeChange { repo_id, .. }
            | NewChange { repo_id, .. }
            | NewChangeAt { repo_id, .. }
            | AbandonChange { repo_id, .. }
            | SquashChange { repo_id, .. }
            | BookmarkCreate { repo_id, .. }
            | BookmarkDelete { repo_id, .. }
            | BookmarkRename { repo_id, .. }
            | BookmarkTrack { repo_id, .. }
            | OpUndo { repo_id }
            | OpRestore { repo_id, .. }
            | FetchAll { repo_id }
            | Push { repo_id }
            | Snapshot { repo_id }
            | CommandFinished { repo_id, .. }
            | CommandFailed { repo_id, .. }
            | RepoExternallyChanged { repo_id, .. }
            | WatchDegraded { repo_id, .. } => Some(*repo_id),
        }
    }

    /// Whether the message originates inside the store (a load or mutation
    /// reply, or a watcher notification) rather than from the UI. Internal
    /// replies may overtake queued control commands in the drain loop;
    /// user gestures must not, to preserve dispatch order.
    pub fn is_store_internal(&self) -> bool {
        use JjMsg::*;
        matches!(
            self,
            RepoOpened { .. }
                | RepoOpenFailed { .. }
                | LogPageLoaded { .. }
                | BookmarksLoaded { .. }
                | OpLogLoaded { .. }
                | ConflictsLoaded { .. }
                | ChangeFilesLoaded { .. }
                | FileDiffLoaded { .. }
                | LoadFailed { .. }
                | CommandFinished { .. }
                | CommandFailed { .. }
                | RepoExternallyChanged { .. }
                | WatchDegraded { .. }
        )
    }
}

impl StoreMessage for JjMsg {
    fn is_control_message(&self) -> bool {
        use JjMsg::*;
        matches!(
            self,
            OpenRepo { .. } | CloseRepo { .. } | SetActiveRepo { .. }
        )
    }

    fn can_overtake_control_message(&self) -> bool {
        self.is_store_internal()
    }

    fn trace_name(&self) -> &'static str {
        use JjMsg::*;
        match self {
            OpenRepo { .. } => "jj_open_repo",
            CloseRepo { .. } => "jj_close_repo",
            SetActiveRepo { .. } => "jj_set_active_repo",
            RepoOpened { .. } => "jj_repo_opened",
            RepoOpenFailed { .. } => "jj_repo_open_failed",
            RefreshRepo { .. } => "jj_refresh_repo",
            LogPageLoaded { .. } => "jj_log_page_loaded",
            BookmarksLoaded { .. } => "jj_bookmarks_loaded",
            OpLogLoaded { .. } => "jj_op_log_loaded",
            ConflictsLoaded { .. } => "jj_conflicts_loaded",
            ChangeFilesLoaded { .. } => "jj_change_files_loaded",
            FileDiffLoaded { .. } => "jj_file_diff_loaded",
            LoadFailed { .. } => "jj_load_failed",
            LoadMoreLog { .. } => "jj_load_more_log",
            LoadChangeFiles { .. } => "jj_load_change_files",
            LoadFileDiff { .. } => "jj_load_file_diff",
            SetRevset { .. } => "jj_set_revset",
            DescribeChange { .. } => "jj_describe_change",
            NewChange { .. } => "jj_new_change",
            NewChangeAt { .. } => "jj_new_change_at",
            AbandonChange { .. } => "jj_abandon_change",
            SquashChange { .. } => "jj_squash_change",
            BookmarkCreate { .. } => "jj_bookmark_create",
            BookmarkDelete { .. } => "jj_bookmark_delete",
            BookmarkRename { .. } => "jj_bookmark_rename",
            BookmarkTrack { .. } => "jj_bookmark_track",
            OpUndo { .. } => "jj_op_undo",
            OpRestore { .. } => "jj_op_restore",
            FetchAll { .. } => "jj_fetch_all",
            Push { .. } => "jj_push",
            Snapshot { .. } => "jj_snapshot",
            CommandFinished { .. } => "jj_command_finished",
            CommandFailed { .. } => "jj_command_failed",
            RepoExternallyChanged { .. } => "jj_repo_externally_changed",
            WatchDegraded { .. } => "jj_watch_degraded",
        }
    }

    /// The jj store carries the refresh epoch inside each load message
    /// rather than wrapping results in an envelope, so this is a
    /// pass-through (see `JjRepoState::refresh_epoch`).
    fn wrap_load_result(_repo_id: RepoId, _load_epoch: u64, msg: Self) -> Self {
        msg
    }

    fn repo_externally_changed(repo_id: RepoId, change: RepoExternalChange) -> Self {
        JjMsg::RepoExternallyChanged { repo_id, change }
    }

    fn repo_watch_degraded(repo_id: RepoId, reason: RepoWatchDegradedReason) -> Self {
        JjMsg::WatchDegraded { repo_id, reason }
    }
}

/// One background action the reducer asks for, executed by the store loop.
pub enum JjEffect {
    OpenRepo {
        repo_id: RepoId,
        workdir: PathBuf,
    },
    LoadLogPage {
        repo_id: RepoId,
        epoch: u64,
        revset: String,
        skip: usize,
        limit: usize,
    },
    LoadBookmarks {
        repo_id: RepoId,
        epoch: u64,
    },
    LoadOpLog {
        repo_id: RepoId,
        epoch: u64,
        limit: usize,
    },
    LoadConflicts {
        repo_id: RepoId,
        epoch: u64,
    },
    LoadChangeFiles {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
    },
    LoadFileDiff {
        repo_id: RepoId,
        epoch: u64,
        change: ChangeId,
        path: String,
    },
    RunMutation {
        repo_id: RepoId,
        operation: &'static str,
        mutation: JjMutation,
    },
}

impl std::fmt::Debug for JjEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JjEffect::OpenRepo { repo_id, workdir } => f
                .debug_struct("OpenRepo")
                .field("repo_id", repo_id)
                .field("workdir", workdir)
                .finish(),
            JjEffect::LoadLogPage {
                repo_id,
                epoch,
                revset,
                skip,
                limit,
            } => f
                .debug_struct("LoadLogPage")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("revset", revset)
                .field("skip", skip)
                .field("limit", limit)
                .finish(),
            JjEffect::LoadBookmarks { repo_id, epoch } => f
                .debug_struct("LoadBookmarks")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .finish(),
            JjEffect::LoadOpLog {
                repo_id,
                epoch,
                limit,
            } => f
                .debug_struct("LoadOpLog")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("limit", limit)
                .finish(),
            JjEffect::LoadConflicts { repo_id, epoch } => f
                .debug_struct("LoadConflicts")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .finish(),
            JjEffect::LoadChangeFiles {
                repo_id,
                epoch,
                change,
            } => f
                .debug_struct("LoadChangeFiles")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .finish(),
            JjEffect::LoadFileDiff {
                repo_id,
                epoch,
                change,
                path,
            } => f
                .debug_struct("LoadFileDiff")
                .field("repo_id", repo_id)
                .field("epoch", epoch)
                .field("change", change)
                .field("path", path)
                .finish(),
            JjEffect::RunMutation {
                repo_id, operation, ..
            } => f
                .debug_struct("RunMutation")
                .field("repo_id", repo_id)
                .field("operation", operation)
                .finish(),
        }
    }
}

/// The mutation a `RunMutation` effect executes against the repository.
pub enum JjMutation {
    Describe {
        change: ChangeId,
        message: String,
    },
    New {
        message: Option<String>,
    },
    /// `jj new <onto>` — open a fresh change on top of a selected row.
    NewAt {
        onto: ChangeId,
    },
    Abandon {
        change: ChangeId,
    },
    Squash {
        from: ChangeId,
        into: Option<ChangeId>,
    },
    BookmarkCreate {
        name: String,
        target: ChangeId,
    },
    BookmarkDelete {
        name: String,
    },
    BookmarkRename {
        old_name: String,
        new_name: String,
    },
    BookmarkTrack {
        name: String,
        remote: Option<String>,
    },
    OpUndo,
    OpRestore {
        op_id: String,
    },
    FetchAll,
    Push,
    Snapshot,
}
