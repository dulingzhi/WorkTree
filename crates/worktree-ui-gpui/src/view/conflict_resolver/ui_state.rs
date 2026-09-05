use super::three_way::{ThreeWayColumn, ThreeWaySides};
use super::*;
use crate::view::caches::{DeferredLineStarts, LoadableImagePreview, LoadableMarkdownDoc};
use crate::view::diff_prefs::DiffWhitespaceMode;
use crate::view::preview_kind::ConflictResolverPreviewMode;
use gpui::SharedString;
use rustc_hash::{FxHashMap, FxHashSet};
use std::ops::Range;
use std::sync::Arc;
use worktree_state::model::{Loadable, RepoId};
#[derive(Clone, Debug)]
pub(in crate::view) struct ConflictResolverMarkdownPreviewState {
    pub(in crate::view) source_hash: Option<u64>,
    pub(in crate::view) documents: ThreeWaySides<LoadableMarkdownDoc>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum ResolverPickTarget {
    /// Append a specific line from the 3-way resolver pane.
    ThreeWayLine {
        line_ix: usize,
        choice: super::ConflictChoice,
    },
    /// Append a specific line from the 2-way split resolver pane.
    TwoWaySplitLine {
        row_ix: usize,
        side: super::ConflictPickSide,
    },
    /// Pick a full conflict chunk for the requested side.
    Chunk {
        conflict_ix: usize,
        choice: super::ConflictChoice,
        /// Optional resolved-output line that initiated this pick.
        /// When present, chunk pick scopes to the marker chunk at this line.
        output_line_ix: Option<usize>,
    },
}

/// Identity captured when a conflict-region Join entry is built. The action
/// is accepted only while this exact resolver revision remains current, so an
/// open menu cannot join a different pair after region indices shift.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) struct ConflictResolverJoinTarget {
    pub(in crate::view) repo_id: RepoId,
    pub(in crate::view) path: worktree_state::msg::RepoPath,
    pub(in crate::view) conflict_rev: u64,
    pub(in crate::view) first_region_index: usize,
}

impl Default for ConflictResolverMarkdownPreviewState {
    fn default() -> Self {
        Self {
            source_hash: None,
            documents: ThreeWaySides {
                base: Loadable::NotLoaded,
                ours: Loadable::NotLoaded,
                theirs: Loadable::NotLoaded,
            },
        }
    }
}

impl ConflictResolverMarkdownPreviewState {
    pub(in crate::view) fn document(&self, side: ThreeWayColumn) -> &LoadableMarkdownDoc {
        &self.documents[side]
    }
}

#[derive(Clone, Debug)]
pub(in crate::view) struct ConflictResolverImagePreviewState {
    pub(in crate::view) source_hash: Option<u64>,
    pub(in crate::view) path: Option<std::path::PathBuf>,
    pub(in crate::view) images: ThreeWaySides<LoadableImagePreview>,
}

impl Default for ConflictResolverImagePreviewState {
    fn default() -> Self {
        Self {
            source_hash: None,
            path: None,
            images: ThreeWaySides {
                base: Loadable::NotLoaded,
                ours: Loadable::NotLoaded,
                theirs: Loadable::NotLoaded,
            },
        }
    }
}

impl ConflictResolverImagePreviewState {
    pub(in crate::view) fn image(&self, side: ThreeWayColumn) -> &LoadableImagePreview {
        &self.images[side]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct ResolvedOutputConflictMarker {
    pub(in crate::view) conflict_ix: usize,
    pub(in crate::view) range_start: usize,
    pub(in crate::view) range_end: usize,
    pub(in crate::view) is_start: bool,
    pub(in crate::view) is_end: bool,
    pub(in crate::view) unresolved: bool,
}

/// Resolved-output outline metadata: per-line provenance, conflict markers, and source index.
/// Shared between visible state (`ConflictResolverUiState`) and incremental-recompute stash.
#[derive(Clone, Debug, Default)]
pub(in crate::view) struct ResolvedOutlineData {
    /// Per-line provenance metadata.
    pub(in crate::view) meta: Vec<super::ResolvedLineMeta>,
    /// Per-line conflict marker metadata for gutter markers.
    pub(in crate::view) markers: Vec<Option<ResolvedOutputConflictMarker>>,
    /// Source line keys currently represented in resolved output (for dedupe/plus-icon).
    pub(in crate::view) sources_index: FxHashSet<super::SourceLineKey>,
}

/// Mode-specific state for streamed (giant-file) conflict resolution.
///
/// Uses lazy paged access and span-based projections instead of
/// eagerly materializing all rows.
#[derive(Clone, Debug, Default)]
pub(in crate::view) struct StreamedConflictState {
    pub(in crate::view) three_way_visible_projection: super::ThreeWayVisibleProjection,
    pub(in crate::view) split_row_index: super::ConflictSplitRowIndex,
    pub(in crate::view) two_way_split_projection: super::TwoWaySplitProjection,
}

#[derive(Clone, Debug)]
pub(in crate::view) enum ConflictModeState {
    Streamed(StreamedConflictState),
}

impl Default for ConflictModeState {
    fn default() -> Self {
        Self::Streamed(StreamedConflictState::default())
    }
}

/// section 30 split: a drag selection of aligned rows within one conflict block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct ConflictRowSelection {
    /// Visible conflict block the selection is anchored in.
    pub(in crate::view) conflict_ix: usize,
    /// Aligned row where the drag started.
    pub(in crate::view) anchor_row: usize,
    /// Aligned row under the cursor (clamped to the block).
    pub(in crate::view) head_row: usize,
    /// True while the drag is in progress.
    pub(in crate::view) selecting: bool,
}

impl ConflictRowSelection {
    /// Inclusive aligned-row range covered, normalized so start <= end.
    pub(in crate::view) fn row_range(&self) -> std::ops::RangeInclusive<usize> {
        let lo = self.anchor_row.min(self.head_row);
        let hi = self.anchor_row.max(self.head_row);
        lo..=hi
    }
}

/// KDiff3 manual diff help: lines marked in one source column, pending the
/// Ctrl+Y that pins them against the other columns' marks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct AlignmentLineSelection {
    /// Line where the mark started.
    pub(in crate::view) anchor: usize,
    /// Line last marked.
    pub(in crate::view) head: usize,
}

impl AlignmentLineSelection {
    /// Half-open line range covered, normalized so start <= end.
    pub(in crate::view) fn line_range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head) + 1
    }

    pub(in crate::view) fn contains(self, line: usize) -> bool {
        self.line_range().contains(&line)
    }
}

