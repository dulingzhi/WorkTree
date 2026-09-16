//! The history pane: the `HistoryView` state core with its input, reveal and
//! cache orchestration, one domain module per family under `history/`:
//!
//! - `columns` — column geometry: design and pixel widths, the drag layout,
//!   visible-column computation and resize-state clamping
//! - `reveal` — the selection and reveal vocabulary: selected-index caches,
//!   selection highlights, worktree reveal targets, pending-reveal decisions
//! - `cache_build` — the branch-head inputs, stash detection, base/decoration
//!   cache assembly and lane attribution
//! - `history_panel` — the pane's render tree
//!
//! This file is the module root: it declares the domains, keeps the
//! `HistoryView` struct and its impl blocks, and re-exports the surface the
//! panes and the tests consume, so `crate::view::panes::history` keeps naming
//! the items it always did.

use super::super::*;
use super::PaneChromeExt;
use crate::view::caches::{
    HistoryListPlan, HistoryListPlanCache, HistoryShortShaVm, HistoryVisibleIndices, HistoryWhenVm,
    HistoryWorktreeRowAnchor, analyze_history_stashes, build_history_branch_containment_bits,
    build_history_branch_ref_items_by_target, build_history_branch_text_by_target,
    build_history_tag_names_by_target, build_history_visible_indices,
    history_ref_items_from_displayed_refs, next_history_stash_tip_for_commit_ix,
    related_commit_contains,
};
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

mod cache_build;
mod columns;
mod history_panel;
mod reveal;

use cache_build::{build_history_base_cache, build_history_decoration_cache};
#[cfg(test)]
use cache_build::{
    graph_branch_heads, history_row_attribution_branch, is_probable_stash_tip,
    stash_summary_from_log_summary,
};
pub(in crate::view) use columns::{
    HistoryColumnDragLayout, history_column_resize_drag_params, history_column_resize_max_width,
    history_column_resize_state, history_resize_state_visible_columns,
    history_visible_columns_for_layout, history_visible_columns_for_layout_with_resize_state,
};
use columns::{
    HistoryColumnWidths, default_history_column_design_widths,
    history_column_drag_clamped_width_for_state, history_column_drag_next_width,
    history_columns_available_width, history_reset_widths_for_available_width, history_scale,
    history_scaled_px, scaled_history_column_widths,
};
#[cfg(test)]
use columns::{
    default_history_column_widths, history_column_drag_clamped_width,
    history_visible_columns_for_width,
};
#[cfg(test)]
pub(in crate::view) use columns::{
    history_resize_state_preserves_visible_columns,
    history_resize_state_visible_columns_for_current_width,
};
use reveal::{
    HistoryLaneAnchor, HistorySelectedLaneColorCache, HistorySelectedListIndexCache,
    HistorySelectionHighlight, HistorySelectionRef, PendingHistoryReveal,
    PendingHistoryRevealDecision, WorktreeRevealTarget, build_selection_highlight,
    decide_pending_history_reveal, resolve_history_selected_list_index,
    set_history_selected_list_index_cache, worktree_reveal_target, worktree_row_list_ix,
};
#[cfg(test)]
use reveal::{peek_history_selected_list_index, rows_reachable_from};

pub(in super::super) fn history_scrollbar_gutter() -> Pixels {
    crate::view::components::Scrollbar::gutter(crate::view::components::ScrollbarAxis::Vertical)
}

pub(in super::super) struct HistoryView {
    pub(in super::super) store: Arc<AppStore>,
    state: Arc<AppState>,
    pub(in super::super) theme: AppTheme,
    pub(in super::super) ui_scale_percent: u32,
    pub(in super::super) date_time_format: DateTimeFormat,
    pub(in super::super) timezone: Timezone,
    pub(in super::super) show_timezone: bool,
    pub(in super::super) history_relative_dates: bool,
    pub(in super::super) history_highlight_commit_chain: bool,
    _ui_model_subscription: gpui::Subscription,
    root_view: WeakEntity<WorkTreeView>,
    notify_fingerprint: u64,
    pub(in super::super) active_context_menu_invoker: Option<SharedString>,
    pub(in super::super) last_window_size: Size<Pixels>,
    pub(in super::super) history_content_width: Pixels,

    pub(in super::super) history_cache_seq: u64,
    pub(in super::super) history_cache_inflight: Option<HistoryCacheBuildRequest>,
    history_col_branch_design: f32,
    history_col_graph_design: f32,
    history_col_author_design: f32,
    history_col_date_design: f32,
    history_col_sha_design: f32,
    pub(in super::super) history_col_branch: Pixels,
    pub(in super::super) history_col_graph: Pixels,
    pub(in super::super) history_col_author: Pixels,
    pub(in super::super) history_col_date: Pixels,
    pub(in super::super) history_col_sha: Pixels,
    pub(in super::super) history_show_graph: bool,
    pub(in super::super) history_show_author: bool,
    pub(in super::super) history_show_date: bool,
    pub(in super::super) history_show_sha: bool,
    pub(in super::super) history_show_tags: bool,
    pub(in super::super) history_auto_fetch_tags_on_repo_activation: bool,
    pub(in super::super) history_col_graph_auto: bool,
    pub(in super::super) history_col_resize: Option<HistoryColResizeState>,
    pub(in super::super) history_cache: Option<HistoryCache>,
    history_selected_list_index_cache: Option<HistorySelectedListIndexCache>,
    selected_branch: Option<SelectedBranch>,
    pending_history_reveal: Option<PendingHistoryReveal>,
    /// Last browse-point commit we scrolled to, so a new one is revealed only when
    /// the historical browse point actually changes.
    last_browse_commit: Option<CommitId>,
    pub(in super::super) history_worktree_summary_cache: Option<HistoryWorktreeSummaryCache>,
    history_list_plan_cache: Option<HistoryListPlanCache>,
    history_selected_lane_color_cache: Option<HistorySelectedLaneColorCache>,
    pub(in super::super) history_stash_ids_cache: Option<HistoryStashIdsCache>,
    pub(in super::super) history_scroll: UniformListScrollHandle,
    pub(in super::super) history_panel_focus_handle: FocusHandle,
    /// Minute tick that re-renders the table while the relative date format is
    /// active, so "2 mins ago" labels don't freeze. `None` for absolute formats.
    relative_time_tick: Option<gpui::Task<()>>,
}

