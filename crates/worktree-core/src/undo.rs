//! Operation-level undo over the HEAD reflog.
//!
//! `git reset` is git's universal undo, and the reflog records where HEAD sat
//! before every operation — so undoing a completed operation is "move the
//! branch back to the recorded position", with the reset mode choosing how
//! much of the working tree comes along. This module classifies the newest
//! reflog entry into one of the operations whose reverse we know how to
//! express, and resolves the position to return to.
//!
//! In-progress operations (a conflicted merge or rebase) are NOT handled
//! here: their reverse is an abort, not a ref move, and the UI knows those
//! states from its own loaded flags — see the popover that consumes this.

use crate::domain::ReflogEntry;
use crate::services::ResetMode;
use std::sync::Arc;

/// The operation a reflog entry records, restricted to the kinds whose
/// reverse we can express as a single reset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UndoKind {
    /// `reset: moving to …` — undo returns to the pre-reset position.
    Reset,
    /// `merge <name>: …` (a completed merge) — undo returns to the first
    /// parent's tip, the position before the merge commit was created.
    Merge,
    /// `pull: …` (a merge-shaped pull) — undo returns to the pre-pull tip.
    Pull,
    /// `rebase (start|pick|continue|finish): …` — undo returns to the entry
    /// that precedes the whole rebase run.
    Rebase,
    /// `commit: …` / `commit (merge): …` — undo returns to the parent.
    Commit,
}

impl UndoKind {
    /// Stable, human-readable name for logs and tests.
    pub fn as_str(&self) -> &'static str {
        match self {
            UndoKind::Reset => "reset",
            UndoKind::Merge => "merge",
            UndoKind::Pull => "pull",
            UndoKind::Rebase => "rebase",
            UndoKind::Commit => "commit",
        }
    }
}

/// How to carry the undo out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UndoAction {
    /// Move the current branch back to `target` (the position recorded
    /// before the operation). `default_mode` is the suggested reset mode —
    /// the safety preview lets the user soften it before confirming.
    ResetBack {
        target: Arc<str>,
        default_mode: ResetMode,
    },
}

/// One undoable operation, resolved from the reflog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoPlan {
    pub kind: UndoKind,
    /// The reflog message of the operation being undone, verbatim — it is
    /// already the human summary git wrote (`merge main: Fast-forward`).
    pub operation: Arc<str>,
    pub action: UndoAction,
}