#[derive(Clone, Debug)]
pub(in crate::view) struct ConflictResolverUiState {
    pub(in crate::view) repo_id: Option<RepoId>,
    pub(in crate::view) path: Option<std::path::PathBuf>,
    pub(in crate::view) shared_path: Option<worktree_state::msg::RepoPath>,
    pub(in crate::view) loaded_file: Option<worktree_state::model::ConflictFile>,
    pub(in crate::view) conflict_syntax_language: Option<crate::view::rows::DiffSyntaxLanguage>,
    pub(in crate::view) source_hash: Option<u64>,
    /// The editable output contains preserved worktree text whose conflict
    /// spans could not be mapped safely onto the stage projection.
    pub(in crate::view) output_is_protected: bool,
    /// The user asked for the stage projection anyway, via *Reset conflict
    /// markers*, so protection stays off for this conflict however the
    /// worktree payload reads.
    ///
    /// Without this the reset lasts until the next store round-trip: the resync
    /// recomputes protection from the same unchanged worktree payload and turns
    /// it straight back on, which is what made the button look like it did
    /// nothing. A re-bootstrap drops the waiver, so it lasts exactly as long as
    /// the conflict and the file content it was granted for.
    pub(in crate::view) output_protection_waived: bool,
    /// Marker-backed geometry used for reset and source-region rendering.
    pub(in crate::view) current: Option<std::sync::Arc<str>>,
    pub(in crate::view) marker_segments: Vec<super::ConflictSegment>,
    /// section 30 collapsed context mode: fold unchanged runs in the source columns.
    pub(in crate::view) collapse_context: bool,
    /// Per-fold reveal state for collapsed context mode, keyed by fold id.
    pub(in crate::view) context_fold_reveals: FxHashMap<usize, super::ConflictFoldReveal>,
    /// section 30 collapsed context mode for the resolved output pane: fold
    /// projection in output line space. `None` ⇒ pass-through (one row per
    /// line). Rebuilt lazily after its inputs change.
    pub(in crate::view) resolved_output_visible: Option<super::ThreeWayVisibleProjection>,
    pub(in crate::view) resolved_output_visible_dirty: bool,
    /// Per-fold reveal state for resolved-output folds (output-line fold ids).
    pub(in crate::view) output_context_fold_reveals: FxHashMap<usize, super::ConflictFoldReveal>,
    /// Mapping from visible block index to `ConflictSession` region index.
    pub(in crate::view) conflict_region_indices: Vec<usize>,
    /// Mapping from visible marker block index to its semantic merge-plan
    /// block. Empty for marker-only/fallback sessions.
    pub(in crate::view) display_plan_block_indices: Vec<usize>,
    /// Whether each raw session region includes a diff3 base marker. This is
    /// kept separate from display blocks, whose base may be populated from
    /// the shared ancestor for picking.
    pub(in crate::view) conflict_region_marker_has_base: Vec<bool>,
    /// Actionable conflict block currently selected in the displayed marker
    /// projection. Semantic targets without a displayed block leave this unset.
    pub(in crate::view) active_conflict: Option<usize>,
    /// Ordered semantic resolver navigation targets.
    pub(in crate::view) nav_targets: Vec<super::ConflictNavTarget>,
    /// Aligned source rows retained for every original session region before
    /// manual/automatic resolutions are materialized into plain display text.
    pub(in crate::view) original_region_aligned_ranges: Vec<Option<Range<usize>>>,
    pub(in crate::view) hovered_conflict: Option<(usize, ThreeWayColumn)>,
    /// section 30 split: in-progress or completed drag selection of aligned rows
    /// inside one conflict block, used to split that block at the selection
    /// boundary. Cleared whenever the conflict source rebuilds.
    pub(in crate::view) row_selection: Option<ConflictRowSelection>,
    /// KDiff3 manual diff help: lines marked per source column, independent of
    /// the block-scoped `row_selection` because a manual alignment exists
    /// precisely to pin lines the automatic alignment put in different blocks.
    pub(in crate::view) alignment_selection: ThreeWaySides<Option<AlignmentLineSelection>>,
    /// Streamed conflict state for the single conflict rendering/runtime path.
    pub(in crate::view) mode_state: ConflictModeState,
    pub(in crate::view) view_mode: ConflictResolverViewMode,
    /// Backing text for each three-way source side.
    pub(in crate::view) three_way_text: ThreeWaySides<SharedString>,
    /// Per-side line start offsets into `three_way_text`, materialized lazily.
    pub(in crate::view) three_way_line_starts: ThreeWaySides<DeferredLineStarts>,
    pub(in crate::view) three_way_len: usize,
    /// section 30 aligned row space: maps visual rows to per-side lines. Identity
    /// (row == line) when alignment is unavailable.
    pub(in crate::view) three_way_aligned: super::ThreeWayAlignedMap,
    /// kdiff3-style minimap column bands, in visible-row space. Empty when no
    /// alignment is available, which hides the column.
    pub(in crate::view) minimap_bands: Arc<[worktree_core::merge::MinimapRowKind]>,
    /// Exact merge-plan row ranges for the currently visible marker blocks.
    ///
    /// `None` is the legacy/current-only fallback where ranges must be
    /// estimated from marker text.
    pub(in crate::view) merge_plan_aligned_conflict_ranges: Option<Vec<Range<usize>>>,
    /// Whether the three-way visible projection/ranges have been built at
    /// least once for the current conflict source.
    pub(in crate::view) three_way_visible_state_ready: bool,
    /// Per-side conflict ranges for O(log n) binary-search lookups and
    /// conflict-to-visible mapping. The ours ranges remain the anchor space for
    /// legacy three-way visible projections.
    pub(in crate::view) three_way_conflict_ranges: ThreeWaySides<Vec<Range<usize>>>,
    /// Visible-row indices used to measure horizontal width for each three-way input column.
    pub(in crate::view) three_way_horizontal_measure_rows: [usize; 3],
    pub(in crate::view) conflict_has_base: Vec<bool>,
    /// Current choice for each conflict block, cached to avoid rebuilding it
    /// from `marker_segments` on every render.
    pub(in crate::view) conflict_choices: Vec<super::ConflictChoice>,
    /// Ignore-whitespace visual row kinds by two-way split source row.
    pub(in crate::view) two_way_split_visual_kind_cache:
        FxHashMap<usize, worktree_core::file_diff::FileDiffRowKind>,
    /// Visible-row indices used to measure horizontal width for the two-way split inputs.
    pub(in crate::view) two_way_horizontal_measure_rows: [usize; 2],
    pub(in crate::view) three_way_word_highlights: ThreeWaySides<super::WordHighlights>,
    /// Aligned two-way (ours↔theirs) word highlights keyed by aligned row,
    /// precomputed once per rebuild and shared by both diff columns.
    pub(in crate::view) two_way_aligned_word_highlights:
        FxHashMap<usize, super::TwoWayWordHighlightPair>,
    /// Bounded on-demand word highlights for giant block-local two-way rows.
    pub(in crate::view) two_way_split_word_highlight_cache: super::ConflictSplitWordHighlightCache,
    pub(in crate::view) nav_anchor: Option<super::ConflictNavAnchor>,
    pub(in crate::view) hide_resolved: bool,
    /// True when any conflict side contains non-UTF8 binary data.
    pub(in crate::view) is_binary_conflict: bool,
    /// Byte sizes of the three conflict sides (for binary UI display).
    pub(in crate::view) binary_side_sizes: [Option<usize>; 3],
    /// The resolver strategy for the current conflict (set during sync).
    pub(in crate::view) strategy: Option<worktree_core::conflict_session::ConflictResolverStrategy>,
    /// The conflict kind for the current file (set during sync).
    pub(in crate::view) conflict_kind: Option<worktree_core::domain::FileConflictKind>,
    /// Last autosolve trace summary shown in resolver UI.
    pub(in crate::view) last_autosolve_summary: Option<SharedString>,
    /// KDiff3-style report captured when this resolver file opened.
    ///
    /// This stays fixed while the user makes manual picks, so the toast
    /// describes the open-time state rather than a later live state.
    pub(in crate::view) open_summary_counts: Option<super::ConflictSummaryCounts>,
    /// True once the one-shot open-summary toast (total / auto-solved /
    /// unsolved, kdiff3-style) has been pushed for this resolver open.
    pub(in crate::view) open_summary_announced: bool,
    /// Tracks the last-seen `conflict_rev` from state so we can detect
    /// state-side session changes (e.g. hide-resolved, bulk picks, autosolve)
    /// that don't change the underlying file content.
    pub(in crate::view) conflict_rev: u64,
    /// Sequence token for debounced resolved-output outline recompute tasks.
    pub(in crate::view) resolver_pending_recompute_seq: u64,
    /// Resolved-output outline metadata (provenance, conflict markers, source index).
    pub(in crate::view) resolved_outline: ResolvedOutlineData,
    /// Cached per-line gutter render state for resolved-output preview rows.
    pub(in crate::view) resolved_outline_gutter_rows: Vec<super::ResolvedOutputGutterRow>,
    /// Cached rendered markdown previews for the merge-input sides.
    pub(in crate::view) markdown_preview: ConflictResolverMarkdownPreviewState,
    /// Cached image previews for the merge-input sides.
    pub(in crate::view) image_preview: ConflictResolverImagePreviewState,
    /// Preview mode for the merge-input pane (Text vs rendered Preview).
    pub(in crate::view) resolver_preview_mode: ConflictResolverPreviewMode,
}