impl PaneChromeExt for HistoryView {
    fn root_view(&self) -> &WeakEntity<WorkTreeView> {
        &self.root_view
    }

    fn theme_slot(&mut self) -> &mut AppTheme {
        &mut self.theme
    }
}

impl HistoryView {
    fn notify_fingerprint_for(state: &AppState, show_history_tags: bool) -> u64 {
        let mut hasher = FxHasher::default();
        state.active_repo.hash(&mut hasher);

        if let Some(repo_id) = state.active_repo
            && let Some(repo) = state.repos.iter().find(|r| r.id == repo_id)
        {
            // The whole repo-level fingerprint is now a single derived key on
            // `RepoState` (`history_cache_rev`): it folds in every revision the
            // view cares about, including the coarse `content_rev` ping, so the
            // view no longer maintains its own list (and cannot silently miss a
            // new source). `tags_rev` stays gated by the display toggle here.
            repo.history_cache_rev().hash(&mut hasher);
            if show_history_tags {
                repo.tags_rev.hash(&mut hasher);
            }
        }

        hasher.finish()
    }

    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn new(
        store: Arc<AppStore>,
        ui_model: Entity<AppUiModel>,
        theme: AppTheme,
        ui_scale_percent: u32,
        date_time_format: DateTimeFormat,
        timezone: Timezone,
        show_timezone: bool,
        history_relative_dates: bool,
        history_highlight_commit_chain: bool,
        history_show_graph: bool,
        history_show_author: bool,
        history_show_date: bool,
        history_show_sha: bool,
        history_show_tags: bool,
        history_auto_fetch_tags_on_repo_activation: bool,
        root_view: WeakEntity<WorkTreeView>,
        last_window_size: Size<Pixels>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let state = Arc::clone(&ui_model.read(cx).state);
        let initial_fingerprint = Self::notify_fingerprint_for(&state, history_show_tags);
        let subscription = cx.observe(&ui_model, |this, model, cx| {
            let next = Arc::clone(&model.read(cx).state);
            let next_fingerprint = Self::notify_fingerprint_for(&next, this.history_show_tags);
            let changed = next_fingerprint != this.notify_fingerprint;
            this.state = next;

            // When the historical browse point changes, scroll the history to that
            // commit (its row is highlighted purple by the canvas).
            let browse_commit = this
                .active_repo()
                .and_then(|repo| repo.browsing_commit().cloned());
            if browse_commit != this.last_browse_commit {
                this.last_browse_commit = browse_commit.clone();
                if let (Some(repo_id), Some(commit_id)) = (this.active_repo_id(), browse_commit) {
                    this.request_reveal_commit(repo_id, commit_id, Some(LogScope::AllBranches), cx);
                }
            }

            if changed {
                this.notify_fingerprint = next_fingerprint;
                this.dismiss_history_refs_hover(cx);
                cx.notify();
            }
        });

        let history_panel_focus_handle = cx.focus_handle().tab_index(0).tab_stop(false);
        let default_design_widths = default_history_column_design_widths();
        let scale = ui_scale::UiScale::from_percent(ui_scale_percent);
        let default_widths = scaled_history_column_widths(default_design_widths, scale);

        Self {
            store,
            state,
            theme,
            ui_scale_percent,
            date_time_format,
            timezone,
            show_timezone,
            history_relative_dates,
            history_highlight_commit_chain,
            _ui_model_subscription: subscription,
            root_view,
            notify_fingerprint: initial_fingerprint,
            active_context_menu_invoker: None,
            last_window_size,
            history_content_width: history_columns_available_width(last_window_size.width),
            history_cache_seq: 0,
            history_cache_inflight: None,
            history_col_branch_design: default_design_widths.branch,
            history_col_graph_design: default_design_widths.graph,
            history_col_author_design: default_design_widths.author,
            history_col_date_design: default_design_widths.date,
            history_col_sha_design: default_design_widths.sha,
            history_col_branch: default_widths.branch,
            history_col_graph: default_widths.graph,
            history_col_author: default_widths.author,
            history_col_date: default_widths.date,
            history_col_sha: default_widths.sha,
            history_show_graph,
            history_show_author,
            history_show_date,
            history_show_sha,
            history_show_tags,
            history_auto_fetch_tags_on_repo_activation,
            history_col_graph_auto: true,
            history_col_resize: None,
            history_cache: None,
            history_selected_list_index_cache: None,
            selected_branch: None,
            pending_history_reveal: None,
            last_browse_commit: None,
            history_worktree_summary_cache: None,
            history_list_plan_cache: None,
            history_selected_lane_color_cache: None,
            history_stash_ids_cache: None,
            history_scroll: UniformListScrollHandle::default(),
            history_panel_focus_handle,
            relative_time_tick: None,
        }
    }

