//! `GitRepository` status-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{CancellationToken, Result, StatusForPaths};
use crate::domain::{FileStatus, RepoStatus, UpstreamDivergence};
use crate::error::{Error, ErrorKind};
use std::path::Path;
use std::path::PathBuf;

pub trait GitRepositoryStatus {
    fn worktree_status(&self) -> Result<Vec<FileStatus>> {
        self.status().map(|status| status.unstaged)
    }

    fn worktree_status_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<FileStatus>> {
        cancellation.check_cancelled()?;
        let status = self.worktree_status()?;
        cancellation.check_cancelled()?;
        Ok(status)
    }

    fn staged_status(&self) -> Result<Vec<FileStatus>> {
        self.status().map(|status| status.staged)
    }

    fn staged_status_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<FileStatus>> {
        cancellation.check_cancelled()?;
        let status = self.staged_status()?;
        cancellation.check_cancelled()?;
        Ok(status)
    }

    fn status(&self) -> Result<RepoStatus>;

    /// Path-targeted status rescan: fresh entries for exactly `paths`
    /// (repo-relative), both lanes, or a signal that the shape is not
    /// mergeable (renames) and the caller should fall back to a full scan.
    fn status_for_paths(&self, _paths: &[PathBuf]) -> Result<StatusForPaths> {
        Err(Error::new(ErrorKind::Unsupported(
            "path-targeted status is not implemented for this backend",
        )))
    }

    fn status_cancellable(&self, cancellation: &CancellationToken) -> Result<RepoStatus> {
        cancellation.check_cancelled()?;
        let status = self.status()?;
        cancellation.check_cancelled()?;
        Ok(status)
    }

    fn upstream_divergence(&self) -> Result<Option<UpstreamDivergence>> {
        Ok(None)
    }

    fn upstream_divergence_cancellable(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Option<UpstreamDivergence>> {
        cancellation.check_cancelled()?;
        let divergence = self.upstream_divergence()?;
        cancellation.check_cancelled()?;
        Ok(divergence)
    }

    /// Paths currently marked assume-unchanged in the index. Cheap enough to
    /// list wholesale (`git ls-files -v`); entries tagged lowercase are the
    /// marked ones.
    fn assume_unchanged_list(&self) -> Result<Vec<PathBuf>> {
        let _ = self;
        Err(Error::new(ErrorKind::Unsupported(
            "assume-unchanged listing is not implemented for this backend",
        )))
    }

    /// Mark or unmark `path` assume-unchanged in the index
    /// (`git update-index --[no-]assume-unchanged`). Marked files drop out of
    /// status until the flag is cleared, so a status refresh must follow.
    fn set_assume_unchanged(&self, path: &Path, enable: bool) -> Result<()> {
        let _ = (self, path, enable);
        Err(Error::new(ErrorKind::Unsupported(
            "assume-unchanged update is not implemented for this backend",
        )))
    }
}
