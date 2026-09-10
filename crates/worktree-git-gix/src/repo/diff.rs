use super::{
    GixRepo,
    conflict_stages::{
        ConflictStageData, gix_index_conflict_stage_data, gix_index_stage_object_id_optional,
    },
};
use crate::util::{
    git_command_failed_error, run_git_parsed_stdout, run_git_parsed_stdout_cancellable,
    run_git_raw_output,
};
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use worktree_core::conflict_session::{
    ConflictPayload, ConflictResolverStrategy, ConflictSession, canonicalize_stage_parts,
};
use worktree_core::domain::{
    Diff, DiffArea, DiffPreviewTextSide, DiffTarget, FileDiffImage, FileDiffText,
    FileDiffTextSource,
};
use worktree_core::error::{Error, ErrorKind};
use worktree_core::path_utils::strip_windows_verbatim_prefix;
use worktree_core::services::{CancellationToken, ConflictFileStages, Result};

#[cfg(not(test))]
const MAX_IMAGE_DIFF_SIDE_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(test)]
const MAX_IMAGE_DIFF_SIDE_BYTES: u64 = 1024;

impl GixRepo {
    /// The shared `git -c …` prefix every unified-diff invocation starts
    /// with: format pins the diff header so a user's git config cannot
    /// reshape what the UI (or a prompt) reads out of it.
    fn unified_diff_config_command(&self) -> Command {
        let mut cmd = self.git_workdir_cmd();
        cmd.arg("-c").arg("color.ui=false");
        // Pin the header format: the UI resolves a file's path out of the
        // `diff --git` / `---` / `+++` lines, and these settings are the ones a
        // user's git config could otherwise use to reshape them.
        cmd.arg("-c")
            // Keeps non-ASCII names as literal UTF-8 instead of octal escapes.
            .arg("core.quotepath=false")
            .arg("-c")
            .arg("diff.mnemonicPrefix=false")
            .arg("-c")
            .arg("diff.noprefix=false")
            .arg("-c")
            .arg("diff.srcPrefix=a/")
            .arg("-c")
            .arg("diff.dstPrefix=b/");
        cmd.arg("--no-pager");
        cmd
    }

    pub(super) fn build_unified_diff_command(&self, target: &DiffTarget) -> Command {
        let mut cmd = self.unified_diff_config_command();

        match target {
            DiffTarget::WorkingTree { path, area } => {
                cmd.arg("diff").arg("--no-ext-diff");
                if matches!(area, DiffArea::Unstaged) {
                    // Match the staged view on Windows by suppressing CR-at-EOL-only
                    // worktree noise before content is normalized into the index.
                    cmd.arg("--ignore-cr-at-eol");
                }
                if matches!(area, DiffArea::Staged) {
                    cmd.arg("--cached");
                }
                cmd.arg("--").arg(path);
            }
            DiffTarget::Commit { commit_id, path } => {
                cmd.arg("show")
                    .arg("--no-ext-diff")
                    .arg("--pretty=format:")
                    .arg(commit_id.as_ref());
                if let Some(path) = path {
                    cmd.arg("--").arg(path);
                }
            }
            DiffTarget::CommitRange {
                from_commit_id,
                to_commit_id,
                path,
            } => {
                cmd.arg("diff")
                    .arg("--no-ext-diff")
                    .arg(from_commit_id.as_ref());
                // `None` tip: `git diff <from>` compares against the working tree.
                if let Some(to_commit_id) = to_commit_id {
                    cmd.arg(to_commit_id.as_ref());
                }
                if let Some(path) = path {
                    cmd.arg("--").arg(path);
                }
            }
        }

        cmd
    }

    pub(super) fn diff_unified(&self, target: &DiffTarget) -> Result<String> {
        self.run_unified_diff(self.build_unified_diff_command(target))
    }

    /// The whole staged area as one unified diff (`git diff --cached`), the
    /// payload AI commit-message generation summarizes. Unlike
    /// [`Self::diff_unified`] it is not filtered to a single path.
    pub(super) fn staged_diff_unified(&self) -> Result<String> {
        let mut cmd = self.unified_diff_config_command();
        cmd.arg("diff").arg("--no-ext-diff").arg("--cached");
        self.run_unified_diff(cmd)
    }

    /// Run a unified-diff command and collect its stdout. Shared by every
    /// caller so the "exit 1 means differences, not failure" rule and the
    /// UTF-8 requirement stay in one place.
    pub(super) fn run_unified_diff(&self, cmd: Command) -> Result<String> {
        let label = "git diff";
        let output = run_git_raw_output(cmd, label)?;

        // git diff exits 1 when there are differences — that is not a failure.
        let ok_exit = output.status.success() || output.status.code() == Some(1);
        if !ok_exit {
            return Err(git_command_failed_error(label, output));
        }

        String::from_utf8(output.stdout).map_err(|_| {
            Error::new(ErrorKind::Backend(
                "git diff produced non-UTF-8 output".to_string(),
            ))
        })
    }

    pub(super) fn diff_parsed(&self, target: &DiffTarget) -> Result<Diff> {
        if let Some(diff) = self.synthetic_simple_commit_path_diff(target)? {
            return Ok(diff);
        }

        let target = target.clone();
        run_git_parsed_stdout(
            self.build_unified_diff_command(&target),
            "git diff",
            true,
            move |stdout| {
                Diff::from_unified_reader(target, BufReader::new(stdout)).map_err(|err| {
                    Error::new(ErrorKind::Backend(format!(
                        "git diff produced non-UTF-8 output: {err}"
                    )))
                })
            },
        )
    }