    /// Keeps a minute-interval re-render task alive while relative history
    /// dates are enabled; drops it (cancelling the task) otherwise.
    pub(in super::super) fn ensure_relative_time_tick(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.history_relative_dates {
            self.relative_time_tick = None;
            return;
        }
        if self.relative_time_tick.is_some() {
            return;
        }
        // The test scheduler would treat a sleeping loop as forever-pending work.
        if !crate::ui_runtime::current().uses_live_store_poller() {
            return;
        }
        self.relative_time_tick = Some(cx.spawn(
            async move |view: WeakEntity<HistoryView>, cx: &mut gpui::AsyncApp| {
                loop {
                    smol::Timer::after(std::time::Duration::from_secs(60)).await;
                    if view.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            },
        ));
    }

    pub(in super::super) fn active_repo_id(&self) -> Option<RepoId> {
        self.state.active_repo
    }

    pub(in super::super) fn active_repo(&self) -> Option<&RepoState> {
        let repo_id = self.active_repo_id()?;
        self.state.repos.iter().find(|r| r.id == repo_id)
    }

    /// Visible commit ids in log order for shift-click range selection.
    /// Hidden rows (stash helper commits) are excluded, matching what the
    /// user sees.
    pub(in super::super) fn visible_commit_ids_for_repo(
        &self,
        repo_id: RepoId,
    ) -> Option<Vec<CommitId>> {
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let page = Self::display_log_page_for_repo(repo)?;
        let cache = self
            .history_cache
            .as_ref()
            .filter(|cache| cache.base.request.repo_id == repo_id)?;
        Some(
            cache
                .base
                .visible_indices
                .iter()
                .filter_map(|ix| page.commits.get(ix).map(|c| c.id.clone()))
                .collect(),
        )
    }

    pub(in crate::view) fn show_commit_message_hover(
        &mut self,
        next: crate::view::commit_message_hover::CommitMessageHoverState,
        pointer: Point<Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.show_commit_message_hover(next, pointer, cx)
        });
    }

    pub(in crate::view) fn show_history_refs_hover(
        &mut self,
        repo_id: RepoId,
        commit_id: CommitId,
        source_bounds: Bounds<Pixels>,
        items: Arc<[HistoryRefListItem]>,
        pointer: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.show_history_refs_hover(
                repo_id,
                commit_id,
                source_bounds,
                items,
                pointer,
                window,
                cx,
            );
        });
    }

    pub(in crate::view) fn display_log_page_for_repo(repo: &RepoState) -> Option<Arc<LogPage>> {
        match &repo.log {
            Loadable::Ready(page) => Some(Arc::clone(page)),
            Loadable::Loading => repo
                .history_state
                .retained_log_while_loading
                .as_ref()
                .map(Arc::clone),
            Loadable::NotLoaded | Loadable::Error(_) => None,
        }
    }

    fn live_log_page_has_more_for_repo(repo: &RepoState) -> Option<bool> {
        match &repo.log {
            Loadable::Ready(page) => Some(page.next_cursor.is_some()),
            Loadable::Loading | Loadable::NotLoaded | Loadable::Error(_) => None,
        }
    }

    fn attached_head_target_for_repo(repo: &RepoState) -> Option<CommitId> {
        let Loadable::Ready(head_branch) = &repo.head_branch else {
            return None;
        };
        if head_branch == "HEAD" {
            return None;
        }
        let Loadable::Ready(branches) = &repo.branches else {
            return None;
        };
        branches
            .iter()
            .find(|branch| branch.name == *head_branch)
            .map(|branch| branch.target.clone())
    }

    fn history_base_cache_request_for_repo(
        &self,
        repo: &RepoState,
        page: &LogPage,
    ) -> HistoryBaseCacheRequest {
        HistoryBaseCacheRequest {
            repo_id: repo.id,
            history_scope: repo.history_state.history_scope,
            log_fingerprint: Self::log_fingerprint(&page.commits),
            head_branch_rev: repo.head_branch_rev,
            detached_head_commit: repo.detached_head_commit.clone(),
            head_branch_target: Self::attached_head_target_for_repo(repo),
            branches_rev: if repo.history_state.history_scope.is_current_branch_mode() {
                0
            } else {
                repo.branches_rev
            },
            remote_branches_rev: if repo.history_state.history_scope.is_current_branch_mode() {
                0
            } else {
                repo.remote_branches_rev
            },
            stashes_rev: repo.stashes_rev,
        }
    }

    pub(in crate::view) fn ui_scale(&self) -> ui_scale::UiScale {
        history_scale(self.ui_scale_percent)
    }

    fn sync_history_column_widths_from_design(&mut self) {
        let scale = self.ui_scale();
        self.history_col_branch = scale.px(self.history_col_branch_design);
        self.history_col_graph = scale.px(self.history_col_graph_design);
        self.history_col_author = scale.px(self.history_col_author_design);
        self.history_col_date = scale.px(self.history_col_date_design);
        self.history_col_sha = scale.px(self.history_col_sha_design);
    }

    fn sync_history_column_design_widths_from_pixels(&mut self) {
        let scale = self.ui_scale();
        self.history_col_branch_design = scale.design_units_from_pixels(self.history_col_branch);
        self.history_col_graph_design = scale.design_units_from_pixels(self.history_col_graph);
        self.history_col_author_design = scale.design_units_from_pixels(self.history_col_author);
        self.history_col_date_design = scale.design_units_from_pixels(self.history_col_date);
        self.history_col_sha_design = scale.design_units_from_pixels(self.history_col_sha);
    }

    fn history_decoration_cache_request_for_repo(
        &self,
        repo: &RepoState,
        page: &LogPage,
    ) -> HistoryDecorationCacheRequest {
        HistoryDecorationCacheRequest {
            base_request: self.history_base_cache_request_for_repo(repo, page),
            head_branch_rev: repo.head_branch_rev,
            detached_head_commit: repo.detached_head_commit.clone(),
            branches_rev: repo.branches_rev,
            remote_branches_rev: repo.remote_branches_rev,
            tags_rev: if self.history_show_tags {
                repo.tags_rev
            } else {
                0
            },
        }
    }

    pub(in crate::view) fn request_reveal_commit(
        &mut self,
        repo_id: RepoId,
        commit_id: CommitId,
        fallback_scope: Option<LogScope>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.request_reveal_commit_inner(repo_id, commit_id, fallback_scope, None, cx);
    }

    /// Focus whatever best represents a worktree in the log.
    ///
    /// The rule is the same for every worktree row in the sidebar, including the
    /// one this tab is checked out on: land on its uncommitted-changes row when
    /// it has changes, and on the commit its HEAD points at when it does not.
    /// Only the *current* worktree's changes live in the pinned row at the top;
    /// every other worktree's live in a row of their own.
    pub(in crate::view) fn reveal_worktree(
        &mut self,
        repo_id: RepoId,
        path: PathBuf,
        is_current: bool,
        head: Option<CommitId>,
        cx: &mut gpui::Context<Self>,
    ) {
        let current_has_changes = self.ensure_history_worktree_summary_cache().0;
        // `None` while the scan has not answered -- see `worktree_reveal_target`.
        let worktree_is_dirty = self
            .active_repo()
            .and_then(|repo| match &repo.worktree_dirty {
                Loadable::Ready(dirty) => Some(dirty.iter().any(|summary| summary.path == path)),
                _ => None,
            });

        match worktree_reveal_target(is_current, current_has_changes, worktree_is_dirty, head) {
            WorktreeRevealTarget::WorkingTreeSummaryRow => {
                self.select_working_tree_summary_row(repo_id, cx)
            }
            WorktreeRevealTarget::WorktreeRow {
                head,
                fallback_scope,
            } => self.request_reveal_worktree(repo_id, head, fallback_scope, path, cx),
            WorktreeRevealTarget::Commit {
                head,
                fallback_scope,
            } => self.request_reveal_commit(repo_id, head, fallback_scope, cx),
            WorktreeRevealTarget::Nothing => {}
        }
    }

    /// Select the pinned uncommitted-changes row at the top of the log.
    pub(in crate::view) fn select_working_tree_summary_row(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        self.store
            .dispatch(Msg::SelectWorkingTreeSummary { repo_id });
        self.dismiss_history_refs_hover(cx);
        self.history_scroll
            .scroll_to_item_strict(0, gpui::ScrollStrategy::Center);
        cx.notify();
    }

    /// Reveal the row for a linked worktree's uncommitted changes, locating it by
    /// the commit that worktree has checked out.
    pub(in crate::view) fn request_reveal_worktree(
        &mut self,
        repo_id: RepoId,
        commit_id: CommitId,
        fallback_scope: Option<LogScope>,
        worktree_path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        self.store.dispatch(Msg::SelectWorktreeUncommitted {
            repo_id,
            path: worktree_path.clone(),
        });
        self.request_reveal_commit_inner(
            repo_id,
            commit_id,
            fallback_scope,
            Some(worktree_path),
            cx,
        );
    }

    fn request_reveal_commit_inner(
        &mut self,
        repo_id: RepoId,
        commit_id: CommitId,
        fallback_scope: Option<LogScope>,
        worktree_path: Option<PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = PendingHistoryReveal {
            repo_id,
            commit_id,
            fallback_scope,
            worktree_path,
        };
        if self.pending_history_reveal.as_ref() != Some(&next) {
            self.pending_history_reveal = Some(next);
        }
        self.drive_pending_history_reveal(cx);
        cx.notify();
    }

    pub(in crate::view) fn set_selected_branch(
        &mut self,
        repo_id: RepoId,
        section: BranchSection,
        name: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = Some(SelectedBranch {
            repo_id,
            section,
            name: name.to_string(),
        });
        if self.selected_branch.as_ref() == next.as_ref() {
            return;
        }
        self.selected_branch = next;
        cx.notify();
    }

    pub(in super::super) fn selected_branch_for_history_row(
        &self,
        repo_id: RepoId,
        selected: bool,
    ) -> Option<SelectedHistoryBranch> {
        selected_branch_for_history_row(self.selected_branch.as_ref(), repo_id, selected)
    }

    pub(in super::super) fn history_visible_column_preferences(&self) -> (bool, bool, bool, bool) {
        (
            self.history_show_graph,
            self.history_show_author,
            self.history_show_date,
            self.history_show_sha,
        )
    }

    pub(in super::super) fn history_visible_columns(&self) -> (bool, bool, bool, bool) {
        let available = self.history_content_width;
        let layout = HistoryColumnDragLayout {
            show_graph: self.history_show_graph,
            show_author: self.history_show_author,
            show_date: self.history_show_date,
            show_sha: self.history_show_sha,
            branch_w: self.history_col_branch,
            graph_w: self.history_col_graph,
            author_w: self.history_col_author,
            date_w: self.history_col_date,
            sha_w: self.history_col_sha,
        };
        let (show_author, show_date, show_sha) =
            history_visible_columns_for_layout_with_resize_state(
                available,
                layout,
                self.history_col_resize.as_ref(),
                self.ui_scale_percent,
            );
        (self.history_show_graph, show_author, show_date, show_sha)
    }

    pub(in super::super) fn reset_history_column_widths(&mut self) {
        let widths = history_reset_widths_for_available_width(
            self.history_content_width,
            self.history_show_graph,
            (
                self.history_show_author,
                self.history_show_date,
                self.history_show_sha,
            ),
            self.ui_scale_percent,
        );
        self.history_col_branch = widths.branch;
        self.history_col_graph = widths.graph;
        self.history_col_author = widths.author;
        self.history_col_date = widths.date;
        self.history_col_sha = widths.sha;
        self.sync_history_column_design_widths_from_pixels();
        self.history_col_graph_auto = true;
        self.history_col_resize = None;
    }

    pub(in super::super) fn history_column_width_mut(
        &mut self,
        handle: HistoryColResizeHandle,
    ) -> &mut Pixels {
        match handle {
            HistoryColResizeHandle::Branch => &mut self.history_col_branch,
            HistoryColResizeHandle::Graph => &mut self.history_col_graph,
            HistoryColResizeHandle::Author => &mut self.history_col_author,
            HistoryColResizeHandle::Date => &mut self.history_col_date,
            HistoryColResizeHandle::Sha => &mut self.history_col_sha,
        }
    }

    pub(in super::super) fn set_active_context_menu_invoker(
        &mut self,
        next: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.active_context_menu_invoker == next {
            return;
        }
        self.active_context_menu_invoker = next;
        cx.notify();
    }

    pub(in super::super) fn apply_ui_scale_percent(
        &mut self,
        previous_percent: u32,
        next_percent: u32,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_scale_percent == next_percent {
            return;
        }

        debug_assert_eq!(self.ui_scale_percent, previous_percent);
        self.sync_history_column_design_widths_from_pixels();
        self.ui_scale_percent = next_percent;
        self.history_col_resize = None;
        self.sync_history_column_widths_from_design();
        cx.notify();
    }

    pub(in super::super) fn set_date_time_format(
        &mut self,
        next: DateTimeFormat,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.date_time_format == next {
            return;
        }
        self.date_time_format = next;
        cx.notify();
    }

    pub(in super::super) fn set_history_highlight_commit_chain(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_highlight_commit_chain == enabled {
            return;
        }
        self.history_highlight_commit_chain = enabled;
        cx.notify();
    }

    pub(in super::super) fn set_history_relative_dates(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_relative_dates == enabled {
            return;
        }
        self.history_relative_dates = enabled;
        self.ensure_relative_time_tick(cx);
        cx.notify();
    }

    pub(in super::super) fn set_timezone(&mut self, next: Timezone, cx: &mut gpui::Context<Self>) {
        if self.timezone == next {
            return;
        }
        self.timezone = next;
        cx.notify();
    }

    pub(in super::super) fn set_show_timezone(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.show_timezone == enabled {
            return;
        }
        self.show_timezone = enabled;
        cx.notify();
    }

    pub(in super::super) fn history_tag_preferences(&self) -> (bool, bool) {
        (
            self.history_show_tags,
            self.history_auto_fetch_tags_on_repo_activation,
        )
    }

    pub(in super::super) fn set_history_column_preferences(
        &mut self,
        show_graph: bool,
        show_author: bool,
        show_date: bool,
        show_sha: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_show_graph == show_graph
            && self.history_show_author == show_author
            && self.history_show_date == show_date
            && self.history_show_sha == show_sha
        {
            return;
        }

        self.history_show_graph = show_graph;
        self.history_show_author = show_author;
        self.history_show_date = show_date;
        self.history_show_sha = show_sha;
        self.history_col_resize = None;
        cx.notify();
    }

    pub(in super::super) fn set_history_tag_preferences(
        &mut self,
        show_tags: bool,
        auto_fetch_tags_on_repo_activation: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.history_show_tags == show_tags
            && self.history_auto_fetch_tags_on_repo_activation == auto_fetch_tags_on_repo_activation
        {
            return;
        }

        let show_tags_changed = self.history_show_tags != show_tags;
        self.history_show_tags = show_tags;
        self.history_auto_fetch_tags_on_repo_activation = auto_fetch_tags_on_repo_activation;
        if show_tags_changed {
            self.notify_fingerprint = Self::notify_fingerprint_for(&self.state, show_tags);
            self.history_cache_inflight = None;
        }
        cx.notify();
    }

    pub(in super::super) fn set_last_window_size(&mut self, size: Size<Pixels>) {
        self.last_window_size = size;
    }

    pub(in super::super) fn set_history_content_width(&mut self, width: Pixels) {
        self.history_content_width = history_columns_available_width(width);
    }

    pub(in crate::view) fn drive_pending_history_reveal(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(pending) = self.pending_history_reveal.clone() else {
            return;
        };

        let plan = self.ensure_history_list_plan();
        let (
            active_repo_id,
            current_scope,
            log_rev,
            stashes_rev,
            page,
            cache_request_matches,
            decision,
        ) = {
            let active_repo_id = self.active_repo_id();
            let Some(repo) = self.active_repo() else {
                let decision = decide_pending_history_reveal(
                    &pending,
                    active_repo_id,
                    None,
                    None,
                    0,
                    0,
                    false,
                    None,
                    None,
                    false,
                    None,
                    &plan,
                    self.history_selected_list_index_cache.as_ref(),
                );
                return self.finish_pending_history_reveal(decision, pending, None, &plan, cx);
            };

            let current_scope = repo.history_state.history_scope;
            let log_rev = repo.log_rev;
            let stashes_rev = repo.stashes_rev;
            let log_loading_more = repo.history_state.log_loading_more;
            let display_page = Self::display_log_page_for_repo(repo);
            let live_page_has_more = Self::live_log_page_has_more_for_repo(repo);
            let cache_request_matches = display_page.as_ref().is_some_and(|page| {
                let request = self.history_base_cache_request_for_repo(repo, page.as_ref());
                self.history_cache
                    .as_ref()
                    .is_some_and(|cache| cache.base.request == request)
            });
            let visible_indices = if cache_request_matches {
                self.history_cache
                    .as_ref()
                    .map(|cache| &cache.base.visible_indices)
            } else {
                None
            };
            let decision = decide_pending_history_reveal(
                &pending,
                active_repo_id,
                Some(current_scope),
                repo.history_state.selected_commit.as_ref(),
                log_rev,
                stashes_rev,
                log_loading_more,
                display_page.as_deref(),
                live_page_has_more,
                cache_request_matches,
                visible_indices,
                &plan,
                self.history_selected_list_index_cache.as_ref(),
            );

            (
                active_repo_id,
                current_scope,
                log_rev,
                stashes_rev,
                display_page,
                cache_request_matches,
                decision,
            )
        };

        let cache_meta =
            (active_repo_id == Some(pending.repo_id) && page.is_some() && cache_request_matches)
                .then_some((log_rev, stashes_rev, current_scope));

        self.finish_pending_history_reveal(decision, pending, cache_meta, &plan, cx);
    }

    fn finish_pending_history_reveal(
        &mut self,
        decision: PendingHistoryRevealDecision,
        pending: PendingHistoryReveal,
        cache_meta: Option<(u64, u64, LogScope)>,
        plan: &HistoryListPlan,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Some(scope) = decision.set_scope {
            self.store.dispatch(Msg::SetHistoryScope {
                repo_id: pending.repo_id,
                scope,
            });
            return;
        }

        match (&pending.worktree_path, decision.select_commit) {
            // A reveal aimed at a worktree row selects the row, not the commit
            // that located it -- and only when the row is not already selected.
            // This runs on every render of the history panel and the reveal
            // stays pending for as long as pagination takes, so dispatching
            // unconditionally would ask for the same selection every frame.
            (Some(path), _) => {
                let already_selected = self.active_repo().is_some_and(|repo| {
                    repo.history_state.worktree_selection.as_deref() == Some(path.as_path())
                });
                if !already_selected {
                    self.store.dispatch(Msg::SelectWorktreeUncommitted {
                        repo_id: pending.repo_id,
                        path: path.clone(),
                    });
                }
            }
            (None, Some(commit_id)) => self.store.dispatch(Msg::SelectCommit {
                repo_id: pending.repo_id,
                commit_id,
            }),
            (None, None) => {}
        }

        // The worktree row sits one line above the commit that located it, so
        // scroll to the row itself once the plan knows where it landed.
        // Two indices, bound together: the commit's own row, and the row to scroll
        // to -- the worktree's, when the reveal was aimed at one, which sits one
        // line above it.
        let reveal_rows = decision.scroll_to_list_ix.map(|commit_list_ix| {
            let scroll_to = pending
                .worktree_path
                .as_deref()
                .and_then(|path| worktree_row_list_ix(plan, self.active_repo(), path))
                .unwrap_or(commit_list_ix);
            (commit_list_ix, scroll_to)
        });

        if let Some((commit_list_ix, list_ix)) = reveal_rows {
            if let Some((log_rev, stashes_rev, history_scope)) = cache_meta {
                // The cache is keyed on the commit and read back as *its* row,
                // so it takes the commit's own index -- not the worktree row we
                // scrolled to, which sits one line above it.
                set_history_selected_list_index_cache(
                    &mut self.history_selected_list_index_cache,
                    pending.repo_id,
                    log_rev,
                    stashes_rev,
                    history_scope,
                    plan,
                    Some(pending.commit_id.clone()),
                    commit_list_ix,
                );
            }
            self.dismiss_history_refs_hover(cx);
            self.history_scroll
                .scroll_to_item_strict(list_ix, gpui::ScrollStrategy::Center);
        } else if decision.load_more {
            self.store.dispatch(Msg::LoadMoreHistory {
                repo_id: pending.repo_id,
            });
        }

        if decision.clear_pending {
            self.pending_history_reveal = None;
            // The target no longer needs shielding from page reconciliation.
            self.store.dispatch(Msg::FinishCommitReveal {
                repo_id: pending.repo_id,
            });
            cx.notify();
        }
    }
}

