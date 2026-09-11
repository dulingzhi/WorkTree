//! `GitRepository` remotes-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{
    CancellationToken, CommandOutput, ForcePushLease, MergeRequestPushOptions, PullMode,
    RemoteUrlKind, Result, SafePushAfterCommitContext, SafePushAfterCommitDecision,
};
use crate::domain::{CommitId, Remote, RemoteBranch};
use crate::error::{Error, ErrorKind};

pub trait GitRepositoryRemotes {
    fn head_commit_id(&self) -> Result<Option<CommitId>> {
        Err(Error::new(ErrorKind::Unsupported(
            "reading HEAD commit id is not implemented for this backend",
        )))
    }

    fn list_remotes(&self) -> Result<Vec<Remote>>;

    fn list_remotes_cancellable(&self, cancellation: &CancellationToken) -> Result<Vec<Remote>> {
        cancellation.check_cancelled()?;
        let remotes = self.list_remotes()?;
        cancellation.check_cancelled()?;
        Ok(remotes)
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>>;

    fn list_remote_branches_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RemoteBranch>> {
        cancellation.check_cancelled()?;
        let branches = self.list_remote_branches()?;
        cancellation.check_cancelled()?;
        Ok(branches)
    }

    fn prune_merged_branches_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "pruning merged branches is not implemented for this backend",
        )))
    }

    fn add_remote_with_output(&self, _name: &str, _url: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git remote add is not implemented for this backend",
        )))
    }

    fn remove_remote_with_output(&self, _name: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git remote remove is not implemented for this backend",
        )))
    }

    fn set_remote_url_with_output(
        &self,
        _name: &str,
        _url: &str,
        _kind: RemoteUrlKind,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git remote set-url is not implemented for this backend",
        )))
    }

    fn fetch_all(&self) -> Result<()>;

    fn pull(&self, mode: PullMode) -> Result<()>;

    fn push(&self) -> Result<()>;

    fn push_force(&self) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "force push is not implemented for this backend",
        )))
    }

    fn push_set_upstream(&self, _remote: &str, _branch: &str) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "pushing with --set-upstream is not implemented for this backend",
        )))
    }

    fn fetch_all_with_output(&self) -> Result<CommandOutput> {
        self.fetch_all()?;
        Ok(CommandOutput::empty_success("git fetch --all"))
    }

    fn fetch_all_with_output_prune(&self, _prune: bool) -> Result<CommandOutput> {
        self.fetch_all_with_output()
    }

    fn pull_with_output(&self, mode: PullMode) -> Result<CommandOutput> {
        self.pull(mode)?;
        Ok(CommandOutput::empty_success("git pull"))
    }

    fn push_with_output(&self) -> Result<CommandOutput> {
        self.push()?;
        Ok(CommandOutput::empty_success("git push"))
    }

    fn push_force_with_output(&self) -> Result<CommandOutput> {
        self.push_force()?;
        Ok(CommandOutput::empty_success("git push --force-with-lease"))
    }

    fn safe_push_after_commit(
        &self,
        _context: &SafePushAfterCommitContext,
    ) -> Result<SafePushAfterCommitDecision> {
        Err(Error::new(ErrorKind::Unsupported(
            "safe push after commit is not implemented for this backend",
        )))
    }

    fn push_force_with_lease_with_output(&self, lease: &ForcePushLease) -> Result<CommandOutput> {
        let _ = lease;
        Err(Error::new(ErrorKind::Unsupported(
            "oid-specific force push with lease is not implemented for this backend",
        )))
    }

    fn push_merge_request_with_output(
        &self,
        _options: &MergeRequestPushOptions,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "merge-request push options are not implemented for this backend",
        )))
    }

    fn push_set_upstream_with_output(&self, remote: &str, branch: &str) -> Result<CommandOutput> {
        self.push_set_upstream(remote, branch)?;
        Ok(CommandOutput::empty_success(format!(
            "git push --set-upstream {remote} HEAD:refs/heads/{branch}"
        )))
    }

    fn set_upstream_branch_with_output(
        &self,
        _branch: &str,
        _upstream: &str,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "setting a branch upstream is not implemented for this backend",
        )))
    }

    fn unset_upstream_branch_with_output(&self, _branch: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "unsetting a branch upstream is not implemented for this backend",
        )))
    }

    /// Fast-forward a branch to its configured upstream, refusing to move it
    /// when the update is not a fast-forward.
    fn fast_forward_branch_to_upstream_with_output(&self, _branch: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "fast-forwarding to the upstream branch is not implemented for this backend",
        )))
    }

    fn delete_remote_branch_with_output(
        &self,
        _remote: &str,
        _branch: &str,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "remote branch deletion is not implemented for this backend",
        )))
    }

    /// Delete several branches on one remote.
    ///
    /// A batch method rather than a caller-side loop because deleting is a push:
    /// one invocation carrying every ref is a single network round trip, where
    /// the loop pays one per branch. The default keeps that loop so backends
    /// that only implement the single-branch call stay correct.
    fn delete_remote_branches_with_output(
        &self,
        remote: &str,
        branches: &[String],
    ) -> Result<CommandOutput> {
        let mut last = CommandOutput::empty_success("git push --delete");
        for branch in branches {
            last = self.delete_remote_branch_with_output(remote, branch)?;
        }
        Ok(last)
    }

    fn pull_branch_with_output(&self, _remote: &str, _branch: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "pulling a specific remote branch is not implemented for this backend",
        )))
    }

    fn merge_ref_with_output(&self, _reference: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "merging a specific ref is not implemented for this backend",
        )))
    }

    fn squash_ref_with_output(&self, _reference: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "squashing a specific ref is not implemented for this backend",
        )))
    }

    /// Persist (`Some`) or clear (`None`) the per-repo SSH key recorded for
    /// `remote` in the repository's git config (`remote.<name>.sshkey`).
    fn set_remote_ssh_key_with_output(
        &self,
        _remote: &str,
        _key: Option<&str>,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "remote ssh key config is not implemented for this backend",
        )))
    }
}