    pub(super) fn diff_parsed_cancellable(
        &self,
        target: &DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<Diff> {
        cancellation.check_cancelled()?;
        if let Some(diff) = self.synthetic_simple_commit_path_diff(target)? {
            cancellation.check_cancelled()?;
            return Ok(diff);
        }

        let target = target.clone();
        run_git_parsed_stdout_cancellable(
            self.build_unified_diff_command(&target),
            "git diff",
            true,
            cancellation,
            move |stdout| {
                Diff::from_unified_reader(target, BufReader::new(stdout)).map_err(|err| {
                    Error::new(ErrorKind::Backend(format!(
                        "git diff produced non-UTF-8 output: {err}"
                    )))
                })
            },
        )
    }

    fn file_diff_source_from_blob_id(
        &self,
        blob_id: gix::ObjectId,
        logical_path: &Path,
    ) -> Result<Option<FileDiffTextSource>> {
        let Some(path) = self.cached_preview_blob_file_path(blob_id, logical_path)? else {
            return Ok(None);
        };
        Ok(Some(FileDiffTextSource::with_identity(
            path,
            format!("blob:{blob_id}"),
        )))
    }

    fn file_diff_source_from_revision_path(
        &self,
        repo: &gix::Repository,
        revision: &str,
        path: &Path,
    ) -> Result<Option<FileDiffTextSource>> {
        let Some(blob_id) = gix_revision_path_blob_object_id_optional(repo, revision, path)? else {
            return Ok(None);
        };
        self.file_diff_source_from_blob_id(blob_id, path)
    }

    fn file_diff_source_from_index_stage(
        &self,
        repo: &gix::Repository,
        path: &Path,
        stage: u8,
    ) -> Result<Option<FileDiffTextSource>> {
        let Some(blob_id) = gix_index_stage_object_id_optional(repo, path, stage)? else {
            return Ok(None);
        };
        self.file_diff_source_from_blob_id(blob_id, path)
    }

    fn file_diff_source_from_worktree_path_optional(
        &self,
        repo: &gix::Repository,
        path: &Path,
    ) -> Result<Option<FileDiffTextSource>> {
        self.cached_git_normalized_worktree_file_source(repo, path)
    }

    fn cached_git_normalized_worktree_file_source(
        &self,
        repo: &gix::Repository,
        path: &Path,
    ) -> Result<Option<FileDiffTextSource>> {
        let full = match worktree_file_path_optional(&self.spec.workdir, path) {
            Some(full) => full,
            None => return Ok(None),
        };

        let (mut pipeline, index) = repo.filter_pipeline(None).map_err(|e| {
            Error::new(ErrorKind::Backend(format!(
                "gix worktree filter pipeline: {e}"
            )))
        })?;
        let file = std::fs::File::open(&full).map_err(io_err_to_error)?;
        let normalized = pipeline.convert_to_git(file, path, &index).map_err(|e| {
            Error::new(ErrorKind::Backend(format!(
                "gix worktree-to-git conversion: {e}"
            )))
        })?;

        let mut tmp_file =
            tempfile::NamedTempFile::new_in(std::env::temp_dir()).map_err(io_err_to_error)?;
        let mut content_hasher = FxHasher::default();
        // The identity hashes the raw normalized bytes; UTF-8 validity only
        // decides whether the cached copy needs a display transcode.
        let mut utf8_check = StreamingUtf8Check::default();
        match normalized {
            gix::filter::plumbing::pipeline::convert::ToGitOutcome::Unchanged(mut file) => {
                copy_hash_and_check_utf8(
                    &mut file,
                    &mut tmp_file,
                    &mut content_hasher,
                    &mut utf8_check,
                )?;
            }
            gix::filter::plumbing::pipeline::convert::ToGitOutcome::Process(mut file) => {
                copy_hash_and_check_utf8(
                    &mut file,
                    &mut tmp_file,
                    &mut content_hasher,
                    &mut utf8_check,
                )?;
            }
            gix::filter::plumbing::pipeline::convert::ToGitOutcome::Buffer(bytes) => {
                bytes.hash(&mut content_hasher);
                utf8_check.push(bytes.as_ref());
                tmp_file.write_all(bytes).map_err(io_err_to_error)?;
            }
        }
        if !utf8_check.finish() {
            rewrite_temp_file_as_utf8(&mut tmp_file)?;
        }
        tmp_file.flush().map_err(io_err_to_error)?;

        let identity = worktree_source_identity(&self.spec.workdir, path, content_hasher.finish());
        let cache_path = worktree_git_cache_path(path, &identity);
        persist_worktree_git_cache_file(tmp_file, &cache_path)?;
        Ok(Some(FileDiffTextSource::with_identity(
            cache_path,
            format!("worktree-git:{identity}"),
        )))
    }

    pub(super) fn diff_file_text(&self, target: &DiffTarget) -> Result<Option<FileDiffText>> {
        match target {
            DiffTarget::WorkingTree { path, area } => {
                let full_path = if path.is_absolute() {
                    path.clone()
                } else {
                    self.spec.workdir.join(path)
                };
                if std::fs::metadata(&full_path).is_ok_and(|m| m.is_dir()) {
                    return Ok(None);
                }

                let repo = self._repo.to_thread_local();
                let repo_path = to_repo_path(path, &self.spec.workdir)?;
                let (old, new) = match area {
                    DiffArea::Unstaged => {
                        let old = match gix_index_unconflicted_blob_id_optional(&repo, &repo_path)?
                        {
                            IndexUnconflictedBlobId::Present(blob_id) => {
                                self.file_diff_source_from_blob_id(blob_id, &repo_path)?
                            }
                            IndexUnconflictedBlobId::Missing => None,
                            IndexUnconflictedBlobId::Unmerged => {
                                let ours =
                                    self.file_diff_source_from_index_stage(&repo, &repo_path, 2)?;
                                let theirs =
                                    self.file_diff_source_from_index_stage(&repo, &repo_path, 3)?;
                                return Ok(Some(FileDiffText::new_sources(
                                    path.clone(),
                                    ours,
                                    theirs,
                                )));
                            }
                        };
                        let new =
                            self.file_diff_source_from_worktree_path_optional(&repo, &repo_path)?;
                        (old, new)
                    }
                    DiffArea::Staged => {
                        let old =
                            self.file_diff_source_from_revision_path(&repo, "HEAD", &repo_path)?;
                        let new = match gix_index_unconflicted_blob_id_optional(&repo, &repo_path)?
                        {
                            IndexUnconflictedBlobId::Present(blob_id) => {
                                self.file_diff_source_from_blob_id(blob_id, &repo_path)?
                            }
                            IndexUnconflictedBlobId::Missing => None,
                            IndexUnconflictedBlobId::Unmerged => self
                                .file_diff_source_from_index_stage(&repo, &repo_path, 2)?
                                .or(self.file_diff_source_from_index_stage(&repo, &repo_path, 3)?),
                        };
                        (old, new)
                    }
                };

                Ok(Some(FileDiffText::new_sources(path.clone(), old, new)))
            }
            DiffTarget::Commit { commit_id, path } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                let parent = gix_first_parent_optional(&repo, commit_id.as_ref())?;

                let old = match parent {
                    Some(parent) => {
                        self.file_diff_source_from_revision_path(&repo, &parent, path)?
                    }
                    None => None,
                };
                let new =
                    self.file_diff_source_from_revision_path(&repo, commit_id.as_ref(), path)?;

                Ok(Some(FileDiffText::new_sources(path.clone(), old, new)))
            }
            DiffTarget::CommitRange {
                from_commit_id,
                to_commit_id,
                path,
            } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                let old =
                    self.file_diff_source_from_revision_path(&repo, from_commit_id.as_ref(), path)?;
                let new = match to_commit_id {
                    Some(to_commit_id) => self.file_diff_source_from_revision_path(
                        &repo,
                        to_commit_id.as_ref(),
                        path,
                    )?,
                    // Working-tree tip: the new side is the live worktree file.
                    None => {
                        let repo_path = to_repo_path(path, &self.spec.workdir)?;
                        self.file_diff_source_from_worktree_path_optional(&repo, &repo_path)?
                    }
                };

                Ok(Some(FileDiffText::new_sources(path.clone(), old, new)))
            }
        }
    }

    pub(super) fn diff_preview_text_file(
        &self,
        target: &DiffTarget,
        side: DiffPreviewTextSide,
    ) -> Result<Option<std::path::PathBuf>> {
        match target {
            DiffTarget::WorkingTree { path, area } => {
                let full_path = if path.is_absolute() {
                    path.clone()
                } else {
                    self.spec.workdir.join(path)
                };
                if std::fs::metadata(&full_path).is_ok_and(|m| m.is_dir()) {
                    return Ok(None);
                }

                let repo = self._repo.to_thread_local();
                let repo_path = to_repo_path(path, &self.spec.workdir)?;
                match (area, side) {
                    (DiffArea::Unstaged, DiffPreviewTextSide::New) => {
                        Ok(worktree_file_path_optional(&self.spec.workdir, &repo_path))
                    }
                    (DiffArea::Unstaged, DiffPreviewTextSide::Old)
                    | (DiffArea::Staged, DiffPreviewTextSide::New) => {
                        let blob_id =
                            match gix_index_unconflicted_blob_id_optional(&repo, &repo_path)? {
                                IndexUnconflictedBlobId::Present(id) => Some(id),
                                IndexUnconflictedBlobId::Missing
                                | IndexUnconflictedBlobId::Unmerged => None,
                            };
                        match blob_id {
                            Some(blob_id) => {
                                self.cached_preview_blob_file_path(blob_id, &repo_path)
                            }
                            None => Ok(None),
                        }
                    }
                    (DiffArea::Staged, DiffPreviewTextSide::Old) => {
                        let blob_id =
                            gix_revision_path_blob_object_id_optional(&repo, "HEAD", &repo_path)?;
                        match blob_id {
                            Some(blob_id) => {
                                self.cached_preview_blob_file_path(blob_id, &repo_path)
                            }
                            None => Ok(None),
                        }
                    }
                }
            }
            DiffTarget::Commit { commit_id, path } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                let blob_id = match side {
                    DiffPreviewTextSide::New => {
                        gix_revision_path_blob_object_id_optional(&repo, commit_id.as_ref(), path)?
                    }
                    DiffPreviewTextSide::Old => {
                        let Some(parent) = gix_first_parent_optional(&repo, commit_id.as_ref())?
                        else {
                            return Ok(None);
                        };
                        gix_revision_path_blob_object_id_optional(&repo, &parent, path)?
                    }
                };

                match blob_id {
                    Some(blob_id) => self.cached_preview_blob_file_path(blob_id, path),
                    None => Ok(None),
                }
            }
            DiffTarget::CommitRange {
                from_commit_id,
                to_commit_id,
                path,
            } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                // Working-tree tip + New side: the preview is the live worktree file.
                if matches!(side, DiffPreviewTextSide::New) && to_commit_id.is_none() {
                    let repo_path = to_repo_path(path, &self.spec.workdir)?;
                    return Ok(worktree_file_path_optional(&self.spec.workdir, &repo_path));
                }
                let blob_id = match side {
                    DiffPreviewTextSide::New => gix_revision_path_blob_object_id_optional(
                        &repo,
                        to_commit_id
                            .as_ref()
                            .expect("worktree tip handled above")
                            .as_ref(),
                        path,
                    )?,
                    DiffPreviewTextSide::Old => gix_revision_path_blob_object_id_optional(
                        &repo,
                        from_commit_id.as_ref(),
                        path,
                    )?,
                };

                match blob_id {
                    Some(blob_id) => self.cached_preview_blob_file_path(blob_id, path),
                    None => Ok(None),
                }
            }
        }
    }

    fn cached_preview_blob_file_path(
        &self,
        blob_id: gix::ObjectId,
        logical_path: &Path,
    ) -> Result<Option<std::path::PathBuf>> {
        let repo = self._repo.to_thread_local();
        if !gix_object_id_is_blob(&repo, blob_id)? {
            return Ok(None);
        }

        let cache_path = preview_blob_cache_path(&self.spec.workdir, logical_path, &blob_id);
        if std::fs::metadata(&cache_path).is_ok_and(|m| m.is_file()) {
            return Ok(Some(cache_path));
        }

        let mut tmp_file =
            tempfile::NamedTempFile::new_in(std::env::temp_dir()).map_err(io_err_to_error)?;
        let mut command = self.git_workdir_cmd();
        command
            .arg("cat-file")
            .arg("blob")
            .arg(blob_id.to_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(io_err_to_error)?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            Error::new(ErrorKind::Backend(
                "git cat-file did not expose stdout".to_string(),
            ))
        })?;
        // Diff sides must be valid UTF-8 for the viewer; a blob saved in a
        // legacy encoding (GBK and friends) is transcode-detected in place
        // while it streams through.
        let mut utf8_check = StreamingUtf8Check::default();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = stdout.read(&mut buffer).map_err(io_err_to_error)?;
            if read == 0 {
                break;
            }
            utf8_check.push(&buffer[..read]);
            tmp_file
                .write_all(&buffer[..read])
                .map_err(io_err_to_error)?;
        }
        let blob_is_utf8 = utf8_check.finish();

        let output = child.wait_with_output().map_err(io_err_to_error)?;
        if !output.status.success() {
            return Err(git_command_failed_error("git cat-file", output));
        }
        if !blob_is_utf8 {
            rewrite_temp_file_as_utf8(&mut tmp_file)?;
        }
        tmp_file.flush().map_err(io_err_to_error)?;

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err_to_error)?;
        }
        match tmp_file.persist(&cache_path) {
            Ok(_) => Ok(Some(cache_path)),
            Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => {
                Ok(Some(cache_path))
            }
            Err(err) => Err(io_err_to_error(err.error)),
        }
    }

    pub(super) fn lfs_new_side_smudged(&self, target: &DiffTarget) -> Result<Option<Vec<u8>>> {
        let Some(change) = self.lfs_pointer_change(target)? else {
            return Ok(None);
        };
        let Some(new) = change.new.as_ref() else {
            return Ok(None);
        };
        let Some(oid) = new.oid.as_deref() else {
            return Ok(None);
        };
        let pointer = lfs_pointer_text(oid, new.size.unwrap_or(0));
        let bytes = self.lfs_smudge_bytes(pointer.as_bytes())?;
        Ok(Some(bytes))
    }

    pub(super) fn diff_file_image(&self, target: &DiffTarget) -> Result<Option<FileDiffImage>> {
        match target {
            DiffTarget::WorkingTree { path, area } => {
                let full_path = if path.is_absolute() {
                    path.clone()
                } else {
                    self.spec.workdir.join(path)
                };
                if std::fs::metadata(&full_path).is_ok_and(|m| m.is_dir()) {
                    return Ok(None);
                }

                let repo = self._repo.to_thread_local();
                let repo_path = to_repo_path(path, &self.spec.workdir)?;
                let (old, new) = match area {
                    DiffArea::Unstaged => {
                        let old = match gix_index_unconflicted_image_blob_bytes_optional(
                            &repo, &repo_path,
                        )? {
                            IndexUnconflictedBlob::Present(bytes) => Some(bytes),
                            IndexUnconflictedBlob::Missing => None,
                            IndexUnconflictedBlob::Unmerged => {
                                let ours = gix_index_stage_image_blob_bytes_optional(
                                    &repo, &repo_path, 2,
                                )?;
                                let theirs = gix_index_stage_image_blob_bytes_optional(
                                    &repo, &repo_path, 3,
                                )?;
                                return Ok(Some(FileDiffImage {
                                    path: path.clone(),
                                    old: ours,
                                    new: theirs,
                                }));
                            }
                        };
                        let new = read_worktree_image_file_bytes_optional(
                            &self.spec.workdir,
                            &repo_path,
                        )?;
                        (old, new)
                    }
                    DiffArea::Staged => {
                        let old =
                            gix_revision_path_image_blob_bytes_optional(&repo, "HEAD", &repo_path)?;
                        let new = match gix_index_unconflicted_image_blob_bytes_optional(
                            &repo, &repo_path,
                        )? {
                            IndexUnconflictedBlob::Present(bytes) => Some(bytes),
                            IndexUnconflictedBlob::Missing => None,
                            IndexUnconflictedBlob::Unmerged => {
                                gix_index_stage_image_blob_bytes_optional(&repo, &repo_path, 2)?.or(
                                    gix_index_stage_image_blob_bytes_optional(
                                        &repo, &repo_path, 3,
                                    )?,
                                )
                            }
                        };
                        (old, new)
                    }
                };

                Ok(Some(FileDiffImage {
                    path: path.clone(),
                    old,
                    new,
                }))
            }
            DiffTarget::Commit { commit_id, path } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                let parent = gix_first_parent_optional(&repo, commit_id.as_ref())?;

                let old = match parent {
                    Some(parent) => {
                        gix_revision_path_image_blob_bytes_optional(&repo, &parent, path)?
                    }
                    None => None,
                };
                let new =
                    gix_revision_path_image_blob_bytes_optional(&repo, commit_id.as_ref(), path)?;

                Ok(Some(FileDiffImage {
                    path: path.clone(),
                    old,
                    new,
                }))
            }
            DiffTarget::CommitRange {
                from_commit_id,
                to_commit_id,
                path,
            } => {
                let Some(path) = path else {
                    return Ok(None);
                };

                let repo = self._repo.to_thread_local();
                let old = gix_revision_path_image_blob_bytes_optional(
                    &repo,
                    from_commit_id.as_ref(),
                    path,
                )?;
                let new = match to_commit_id {
                    Some(to_commit_id) => gix_revision_path_image_blob_bytes_optional(
                        &repo,
                        to_commit_id.as_ref(),
                        path,
                    )?,
                    // Working-tree tip: read the live worktree image bytes.
                    None => {
                        let repo_path = to_repo_path(path, &self.spec.workdir)?;
                        read_worktree_image_file_bytes_optional(&self.spec.workdir, &repo_path)?
                    }
                };

                Ok(Some(FileDiffImage {
                    path: path.clone(),
                    old,
                    new,
                }))
            }
        }
    }

    pub(super) fn conflict_file_stages(&self, path: &Path) -> Result<Option<ConflictFileStages>> {
        // No index conflict stages for this path means nothing to load.
        // That covers plain directories — and used to swallow submodule
        // conflicts, whose worktree path is a directory but whose index
        // stages are gitlink pointers very much worth loading.
        let repo = self._repo.to_thread_local();
        let repo_path = to_repo_path(path, &self.spec.workdir)?;
        let stage_data = gix_index_conflict_stage_data(&repo, &repo_path)?;
        if stage_data.conflict_kind.is_none() {
            return Ok(None);
        }
        Ok(Some(conflict_file_stages_from_stage_data(
            &repo_path, stage_data,
        )))
    }

    pub(super) fn conflict_session(&self, path: &Path) -> Result<Option<ConflictSession>> {
        let repo_path = to_repo_path(path, &self.spec.workdir)?;
        let repo = self._repo.to_thread_local();
        let stage_data = gix_index_conflict_stage_data(&repo, &repo_path)?;
        let Some(conflict_kind) = stage_data.conflict_kind else {
            return Ok(None);
        };
        // A gitlink conflict has two commit pointers as its stages — there
        // is no text to merge, exactly like a binary side pick.
        let is_submodule = stage_data.is_gitlink;

        let stages = conflict_file_stages_from_stage_data(&repo_path, stage_data);
        let current =
            read_worktree_file_conflict_payload_known_optional(&self.spec.workdir, &repo_path);

        let base = ConflictPayload::from_stage_parts(stages.base_bytes, stages.base);
        let ours = ConflictPayload::from_stage_parts(stages.ours_bytes, stages.ours);
        let theirs = ConflictPayload::from_stage_parts(stages.theirs_bytes, stages.theirs);
        // Gitlink stages render as their commit ids, but they are pointers,
        // not text — retagging them keeps the strategy (and every payload
        // consumer) honest about there being nothing to merge.
        let (base, ours, theirs) = if is_submodule {
            (
                submodule_pointer(base),
                submodule_pointer(ours),
                submodule_pointer(theirs),
            )
        } else {
            (base, ours, theirs)
        };

        let is_binary = base.is_binary() || ours.is_binary() || theirs.is_binary();
        let strategy = ConflictResolverStrategy::for_conflict(conflict_kind, is_binary);
        let session = if strategy == ConflictResolverStrategy::FullTextResolver {
            // Full-text sessions use one stage-derived merge plan for aligned
            // rows, conflict regions, and the marker projection while retaining
            // the independently loaded worktree payload as the output seed.
            ConflictSession::from_stage_inputs_with_current(
                repo_path,
                conflict_kind,
                base,
                ours,
                theirs,
                current,
            )
        } else {
            // Binary and file-decision resolvers still need the current
            // worktree payload (including an absent payload) for their
            // specialized completion behavior.
            match current {
                Some(ConflictPayload::Text(current)) => ConflictSession::from_merged_shared_text(
                    repo_path,
                    conflict_kind,
                    base,
                    ours,
                    theirs,
                    current,
                ),
                Some(current) => ConflictSession::new_with_current(
                    repo_path,
                    conflict_kind,
                    base,
                    ours,
                    theirs,
                    current,
                ),
                None => ConflictSession::new(repo_path, conflict_kind, base, ours, theirs),
            }
        };
        Ok(Some(session))
    }

    fn synthetic_simple_commit_path_diff(&self, target: &DiffTarget) -> Result<Option<Diff>> {
        let repo = self._repo.to_thread_local();
        let Some((path, old_revision, new_revision)) = commit_path_diff_revisions(target, &repo)?
        else {
            return Ok(None);
        };
        let old = match old_revision.as_deref() {
            Some(revision) => gix_revision_path_blob_entry_optional(&repo, revision, &path)?,
            None => None,
        };
        let new = gix_revision_path_blob_entry_optional(&repo, &new_revision, &path)?;

        let (prefix, body_text, blob) = match (old, new) {
            (None, Some(new)) => {
                let new_text = decode_utf8_bytes(new.bytes)?;
                (
                    UnifiedBlobPrefix::Add,
                    new_text,
                    UnifiedBlobDiff {
                        mode: new.mode,
                        short_id: new.short_id,
                    },
                )
            }
            (Some(old), None) => {
                let old_text = decode_utf8_bytes(old.bytes)?;
                (
                    UnifiedBlobPrefix::Remove,
                    old_text,
                    UnifiedBlobDiff {
                        mode: old.mode,
                        short_id: old.short_id,
                    },
                )
            }
            _ => return Ok(None),
        };

        Ok(Some(build_simple_commit_path_diff(
            target.clone(),
            &path,
            body_text.as_str(),
            prefix,
            &blob,
        )))
    }
}