// Render impl is in history_panel.rs

// --- History cache methods ---

use worktree_core::domain::{LogPage, LogScope, RemoteBranch, StashEntry};

impl HistoryView {
    /// The lane the selection sits on. Every other lane — and everything else
    /// coloured from a lane, the nodes, the message borders and the graph fade —
    /// washes out against it.
    ///
    /// The anchor is the selected commit, or HEAD while the uncommitted-changes
    /// row holds the selection: those changes sit on HEAD, so selecting that row
    /// lights the lane they will land on rather than leaving the list unwashed.
    /// A multi-selection has no single lane to pick, so nothing washes.
    ///
    /// Memoised because resolving it is a scan of the page — the colour is one
    /// lookup, but pinning it to a row span walks the lane's whole run — and this
    /// is asked once per render rather than once per row.
    pub(in super::super) fn history_selected_lane(
        &mut self,
        show_worktree_summary_row: bool,
    ) -> Option<crate::view::rows::history_graph_paint::SelectedLane> {
        self.history_selection_highlight(show_worktree_summary_row)
            .lane
    }

    /// The selection highlight's row-membership half: whether each visible row
    /// belongs to the branch/commit the selection is anchored to. Shares the
    /// lane memo, so both halves stay keyed on the same anchor.
    pub(in super::super) fn history_related_rows(
        &mut self,
        show_worktree_summary_row: bool,
    ) -> Option<Arc<[bool]>> {
        self.history_selection_highlight(show_worktree_summary_row)
            .related_rows
    }

