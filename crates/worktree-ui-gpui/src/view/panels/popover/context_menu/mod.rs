//! The context menu: entry models, activation, and the file/ignore/discard
//! actions entries dispatch to.
//!
//! Every item is re-exported below, so `context_menu::X` still names the same thing it always did.

mod helpers;
mod impl_discard;
mod impl_gitignore;
mod impl_menu;
mod impl_paths;

mod branch;
mod branch_group;
mod branch_section;
mod browse_history;
mod change_tracking_settings;
mod commit;
mod commit_file;
mod commit_options;
mod commit_sha_link;
mod conflict_resolver_chunk;
mod conflict_resolver_input_row;
mod conflict_resolver_output;
mod diff_actions;
mod diff_content_mode_settings;
mod diff_editor;
mod diff_hunk;
mod file_browser_file;
mod file_browser_folder;
mod history_branch_filter;
mod mergetool_settings;
mod pinned_section;
mod previous_commit_messages;
mod pull;
mod pull_request;
mod push;
mod reflog_entry;
mod remote;
mod repo_picker_row;
mod repo_tab;
mod stash;
mod status_file;
mod submodule;
mod submodule_inner_diff;
mod submodule_section;
mod tag;
mod terminal;
#[cfg(test)]
mod tests;
mod ui_scale_picker;
mod web_link;
mod worktree;
mod worktree_section;

use super::*;

#[cfg(test)]
use helpers::context_menu_entry_tooltip;
pub(in crate::view::panels::popover) use helpers::push_copy_path_entries;
use helpers::{action_menu_title, active_branch_tracking_upstream_name};
#[cfg(test)]
pub(in super::super) use helpers::{
    context_menu_activate_entry_ix, context_menu_shortcut_entry_ix,
};