fn commit_path_diff_revisions(
    target: &DiffTarget,
    repo: &gix::Repository,
) -> Result<Option<(std::path::PathBuf, Option<String>, String)>> {
    match target {
        DiffTarget::Commit {
            commit_id,
            path: Some(path),
        } => Ok(Some((
            path.clone(),
            gix_first_parent_optional(repo, commit_id.as_ref())?,
            commit_id.as_ref().to_string(),
        ))),
        DiffTarget::CommitRange {
            from_commit_id,
            to_commit_id: Some(to_commit_id),
            path: Some(path),
        } => Ok(Some((
            path.clone(),
            Some(from_commit_id.as_ref().to_string()),
            to_commit_id.as_ref().to_string(),
        ))),
        // Working-tree tip has no revision string for the new side; fall back to
        // the full unified-diff parse (which reads the worktree via the CLI).
        _ => Ok(None),
    }
}

fn conflict_file_stages_from_stage_data(
    path: &Path,
    stage_data: ConflictStageData,
) -> ConflictFileStages {
    let (base_bytes, base) =
        canonicalize_stage_parts(stage_data.base_bytes.map(Arc::<[u8]>::from), None);
    let (ours_bytes, ours) =
        canonicalize_stage_parts(stage_data.ours_bytes.map(Arc::<[u8]>::from), None);
    let (theirs_bytes, theirs) =
        canonicalize_stage_parts(stage_data.theirs_bytes.map(Arc::<[u8]>::from), None);

    ConflictFileStages {
        path: path.to_path_buf(),
        base_bytes,
        ours_bytes,
        theirs_bytes,
        base,
        ours,
        theirs,
    }
}