    /// Both halves of the selection highlight, through the shared memo.
    fn history_selection_highlight(
        &mut self,
        show_worktree_summary_row: bool,
    ) -> HistorySelectionHighlight {
        let Some((repo_id, anchor)) = self.selection_highlight_anchor(show_worktree_summary_row)
        else {
            return HistorySelectionHighlight::default();
        };

        let Some(cache) = self
            .history_cache
            .as_ref()
            .filter(|cache| cache.base.request.repo_id == repo_id)
        else {
            return HistorySelectionHighlight::default();
        };
        let base_request = &cache.base.request;

        if let Some(memo) = &self.history_selected_lane_color_cache
            && memo.base_request == *base_request
            && memo.anchor == anchor
        {
            return HistorySelectionHighlight {
                lane: memo.lane,
                related_rows: memo.related_rows.clone(),
            };
        }

        let highlight = build_selection_highlight(cache, anchor.clone());
        self.history_selected_lane_color_cache = Some(HistorySelectedLaneColorCache {
            base_request: base_request.clone(),
            anchor,
            lane: highlight.lane,
            related_rows: highlight.related_rows.clone(),
        });
        highlight
    }

    /// What the selection highlight is anchored to, or `None` when nothing is
    /// highlighted: the feature is off, a multi-selection is active, no repo
    /// is open, or no single row carries the selection.
    fn selection_highlight_anchor(
        &self,
        show_worktree_summary_row: bool,
    ) -> Option<(RepoId, HistoryLaneAnchor)> {
        if !self.history_highlight_commit_chain {
            return None;
        }
        let repo = self.active_repo()?;
        if repo.history_state.multi_selection.is_multi() {
            return None;
        }
        // A selected worktree row highlights that worktree's branch, not the
        // commit underneath it -- the two differ whenever the branch is
        // behind and has been given a lane of its own.
        let worktree_anchor = repo
            .history_state
            .worktree_selection
            .as_ref()
            .and_then(|path| match &repo.worktree_dirty {
                Loadable::Ready(dirty) => dirty.iter().find(|summary| &summary.path == path),
                _ => None,
            })
            .and_then(|summary| {
                Some(HistoryLaneAnchor::Worktree {
                    head: summary.head.clone()?,
                    on_branch: summary.branch.is_some() && !summary.detached,
                })
            });
        let anchor = worktree_anchor.or_else(|| {
            // The uncommitted sentinel names the pinned working-tree row, not
            // any commit in the log -- its chain highlight is HEAD's, exactly
            // as when the row is selected without a `selected_commit`.
            repo.history_state
                .selected_commit
                .clone()
                .filter(|commit_id| !commit_id.is_uncommitted())
                .or_else(|| {
                    show_worktree_summary_row
                        .then(|| repo.head_commit_id())
                        .flatten()
                })
                .map(HistoryLaneAnchor::Commit)
        })?;
        Some((repo.id, anchor))
    }

