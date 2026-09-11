//! `PopoverHost` path actions: opening, revealing and copying paths, and the
//! status selection they act on.

use super::super::*;
use super::helpers::normalize_platform_path;

// @split-module: impl_paths
impl PopoverHost {
    pub(in crate::view::panels::popover) fn workdir_for_repo(
        &self,
        repo_id: RepoId,
    ) -> Option<std::path::PathBuf> {
        self.state
            .repos
            .iter()
            .find(|r| r.id == repo_id)
            .map(|r| r.spec.workdir.clone())
    }

    pub(super) fn resolve_workdir_path(
        &self,
        repo_id: RepoId,
        path: &std::path::Path,
    ) -> Result<std::path::PathBuf, String> {
        if path.is_absolute()
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir
                        | std::path::Component::Prefix(_)
                        | std::path::Component::RootDir
                )
            })
        {
            return Err(crate::i18n::t!("toast.context_menu.path_outside_repo").into_owned());
        }

        let workdir = self
            .workdir_for_repo(repo_id)
            .ok_or_else(|| crate::i18n::t!("toast.context_menu.repo_unavailable").into_owned())?;
        Ok(normalize_platform_path(workdir.join(path)))
    }

    pub(super) fn open_path_default(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(), std::io::Error> {
        super::super::super::platform_open::open_path(path)
    }

    fn open_file_location(&mut self, path: &std::path::Path) -> Result<(), std::io::Error> {
        super::super::super::platform_open::open_file_location(path)
    }

    pub(super) fn reveal_path_in_file_manager(
        &mut self,
        path: std::path::PathBuf,
        fallback: Option<std::path::PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) {
        let target = if path.exists() {
            path
        } else {
            path.parent()
                .map(ToOwned::to_owned)
                .or(fallback)
                .unwrap_or(path)
        };

        if !target.exists() {
            self.push_toast(
                components::ToastKind::Error,
                format!("Path not found: {}", target.display()),
                cx,
            );
        } else if let Err(err) = self.open_file_location(&target) {
            self.push_toast(
                components::ToastKind::Error,
                format!("Failed to open location: {err}"),
                cx,
            );
        }
    }

    /// The paths a context-menu action on `clicked_path` covers, plus whether
    /// they came out of the row selection. Reads only — see
    /// [`Self::clear_status_multi_selection`] for the other half.
    /// The screen point a follow-up popover should open at, derived from the
    /// anchor of the menu that is currently open.
    ///
    /// Every context-menu action that opens a dialog needs this, and the six
    /// copies it replaced all carried the same duplicated fallback constant.
    pub(in crate::view::panels::popover) fn popover_anchor_point(&self) -> gpui::Point<Pixels> {
        self.popover_anchor
            .as_ref()
            .map(|anchor| match anchor {
                PopoverAnchor::Point(point) => *point,
                PopoverAnchor::Bounds(bounds) => bounds.bottom_right(),
                PopoverAnchor::Centered => point(px(64.0), px(64.0)),
            })
            .unwrap_or_else(|| point(px(64.0), px(64.0)))
    }

    pub(super) fn status_paths_for_action(
        &self,
        repo_id: RepoId,
        area: DiffArea,
        clicked_path: &std::path::PathBuf,
        cx: &gpui::App,
    ) -> (Vec<std::path::PathBuf>, bool) {
        self.details_pane
            .read(cx)
            .status_selected_paths_for_action(repo_id, area, clicked_path)
    }

    /// Drop the row selection because an action has gone ahead with it. Never
    /// call this before the action is settled: a confirmation the user cancels
    /// must leave the selection standing.
    pub(in crate::view::panels::popover) fn clear_status_multi_selection(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        self.details_pane.update(cx, |pane, cx| {
            pane.clear_status_multi_selection(repo_id);
            cx.notify();
        });
    }

    pub(super) fn take_status_paths_for_action(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        clicked_path: &std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> (Vec<std::path::PathBuf>, bool) {
        let (paths, used_selection) = self.status_paths_for_action(repo_id, area, clicked_path, cx);
        if used_selection {
            self.clear_status_multi_selection(repo_id, cx);
        }
        (paths, used_selection)
    }

    pub(super) fn repo_is_open(&self, repo_id: RepoId) -> bool {
        self.state.repos.iter().any(|repo| repo.id == repo_id)
    }

    /// A toast rather than an error banner: nothing failed, the row just went
    /// stale, and the banner belongs to whichever repository is active now — not
    /// to the one that left.
    pub(super) fn warn_repository_gone(&mut self, cx: &mut gpui::Context<Self>) {
        self.push_toast(
            components::ToastKind::Warning,
            crate::i18n::t!("toast.context_menu.repo_gone").into_owned(),
            cx,
        );
    }
}