/// Classifies the newest reflog entry and resolves where undo should land.
///
/// `entries` is newest-first (index 0 is `HEAD@{0}`), the order
/// [`crate::services::GitRepository::reflog_head`] returns. Returns `None`
/// when there is nothing to undo: fewer than two entries, or an operation
/// whose reverse this doesn't model (a checkout, a fetch, a stash, …).
pub fn classify_undo(entries: &[ReflogEntry]) -> Option<UndoPlan> {
    let (last, rest) = entries.split_first()?;
    let previous = rest.first()?;
    let message = last.message.as_ref();

    // A completed rebase spans several reflog entries ((start), (pick)…,
    // (finish)); the position to return to is the one before the run began,
    // not `entries[1]` — that is the last picked commit of the NEW chain.
    if message.starts_with("rebase") {
        let target = rest
            .iter()
            .find(|entry| !entry.message.starts_with("rebase"))?;
        return Some(UndoPlan {
            kind: UndoKind::Rebase,
            operation: Arc::clone(&last.message),
            action: UndoAction::ResetBack {
                target: Arc::clone(&target.new_id.0),
                // Going back past a rebase means abandoning the rewritten
                // commits wholesale; mixed would only smear them into the
                // worktree as unstaged changes.
                default_mode: ResetMode::Hard,
            },
        });
    }

    let (kind, default_mode) = if let Some(rest) = message.strip_prefix("commit") {
        // Covers both `commit: ` and `commit (merge): ` — and rejects
        // look-alikes such as `commits: batch`.
        if !rest.starts_with(": ") && !rest.starts_with(" (") {
            return None;
        }
        // Undoing a commit usually means "uncommit but keep my work staged".
        (UndoKind::Commit, ResetMode::Soft)
    } else if message.starts_with("merge ") {
        (UndoKind::Merge, ResetMode::Mixed)
    } else if message.starts_with("pull: ") {
        (UndoKind::Pull, ResetMode::Mixed)
    } else if message.starts_with("reset: moving to ") {
        // A reset already decided the worktree's fate; undo restores exactly.
        (UndoKind::Reset, ResetMode::Hard)
    } else {
        return None;
    };

    Some(UndoPlan {
        kind,
        operation: Arc::clone(&last.message),
        action: UndoAction::ResetBack {
            target: Arc::clone(&previous.new_id.0),
            default_mode,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::CommitId;
    use std::time::SystemTime;

    fn entry(index: usize, sha: &str, message: &str) -> ReflogEntry {
        ReflogEntry {
            index,
            new_id: CommitId(sha.into()),
            message: message.into(),
            time: Some(SystemTime::UNIX_EPOCH),
            selector: format!("HEAD@{{{index}}}").into(),
            author: "Alice".into(),
        }
    }

    fn target(plan: &UndoPlan) -> &str {
        match &plan.action {
            UndoAction::ResetBack { target, .. } => target.as_ref(),
        }
    }

    fn default_mode(plan: &UndoPlan) -> ResetMode {
        match &plan.action {
            UndoAction::ResetBack { default_mode, .. } => *default_mode,
        }
    }

    #[test]
    fn fewer_than_two_entries_have_nothing_to_undo() {
        assert!(classify_undo(&[]).is_none());
        assert!(classify_undo(&[entry(0, "a", "commit: x")]).is_none());
    }

    #[test]
    fn checkout_and_unknown_operations_are_not_undoable() {
        let entries = vec![
            entry(0, "b", "checkout: moving from main to feature"),
            entry(1, "a", "commit: x"),
        ];
        assert!(classify_undo(&entries).is_none());
        let entries = vec![entry(0, "b", "stash: WIP on main"), entry(1, "a", "commit: x")];
        assert!(classify_undo(&entries).is_none());
    }

    #[test]
    fn commit_prefix_matches_plain_and_merge_commits() {
        for message in ["commit: add feature", "commit (merge): merge branch 'x'"] {
            let entries = vec![entry(0, "b", message), entry(1, "a", "reset: moving to HEAD")];
            let plan = classify_undo(&entries).unwrap_or_else(|| panic!("{message}"));
            assert_eq!(plan.kind, UndoKind::Commit);
            assert_eq!(plan.operation.as_ref(), message);
            assert_eq!(target(&plan), "a");
            assert_eq!(default_mode(&plan), ResetMode::Soft);
        }
    }

    #[test]
    fn commit_like_words_do_not_match() {
        // `commits.`-style messages (custom hooks) must not classify as commits.
        let entries = vec![entry(0, "b", "commits: batch"), entry(1, "a", "commit: x")];
        assert!(classify_undo(&entries).is_none());
    }

    #[test]
    fn merge_pull_and_reset_return_to_the_previous_position() {
        for (message, kind, mode) in [
            ("merge main: Fast-forward", UndoKind::Merge, ResetMode::Mixed),
            (
                "merge origin/main: Merge made by the 'ort' strategy.",
                UndoKind::Merge,
                ResetMode::Mixed,
            ),
            ("pull: Fast-forward", UndoKind::Pull, ResetMode::Mixed),
            (
                "pull: Merge made by the 'ort' strategy.",
                UndoKind::Pull,
                ResetMode::Mixed,
            ),
            ("reset: moving to origin/main", UndoKind::Reset, ResetMode::Hard),
        ] {
            let entries = vec![entry(0, "b", message), entry(1, "a", "commit: base")];
            let plan = classify_undo(&entries).unwrap_or_else(|| panic!("{message}"));
            assert_eq!(plan.kind, kind, "{message}");
            assert_eq!(plan.operation.as_ref(), message);
            assert_eq!(target(&plan), "a", "{message}");
            assert_eq!(default_mode(&plan), mode, "{message}");
        }
    }

    #[test]
    fn rebase_undo_targets_the_position_before_the_whole_run() {
        let entries = vec![
            entry(0, "new3", "rebase (finish): returning to refs/heads/topic"),
            entry(1, "new2", "rebase (pick): third"),
            entry(2, "new1", "rebase (pick): second"),
            entry(3, "onto", "rebase (start): checkout abc1234"),
            entry(4, "oldtip", "commit: base"),
        ];
        let plan = classify_undo(&entries).expect("completed rebase is undoable");
        assert_eq!(plan.kind, UndoKind::Rebase);
        assert_eq!(target(&plan), "oldtip");
        assert_eq!(default_mode(&plan), ResetMode::Hard);
    }

    #[test]
    fn a_rebase_run_that_exhausts_the_window_is_not_undoable() {
        // The pre-rebase position fell outside the loaded reflog window, so
        // there is no safe target to return to.
        let entries = vec![
            entry(0, "new2", "rebase (finish): returning to refs/heads/topic"),
            entry(1, "new1", "rebase (pick): second"),
            entry(2, "onto", "rebase (start): checkout abc1234"),
        ];
        assert!(classify_undo(&entries).is_none());
    }
}