    /// Builds (or reuses) the mapping from list indices to rows.
    ///
    /// A dirty worktree only earns a row when its HEAD is one of the commits
    /// currently on screen — anchoring it anywhere else would misstate which
    /// commit the changes sit on top of. Worktrees whose HEAD has scrolled out
    /// of the loaded page, or that are on a branch outside the current scope,
    /// simply do not appear.
    pub(in super::super) fn ensure_history_list_plan(&mut self) -> HistoryListPlan {
        let (show_working_tree_summary_row, _) = self.ensure_history_worktree_summary_cache();

        let Some(repo) = self.active_repo() else {
            self.history_list_plan_cache = None;
            return HistoryListPlan::new(show_working_tree_summary_row, Vec::new());
        };
        let repo_id = repo.id;
        let worktrees_rev = repo.worktrees_rev;
        let worktree_dirty_rev = repo.worktree_dirty_rev;

        let Some(cache) = self
            .history_cache
            .as_ref()
            .filter(|cache| cache.base.request.repo_id == repo_id)
        else {
            self.history_list_plan_cache = None;
            return HistoryListPlan::new(show_working_tree_summary_row, Vec::new());
        };
        let base_request = &cache.base.request;

        if let Some(cached) = &self.history_list_plan_cache
            && cached.base_request == *base_request
            && cached.worktrees_rev == worktrees_rev
            && cached.worktree_dirty_rev == worktree_dirty_rev
            && cached.show_working_tree_summary_row == show_working_tree_summary_row
        {
            return cached.plan.clone();
        }

        let anchors = (|| {
            let Loadable::Ready(dirty) = &repo.worktree_dirty else {
                return Vec::new();
            };
            if dirty.is_empty() {
                return Vec::new();
            }

            // The base cache already indexed the page by commit id, off the render
            // path. Rebuilding that map here would walk every visible commit on
            // every scan revision to answer one lookup per dirty worktree.
            dirty
                .iter()
                .enumerate()
                .filter_map(|(worktree_ix, summary)| {
                    let head = summary.head.as_ref()?;
                    let visible_ix = cache.base.visible_ix_by_commit.get(head).copied()?;
                    Some(HistoryWorktreeRowAnchor {
                        visible_ix,
                        worktree_ix,
                    })
                })
                .collect()
        })();

        let plan = HistoryListPlan::new(show_working_tree_summary_row, anchors);
        // Cloned here rather than up front so a cache hit -- the common case, once
        // per render -- costs a comparison and nothing else.
        let base_request = base_request.clone();
        self.history_list_plan_cache = Some(HistoryListPlanCache {
            base_request,
            worktrees_rev,
            worktree_dirty_rev,
            show_working_tree_summary_row,
            plan: plan.clone(),
        });
        plan
    }

