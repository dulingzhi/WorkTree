//! Side-by-side file diff planning and row materialization.
//!
//! Split out of the former single-file `file_diff` module along its six
//! boundaries. Every item is re-exported below, so `file_diff::X` still
//! names the same thing it always did.

mod align;
#[cfg(feature = "benchmarks")]
mod benchmark;
mod levenshtein;
mod line_text;
mod plan;
mod rows_anchors;

#[cfg(test)]
mod tests;

pub(crate) use align::{histogram_edits, myers_edits, split_lines};
#[cfg(feature = "benchmarks")]
pub use benchmark::{
    BenchmarkReplacementDistanceBackend, benchmark_side_by_side_plan_with_replacement_backend,
};
pub use line_text::{FileDiffEofNewline, FileDiffLineText, FileDiffRowKind};
pub(crate) use plan::{DiffHunk, Edit, EditKind, edits_to_hunks_with, reconstruct_side_with};
pub use plan::{
    FileDiffPlan, FileDiffPlanRun, PlanRowView, append_side_by_side_rows_with_offsets,
    for_each_side_by_side_row, plan_changed_line_masks, plan_emitted_line_prefix_counts,
    plan_line_to_row_maps, plan_row_region_anchors, side_by_side_plan,
    side_by_side_plan_from_lines, side_by_side_rows, side_by_side_rows_with_anchors,
};
pub use rows_anchors::{
    FileDiffAnchors, FileDiffRegionAnchor, FileDiffRow, FileDiffRowAnchor, FileDiffRowsWithAnchors,
};
