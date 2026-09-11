//! Conflict resolver actions and state sync for [`MainPaneView`].
//!
//! Facade over the five domain modules: conflict navigation in
//! `conflict_nav`, bootstrap/session sync in `bootstrap`, picks in `pick`,
//! resolved-output editing in `output_edit` and row selection/manual
//! alignment in `alignment`. Extracted from `diff_interaction.rs`: mergetool
//! bootstrap tracing, conflict navigation, pick/choice application, output
//! editing ops, session resolution sync, and autosolve dispatch. See
//! UI_DESIGN.md section 30.

use super::core_impl::uniform_list_base_handle;
#[cfg(test)]
use super::helpers::conflict_marker_nav_entries_from_markers;
use super::helpers::{
    ResolvedOutputSourceRevision, append_choice_after_conflict_block, append_line_insertion_text,
    centered_reveal_scroll_y, conflict_group_indices_for_choice,
    conflict_group_member_indices_for_ix, conflict_region_index_is_unique,
    first_output_marker_line_for_conflict, line_content_byte_range_for_index,
    output_line_range_for_conflict_block_in_text, reset_conflict_block_selection,
    resolved_outline_delta_for_snapshot_transition, resolved_output_marker_for_line,
    resolved_output_markers_for_text, source_line_count, split_line_count,
    split_target_conflict_block_into_subchunks,
};
use super::*;
use crate::kit::text_model::TextModelSnapshot;
use worktree_core::mergetool_trace::{self};

mod alignment;
mod bootstrap;
mod conflict_nav;
mod output_edit;
mod pick;
