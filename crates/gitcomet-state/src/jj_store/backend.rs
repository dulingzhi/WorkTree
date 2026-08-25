//! How the jj store obtains repository handles — the seam the tests use to
//! substitute an in-memory repository.

use std::path::Path;
use std::sync::Arc;

use gitcomet_core::services::Result;
use gitcomet_jj_core::{JjCliRepository, JjRepository};

/// Opens jj repository handles for workdirs.
pub trait JjBackend: Send + Sync {
    fn open(&self, workdir: &Path) -> Result<Arc<dyn JjRepository>>;
}

/// The production backend: the jj CLI.
#[derive(Clone, Copy, Debug, Default)]
pub struct CliJjBackend;

impl JjBackend for CliJjBackend {
    fn open(&self, workdir: &Path) -> Result<Arc<dyn JjRepository>> {
        Ok(Arc::new(JjCliRepository::open(workdir)?))
    }
}