fn to_repo_path(path: &Path, workdir: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    if let Ok(relative) = path.strip_prefix(workdir) {
        return Ok(relative.to_path_buf());
    }

    if let Some(normalized_path) = canonicalize_existing_path_prefix(path)
        && let Ok(relative) = normalized_path.strip_prefix(workdir)
    {
        return Ok(relative.to_path_buf());
    }

    Err(Error::new(ErrorKind::Backend(format!(
        "path '{}' is outside repository workdir '{}'",
        path.display(),
        workdir.display()
    ))))
}

fn canonicalize_existing_path_prefix(path: &Path) -> Option<PathBuf> {
    let mut missing_components = Vec::new();
    let mut current = path;

    loop {
        match current.canonicalize() {
            Ok(canonical) => {
                let mut normalized = strip_windows_verbatim_prefix(canonical);
                for component in missing_components.iter().rev() {
                    normalized.push(component);
                }
                return Some(normalized);
            }
            Err(_) => {
                missing_components.push(current.file_name()?.to_owned());
                current = current.parent()?;
            }
        }
    }
}

fn ensure_image_diff_side_size(path: &Path, bytes: u64) -> Result<()> {
    if bytes > MAX_IMAGE_DIFF_SIDE_BYTES {
        return Err(Error::new(ErrorKind::Backend(format!(
            "image diff side '{}' is {bytes} bytes, above the {MAX_IMAGE_DIFF_SIDE_BYTES} byte limit",
            path.display()
        ))));
    }
    Ok(())
}

