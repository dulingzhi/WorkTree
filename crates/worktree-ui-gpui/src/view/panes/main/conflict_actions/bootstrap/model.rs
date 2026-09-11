//! The bootstrap model: what a resolver sync is asked to do and what it produced.

use super::trace::{MergetoolBootstrapTraceDecisions, MergetoolTraceContext};

use crate::view::caches::DeferredLineStarts;
use crate::view::conflict_resolver;
use crate::view::conflict_resolver::ConflictModeState;
use crate::view::conflict_resolver::ConflictResolverViewMode;
use crate::view::conflict_resolver::ThreeWaySides;
use crate::view::preview_kind::ConflictResolverPreviewMode;
use crate::view::rows;
use gpui::SharedString;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use worktree_state::model::RepoId;
/// Render the current semantic plan decisions into the marker/text projection
/// consumed by the resolver UI. This differs from `marker_projection`, which
/// intentionally remains the immutable structural baseline used to detect
/// protected worktree edits.
pub(super) fn conflict_session_plan_projection(
    session: &worktree_core::conflict_session::ConflictSession,
) -> Option<(Arc<str>, Vec<usize>)> {
    let mut projection = session.merge_plan.clone()?;
    // ConflictRegion remains the compatibility/autosolve model for original
    // marker blocks. Keep those blocks present in this structural projection;
    // their live choices are applied to parsed blocks below. Plan-only deltas
    // retain their current selection, which is the structural gap this path
    // closes.
    for block_index in &session.region_plan_blocks {
        projection.replace_selection(*block_index, worktree_core::merge::OrderedSelection::new());
    }
    let projected_plan_blocks = projection.unresolved_blocks.clone();
    let options = worktree_core::merge::MergeOptions {
        style: if projection.has_base() {
            worktree_core::merge::ConflictStyle::Diff3
        } else {
            worktree_core::merge::ConflictStyle::Merge
        },
        ..Default::default()
    };
    Some((
        Arc::from(worktree_core::merge::render_merge_plan(&projection, &options).output),
        projected_plan_blocks,
    ))
}

/// The conflicted working-tree entry [`MainPaneView::sync_conflict_resolver`]
/// binds to: the repo, the file path and the conflict kind, resolved by the
/// gate phase before any resolver state is touched.
pub(super) struct ConflictSyncTarget {
    pub(super) repo_id: RepoId,
    pub(super) path: PathBuf,
    pub(super) conflict_kind: Option<worktree_core::domain::FileConflictKind>,
}

/// Argument pack for the bootstrap phases, bundling the values the gate and
/// strategy selection produce so the phase functions stay small in arity.
/// Every field is read at the same program point the pre-split monolith read
/// it; the pack only carries the values across the call boundary.
pub(super) struct ConflictBootstrapInput {
    pub(super) repo_id: RepoId,
    pub(super) path: PathBuf,
    pub(super) shared_path: worktree_state::msg::RepoPath,
    pub(super) conflict_syntax_language: Option<rows::DiffSyntaxLanguage>,
    pub(super) source_hash: u64,
    pub(super) conflict_strategy: Option<worktree_core::conflict_session::ConflictResolverStrategy>,
    pub(super) conflict_kind: Option<worktree_core::domain::FileConflictKind>,
    pub(super) needs_full_side_payloads: bool,
    pub(super) conflict_rev: u64,
}

/// The computed half of a text-conflict bootstrap: the values that move into
/// [`ConflictResolverUiState`] at install time, plus [`ConflictBootstrapPost`]
/// for everything the finish phase still needs afterwards (trace context,
/// counters, the resolved output and the fresh-open guards). Field values are
/// produced by exactly the statements the pre-split monolith ran, in order.
pub(super) struct ConflictBootstrap {
    pub(super) post: ConflictBootstrapPost,
    pub(super) marker_snapshot: Option<Arc<str>>,
    pub(super) output_is_protected: bool,
    pub(super) marker_segments: Vec<conflict_resolver::ConflictSegment>,
    pub(super) collapse_context: bool,
    pub(super) conflict_region_indices: Vec<usize>,
    pub(super) display_plan_block_indices: Vec<usize>,
    pub(super) conflict_region_marker_has_base: Vec<bool>,
    pub(super) active_conflict: Option<usize>,
    pub(super) nav_targets: Vec<conflict_resolver::ConflictNavTarget>,
    pub(super) original_region_aligned_ranges: Vec<Option<std::ops::Range<usize>>>,
    pub(super) mode_state: ConflictModeState,
    pub(super) view_mode: ConflictResolverViewMode,
    pub(super) three_way_text: ThreeWaySides<SharedString>,
    pub(super) three_way_line_starts: ThreeWaySides<DeferredLineStarts>,
    pub(super) three_way_len: usize,
    pub(super) three_way_aligned: conflict_resolver::ThreeWayAlignedMap,
    pub(super) merge_plan_aligned_conflict_ranges: Option<Vec<std::ops::Range<usize>>>,
    pub(super) three_way_word_highlights: ThreeWaySides<conflict_resolver::WordHighlights>,
    pub(super) two_way_aligned_word_highlights:
        FxHashMap<usize, conflict_resolver::TwoWayWordHighlightPair>,
    pub(super) nav_anchor: Option<conflict_resolver::ConflictNavAnchor>,
    pub(super) hide_resolved: bool,
    pub(super) last_autosolve_summary: Option<SharedString>,
    pub(super) open_summary_counts: Option<conflict_resolver::ConflictSummaryCounts>,
    pub(super) open_summary_announced: bool,
    pub(super) resolver_preview_mode: ConflictResolverPreviewMode,
}

/// See [`ConflictBootstrap`].
pub(super) struct ConflictBootstrapPost {
    pub(super) trace_ctx: MergetoolTraceContext,
    pub(super) trace_decisions: MergetoolBootstrapTraceDecisions,
    pub(super) bootstrap_started: Instant,
    pub(super) conflict_block_count: usize,
    pub(super) diff_row_count: usize,
    pub(super) inline_row_count: usize,
    pub(super) resolved_line_count: Option<usize>,
    pub(super) resolved_output_text: Option<conflict_resolver::ResolvedOutputText>,
    pub(super) streamed_output_projection: Option<conflict_resolver::ResolvedOutputProjection>,
    pub(super) is_same_conflict: bool,
    pub(super) needs_full_side_texts: bool,
    pub(super) full_text_plan_upgrade_expected: bool,
    pub(super) three_way_needs_background: ThreeWaySides<bool>,
}
