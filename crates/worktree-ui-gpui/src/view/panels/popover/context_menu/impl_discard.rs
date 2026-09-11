//! `PopoverHost` discard and its hunk patch construction.

use super::super::*;

// @split-module: impl_discard
impl PopoverHost {
    pub(in crate::view::panels::popover) fn discard_worktree_changes_confirmed(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        path: Option<std::path::PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) {
        let (paths, _used_selection) = match path.as_ref() {
            Some(clicked_path) => {
                let selection = self.details_pane.update(cx, |pane, cx| {
                    let selection = pane
                        .status_multi_selection
                        .get(&repo_id)
                        .map(|sel| sel.selected_paths_for_area(area))
                        .unwrap_or(&[]);

                    let use_selection =
                        selection.len() > 1 && selection.iter().any(|p| p == clicked_path);
                    if !use_selection {
                        return None;
                    }

                    let sel = pane.status_multi_selection.remove(&repo_id)?;
                    cx.notify();
                    Some(sel.take_selected_paths_for_area(area))
                });

                match selection {
                    Some(paths) if !paths.is_empty() => (paths, true),
                    _ => (vec![clicked_path.clone()], false),
                }
            }
            None => {
                let paths = self
                    .details_pane
                    .update(cx, |pane, cx| {
                        let sel = pane.status_multi_selection.remove(&repo_id)?;
                        cx.notify();
                        Some(sel.take_selected_paths_for_area(area))
                    })
                    .unwrap_or_default();
                if paths.is_empty() {
                    return;
                }
                (paths, true)
            }
        };

        if paths.len() > 1 {
            self.store.dispatch(Msg::ClearDiffSelection { repo_id });
            self.store
                .dispatch(Msg::DiscardWorktreeChangesPaths { repo_id, paths });
            return;
        }

        let Some(path) = paths.into_iter().next() else {
            return;
        };

        let is_added_file = self
            .state
            .repos
            .iter()
            .find(|r| r.id == repo_id)
            .and_then(|repo| {
                repo.status_entry_for_path(DiffArea::Unstaged, path.as_path())
                    .or_else(|| repo.status_entry_for_path(DiffArea::Staged, path.as_path()))
                    .map(|status| status.kind)
            })
            .is_some_and(|kind| matches!(kind, FileStatusKind::Untracked | FileStatusKind::Added));

        if is_added_file {
            let path_is_selected = self
                .active_repo()
                .filter(|r| r.id == repo_id)
                .and_then(|r| r.diff_state.diff_target.as_ref())
                .is_some_and(|target| {
                    matches!(target, DiffTarget::WorkingTree { path: selected, .. } if *selected == path)
                });
            if path_is_selected {
                self.store.dispatch(Msg::ClearDiffSelection { repo_id });
            }
        } else {
            self.store.dispatch(Msg::SelectDiff {
                repo_id,
                target: DiffTarget::WorkingTree {
                    path: path.clone(),
                    area: DiffArea::Unstaged,
                },
            });
        }
        self.store
            .dispatch(Msg::DiscardWorktreeChangesPath { repo_id, path });
    }

    pub(in crate::view::panels::popover) fn build_unified_patch_for_hunk_src_ix(
        &self,
        repo_id: RepoId,
        hunk_src_ix: usize,
    ) -> Option<String> {
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let Loadable::Ready(diff) = &repo.diff_state.diff else {
            return None;
        };
        crate::view::diff_utils::build_unified_patch_for_hunk(diff.lines.as_slice(), hunk_src_ix)
    }
}