    pub(in super::super) fn ensure_history_worktree_summary_cache(
        &mut self,
    ) -> (bool, (usize, usize, usize)) {
        enum Action {
            Clear,
            CacheOk {
                show_row: bool,
                counts: (usize, usize, usize),
            },
            Rebuild {
                repo_id: RepoId,
                worktree_status_rev: u64,
                staged_status_rev: u64,
                show_row: bool,
                counts: (usize, usize, usize),
            },
        }

        let action = (|| {
            let Some(repo) = self.active_repo() else {
                return Action::Clear;
            };
            let worktree = repo.worktree_status_entries();
            let staged = repo.staged_status_entries();
            if worktree.is_none() && staged.is_none() {
                return Action::Clear;
            }

            let worktree_status_rev = repo.worktree_status_cache_rev();
            let staged_status_rev = repo.staged_status_cache_rev();

            if let Some(cache) = &self.history_worktree_summary_cache
                && cache.repo_id == repo.id
                && cache.worktree_status_rev == worktree_status_rev
                && cache.staged_status_rev == staged_status_rev
            {
                return Action::CacheOk {
                    show_row: cache.show_row,
                    counts: cache.counts,
                };
            }

            // Shared with the per-worktree scan so the two rows can never
            // report the same tree differently.
            let count_for = worktree_core::domain::count_file_statuses;

            let unstaged_counts = worktree.map_or((0, 0, 0), count_for);
            let staged_counts = staged.map_or((0, 0, 0), count_for);
            let show_row = worktree.is_some_and(|entries| !entries.is_empty())
                || staged.is_some_and(|entries| !entries.is_empty());
            let counts = (
                unstaged_counts.0 + staged_counts.0,
                unstaged_counts.1 + staged_counts.1,
                unstaged_counts.2 + staged_counts.2,
            );

            Action::Rebuild {
                repo_id: repo.id,
                worktree_status_rev,
                staged_status_rev,
                show_row,
                counts,
            }
        })();

        match action {
            Action::Clear => {
                self.history_worktree_summary_cache = None;
                (false, (0, 0, 0))
            }
            Action::CacheOk { show_row, counts } => (show_row, counts),
            Action::Rebuild {
                repo_id,
                worktree_status_rev,
                staged_status_rev,
                show_row,
                counts,
            } => {
                self.history_worktree_summary_cache = Some(HistoryWorktreeSummaryCache {
                    repo_id,
                    worktree_status_rev,
                    staged_status_rev,
                    show_row,
                    counts,
                });
                (show_row, counts)
            }
        }
    }

    pub(in super::super) fn ensure_history_stash_ids_cache(
        &mut self,
    ) -> Option<Arc<FxHashSet<CommitId>>> {
        enum Action {
            Clear,
            CacheOk(Arc<FxHashSet<CommitId>>),
            Rebuild {
                repo_id: RepoId,
                stashes_rev: u64,
                ids: Arc<FxHashSet<CommitId>>,
            },
        }

        let action = (|| {
            let Some(repo) = self.active_repo() else {
                return Action::Clear;
            };
            let Loadable::Ready(stashes) = &repo.stashes else {
                return Action::Clear;
            };
            if stashes.is_empty() {
                return Action::Clear;
            }

            let stashes_rev = repo.stashes_rev;
            if let Some(cache) = &self.history_stash_ids_cache
                && cache.repo_id == repo.id
                && cache.stashes_rev == stashes_rev
            {
                return Action::CacheOk(Arc::clone(&cache.ids));
            }

            let ids: FxHashSet<_> = stashes.iter().map(|s| s.id.clone()).collect();
            let ids = Arc::new(ids);
            Action::Rebuild {
                repo_id: repo.id,
                stashes_rev,
                ids: Arc::clone(&ids),
            }
        })();

        match action {
            Action::Clear => {
                self.history_stash_ids_cache = None;
                None
            }
            Action::CacheOk(ids) => Some(ids),
            Action::Rebuild {
                repo_id,
                stashes_rev,
                ids,
            } => {
                self.history_stash_ids_cache = Some(HistoryStashIdsCache {
                    repo_id,
                    stashes_rev,
                    ids: Arc::clone(&ids),
                });
                Some(ids)
            }
        }
    }

