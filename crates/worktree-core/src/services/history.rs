//! `GitRepository` history-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{
    BisectState, BisectVerdict, CancellationToken, CommandOutput, InteractiveRebaseEntry,
    ResetMode, Result, SequencerState,
};
use crate::domain::CommitId;
use crate::error::{Error, ErrorKind};

pub trait GitRepositoryHistory {
    /// Runs a single cherry-pick. `mainline` is Git's 1-based parent number
    /// for a merge commit and must be `None` for non-merge commits.
    fn cherry_pick_with_output(
        &self,
        _id: &CommitId,
        _commit: bool,
        _mainline: Option<usize>,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git cherry-pick is not implemented for this backend",
        )))
    }

    fn rebase_with_output(&self, _onto: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git rebase is not implemented for this backend",
        )))
    }

    fn rebase_continue_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git rebase --continue is not implemented for this backend",
        )))
    }

    fn rebase_abort_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git rebase --abort is not implemented for this backend",
        )))
    }

    fn list_commits_for_interactive_rebase(
        &self,
        _base: &str,
    ) -> Result<Vec<InteractiveRebaseEntry>> {
        Err(Error::new(ErrorKind::Unsupported(
            "listing commits for interactive rebase is not implemented for this backend",
        )))
    }

    fn interactive_rebase_with_output(
        &self,
        _base: &str,
        _entries: &[InteractiveRebaseEntry],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git rebase -i is not implemented for this backend",
        )))
    }

    fn interactive_cherry_pick_with_output(
        &self,
        _entries: &[InteractiveRebaseEntry],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "interactive cherry-pick is not implemented for this backend",
        )))
    }

    fn merge_abort_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git merge --abort is not implemented for this backend",
        )))
    }

    fn rebase_in_progress(&self) -> Result<bool> {
        Ok(false)
    }

    fn rebase_in_progress_cancellable(&self, cancellation: &CancellationToken) -> Result<bool> {
        cancellation.check_cancelled()?;
        let in_progress = self.rebase_in_progress()?;
        cancellation.check_cancelled()?;
        Ok(in_progress)
    }

    fn sequencer_state(&self) -> Result<SequencerState> {
        Ok(if self.rebase_in_progress()? {
            SequencerState::RebaseOrApply
        } else {
            SequencerState::None
        })
    }

    fn sequencer_state_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<SequencerState> {
        cancellation.check_cancelled()?;
        let state = self.sequencer_state()?;
        cancellation.check_cancelled()?;
        Ok(state)
    }

    /// Parsed state of the in-progress bisect session, or `None` when the
    /// repository is not bisecting.
    fn bisect_state(&self) -> Result<Option<BisectState>> {
        Ok(None)
    }

    fn bisect_state_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Option<BisectState>> {
        cancellation.check_cancelled()?;
        let state = self.bisect_state()?;
        cancellation.check_cancelled()?;
        Ok(state)
    }

    fn bisect_start_with_output(
        &self,
        _bad: Option<&str>,
        _goods: &[String],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git bisect start is not implemented for this backend",
        )))
    }

    fn bisect_mark_with_output(
        &self,
        _verdict: BisectVerdict,
        _commit: Option<&str>,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git bisect mark is not implemented for this backend",
        )))
    }

    fn bisect_reset_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git bisect reset is not implemented for this backend",
        )))
    }

    fn merge_commit_message(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn merge_commit_message_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Option<String>> {
        cancellation.check_cancelled()?;
        let message = self.merge_commit_message()?;
        cancellation.check_cancelled()?;
        Ok(message)
    }

    /// Builds the default combined message for squashing the linear commit
    /// range `oldest..=head`: the oldest commit's full message first, younger
    /// messages appended as paragraphs.
    fn squash_message_preview(&self, _oldest: &CommitId, _head: &CommitId) -> Result<String> {
        Err(Error::new(ErrorKind::Unsupported(
            "squashing commits is not implemented for this backend",
        )))
    }

    /// Squashes the linear first-parent range `oldest..=expected_head` (which
    /// must end at the current HEAD) into a single commit carrying `message`,
    /// preserving the oldest commit's author. Must not touch the worktree or
    /// index, and must fail without changing refs when HEAD no longer equals
    /// `expected_head`.
    fn squash_commits_with_output(
        &self,
        _oldest: &CommitId,
        _expected_head: &CommitId,
        _message: &str,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "squashing commits is not implemented for this backend",
        )))
    }

    fn reset_with_output(&self, _target: &str, _mode: ResetMode) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git reset is not implemented for this backend",
        )))
    }
}
