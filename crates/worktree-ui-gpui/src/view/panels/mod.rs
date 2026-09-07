use super::components::{ContextMenuItem, ContextMenuModel, ContextMenuRows, ContextMenuSegment};
use super::*;

pub(in crate::view) const COMMIT_DETAILS_MESSAGE_MAX_HEIGHT_PX: f32 = 240.0;
pub(in crate::view) const COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX: f32 = 200.0;

// The menu-action types sink to the components leaf layer, next to the menu
// model that carries them; the re-export keeps every `panels::...` consumer
// path resolving unchanged.
pub(in crate::view) use crate::view::components::{
    AddRepoMenuAction, AppMenuAction, ContextMenuAction,
};

// HistoryColResizeDragGhost moved to view/mod.rs for accessibility from panes::HistoryView.

mod action_bar;
mod bars;
mod bottom_status_bar;
pub(in crate::view) mod layout;
mod popover;
mod reflog_host;
mod repo_tabs_bar;

pub(super) use action_bar::{ActionBarView, action_bar_height};
pub(super) use bottom_status_bar::BottomStatusBarView;
pub(super) use popover::PopoverHost;
#[cfg(test)]
pub(super) use popover::RemoteRow;
pub(in crate::view) use popover::merge_request_push::git_output;
/// The reflog pane's header button asks whether an undo is available before
/// rendering, so the resolver ships one hop out of the private popover tree.
pub(in crate::view) use popover::undo_last_action::{UndoResolution, resolve_undo};
pub(super) use popover::{
    AutosquashMode, BranchPickerPurpose, PopoverKind, RemotePickerPurpose, RemotePopoverKind,
    RepoPopoverKind, StashPickerPurpose, SubmodulePopoverKind, WorktreePopoverKind,
};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use popover::{benchmark_branch_checkout_rows, benchmark_workspace_rows};
/// Layout guards outside this module assert against the tab padding, so they
/// follow the constant instead of hardcoding the current value.
#[cfg(test)]
pub(in crate::view) use repo_tabs_bar::REPO_TAB_SIDE_PADDING_PX;
pub(super) use repo_tabs_bar::RepoTabsBarView;
#[allow(unused_imports)]
pub(in crate::view) use repo_tabs_bar::repo_tab_insert_before_for_drag_cursor;

#[cfg(test)]
mod tests;
