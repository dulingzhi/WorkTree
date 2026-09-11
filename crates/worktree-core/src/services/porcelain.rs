//! `GitRepository` porcelain-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{CancellationToken, CommandOutput, CommitOperationOutcome, MergetoolResult, Result};
use crate::domain::{Branch, CommitId, RefMetadata, RemoteTag, StashEntry, Tag};
use crate::error::{Error, ErrorKind};
use std::path::Path;
use std::path::PathBuf;

pub trait GitRepositoryPorcelain {
    fn current_branch(&self) -> Result<String>;

    fn current_branch_cancellable(&self, cancellation: &CancellationToken) -> Result<String> {
        cancellation.check_cancelled()?;
        let branch = self.current_branch()?;
        cancellation.check_cancelled()?;
        Ok(branch)
    }

    fn list_branches(&self) -> Result<Vec<Branch>>;

    fn list_branches_cancellable(&self, cancellation: &CancellationToken) -> Result<Vec<Branch>> {
        cancellation.check_cancelled()?;
        let branches = self.list_branches()?;
        cancellation.check_cancelled()?;
        Ok(branches)
    }

    fn list_tags(&self) -> Result<Vec<Tag>> {
        Err(Error::new(ErrorKind::Unsupported(
            "tag listing is not implemented for this backend",
        )))
    }

    fn list_tags_cancellable(&self, cancellation: &CancellationToken) -> Result<Vec<Tag>> {
        cancellation.check_cancelled()?;
        let tags = self.list_tags()?;
        cancellation.check_cancelled()?;
        Ok(tags)
    }

    fn list_remote_tags(&self) -> Result<Vec<RemoteTag>> {
        Err(Error::new(ErrorKind::Unsupported(
            "remote tag listing is not implemented for this backend",
        )))
    }

    fn list_remote_tags_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RemoteTag>> {
        cancellation.check_cancelled()?;
        let tags = self.list_remote_tags()?;
        cancellation.check_cancelled()?;
        Ok(tags)
    }

    fn create_branch(&self, name: &str, target: &CommitId) -> Result<()>;

    fn rename_branch(&self, _old_name: &str, _new_name: &str) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "branch renaming is not implemented for this backend",
        )))
    }

    fn delete_branch(&self, name: &str) -> Result<()>;

    fn delete_branch_force(&self, _name: &str) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "force branch deletion is not implemented for this backend",
        )))
    }

    fn checkout_branch(&self, name: &str) -> Result<()>;

    fn checkout_remote_branch(
        &self,
        _remote: &str,
        _branch: &str,
        _local_branch: &str,
    ) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "remote branch checkout is not implemented for this backend",
        )))
    }

    /// Checks out GitHub pull request `number` from `remote` (`git fetch
    /// <remote> refs/pull/<number>/head` + a local `pr/<number>` branch).
    fn checkout_pull_request(&self, _remote: &str, _number: u64) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "pull request checkout is not implemented for this backend",
        )))
    }

    fn checkout_commit(&self, id: &CommitId) -> Result<()>;

    fn cherry_pick(&self, id: &CommitId) -> Result<()>;

    fn revert(&self, id: &CommitId) -> Result<()>;

    /// Creates a stash. `paths` restricts the stash to those worktree
    /// paths (Git pathspecs, repo-relative); empty stashes everything.
    fn stash_create(
        &self,
        message: &str,
        include_untracked: bool,
        keep_index: bool,
        paths: &[PathBuf],
    ) -> Result<()>;

    fn stash_list(&self) -> Result<Vec<StashEntry>>;

    fn stash_list_cancellable(&self, cancellation: &CancellationToken) -> Result<Vec<StashEntry>> {
        cancellation.check_cancelled()?;
        let stashes = self.stash_list()?;
        cancellation.check_cancelled()?;
        Ok(stashes)
    }

    fn stash_apply(&self, index: usize) -> Result<()>;

    fn stash_drop(&self, index: usize) -> Result<()>;

    /// Checks out the stash's base commit as a new branch and applies the
    /// stash there, dropping it on success.
    fn stash_branch(&self, _branch: &str, _index: usize) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "git stash branch is not implemented for this backend",
        )))
    }

    fn stage(&self, paths: &[&Path]) -> Result<()>;

    fn unstage(&self, paths: &[&Path]) -> Result<()>;

    fn commit(&self, message: &str) -> Result<()>;

    fn commit_with_outcome(&self, message: &str) -> Result<CommitOperationOutcome> {
        self.commit(message)?;
        Ok(CommitOperationOutcome::default())
    }

    fn commit_amend(&self, _message: &str) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "commit amend is not implemented for this backend",
        )))
    }

    fn commit_amend_with_outcome(&self, message: &str) -> Result<CommitOperationOutcome> {
        self.commit_amend(message)?;
        Ok(CommitOperationOutcome::default())
    }

    fn create_tag_with_output(
        &self,
        _name: &str,
        _target: &str,
        _message: Option<&str>,
        _annotated: bool,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git tag creation is not implemented for this backend",
        )))
    }

    fn delete_tag_with_output(&self, _name: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "git tag deletion is not implemented for this backend",
        )))
    }

    fn prune_local_tags_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "pruning local tags is not implemented for this backend",
        )))
    }

    fn push_tag_with_output(&self, _remote: &str, _name: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "pushing tags is not implemented for this backend",
        )))
    }

    fn delete_remote_tag_with_output(&self, _remote: &str, _name: &str) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "remote tag deletion is not implemented for this backend",
        )))
    }

    fn commit_amend_with_output(&self, message: &str) -> Result<CommandOutput> {
        self.commit_amend(message)?;
        Ok(CommandOutput::empty_success("git commit --amend"))
    }

    /// Launch an external mergetool for a conflicted file.
    ///
    /// Materializes BASE, LOCAL, REMOTE temp files from the conflict stages,
    /// invokes the configured (or specified) mergetool, reads back the merged
    /// output, writes it to the worktree, and stages the result.
    ///
    /// `preference` is the app-level external-merge-tool selection from the
    /// UI layer; `FromGitConfig` keeps the git-config-driven behavior.
    fn launch_mergetool(
        &self,
        _path: &Path,
        _preference: &crate::external_merge_tool::ExternalMergeToolSelection,
    ) -> Result<MergetoolResult> {
        Err(Error::new(ErrorKind::Unsupported(
            "external mergetool is not implemented for this backend",
        )))
    }

    /// Write a zip archive of `revision`'s tree to `dest`
    /// (`git archive --format=zip --output=<dest> <revision>`).
    fn archive_zip_with_output(&self, _revision: &str, _dest: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "archive export is not implemented for this backend",
        )))
    }

    /// Tip-commit author/date/summary for every local and remote-tracking ref,
    /// as `(short refname, metadata)` pairs. Purely decorative — callers render
    /// name-only rows when this is unavailable, so backends may leave it
    /// unimplemented.
    fn list_ref_metadata(&self) -> Result<Vec<(String, RefMetadata)>> {
        Err(Error::new(ErrorKind::Unsupported(
            "ref metadata listing is not implemented for this backend",
        )))
    }

    fn list_ref_metadata_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<(String, RefMetadata)>> {
        cancellation.check_cancelled()?;
        let metadata = self.list_ref_metadata()?;
        cancellation.check_cancelled()?;
        Ok(metadata)
    }

    fn discard_worktree_changes(&self, paths: &[&Path]) -> Result<()>;
}
