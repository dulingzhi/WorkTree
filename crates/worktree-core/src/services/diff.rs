//! `GitRepository` diff-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{
    BlameLine, CancellationToken, CommandOutput, ConflictFileStages, ConflictSide, Result,
};
use crate::conflict_session::ConflictSession;
use crate::domain::{
    Diff, DiffArea, DiffPreviewTextSide, DiffTarget, FileDiffImage, FileDiffText, LfsPointerChange,
};
use crate::error::{Error, ErrorKind};
use std::path::Path;
use std::path::PathBuf;

pub trait GitRepositoryDiff {
    fn diff_unified(&self, target: &DiffTarget) -> Result<String>;

    /// The whole staged area as one unified diff — the payload AI
    /// commit-message generation summarizes. Unlike `diff_unified` it is not
    /// filtered to a single path.
    ///
    /// Default implementation reports the backend as unsupported so test
    /// doubles stay unaffected.
    fn staged_diff_unified(&self) -> Result<String> {
        Err(Error::new(ErrorKind::Unsupported(
            "staged diff is not implemented for this backend",
        )))
    }

    /// Load and parse unified diff rows for the target.
    ///
    /// Default implementation goes through `diff_unified`; backends may
    /// override for streaming parsing to avoid large monolithic allocations.
    fn diff_parsed(&self, target: &DiffTarget) -> Result<Diff> {
        self.diff_unified(target)
            .map(|text| Diff::from_unified(target.clone(), &text))
    }