impl Default for ConflictResolverUiState {
    fn default() -> Self {
        Self {
            repo_id: None,
            path: None,
            shared_path: None,
            loaded_file: None,
            collapse_context: false,
            context_fold_reveals: FxHashMap::default(),
            conflict_syntax_language: None,
            source_hash: None,
            output_is_protected: false,
            output_protection_waived: false,
            current: None,
            marker_segments: Vec::new(),
            conflict_region_indices: Vec::new(),
            display_plan_block_indices: Vec::new(),
            conflict_region_marker_has_base: Vec::new(),
            active_conflict: None,
            nav_targets: Vec::new(),
            original_region_aligned_ranges: Vec::new(),
            hovered_conflict: None,
            row_selection: None,
            alignment_selection: ThreeWaySides::default(),
            mode_state: ConflictModeState::default(),
            view_mode: ConflictResolverViewMode::TwoWayDiff,
            three_way_text: ThreeWaySides::default(),
            three_way_line_starts: ThreeWaySides::default(),
            three_way_len: 0,
            three_way_aligned: super::ThreeWayAlignedMap::default(),
            minimap_bands: Arc::from([]),
            merge_plan_aligned_conflict_ranges: None,
            three_way_visible_state_ready: false,
            three_way_conflict_ranges: ThreeWaySides::default(),
            three_way_horizontal_measure_rows: [0; 3],
            conflict_has_base: Vec::new(),
            conflict_choices: Vec::new(),
            two_way_split_visual_kind_cache: FxHashMap::default(),
            two_way_horizontal_measure_rows: [0; 2],
            three_way_word_highlights: ThreeWaySides::default(),
            two_way_aligned_word_highlights: FxHashMap::default(),
            two_way_split_word_highlight_cache: Default::default(),
            nav_anchor: None,
            hide_resolved: false,
            is_binary_conflict: false,
            binary_side_sizes: [None; 3],
            strategy: None,
            conflict_kind: None,
            last_autosolve_summary: None,
            open_summary_counts: None,
            open_summary_announced: false,
            conflict_rev: 0,
            resolver_pending_recompute_seq: 0,
            resolved_outline: ResolvedOutlineData::default(),
            resolved_outline_gutter_rows: Vec::new(),
            resolved_output_visible: None,
            resolved_output_visible_dirty: true,
            output_context_fold_reveals: FxHashMap::default(),
            markdown_preview: ConflictResolverMarkdownPreviewState::default(),
            image_preview: ConflictResolverImagePreviewState::default(),
            resolver_preview_mode: ConflictResolverPreviewMode::default(),
        }
    }
}

fn indexed_line_text<'a>(text: &'a str, line_starts: &[usize], line_ix: usize) -> Option<&'a str> {
    if text.is_empty() {
        return None;
    }
    let text_len = text.len();
    let start = line_starts.get(line_ix).copied().unwrap_or(text_len);
    if start >= text_len {
        return None;
    }
    let mut end = line_starts
        .get(line_ix.saturating_add(1))
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    if end > start && text.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n') {
        end = end.saturating_sub(1);
    }
    Some(text.get(start..end).unwrap_or(""))
}

fn append_conflict_row_without_whitespace(
    row: &worktree_core::file_diff::FileDiffRow,
    old_out: &mut String,
    new_out: &mut String,
) {
    use worktree_core::file_diff::FileDiffRowKind as RK;

    match row.kind {
        RK::Context => {}
        RK::Remove => {
            if let Some(text) = row.old.as_ref() {
                old_out.extend(text.as_ref().chars().filter(|ch| !ch.is_whitespace()));
            }
        }
        RK::Add => {
            if let Some(text) = row.new.as_ref() {
                new_out.extend(text.as_ref().chars().filter(|ch| !ch.is_whitespace()));
            }
        }
        RK::Modify => {
            if let Some(text) = row.old.as_ref() {
                old_out.extend(text.as_ref().chars().filter(|ch| !ch.is_whitespace()));
            }
            if let Some(text) = row.new.as_ref() {
                new_out.extend(text.as_ref().chars().filter(|ch| !ch.is_whitespace()));
            }
        }
    }
}

impl ConflictResolverUiState {
    pub(in crate::view) fn matches_target(&self, repo_id: RepoId, path: &std::path::Path) -> bool {
        self.repo_id == Some(repo_id) && self.path.as_deref() == Some(path)
    }

    pub(in crate::view) fn dispatch_path(&self) -> Option<worktree_state::msg::RepoPath> {
        self.shared_path.clone()
    }

    pub(in crate::view) fn selected_nav_target_index(&self) -> Option<usize> {
        let anchor = self.nav_anchor?;
        self.nav_targets
            .iter()
            .position(|target| target.id == anchor.id)
    }

    pub(in crate::view) fn nav_target_index_for_aligned_row(&self, row: usize) -> Option<usize> {
        self.nav_targets.iter().position(|target| {
            target
                .aligned_rows
                .as_ref()
                .is_some_and(|range| range.contains(&row))
        })
    }

    pub(in crate::view) fn selected_nav_target_contains_aligned_row(&self, row: usize) -> bool {
        self.selected_nav_target_index()
            .and_then(|index| self.nav_targets.get(index))
            .and_then(|target| target.aligned_rows.as_ref())
            .is_some_and(|range| range.contains(&row))
    }

    /// Whether the conflict a row belongs to is the selected one.
    ///
    /// `conflict_ix` is `None` for a row in no conflict at all, and
    /// `active_conflict` is `None` whenever nothing is selected — for instance
    /// right after a pick moves the anchor onto a block that renders no marker.
    /// Comparing the two options directly made those two `None`s match, which
    /// painted the active-conflict marker on every row *outside* a conflict.
    pub(in crate::view) fn conflict_is_active(&self, conflict_ix: Option<usize>) -> bool {
        conflict_ix.is_some() && conflict_ix == self.active_conflict
    }

    fn nav_target_matches_display(
        &self,
        target: &super::ConflictNavTarget,
        display_conflict_index: usize,
    ) -> bool {
        target.display_conflict_index == Some(display_conflict_index)
            || target.region_index.is_some_and(|region_index| {
                self.conflict_region_indices
                    .get(display_conflict_index)
                    .copied()
                    == Some(region_index)
            })
            || matches!(
                target.id,
                super::ConflictNavTargetId::DisplayBlock(index)
                    if index == display_conflict_index
            )
    }

    pub(in crate::view) fn select_nav_target(&mut self, target_index: usize) -> bool {
        let Some(target) = self.nav_targets.get(target_index) else {
            return false;
        };
        self.nav_anchor = Some(target.anchor());
        self.active_conflict = target.display_conflict_index;
        true
    }

    pub(in crate::view) fn select_display_conflict(
        &mut self,
        display_conflict_index: usize,
    ) -> bool {
        let Some(target_index) = self
            .nav_targets
            .iter()
            .position(|target| self.nav_target_matches_display(target, display_conflict_index))
        else {
            return false;
        };
        self.nav_anchor = Some(self.nav_targets[target_index].anchor());
        self.active_conflict = Some(display_conflict_index);
        true
    }

    pub(in crate::view) fn reconcile_nav_targets(
        &mut self,
        targets: Vec<super::ConflictNavTarget>,
    ) {
        let previous_targets = std::mem::replace(&mut self.nav_targets, targets);
        let previous_active = self.active_conflict;
        let selected = super::reconcile_conflict_nav_target_index(
            self.nav_anchor,
            &previous_targets,
            &self.nav_targets,
        );
        let Some(selected) = selected else {
            self.nav_anchor = None;
            self.active_conflict = None;
            return;
        };
        let target = &self.nav_targets[selected];
        self.nav_anchor = Some(target.anchor());
        self.active_conflict = previous_active
            .filter(|display| self.nav_target_matches_display(target, *display))
            .or(target.display_conflict_index);
    }

    pub(in crate::view) fn output_line_for_nav_target_provenance(
        &self,
        target: &super::ConflictNavTarget,
    ) -> Option<usize> {
        let aligned_rows = target.aligned_rows.as_ref()?;
        self.resolved_outline.meta.iter().find_map(|meta| {
            let side = match (self.view_mode, meta.source) {
                (ConflictResolverViewMode::ThreeWay, super::ResolvedLineSource::A) => {
                    ThreeWayColumn::Base
                }
                (ConflictResolverViewMode::ThreeWay, super::ResolvedLineSource::B) => {
                    ThreeWayColumn::Ours
                }
                (ConflictResolverViewMode::ThreeWay, super::ResolvedLineSource::C) => {
                    ThreeWayColumn::Theirs
                }
                (ConflictResolverViewMode::TwoWayDiff, super::ResolvedLineSource::A) => {
                    ThreeWayColumn::Ours
                }
                (ConflictResolverViewMode::TwoWayDiff, super::ResolvedLineSource::B) => {
                    ThreeWayColumn::Theirs
                }
                (ConflictResolverViewMode::TwoWayDiff, super::ResolvedLineSource::C)
                | (_, super::ResolvedLineSource::Manual) => return None,
            };
            let source_line = usize::try_from(meta.input_line?).ok()?.checked_sub(1)?;
            let aligned_row = self.three_way_row_for_side_line(side, source_line);
            (aligned_rows.contains(&aligned_row)
                || (aligned_rows.is_empty() && aligned_rows.start == aligned_row))
                .then_some(meta.output_line as usize)
        })
    }

