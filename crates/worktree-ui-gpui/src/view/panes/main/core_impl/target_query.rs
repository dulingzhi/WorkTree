//! Queries over the currently rendered diff target: active repo, loadables,
//! cache currency and blame capability.
use super::super::helpers::{CollapsedDiffProjectionIdentity, historical_browse_content};
use super::*;
use worktree_core::domain::{Diff, FileDiffImage, FileDiffText, LfsPointerChange};

/// Resolve the file path and blame source for a diff target, or `None` for
/// targets that do not support blame annotation (e.g. whole-commit diffs with no
/// selected path).
///
/// Committed-file diffs blame the committed revision shown on the new side.
/// Working-tree diffs blame the displayed new-side content for their area (see
/// [`worktree_core::services::Repo::blame_worktree_file`]); lines not yet
/// committed are surfaced as "Not Committed Yet". In both cases blame is
/// computed against the exact content rendered on the new side, so the 1:1
/// `new_line` mapping in the annotation column stays correct.
fn blame_path_rev_for_target(
    target: &DiffTarget,
) -> Option<(std::path::PathBuf, worktree_core::domain::BlameSource)> {
    use worktree_core::domain::BlameSource;
    match target {
        DiffTarget::WorkingTree { path, area } => {
            Some((path.clone(), BlameSource::WorkingTree(*area)))
        }
        DiffTarget::Commit {
            commit_id,
            path: Some(path),
        } => Some((
            path.clone(),
            BlameSource::Revision(Some(commit_id.0.to_string())),
        )),
        DiffTarget::CommitRange {
            to_commit_id,
            path: Some(path),
            ..
        } => Some((
            path.clone(),
            match to_commit_id {
                Some(to_commit_id) => BlameSource::Revision(Some(to_commit_id.0.to_string())),
                // Working-tree tip: the new side is the worktree file.
                None => BlameSource::WorkingTree(worktree_core::domain::DiffArea::Unstaged),
            },
        )),
        _ => None,
    }
}

/// Decide whether a blame (re)load should be dispatched for the rendered target.
///
/// `same_target` is whether the currently loaded blame is for the same
/// file/source. `force` requests a retry of a previous failure (an explicit user
/// toggle); the per-frame Render path passes `false` so a persistent error does
/// not cause a dispatch-every-frame loop.
pub(super) fn should_request_blame<T>(
    same_target: bool,
    blame: &worktree_state::model::Loadable<T>,
    force: bool,
) -> bool {
    use worktree_state::model::Loadable;
    if !same_target {
        // A new or changed target always (re)loads.
        return true;
    }
    match blame {
        // Already loaded or in flight for this target: nothing to do.
        Loadable::Ready(_) | Loadable::Loading => false,
        // A previous attempt failed: retry only on an explicit user toggle.
        Loadable::Error(_) => force,
        Loadable::NotLoaded => true,
    }
}

impl MainPaneView {
    /// Scaled pixel width of the annotation column at the current ui scale.
    pub(in crate::view) fn annotate_column_width_px(&self, ui_scale_percent: u32) -> Pixels {
        crate::ui_scale::design_px_from_percent(self.annotate_column_width, ui_scale_percent)
    }

    /// Whether the annotation column should be shown for the currently rendered
    /// diff target. Requires the user toggle to be on AND the target to support
    /// blame (committed-file and working-tree views — see
    /// [`blame_path_rev_for_target`]).
    pub(in crate::view) fn annotation_active(&self) -> bool {
        self.annotate_enabled
            && self
                .rendered_diff_target()
                .and_then(blame_path_rev_for_target)
                .is_some()
    }

    /// Whether the loaded (or retained) blame describes the diff target being
    /// rendered right now. `blame_path`/`blame_source` follow the store snapshot,
    /// which lags the dispatch by at least a frame, so just after a file switch
    /// they still name the previous file — its annotations must not be painted
    /// over the new one's rows.
    pub(in crate::view) fn blame_matches_rendered_target(&self) -> bool {
        let Some((path, source)) = self
            .rendered_diff_target()
            .and_then(blame_path_rev_for_target)
        else {
            return false;
        };
        self.active_repo().is_some_and(|repo| {
            repo.history_state.blame_path.as_deref() == Some(path.as_path())
                && repo.history_state.blame_source.as_ref() == Some(&source)
        })
    }