fn read_worktree_image_file_bytes_optional(workdir: &Path, path: &Path) -> Result<Option<Vec<u8>>> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workdir.join(path)
    };
    let metadata = match std::fs::metadata(&full) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::new(ErrorKind::Io(e.kind()))),
    };
    ensure_image_diff_side_size(path, metadata.len())?;

    match std::fs::read(&full) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::new(ErrorKind::Io(e.kind()))),
    }
}

fn read_worktree_file_conflict_payload_known_optional(
    workdir: &Path,
    path: &Path,
) -> Option<ConflictPayload> {
    let full = workdir.join(path);
    match std::fs::read(&full) {
        Ok(bytes) => Some(ConflictPayload::from_bytes(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(ConflictPayload::Absent),
        Err(_) => None,
    }
}

enum IndexUnconflictedBlob {
    Present(Vec<u8>),
    Missing,
    Unmerged,
}

enum IndexUnconflictedBlobId {
    Present(gix::ObjectId),
    Missing,
    Unmerged,
}

struct RevisionPathBlobEntry {
    bytes: Vec<u8>,
    mode: gix::objs::tree::EntryMode,
    short_id: String,
}

struct UnifiedBlobDiff {
    mode: gix::objs::tree::EntryMode,
    short_id: String,
}

#[derive(Clone, Copy)]
enum UnifiedBlobPrefix {
    Add,
    Remove,
}

fn decode_utf8_bytes(bytes: Vec<u8>) -> Result<String> {
    // Legacy-encoded blobs (GBK and friends) are transcoded instead of
    // rejected; the result is always valid UTF-8.
    Ok(worktree_core::encoding::text_bytes_to_utf8_string(bytes))
}

fn gix_blob_bytes_from_object_id_optional(
    repo: &gix::Repository,
    object_id: gix::ObjectId,
) -> Result<Option<Vec<u8>>> {
    let Some(object) = repo
        .try_find_object(object_id)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix try_find_object: {e}"))))?
    else {
        return Ok(None);
    };

    Ok(match object.try_into_blob() {
        Ok(mut blob) => Some(blob.take_data()),
        Err(_) => None,
    })
}

fn gix_object_id_is_blob(repo: &gix::Repository, object_id: gix::ObjectId) -> Result<bool> {
    let Some(header) = repo
        .try_find_header(object_id)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix try_find_header: {e}"))))?
    else {
        return Ok(false);
    };
    Ok(header.kind() == gix::objs::Kind::Blob)
}

fn gix_image_blob_bytes_from_object_id_optional(
    repo: &gix::Repository,
    object_id: gix::ObjectId,
    path: &Path,
) -> Result<Option<Vec<u8>>> {
    let Some(header) = repo
        .try_find_header(object_id)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix try_find_header: {e}"))))?
    else {
        return Ok(None);
    };
    if header.kind() != gix::objs::Kind::Blob {
        return Ok(None);
    }
    ensure_image_diff_side_size(path, header.size())?;
    gix_blob_bytes_from_object_id_optional(repo, object_id)
}

fn gix_revision_id_optional(
    repo: &gix::Repository,
    revision: &str,
) -> Result<Option<gix::ObjectId>> {
    if revision == "HEAD" {
        return match repo.head_id() {
            Ok(id) => Ok(Some(id.detach())),
            Err(_) => Ok(None),
        };
    }

    if let Ok(id) = gix::ObjectId::from_hex(revision.as_bytes()) {
        return Ok(Some(id));
    }

    let Some(mut reference) = repo
        .try_find_reference(revision)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix try_find_reference: {e}"))))?
    else {
        return Ok(None);
    };

    let id = match reference.try_id() {
        Some(id) => id.detach(),
        None => match reference.peel_to_id() {
            Ok(id) => id.detach(),
            Err(_) => return Ok(None),
        },
    };
    Ok(Some(id))
}

fn gix_revision_path_blob_object_id_optional(
    repo: &gix::Repository,
    revision: &str,
    path: &Path,
) -> Result<Option<gix::ObjectId>> {
    let Some(object_id) = gix_revision_id_optional(repo, revision)? else {
        return Ok(None);
    };

    let object = match repo.find_object(object_id) {
        Ok(object) => object,
        Err(_) => return Ok(None),
    };
    let tree = match object.peel_to_tree() {
        Ok(tree) => tree,
        Err(_) => return Ok(None),
    };

    let Some(entry) = tree
        .lookup_entry_by_path(path)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix lookup_entry_by_path: {e}"))))?
    else {
        return Ok(None);
    };

    Ok(Some(entry.object_id()))
}

fn gix_revision_path_image_blob_bytes_optional(
    repo: &gix::Repository,
    revision: &str,
    path: &Path,
) -> Result<Option<Vec<u8>>> {
    let Some(object_id) = gix_revision_path_blob_object_id_optional(repo, revision, path)? else {
        return Ok(None);
    };
    gix_image_blob_bytes_from_object_id_optional(repo, object_id, path)
}

fn gix_revision_path_blob_entry_optional(
    repo: &gix::Repository,
    revision: &str,
    path: &Path,
) -> Result<Option<RevisionPathBlobEntry>> {
    let Some(object_id) = gix_revision_id_optional(repo, revision)? else {
        return Ok(None);
    };

    let object = match repo.find_object(object_id) {
        Ok(object) => object,
        Err(_) => return Ok(None),
    };
    let tree = match object.peel_to_tree() {
        Ok(tree) => tree,
        Err(_) => return Ok(None),
    };

    let Some(entry) = tree
        .lookup_entry_by_path(path)
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix lookup_entry_by_path: {e}"))))?
    else {
        return Ok(None);
    };

    let Some(bytes) = gix_blob_bytes_from_object_id_optional(repo, entry.object_id())? else {
        return Ok(None);
    };

    Ok(Some(RevisionPathBlobEntry {
        bytes,
        mode: entry.mode(),
        short_id: entry.id().shorten_or_id().to_string(),
    }))
}

fn gix_index_unconflicted_image_blob_bytes_optional(
    repo: &gix::Repository,
    path: &Path,
) -> Result<IndexUnconflictedBlob> {
    let index = repo
        .index_or_load_from_head_or_empty()
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix index: {e}"))))?;

    let path_key = gix::path::os_str_into_bstr(path.as_os_str())
        .map_err(|_| Error::new(ErrorKind::Unsupported("path is not valid UTF-8")))?;

    if let Some(entry) =
        index.entry_by_path_and_stage(path_key, gix::index::entry::Stage::Unconflicted)
    {
        return Ok(
            match gix_image_blob_bytes_from_object_id_optional(repo, entry.id, path)? {
                Some(bytes) => IndexUnconflictedBlob::Present(bytes),
                None => IndexUnconflictedBlob::Missing,
            },
        );
    }

    if index.entry_range(path_key).is_some() {
        return Ok(IndexUnconflictedBlob::Unmerged);
    }

    Ok(IndexUnconflictedBlob::Missing)
}

fn gix_index_unconflicted_blob_id_optional(
    repo: &gix::Repository,
    path: &Path,
) -> Result<IndexUnconflictedBlobId> {
    let index = repo
        .index_or_load_from_head_or_empty()
        .map_err(|e| Error::new(ErrorKind::Backend(format!("gix index: {e}"))))?;

    let path = gix::path::os_str_into_bstr(path.as_os_str())
        .map_err(|_| Error::new(ErrorKind::Unsupported("path is not valid UTF-8")))?;

    if let Some(entry) = index.entry_by_path_and_stage(path, gix::index::entry::Stage::Unconflicted)
    {
        return Ok(IndexUnconflictedBlobId::Present(entry.id));
    }

    if index.entry_range(path).is_some() {
        return Ok(IndexUnconflictedBlobId::Unmerged);
    }

    Ok(IndexUnconflictedBlobId::Missing)
}

fn gix_index_stage_image_blob_bytes_optional(
    repo: &gix::Repository,
    path: &Path,
    stage: u8,
) -> Result<Option<Vec<u8>>> {
    let Some(object_id) = gix_index_stage_object_id_optional(repo, path, stage)? else {
        return Ok(None);
    };
    gix_image_blob_bytes_from_object_id_optional(repo, object_id, path)
}

fn worktree_file_path_optional(workdir: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workdir.join(path)
    };
    std::fs::metadata(&full)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|_| full)
}