    pub(in super::super) fn ensure_history_cache(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(repo) = self.active_repo() else {
            self.history_cache_inflight = None;
            self.history_cache = None;
            return;
        };
        let Some(page) = Self::display_log_page_for_repo(repo) else {
            self.history_cache_inflight = None;
            self.history_cache = None;
            return;
        };

        let base_request = self.history_base_cache_request_for_repo(repo, page.as_ref());
        let decoration_request =
            self.history_decoration_cache_request_for_repo(repo, page.as_ref());
        let request_for_task = HistoryCacheBuildRequest {
            base_request: base_request.clone(),
            decoration_request: decoration_request.clone(),
        };

        let cache_ok = self.history_cache.as_ref().is_some_and(|cache| {
            cache.base.request == base_request && cache.decorations.request == decoration_request
        });
        if cache_ok {
            self.history_cache_inflight = None;
            return;
        }
        if self.history_cache_inflight.as_ref() == Some(&request_for_task) {
            return;
        }

        let base_reuse = self
            .history_cache
            .as_ref()
            .filter(|cache| cache.base.request == base_request)
            .map(|cache| cache.base.clone());
        let head_branch = match &repo.head_branch {
            Loadable::Ready(h) => Some(h.clone()),
            _ => None,
        };
        let branches = match &repo.branches {
            Loadable::Ready(b) => Arc::clone(b),
            _ => Arc::new(Vec::new()),
        };
        let remote_branches = match &repo.remote_branches {
            Loadable::Ready(b) => Arc::clone(b),
            _ => Arc::new(Vec::new()),
        };
        let tags = if self.history_show_tags {
            match &repo.tags {
                Loadable::Ready(t) => Arc::clone(t),
                _ => Arc::new(Vec::new()),
            }
        } else {
            Arc::new(Vec::new())
        };
        let stashes = match &repo.stashes {
            Loadable::Ready(s) => Arc::clone(s),
            _ => Arc::new(Vec::new()),
        };

        self.history_cache_seq = self.history_cache_seq.wrapping_add(1);
        let seq = self.history_cache_seq;
        self.history_cache_inflight = Some(request_for_task.clone());

        let theme = self.theme;

        cx.spawn(
            async move |view: WeakEntity<HistoryView>, cx: &mut gpui::AsyncApp| {
                let request_for_update = request_for_task.clone();
                let base_request_for_build = request_for_task.base_request.clone();
                let decoration_request_for_build = request_for_task.decoration_request.clone();

                let build_rebuild = move || {
                    let base = base_reuse.unwrap_or_else(|| {
                        build_history_base_cache(
                            base_request_for_build,
                            page.as_ref(),
                            theme,
                            head_branch.as_deref(),
                            branches.as_ref(),
                            remote_branches.as_ref(),
                            stashes.as_ref(),
                        )
                    });
                    let decorations = build_history_decoration_cache(
                        decoration_request_for_build,
                        page.as_ref(),
                        &base,
                        head_branch.as_deref(),
                        branches.as_ref(),
                        remote_branches.as_ref(),
                        tags.as_ref(),
                    );

                    HistoryCache { base, decorations }
                };

                let rebuild: HistoryCache =
                    if crate::ui_runtime::current().uses_background_compute() {
                        smol::unblock(build_rebuild).await
                    } else {
                        build_rebuild()
                    };

                let _ = view.update(cx, |this, cx| {
                    if this.history_cache_seq != seq {
                        return;
                    }
                    if this.history_cache_inflight.as_ref() != Some(&request_for_update) {
                        return;
                    }
                    if this.active_repo_id() != Some(request_for_update.base_request.repo_id) {
                        return;
                    }

                    if this.history_col_graph_auto && this.history_col_resize.is_none() {
                        let required = history_scaled_px(
                            HISTORY_GRAPH_MARGIN_X_PX * 2.0
                                + HISTORY_GRAPH_COL_GAP_PX * (rebuild.base.max_lanes as f32),
                            this.ui_scale_percent,
                        );
                        if this.history_show_graph {
                            this.history_col_graph = history_column_drag_next_width(
                                HistoryColResizeHandle::Graph,
                                required.min(history_scaled_px(
                                    HISTORY_COL_GRAPH_MAX_PX,
                                    this.ui_scale_percent,
                                )),
                                this.history_content_width,
                                this.history_show_graph,
                                (
                                    this.history_show_author,
                                    this.history_show_date,
                                    this.history_show_sha,
                                ),
                                HistoryColumnWidths {
                                    branch: this.history_col_branch,
                                    graph: this.history_col_graph,
                                    author: this.history_col_author,
                                    date: this.history_col_date,
                                    sha: this.history_col_sha,
                                },
                                this.ui_scale_percent,
                            );
                            this.history_col_graph_design = this
                                .ui_scale()
                                .design_units_from_pixels(this.history_col_graph);
                        }
                    }

                    this.history_cache_inflight = None;
                    this.history_cache = Some(rebuild);
                    cx.notify();
                });
            },
        )
        .detach();
    }

    fn log_fingerprint(commits: &[Commit]) -> u64 {
        let mut hasher = FxHasher::default();
        commits.len().hash(&mut hasher);
        for id in commits.iter().take(3).map(|c| c.id.as_ref()) {
            id.hash(&mut hasher);
        }
        for id in commits.iter().rev().take(3).map(|c| c.id.as_ref()) {
            id.hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests;
