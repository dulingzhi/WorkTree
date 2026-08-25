//! Jujutsu-flavored repository access for GitComet.
//!
//! This crate owns the *jj-semantic* surface: revset log walks, changes
//! (describe/new/abandon/squash), bookmarks (local and remote refs), the
//! operation log with undo/restore, working-copy snapshots, conflicts, and
//! the change-detail reads (`change_files`/`file_diff_text`) the native
//! panels render. Deeper history tooling (blame, image diffs, cross-VCS
//! diff layout) stays in the git read model, keyed by commit id, which
//! every [`JjChange`] carries.
//!
//! The trait is shaped by jj's model rather than git's (there is no staging
//! area, the working copy is a change, bookmarks track remotes), so the jj
//! store and panels built on it can speak jj natively instead of bending
//! git vocabulary. The v1 implementation [`JjCliRepository`] drives the
//! `jj` CLI with strict `-T` template parsing and a version whitelist; a
//! future version can swap in an embedded jj library behind the same trait.

mod cli;
pub mod domain;
mod parse;
pub mod version;

pub use cli::JjCliRepository;
pub use domain::{
    ChangeId, JjBookmark, JjChange, JjCommitId, JjConflict, JjFileStat, JjFileStatus, JjLogPage,
    JjLogQuery, JjOp,
};

use gitcomet_core::domain::RepoSpec;
use gitcomet_core::error::{Error, ErrorKind};
use gitcomet_core::services::{CommandOutput, Result};

/// Read and write access to a jj repository, in jj's own terms.
///
/// All methods run on a background thread (each is a CLI process spawn in
/// the v1 implementation); the trait is object-safe so the store can hold
/// `Arc<dyn JjRepository>`.
pub trait JjRepository: Send + Sync {
    /// The repository this handle is bound to.
    fn spec(&self) -> &RepoSpec;

    /// Refresh the working-copy snapshot (`jj st`). Every jj command
    /// snapshots implicitly; calling this buys jj-side freshness for the
    /// user's *other* jj tooling at moments the store chooses. Throttling
    /// is the store's policy, not the implementation's.
    fn snapshot(&self) -> Result<()>;

    /// Walk a revset, one page at a time. An empty revset means `all()`;
    /// `JjLogQuery::skip` continues after a previous page (see its docs
    /// for why paging is positional rather than ancestry-based).
    fn log(&self, query: &JjLogQuery) -> Result<JjLogPage>;

    /// The paths a change touches, with status letters (`jj diff -r
    /// <change> --summary`). The empty change yields an empty list.
    fn change_files(&self, change: &ChangeId) -> Result<Vec<JjFileStat>>;

    /// One file's diff for a change as git-style unified text (`jj diff -r
    /// <change> --git -- <path>`), verbatim for the caller to render.
    /// Binary files come back as whatever jj prints for them.
    fn file_diff_text(&self, change: &ChangeId, path: &str) -> Result<String>;

    /// The current working-copy change (`@`).
    fn working_copy(&self) -> Result<JjChange> {
        let mut page = self.log(&JjLogQuery::new("@", 1))?;
        page.changes
            .drain(..)
            .next()
            .ok_or_else(|| Error::new(ErrorKind::Backend("jj log @ returned no changes".into())))
    }

    /// Set a change's description (`jj describe -r`).
    fn describe(&self, change: &ChangeId, message: &str) -> Result<()>;

    /// Open a fresh change on top of `@` (`jj new`), optionally described,
    /// and return it as the new working copy.
    fn new_change(&self, message: Option<&str>) -> Result<JjChange>;

    /// Drop a change, rebasing its descendants (`jj abandon`).
    fn abandon(&self, change: &ChangeId) -> Result<()>;

    /// Move a change's contents into another (`jj squash`); `into` defaults
    /// to the working copy.
    fn squash(&self, from: &ChangeId, into: Option<&ChangeId>) -> Result<()>;

    /// Split a change's contents interactively. The CLI implementation
    /// refuses this — it needs a terminal diff editor.
    fn split(&self, change: &ChangeId) -> Result<()>;

    /// All bookmarks, local and remote refs alike (`--all`).
    fn bookmarks(&self) -> Result<Vec<JjBookmark>>;

    fn bookmark_create(&self, name: &str, target: &ChangeId) -> Result<()>;
    fn bookmark_delete(&self, name: &str) -> Result<()>;
    fn bookmark_rename(&self, old_name: &str, new_name: &str) -> Result<()>;
    /// Track a bookmark's remote counterpart; `remote` of `None` tracks the
    /// plain local bookmark name.
    fn bookmark_track(&self, name: &str, remote: Option<&str>) -> Result<()>;

    /// The most recent operations (`jj op log`), newest first.
    fn op_log(&self, limit: usize) -> Result<Vec<JjOp>>;

    /// Undo the latest operation. jj 0.44 spells this as reverting the
    /// newest operation (`jj op revert <latest>`), which records a new
    /// operation with the inverse effect.
    fn op_undo(&self) -> Result<()>;

    /// Restore the repo to a previous operation's state (`jj op restore`).
    fn op_restore(&self, op_id: &str) -> Result<()>;

    /// Paths with unresolved conflicts in the working-copy change; empty
    /// when there are none.
    fn conflicts(&self) -> Result<Vec<JjConflict>>;

    /// Fetch every remote (`jj git fetch --all-remotes`); jj has no pull —
    /// fetching is pulling. Returns the command output for the command log.
    fn fetch_all_with_output(&self) -> Result<CommandOutput>;

    /// Push tracking bookmarks (`jj git push`). Returns the command output
    /// for the command log.
    fn push_tracked_with_output(&self) -> Result<CommandOutput>;
}