    /// Map a visible input-column row to the resolved-output line it produced.
    ///
    /// Quick search walks the *input* columns, so a hit arrives as a visible
    /// row rather than a nav target and
    /// [`Self::output_line_for_nav_target_provenance`] cannot be reused. This
    /// reads the same provenance table, keyed on the row's own side lines: an
    /// output line belongs to this row when it names one of them as its origin.
    /// `meta` is ordered by output line, so the first hit is the earliest line
    /// the row contributed.
    ///
    /// Returns `None` when the outline carries no provenance — large outputs
    /// skip building it (`should_skip_resolved_outline_provenance`), exactly as
    /// conflict navigation's output reveal already degrades there.
    pub(in crate::view) fn output_line_for_visible_row(&self, visible_ix: usize) -> Option<usize> {
        // Indexed by `ResolvedLineSource` A/B/C, which names different columns
        // per view mode — see `output_line_for_nav_target_provenance`.
        let source_lines: [Option<usize>; 3] = match self.view_mode {
            ConflictResolverViewMode::ThreeWay => {
                let aligned_row = self.three_way_aligned_row_for_visible_row(visible_ix)?;
                [
                    self.three_way_aligned
                        .side_line_for_row(ThreeWayColumn::Base.side_index(), aligned_row),
                    self.three_way_aligned
                        .side_line_for_row(ThreeWayColumn::Ours.side_index(), aligned_row),
                    self.three_way_aligned
                        .side_line_for_row(ThreeWayColumn::Theirs.side_index(), aligned_row),
                ]
            }
            ConflictResolverViewMode::TwoWayDiff => {
                // Split rows carry 1-based line numbers: `old` is Ours (source
                // A here), `new` is Theirs (source B). There is no C.
                //
                // Deliberately *not* dispatched on `two_way_uses_aligned_rows`
                // the way `two_way_visible_len` and friends are: the two-way
                // scan that produces these indices resolves them through
                // `two_way_split_projection` unconditionally, so this has to
                // read the same space to agree with it. Both are wrong together
                // whenever the aligned rows are in use — a pre-existing gap
                // between what two-way search indexes and what it renders, which
                // needs fixing on both sides at once.
                let row = self.two_way_split_visible_row(visible_ix)?.row;
                [
                    row.old_line.and_then(|line| (line as usize).checked_sub(1)),
                    row.new_line.and_then(|line| (line as usize).checked_sub(1)),
                    None,
                ]
            }
        };

        if source_lines.iter().all(Option::is_none) {
            return None;
        }

        self.resolved_outline.meta.iter().find_map(|meta| {
            let side_ix = match meta.source {
                super::ResolvedLineSource::A => 0,
                super::ResolvedLineSource::B => 1,
                super::ResolvedLineSource::C => 2,
                super::ResolvedLineSource::Manual => return None,
            };
            let source_line = usize::try_from(meta.input_line?).ok()?.checked_sub(1)?;
            (source_lines[side_ix] == Some(source_line)).then_some(meta.output_line as usize)
        })
    }

    /// The aligned merge-plan row a visible three-way row stands for.
    ///
    /// Fold summary rows answer with the first row they cover, so a match
    /// inside a fold still reveals the right neighbourhood of the output.
    fn three_way_aligned_row_for_visible_row(&self, visible_ix: usize) -> Option<usize> {
        match self.three_way_visible_item(visible_ix)? {
            super::ThreeWayVisibleItem::Line(row) => Some(row),
            super::ThreeWayVisibleItem::CollapsedContext {
                source_line_start, ..
            } => Some(source_line_start),
            super::ThreeWayVisibleItem::CollapsedBlock(conflict_ix) => {
                let range =
                    self.three_way_conflict_ranges[ThreeWayColumn::Ours].get(conflict_ix)?;
                Some(self.three_way_row_for_side_line(ThreeWayColumn::Ours, range.start))
            }
        }
    }

    pub(in crate::view) fn cached_loaded_file_for_target(
        &self,
        repo_id: RepoId,
        path: &std::path::Path,
    ) -> Option<&worktree_state::model::ConflictFile> {
        self.matches_target(repo_id, path)
            .then_some(self.loaded_file.as_ref())
            .flatten()
    }

    // ----- Mode accessors -----

    /// Return the rendering mode enum (for tracing / external APIs that expect it).
    #[cfg(test)]
    pub(in crate::view) fn rendering_mode(&self) -> super::ConflictRenderingMode {
        super::ConflictRenderingMode::StreamedLargeFile
    }