    fn diff_parsed_cancellable(
        &self,
        target: &DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<Diff> {
        cancellation.check_cancelled()?;
        let diff = self.diff_parsed(target)?;
        cancellation.check_cancelled()?;
        Ok(diff)
    }

    fn diff_file_text(&self, _target: &DiffTarget) -> Result<Option<FileDiffText>> {
        Err(Error::new(ErrorKind::Unsupported(
            "file diff view is not implemented for this backend",
        )))
    }

    fn diff_file_text_cancellable(
        &self,
        target: &DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<Option<FileDiffText>> {
        cancellation.check_cancelled()?;
        let result = self.diff_file_text(target)?;
        cancellation.check_cancelled()?;
        Ok(result)
    }

    fn diff_preview_text_file(
        &self,
        _target: &DiffTarget,
        _side: DiffPreviewTextSide,
    ) -> Result<Option<PathBuf>> {
        Err(Error::new(ErrorKind::Unsupported(
            "preview text file loading is not implemented for this backend",
        )))
    }

    fn diff_preview_text_file_cancellable(
        &self,
        target: &DiffTarget,
        side: DiffPreviewTextSide,
        cancellation: &CancellationToken,
    ) -> Result<Option<PathBuf>> {
        cancellation.check_cancelled()?;
        let result = self.diff_preview_text_file(target, side)?;
        cancellation.check_cancelled()?;
        Ok(result)
    }

    fn diff_file_image(&self, _target: &DiffTarget) -> Result<Option<FileDiffImage>> {
        Err(Error::new(ErrorKind::Unsupported(
            "image diff view is not implemented for this backend",
        )))
    }

    fn diff_file_image_cancellable(
        &self,
        target: &DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<Option<FileDiffImage>> {
        cancellation.check_cancelled()?;
        let result = self.diff_file_image(target)?;
        cancellation.check_cancelled()?;
        Ok(result)
    }

    fn conflict_file_stages(&self, _path: &Path) -> Result<Option<ConflictFileStages>> {
        Err(Error::new(ErrorKind::Unsupported(
            "conflict stage reading is not implemented for this backend",
        )))
    }

    /// Build a backend-native conflict session for a conflicted path.
    ///
    /// Backends that support conflict stages and conflict-kind detection should
    /// return a populated session; unsupported backends return Unsupported.
    fn conflict_session(&self, _path: &Path) -> Result<Option<ConflictSession>> {
        Err(Error::new(ErrorKind::Unsupported(
            "conflict session loading is not implemented for this backend",
        )))
    }

    fn blame_file(&self, _path: &Path, _rev: Option<&str>) -> Result<Vec<BlameLine>> {
        Err(Error::new(ErrorKind::Unsupported(
            "git blame is not implemented for this backend",
        )))
    }

    /// Blame the working-tree content shown on the new side of a staged/unstaged
    /// diff. Lines matching committed history are attributed to their commit;
    /// lines not yet committed are returned as "Not Committed Yet" entries.
    fn blame_worktree_file(&self, _path: &Path, _area: DiffArea) -> Result<Vec<BlameLine>> {
        Err(Error::new(ErrorKind::Unsupported(
            "git blame of working-tree content is not implemented for this backend",
        )))
    }

    fn checkout_conflict_side(&self, _path: &Path, _side: ConflictSide) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "conflict resolution is not implemented for this backend",
        )))
    }

    /// Accept a conflict by explicitly deleting the path and staging removal.
    ///
    /// Used by decision/keep-delete resolvers when the chosen outcome is
    /// "accept deletion" rather than selecting a side's content.
    fn accept_conflict_deletion(&self, _path: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "conflict deletion is not implemented for this backend",
        )))
    }

    /// Restore a conflicted file from stage-1 (base) contents and stage it.
    ///
    /// Useful for decision-style conflicts where users want to explicitly
    /// recover the base version as the resolution result.
    fn checkout_conflict_base(&self, _path: &Path) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "base conflict checkout is not implemented for this backend",
        )))
    }

    /// Whether the repository has LFS wiring installed: the shared pre-push
    /// hook contains the `git lfs pre-push` marker `git lfs install` writes.
    /// Cheap file IO — no git invocation.
    fn lfs_enabled(&self) -> Result<bool> {
        Err(Error::new(ErrorKind::Unsupported(
            "lfs detection is not implemented for this backend",
        )))
    }

    /// Whether `path` carries a `filter=lfs` attribute from `.gitattributes`.
    fn lfs_is_filtered(&self, _path: &Path) -> Result<bool> {
        Err(Error::new(ErrorKind::Unsupported(
            "lfs attribute lookup is not implemented for this backend",
        )))
    }

    /// Old/new LFS pointers for the diff `target`, parsed from the unified
    /// diff of the pointer files. `Ok(None)` when the path's diff carries no
    /// pointer change. Callers gate this on [`Self::lfs_enabled`] and
    /// [`Self::lfs_is_filtered`].
    fn lfs_pointer_change(&self, _target: &DiffTarget) -> Result<Option<LfsPointerChange>> {
        Err(Error::new(ErrorKind::Unsupported(
            "lfs pointer diff is not implemented for this backend",
        )))
    }

    /// Turn LFS pointer bytes into the actual content by piping them through
    /// `git lfs smudge`.
    /// The new side of an LFS pointer change, smudged to its real content —
    /// the bytes an image preview renders. `Ok(None)` when the change has no
    /// new pointer side.
    fn lfs_new_side_smudged(&self, _target: &DiffTarget) -> Result<Option<Vec<u8>>> {
        Err(Error::new(ErrorKind::Unsupported(
            "lfs new-side smudge is not implemented for this backend",
        )))
    }

    fn lfs_smudge_bytes(&self, _input: &[u8]) -> Result<Vec<u8>> {
        Err(Error::new(ErrorKind::Unsupported(
            "lfs smudge is not implemented for this backend",
        )))
    }

    /// Repository cleanup: `git gc`, then `git lfs prune` when LFS is
    /// enabled. A prune failure is reported through the output, not as an
    /// error — an unreachable LFS remote must not fail the whole cleanup.
    fn cleanup_with_output(&self) -> Result<CommandOutput> {
        Err(Error::new(ErrorKind::Unsupported(
            "cleanup is not implemented for this backend",
        )))
    }
}