    /// When annotate is on, ensure blame for the currently displayed file/rev is
    /// loaded. Derives the path and revision from the rendered diff target and
    /// dispatches `LoadBlame`, skipping redundant loads.
    pub(in crate::view) fn request_blame_for_current_target(
        &mut self,
        force: bool,
        _cx: &mut gpui::Context<Self>,
    ) {
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        let Some((path, source)) = self
            .rendered_diff_target()
            .and_then(blame_path_rev_for_target)
        else {
            return;
        };

        if let Some(repo) = self.active_repo() {
            let history = &repo.history_state;
            let same_target = history.blame_path.as_deref() == Some(path.as_path())
                && history.blame_source.as_ref() == Some(&source);
            if !should_request_blame(same_target, &history.blame, force) {
                return;
            }
        }

        self.store.dispatch(Msg::LoadBlame {
            repo_id,
            path,
            source,
        });
    }

    pub(in crate::view) fn active_repo_id(&self) -> Option<RepoId> {
        self.state.active_repo
    }

    pub(in crate::view) fn active_repo(&self) -> Option<&RepoState> {
        let repo_id = self.active_repo_id()?;
        self.state.repos.iter().find(|r| r.id == repo_id)
    }

    pub(in crate::view) fn active_inline_submodule_diff(
        &self,
    ) -> Option<&worktree_state::model::InlineSubmoduleDiffState> {
        self.active_repo()?
            .diff_state
            .inline_submodule_diff
            .as_ref()
    }

    pub(in crate::view) fn selected_inline_submodule_diff_entry(
        &self,
    ) -> Option<&worktree_state::model::InlineSubmoduleDiffEntry> {
        let inline = self.active_inline_submodule_diff()?;
        inline.entries.get(inline.selected_ix)
    }

    pub(in crate::view) fn is_inline_submodule_diff_active(&self) -> bool {
        self.active_inline_submodule_diff().is_some()
    }

    pub(in crate::view) fn rendered_diff_target(&self) -> Option<&DiffTarget> {
        self.active_inline_submodule_diff()
            .map(|inline| &inline.target)
            .or_else(|| self.active_repo()?.diff_state.diff_target.as_ref())
    }

    /// Whether the content pane is showing a file's full content *at the commit
    /// the file browser is pinned to*, i.e. whether it earns the historical
    /// browse tint. See [`historical_browse_content`].
    pub(in crate::view) fn historical_browse_content_active(&self) -> bool {
        let Some(repo) = self.active_repo() else {
            return false;
        };
        historical_browse_content(repo, self.rendered_diff_target())
    }

    pub(in crate::view) fn rendered_patch_diff_loadable(
        &self,
    ) -> Option<&worktree_state::model::Loadable<worktree_state::model::Shared<Diff>>> {
        if let Some(inline) = self.active_inline_submodule_diff() {
            Some(&inline.diff)
        } else {
            self.active_repo().map(|repo| &repo.diff_state.diff)
        }
    }

    pub(in crate::view) fn rendered_patch_diff_rev(&self) -> u64 {
        self.active_inline_submodule_diff()
            .map(|inline| inline.diff_rev)
            .or_else(|| self.active_repo().map(|repo| repo.diff_state.diff_rev))
            .unwrap_or(0)
    }

    fn rendered_file_target_path(target: &DiffTarget) -> Option<&std::path::Path> {
        match target {
            DiffTarget::WorkingTree { path, .. } => Some(path.as_path()),
            DiffTarget::Commit {
                path: Some(path), ..
            }
            | DiffTarget::CommitRange {
                path: Some(path), ..
            } => Some(path.as_path()),
            DiffTarget::Commit { path: None, .. } | DiffTarget::CommitRange { path: None, .. } => {
                None
            }
        }
    }

    pub(in crate::view) fn rendered_file_diff_loadable(
        &self,
    ) -> Option<&worktree_state::model::Loadable<Option<worktree_state::model::Shared<FileDiffText>>>>
    {
        if let Some(inline) = self.active_inline_submodule_diff() {
            Some(&inline.diff_file)
        } else {
            self.active_repo().map(|repo| &repo.diff_state.diff_file)
        }
    }