    /// Access the streamed conflict state.
    #[cfg(test)]
    #[track_caller]
    pub(in crate::view) fn streamed(&self) -> &StreamedConflictState {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s,
        }
    }

    /// Mutably access the streamed conflict state.
    #[cfg(test)]
    #[track_caller]
    pub(in crate::view) fn streamed_mut(&mut self) -> &mut StreamedConflictState {
        match &mut self.mode_state {
            ConflictModeState::Streamed(s) => s,
        }
    }

    pub(in crate::view) fn split_row_index(&self) -> Option<&super::ConflictSplitRowIndex> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => Some(&s.split_row_index),
        }
    }

    pub(in crate::view) fn two_way_split_projection(
        &self,
    ) -> Option<&super::TwoWaySplitProjection> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => Some(&s.two_way_split_projection),
        }
    }

    pub(in crate::view) fn three_way_visible_projection(
        &self,
    ) -> &super::ThreeWayVisibleProjection {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => &s.three_way_visible_projection,
        }
    }

    #[track_caller]
    #[allow(unused_variables)]
    pub(in crate::view) fn debug_assert_rendering_mode_invariants(&self) {}

    pub(in crate::view) fn three_way_line_count(&self, side: ThreeWayColumn) -> usize {
        self.three_way_line_starts[side].line_count()
    }

    pub(in crate::view) fn three_way_line_starts_ref(&self, side: ThreeWayColumn) -> &[usize] {
        self.three_way_line_starts[side].starts(self.three_way_text[side].as_ref())
    }

    pub(in crate::view) fn three_way_shared_line_starts(
        &self,
        side: ThreeWayColumn,
    ) -> Arc<[usize]> {
        self.three_way_line_starts[side].shared_starts(self.three_way_text[side].as_ref())
    }

    pub(in crate::view) fn three_way_line_text(
        &self,
        side: ThreeWayColumn,
        line_ix: usize,
    ) -> Option<&str> {
        indexed_line_text(
            &self.three_way_text[side],
            self.three_way_line_starts_ref(side),
            line_ix,
        )
    }

    /// The side line rendered at an aligned visual row (section 30 aligned row
    /// space), or `None` for padding rows.
    pub(in crate::view) fn three_way_side_line_for_row(
        &self,
        side: ThreeWayColumn,
        row: usize,
    ) -> Option<usize> {
        self.three_way_aligned
            .side_line_for_row(side.side_index(), row)
    }

    /// Text of the side line rendered at an aligned visual row; `None` for
    /// padding rows and rows past the side's end.
    pub(in crate::view) fn three_way_row_text(
        &self,
        side: ThreeWayColumn,
        row: usize,
    ) -> Option<&str> {
        let line_ix = self.three_way_side_line_for_row(side, row)?;
        self.three_way_line_text(side, line_ix)
    }

    /// section 30 R11 (kdiff3 change colours): whether the side columns can tint
    /// rows by their own change vs base — needs a real base and a
    /// non-identity alignment (both-added and unaligned files keep the
    /// marker-region tint).
    pub(in crate::view) fn three_way_per_side_change_rows(&self) -> bool {
        !self.three_way_aligned.is_identity() && !self.three_way_text.base.is_empty()
    }

    /// section 30 R11: whether `column`'s line at aligned `row` differs from the
    /// base line paired at the same row. A line on one side of a padding row
    /// counts as a change; the base column itself is never "changed".
    pub(in crate::view) fn three_way_row_differs_from_base(
        &self,
        column: ThreeWayColumn,
        row: usize,
    ) -> bool {
        if matches!(column, ThreeWayColumn::Base) {
            return false;
        }
        self.three_way_row_text(column, row) != self.three_way_row_text(ThreeWayColumn::Base, row)
    }

    /// The aligned visual row at which a side line renders.
    pub(in crate::view) fn three_way_row_for_side_line(
        &self,
        side: ThreeWayColumn,
        line: usize,
    ) -> usize {
        self.three_way_aligned
            .row_for_side_line(side.side_index(), line)
    }

    /// section 30 split: whether row selection / split is available for the current
    /// conflict. Requires a real aligned row space (so rows map consistently
    /// across columns) and a full-text resolver strategy on non-binary data.
    pub(in crate::view) fn conflict_row_selection_enabled(&self) -> bool {
        !self.three_way_aligned.is_identity()
            && !self.is_binary_conflict
            && self.strategy
                == Some(worktree_core::conflict_session::ConflictResolverStrategy::FullTextResolver)
    }

    /// section 30 split: whether aligned `row` is inside the current row selection
    /// (highlighted in every source column since rows are shared).
    pub(in crate::view) fn conflict_row_is_selected(&self, row: usize) -> bool {
        self.row_selection
            .is_some_and(|sel| sel.row_range().contains(&row))
    }

    /// KDiff3 manual diff help: whether the resolver can pin alignments at all.
    ///
    /// Shares the row-selection preconditions: a real aligned row space and a
    /// full-text resolver on non-binary data.
    pub(in crate::view) fn manual_alignment_enabled(&self) -> bool {
        self.conflict_row_selection_enabled()
    }

    /// Mark `line` in `column` for a manual alignment.
    ///
    /// `extend` grows the column's existing mark from its anchor; otherwise it
    /// starts a fresh single-line mark. Each column is marked independently —
    /// that is the whole point, since a manual alignment pins lines the
    /// automatic alignment placed on different rows.
    pub(in crate::view) fn set_alignment_selection(
        &mut self,
        column: ThreeWayColumn,
        line: usize,
        extend: bool,
    ) {
        let anchor = match self.alignment_selection[column] {
            Some(selection) if extend => selection.anchor,
            _ => line,
        };
        self.alignment_selection[column] = Some(AlignmentLineSelection { anchor, head: line });
    }

    /// Drop every pending alignment mark. Returns whether anything was marked.
    pub(in crate::view) fn clear_alignment_selections(&mut self) -> bool {
        let had_any = self.has_alignment_selection();
        self.alignment_selection = ThreeWaySides::default();
        had_any
    }

    pub(in crate::view) fn has_alignment_selection(&self) -> bool {
        ThreeWayColumn::ALL
            .iter()
            .any(|column| self.alignment_selection[*column].is_some())
    }

    /// Whether `line` of `column` carries a pending alignment mark.
    pub(in crate::view) fn alignment_line_is_selected(
        &self,
        column: ThreeWayColumn,
        line: usize,
    ) -> bool {
        self.alignment_selection[column].is_some_and(|selection| selection.contains(line))
    }

    /// Build the entry a Ctrl+Y would pin from the current marks.
    ///
    /// A column the user left unmarked still needs a position, or the entry
    /// could not be ordered against the others. The aligned row where the
    /// marked columns begin gives it one, and it pins an empty range there —
    /// "the marked lines align against nothing on this side", which is how a
    /// one-sided block gets forced.
    ///
    /// Returns `None` when nothing is marked or the plan cannot be pinned.
    pub(in crate::view) fn manual_alignment_from_selections(
        &self,
        has_base: bool,
    ) -> Option<worktree_core::merge::ManualAlignment> {
        if !self.manual_alignment_enabled() || !self.has_alignment_selection() {
            return None;
        }
        let anchor_row = ThreeWayColumn::ALL
            .iter()
            .filter_map(|column| {
                let selection = self.alignment_selection[*column]?;
                Some(
                    self.three_way_aligned
                        .aligned_range_for_side_range(column.side_index(), selection.line_range())
                        .start,
                )
            })
            .min()?;
        let range_for = |column: ThreeWayColumn| match self.alignment_selection[column] {
            Some(selection) => selection.line_range(),
            None => {
                let line = self
                    .three_way_aligned
                    .side_line_lower_bound(column.side_index(), anchor_row);
                line..line
            }
        };
        let base = if has_base {
            range_for(ThreeWayColumn::Base)
        } else {
            0..0
        };
        Some(worktree_core::merge::ManualAlignment::new(
            base,
            range_for(ThreeWayColumn::Ours),
            range_for(ThreeWayColumn::Theirs),
        ))
    }

    /// section 30 split: the shared aligned-row range of conflict block `conflict_ix`
    /// (all source columns share it after `rebuild_three_way_visible_state`).
    pub(in crate::view) fn three_way_block_aligned_range(
        &self,
        conflict_ix: usize,
    ) -> Option<std::ops::Range<usize>> {
        self.three_way_conflict_ranges[ThreeWayColumn::Ours]
            .get(conflict_ix)
            .cloned()
    }

    /// section 30 split: clamp aligned `row` into conflict block `conflict_ix`.
    pub(in crate::view) fn clamp_row_to_conflict_block(
        &self,
        conflict_ix: usize,
        row: usize,
    ) -> usize {
        match self.three_way_block_aligned_range(conflict_ix) {
            Some(range) if !range.is_empty() => row.clamp(range.start, range.end - 1),
            _ => row,
        }
    }

    /// section 30 split: convert a normalized row selection inside a conflict block
    /// into block-local per-side split boundaries and the target region index.
    /// Returns `None` when selection/split is unavailable, the selection is
    /// degenerate (covers the whole block or nothing), or the block maps to a
    /// non-unique session region.
    pub(in crate::view) fn split_boundaries_for_selection(
        &self,
    ) -> Option<(
        usize,
        worktree_core::conflict_session::ConflictRegionSplitBoundaries,
    )> {
        let selection = self.row_selection?;
        if !self.conflict_row_selection_enabled() {
            return None;
        }
        // Custom/manual resolutions can replace a raw region with display
        // text, shifting every later display-side range away from the
        // immutable source alignment. Only split while display blocks retain
        // a one-to-one, in-order mapping to raw session regions.
        if self.conflict_region_indices.len() != self.conflict_region_marker_has_base.len()
            || self
                .conflict_region_indices
                .iter()
                .enumerate()
                .any(|(block_index, &region_index)| block_index != region_index)
        {
            return None;
        }
        let conflict_ix = selection.conflict_ix;
        let block = self.three_way_block_aligned_range(conflict_ix)?;
        if block.is_empty() {
            return None;
        }
        let row_range = selection.row_range();
        let sel_start = (*row_range.start()).max(block.start);
        let sel_end_inclusive = (*row_range.end()).min(block.end - 1);
        if sel_start > sel_end_inclusive {
            return None;
        }
        // A selection covering the whole block cannot split it.
        if sel_start <= block.start && sel_end_inclusive >= block.end - 1 {
            return None;
        }

        let marker_block = self
            .marker_segments
            .iter()
            .filter_map(|segment| match segment {
                super::ConflictSegment::Block(block) => Some(block),
                super::ConflictSegment::Text(_) => None,
            })
            .nth(conflict_ix)?;
        let line_count = |text: &str| {
            if text.is_empty() {
                0
            } else {
                text.as_bytes()
                    .iter()
                    .filter(|&&byte| byte == b'\n')
                    .count()
                    + usize::from(!text.ends_with('\n'))
            }
        };

        let side_bounds = |side: usize| -> Option<([usize; 2], usize)> {
            // The aligned map is built from the actual staged sides. Its position
            // at the block's first aligned row remains correct when clean context
            // before this block exists on only one side; marker Text segments do
            // not retain enough information to reconstruct that position.
            let base = self
                .three_way_aligned
                .side_line_lower_bound(side, block.start);
            let b0 = self
                .three_way_aligned
                .side_line_lower_bound(side, sel_start)
                .saturating_sub(base);
            let b1 = self
                .three_way_aligned
                .side_line_lower_bound(side, sel_end_inclusive + 1)
                .saturating_sub(base);
            let len = match side {
                0 => line_count(marker_block.base.as_deref().unwrap_or_default()),
                1 => line_count(&marker_block.ours),
                2 => line_count(&marker_block.theirs),
                _ => return None,
            };
            let b0 = b0.min(len);
            let b1 = b1.clamp(b0, len);
            Some(([b0, b1], len))
        };

        let region_index = self.conflict_region_indices.get(conflict_ix).copied()?;
        if self
            .conflict_region_indices
            .iter()
            .filter(|&&index| index == region_index)
            .take(2)
            .count()
            != 1
        {
            return None;
        }
        let has_base = self
            .conflict_region_marker_has_base
            .get(region_index)
            .copied()?;
        let (ours, ours_len) = side_bounds(ThreeWayColumn::Ours.side_index())?;
        let (theirs, theirs_len) = side_bounds(ThreeWayColumn::Theirs.side_index())?;
        let base = if has_base {
            Some(side_bounds(ThreeWayColumn::Base.side_index())?)
        } else {
            None
        };

        // Alignment can contain padding/base-only rows that have no content in
        // the serialized marker block. Do not advertise a split unless the
        // selection owns at least one serialized line and leaves at least one
        // serialized line outside the new region.
        let selected_has_content = ours[0] < ours[1]
            || theirs[0] < theirs[1]
            || base.is_some_and(|(bounds, _)| bounds[0] < bounds[1]);
        let has_content_outside = ours[0] > 0
            || ours[1] < ours_len
            || theirs[0] > 0
            || theirs[1] < theirs_len
            || base.is_some_and(|(bounds, len)| bounds[0] > 0 || bounds[1] < len);
        if !selected_has_content || !has_content_outside {
            return None;
        }

        let boundaries = worktree_core::conflict_session::ConflictRegionSplitBoundaries {
            ours,
            theirs,
            base: base.map(|(bounds, _)| bounds),
        };
        Some((region_index, boundaries))
    }

    /// Whether two consecutive displayed marker blocks can be joined without
    /// crossing malformed marker-looking context. This mirrors the core
    /// surgery guard so an enabled menu item does not silently no-op.
    pub(in crate::view) fn conflict_blocks_have_joinable_context(
        &self,
        first_conflict_ix: usize,
        second_conflict_ix: usize,
    ) -> bool {
        if first_conflict_ix.checked_add(1) != Some(second_conflict_ix) {
            return false;
        }
        let markerish = |text: &str| {
            text.lines().any(|line| {
                line.starts_with("<<<<<<<")
                    || line.starts_with("=======")
                    || line.starts_with(">>>>>>>")
                    || line.starts_with("|||||||")
            })
        };
        let mut conflict_ix = 0usize;
        let mut between = false;
        for segment in &self.marker_segments {
            match segment {
                super::ConflictSegment::Block(_) => {
                    if conflict_ix == second_conflict_ix {
                        return between;
                    }
                    between = conflict_ix == first_conflict_ix;
                    conflict_ix = conflict_ix.saturating_add(1);
                }
                super::ConflictSegment::Text(text) if between => {
                    if markerish(text.as_str()) {
                        return false;
                    }
                }
                super::ConflictSegment::Text(_) => {}
            }
        }
        false
    }

    pub(in crate::view) fn three_way_has_line(&self, side: ThreeWayColumn, line_ix: usize) -> bool {
        self.three_way_line_text(side, line_ix).is_some()
    }

    /// Return source-pane text for a conflict pick choice at a global line index.
    ///
    /// This reads from the indexed merge-input texts directly so callers do not
    /// depend on eager diff rows or streamed page generation.
    pub(in crate::view) fn source_line_text_for_choice(
        &self,
        choice: super::ConflictChoice,
        line_ix: usize,
    ) -> Option<&str> {
        match choice {
            super::ConflictChoice::Base if self.view_mode == ConflictResolverViewMode::ThreeWay => {
                self.three_way_line_text(ThreeWayColumn::Base, line_ix)
            }
            super::ConflictChoice::Ours => self.three_way_line_text(ThreeWayColumn::Ours, line_ix),
            super::ConflictChoice::Theirs => {
                self.three_way_line_text(ThreeWayColumn::Theirs, line_ix)
            }
            super::ConflictChoice::Base | super::ConflictChoice::Both => None,
            _ => None,
        }
    }

    /// Look up the visible item at `visible_ix`, dispatching between the eager
    /// map (small files) and the span-based projection (giant files).
    pub(in crate::view) fn three_way_visible_item(
        &self,
        visible_ix: usize,
    ) -> Option<super::ThreeWayVisibleItem> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s.three_way_visible_projection.get(visible_ix),
        }
    }

    /// Number of visible rows in the three-way view.
    pub(in crate::view) fn three_way_visible_len(&self) -> usize {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s.three_way_visible_projection.len(),
        }
    }

    /// Look up the conflict index for a given line on a given side.
    /// Uses binary search on per-side ranges in giant mode, O(1) array lookup otherwise.
    pub(in crate::view) fn conflict_index_for_side_line(
        &self,
        side: ThreeWayColumn,
        line_ix: usize,
    ) -> Option<usize> {
        let ranges = &self.three_way_conflict_ranges[side];
        super::conflict_index_for_line(ranges, line_ix)
    }

    /// Find the visible index for a conflict range, using the projection in giant mode.
    pub(in crate::view) fn visible_index_for_conflict(&self, range_ix: usize) -> Option<usize> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => {
                s.three_way_visible_projection.visible_index_for_conflict(
                    &self.three_way_conflict_ranges[ThreeWayColumn::Ours],
                    range_ix,
                )
            }
        }
    }

    /// Find the visible row for an aligned merge-plan row. Context hidden by a
    /// fold maps to the fold summary row.
    pub(in crate::view) fn visible_index_for_aligned_row(&self, row: usize) -> Option<usize> {
        self.three_way_visible_projection()
            .visible_index_for_source_line(row)
    }

    // ----- Two-way split dispatch (giant vs eager) -----

    /// section 30 aligned row space: whether the two-way view renders the shared
    /// aligned whole-file rows (full mode) instead of the block-local
    /// `ConflictSplitRowIndex` rows (giant files / sides not loaded).
    pub(in crate::view) fn two_way_uses_aligned_rows(&self) -> bool {
        !self.three_way_aligned.is_identity()
    }

    /// Number of visible rows in the two-way view (aligned or block-local).
    pub(in crate::view) fn two_way_visible_len(&self) -> usize {
        if self.two_way_uses_aligned_rows() {
            self.three_way_visible_len()
        } else {
            self.two_way_split_visible_len()
        }
    }

    /// Number of visible rows in the two-way split view.
    pub(in crate::view) fn two_way_split_visible_len(&self) -> usize {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s.two_way_split_projection.visible_len(),
        }
    }

    /// Retrieve a materialized split row for the given visible index,
    /// dispatching between the paged index (giant) and the eager `diff_rows`
    /// array (small).
    pub(in crate::view) fn two_way_split_visible_row(
        &self,
        visible_ix: usize,
    ) -> Option<super::TwoWaySplitVisibleRow> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => {
                let (source_row_ix, conflict_ix) = s.two_way_split_projection.get(visible_ix)?;
                let row = s
                    .split_row_index
                    .row_at(&self.marker_segments, source_row_ix)?;
                Some(super::TwoWaySplitVisibleRow {
                    source_row_ix,
                    row,
                    conflict_ix,
                })
            }
        }
    }

    /// Retrieve a split row by source row index (not visible index).
    pub(in crate::view) fn two_way_split_row_by_source(
        &self,
        row_ix: usize,
    ) -> Option<worktree_core::file_diff::FileDiffRow> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => {
                s.split_row_index.row_at(&self.marker_segments, row_ix)
            }
        }
    }

    pub(in crate::view) fn two_way_split_visual_kind_at(
        &mut self,
        row_ix: usize,
        row: &worktree_core::file_diff::FileDiffRow,
        whitespace_mode: DiffWhitespaceMode,
    ) -> worktree_core::file_diff::FileDiffRowKind {
        use worktree_core::file_diff::FileDiffRowKind as RK;

        if whitespace_mode == DiffWhitespaceMode::Show || matches!(row.kind, RK::Context) {
            return row.kind;
        }

        if let Some(kind) = self.two_way_split_visual_kind_cache.get(&row_ix).copied() {
            return kind;
        }

        self.cache_two_way_split_visual_kind_run(row_ix);
        self.two_way_split_visual_kind_cache
            .get(&row_ix)
            .copied()
            .unwrap_or(row.kind)
    }

    fn cache_two_way_split_visual_kind_run(&mut self, row_ix: usize) {
        use worktree_core::file_diff::FileDiffRowKind as RK;

        let mut start = row_ix;
        while start > 0 {
            let Some(prev) = self.two_way_split_row_by_source(start - 1) else {
                break;
            };
            if matches!(prev.kind, RK::Context) {
                break;
            }
            start -= 1;
        }

        let mut old_stripped = String::new();
        let mut new_stripped = String::new();
        let mut end = start;
        while let Some(next) = self.two_way_split_row_by_source(end) {
            if matches!(next.kind, RK::Context) {
                break;
            }
            append_conflict_row_without_whitespace(&next, &mut old_stripped, &mut new_stripped);
            end += 1;
        }

        if start == end {
            return;
        }

        if old_stripped == new_stripped {
            for ix in start..end {
                self.two_way_split_visual_kind_cache.insert(ix, RK::Context);
            }
            return;
        }

        for ix in start..end {
            if let Some(row) = self.two_way_split_row_by_source(ix) {
                self.two_way_split_visual_kind_cache.insert(ix, row.kind);
            }
        }
    }

    /// Find the first visible index for a conflict in two-way split view.
    pub(in crate::view) fn two_way_split_visible_ix_for_conflict(
        &self,
        conflict_ix: usize,
    ) -> Option<usize> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s
                .two_way_split_projection
                .visible_index_for_conflict(conflict_ix),
        }
    }

    /// Map a two-way split visible index back to its conflict index.
    #[cfg(test)]
    pub(in crate::view) fn two_way_split_conflict_ix_for_visible(
        &self,
        visible_ix: usize,
    ) -> Option<usize> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => s
                .two_way_split_projection
                .get(visible_ix)
                .and_then(|(_, ci)| ci),
        }
    }

    /// Build unresolved conflict navigation entries for two-way split view.
    #[cfg(test)]
    pub(in crate::view) fn two_way_split_nav_entries(&self) -> Vec<usize> {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => {
                super::unresolved_conflict_indices(&self.marker_segments)
                    .into_iter()
                    .filter_map(|ci| s.two_way_split_projection.visible_index_for_conflict(ci))
                    .collect()
            }
        }
    }

    // ----- Unified two-way dispatch (aligned vs block-local) -----

    /// Build unresolved conflict navigation entries for the current two-way
    /// conflict diff view.
    #[cfg(test)]
    pub(in crate::view) fn two_way_nav_entries(&self) -> Vec<usize> {
        if self.two_way_uses_aligned_rows() {
            return super::unresolved_conflict_indices(&self.marker_segments)
                .into_iter()
                .filter_map(|ci| self.visible_index_for_conflict(ci))
                .collect();
        }
        self.two_way_split_nav_entries()
    }

    /// Map a two-way visible index to its conflict index.
    #[cfg(test)]
    pub(in crate::view) fn two_way_conflict_ix_for_visible(
        &self,
        visible_ix: usize,
    ) -> Option<usize> {
        if self.two_way_uses_aligned_rows() {
            return match self.three_way_visible_item(visible_ix)? {
                super::ThreeWayVisibleItem::CollapsedBlock(ri) => Some(ri),
                super::ThreeWayVisibleItem::Line(row) => {
                    // Conflict ranges are aligned-row ranges shared by all
                    // columns, so any side works for the lookup.
                    self.conflict_index_for_side_line(ThreeWayColumn::Ours, row)
                }
                super::ThreeWayVisibleItem::CollapsedContext { .. } => None,
            };
        }
        self.two_way_split_conflict_ix_for_visible(visible_ix)
    }

    /// Find the first visible index for a conflict in the current two-way diff
    /// view.
    pub(in crate::view) fn two_way_visible_ix_for_conflict(
        &self,
        conflict_ix: usize,
    ) -> Option<usize> {
        if self.two_way_uses_aligned_rows() {
            return self.visible_index_for_conflict(conflict_ix);
        }
        self.two_way_split_visible_ix_for_conflict(conflict_ix)
    }

    /// Return (diff_row_count, inline_row_count) for trace recording.
    pub(in crate::view) fn two_way_row_counts(&self) -> (usize, usize) {
        match &self.mode_state {
            ConflictModeState::Streamed(s) => (s.split_row_index.total_rows(), 0),
        }
    }

    pub(in crate::view) fn three_way_horizontal_measure_row(&self, side: ThreeWayColumn) -> usize {
        match side {
            ThreeWayColumn::Base => self.three_way_horizontal_measure_rows[0],
            ThreeWayColumn::Ours => self.three_way_horizontal_measure_rows[1],
            ThreeWayColumn::Theirs => self.three_way_horizontal_measure_rows[2],
        }
    }

    pub(in crate::view) fn two_way_horizontal_measure_row(
        &self,
        side: super::ConflictPickSide,
    ) -> usize {
        // Aligned two-way rows share the three-way row space, so the
        // three-way per-column measurements apply directly.
        if self.two_way_uses_aligned_rows() {
            return match side {
                super::ConflictPickSide::Ours => {
                    self.three_way_horizontal_measure_row(ThreeWayColumn::Ours)
                }
                super::ConflictPickSide::Theirs => {
                    self.three_way_horizontal_measure_row(ThreeWayColumn::Theirs)
                }
            };
        }
        match side {
            super::ConflictPickSide::Ours => self.two_way_horizontal_measure_rows[0],
            super::ConflictPickSide::Theirs => self.two_way_horizontal_measure_rows[1],
        }
    }

    fn refresh_three_way_horizontal_measure_rows(&mut self) {
        self.three_way_horizontal_measure_rows = self.compute_three_way_horizontal_measure_rows();
    }

    fn refresh_two_way_horizontal_measure_rows(&mut self) {
        self.two_way_horizontal_measure_rows = self.compute_two_way_horizontal_measure_rows();
    }

    fn compute_three_way_horizontal_measure_rows(&self) -> [usize; 3] {
        let has_hidden_resolved_blocks = self.hide_resolved
            && self.marker_segments.iter().any(|segment| {
                matches!(
                    segment,
                    super::ConflictSegment::Block(block) if block.resolved
                )
            });
        if self.collapse_context || has_hidden_resolved_blocks {
            // This helper already returns indices in the compact visible
            // projection. Mapping them as side-line indices would apply the
            // alignment a second time and can select an unrelated row. Context
            // folding likewise changes visible indices even without a hidden
            // resolved block.
            return self.compute_three_way_horizontal_measure_rows_from_visible_projection();
        }

        let rows = self.compute_three_way_horizontal_measure_side_lines();
        // The scan yields indices in each stage's own text; width measurement
        // wants their corresponding aligned rows.
        [
            self.three_way_row_for_side_line(ThreeWayColumn::Base, rows[0]),
            self.three_way_row_for_side_line(ThreeWayColumn::Ours, rows[1]),
            self.three_way_row_for_side_line(ThreeWayColumn::Theirs, rows[2]),
        ]
    }

    fn compute_three_way_horizontal_measure_side_lines(&self) -> [usize; 3] {
        // Marker text is the merge result, not any one index stage. Clean
        // changes outside conflict markers can therefore add or remove lines
        // on only one side. Walking marker segments and advancing all three
        // counters together produces invalid stage coordinates (and can make
        // a column measure a short row instead of its widest row). Scan each
        // actual stage text independently instead.
        [
            super::scan_text_line_stats(self.three_way_text.base.as_ref())
                .widest_line()
                .map_or(0, |(line_ix, _)| line_ix),
            super::scan_text_line_stats(self.three_way_text.ours.as_ref())
                .widest_line()
                .map_or(0, |(line_ix, _)| line_ix),
            super::scan_text_line_stats(self.three_way_text.theirs.as_ref())
                .widest_line()
                .map_or(0, |(line_ix, _)| line_ix),
        ]
    }

    fn compute_three_way_horizontal_measure_rows_from_visible_projection(&self) -> [usize; 3] {
        let mut best_rows = [0usize; 3];
        let mut best_lens = [0usize; 3];

        for span in self.three_way_visible_projection().spans() {
            let super::ThreeWayVisibleSpan::Lines {
                visible_start,
                source_line_start,
                len,
            } = *span
            else {
                continue;
            };

            for offset in 0..len {
                let visible_ix = visible_start + offset;
                let line_ix = source_line_start + offset;

                for (slot, side) in [
                    ThreeWayColumn::Base,
                    ThreeWayColumn::Ours,
                    ThreeWayColumn::Theirs,
                ]
                .into_iter()
                .enumerate()
                {
                    let width = self.three_way_row_text(side, line_ix).map_or(0, str::len);
                    if width > best_lens[slot] {
                        best_lens[slot] = width;
                        best_rows[slot] = visible_ix;
                    }
                }
            }
        }

        best_rows
    }

    fn compute_two_way_horizontal_measure_rows(&self) -> [usize; 2] {
        let Some(split_row_index) = self.split_row_index() else {
            return [0; 2];
        };
        let Some(projection) = self.two_way_split_projection() else {
            return [0; 2];
        };

        let [ours_source_row, theirs_source_row] = split_row_index
            .widest_source_rows_by_text_len(&self.marker_segments, self.hide_resolved);

        [
            ours_source_row
                .and_then(|row_ix| projection.source_to_visible(row_ix))
                .unwrap_or(0),
            theirs_source_row
                .and_then(|row_ix| projection.source_to_visible(row_ix))
                .unwrap_or(0),
        ]
    }

    /// Pre-computed word highlights for a source row in the two-way split view.
    /// Return an already-computed giant-mode word highlight pair.
    pub(in crate::view) fn two_way_split_word_highlight(
        &self,
        row_ix: usize,
    ) -> Option<Arc<super::TwoWayWordHighlightPair>> {
        self.two_way_split_word_highlight_cache.get(row_ix)
    }

    /// Cache a giant-mode word highlight pair so the other split column and
    /// later frames reuse the same word diff.
    pub(in crate::view) fn cache_two_way_split_word_highlight(
        &mut self,
        row_ix: usize,
        highlights: super::TwoWayWordHighlightPair,
    ) -> Arc<super::TwoWayWordHighlightPair> {
        self.two_way_split_word_highlight_cache
            .insert(row_ix, highlights)
    }

    pub(in crate::view) fn two_way_split_word_highlight_for_row(
        &mut self,
        row_ix: usize,
        row: &worktree_core::file_diff::FileDiffRow,
    ) -> Option<Arc<super::TwoWayWordHighlightPair>> {
        self.two_way_split_word_highlight(row_ix).or_else(|| {
            super::compute_word_highlights_for_row(row)
                .map(|highlights| self.cache_two_way_split_word_highlight(row_ix, highlights))
        })
    }

    /// Rebuild three-way visible state (conflict maps + visible map/projection)
    /// from current marker segments and line counts.
    pub(in crate::view) fn rebuild_three_way_visible_state(&mut self) {
        let maps = super::build_three_way_conflict_maps_without_line_maps(
            &self.marker_segments,
            self.three_way_line_count(ThreeWayColumn::Base),
            self.three_way_line_count(ThreeWayColumn::Ours),
            self.three_way_line_count(ThreeWayColumn::Theirs),
        );
        let block_count = maps.conflict_ranges[1].len();
        let exact_plan_ranges = self
            .merge_plan_aligned_conflict_ranges
            .as_ref()
            .filter(|ranges| {
                ranges.len() == block_count
                    && ranges
                        .iter()
                        .all(|range| range.start <= range.end && range.end <= self.three_way_len)
                    && ranges.windows(2).all(|pair| pair[0].end <= pair[1].start)
            })
            .cloned();
        let aligned_ranges = exact_plan_ranges.unwrap_or_else(|| {
            // Legacy/current-only fallback: project marker-text offsets back
            // through the side alignment. Marker text is output space rather
            // than source space, so this is necessarily an estimate.
            super::project_conflict_ranges_to_aligned_rows(
                &self.marker_segments,
                &self.three_way_aligned,
                [
                    self.three_way_line_count(ThreeWayColumn::Base),
                    self.three_way_line_count(ThreeWayColumn::Ours),
                    self.three_way_line_count(ThreeWayColumn::Theirs),
                ],
            )
        });
        let three_way_visible_projection = super::build_three_way_visible_projection_with_options(
            self.three_way_len,
            &aligned_ranges,
            &maps.conflict_resolved,
            super::ThreeWayVisibleOptions {
                hide_resolved: self.hide_resolved,
                collapse_context: self.collapse_context,
                context_fold_reveals: Some(&self.context_fold_reveals),
            },
        );
        self.apply_three_way_conflict_maps(maps);
        // All columns share the aligned conflict ranges.
        self.three_way_conflict_ranges = ThreeWaySides {
            base: aligned_ranges.clone(),
            ours: aligned_ranges.clone(),
            theirs: aligned_ranges,
        };
        match &mut self.mode_state {
            ConflictModeState::Streamed(s) => {
                s.three_way_visible_projection = three_way_visible_projection;
            }
        }
        self.three_way_visible_state_ready = true;
        self.refresh_three_way_horizontal_measure_rows();
        self.rebuild_minimap_bands();
    }

    /// Recompute the minimap column's bands for the current projection.
    ///
    /// Runs from `rebuild_three_way_visible_state`, after the aligned conflict
    /// ranges are in place, so a pick recolors the band it settles.
    pub(in crate::view) fn rebuild_minimap_bands(&mut self) {
        let projection = match &self.mode_state {
            ConflictModeState::Streamed(s) => &s.three_way_visible_projection,
        };
        let resolved = super::resolved_conflict_flags_from_segments(&self.marker_segments);
        self.minimap_bands = super::build_minimap_bands(
            &self.three_way_aligned,
            projection,
            &self.three_way_conflict_ranges[ThreeWayColumn::Ours],
            &resolved,
            super::CONFLICT_BOTTOM_OVERSCROLL_ROWS,
        )
        .into();
    }

    /// Whether the minimap column has anything to show.
    pub(in crate::view) fn has_minimap(&self) -> bool {
        !self.minimap_bands.is_empty()
    }

    /// Rebuild two-way visible state from current marker segments.
    /// Rebuilds the streamed split row index and projection.
    pub(in crate::view) fn rebuild_two_way_visible_state(&mut self) {
        self.two_way_split_visual_kind_cache.clear();
        self.two_way_split_word_highlight_cache.clear();
        let ConflictModeState::Streamed(s) = &mut self.mode_state;
        s.split_row_index = super::ConflictSplitRowIndex::new(
            &self.marker_segments,
            super::BLOCK_LOCAL_DIFF_CONTEXT_LINES,
        );
        self.rebuild_two_way_visible_projections();
    }

    /// Rebuild streamed two-way visible projections from the current split-row index.
    pub(in crate::view) fn rebuild_two_way_visible_projections(&mut self) {
        match &mut self.mode_state {
            ConflictModeState::Streamed(s) => {
                s.two_way_split_projection = super::TwoWaySplitProjection::new(
                    &s.split_row_index,
                    &self.marker_segments,
                    self.hide_resolved,
                );
            }
        }
        self.debug_assert_rendering_mode_invariants();
        self.refresh_two_way_horizontal_measure_rows();
    }

    /// Apply three-way conflict maps to state fields.
    pub(in crate::view) fn apply_three_way_conflict_maps(
        &mut self,
        maps: super::ThreeWayConflictMaps,
    ) {
        let [base_ranges, ours_ranges, theirs_ranges] = maps.conflict_ranges;
        self.three_way_conflict_ranges = ThreeWaySides {
            base: base_ranges,
            ours: ours_ranges,
            theirs: theirs_ranges,
        };
        self.conflict_has_base = maps.conflict_has_base;
        self.refresh_conflict_choices_from_segments();
    }

    pub(in crate::view) fn refresh_conflict_has_base_from_segments(&mut self) {
        self.conflict_has_base = self
            .marker_segments
            .iter()
            .filter_map(|segment| match segment {
                super::ConflictSegment::Block(block) => Some(block.base.is_some()),
                super::ConflictSegment::Text(_) => None,
            })
            .collect();
        self.refresh_conflict_choices_from_segments();
    }

    pub(in crate::view) fn refresh_conflict_choices_from_segments(&mut self) {
        self.conflict_choices = self
            .marker_segments
            .iter()
            .filter_map(|segment| match segment {
                super::ConflictSegment::Block(block) => Some(block.choice),
                super::ConflictSegment::Text(_) => None,
            })
            .collect();
    }

    pub(in crate::view) fn has_three_way_visible_state_ready(&self) -> bool {
        self.three_way_visible_state_ready
    }
}
