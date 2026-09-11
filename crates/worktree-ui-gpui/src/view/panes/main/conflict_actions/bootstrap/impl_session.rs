//! `MainPaneView` session state: clearing, sync targeting, and load requests.

use super::model::ConflictSyncTarget;

use crate::view::conflict_resolver;
use crate::view::conflict_resolver::ConflictResolverUiState;
use crate::view::panes::main::state::MainPaneView;
use std::path::PathBuf;
use worktree_core::domain::DiffArea;
use worktree_core::domain::DiffTarget;
use worktree_state::model::Loadable;
use worktree_state::model::RepoId;
use worktree_state::msg::Msg;
// @split-module: impl_session
impl MainPaneView {
    pub(super) fn clear_conflict_resolver_state(&mut self) {
        self.conflict_resolver = ConflictResolverUiState::default();
        self.conflict_resolved_output_saved_snapshot = None;
        self.conflict_resolved_output_modified = false;
        self.conflict_resolved_output_block_map =
            conflict_resolver::ResolvedOutputBlockMap::default();
        self.conflict_resolver_invalidate_resolved_outline();
    }

    /// Gate phase: resolve the conflicted working-tree entry the resolver
    /// binds to. `None` covers every "not a conflict target" case the
    /// pre-split monolith answered by clearing the resolver state; the caller
    /// performs that clear.
    pub(super) fn conflict_resolver_sync_target(&self) -> Option<ConflictSyncTarget> {
        let repo_id = self.active_repo_id()?;
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let Some(DiffTarget::WorkingTree { path, area }) = repo.diff_state.diff_target.as_ref()
        else {
            return None;
        };
        if *area != DiffArea::Unstaged {
            return None;
        }
        let conflict_entry = repo
            .status_entry_for_path(DiffArea::Unstaged, path.as_path())
            .filter(|entry| entry.kind == worktree_core::domain::FileStatusKind::Conflicted)?;
        Some(ConflictSyncTarget {
            repo_id,
            path: path.clone(),
            conflict_kind: conflict_entry.conflict,
        })
    }

    /// CurrentOnly first load: reset the resolver and the output input, then
    /// dispatch the load request. The caller returns immediately afterwards.
    pub(super) fn begin_conflict_file_current_only_load(
        &mut self,
        repo_id: RepoId,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        self.clear_conflict_resolver_state();
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_text("", cx);
        });
        self.store.dispatch(Msg::LoadConflictFile {
            repo_id,
            path,
            mode: worktree_state::model::ConflictFileLoadMode::CurrentOnly,
        });
    }

    pub(in crate::view) fn request_conflict_file_load_mode(
        &mut self,
        mode: worktree_state::model::ConflictFileLoadMode,
    ) -> bool {
        let Some(repo_id) = self.active_repo_id() else {
            return false;
        };
        let Some(path) = self.conflict_resolver.path.clone() else {
            return false;
        };
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        if repo.conflict_state.conflict_file_path.as_ref() != Some(&path) {
            return false;
        }
        if repo.conflict_state.conflict_file_load_mode == mode
            || matches!(repo.conflict_state.conflict_file, Loadable::Loading)
        {
            return false;
        }

        self.store.dispatch(Msg::LoadConflictFile {
            repo_id,
            path,
            mode,
        });
        true
    }
}