fn copy_hash_and_check_utf8(
    reader: &mut impl Read,
    writer: &mut impl Write,
    hasher: &mut FxHasher,
    utf8_check: &mut StreamingUtf8Check,
) -> Result<()> {
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(io_err_to_error)?;
        if read == 0 {
            return Ok(());
        }
        utf8_check.push(&buffer[..read]);
        buffer[..read].hash(hasher);
        writer.write_all(&buffer[..read]).map_err(io_err_to_error)?;
    }
}

/// Incremental UTF-8 validity check over a byte stream. Multi-byte sequences
/// split across chunks are carried in a small tail so a split surrogate pair
/// is not misjudged. Once a stream is known invalid it stays invalid — the
/// caller only needs a verdict, not the error position.
#[derive(Default)]
struct StreamingUtf8Check {
    valid: bool,
    tail: Vec<u8>,
}

impl StreamingUtf8Check {
    fn push(&mut self, chunk: &[u8]) {
        if !self.valid {
            return;
        }
        if !self.tail.is_empty() {
            let mut combined = Vec::with_capacity(self.tail.len() + chunk.len());
            combined.extend_from_slice(&self.tail);
            combined.extend_from_slice(chunk);
            match std::str::from_utf8(&combined) {
                Ok(_) => {
                    self.tail.clear();
                    return;
                }
                Err(error) => {
                    if error.error_len().is_some() {
                        self.valid = false;
                        self.tail.clear();
                        return;
                    }
                    let valid_up_to = error.valid_up_to();
                    self.tail.clear();
                    self.tail.extend_from_slice(&combined[valid_up_to..]);
                    return;
                }
            }
        }
        match std::str::from_utf8(chunk) {
            Ok(_) => {}
            Err(error) => {
                if error.error_len().is_some() {
                    self.valid = false;
                } else {
                    let valid_up_to = error.valid_up_to();
                    self.tail.extend_from_slice(&chunk[valid_up_to..]);
                }
            }
        }
    }