    pub(in crate::view) fn rendered_file_image_diff_loadable(
        &self,
    ) -> Option<
        &worktree_state::model::Loadable<Option<worktree_state::model::Shared<FileDiffImage>>>,
    > {
        if let Some(inline) = self.active_inline_submodule_diff() {
            Some(&inline.diff_file_image)
        } else {
            self.active_repo()
                .map(|repo| &repo.diff_state.diff_file_image)
        }
    }

    pub(in crate::view) fn rendered_file_lfs_diff_loadable(
        &self,
    ) -> Option<
        &worktree_state::model::Loadable<Option<worktree_state::model::Shared<LfsPointerChange>>>,
    > {
        // Inline submodule diffs keep the plain text path — the submodule
        // workdir's LFS wiring is not introspected, so no panel there.
        self.active_repo()
            .map(|repo| &repo.diff_state.diff_file_lfs)
    }

    pub(in crate::view) fn rendered_file_diff_rev(&self) -> u64 {
        self.active_inline_submodule_diff()
            .map(|inline| inline.diff_file_rev)
            .or_else(|| self.active_repo().map(|repo| repo.diff_state.diff_file_rev))
            .unwrap_or(0)
    }

    pub(in crate::view) fn rendered_diff_workdir(&self) -> Option<&std::path::Path> {
        self.active_inline_submodule_diff()
            .map(|inline| inline.submodule_repo_path.as_path())
            .or_else(|| self.active_repo().map(|repo| repo.spec.workdir.as_path()))
    }

    pub(in crate::view) fn rendered_file_diff_identity(
        &self,
    ) -> Option<(
        RepoId,
        u64,
        DiffTarget,
        std::path::PathBuf,
        std::path::PathBuf,
    )> {
        let repo_id = self.active_repo_id()?;
        let diff_file_rev = self.rendered_file_diff_rev();
        let diff_target = self.rendered_diff_target()?.clone();
        let workdir = self.rendered_diff_workdir()?.to_path_buf();
        let rel_path = Self::rendered_file_target_path(&diff_target)?;
        let abs_path = workdir.join(rel_path);
        Some((repo_id, diff_file_rev, diff_target, workdir, abs_path))
    }

    pub(in crate::view) fn supports_diff_content_mode_toggle(&self, is_file_preview: bool) -> bool {
        !is_file_preview
            && !self.is_worktree_target_directory()
            && Self::is_file_diff_target(self.rendered_diff_target())
    }

    /// The diff mode actually in effect. Collapsed hides the unchanged parts of
    /// a patch, so a target the state layer loads as whole-file content — an
    /// added, deleted, or untracked file, which has no patch — has nothing to
    /// collapse and stays on Full however the setting is set.
    pub(in crate::view) fn effective_diff_content_mode(&self) -> DiffContentMode {
        if self.diff_content_mode == DiffContentMode::Collapsed
            && matches!(
                self.rendered_patch_diff_loadable(),
                Some(Loadable::NotLoaded)
            )
        {
            return DiffContentMode::Full;
        }
        self.diff_content_mode
    }

    pub(in crate::view) fn wants_file_diff_view(&self, is_file_preview: bool) -> bool {
        self.effective_diff_content_mode() == DiffContentMode::Full
            && self.supports_diff_content_mode_toggle(is_file_preview)
    }

    pub(in crate::view) fn wants_collapsed_diff_view(&self, is_file_preview: bool) -> bool {
        self.effective_diff_content_mode() == DiffContentMode::Collapsed
            && self.supports_diff_content_mode_toggle(is_file_preview)
    }

    pub(super) fn current_main_diff_supports_diff_content_toggle(&self) -> bool {
        let inline_submodule_diff_active = self.is_inline_submodule_diff_active();
        let has_submodule_summary = self
            .active_repo()
            .is_some_and(|repo| !matches!(repo.diff_state.submodule_summary, Loadable::NotLoaded));
        let untracked_directory_notice = if has_submodule_summary || inline_submodule_diff_active {
            None
        } else {
            self.untracked_directory_notice()
        };
        let is_file_preview = self.is_file_preview_active()
            && untracked_directory_notice.is_none()
            && !has_submodule_summary
            && !inline_submodule_diff_active;
        (inline_submodule_diff_active || !has_submodule_summary)
            && self.supports_diff_content_mode_toggle(is_file_preview)
    }

