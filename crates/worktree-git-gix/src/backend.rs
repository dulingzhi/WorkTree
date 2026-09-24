use crate::repo::GixRepo;
use std::path::Path;
use std::sync::Arc;
use worktree_core::error::{Error, ErrorKind};
use worktree_core::path_utils::strip_windows_verbatim_prefix;
use worktree_core::services::{CancellationToken, GitBackend, GitRepository, Result};

pub struct GixBackend;

impl Default for GixBackend {
    fn default() -> Self {
        Self
    }
}

impl GixBackend {
    fn open_impl(
        &self,
        workdir: &Path,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Arc<dyn GitRepository>> {
        // Publish the history-cache hooks the first time a repository is opened.
        // This (optional) crate is the only thing that knows about the cache, but
        // its consumers — the app binary, the bench harness, the tests — reach it
        // through `worktree_core`, so the registration has to happen from here.
        crate::install_history_cache_hooks();
        if let Some(cancellation) = cancellation {
            cancellation.check_cancelled()?;
        }

        let workdir = strip_windows_verbatim_prefix(
            workdir
                .canonicalize()
                .map_err(|e| Error::new(ErrorKind::Io(e.kind())))?,
        );
        if let Some(cancellation) = cancellation {
            cancellation.check_cancelled()?;
        }

        let repo = crate::open::open_worktree_repo(&workdir).map_err(|e| match e {
            gix::open::Error::NotARepository { .. } => Error::new(ErrorKind::NotARepository),
            gix::open::Error::Io(io) => Error::new(ErrorKind::Io(io.kind())),
            e => Error::new(ErrorKind::Backend(format!("gix open: {e}"))),
        })?;
        if let Some(cancellation) = cancellation {
            cancellation.check_cancelled()?;
        }

        Ok(Arc::new(GixRepo::new(workdir, repo.into_sync())))
    }
}

impl GitBackend for GixBackend {
    fn open(&self, workdir: &Path) -> Result<Arc<dyn GitRepository>> {
        self.open_impl(workdir, None)
    }

    fn open_cancellable(
        &self,
        workdir: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn GitRepository>> {
        self.open_impl(workdir, Some(cancellation))
    }
}