    fn finish(&mut self) -> bool {
        if self.valid && !self.tail.is_empty() {
            // An incomplete sequence dangling at EOF is invalid UTF-8.
            self.valid = false;
        }
        self.valid
    }
}

/// Rewrites a materialized diff side in place as UTF-8 when it was saved in a
/// legacy encoding (GBK/GB18030, UTF-16, …). Only called after a streaming
/// check rejected the raw bytes, so the extra full read is paid by non-UTF-8
/// files alone.
fn rewrite_temp_file_as_utf8(tmp_file: &mut tempfile::NamedTempFile) -> Result<()> {
    let raw = std::fs::read(tmp_file.path()).map_err(io_err_to_error)?;
    let converted = worktree_core::encoding::text_bytes_to_utf8(raw.as_slice());
    if matches!(converted, std::borrow::Cow::Borrowed(_)) {
        return Ok(());
    }
    tmp_file.as_file_mut().set_len(0).map_err(io_err_to_error)?;
    tmp_file
        .as_file_mut()
        .seek(std::io::SeekFrom::Start(0))
        .map_err(io_err_to_error)?;
    tmp_file
        .write_all(converted.as_ref())
        .map_err(io_err_to_error)?;
    Ok(())
}

fn persist_worktree_git_cache_file(
    tmp_file: tempfile::NamedTempFile,
    cache_path: &Path,
) -> Result<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err_to_error)?;
    }
    match tmp_file.persist(cache_path) {
        Ok(_) => Ok(()),
        Err(err) if err.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let tmp_file = err.file;
            std::fs::remove_file(cache_path).map_err(io_err_to_error)?;
            tmp_file
                .persist(cache_path)
                .map(|_| ())
                .map_err(|err| io_err_to_error(err.error))
        }
        Err(err) => Err(io_err_to_error(err.error)),
    }
}

fn hash_worktree_source_identity(
    hasher: &mut FxHasher,
    workdir: &Path,
    logical_path: &Path,
    normalized_content_hash: u64,
) {
    workdir.hash(hasher);
    logical_path.hash(hasher);
    normalized_content_hash.hash(hasher);
}

fn worktree_source_identity(
    workdir: &Path,
    logical_path: &Path,
    normalized_content_hash: u64,
) -> String {
    let mut hasher = FxHasher::default();
    hash_worktree_source_identity(&mut hasher, workdir, logical_path, normalized_content_hash);
    format!("{:016x}", hasher.finish())
}

fn worktree_git_cache_path(logical_path: &Path, identity: &str) -> std::path::PathBuf {
    let suffix = logical_path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    std::env::temp_dir().join(format!("worktree-diff-worktree-{identity}{suffix}"))
}

fn preview_blob_cache_path(
    workdir: &Path,
    logical_path: &Path,
    blob_id: &gix::ObjectId,
) -> std::path::PathBuf {
    let mut hasher = FxHasher::default();
    workdir.hash(&mut hasher);
    logical_path.hash(&mut hasher);
    blob_id.to_string().hash(&mut hasher);
    let hash = hasher.finish();
    let suffix = logical_path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_default();
    // The "2" version-segments the prefix: caches materialized before
    // non-UTF-8 blobs were transcoded hold raw legacy bytes and must not be
    // served by the early-return above.
    std::env::temp_dir().join(format!("worktree-diff-preview2-{hash:016x}{suffix}"))
}

fn io_err_to_error(error: std::io::Error) -> Error {
    Error::new(ErrorKind::Io(error.kind()))
}

fn gix_first_parent_optional(repo: &gix::Repository, commit: &str) -> Result<Option<String>> {
    let Some(commit_id) = gix_revision_id_optional(repo, commit)? else {
        return Ok(None);
    };

    let commit = match repo.find_commit(commit_id) {
        Ok(commit) => commit,
        Err(_) => return Ok(None),
    };
    Ok(commit.parent_ids().next().map(|id| id.detach().to_string()))
}

fn build_simple_commit_path_diff(
    target: DiffTarget,
    path: &Path,
    body_text: &str,
    prefix: UnifiedBlobPrefix,
    blob: &UnifiedBlobDiff,
) -> Diff {
    let path_text = path.to_string_lossy();
    let line_count = unified_body_line_count(body_text);
    let mut mode_buf = [0u8; 6];
    let mode_text =
        std::str::from_utf8(blob.mode.as_bytes(&mut mode_buf).as_ref()).unwrap_or("100644");
    let header_capacity = path_text.len().saturating_mul(4).saturating_add(96);
    let body_capacity = body_text.len().saturating_add(line_count);
    let missing_newline_marker = usize::from(!body_text.is_empty() && !body_text.ends_with('\n'))
        .saturating_mul("\\ No newline at end of file\n".len());
    let mut text = String::with_capacity(
        header_capacity
            .saturating_add(body_capacity)
            .saturating_add(missing_newline_marker),
    );

    text.push_str("diff --git a/");
    text.push_str(path_text.as_ref());
    text.push_str(" b/");
    text.push_str(path_text.as_ref());
    text.push('\n');

    match prefix {
        UnifiedBlobPrefix::Add => {
            text.push_str("new file mode ");
            text.push_str(mode_text);
            text.push('\n');
            text.push_str("index 0000000..");
            text.push_str(blob.short_id.as_str());
            text.push('\n');
            text.push_str("--- /dev/null\n");
            text.push_str("+++ b/");
            text.push_str(path_text.as_ref());
            text.push('\n');
            text.push_str("@@ -0,0 +");
            push_unified_hunk_range(&mut text, 1, line_count);
            text.push_str(" @@\n");
        }
        UnifiedBlobPrefix::Remove => {
            text.push_str("deleted file mode ");
            text.push_str(mode_text);
            text.push('\n');
            text.push_str("index ");
            text.push_str(blob.short_id.as_str());
            text.push_str("..0000000\n");
            text.push_str("--- a/");
            text.push_str(path_text.as_ref());
            text.push('\n');
            text.push_str("+++ /dev/null\n");
            text.push_str("@@ -");
            push_unified_hunk_range(&mut text, 1, line_count);
            text.push_str(" +0,0 @@\n");
        }
    }

    append_prefixed_unified_body(&mut text, prefix, body_text);
    Diff::from_unified_owned(target, text)
}