    pub(super) fn current_main_diff_wants_file_diff(&self) -> bool {
        let inline_submodule_diff_active = self.is_inline_submodule_diff_active();
        let has_submodule_summary = self
            .active_repo()
            .is_some_and(|repo| !matches!(repo.diff_state.submodule_summary, Loadable::NotLoaded));
        let untracked_directory_notice = if has_submodule_summary || inline_submodule_diff_active {
            None
        } else {
            self.untracked_directory_notice()
        };
        let is_file_preview = self.is_file_preview_active()
            && untracked_directory_notice.is_none()
            && !has_submodule_summary
            && !inline_submodule_diff_active;
        self.current_main_diff_supports_diff_content_toggle()
            && self.wants_file_diff_view(is_file_preview)
    }

    fn rendered_patch_diff_cache_is_current(&self) -> bool {
        self.active_repo_id().is_some_and(|repo_id| {
            self.diff_cache_repo_id == Some(repo_id)
                && self.diff_cache_rev == self.rendered_patch_diff_rev()
                && self.diff_cache_target == self.rendered_diff_target().cloned()
        })
    }

    pub(super) fn rendered_file_diff_cache_is_current(&self) -> bool {
        let Some((repo_id, diff_file_rev, diff_target, _workdir, abs_path)) =
            self.rendered_file_diff_identity()
        else {
            return false;
        };

        self.file_diff_cache_repo_id == Some(repo_id)
            && self.file_diff_cache_rev == diff_file_rev
            && self.file_diff_cache_target == Some(diff_target)
            && self.file_diff_cache_whitespace_mode == self.diff_whitespace_mode
            && self.file_diff_cache_path.as_ref() == Some(&abs_path)
    }

    pub(in crate::view) fn is_collapsed_diff_projection_active(&self) -> bool {
        self.effective_diff_content_mode() == DiffContentMode::Collapsed
            && self.current_main_diff_supports_diff_content_toggle()
            && self.rendered_patch_diff_cache_is_current()
            && self.rendered_file_diff_cache_is_current()
    }

    pub(in crate::view) fn collapsed_visible_row(
        &self,
        visible_ix: usize,
    ) -> Option<CollapsedDiffVisibleRow> {
        self.collapsed_diff_visible_rows.get(visible_ix).copied()
    }

    pub(in crate::view) fn current_collapsed_diff_projection_identity(
        &self,
    ) -> Option<CollapsedDiffProjectionIdentity> {
        let (repo_id, _diff_file_rev, diff_target, _workdir, abs_path) =
            self.rendered_file_diff_identity()?;
        Some(CollapsedDiffProjectionIdentity {
            repo_id,
            diff_target,
            file_path: abs_path,
            diff_whitespace_mode: self.diff_whitespace_mode,
            patch_content_signature: self.diff_cache_content_signature,
            file_content_signature: self.file_diff_cache_content_signature,
        })
    }

    pub(in crate::view) fn reset_collapsed_diff_projection(&mut self, clear_reveals: bool) {
        self.collapsed_diff_hunks.clear();
        self.collapsed_diff_hunk_ix_by_src_ix.clear();
        if clear_reveals {
            self.collapsed_diff_reveals.clear();
            self.collapsed_diff_projection_identity = None;
        }
        self.collapsed_diff_visible_rows.clear();
        self.collapsed_diff_hunk_visible_indices.clear();
        self.collapsed_diff_header_display_cache.clear();
        self.diff_visible_projection_rev = self.diff_visible_projection_rev.wrapping_add(1);
        if clear_reveals {
            self.diff_visible_cache_projection_rev = u64::MAX;
        }
    }

    pub(in crate::view) fn invalidate_collapsed_diff_visible_projection(&mut self) {
        self.collapsed_diff_visible_rows.clear();
        self.collapsed_diff_hunk_visible_indices.clear();
        self.collapsed_diff_header_display_cache.clear();
        self.diff_visible_projection_rev = self.diff_visible_projection_rev.wrapping_add(1);
    }

    pub(super) fn rendered_diff_target_for_state(state: &AppState) -> Option<DiffTarget> {
        let repo_id = state.active_repo?;
        let repo = state.repos.iter().find(|repo| repo.id == repo_id)?;
        repo.diff_state
            .inline_submodule_diff
            .as_ref()
            .map(|inline| inline.target.clone())
            .or_else(|| repo.diff_state.diff_target.clone())
    }
}
