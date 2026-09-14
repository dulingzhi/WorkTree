//! `PopoverHost` accessors the tests read state and geometry through.

#[cfg(test)]
use super::super::*;
#[cfg(test)]
use super::kinds::PopoverKind;
use super::popover_host::PopoverHost;

// @split-module: impl_tests_api
impl PopoverHost {
    #[cfg(test)]
    pub(in crate::view) fn create_branch_input_focus_handle_for_test(
        &self,
        app: &App,
    ) -> FocusHandle {
        self.create_branch
            .create_branch_input
            .read(app)
            .focus_handle()
    }

    /// The history author filter's search box, once its popover has opened it.
    #[cfg(test)]
    pub(in crate::view) fn history_author_filter_search_input_for_test(
        &self,
    ) -> Option<&Entity<components::TextInput>> {
        self.history_author_filter
            .history_author_filter_search_input
            .as_ref()
    }

    /// Scrolls the author dropdown to a displayed row exactly as its keyboard
    /// navigation does.
    #[cfg(test)]
    pub(in crate::view) fn scroll_history_author_filter_to_item_for_test(
        &mut self,
        ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        self.scroll_history_author_filter_to_row(ix, cx);
    }

    #[cfg(test)]
    pub(in crate::view) fn popover_kind_for_tests(&self) -> Option<PopoverKind> {
        self.popover.clone()
    }

    #[cfg(test)]
    pub(in crate::view) fn popover_opened_from_diff_panel_for_tests(&self) -> bool {
        self.popover_opened_from_diff_panel
    }

    /// The box the open popover hangs off, when it was anchored to one.
    #[cfg(test)]
    pub(in crate::view) fn popover_anchor_bounds_for_tests(&self) -> Option<Bounds<Pixels>> {
        match self.popover_anchor {
            Some(PopoverAnchor::Bounds(bounds)) => Some(bounds),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(in crate::view) fn worktree_path_input_text_for_tests(&self, app: &gpui::App) -> String {
        self.worktree_add
            .worktree_path_input
            .read(app)
            .text()
            .to_string()
    }

    #[cfg(test)]
    pub(in crate::view) fn worktree_ref_source_target_for_tests(&self) -> &str {
        &self.worktree_add.worktree_ref_source_target
    }
}