fn append_prefixed_unified_body(target: &mut String, prefix: UnifiedBlobPrefix, text: &str) {
    if text.is_empty() {
        return;
    }

    let prefix_char = match prefix {
        UnifiedBlobPrefix::Add => '+',
        UnifiedBlobPrefix::Remove => '-',
    };

    let mut emitted_trailing_newline = false;
    for line in text.split_inclusive('\n') {
        target.push(prefix_char);
        target.push_str(line);
        emitted_trailing_newline = line.ends_with('\n');
    }

    if !emitted_trailing_newline {
        target.push('\n');
        target.push_str("\\ No newline at end of file\n");
    }
}

fn push_unified_hunk_range(target: &mut String, start: usize, count: usize) {
    match count {
        0 => {
            target.push_str(start.to_string().as_str());
            target.push_str(",0");
        }
        1 => {
            target.push_str(start.to_string().as_str());
        }
        _ => {
            target.push_str(start.to_string().as_str());
            target.push(',');
            target.push_str(count.to_string().as_str());
        }
    }
}

fn unified_body_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.as_bytes()
            .iter()
            .filter(|&&byte| byte == b'\n')
            .count()
            + usize::from(!text.ends_with('\n'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_core::domain::{DiffArea, DiffTarget};
    use worktree_core::error::ErrorKind;
    use worktree_test_support::run_git;

    fn init_test_repo(workdir: &Path) {
        run_git(workdir, &["init"]);
        run_git(workdir, &["config", "commit.gpgsign", "false"]);
        run_git(workdir, &["config", "user.name", "Test User"]);
        run_git(workdir, &["config", "user.email", "test@example.com"]);
    }

    fn open_repo(workdir: &Path) -> GixRepo {
        let thread_safe_repo = gix::open(workdir).expect("open repo").into_sync();
        GixRepo::new(workdir.to_path_buf(), thread_safe_repo)
    }

    #[test]
    fn read_worktree_image_file_bytes_rejects_oversized_file_before_reading() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("large.png");
        let file = std::fs::File::create(&path).expect("create sparse image");
        file.set_len(MAX_IMAGE_DIFF_SIDE_BYTES + 1)
            .expect("make sparse image oversized");

        let err = read_worktree_image_file_bytes_optional(tmp.path(), Path::new("large.png"))
            .expect_err("oversized image should fail");

        let ErrorKind::Backend(message) = err.kind() else {
            panic!("expected backend size-limit error, got {err:?}");
        };
        assert!(message.contains("image diff side"));
        assert!(message.contains("byte limit"));
    }

    #[test]
    fn read_worktree_image_file_bytes_allows_file_at_size_limit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bytes = vec![7; MAX_IMAGE_DIFF_SIDE_BYTES as usize];
        std::fs::write(tmp.path().join("limit.png"), &bytes).expect("write image at limit");

        let loaded = read_worktree_image_file_bytes_optional(tmp.path(), Path::new("limit.png"))
            .expect("load image at limit")
            .expect("image at limit");

        assert_eq!(loaded, bytes);
    }

    #[test]
    fn worktree_source_identity_changes_with_normalized_content_hash() {
        let workdir = Path::new("/tmp/repo");
        let path = Path::new("src/lib.rs");

        let first = worktree_source_identity(workdir, path, 0x11);
        let second = worktree_source_identity(workdir, path, 0x22);

        assert_ne!(first, second);
    }

    #[test]
    fn persist_worktree_git_cache_file_replaces_existing_cache_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("worktree-diff-worktree-test.txt");
        std::fs::write(&cache_path, b"old normalized content").expect("write stale cache");

        let mut replacement = tempfile::NamedTempFile::new_in(tmp.path()).expect("temp file");
        replacement
            .write_all(b"new normalized content")
            .expect("write replacement cache");
        replacement.flush().expect("flush replacement cache");

        persist_worktree_git_cache_file(replacement, &cache_path)
            .expect("replace stale normalized cache file");

        assert_eq!(
            std::fs::read(&cache_path).expect("read replaced cache"),
            b"new normalized content"
        );
    }

    #[test]
    fn diff_file_text_for_staged_gitlink_returns_empty_sources() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());
        run_git(
            tmp.path(),
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "160000,1111111111111111111111111111111111111111,vendor/sub",
            ],
        );

        let repo = open_repo(tmp.path());
        let diff = repo
            .diff_file_text(&DiffTarget::WorkingTree {
                path: "vendor/sub".into(),
                area: DiffArea::Staged,
            })
            .expect("gitlink text diff should not error")
            .expect("file diff text object");

        assert_eq!(diff.path, Path::new("vendor/sub"));
        assert!(diff.old_source.is_none());
        assert!(diff.new_source.is_none());
    }

    #[test]
    fn diff_file_text_transcodes_gbk_sides_to_utf8() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_test_repo(tmp.path());

        // `int main() { return 0; } // 中文` with the comment in GBK — the
        // shape of a legacy Windows source file.
        let gbk_committed: &[u8] = b"int main() { return 0; } // \xD6\xD0\xCE\xC4\n";
        std::fs::write(tmp.path().join("legacy.cpp"), gbk_committed).expect("write GBK file");
        run_git(tmp.path(), &["add", "legacy.cpp"]);
        run_git(tmp.path(), &["commit", "-m", "add GBK source"]);

        let repo = open_repo(tmp.path());

        // Worktree side alone: modify the file with different GBK content.
        let gbk_modified: &[u8] = b"int main() { return 1; } // \xD6\xD0\xCE\xC4\xD7\xA2\xCA\xCD\n";
        std::fs::write(tmp.path().join("legacy.cpp"), gbk_modified).expect("write modified GBK");

        let diff = repo
            .diff_file_text(&DiffTarget::WorkingTree {
                path: "legacy.cpp".into(),
                area: DiffArea::Unstaged,
            })
            .expect("GBK text diff should not error")
            .expect("file diff text object");

        for (label, source) in [("old", diff.old_source), ("new", diff.new_source)] {
            let source = source.expect("{label} source exists");
            let raw = std::fs::read(&source.path).expect("read materialized side");
            let text = String::from_utf8(raw).expect("{label} side must be valid UTF-8");
            assert!(
                text.contains("中文"),
                "{label} side should decode the GBK comment"
            );
        }
    }
}

/// Retag a gitlink stage's text payload as a submodule pointer, leaving
/// absent sides absent and anything unexpected untouched.
fn submodule_pointer(payload: ConflictPayload) -> ConflictPayload {
    match payload {
        ConflictPayload::Text(pointer) => ConflictPayload::Submodule(pointer),
        other => other,
    }
}

/// The canonical pointer file content for one (oid, size) pair — the exact
/// bytes `git lfs smudge` expects on stdin, reconstructed because the diff
/// view holds the parsed change, not the raw pointer file.
fn lfs_pointer_text(oid: &str, size: u64) -> String {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
}

#[cfg(test)]
mod lfs_pointer_text_tests {
    #[test]
    fn pointer_text_matches_the_canonical_lfs_header() {
        assert_eq!(
            super::lfs_pointer_text("abc123", 42),
            "version https://git-lfs.github.com/spec/v1\noid sha256:abc123\nsize 42\n"
        );
    }
}
