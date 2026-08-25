//! Domain types for the jj flavor, shaped by jj semantics rather than git's.
//!
//! Field choices follow the core domain's conventions where they meet
//! (author name/email as strings, timestamps as Unix-seconds `i64`) so the
//! jj change-list panel can reuse the git history row rendering; everything
//! jj-specific (change ids, divergent flags, remote bookmark refs) stays in
//! its own vocabulary instead of being forced into git shapes.

use std::fmt;

/// A jj change id in its short form (`change_id.short()`, e.g. `wqnwyzpk`).
///
/// Short ids are what every jj command accepts and what the CLI prints, so
/// they are the currency throughout this crate. jj may re-abbreviate them
/// further in some outputs; parsing is tolerant of any non-empty id string.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ChangeId(pub String);

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ChangeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A git commit id backing a jj change, short form.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct JjCommitId(pub String);

impl fmt::Display for JjCommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for JjCommitId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// One entry of a jj log page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjChange {
    pub change_id: ChangeId,
    pub commit_id: JjCommitId,
    /// The change has multiple heads (rewritten concurrently elsewhere).
    pub divergent: bool,
    /// The change's commit has conflicts embedded.
    pub conflicted: bool,
    /// This change is the current working copy (`@`).
    pub is_working_copy: bool,
    /// Bookmarks pointing at the change, local and remote refs alike, in
    /// jj's rendering (`main`, `main@origin`).
    pub bookmarks: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    /// Committer timestamp in Unix seconds, matching the core domain.
    pub committed_at_unix: i64,
    /// May be empty (undescribed change) and multi-line.
    pub description: String,
}

/// A page request for [`crate::JjRepository::log`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjLogQuery {
    /// The revset to walk; empty means `all()`.
    pub revset: String,
    /// Maximum number of changes to return.
    pub limit: usize,
    /// Rows to skip from the top of the revset — the cursor produced by a
    /// previous page.
    pub skip: usize,
}

impl JjLogQuery {
    pub fn new(revset: impl Into<String>, limit: usize) -> Self {
        Self {
            revset: revset.into(),
            limit,
            skip: 0,
        }
    }

    /// Continue after a previous page.
    pub fn after(mut self, cursor: usize) -> Self {
        self.skip = cursor;
        self
    }
}

/// One page of jj log results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjLogPage {
    pub changes: Vec<JjChange>,
    /// Set when the revset may hold more changes beyond this page — the
    /// `skip` for the next query. A subsequent page may still come back
    /// empty, in which case it reports no further cursor.
    pub next_cursor: Option<usize>,
}

/// A bookmark, split into its local ref and per-remote refs as jj reports
/// them (`jj bookmark list --all`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjBookmark {
    pub name: String,
    /// `None` for the local bookmark; `Some(remote)` for a remote-tracking
    /// ref such as `origin`.
    pub remote: Option<String>,
    pub target_commit_id: JjCommitId,
    pub conflicted: bool,
}

impl JjBookmark {
    /// Whether this is the local half of a local/remote bookmark pair.
    pub fn is_local(&self) -> bool {
        self.remote.is_none()
    }
}

/// One entry of the jj operation log (`jj op log`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjOp {
    pub op_id: String,
    /// Human-readable summary, e.g. `check out git commit ...`.
    pub description: String,
    /// `user` field as jj renders it (name and/or email).
    pub user: String,
    /// Operation start time in Unix seconds.
    pub started_at_unix: i64,
}

/// A path with an unresolved conflict in the working-copy change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjConflict {
    pub path: String,
}

/// One path a change touches, from `jj diff --summary`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JjFileStat {
    /// The path after the change (the rename target, for renames).
    pub path: String,
    pub status: JjFileStatus,
}

/// The status letter `jj diff --summary` prefixes each path with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JjFileStatus {
    Added,
    Modified,
    Removed,
    /// `R {old => new}` — jj's brace rename notation; `path` carries the
    /// new name and `from` the old one.
    Renamed {
        from: String,
    },
    /// A path whose change carries conflict markers (the `C` letter).
    Conflict,
}

impl JjFileStatus {
    /// The single-letter label, as jj spells it.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Removed => "D",
            Self::Renamed { .. } => "R",
            Self::Conflict => "C",
        }
    }
}
