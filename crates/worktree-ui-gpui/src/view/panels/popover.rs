use super::*;
use worktree_core::services::InteractiveRebaseAction;

mod add_repo_menu;
mod add_to_gitignore_prompt;
mod agent_sessions;
mod app_menu;
mod assume_unchanged_manager;
mod author_filter;
mod branch_picker;
mod checkout_remote_branch_prompt;
mod cherry_pick_commit_confirm;
mod clone_repo;
mod commit_prompt;
mod commit_search_picker;
pub(in super::super) mod context_menu;
mod create_branch_from_ref_prompt;
mod create_tag_prompt;
mod delete_branches_confirm;
mod delete_remote_branch_confirm;
mod dialog;
mod discard_all_confirm;
mod discard_changes_confirm;
mod dispatch;
mod file_history;
mod fingerprint;
mod force_delete_branch_confirm;
mod force_push_confirm;
mod force_remove_worktree_confirm;
mod geometry;
mod history_ref_filter;
mod host;
mod hunk_explanation;
mod merge_abort_confirm;
mod merge_commit_confirm;
pub(in crate::view) mod merge_request_push;
mod merge_request_push_description;
mod open;
mod picker_nav;
mod picker_row_menu;
mod pull_reconcile_prompt;
mod push_set_upstream_prompt;
mod rebase_onto_confirm;
mod remote_add_prompt;
mod remote_edit_url_prompt;
mod remote_picker;
mod remote_remove_confirm;
mod remote_ssh_key_prompt;
mod rename_branch_prompt;
mod repo_picker;
mod repo_settings;
mod reset_prompt;
mod rows_cache;
mod search_inputs;
mod squash_prompt;
mod stage_conflict_markers_confirm;
mod stash_branch_prompt;
mod stash_drop_confirm;
mod stash_picker_prompt;
mod stash_prompt;
mod statistics;
mod submit;
mod submodule_add_prompt;
mod submodule_change_pointer_prompt;
mod submodule_picker;
mod submodule_remove_confirm;
mod submodule_trust_confirm;
mod tag_picker;
mod terminal_shutdown_confirm;
pub(crate) mod undo_last_action;
mod unsaved_file_edits_confirm;
mod upstream_picker;
mod workspace_picker;
mod worktree_add_prompt;
mod worktree_picker;
mod worktree_remove_confirm;

// The host mechanism lives in the domain modules below; these named
// re-exports keep every `popover::` path that the panels tree, the prompt
// modules and the tests consume working unchanged.

// Host taxonomy and record.
#[cfg(test)]
pub(in crate::view) use host::RemoteRow;
pub(in crate::view) use host::{
    AutosquashMode, BranchPickerPurpose, PopoverHost, PopoverKind, RemotePickerPurpose,
    RemotePopoverKind, RepoPopoverKind, StashPickerPurpose, SubmodulePopoverKind,
    WorktreePopoverKind,
};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use host::{benchmark_branch_checkout_rows, benchmark_workspace_rows};

// Geometry: popover sizes, anchors and UI-scale helpers.
pub(in crate::view) use dialog::focusable_toggle_row;
pub(in crate::view::panels) use geometry::popover_scaled_px_fn;
pub(in crate::view) use geometry::{
    PopoverWidthSpec, popover_scaled_px, popover_scaled_px_from_percent, popover_ui_scale,
    popover_ui_scale_percent, popover_width_spec,
};

// Prompt dialog widgets and focus navigation.
pub(in crate::view::panels) use dialog::{
    ConfirmDialog, DialogFocus, cancel_button, cancel_button_labeled, dialog_cancel_button,
    dialog_divider, hotkey_hint, input_label, is_submittable_branch_name, popover_title,
};

// Names that stay private to the popover subtree: the prompt modules and the
// domain files resolve them through `use super::*`, so they stay nameable here.
use geometry::{
    DEFAULT_CONTEXT_MENU_WIDTH, DIALOG_360_WIDTH, DIALOG_380_WIDTH, DIALOG_420_WIDTH,
    DIALOG_440_WIDTH, DIALOG_460_WIDTH, DIALOG_540_WIDTH, HISTORY_AUTHOR_FILTER_WIDTH,
    HISTORY_REF_FILTER_WIDTH, LARGE_PICKER_WIDTH, PICKER_WIDTH, PopoverAnchor, REPO_TAB_MENU_WIDTH,
    choose_popover_anchor_corner, popover_anchor_corner, popover_preferred_anchor_width,
};
use open::{popover_is_confirm_dialog, popover_is_context_menu};

#[cfg(test)]
mod tests;
