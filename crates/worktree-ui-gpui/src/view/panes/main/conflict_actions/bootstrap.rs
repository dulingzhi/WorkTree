//! Resolver bootstrap and session sync: the mergetool trace context and
//! source fingerprints, `sync_conflict_resolver` and its lightweight
//! resync, load-mode requests, view-mode/hide-resolved/collapse toggles,
//! visible-map rebuilds and the session/count query accessors.

use super::*;

/// Render the current semantic plan decisions into the marker/text projection
/// consumed by the resolver UI. This differs from `marker_projection`, which
/// intentionally remains the immutable structural baseline used to detect
/// protected worktree edits.
fn conflict_session_plan_projection(
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

/// Pre-computed side stats for mergetool trace events.  Computing these once
/// avoids redundant full-text newline counts across the ~10 trace events per
/// bootstrap.  When tracing is disabled, stats are left at `Default` so the
/// newline counting never runs.
struct MergetoolTraceContext {
    path: PathBuf,
    base: MergetoolTraceSideStats,
    ours: MergetoolTraceSideStats,
    theirs: MergetoolTraceSideStats,
    current: MergetoolTraceSideStats,
}

impl MergetoolTraceContext {
    fn new(
        path: PathBuf,
        base_text: &str,
        ours_text: &str,
        theirs_text: &str,
        current_text: Option<&str>,
    ) -> Self {
        if !mergetool_trace::is_enabled() {
            return Self {
                path,
                base: MergetoolTraceSideStats::default(),
                ours: MergetoolTraceSideStats::default(),
                theirs: MergetoolTraceSideStats::default(),
                current: MergetoolTraceSideStats::default(),
            };
        }
        Self {
            path,
            base: MergetoolTraceSideStats::from_text(Some(base_text)),
            ours: MergetoolTraceSideStats::from_text(Some(ours_text)),
            theirs: MergetoolTraceSideStats::from_text(Some(theirs_text)),
            current: MergetoolTraceSideStats::from_text(current_text),
        }
    }

    fn event(&self, stage: MergetoolTraceStage, started: Instant) -> MergetoolTraceEvent {
        MergetoolTraceEvent::new(stage, Some(self.path.clone()), started.elapsed())
            .with_base(self.base)
            .with_ours(self.ours)
            .with_theirs(self.theirs)
            .with_current(self.current)
    }

    fn bootstrap_event(
        &self,
        stage: MergetoolTraceStage,
        started: Instant,
        decisions: MergetoolBootstrapTraceDecisions,
    ) -> MergetoolTraceEvent {
        decisions.apply_to_event(self.event(stage, started))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MergetoolBootstrapTraceDecisions {
    rendering_mode: Option<MergetoolTraceRenderingMode>,
    whole_block_diff_ran: Option<bool>,
    full_output_generated: Option<bool>,
    full_syntax_parse_requested: Option<bool>,
}

impl MergetoolBootstrapTraceDecisions {
    fn apply_to_event(self, event: MergetoolTraceEvent) -> MergetoolTraceEvent {
        event
            .with_rendering_mode(self.rendering_mode)
            .with_whole_block_diff_ran(self.whole_block_diff_ran)
            .with_full_output_generated(self.full_output_generated)
            .with_full_syntax_parse_requested(self.full_syntax_parse_requested)
    }
}

fn trace_rendering_mode(
    mode: conflict_resolver::ConflictRenderingMode,
) -> MergetoolTraceRenderingMode {
    match mode {
        conflict_resolver::ConflictRenderingMode::EagerSmallFile => {
            MergetoolTraceRenderingMode::EagerSmallFile
        }
        conflict_resolver::ConflictRenderingMode::StreamedLargeFile => {
            MergetoolTraceRenderingMode::StreamedLargeFile
        }
    }
}

const CONFLICT_SOURCE_FINGERPRINT_SAMPLE_COUNT: usize = 8;
const CONFLICT_SOURCE_FINGERPRINT_WINDOW_BYTES: usize = 256;

// This is a lightweight UI cache key, not a cryptographic hash. Domain labels
// keep the text/bytes/none cases distinct without opaque numeric seeds.
fn sampled_content_fingerprint(bytes: &[u8], domain: &str) -> u64 {
    use std::hash::Hasher;

    let mut hasher = FxHasher::default();
    hasher.write_usize(domain.len());
    hasher.write(domain.as_bytes());
    hasher.write_usize(bytes.len());
    if bytes.is_empty() {
        return hasher.finish();
    }

    let window_len = CONFLICT_SOURCE_FINGERPRINT_WINDOW_BYTES.min(bytes.len());
    let sample_count = if bytes.len() <= window_len {
        1
    } else {
        CONFLICT_SOURCE_FINGERPRINT_SAMPLE_COUNT
    };
    let max_start = bytes.len().saturating_sub(window_len);
    let denominator = sample_count.saturating_sub(1).max(1);
    for sample_ix in 0..sample_count {
        let start = if sample_count == 1 {
            0
        } else {
            sample_ix.saturating_mul(max_start) / denominator
        };
        hasher.write_usize(start);
        hasher.write(&bytes[start..start.saturating_add(window_len)]);
    }
    hasher.finish()
}

fn shared_text_fingerprint(text: &Option<std::sync::Arc<str>>) -> u64 {
    let Some(text) = text.as_ref() else {
        return sampled_content_fingerprint(&[], "conflict-source:text:none");
    };
    sampled_content_fingerprint(text.as_bytes(), "conflict-source:text")
}

fn shared_bytes_fingerprint(bytes: &Option<std::sync::Arc<[u8]>>) -> u64 {
    let Some(bytes) = bytes.as_ref() else {
        return sampled_content_fingerprint(&[], "conflict-source:bytes:none");
    };
    sampled_content_fingerprint(bytes.as_ref(), "conflict-source:bytes")
}

fn conflict_file_source_fingerprint(file: &worktree_state::model::ConflictFile) -> u64 {
    let side_fingerprint = |text: &Option<std::sync::Arc<str>>,
                            bytes: &Option<std::sync::Arc<[u8]>>,
                            side_domain: &str| {
        let value = if text.is_some() {
            shared_text_fingerprint(text)
        } else {
            shared_bytes_fingerprint(bytes)
        };
        sampled_content_fingerprint(&value.to_le_bytes(), side_domain)
    };

    let mut acc = sampled_content_fingerprint(&[], "conflict-source:file");
    for (side_domain, text, bytes) in [
        ("conflict-source:side:base", &file.base, &file.base_bytes),
        ("conflict-source:side:ours", &file.ours, &file.ours_bytes),
        (
            "conflict-source:side:theirs",
            &file.theirs,
            &file.theirs_bytes,
        ),
        (
            "conflict-source:side:current",
            &file.current,
            &file.current_bytes,
        ),
    ] {
        acc = acc.rotate_left(13) ^ side_fingerprint(text, bytes, side_domain);
    }
    acc
}

/// The conflicted working-tree entry [`MainPaneView::sync_conflict_resolver`]
/// binds to: the repo, the file path and the conflict kind, resolved by the
/// gate phase before any resolver state is touched.
struct ConflictSyncTarget {
    repo_id: RepoId,
    path: PathBuf,
    conflict_kind: Option<worktree_core::domain::FileConflictKind>,
}

/// Argument pack for the bootstrap phases, bundling the values the gate and
/// strategy selection produce so the phase functions stay small in arity.
/// Every field is read at the same program point the pre-split monolith read
/// it; the pack only carries the values across the call boundary.
struct ConflictBootstrapInput {
    repo_id: RepoId,
    path: PathBuf,
    shared_path: worktree_state::msg::RepoPath,
    conflict_syntax_language: Option<rows::DiffSyntaxLanguage>,
    source_hash: u64,
    conflict_strategy: Option<worktree_core::conflict_session::ConflictResolverStrategy>,
    conflict_kind: Option<worktree_core::domain::FileConflictKind>,
    needs_full_side_payloads: bool,
    conflict_rev: u64,
}

/// The computed half of a text-conflict bootstrap: the values that move into
/// [`ConflictResolverUiState`] at install time, plus [`ConflictBootstrapPost`]
/// for everything the finish phase still needs afterwards (trace context,
/// counters, the resolved output and the fresh-open guards). Field values are
/// produced by exactly the statements the pre-split monolith ran, in order.
struct ConflictBootstrap {
    post: ConflictBootstrapPost,
    marker_snapshot: Option<Arc<str>>,
    output_is_protected: bool,
    marker_segments: Vec<conflict_resolver::ConflictSegment>,
    collapse_context: bool,
    conflict_region_indices: Vec<usize>,
    display_plan_block_indices: Vec<usize>,
    conflict_region_marker_has_base: Vec<bool>,
    active_conflict: Option<usize>,
    nav_targets: Vec<conflict_resolver::ConflictNavTarget>,
    original_region_aligned_ranges: Vec<Option<std::ops::Range<usize>>>,
    mode_state: ConflictModeState,
    view_mode: ConflictResolverViewMode,
    three_way_text: ThreeWaySides<SharedString>,
    three_way_line_starts: ThreeWaySides<DeferredLineStarts>,
    three_way_len: usize,
    three_way_aligned: conflict_resolver::ThreeWayAlignedMap,
    merge_plan_aligned_conflict_ranges: Option<Vec<std::ops::Range<usize>>>,
    three_way_word_highlights: ThreeWaySides<conflict_resolver::WordHighlights>,
    two_way_aligned_word_highlights: FxHashMap<usize, conflict_resolver::TwoWayWordHighlightPair>,
    nav_anchor: Option<conflict_resolver::ConflictNavAnchor>,
    hide_resolved: bool,
    last_autosolve_summary: Option<SharedString>,
    open_summary_counts: Option<conflict_resolver::ConflictSummaryCounts>,
    open_summary_announced: bool,
    resolver_preview_mode: ConflictResolverPreviewMode,
}

/// See [`ConflictBootstrap`].
struct ConflictBootstrapPost {
    trace_ctx: MergetoolTraceContext,
    trace_decisions: MergetoolBootstrapTraceDecisions,
    bootstrap_started: Instant,
    conflict_block_count: usize,
    diff_row_count: usize,
    inline_row_count: usize,
    resolved_line_count: Option<usize>,
    resolved_output_text: Option<conflict_resolver::ResolvedOutputText>,
    streamed_output_projection: Option<conflict_resolver::ResolvedOutputProjection>,
    is_same_conflict: bool,
    needs_full_side_texts: bool,
    full_text_plan_upgrade_expected: bool,
    three_way_needs_background: ThreeWaySides<bool>,
}

impl MainPaneView {
    fn clear_conflict_resolver_state(&mut self) {
        self.conflict_resolver = ConflictResolverUiState::default();
        self.conflict_resolved_output_saved_snapshot = None;
        self.conflict_resolved_output_modified = false;
        self.conflict_resolved_output_block_map =
            conflict_resolver::ResolvedOutputBlockMap::default();
        self.conflict_resolver_invalidate_resolved_outline();
    }

    /// Gate phase: resolve the conflicted working-tree entry the resolver
    /// binds to. `None` covers every "not a conflict target" case the
    /// pre-split monolith answered by clearing the resolver state; the caller
    /// performs that clear.
    fn conflict_resolver_sync_target(&self) -> Option<ConflictSyncTarget> {
        let repo_id = self.active_repo_id()?;
        let repo = self.state.repos.iter().find(|r| r.id == repo_id)?;
        let Some(DiffTarget::WorkingTree { path, area }) = repo.diff_state.diff_target.as_ref()
        else {
            return None;
        };
        if *area != DiffArea::Unstaged {
            return None;
        }
        let conflict_entry = repo
            .status_entry_for_path(DiffArea::Unstaged, path.as_path())
            .filter(|entry| entry.kind == worktree_core::domain::FileStatusKind::Conflicted)?;
        Some(ConflictSyncTarget {
            repo_id,
            path: path.clone(),
            conflict_kind: conflict_entry.conflict,
        })
    }

    /// CurrentOnly first load: reset the resolver and the output input, then
    /// dispatch the load request. The caller returns immediately afterwards.
    fn begin_conflict_file_current_only_load(
        &mut self,
        repo_id: RepoId,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        self.clear_conflict_resolver_state();
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_text("", cx);
        });
        self.store.dispatch(Msg::LoadConflictFile {
            repo_id,
            path,
            mode: worktree_state::model::ConflictFileLoadMode::CurrentOnly,
        });
    }

    /// Binary conflicts carry no marker geometry: populate the minimal state
    /// (side sizes for the UI) and request the Full payload upgrade when the
    /// CurrentOnly first load omitted the side bytes.
    fn apply_binary_conflict_bootstrap(
        &mut self,
        input: ConflictBootstrapInput,
        file: &worktree_state::model::ConflictFile,
    ) {
        let binary_side_sizes = [
            file.base_bytes.as_ref().map(|b| b.len()),
            file.ours_bytes.as_ref().map(|b| b.len()),
            file.theirs_bytes.as_ref().map(|b| b.len()),
        ];
        self.conflict_resolver = ConflictResolverUiState {
            repo_id: Some(input.repo_id),
            path: Some(input.path),
            shared_path: Some(input.shared_path),
            loaded_file: Some(file.clone()),
            conflict_syntax_language: input.conflict_syntax_language,
            source_hash: Some(input.source_hash),
            is_binary_conflict: true,
            binary_side_sizes,
            strategy: input.conflict_strategy,
            conflict_kind: input.conflict_kind,
            last_autosolve_summary: None,
            open_summary_counts: None,
            conflict_rev: input.conflict_rev,
            ..ConflictResolverUiState::default()
        };
        self.conflict_resolver_invalidate_resolved_outline();
        if input.needs_full_side_payloads {
            let _ = self
                .request_conflict_file_load_mode(worktree_state::model::ConflictFileLoadMode::Full);
        }
    }

    /// Compute phase of the resolver bootstrap: parse the marker
    /// projection, build the aligned row space and the word highlights,
    /// derive the resolved output and collect the preserved per-open UI
    /// state. Everything the install/finish phases need comes back in
    /// [`ConflictBootstrap`]; the only `self` writes are the segment and
    /// syntax caches, at the same statements and order as the pre-split
    /// monolith. Returns `None` only on the unreachable repo-vanished
    //  path (the gate found the repo; nothing mutates `self.state.repos`).
    fn compute_conflict_bootstrap(
        &mut self,
        input: &ConflictBootstrapInput,
        file: &worktree_state::model::ConflictFile,
    ) -> Option<ConflictBootstrap> {
        let bootstrap_started = Instant::now();
        // The gate already established this repo exists; nothing before
        // this lookup mutates `self.state.repos`, so the `?` never fires and
        // only keeps the borrowck-honest shape.
        let repo = self.state.repos.iter().find(|r| r.id == input.repo_id)?;
        let session = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .filter(|session| session.path == input.path);
        let current_text = session
            .and_then(|session| match session.current.as_ref() {
                Some(worktree_core::conflict_session::ConflictPayload::Text(text)) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .or_else(|| file.current.clone());
        let structural_marker_snapshot = session
            .and_then(|session| session.marker_projection.clone())
            .or_else(|| current_text.clone());
        let plan_projection = session.and_then(conflict_session_plan_projection);
        let marker_snapshot = plan_projection
            .as_ref()
            .map(|(text, _)| Arc::clone(text))
            .or_else(|| structural_marker_snapshot.clone());
        let output_is_protected = worktree_output_requires_protection(
            current_text.as_deref(),
            structural_marker_snapshot.as_deref(),
            file.base.as_deref(),
            file.ours.as_deref(),
            file.theirs.as_deref(),
        );
        let current_text_ref = current_text.as_deref();
        let base_text = file.base.as_deref().unwrap_or("");
        let ours_text = file.ours.as_deref().unwrap_or("");
        let theirs_text = file.theirs.as_deref().unwrap_or("");
        let trace_ctx = MergetoolTraceContext::new(
            input.path.clone(),
            base_text,
            ours_text,
            theirs_text,
            current_text_ref,
        );
        let is_same_conflict = self.conflict_resolver.repo_id == Some(input.repo_id)
            && self.conflict_resolver.path.as_ref() == Some(&input.path);
        // True when the fast CurrentOnly first paint is showing: no side text
        // has been loaded yet (a Full load provides at least one stage for
        // real conflicts).
        let needs_full_side_texts =
            file.base.is_none() && file.ours.is_none() && file.theirs.is_none();
        const FULL_LOAD_UPGRADE_MAX_CURRENT_LINES: usize = 100_000;
        let full_text_plan_upgrade_expected = needs_full_side_texts
            && matches!(
                input.conflict_strategy,
                Some(worktree_core::conflict_session::ConflictResolverStrategy::FullTextResolver)
            )
            && current_text
                .as_deref()
                .is_some_and(|text| count_newlines(text) < FULL_LOAD_UPGRADE_MAX_CURRENT_LINES);
        let three_way_base_len = if base_text.is_empty() {
            0
        } else {
            count_newlines(base_text).saturating_add(1)
        };
        let three_way_ours_len = if ours_text.is_empty() {
            0
        } else {
            count_newlines(ours_text).saturating_add(1)
        };
        let three_way_theirs_len = if theirs_text.is_empty() {
            0
        } else {
            count_newlines(theirs_text).saturating_add(1)
        };
        let three_way_side_max_len = three_way_base_len
            .max(three_way_ours_len)
            .max(three_way_theirs_len);

        let marker_parse_started = Instant::now();
        let mut marker_segments = if let Some(cur) = marker_snapshot.clone() {
            conflict_resolver::parse_conflict_markers_shared_nonempty(cur)
        } else {
            Vec::new()
        };
        let conflict_region_marker_has_base = marker_segments
            .iter()
            .filter_map(|segment| match segment {
                conflict_resolver::ConflictSegment::Block(block) => Some(block.base.is_some()),
                conflict_resolver::ConflictSegment::Text(_) => None,
            })
            .collect();
        let rendering_mode = conflict_resolver::select_conflict_rendering_mode(
            &marker_segments,
            three_way_side_max_len,
        );
        // section 30 aligned row space: compute the kdiff3-style alignment once per
        // bootstrap (side texts are immutable for the session). Files without
        // a base version (e.g. both-added conflicts) align ours↔theirs
        // directly with empty base ranges, so the two-way view gets the same
        // whole-file row space. Fall back to the identity map when side texts
        // are unavailable (CurrentOnly load) or when the alignment diff would
        // be impractical (large files whose sides no longer share most of
        // their lines — whole-file conflicts make Myers effectively
        // quadratic).
        let three_way_aligned =
            if let Some(plan) = session.and_then(|session| session.merge_plan.as_ref()) {
                conflict_resolver::ThreeWayAlignedMap::from_alignment(
                    &worktree_core::merge::align_merge_plan(plan),
                )
            } else if !base_text.is_empty()
                && !ours_text.is_empty()
                && !theirs_text.is_empty()
                && conflict_resolver::three_way_alignment_is_practical(
                    base_text,
                    ours_text,
                    theirs_text,
                )
            {
                conflict_resolver::ThreeWayAlignedMap::from_alignment(
                    &worktree_core::merge::align_three_way(
                        base_text,
                        ours_text,
                        theirs_text,
                        worktree_core::merge::DiffAlgorithm::Myers,
                    ),
                )
            } else if base_text.is_empty()
                && !ours_text.is_empty()
                && !theirs_text.is_empty()
                && conflict_resolver::two_way_alignment_is_practical(ours_text, theirs_text)
            {
                conflict_resolver::ThreeWayAlignedMap::from_alignment(
                    &worktree_core::merge::align_two_way(
                        ours_text,
                        theirs_text,
                        worktree_core::merge::DiffAlgorithm::Myers,
                    ),
                )
            } else {
                conflict_resolver::ThreeWayAlignedMap::default()
            };
        let three_way_len = if three_way_aligned.is_identity() {
            three_way_side_max_len
        } else {
            three_way_aligned.aligned_len()
        };
        let full_syntax_parse_requested = input.conflict_syntax_language.is_some()
            && [base_text, ours_text, theirs_text]
                .into_iter()
                .any(|text| !text.is_empty());
        let mut trace_decisions = MergetoolBootstrapTraceDecisions {
            rendering_mode: Some(trace_rendering_mode(rendering_mode)),
            full_syntax_parse_requested: Some(full_syntax_parse_requested),
            ..Default::default()
        };
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::ParseConflictMarkers,
                    marker_parse_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_resolver::conflict_count(
                    &marker_segments,
                )))
        });

        // When conflict markers are 2-way (no base section), populate block.base
        // from the git ancestor file so "A (base)" picks work.
        if let Some(base_text) = file.base.clone() {
            conflict_resolver::populate_block_bases_from_shared_ancestor(
                &mut marker_segments,
                base_text,
            );
        }
        let original_display_aligned_ranges =
            conflict_resolver::project_conflict_ranges_to_aligned_rows(
                &marker_segments,
                &three_way_aligned,
                [three_way_base_len, three_way_ours_len, three_way_theirs_len],
            );
        let original_region_aligned_ranges = session
            .map(|session| {
                conflict_resolver::conflict_nav_region_aligned_ranges(
                    session,
                    &original_display_aligned_ranges,
                )
            })
            .unwrap_or_else(|| {
                original_display_aligned_ranges
                    .iter()
                    .cloned()
                    .map(Some)
                    .collect()
            });
        let mut conflict_region_indices =
            conflict_resolver::sequential_conflict_region_indices(&marker_segments);
        let mut display_plan_block_indices = Vec::new();
        if let Some(session) = session {
            if let Some((_, projected_plan_blocks)) = plan_projection.as_ref()
                && let Some(applied) =
                    conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
                        &mut marker_segments,
                        session,
                        projected_plan_blocks,
                    )
            {
                conflict_region_indices = applied.block_region_indices;
                display_plan_block_indices = applied.block_plan_indices;
            } else {
                let applied = conflict_resolver::apply_session_region_resolutions_with_index_map(
                    &mut marker_segments,
                    &session.regions,
                );
                conflict_region_indices = applied.block_region_indices;
            }
        }
        let merge_plan_aligned_conflict_ranges = session.and_then(|session| {
            conflict_resolver::merge_plan_aligned_conflict_ranges(
                session,
                &conflict_region_indices,
                &display_plan_block_indices,
            )
        });
        let conflict_block_count = conflict_resolver::conflict_count(&marker_segments);

        let resolved_started = Instant::now();
        let (resolved_output_text, streamed_output_projection) = if output_is_protected {
            trace_decisions.full_output_generated = Some(false);
            (
                current_text
                    .clone()
                    .map(conflict_resolver::ResolvedOutputText::Shared),
                None,
            )
        } else if rendering_mode.is_streamed_large_file() && !marker_segments.is_empty() {
            trace_decisions.full_output_generated = Some(false);
            (
                None,
                Some(conflict_resolver::ResolvedOutputProjection::from_segments(
                    &marker_segments,
                )),
            )
        } else {
            trace_decisions.full_output_generated = Some(true);
            (
                Some(conflict_resolver::bootstrap_resolved_output_text(
                    &marker_segments,
                    marker_snapshot.as_ref(),
                    file.ours.as_ref(),
                    file.theirs.as_ref(),
                )),
                None,
            )
        };
        let resolved_line_count = if mergetool_trace::is_enabled() {
            streamed_output_projection
                .as_ref()
                .map(conflict_resolver::ResolvedOutputProjection::len)
                .or_else(|| {
                    resolved_output_text
                        .as_ref()
                        .map(|resolved| resolved.line_count())
                })
        } else {
            None
        };
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::GenerateResolvedText,
                    resolved_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
                .with_resolved_output_line_count(resolved_line_count)
        });

        // Use `SharedString::from` (not `SharedString::new`) so the existing
        // `Arc<str>` is passed through to the `SmolStr` backing without a fresh
        // allocation. `SharedString::new` always copies via `SmolStr::new`,
        // whereas `From<Arc<str>>` reuses the heap allocation for non-inline
        // strings.
        let three_way_text = ThreeWaySides {
            base: file
                .base
                .clone()
                .map(SharedString::from)
                .unwrap_or_default(),
            ours: file
                .ours
                .clone()
                .map(SharedString::from)
                .unwrap_or_default(),
            theirs: file
                .theirs
                .clone()
                .map(SharedString::from)
                .unwrap_or_default(),
        };
        let three_way_line_starts: ThreeWaySides<DeferredLineStarts> = ThreeWaySides {
            base: DeferredLineStarts::with_line_count(three_way_base_len),
            ours: DeferredLineStarts::with_line_count(three_way_ours_len),
            theirs: DeferredLineStarts::with_line_count(three_way_theirs_len),
        };

        // Conflicts now always use the streamed split index. Bootstrap only
        // records the lazy row count here; visible projections are rebuilt
        // after state construction.
        let diff_rows_started = Instant::now();
        let index = conflict_resolver::ConflictSplitRowIndex::new(
            &marker_segments,
            conflict_resolver::BLOCK_LOCAL_DIFF_CONTEXT_LINES,
        );
        trace_decisions.whole_block_diff_ran = Some(false);
        let diff_row_count = index.total_rows();
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::SideBySideRows,
                    diff_rows_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
                .with_diff_row_count(Some(diff_row_count))
        });
        let mode_state = ConflictModeState::Streamed(StreamedConflictState {
            split_row_index: index,
            ..StreamedConflictState::default()
        });
        let inline_row_count = 0;

        // section 30 R11: the aligned row space gives exact base↔side line pairs, so
        // word highlights come from a row-capped per-line word diff instead of
        // the old whole-file side_by_side/myers pass (which the streamed path
        // had to skip). Identity maps (no side texts / impractical alignment)
        // and both-added conflicts (two-way highlight path) stay empty.
        let three_way_word_highlights_started = Instant::now();
        let three_way_word_highlights = if !three_way_aligned.is_identity() && !base_text.is_empty()
        {
            let (wh_base, wh_ours, wh_theirs) =
                conflict_resolver::compute_aligned_three_way_word_highlights(
                    &three_way_aligned,
                    base_text,
                    three_way_line_starts.base.starts(base_text),
                    ours_text,
                    three_way_line_starts.ours.starts(ours_text),
                    theirs_text,
                    three_way_line_starts.theirs.starts(theirs_text),
                );
            ThreeWaySides {
                base: wh_base,
                ours: wh_ours,
                theirs: wh_theirs,
            }
        } else {
            ThreeWaySides::default()
        };
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::ComputeThreeWayWordHighlights,
                    three_way_word_highlights_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
        });

        // section 30 R11: aligned two-way (ours↔theirs) word highlights, computed
        // once here and shared by both diff columns, replacing the old
        // per-render/per-column inline word diff. Independent of base, so this
        // runs even for both-added conflicts where the three-way pass stays empty.
        let two_way_word_highlights_started = Instant::now();
        let two_way_aligned_word_highlights = if three_way_aligned.is_identity() {
            FxHashMap::default()
        } else {
            conflict_resolver::compute_aligned_two_way_word_highlights(
                &three_way_aligned,
                ours_text,
                three_way_line_starts.ours.starts(ours_text),
                theirs_text,
                three_way_line_starts.theirs.starts(theirs_text),
            )
        };
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::ComputeTwoWayWordHighlights,
                    two_way_word_highlights_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
                .with_diff_row_count(Some(diff_row_count))
        });

        // Three-way conflict maps and visible state are deferred to
        // `rebuild_three_way_visible_state()` after state construction.

        let three_way_source_available = file.base.is_some()
            || (needs_full_side_texts
                && matches!(
                    input.conflict_kind,
                    Some(worktree_core::domain::FileConflictKind::BothModified)
                ));
        let view_mode = if is_same_conflict {
            self.conflict_resolver.view_mode
        } else if matches!(
            input.conflict_strategy,
            Some(worktree_core::conflict_session::ConflictResolverStrategy::FullTextResolver)
        ) && three_way_source_available
            && self.mergetool_view_three_way
        {
            ConflictResolverViewMode::ThreeWay
        } else {
            // Base-absent conflicts and non-full-text strategies always open
            // two-way; base-present opens honor the persisted last-used mode.
            ConflictResolverViewMode::TwoWayDiff
        };

        let hide_resolved = if is_same_conflict {
            self.conflict_resolver.hide_resolved
        } else {
            repo.conflict_state.conflict_hide_resolved
        };
        let collapse_context = if is_same_conflict {
            self.conflict_resolver.collapse_context
        } else {
            // Fresh opens honor the persisted collapse-unchanged default.
            self.mergetool_collapse_unchanged
        };
        let nav_anchor = if is_same_conflict {
            self.conflict_resolver.nav_anchor
        } else {
            None
        };
        let nav_targets = if is_same_conflict {
            self.conflict_resolver.nav_targets.clone()
        } else {
            Vec::new()
        };
        let active_conflict = if is_same_conflict {
            self.conflict_resolver
                .active_conflict
                .filter(|index| *index < conflict_resolver::conflict_count(&marker_segments))
        } else {
            None
        };
        let resolver_preview_mode = if is_same_conflict {
            self.conflict_resolver.resolver_preview_mode
        } else {
            ConflictResolverPreviewMode::default()
        };
        let last_autosolve_summary = if is_same_conflict {
            self.conflict_resolver.last_autosolve_summary.clone()
        } else {
            repo.conflict_state
                .conflict_session
                .as_ref()
                .and_then(conflict_resolver::on_open_autosolve_summary)
                .map(Into::into)
        };
        let session_open_summary = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .filter(|session| session.path.as_path() == input.path.as_path())
            // CurrentOnly is a provisional marker-only session. Wait for its
            // Full upgrade so the open snapshot uses the plan-backed KDiff3
            // denominator and exact whitespace classification.
            .filter(|_| !full_text_plan_upgrade_expected)
            .map(conflict_resolver::conflict_session_summary_counts);
        // Same-conflict syncs keep the open-time snapshot, but backfill it when
        // still unset: the fast CurrentOnly first paint can run before the
        // session (and its autosolve pass) exists.
        let open_summary_counts =
            if is_same_conflict && self.conflict_resolver.open_summary_counts.is_some() {
                self.conflict_resolver.open_summary_counts
            } else {
                session_open_summary
            };
        let open_summary_announced = (is_same_conflict
            && self.conflict_resolver.open_summary_announced)
            || self
                .conflict_open_summary_toasted_files
                .contains(&(input.repo_id, input.path.clone()));

        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();

        // Try foreground tree-sitter parse for each merge-input side.
        // If a parse times out, we schedule a background task below.
        let budget = self.full_document_syntax_budget();
        let mut three_way_prepared_docs =
            ThreeWaySides::<Option<rows::PreparedDiffSyntaxDocument>>::default();
        let mut three_way_needs_background = ThreeWaySides::<bool>::default();
        if let Some(language) = input.conflict_syntax_language {
            for side in ThreeWayColumn::ALL {
                let text = &three_way_text[side];
                let doc_slot = &mut three_way_prepared_docs[side];
                let bg_slot = &mut three_way_needs_background[side];
                if text.is_empty() {
                    continue;
                }
                let line_starts = three_way_line_starts[side].shared_starts(text.as_ref());
                match rows::prepare_diff_syntax_document_with_budget_reuse_text(
                    language,
                    rows::DiffSyntaxMode::Auto,
                    text.clone(),
                    line_starts.clone(),
                    budget,
                    None,
                    None,
                ) {
                    rows::PrepareDiffSyntaxDocumentResult::Ready(doc) => {
                        *doc_slot = Some(doc);
                    }
                    rows::PrepareDiffSyntaxDocumentResult::TimedOut => {
                        *bg_slot = true;
                    }
                    rows::PrepareDiffSyntaxDocumentResult::Unsupported => {}
                }
            }
        }
        self.conflict_three_way_prepared_syntax_documents = three_way_prepared_docs;
        self.conflict_three_way_syntax_inflight = ThreeWaySides::default();
        Some(ConflictBootstrap {
            post: ConflictBootstrapPost {
                trace_ctx,
                trace_decisions,
                bootstrap_started,
                conflict_block_count,
                diff_row_count,
                inline_row_count,
                resolved_line_count,
                resolved_output_text,
                streamed_output_projection,
                is_same_conflict,
                needs_full_side_texts,
                full_text_plan_upgrade_expected,
                three_way_needs_background,
            },
            marker_snapshot,
            output_is_protected,
            marker_segments,
            collapse_context,
            conflict_region_indices,
            display_plan_block_indices,
            conflict_region_marker_has_base,
            active_conflict,
            nav_targets,
            original_region_aligned_ranges,
            mode_state,
            view_mode,
            three_way_text,
            three_way_line_starts,
            three_way_len,
            three_way_aligned,
            merge_plan_aligned_conflict_ranges,
            three_way_word_highlights,
            two_way_aligned_word_highlights,
            nav_anchor,
            hide_resolved,
            last_autosolve_summary,
            open_summary_counts,
            open_summary_announced,
            resolver_preview_mode,
        })
    }

    pub(in crate::view::panes::main) fn sync_conflict_resolver(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(target) = self.conflict_resolver_sync_target() else {
            self.clear_conflict_resolver_state();
            return;
        };
        let ConflictSyncTarget {
            repo_id,
            path,
            conflict_kind,
        } = target;
        // The gate already established this repo exists; nothing between the
        // gate and this lookup mutates `self.state.repos`, so the `else` is
        // unreachable and simply skips the sync.
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return;
        };

        let should_load = repo.conflict_state.conflict_file_path.as_ref() != Some(&path)
            && !matches!(repo.conflict_state.conflict_file, Loadable::Loading);
        if should_load {
            self.begin_conflict_file_current_only_load(repo_id, path, cx);
            return;
        }

        let Loadable::Ready(Some(file)) = &repo.conflict_state.conflict_file else {
            return;
        };
        if file.path != path {
            return;
        }

        let source_hash = conflict_file_source_fingerprint(file);

        let needs_rebuild = self.conflict_resolver.repo_id != Some(repo_id)
            || self.conflict_resolver.path.as_ref() != Some(&path)
            || self.conflict_resolver.source_hash != Some(source_hash);

        // When the file content hasn't changed but state-side conflict data has
        // been updated (e.g. hide_resolved toggled externally, bulk picks, or
        // autosolve applied from state), do a lightweight re-sync that re-applies
        // session resolutions and rebuilds visible maps without recomputing the
        // expensive diff/highlight data.
        if !needs_rebuild {
            if self.conflict_resolver.conflict_rev != repo.conflict_state.conflict_rev {
                self.resync_conflict_resolver_from_state(cx);
            }
            return;
        }

        self.conflict_diff_segments_cache_split.clear();
        self.conflict_diff_query_segments_cache_split.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.conflict_diff_query_cache_query = SharedString::default();

        // A CurrentOnly load intentionally omits all three immutable conflict
        // sides. Specialized resolvers need those exact bytes before their
        // completion actions can be enabled.
        let needs_full_side_payloads =
            file.base_bytes.is_none() && file.ours_bytes.is_none() && file.theirs_bytes.is_none();

        // Use the ConflictSession from state for strategy if available,
        // otherwise fall back to local computation.
        let (conflict_strategy, is_binary) = if let Some(session) =
            &repo.conflict_state.conflict_session
        {
            let binary =
                session.base.is_binary() || session.ours.is_binary() || session.theirs.is_binary();
            (Some(session.strategy), binary)
        } else {
            let binary = conflict_file_is_binary(file);
            (
                Self::conflict_resolver_strategy(conflict_kind, binary),
                binary,
            )
        };
        let conflict_syntax_language = rows::diff_syntax_language_for_path(&path);
        let shared_path = worktree_state::msg::RepoPath::from(path.clone());

        // For binary conflicts, populate minimal state and return early.
        if is_binary {
            // Cloning detaches the file (and `conflict_rev` below) from the
            // `self.state.repos` borrow so the phase call can take `&mut self`.
            let file = file.clone();
            self.apply_binary_conflict_bootstrap(
                ConflictBootstrapInput {
                    repo_id,
                    path,
                    shared_path,
                    conflict_syntax_language,
                    source_hash,
                    conflict_strategy,
                    conflict_kind,
                    needs_full_side_payloads,
                    conflict_rev: repo.conflict_state.conflict_rev,
                },
                &file,
            );
            return;
        }

        // Detach the file from the `self.state.repos` borrow so the
        // compute phase can take `&mut self`. `conflict_rev` joins the
        // pack here: no `self.state.repos` write can intervene before the
        // install phase consumes it, so the value matches the monolith's
        // read inside the state literal.
        let file = file.clone();
        let input = ConflictBootstrapInput {
            repo_id,
            path: path.clone(),
            shared_path: shared_path.clone(),
            conflict_syntax_language,
            source_hash,
            conflict_strategy,
            conflict_kind,
            needs_full_side_payloads,
            conflict_rev: repo.conflict_state.conflict_rev,
        };
        let Some(bootstrap) = self.compute_conflict_bootstrap(&input, &file) else {
            // Unreachable: the gate found this repo and nothing mutated
            // `self.state.repos` since.
            return;
        };
        // Restore the monolith's locals so the install/finish code below
        // stays verbatim until those phases are extracted.
        let ConflictBootstrap {
            post,
            marker_snapshot,
            output_is_protected,
            marker_segments,
            collapse_context,
            conflict_region_indices,
            display_plan_block_indices,
            conflict_region_marker_has_base,
            active_conflict,
            nav_targets,
            original_region_aligned_ranges,
            mode_state,
            view_mode,
            three_way_text,
            three_way_line_starts,
            three_way_len,
            three_way_aligned,
            merge_plan_aligned_conflict_ranges,
            three_way_word_highlights,
            two_way_aligned_word_highlights,
            nav_anchor,
            hide_resolved,
            last_autosolve_summary,
            open_summary_counts,
            open_summary_announced,
            resolver_preview_mode,
        } = bootstrap;
        let ConflictBootstrapPost {
            trace_ctx,
            trace_decisions,
            bootstrap_started,
            conflict_block_count,
            diff_row_count,
            inline_row_count,
            resolved_line_count,
            resolved_output_text,
            streamed_output_projection,
            is_same_conflict,
            needs_full_side_texts,
            full_text_plan_upgrade_expected,
            three_way_needs_background,
        } = post;
        let shared_path = worktree_state::msg::RepoPath::from(path.clone());

        // Build state with core/shared fields; mode-dependent visible state
        // is populated by the rebuild methods below.
        self.conflict_resolver = ConflictResolverUiState {
            repo_id: Some(repo_id),
            path: Some(path),
            shared_path: Some(shared_path),
            loaded_file: Some(file.clone()),
            conflict_syntax_language,
            source_hash: Some(source_hash),
            output_is_protected,
            // A re-bootstrap means a different conflict or different file
            // content, so an earlier waiver no longer speaks for it.
            output_protection_waived: false,
            current: marker_snapshot,
            marker_segments,
            collapse_context,
            context_fold_reveals: if is_same_conflict {
                std::mem::take(&mut self.conflict_resolver.context_fold_reveals)
            } else {
                FxHashMap::default()
            },
            resolved_output_visible: None,
            resolved_output_visible_dirty: true,
            output_context_fold_reveals: if is_same_conflict {
                std::mem::take(&mut self.conflict_resolver.output_context_fold_reveals)
            } else {
                FxHashMap::default()
            },
            conflict_region_indices,
            display_plan_block_indices,
            conflict_region_marker_has_base,
            active_conflict,
            nav_targets,
            original_region_aligned_ranges,
            hovered_conflict: None,
            // section 30 split: any pending row selection is invalidated by a source
            // rebuild (which happens after a split changes the segmentation).
            row_selection: None,
            // Pending alignment marks are line numbers into the old source, so
            // a rebuild invalidates them the same way.
            alignment_selection: ThreeWaySides::default(),
            mode_state,
            view_mode,
            three_way_text,
            three_way_line_starts,
            three_way_len,
            three_way_aligned,
            minimap_bands: Arc::from([]),
            merge_plan_aligned_conflict_ranges,
            three_way_visible_state_ready: false,
            three_way_conflict_ranges: ThreeWaySides::default(),
            three_way_horizontal_measure_rows: [0; 3],
            conflict_has_base: Vec::new(),
            conflict_choices: Vec::new(),
            two_way_split_visual_kind_cache: FxHashMap::default(),
            two_way_horizontal_measure_rows: [0; 2],
            three_way_word_highlights,
            two_way_aligned_word_highlights,
            two_way_split_word_highlight_cache: Default::default(),
            nav_anchor,
            hide_resolved,
            is_binary_conflict: false,
            binary_side_sizes: [None; 3],
            strategy: conflict_strategy,
            conflict_kind,
            last_autosolve_summary,
            open_summary_counts,
            open_summary_announced,
            conflict_rev: input.conflict_rev,
            resolver_pending_recompute_seq: 0,
            resolved_outline: ResolvedOutlineData::default(),
            resolved_outline_gutter_rows: Vec::new(),
            markdown_preview: ConflictResolverMarkdownPreviewState::default(),
            image_preview: ConflictResolverImagePreviewState::default(),
            resolver_preview_mode,
        };
        // Populate mode-dependent visible state using the same code path as
        // later rebuilds (hide-resolved toggle, conflict picks, etc.). The
        // aligned two-way view shares the three-way projection, so it needs
        // the same build.
        let three_way_rebuild_started = Instant::now();
        if self.conflict_resolver.view_mode == ConflictResolverViewMode::ThreeWay
            || self.conflict_resolver.two_way_uses_aligned_rows()
        {
            self.conflict_resolver.rebuild_three_way_visible_state();
        } else {
            self.conflict_resolver
                .refresh_conflict_has_base_from_segments();
        }
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::BuildThreeWayConflictMaps,
                    three_way_rebuild_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
        });
        self.conflict_resolver.rebuild_two_way_visible_projections();
        self.conflict_resolver_refresh_nav_targets();

        let output_path = self.conflict_resolver.path.clone();
        if let Some(projection) = streamed_output_projection {
            self.refresh_streamed_resolved_output_preview_from_projection(
                projection,
                output_path.as_ref(),
            );
        } else if let Some(resolved) = resolved_output_text {
            self.conflict_resolved_output_projection = None;
            let input_set_text_started = Instant::now();
            self.fill_conflict_resolved_output_buffer(resolved.into_shared_string(), cx);
            mergetool_trace::record_with(|| {
                trace_ctx
                    .bootstrap_event(
                        MergetoolTraceStage::ConflictResolverInputSetText,
                        input_set_text_started,
                        trace_decisions,
                    )
                    .with_conflict_block_count(Some(conflict_block_count))
                    .with_diff_row_count(Some(diff_row_count))
                    .with_inline_row_count(Some(inline_row_count))
                    .with_resolved_output_line_count(resolved_line_count)
            });
            self.conflict_resolved_preview_path = output_path.clone();
            let source_revision = self.conflict_resolver_input.read_with(cx, |input, _| {
                ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
            });
            self.conflict_resolved_preview_source_revision = Some(source_revision);
            self.schedule_conflict_resolved_outline_recompute(
                output_path.clone(),
                source_revision,
                None,
                cx,
            );
        }
        // The resolved output is an editable, kdiff3-style free-text pane, so the
        // merged text must live in the buffer (not a read-only streamed
        // projection). Materialize here at bootstrap — driven, deterministic, and
        // one-time — rather than in the render path. Once materialized the output
        // stays out of streamed mode for this open, so every downstream refresh
        // keeps the buffer authoritative (all streamed paths are gated on
        // `conflict_resolved_output_is_streamed`).
        self.ensure_conflict_resolved_output_materialized(cx);
        self.rebuild_conflict_resolved_output_block_map(cx);
        self.mark_conflict_resolved_output_saved(cx);
        // On a fresh open, center the first unresolved semantic target (then
        // the first original conflict, then the first delta). Deferred item
        // scrolls apply once the lists lay out.
        if !is_same_conflict
            && let Some(target_index) = self.conflict_resolver.selected_nav_target_index()
        {
            self.conflict_jump_to_nav_target(target_index, cx);
        }
        // kdiff3-style one-shot open summary: announce total / auto-solved /
        // unsolved once per resolver open, as soon as the stage-backed report
        // is available (the fast first paint may be CurrentOnly).
        if !self.conflict_resolver.open_summary_announced
            && let Some(counts) = self.conflict_resolver.open_summary_counts
            && let Some(message) = conflict_resolver::format_open_summary_toast(counts)
        {
            self.conflict_resolver.open_summary_announced = true;
            if let (Some(repo_id), Some(path)) = (
                self.conflict_resolver.repo_id,
                self.conflict_resolver.path.as_ref(),
            ) {
                self.conflict_open_summary_toasted_files
                    .insert((repo_id, path.clone()));
            }
            // The sync runs inside a WorkTreeView update; push the
            // toast after the current update flush to avoid reentrant
            // root-view updates.
            let root_view = self.root_view.clone();
            cx.defer(move |cx| {
                let _ = root_view.update(cx, |root, cx| {
                    root.push_toast(crate::view::components::ToastKind::Success, message, cx);
                });
            });
        }
        // section 30 aligned row space: whole-file column rows (three-way and
        // two-way full mode) need the side texts, which the fast CurrentOnly
        // first paint does not include. Upgrade fresh opens of reasonably
        // sized text conflicts to a Full load in the background; this
        // bootstrap re-runs with the sides once it lands. Giant files stay
        // on the block-local rows (the alignment gates reject them anyway).
        let specialized_strategy_needs_full_sides =
            conflict_strategy_needs_full_side_payloads(conflict_strategy);
        if !is_same_conflict
            && needs_full_side_texts
            && (specialized_strategy_needs_full_sides || full_text_plan_upgrade_expected)
        {
            let _ = self
                .request_conflict_file_load_mode(worktree_state::model::ConflictFileLoadMode::Full);
        }
        mergetool_trace::record_with(|| {
            trace_ctx
                .bootstrap_event(
                    MergetoolTraceStage::ConflictResolverBootstrapTotal,
                    bootstrap_started,
                    trace_decisions,
                )
                .with_conflict_block_count(Some(conflict_block_count))
                .with_diff_row_count(Some(diff_row_count))
                .with_inline_row_count(Some(inline_row_count))
                .with_resolved_output_line_count(resolved_line_count)
        });

        // Schedule background syntax parses for merge-input sides that timed out.
        // Collect data up front to avoid borrowing conflict_resolver across the
        // mutable ensure_* call.
        if let Some(language) = conflict_syntax_language {
            let bg_source_hash = self.conflict_resolver.source_hash;
            let bg_sides: Vec<_> = ThreeWayColumn::ALL
                .into_iter()
                .filter(|&side| three_way_needs_background[side])
                .map(|side| {
                    (
                        side,
                        self.conflict_resolver.three_way_text[side].clone(),
                        self.conflict_resolver.three_way_shared_line_starts(side),
                    )
                })
                .collect();
            for (side, text, line_starts) in bg_sides {
                self.ensure_conflict_three_way_background_syntax_prepare(
                    side,
                    text,
                    line_starts,
                    language,
                    bg_source_hash,
                    cx,
                );
            }
        }

        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_preserving_current();
        }
    }

    /// Lightweight re-sync when `conflict_rev` changed but file content is the
    /// same. Re-parses markers, re-applies session resolutions, reads
    /// `hide_resolved` from state, and rebuilds visible maps — without
    /// recomputing the expensive diff rows and word highlights.
    pub(super) fn resync_conflict_resolver_from_state(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return;
        };
        let Loadable::Ready(Some(file)) = &repo.conflict_state.conflict_file else {
            return;
        };
        let previous_blocks: Vec<_> = self
            .conflict_resolver
            .marker_segments
            .iter()
            .filter_map(|segment| match segment {
                conflict_resolver::ConflictSegment::Block(block) => Some(block.clone()),
                conflict_resolver::ConflictSegment::Text(_) => None,
            })
            .collect();
        let previous_region_indices = self.conflict_resolver.conflict_region_indices.clone();
        let previous_marker_projection = self.conflict_resolver.current.clone();
        let previous_output_is_protected = self.conflict_resolver.output_is_protected;
        let live_materialized_output = (!self.conflict_resolved_output_is_streamed()).then(|| {
            self.conflict_resolver_input
                .read_with(cx, |input, _| input.text().to_string())
        });
        let previous_map_valid = live_materialized_output.as_ref().is_some_and(|output| {
            self.conflict_resolved_output_block_map
                .is_valid_for(&self.conflict_resolver.marker_segments, output.as_str())
        });
        let previous_generated_output_matches_live =
            live_materialized_output.as_deref().is_some_and(|output| {
                conflict_resolver::generate_resolved_text(&self.conflict_resolver.marker_segments)
                    == output
            });

        let worktree_current = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .and_then(|session| match session.current.as_ref() {
                Some(worktree_core::conflict_session::ConflictPayload::Text(text)) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .or_else(|| file.current.clone());
        let structural_marker_snapshot = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .and_then(|session| session.marker_projection.clone())
            .or_else(|| worktree_current.clone());
        let plan_projection = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .and_then(conflict_session_plan_projection);
        let marker_snapshot = plan_projection
            .as_ref()
            .map(|(text, _)| Arc::clone(text))
            .or_else(|| structural_marker_snapshot.clone());
        let next_output_is_protected = !self.conflict_resolver.output_protection_waived
            && worktree_output_requires_protection(
                worktree_current.as_deref(),
                structural_marker_snapshot.as_deref(),
                file.base.as_deref(),
                file.ours.as_deref(),
                file.theirs.as_deref(),
            );
        // The stage-derived marker snapshot drives conflict geometry. The
        // worktree payload remains independent so a partial or complete manual
        // resolution can be retained without making stale worktree markers the
        // structural source of truth.
        let mut marker_segments = marker_snapshot
            .clone()
            .map(conflict_resolver::parse_conflict_markers_shared_nonempty)
            .unwrap_or_default();
        let conflict_region_marker_has_base = marker_segments
            .iter()
            .filter_map(|segment| match segment {
                conflict_resolver::ConflictSegment::Block(block) => Some(block.base.is_some()),
                conflict_resolver::ConflictSegment::Text(_) => None,
            })
            .collect();
        // Re-populate bases from ancestor (needed for 2-way markers).
        if let Some(base_text) = file.base.clone() {
            conflict_resolver::populate_block_bases_from_shared_ancestor(
                &mut marker_segments,
                base_text,
            );
        }
        let original_display_aligned_ranges =
            conflict_resolver::project_conflict_ranges_to_aligned_rows(
                &marker_segments,
                &self.conflict_resolver.three_way_aligned,
                [
                    self.conflict_resolver
                        .three_way_line_count(ThreeWayColumn::Base),
                    self.conflict_resolver
                        .three_way_line_count(ThreeWayColumn::Ours),
                    self.conflict_resolver
                        .three_way_line_count(ThreeWayColumn::Theirs),
                ],
            );
        let mut conflict_region_indices =
            conflict_resolver::sequential_conflict_region_indices(&marker_segments);

        // Re-apply session region resolutions from state.
        let session = repo
            .conflict_state
            .conflict_session
            .as_ref()
            .filter(|session| session.path == file.path);
        let original_region_aligned_ranges = session
            .map(|session| {
                conflict_resolver::conflict_nav_region_aligned_ranges(
                    session,
                    &original_display_aligned_ranges,
                )
            })
            .unwrap_or_else(|| {
                original_display_aligned_ranges
                    .iter()
                    .cloned()
                    .map(Some)
                    .collect()
            });
        let mut display_plan_block_indices = Vec::new();
        if let Some(session) = session {
            if let Some((_, projected_plan_blocks)) = plan_projection.as_ref()
                && let Some(applied) =
                    conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
                        &mut marker_segments,
                        session,
                        projected_plan_blocks,
                    )
            {
                conflict_region_indices = applied.block_region_indices;
                display_plan_block_indices = applied.block_plan_indices;
            } else {
                let applied = conflict_resolver::apply_session_region_resolutions_with_index_map(
                    &mut marker_segments,
                    &session.regions,
                );
                conflict_region_indices = applied.block_region_indices;
            }
        }
        let merge_plan_aligned_conflict_ranges = session.and_then(|session| {
            conflict_resolver::merge_plan_aligned_conflict_ranges(
                session,
                &conflict_region_indices,
                &display_plan_block_indices,
            )
        });

        let use_streamed_projection = self.conflict_resolved_output_is_streamed()
            && !marker_segments.is_empty()
            && !next_output_is_protected;
        let next_blocks: Vec<_> = marker_segments
            .iter()
            .filter_map(|segment| match segment {
                conflict_resolver::ConflictSegment::Block(block) => Some(block),
                conflict_resolver::ConflictSegment::Text(_) => None,
            })
            .collect();
        let mapped_replacements = (previous_marker_projection.as_deref()
            == marker_snapshot.as_deref()
            && previous_map_valid
            && previous_region_indices == conflict_region_indices
            && previous_blocks.len() == next_blocks.len()
            && previous_blocks
                .iter()
                .zip(&next_blocks)
                .all(|(previous, next)| {
                    previous.base == next.base
                        && previous.ours == next.ours
                        && previous.theirs == next.theirs
                }))
        .then(|| {
            previous_blocks
                .iter()
                .zip(&next_blocks)
                .enumerate()
                .filter_map(|(index, (previous, next))| {
                    (previous.choice != next.choice || previous.resolved != next.resolved)
                        .then_some(index)
                })
                .collect::<Vec<_>>()
        });
        let resolved = (!use_streamed_projection).then(|| {
            if next_output_is_protected {
                worktree_current
                    .clone()
                    .map(conflict_resolver::ResolvedOutputText::Shared)
                    .unwrap_or_else(|| {
                        conflict_resolver::bootstrap_resolved_output_text(
                            &marker_segments,
                            marker_snapshot.as_ref(),
                            file.ours.as_ref(),
                            file.theirs.as_ref(),
                        )
                    })
            } else {
                conflict_resolver::bootstrap_resolved_output_text(
                    &marker_segments,
                    marker_snapshot.as_ref(),
                    file.ours.as_ref(),
                    file.theirs.as_ref(),
                )
            }
        });

        // Read hide_resolved from state (authoritative source).
        let hide_resolved = repo.conflict_state.conflict_hide_resolved;

        let new_rev = repo.conflict_state.conflict_rev;

        // Update only the fields that change during a state re-sync.
        self.conflict_resolver.current = marker_snapshot;
        self.conflict_resolver.output_is_protected = next_output_is_protected;
        self.conflict_resolver.marker_segments = marker_segments;
        self.conflict_resolver.conflict_region_indices = conflict_region_indices;
        self.conflict_resolver.display_plan_block_indices = display_plan_block_indices;
        self.conflict_resolver.merge_plan_aligned_conflict_ranges =
            merge_plan_aligned_conflict_ranges;
        self.conflict_resolver.original_region_aligned_ranges = original_region_aligned_ranges;
        self.conflict_resolver.conflict_region_marker_has_base = conflict_region_marker_has_base;
        self.conflict_resolver.hide_resolved = hide_resolved;
        self.conflict_resolver.row_selection = None;
        self.conflict_resolver.conflict_syntax_language = self
            .conflict_resolver
            .path
            .as_ref()
            .and_then(rows::diff_syntax_language_for_path);
        self.conflict_resolver.loaded_file = Some(file.clone());
        self.conflict_resolver.conflict_rev = new_rev;

        // Clear segment caches since marker_segments changed.
        self.clear_conflict_diff_style_caches();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.conflict_resolver_rebuild_visible_map();

        let output_path = self.conflict_resolver.path.clone();
        // Protection has to be able to clear itself. Carrying
        // `previous_output_is_protected` alone re-armed the flag on every
        // resync, so a session that was protected once stayed protected for
        // good — with the markers undecorated and every pick a silent no-op —
        // no matter what the predicate said afterwards. Unsaved manual edits to
        // the buffer are still held by the second disjunct, which is the case
        // that branch exists for.
        let preserve_unmapped_live_output = live_materialized_output.is_some()
            && ((previous_output_is_protected && next_output_is_protected)
                || (self.conflict_resolved_output_modified
                    && mapped_replacements.is_none()
                    && !previous_generated_output_matches_live));
        let mut preserved_materialized_output = preserve_unmapped_live_output;
        if preserve_unmapped_live_output {
            self.conflict_resolver.output_is_protected = true;
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
        }
        if use_streamed_projection {
            self.refresh_streamed_resolved_output_preview_from_markers(output_path.as_ref());
        } else if !preserved_materialized_output && let Some(block_indices) = mapped_replacements {
            let choices_unchanged = block_indices.is_empty();
            preserved_materialized_output = choices_unchanged
                || self.conflict_resolver_replace_mapped_blocks(&block_indices, cx);
            if preserved_materialized_output && choices_unchanged {
                let source_revision = self.conflict_resolver_input.read_with(cx, |input, _| {
                    ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
                });
                self.conflict_resolved_preview_path = output_path.clone();
                self.conflict_resolved_preview_source_revision = Some(source_revision);
                self.schedule_conflict_resolved_outline_recompute(
                    output_path.clone(),
                    source_revision,
                    None,
                    cx,
                );
            }
        }
        if !use_streamed_projection
            && !preserved_materialized_output
            && let Some(resolved) = resolved
        {
            self.conflict_resolved_output_projection = None;
            self.fill_conflict_resolved_output_buffer(resolved.into_shared_string(), cx);
            self.conflict_resolved_preview_path = output_path.clone();
            let source_revision = self.conflict_resolver_input.read_with(cx, |input, _| {
                ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
            });
            self.conflict_resolved_preview_source_revision = Some(source_revision);
            self.schedule_conflict_resolved_outline_recompute(
                output_path,
                source_revision,
                None,
                cx,
            );
        }
        if !preserved_materialized_output {
            self.rebuild_conflict_resolved_output_block_map(cx);
        }

        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_preserving_current();
        }
    }

    pub(in crate::view) fn request_conflict_file_load_mode(
        &mut self,
        mode: worktree_state::model::ConflictFileLoadMode,
    ) -> bool {
        let Some(repo_id) = self.active_repo_id() else {
            return false;
        };
        let Some(path) = self.conflict_resolver.path.clone() else {
            return false;
        };
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        if repo.conflict_state.conflict_file_path.as_ref() != Some(&path) {
            return false;
        }
        if repo.conflict_state.conflict_file_load_mode == mode
            || matches!(repo.conflict_state.conflict_file, Loadable::Loading)
        {
            return false;
        }

        self.store.dispatch(Msg::LoadConflictFile {
            repo_id,
            path,
            mode,
        });
        true
    }

    pub(in crate::view) fn conflict_resolver_set_view_mode(
        &mut self,
        view_mode: ConflictResolverViewMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolver.view_mode == view_mode {
            if view_mode == ConflictResolverViewMode::ThreeWay {
                let _ = self.request_conflict_file_load_mode(
                    worktree_state::model::ConflictFileLoadMode::Full,
                );
            }
            return;
        }
        self.conflict_resolver.view_mode = view_mode;
        self.set_mergetool_view_three_way_and_persist(
            view_mode == ConflictResolverViewMode::ThreeWay,
            cx,
        );
        self.conflict_resolver.hovered_conflict = None;
        // View-mode switches rebuild visible projections and can temporarily
        // reuse the same cache keys with different row text or syntax state.
        // Drop both caches so the next draw restyles from the current prepared
        // documents instead of pinning stale fallback output across toggles.
        self.clear_conflict_diff_style_caches_preserving_query();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        if view_mode == ConflictResolverViewMode::ThreeWay
            && self
                .request_conflict_file_load_mode(worktree_state::model::ConflictFileLoadMode::Full)
        {
            // Build three-way visible state from the data we already have so
            // the view shows existing rows (with syntax) while the full file
            // reloads in the background.
            self.conflict_resolver.rebuild_three_way_visible_state();
            cx.notify();
            return;
        }
        if view_mode == ConflictResolverViewMode::ThreeWay {
            self.conflict_resolver.rebuild_three_way_visible_state();
        } else {
            // Rebuild two-way visible projections so the split view reflects
            // the current hide_resolved state and resolved conflict choices.
            self.conflict_resolver.rebuild_two_way_visible_projections();
        }
        let path = self.conflict_resolver.path.clone();
        let output_line_count = if self.conflict_resolved_output_is_streamed() {
            self.conflict_resolved_preview_line_count.max(1)
        } else {
            self.conflict_resolver_input.read_with(cx, |input, _| {
                input.text_snapshot().shared_line_starts().len().max(1)
            })
        };
        if should_skip_resolved_outline_provenance(view_mode, output_line_count) {
            // The existing marker overlay remains valid across view-mode switches,
            // but view-mode-specific provenance/dedupe metadata is too expensive to
            // rebuild synchronously for huge outputs.
            self.conflict_resolver.resolved_outline.meta.clear();
            self.conflict_resolver
                .resolved_outline
                .sources_index
                .clear();
            self.conflict_resolver.resolved_outline_gutter_rows.clear();
        } else {
            self.recompute_conflict_resolved_outline_and_provenance(path.as_ref(), cx);
        }
        if self.diff_search_has_query() {
            self.diff_search_recompute_matches_preserving_current();
        }
        cx.notify();
    }

    pub(in crate::view) fn conflict_resolver_toggle_hide_resolved(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.conflict_resolver.hide_resolved = !self.conflict_resolver.hide_resolved;
        self.conflict_resolver_rebuild_visible_map();
        if let (Some(repo_id), Some(path)) = (
            self.conflict_resolver
                .repo_id
                .or_else(|| self.active_repo_id()),
            self.conflict_resolver.dispatch_path(),
        ) {
            self.store.dispatch(Msg::ConflictSetHideResolved {
                repo_id,
                path,
                hide_resolved: self.conflict_resolver.hide_resolved,
            });
        }
        cx.notify();
    }

    /// Toggle section 30 collapsed context mode: fold unchanged runs beyond the
    /// per-conflict context window in the source columns.
    pub(in crate::view) fn conflict_resolver_toggle_collapse_context(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.conflict_resolver.collapse_context = !self.conflict_resolver.collapse_context;
        // The live toggle doubles as the persisted default for the next
        // conflicted file (cog-menu setting).
        if self.mergetool_collapse_unchanged != self.conflict_resolver.collapse_context {
            self.mergetool_collapse_unchanged = self.conflict_resolver.collapse_context;
            self.schedule_ui_settings_persist(cx);
        }
        self.conflict_resolver.context_fold_reveals.clear();
        self.conflict_resolver.output_context_fold_reveals.clear();
        self.conflict_resolver.resolved_output_visible_dirty = true;
        self.conflict_resolver_rebuild_visible_map();
        // Keep the semantic target in view across the row-space change.
        if let Some(target_index) = self.conflict_resolver.selected_nav_target_index()
            && let Some(target) = self.conflict_resolver.nav_targets.get(target_index)
            && let Some(vi) = self.conflict_resolver_visible_ix_for_nav_target(target)
        {
            self.conflict_resolver_scroll_all_columns(vi, gpui::ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Fully expand one collapsed context fold.
    pub(in crate::view) fn conflict_resolver_expand_context_fold(
        &mut self,
        fold_id: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let reveal = self
            .conflict_resolver
            .context_fold_reveals
            .entry(fold_id)
            .or_default();
        if reveal.expand_all {
            return;
        }
        reveal.expand_all = true;
        self.conflict_resolver_rebuild_visible_map();
        cx.notify();
    }

    /// Reveal [`CONFLICT_FOLD_REVEAL_STEP`] more lines at one edge of a fold
    /// (top = extend the context above downward; bottom = extend the context
    /// below upward), mirroring the diff view's collapsed-hunk arrows.
    ///
    /// [`CONFLICT_FOLD_REVEAL_STEP`]: conflict_resolver::CONFLICT_FOLD_REVEAL_STEP
    pub(in crate::view) fn conflict_resolver_reveal_context_fold(
        &mut self,
        fold_id: usize,
        from_top: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let reveal = self
            .conflict_resolver
            .context_fold_reveals
            .entry(fold_id)
            .or_default();
        if from_top {
            reveal.top += conflict_resolver::CONFLICT_FOLD_REVEAL_STEP;
        } else {
            reveal.bottom += conflict_resolver::CONFLICT_FOLD_REVEAL_STEP;
        }
        self.conflict_resolver_rebuild_visible_map();
        cx.notify();
    }

    pub(super) fn conflict_resolver_rebuild_visible_map(&mut self) {
        if self.conflict_resolver.view_mode == ConflictResolverViewMode::ThreeWay
            || self.conflict_resolver.has_three_way_visible_state_ready()
            || self.conflict_resolver.two_way_uses_aligned_rows()
        {
            self.conflict_resolver.rebuild_three_way_visible_state();
        } else {
            self.conflict_resolver
                .refresh_conflict_has_base_from_segments();
        }
        let block_count = self
            .conflict_resolver
            .marker_segments
            .iter()
            .filter(|seg| matches!(seg, conflict_resolver::ConflictSegment::Block(_)))
            .count();
        if self
            .conflict_resolver
            .hovered_conflict
            .is_some_and(|(ix, _)| ix >= block_count)
        {
            self.conflict_resolver.hovered_conflict = None;
        }
        self.conflict_resolver.rebuild_two_way_visible_state();
        self.conflict_resolver_refresh_nav_targets();
        self.conflict_resolver
            .debug_assert_rendering_mode_invariants();
    }

    pub(in crate::view) fn conflict_resolver_conflict_count(&self) -> usize {
        let (total, _) = conflict_resolver::effective_conflict_counts(
            &self.conflict_resolver.marker_segments,
            self.conflict_resolver_session_counts(),
        );
        total
    }

    pub(in crate::view) fn conflict_resolver_session_counts(&self) -> Option<(usize, usize)> {
        let resolver_path = self.conflict_resolver.path.as_ref()?;
        let session = self
            .active_repo()?
            .conflict_state
            .conflict_session
            .as_ref()?;
        if session.path.as_path() != resolver_path.as_path() {
            return None;
        }
        Some((session.total_regions(), session.solved_count()))
    }

    pub(in crate::view) fn conflict_resolver_summary_counts(
        &self,
    ) -> Option<conflict_resolver::ConflictSummaryCounts> {
        let resolver_path = self.conflict_resolver.path.as_ref()?;
        let session = self
            .active_repo()?
            .conflict_state
            .conflict_session
            .as_ref()?;
        if session.path.as_path() != resolver_path.as_path() {
            return None;
        }
        Some(conflict_resolver::conflict_session_summary_counts(session))
    }

    /// Live collapse-unchanged-context state, read by the cog settings menu.
    pub(in crate::view) fn conflict_resolver_collapse_context(&self) -> bool {
        self.conflict_resolver.collapse_context
    }

    pub(in crate::view) fn conflict_resolver_resolved_count(&self) -> usize {
        let (_, resolved) = conflict_resolver::effective_conflict_counts(
            &self.conflict_resolver.marker_segments,
            self.conflict_resolver_session_counts(),
        );
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_automatic_delta() -> worktree_core::conflict_session::ConflictSession {
        use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
        use worktree_core::domain::FileConflictKind;

        ConflictSession::from_stage_inputs(
            std::path::PathBuf::from("file.txt"),
            FileConflictKind::BothModified,
            ConflictPayload::Text("start\nold-local\nmiddle\nold-conflict\nend\n".into()),
            ConflictPayload::Text("start\nnew-local\nmiddle\nours-conflict\nend\n".into()),
            ConflictPayload::Text("start\nold-local\nmiddle\ntheirs-conflict\nend\n".into()),
        )
    }

    #[test]
    fn live_plan_projection_renders_an_automatic_delta_override() {
        use worktree_core::merge::MergeSource;

        let mut session = session_with_automatic_delta();
        let automatic_id = session
            .merge_plan
            .as_ref()
            .unwrap()
            .blocks
            .iter()
            .find(|block| block.is_delta && !block.original_conflict)
            .unwrap()
            .id;
        let (automatic, _) = conflict_session_plan_projection(&session).unwrap();
        assert!(automatic.contains("new-local\n"));

        assert!(session.replace_plan_block_selection(automatic_id, MergeSource::C.into()));
        let (overridden, _) = conflict_session_plan_projection(&session).unwrap();
        assert!(overridden.contains("old-local\n"));
        assert!(!overridden.contains("new-local\n"));
    }

    #[test]
    fn an_unresolved_automatic_delta_gets_a_visible_plan_block_mapping() {
        use worktree_core::merge::MergeSource;

        let mut session = session_with_automatic_delta();
        let (automatic_index, automatic_id) = session
            .merge_plan
            .as_ref()
            .unwrap()
            .blocks
            .iter()
            .enumerate()
            .find(|(_, block)| block.is_delta && !block.original_conflict)
            .map(|(index, block)| (index, block.id))
            .unwrap();
        assert!(session.toggle_plan_block_source(automatic_id, MergeSource::B));
        let (projection, projected_plan_blocks) =
            conflict_session_plan_projection(&session).unwrap();
        let mut segments = conflict_resolver::parse_conflict_markers(projection.as_ref());
        let applied = conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
            &mut segments,
            &session,
            &projected_plan_blocks,
        )
        .expect("exact mapping");
        let plan_blocks = applied.block_plan_indices;
        assert!(plan_blocks.contains(&automatic_index));
        assert_eq!(
            plan_blocks,
            session.merge_plan.as_ref().unwrap().unresolved_blocks
        );
    }

    #[test]
    fn plan_whitespace_classification_reaches_the_display_blocks() {
        use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
        use worktree_core::domain::FileConflictKind;

        // Both sides only respaced the same line, so kdiff3's per-row rule
        // marks the block whitespace-only.
        let session = ConflictSession::from_stage_inputs(
            std::path::PathBuf::from("file.txt"),
            FileConflictKind::BothModified,
            ConflictPayload::Text("value = 1\n".into()),
            ConflictPayload::Text("value=1\n".into()),
            ConflictPayload::Text("value  =  1\n".into()),
        );
        assert!(
            session
                .merge_plan
                .as_ref()
                .expect("plan-backed session")
                .blocks
                .iter()
                .any(|block| block.whitespace_conflict),
            "fixture should produce a whitespace conflict"
        );

        let (projection, projected_plan_blocks) =
            conflict_session_plan_projection(&session).unwrap();
        let mut segments = conflict_resolver::parse_conflict_markers(projection.as_ref());
        conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
            &mut segments,
            &session,
            &projected_plan_blocks,
        )
        .expect("exact mapping");

        assert!(
            segments.iter().any(|segment| matches!(
                segment,
                conflict_resolver::ConflictSegment::Block(block) if block.whitespace_only
            )),
            "the plan's whitespace verdict should land on the display block"
        );
    }

    #[test]
    fn conflict_file_source_fingerprint_is_stable_across_fresh_allocations() {
        let make_file = || worktree_state::model::ConflictFile {
            path: std::path::PathBuf::from("index.html").into(),
            base_bytes: Some(std::sync::Arc::<[u8]>::from(b"base\nbytes\n".as_slice())),
            ours_bytes: None,
            theirs_bytes: Some(std::sync::Arc::<[u8]>::from(b"theirs\nbytes\n".as_slice())),
            current_bytes: None,
            base: Some(std::sync::Arc::<str>::from("base\ntext\n")),
            ours: Some(std::sync::Arc::<str>::from("ours\ntext\n")),
            theirs: Some(std::sync::Arc::<str>::from("theirs\ntext\n")),
            current: Some(std::sync::Arc::<str>::from(
                "<<<<<<< ours\nbody\n=======\nbody\n>>>>>>> theirs\n",
            )),
        };

        let left = make_file();
        let right = make_file();

        assert_eq!(
            conflict_file_source_fingerprint(&left),
            conflict_file_source_fingerprint(&right),
            "content-identical conflict files should keep the lightweight resync path even when backing Arcs are freshly allocated",
        );
    }

    #[test]
    fn shared_content_fingerprints_keep_domains_distinct() {
        let none_text = None;
        let empty_text = Some(std::sync::Arc::<str>::from(""));
        let text = Some(std::sync::Arc::<str>::from("shared payload"));

        let none_bytes = None;
        let empty_bytes = Some(std::sync::Arc::<[u8]>::from(b"".as_slice()));
        let bytes = Some(std::sync::Arc::<[u8]>::from(b"shared payload".as_slice()));

        assert_ne!(
            shared_text_fingerprint(&none_text),
            shared_text_fingerprint(&empty_text),
            "missing text should not collide with an empty text payload",
        );
        assert_ne!(
            shared_bytes_fingerprint(&none_bytes),
            shared_bytes_fingerprint(&empty_bytes),
            "missing bytes should not collide with an empty byte payload",
        );
        assert_ne!(
            shared_text_fingerprint(&text),
            shared_bytes_fingerprint(&bytes),
            "text and byte payloads use separate fingerprint domains",
        );
    }
}
