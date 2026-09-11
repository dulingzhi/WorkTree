//! `GitRepository` worktree-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{
    CancellationToken, CommandOutput, Result, SubmoduleTrustDecision, SubmoduleTrustTarget,
};
use crate::domain::{CommitId, FileEntry, Submodule, Worktree};
use crate::error::{Error, ErrorKind};
use std::path::Path;

pub trait GitRepositoryWorktree {
    fn export_patch_with_output(
        &self,
        _commit_id: &CommitId,
        _dest: &Path,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "patch export is not implemented for this backend",
        )))
    }

    fn apply_patch_with_output(&self, _patch: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "patch apply is not implemented for this backend",
        )))
    }

    fn apply_unified_patch_to_index_with_output(
        &self,
        _patch: &str,
        _reverse: bool,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "index patch apply is not implemented for this backend",
        )))
    }

    fn apply_unified_patch_to_worktree_with_output(
        &self,
        _patch: &str,
        _reverse: bool,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree patch apply is not implemented for this backend",
        )))
    }

    fn list_worktrees(&self) -> Result<Vec<Worktree>> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree listing is not implemented for this backend",
        )))
    }

    fn list_worktrees_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Worktree>> {
        cancellation.check_cancelled()?;
        let worktrees = self.list_worktrees()?;
        cancellation.check_cancelled()?;
        Ok(worktrees)
    }

    fn add_worktree_with_output(
        &self,
        _path: &Path,
        _reference: Option<&str>,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree add is not implemented for this backend",
        )))
    }

    fn remove_worktree_with_output(&self, _path: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree remove is not implemented for this backend",
        )))
    }

    fn force_remove_worktree_with_output(&self, _path: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree force remove is not implemented for this backend",
        )))
    }

    fn list_submodules(&self) -> Result<Vec<Submodule>> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule listing is not implemented for this backend",
        )))
    }

    fn list_submodules_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Submodule>> {
        cancellation.check_cancelled()?;
        let submodules = self.list_submodules()?;
        cancellation.check_cancelled()?;
        Ok(submodules)
    }

    /// The working directory as it is on disk, not `HEAD`'s tree.
    fn list_worktree_files(&self) -> Result<Vec<FileEntry>> {
        Err(Error::new(ErrorKind::Unsupported(
            "worktree file listing is not implemented for this backend",
        )))
    }

    fn list_tree_files_at_commit(&self, _commit_id: &CommitId) -> Result<Vec<FileEntry>> {
        Err(Error::new(ErrorKind::Unsupported(
            "tree file listing at commit is not implemented for this backend",
        )))
    }

    fn submodule_diff_summary(
        &self,
        _target: &crate::domain::DiffTarget,
    ) -> Result<crate::domain::SubmoduleDiffSummary> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule diff summary is not implemented for this backend",
        )))
    }

    fn submodule_diff_summary_cancellable(
        &self,
        target: &crate::domain::DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<crate::domain::SubmoduleDiffSummary> {
        cancellation.check_cancelled()?;
        let summary = self.submodule_diff_summary(target)?;
        cancellation.check_cancelled()?;
        Ok(summary)
    }

    fn check_submodule_add_trust(
        &self,
        _url: &str,
        _path: &Path,
    ) -> Result<SubmoduleTrustDecision> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule trust checks are not implemented for this backend",
        )))
    }

    fn check_submodule_update_trust(&self) -> Result<SubmoduleTrustDecision> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule trust checks are not implemented for this backend",
        )))
    }

    fn check_submodule_load_trust(&self, _path: &Path) -> Result<SubmoduleTrustDecision> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule trust checks are not implemented for this backend",
        )))
    }

    fn add_submodule_with_output(
        &self,
        _url: &str,
        _path: &Path,
        _branch: Option<&str>,
        _name: Option<&str>,
        _force: bool,
        _approved_sources: &[SubmoduleTrustTarget],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule add is not implemented for this backend",
        )))
    }

    fn update_submodules_with_output(
        &self,
        _approved_sources: &[SubmoduleTrustTarget],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule update is not implemented for this backend",
        )))
    }

    fn load_submodule_with_output(
        &self,
        _path: &Path,
        _approved_sources: &[SubmoduleTrustTarget],
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule update is not implemented for this backend",
        )))
    }

    fn change_submodule_pointer_with_output(
        &self,
        _path: &Path,
        _reference: &str,
    ) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule pointer changes are not implemented for this backend",
        )))
    }

    fn remove_submodule_with_output(&self, _path: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "submodule remove is not implemented for this backend",
        )))
    }
}
