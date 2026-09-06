//! The resolved-output (merge result) editor: live syntax, provenance outline
//! recomputation and the streamed/materialized output projection.
use super::super::helpers::{
    ResolvedOutlineDelta, ResolvedOutputKey, ResolvedOutputSourceRevision,
    StashedResolvedOutlineState, UnresolvedRows, apply_conflict_choice_provenance_hints,
    apply_conflict_choice_provenance_hints_for_ranges, build_resolved_output_conflict_markers,
    build_resolved_output_conflict_markers_from_block_ranges, conflict_marker_ranges_for_block,
    count_newlines, dirty_byte_range_to_line_range, indexed_line_count,
    remap_resolved_output_conflict_block_ranges_for_delta, resolved_outline_delta_between_texts,
    resolved_output_conflict_block_line_ranges, resolved_output_conflict_block_ranges_in_text,
    resolved_output_heuristic_highlight_provider, resolved_output_heuristic_provider_binding_key,
    resolved_output_live_highlight_provider, resolved_output_live_provider_binding_key,
    resolved_output_live_syntax_mask, resolved_output_placeholder_protected_ranges,
    resolved_output_unresolved_rows, resolved_output_unresolved_spans_for_active,
    shifted_line_index, should_skip_resolved_outline_provenance, write_conflict_markers_for_ranges,
};
use super::*;
use crate::kit::text_model::TextModelSnapshot;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use std::time::Instant;
use worktree_core::mergetool_trace::{
    self, MergetoolTraceEvent, MergetoolTraceSideStats, MergetoolTraceStage,
};

fn line_ranges_intersect(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

fn shift_resolved_output_marker(
    marker: ResolvedOutputConflictMarker,
    line_delta: isize,
) -> ResolvedOutputConflictMarker {
    ResolvedOutputConflictMarker {
        conflict_ix: marker.conflict_ix,
        range_start: shifted_line_index(marker.range_start, line_delta),
        range_end: shifted_line_index(marker.range_end, line_delta),
        is_start: marker.is_start,
        is_end: marker.is_end,
        unresolved: marker.unresolved,
    }
}

fn record_resolved_outline_trace(
    path: Option<&std::path::PathBuf>,
    started: Instant,
    pane: &MainPaneView,
    output_line_count: usize,
) {
    let path = path.cloned();
    let elapsed = started.elapsed();
    let (diff_row_count, inline_row_count) = pane.conflict_resolver.two_way_row_counts();
    mergetool_trace::record_with(|| {
        MergetoolTraceEvent::new(MergetoolTraceStage::ResolvedOutlineRecompute, path, elapsed)
            .with_base(MergetoolTraceSideStats::from_text(Some(
                pane.conflict_resolver.three_way_text.base.as_ref(),
            )))
            .with_ours(MergetoolTraceSideStats::from_text(Some(
                pane.conflict_resolver.three_way_text.ours.as_ref(),
            )))
            .with_theirs(MergetoolTraceSideStats::from_text(Some(
                pane.conflict_resolver.three_way_text.theirs.as_ref(),
            )))
            .with_conflict_block_count(Some(conflict_resolver::conflict_count(
                &pane.conflict_resolver.marker_segments,
            )))
            .with_diff_row_count(Some(diff_row_count))
            .with_inline_row_count(Some(inline_row_count))
            .with_resolved_output_line_count(Some(output_line_count))
    });
}

struct ResolvedOutlineComputation {
    output_line_count: usize,
    outline: ResolvedOutlineData,
}

enum ResolvedOutlineSourceView<'a> {
    ThreeWay {
        base_text: &'a str,
        base_line_starts: &'a [usize],
        ours_text: &'a str,
        ours_line_starts: &'a [usize],
        theirs_text: &'a str,
        theirs_line_starts: &'a [usize],
    },
    TwoWay {
        ours_text: &'a str,
        ours_line_starts: &'a [usize],
        theirs_text: &'a str,
        theirs_line_starts: &'a [usize],
    },
}

impl ResolvedOutlineSourceView<'_> {
    fn view_mode(&self) -> ConflictResolverViewMode {
        match self {
            Self::ThreeWay { .. } => ConflictResolverViewMode::ThreeWay,
            Self::TwoWay { .. } => ConflictResolverViewMode::TwoWayDiff,
        }
    }
}

#[derive(Clone)]
enum OwnedResolvedOutlineSourceData {
    ThreeWay {
        base_text: Arc<str>,
        base_line_starts: Arc<[usize]>,
        ours_text: Arc<str>,
        ours_line_starts: Arc<[usize]>,
        theirs_text: Arc<str>,
        theirs_line_starts: Arc<[usize]>,
    },
    TwoWay {
        ours_text: Arc<str>,
        ours_line_starts: Arc<[usize]>,
        theirs_text: Arc<str>,
        theirs_line_starts: Arc<[usize]>,
    },
}

impl OwnedResolvedOutlineSourceData {
    fn as_view(&self) -> ResolvedOutlineSourceView<'_> {
        match self {
            Self::ThreeWay {
                base_text,
                base_line_starts,
                ours_text,
                ours_line_starts,
                theirs_text,
                theirs_line_starts,
            } => ResolvedOutlineSourceView::ThreeWay {
                base_text,
                base_line_starts,
                ours_text,
                ours_line_starts,
                theirs_text,
                theirs_line_starts,
            },
            Self::TwoWay {
                ours_text,
                ours_line_starts,
                theirs_text,
                theirs_line_starts,
            } => ResolvedOutlineSourceView::TwoWay {
                ours_text,
                ours_line_starts,
                theirs_text,
                theirs_line_starts,
            },
        }
    }
}

#[derive(Clone)]
struct BackgroundResolvedOutlineRecomputeRequest {
    output_text: Arc<str>,
    output_line_count: usize,
    marker_segments: Vec<conflict_resolver::ConflictSegment>,
    block_map: conflict_resolver::ResolvedOutputBlockMap,
    sources: OwnedResolvedOutlineSourceData,
}

struct ResolvedOutlineIncrementalBase<'a> {
    text: &'a TextModelSnapshot,
    line_starts: &'a Arc<[usize]>,
    marker_segments: &'a [conflict_resolver::ConflictSegment],
    view_mode: ConflictResolverViewMode,
}

fn compute_resolved_outline_computation(
    output_text: &str,
    output_line_count: usize,
    marker_segments: &[conflict_resolver::ConflictSegment],
    block_map: &conflict_resolver::ResolvedOutputBlockMap,
    sources: ResolvedOutlineSourceView<'_>,
) -> ResolvedOutlineComputation {
    let view_mode = sources.view_mode();
    let markers = build_resolved_output_conflict_markers(
        marker_segments,
        output_text,
        output_line_count,
        block_map,
    );
    if should_skip_resolved_outline_provenance(view_mode, output_line_count) {
        return ResolvedOutlineComputation {
            output_line_count,
            outline: ResolvedOutlineData {
                meta: Vec::new(),
                markers,
                sources_index: FxHashSet::default(),
            },
        };
    }

    let mut meta = match sources {
        ResolvedOutlineSourceView::ThreeWay {
            base_text,
            base_line_starts,
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        } => conflict_resolver::compute_resolved_line_provenance_from_text_with_indexed_sources(
            output_text,
            base_text,
            base_line_starts,
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        ),
        ResolvedOutlineSourceView::TwoWay {
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        } => conflict_resolver::compute_resolved_line_provenance_from_text_two_way_indexed_sources(
            output_text,
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        ),
    };
    apply_conflict_choice_provenance_hints(&mut meta, marker_segments, output_text, view_mode);
    let sources_index = conflict_resolver::build_resolved_output_line_sources_index_from_text(
        &meta,
        output_text,
        view_mode,
    );

    ResolvedOutlineComputation {
        output_line_count,
        outline: ResolvedOutlineData {
            meta,
            markers,
            sources_index,
        },
    }
}

fn compute_resolved_outline_computation_from_projection(
    projection: &conflict_resolver::ResolvedOutputProjection,
    marker_segments: &[conflict_resolver::ConflictSegment],
    view_mode: ConflictResolverViewMode,
    sources: Option<ResolvedOutlineSourceView<'_>>,
) -> ResolvedOutlineComputation {
    let output_line_count = projection.len();
    let block_ranges = projection.conflict_line_ranges();
    let markers = build_resolved_output_conflict_markers_from_block_ranges(
        marker_segments,
        block_ranges,
        output_line_count,
    );
    if should_skip_resolved_outline_provenance(view_mode, output_line_count) {
        return ResolvedOutlineComputation {
            output_line_count,
            outline: ResolvedOutlineData {
                meta: Vec::new(),
                markers,
                sources_index: FxHashSet::default(),
            },
        };
    }

    let Some(sources) = sources else {
        return ResolvedOutlineComputation {
            output_line_count,
            outline: ResolvedOutlineData {
                meta: Vec::new(),
                markers,
                sources_index: FxHashSet::default(),
            },
        };
    };
    let mut source_lookup: FxHashMap<&str, (conflict_resolver::ResolvedLineSource, Option<u32>)> =
        FxHashMap::default();
    match sources {
        ResolvedOutlineSourceView::ThreeWay {
            base_text,
            base_line_starts,
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        } => {
            insert_lookup_from_indexed_text(
                &mut source_lookup,
                conflict_resolver::ResolvedLineSource::C,
                theirs_text,
                theirs_line_starts,
            );
            insert_lookup_from_indexed_text(
                &mut source_lookup,
                conflict_resolver::ResolvedLineSource::B,
                ours_text,
                ours_line_starts,
            );
            insert_lookup_from_indexed_text(
                &mut source_lookup,
                conflict_resolver::ResolvedLineSource::A,
                base_text,
                base_line_starts,
            );
        }
        ResolvedOutlineSourceView::TwoWay {
            ours_text,
            ours_line_starts,
            theirs_text,
            theirs_line_starts,
        } => {
            insert_lookup_from_indexed_text(
                &mut source_lookup,
                conflict_resolver::ResolvedLineSource::B,
                theirs_text,
                theirs_line_starts,
            );
            insert_lookup_from_indexed_text(
                &mut source_lookup,
                conflict_resolver::ResolvedLineSource::A,
                ours_text,
                ours_line_starts,
            );
        }
    }

    let mut meta = Vec::with_capacity(output_line_count);
    for line_ix in 0..output_line_count {
        let line = projection
            .line_text(marker_segments, line_ix)
            .unwrap_or(std::borrow::Cow::Borrowed(""));
        let (source, input_line) = source_lookup
            .get(line.as_ref())
            .copied()
            .unwrap_or((conflict_resolver::ResolvedLineSource::Manual, None));
        meta.push(conflict_resolver::ResolvedLineMeta {
            output_line: u32::try_from(line_ix).unwrap_or(u32::MAX),
            source,
            input_line,
        });
    }
    apply_conflict_choice_provenance_hints_for_ranges(
        &mut meta,
        marker_segments,
        block_ranges,
        view_mode,
    );

    let mut sources_index = FxHashSet::default();
    sources_index.reserve(meta.len());
    for (line_ix, line_meta) in meta.iter().enumerate() {
        if line_meta.source == conflict_resolver::ResolvedLineSource::Manual {
            continue;
        }
        let Some(line_no) = line_meta.input_line else {
            continue;
        };
        let Some(line) = projection.line_text(marker_segments, line_ix) else {
            continue;
        };
        sources_index.insert(conflict_resolver::SourceLineKey::new(
            view_mode,
            line_meta.source,
            line_no,
            line.as_ref(),
        ));
    }

    ResolvedOutlineComputation {
        output_line_count,
        outline: ResolvedOutlineData {
            meta,
            markers,
            sources_index,
        },
    }
}

fn insert_lookup_from_indexed_text<'a>(
    lookup: &mut FxHashMap<&'a str, (conflict_resolver::ResolvedLineSource, Option<u32>)>,
    source: conflict_resolver::ResolvedLineSource,
    text: &'a str,
    line_starts: &[usize],
) {
    let line_count = indexed_line_count(text, line_starts);
    for line_ix in (0..line_count).rev() {
        let line = rows::resolved_output_line_text(text, line_starts, line_ix);
        lookup.insert(
            line,
            (
                source,
                Some(u32::try_from(line_ix.saturating_add(1)).unwrap_or(u32::MAX)),
            ),
        );
    }
}

fn update_line_sources_index_for_range(
    index: &mut FxHashSet<conflict_resolver::SourceLineKey>,
    view_mode: ConflictResolverViewMode,
    meta: &[conflict_resolver::ResolvedLineMeta],
    text: &str,
    line_starts: &[usize],
    line_range: Range<usize>,
    insert: bool,
) {
    if line_range.start >= line_range.end {
        return;
    }
    for line_ix in line_range {
        let Some(line_meta) = meta.get(line_ix) else {
            break;
        };
        if line_meta.source == conflict_resolver::ResolvedLineSource::Manual {
            continue;
        }
        let Some(line_no) = line_meta.input_line else {
            continue;
        };
        let key = conflict_resolver::SourceLineKey::new(
            view_mode,
            line_meta.source,
            line_no,
            rows::resolved_output_line_text(text, line_starts, line_ix),
        );
        if insert {
            index.insert(key);
        } else {
            index.remove(&key);
        }
    }
}

/// The row the resolved-output column measures its width against.
///
/// O(1): the rope carries the widest row in its summary, so the measurement
/// never scans the document. Ties keep the earliest row, matching the linear
/// scan this replaced.
pub(super) fn resolved_output_measure_row(snapshot: &TextModelSnapshot) -> usize {
    snapshot.rope().longest_row() as usize
}

impl MainPaneView {
    pub(in crate::view) fn conflict_resolved_output_is_streamed(&self) -> bool {
        self.conflict_resolved_output_projection.is_some()
    }

    pub(in crate::view) fn rebuild_conflict_resolved_output_block_map(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolver.output_is_protected {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
            return;
        }
        let map = conflict_resolver::ResolvedOutputBlockMap::from_segments(
            &self.conflict_resolver.marker_segments,
        );
        if self.conflict_resolved_output_is_streamed()
            || self.conflict_resolver_input.read_with(cx, |input, _| {
                map.is_valid_for(&self.conflict_resolver.marker_segments, input.text())
            })
        {
            self.conflict_resolved_output_block_map = map;
        } else {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
        }
    }

    pub(in crate::view::panes::main) fn apply_conflict_resolved_output_edit_deltas(
        &mut self,
        edit_deltas: Vec<(Range<usize>, Range<usize>)>,
        output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    ) {
        if edit_deltas.is_empty() {
            return;
        }
        if !self
            .conflict_resolved_output_block_map
            .apply_edit_deltas(edit_deltas)
            || !self
                .conflict_resolved_output_block_map
                .is_valid_for(&self.conflict_resolver.marker_segments, output_text)
        {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
        }
    }

    pub(in crate::view) fn conflict_resolved_output_is_modified(&self) -> bool {
        self.conflict_resolved_output_modified
    }

    pub(in crate::view) fn mark_conflict_resolved_output_saved(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.conflict_resolved_output_saved_snapshot =
            (!self.conflict_resolved_output_is_streamed()).then(|| {
                self.conflict_resolver_input
                    .read_with(cx, |input, _| input.text_snapshot())
            });
        self.conflict_resolved_output_modified = false;
    }

    fn sync_conflict_resolved_preview_projection(
        &mut self,
        projection: conflict_resolver::ResolvedOutputProjection,
        path: Option<&std::path::PathBuf>,
    ) {
        self.conflict_resolved_output_block_map =
            conflict_resolver::ResolvedOutputBlockMap::from_segments(
                &self.conflict_resolver.marker_segments,
            );
        self.conflict_resolved_output_projection = Some(projection.clone());
        self.conflict_resolved_preview_path = path.cloned();
        self.conflict_resolved_preview_source_revision = None;
        self.conflict_resolved_preview_text = TextModelSnapshot::default();
        self.conflict_resolved_preview_syntax_language =
            path.and_then(rows::diff_syntax_language_for_path);
        self.conflict_resolved_preview_line_count = projection.len();
        self.conflict_resolved_preview_line_starts = Arc::default();
        self.conflict_resolved_output_measure_row = projection.widest_line_ix();
        self.conflict_resolved_outline_stash = None;
        self.conflict_resolver.resolved_output_visible_dirty = true;
    }

    pub(in crate::view) fn refresh_streamed_resolved_output_preview_from_projection(
        &mut self,
        projection: conflict_resolver::ResolvedOutputProjection,
        path: Option<&std::path::PathBuf>,
    ) {
        let trace_started = Instant::now();
        let output_line_count = projection.len();
        let view_mode = self.conflict_resolver.view_mode;
        let computed = compute_resolved_outline_computation_from_projection(
            &projection,
            &self.conflict_resolver.marker_segments,
            view_mode,
            (!should_skip_resolved_outline_provenance(view_mode, output_line_count))
                .then(|| self.resolved_outline_source_view()),
        );
        self.sync_conflict_resolved_preview_projection(projection, path);
        self.apply_resolved_outline_computation(path, trace_started, computed);
    }

    pub(in crate::view) fn refresh_streamed_resolved_output_preview_from_markers(
        &mut self,
        path: Option<&std::path::PathBuf>,
    ) {
        let projection = conflict_resolver::ResolvedOutputProjection::from_segments(
            &self.conflict_resolver.marker_segments,
        );
        self.refresh_streamed_resolved_output_preview_from_projection(projection, path);
    }

    pub(in crate::view) fn ensure_conflict_resolved_output_materialized(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.conflict_resolved_output_is_streamed() {
            return;
        }

        // No size ceiling here. The output pane is editable by definition, and a
        // read-only fallback above some line count is a worse answer than a
        // slower one: the user opened a merge to resolve it. The buffer is
        // rope-backed and every hot path (syntax refresh, unresolved-row scan,
        // shaping) reads windows rather than the whole document, so cost scales
        // with the visible region plus the conflict count, not the file.
        let resolved =
            conflict_resolver::generate_resolved_text(&self.conflict_resolver.marker_segments);
        let path = self.conflict_resolver.path.clone();
        self.conflict_resolved_output_projection = None;
        self.conflict_resolved_preview_path = path.clone();
        self.fill_conflict_resolved_output_buffer(resolved, cx);
        self.conflict_resolved_preview_source_revision =
            Some(self.conflict_resolver_input.read_with(cx, |input, _| {
                ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
            }));
        self.rebuild_conflict_resolved_output_block_map(cx);
        self.recompute_conflict_resolved_outline_and_provenance(path.as_ref(), cx);
    }

    /// Load merged text into the resolved-output editor.
    ///
    /// Every path that fills this buffer goes through here, because filling it
    /// has one non-obvious obligation: `set_text` leaves the caret at
    /// end-of-document, and the pane opens scrolled to the top, so a caret
    /// parked at the far end sends the first arrow key (or undo) autoscrolling
    /// to the bottom of a file the user was reading from the top. Park it where
    /// the view actually is; the user has not placed a caret yet.
    pub(in crate::view) fn fill_conflict_resolved_output_buffer(
        &mut self,
        text: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        let text = text.into();
        let line_ending = crate::kit::TextInput::detect_line_ending(text.as_ref());
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_line_ending(line_ending);
            input.set_text(text, cx);
            input.set_selected_range(0..0, false, cx);
        });
    }

    /// Configure the resolved-output `TextInput` for rendering as the editable
    /// output pane. This is called from the render path, so it must stay cheap
    /// and side-effect free: the merged text is materialized into the buffer at
    /// bootstrap (see [`ensure_conflict_resolved_output_materialized`]), not here.
    /// It only points the editor at its shared scroll handle so the line-number
    /// gutter and the column scroll-sync group stay coupled to it.
    pub(in crate::view) fn prepare_conflict_resolved_output_editor(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.sync_conflict_resolved_output_active_conflict_highlight(cx);
        let scroll = self.conflict_resolved_output_editor_scroll.clone();
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_read_only(false, cx);
            input.set_vertical_scroll_handle(Some(scroll));
            // Lay the editor out at content width so its `overflow_scroll`
            // container carries a horizontal `max_offset` on the shared handle,
            // letting the resolved output scroll-sync with the source columns
            // on the horizontal axis too.
            input.set_content_width_layout(true);
        });
    }

    /// Unresolved rows for `snapshot`, from the cache when it is still current.
    ///
    /// A miss only happens when navigation runs before any refresh has scanned
    /// this revision; the scan is then done once and cached like any other.
    fn conflict_resolved_output_unresolved_rows_for(
        &mut self,
        snapshot: &TextModelSnapshot,
    ) -> UnresolvedRows {
        let key = ResolvedOutputKey::new(
            snapshot,
            &self.conflict_resolver.marker_segments,
            &self.conflict_resolved_output_block_map,
        );
        if let Some((cached_for, rows)) = self.conflict_resolved_output_unresolved_rows.as_ref()
            && *cached_for == key
        {
            return Arc::clone(rows);
        }

        #[cfg(test)]
        {
            self.conflict_resolved_output_full_scans += 1;
        }
        let rows = resolved_output_unresolved_rows(
            &self.conflict_resolver.marker_segments,
            &snapshot.rope(),
            &self.conflict_resolved_output_block_map,
        );
        self.conflict_resolved_output_unresolved_rows = Some((key, Arc::clone(&rows)));
        rows
    }

    /// Rebuild the output highlights when conflict navigation lands on another
    /// conflict, so the yellow wash follows the selection.
    ///
    /// Every other refresh path hangs off the text or the tree, and navigation
    /// moves neither — it only reassigns `active_conflict`, from a dozen call
    /// sites on a state struct that cannot reach the input. Comparing against the
    /// conflict the installed provider was built for catches all of them in one
    /// place, and makes the common render (nothing moved) a single comparison.
    fn sync_conflict_resolved_output_active_conflict_highlight(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_highlighted_conflict
            == self.conflict_resolver.active_conflict
        {
            return;
        }
        self.conflict_resolved_output_highlighted_conflict = self.conflict_resolver.active_conflict;
        // Streamed output is drawn row by row from the projection, which reads
        // the active conflict as it renders; only the editable buffer carries
        // highlights that have to be reinstalled.
        if self.conflict_resolved_output_is_streamed() {
            return;
        }
        // With a tree in hand, rebinding the provider is the whole job:
        // navigation moves no text, so the tree, the placeholder mask and the
        // protected spans all still stand. Going through the full syntax refresh
        // would redo them on every jump — and on the tree-less arm it would
        // re-tokenize the entire document, which is exactly the kind of
        // per-keypress cost the live engine exists to avoid.
        if self.conflict_resolved_output_live_syntax.is_some() {
            self.rebind_conflict_resolved_output_highlight_provider(cx);
            return;
        }
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        self.refresh_conflict_resolved_output_syntax(&output_snapshot, None, cx);
    }

    pub(in crate::view) fn current_conflict_resolved_output_text(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        if self.conflict_resolved_output_is_streamed() {
            conflict_resolver::generate_resolved_text(&self.conflict_resolver.marker_segments)
        } else {
            self.conflict_resolver_input
                .read_with(cx, |input, _| input.text().to_string())
        }
    }

    pub(in crate::view) fn conflict_resolver_save_contents_from_text(
        &mut self,
        text: String,
    ) -> String {
        self.conflict_resolver_sync_session_resolutions_from_output(&text);
        text
    }

    pub(in crate::view) fn ensure_prepared_syntax_chunk_poll(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.syntax_chunk_poll_task.is_some() {
            return;
        }

        if !crate::ui_runtime::current().uses_background_compute() {
            while self.apply_prepared_syntax_chunk_updates(cx) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            self.syntax_chunk_poll_task = None;
            return;
        }

        let task = cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| loop {
                let should_continue = view
                    .update(cx, |this, cx| this.apply_prepared_syntax_chunk_updates(cx))
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }

                smol::Timer::after(std::time::Duration::from_millis(16)).await;
            },
        );
        self.syntax_chunk_poll_task = Some(task);
    }

    fn apply_prepared_syntax_chunk_updates(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let mut applied = false;

        let split_left_applied = self
            .file_diff_split_prepared_syntax_document(DiffTextRegion::SplitLeft)
            .map(rows::drain_completed_prepared_diff_syntax_chunk_builds_for_document)
            .unwrap_or(0);
        if split_left_applied > 0 {
            self.file_diff_style_cache_epochs.bump_left();
            applied = true;
        }

        let split_right_applied = self
            .file_diff_split_prepared_syntax_document(DiffTextRegion::SplitRight)
            .map(rows::drain_completed_prepared_diff_syntax_chunk_builds_for_document)
            .unwrap_or(0);
        if split_right_applied > 0 {
            self.file_diff_style_cache_epochs.bump_right();
            applied = true;
        }

        let worktree_preview_applied = self
            .worktree_preview_prepared_syntax_document()
            .map(rows::drain_completed_prepared_diff_syntax_chunk_builds_for_document)
            .unwrap_or(0);
        if worktree_preview_applied > 0 {
            self.worktree_preview_style_cache_epoch =
                self.worktree_preview_style_cache_epoch.wrapping_add(1);
            applied = true;
        }

        if rows::drain_completed_prepared_diff_syntax_chunk_builds() > 0 {
            applied = true;
        }

        if applied {
            cx.notify();
        }

        let pending = rows::has_pending_prepared_diff_syntax_chunk_builds();
        if !pending {
            self.syntax_chunk_poll_task = None;
        }
        pending
    }

    /// Build the first tree off-thread after the foreground budget ran out.
    ///
    /// Guarded on the revision it is building for, so a burst of refreshes over
    /// the same text schedules one parse rather than one per call. A result for
    /// text the buffer has since moved past is never installed — it is re-issued
    /// against the current text instead, so the pane cannot be left on the
    /// heuristic fallback by an edit that raced the parse.
    fn ensure_conflict_resolved_output_live_syntax_build(
        &mut self,
        language: rows::DiffSyntaxLanguage,
        rope: crate::kit::rope::Rope,
        mask: Arc<[Range<usize>]>,
        revision: ResolvedOutputSourceRevision,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_live_syntax_building == Some(revision) {
            return;
        }
        self.conflict_resolved_output_live_syntax_building = Some(revision);

        let build_mask = Arc::clone(&mask);
        self.conflict_resolved_output_live_syntax_build =
            Some(cx.spawn(async move |view: WeakEntity<MainPaneView>, cx| {
                let build = move || rows::LiveSyntaxDocument::new(language, rope, build_mask, None);
                let built = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(build).await
                } else {
                    build()
                };
                let _ = view.update(cx, |this, cx| {
                    if this.conflict_resolved_output_live_syntax_building != Some(revision) {
                        // A newer generation owns the guard. Leave it alone --
                        // clearing it here would let its own scheduling check
                        // pass again and start a duplicate build.
                        return;
                    }
                    this.conflict_resolved_output_live_syntax_building = None;
                    let Some(document) = built else {
                        // Unbudgeted, so this is not a timeout: the text is past
                        // the size ceiling or the language has no wired grammar.
                        // Both are permanent, so re-issuing would spin. The
                        // heuristic arm of the refresh is the right answer here,
                        // and it is the same one the diff panes take.
                        return;
                    };
                    // Zed's `parse_again` (`Buffer::reparse`): a result for text
                    // the buffer has moved past is useless, but so is waiting --
                    // nothing else is guaranteed to come along and ask again, so
                    // re-issue from where the buffer is now.
                    let still_current = this.conflict_resolver_input.read_with(cx, |input, _| {
                        ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
                    }) == revision;
                    if !still_current {
                        this.reissue_conflict_resolved_output_live_syntax_build(cx);
                        return;
                    }
                    this.conflict_resolved_output_live_syntax = Some(document);
                    // Record what it was built for, or the next refresh sees a
                    // stale source, retries in the foreground, fails the budget
                    // again and schedules another build -- forever.
                    this.conflict_resolved_output_live_syntax_source = Some((revision, mask));
                    this.rebind_conflict_resolved_output_highlight_provider(cx);
                });
            }));
    }

    /// Re-run the off-thread first parse against the buffer as it stands now.
    ///
    /// Called when a build lands for text the buffer has already moved past.
    /// Recomputes the source the way [`Self::refresh_conflict_resolved_output_syntax`]
    /// does, so the two cannot disagree about what the tree is being built over.
    /// A no-op once a document exists -- from there on, edits go through
    /// `sync`, which always has a tree to fall back on.
    fn reissue_conflict_resolved_output_live_syntax_build(&mut self, cx: &mut gpui::Context<Self>) {
        if self.conflict_resolved_output_live_syntax.is_some() {
            return;
        }
        let Some(language) = self.conflict_resolved_preview_syntax_language else {
            return;
        };
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        let rope = output_snapshot.rope();
        let protected_ranges = resolved_output_placeholder_protected_ranges(&rope);
        let mask = resolved_output_live_syntax_mask(protected_ranges.as_ref(), &rope);
        let revision = ResolvedOutputSourceRevision::from_snapshot(&output_snapshot);
        self.ensure_conflict_resolved_output_live_syntax_build(
            language,
            output_snapshot.rope(),
            mask,
            revision,
            cx,
        );
    }

    /// Finish a reparse the foreground budget could not.
    ///
    /// Only reachable when an edit landed on a document too large to reparse in
    /// the budget. The viewport is not blocked meanwhile: the `tree.edit()`ed
    /// tree is already positionally correct, so it keeps painting — this just
    /// restores exactness near the edit.
    fn ensure_conflict_resolved_output_live_syntax_reparse(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(request) = self
            .conflict_resolved_output_live_syntax
            .as_ref()
            .and_then(rows::LiveSyntaxDocument::background_reparse_request)
        else {
            self.conflict_resolved_output_live_syntax_reparse = None;
            return;
        };

        self.conflict_resolved_output_live_syntax_reparse =
            Some(cx.spawn(async move |view: WeakEntity<MainPaneView>, cx| {
                let reparse = move || rows::live_syntax_reparse(request);
                let parsed = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(reparse).await
                } else {
                    reparse()
                };
                let Some((version, tree, injections)) = parsed else {
                    return;
                };
                let _ = view.update(cx, |this, cx| {
                    let adopted = this
                        .conflict_resolved_output_live_syntax
                        .as_mut()
                        .is_some_and(|document| {
                            document.adopt_background_tree(version, tree, injections)
                        });
                    if !adopted {
                        // The buffer moved while this was in flight, so the tree
                        // describes text that no longer exists. Re-issue from
                        // wherever the document is now.
                        this.conflict_resolved_output_live_syntax_reparse = None;
                        this.ensure_conflict_resolved_output_live_syntax_reparse(cx);
                        return;
                    }
                    this.conflict_resolved_output_live_syntax_reparse = None;
                    this.rebind_conflict_resolved_output_highlight_provider(cx);
                });
            }));
    }

    /// Hand the input a provider over the document's current tree.
    fn rebind_conflict_resolved_output_highlight_provider(&mut self, cx: &mut gpui::Context<Self>) {
        let Some((version, snapshot)) = self
            .conflict_resolved_output_live_syntax
            .as_ref()
            .map(|document| (document.version(), document.snapshot(self.theme)))
        else {
            return;
        };
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        self.conflict_resolved_output_highlighted_conflict = self.conflict_resolver.active_conflict;
        // Reuse the scan from the last refresh when the text has not moved.
        // Rebinding happens on every conflict jump, and rescanning here is what
        // made navigation scale with the file rather than with the conflict.
        let rows = self.conflict_resolved_output_unresolved_rows_for(&output_snapshot);
        let unresolved_spans = resolved_output_unresolved_spans_for_active(
            rows.as_ref(),
            self.conflict_resolver.active_conflict,
        );
        let binding_key = resolved_output_live_provider_binding_key(
            version,
            self.conflict_resolved_output_provider_theme_epoch,
            &unresolved_spans,
        );
        let provider =
            resolved_output_live_highlight_provider(self.theme, snapshot, unresolved_spans);
        let source_len = output_snapshot.len();
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_highlight_provider_with_key(binding_key, provider, source_len, cx);
        });
    }

    /// Bring the resolved output's live tree up to date with `output_snapshot`
    /// and rebind the highlight provider to it.
    ///
    /// `edit` is the coalesced `(replaced, inserted)` span, or `None` when the
    /// text was replaced wholesale (bootstrap, a conflict resolution, an undo of
    /// one) — which reparses from scratch.
    ///
    /// Cheap enough to run on the keystroke: the tree is edited in place and the
    /// reparse reuses it, rather than rebuilding the prepared document this
    /// replaced.
    ///
    /// The root parse is incremental; the *injected* layers are not. A reparse
    /// re-runs the injection query over the whole document and reparses every
    /// injected region from scratch, each with its own copy of the foreground
    /// budget. On a document with many injections (fenced blocks, `<script>`
    /// bodies) that is the dominant per-keystroke cost and does scale with the
    /// document — the outstanding gap against Zed's `SyntaxMap`, which keys
    /// layers by (language, range) and reparses them incrementally.
    pub(super) fn refresh_conflict_resolved_output_syntax(
        &mut self,
        output_snapshot: &TextModelSnapshot,
        edit: Option<(Range<usize>, Range<usize>)>,
        cx: &mut gpui::Context<Self>,
    ) {
        // Everything below is derived from the *text*. When the text has not
        // moved, all of it still stands and the only thing that can need
        // updating is the provider binding — a theme change, or a different
        // conflict wearing the wash.
        //
        // Only when the caller reports no edit. An `edit` is the caller stating
        // that the text moved, and the tree must be folded forward even if the
        // revision happens to look settled — skipping the sync there leaves the
        // tree describing the pre-edit text, which shows up as the row you just
        // typed into keeping its old colours.
        let revision = ResolvedOutputSourceRevision::from_snapshot(output_snapshot);
        let text_is_unchanged = self
            .conflict_resolved_output_live_syntax_source
            .as_ref()
            .is_some_and(|(built_for, _)| *built_for == revision);
        // The language has to match too, not just the text. The reuse check
        // that would drop a document built by the wrong grammar lives *below*
        // this return, so leaving it out lets a language change with unchanged
        // text keep the previous grammar's tree — the state
        // `conflict_resolver_invalidate_resolved_outline` leaves behind, which
        // clears the language but not the live document.
        let language_is_unchanged = self
            .conflict_resolved_output_live_syntax
            .as_ref()
            .is_some_and(|document| {
                Some(document.language()) == self.conflict_resolved_preview_syntax_language
            });
        if edit.is_none() && text_is_unchanged && language_is_unchanged {
            self.conflict_resolved_output_highlighted_conflict =
                self.conflict_resolver.active_conflict;
            self.rebind_conflict_resolved_output_highlight_provider(cx);
            return;
        }

        // Every read below goes through the rope, and nothing on this path
        // builds either whole-document cache — not the flattened string, and
        // not the line-start array, which is the quieter of the two and was the
        // one that lingered here. `no_materialization_tests` asserts both.
        let rope = output_snapshot.rope();
        self.conflict_resolved_output_highlighted_conflict = self.conflict_resolver.active_conflict;
        #[cfg(test)]
        {
            self.conflict_resolved_output_full_scans += 1;
        }
        let unresolved_rows = resolved_output_unresolved_rows(
            &self.conflict_resolver.marker_segments,
            &rope,
            &self.conflict_resolved_output_block_map,
        );
        self.conflict_resolved_output_unresolved_rows = Some((
            ResolvedOutputKey::new(
                output_snapshot,
                &self.conflict_resolver.marker_segments,
                &self.conflict_resolved_output_block_map,
            ),
            Arc::clone(&unresolved_rows),
        ));
        let unresolved_spans = resolved_output_unresolved_spans_for_active(
            unresolved_rows.as_ref(),
            self.conflict_resolver.active_conflict,
        );
        // The placeholder rows are a rendering of open decisions, so hand them
        // to the buffer as uneditable spans — and hide the same spans from the
        // parser, which would otherwise read `<Merge Conflict>` as code.
        let protected_ranges = resolved_output_placeholder_protected_ranges(&rope);
        let mask = resolved_output_live_syntax_mask(protected_ranges.as_ref(), &rope);
        let budget = Some(self.full_document_syntax_budget().foreground_parse);

        let language = self.conflict_resolved_preview_syntax_language;
        let reusable = self
            .conflict_resolved_output_live_syntax
            .as_ref()
            .is_some_and(|document| Some(document.language()) == language);
        if !reusable {
            self.conflict_resolved_output_live_syntax = None;
            self.conflict_resolved_output_live_syntax_source = None;
        }

        let revision = ResolvedOutputSourceRevision::from_snapshot(output_snapshot);
        let current = self
            .conflict_resolved_output_live_syntax_source
            .as_ref()
            .is_some_and(|(built_for, built_mask)| {
                *built_for == revision && built_mask.as_ref() == mask.as_ref()
            });

        match self.conflict_resolved_output_live_syntax.as_mut() {
            // Nothing about the buffer moved. The tree stands, and so does its
            // version — the binding key below folds in the theme and the
            // unresolved spans, so an overlay change still rebinds while a
            // no-op re-entry does not, which is what stops this method from
            // re-triggering the observe that called it.
            Some(_) if current => {}
            Some(document) => {
                let outcome = document.sync(rope.clone(), Arc::clone(&mask), edit, budget);
                if outcome == rows::LiveSyntaxSyncOutcome::Abandoned {
                    // The edit took the buffer past the size ceiling, so the
                    // document now describes text that no longer exists. Drop it
                    // and take the heuristic arm below, which is the same answer
                    // a buffer that started out this large would have got.
                    self.conflict_resolved_output_live_syntax = None;
                    self.conflict_resolved_output_live_syntax_source = None;
                } else {
                    self.conflict_resolved_output_live_syntax_source =
                        Some((revision, Arc::clone(&mask)));
                }
            }
            None => {
                // Zed's fast path (`Buffer::reparse` under `sync_parse_timeout`):
                // worth a budgeted attempt because a small buffer finishes inside
                // it and never shows a frame of unhighlighted text. Skipped when
                // a build for exactly this text is already off-thread -- that
                // attempt has demonstrably failed once, so re-running it on the
                // keystroke path is pure latency.
                let already_building =
                    self.conflict_resolved_output_live_syntax_building == Some(revision);
                self.conflict_resolved_output_live_syntax =
                    language.filter(|_| !already_building).and_then(|language| {
                        rows::LiveSyntaxDocument::new(
                            language,
                            rope.clone(),
                            Arc::clone(&mask),
                            budget,
                        )
                    });
                self.conflict_resolved_output_live_syntax_source = self
                    .conflict_resolved_output_live_syntax
                    .is_some()
                    .then(|| (revision, Arc::clone(&mask)));

                // A first parse has no tree to fall back on, so exhausting the
                // foreground budget leaves nothing at all -- and an incremental
                // reparse can never rescue it, because there is no document to
                // reparse. Finish it off-thread instead. Not a rare path: the
                // live budget is 1ms and a cold parse of a ~10KB file is
                // already over it, so without this the resolved output would
                // sit on heuristic tokens for the whole session.
                if let Some(language) =
                    language.filter(|_| self.conflict_resolved_output_live_syntax.is_none())
                {
                    self.ensure_conflict_resolved_output_live_syntax_build(
                        language,
                        rope.clone(),
                        Arc::clone(&mask),
                        revision,
                        cx,
                    );
                }
            }
        }

        let live = self
            .conflict_resolved_output_live_syntax
            .as_ref()
            .map(|document| (document.version(), document.snapshot(self.theme)));
        self.ensure_conflict_resolved_output_live_syntax_reparse(cx);

        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_protected_ranges(protected_ranges);
            match live {
                Some((version, snapshot)) => {
                    let provider = resolved_output_live_highlight_provider(
                        self.theme,
                        snapshot,
                        unresolved_spans.clone(),
                    );
                    // Rebinding under a fresh key whenever the text moved is
                    // load-bearing: it resets the interpolation that would
                    // otherwise map these already-current highlights through a
                    // stale patch.
                    let binding_key = resolved_output_live_provider_binding_key(
                        version,
                        self.conflict_resolved_output_provider_theme_epoch,
                        &unresolved_spans,
                    );
                    input.set_highlight_provider_with_key(binding_key, provider, rope.len(), cx);
                }
                None => {
                    // Heuristic tokens, with the open conflicts still called out
                    // in red. Reachable in exactly two states, both permanent and
                    // both shared with the diff panes above -- the language has
                    // no wired grammar, or the text is past
                    // `PREPARED_DIFF_SYNTAX_DOCUMENT_MAX_TEXT_BYTES`. It is *not*
                    // a general fallback: the tokenizer knows only keywords,
                    // strings, numbers and comments, so landing here while a
                    // grammar exists is precisely the bug where the output stops
                    // matching the panes above it. A budget-exhausted first parse
                    // must go to `ensure_conflict_resolved_output_live_syntax_build`
                    // instead.
                    //
                    // A provider rather than a whole-document `set_highlights`:
                    // this arm is reached by the *largest* buffers, and the
                    // tokenizer is line-local, so answering per window is both
                    // exact and proportional to the viewport.
                    let provider = resolved_output_heuristic_highlight_provider(
                        self.theme,
                        rope.clone(),
                        language,
                        unresolved_spans.clone(),
                    );
                    let binding_key = resolved_output_heuristic_provider_binding_key(
                        revision,
                        self.conflict_resolved_output_provider_theme_epoch,
                        &unresolved_spans,
                    );
                    input.set_highlight_provider_with_key(binding_key, provider, rope.len(), cx);
                }
            }
        });
    }

    /// Schedule a background tree-sitter parse for one merge-input side.
    ///
    /// When the parse completes, the prepared document is injected into the
    /// global cache and the three-way styled-text cache is cleared so the next
    /// render picks up document-based syntax highlighting.
    pub(in crate::view) fn ensure_conflict_three_way_background_syntax_prepare(
        &mut self,
        side: ThreeWayColumn,
        text: SharedString,
        line_starts: Arc<[usize]>,
        language: rows::DiffSyntaxLanguage,
        source_hash: Option<u64>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_three_way_syntax_inflight[side] {
            return;
        }
        self.conflict_three_way_syntax_inflight[side] = true;
        let expected_source_hash = source_hash;
        cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                let prepare_document = move || {
                    rows::prepare_diff_syntax_document_in_background_text_with_reuse(
                        language,
                        rows::DiffSyntaxMode::Auto,
                        text,
                        line_starts,
                        None,
                        None,
                    )
                };
                let parsed = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(prepare_document).await
                } else {
                    prepare_document()
                };

                let _ = view.update(cx, |this, cx| {
                    this.conflict_three_way_syntax_inflight[side] = false;

                    // Stale: source hash changed while we were parsing.
                    if this.conflict_resolver.source_hash != expected_source_hash {
                        return;
                    }

                    if let Some(parsed) = parsed {
                        let document =
                            rows::inject_background_prepared_diff_syntax_document(parsed);
                        this.conflict_three_way_prepared_syntax_documents[side] = Some(document);
                        // Invalidate cached styled text so the next render uses
                        // the prepared document across three-way and two-way
                        // conflict views instead of per-line fallback styling.
                        this.clear_conflict_diff_style_caches_preserving_query();
                        this.conflict_three_way_segments_cache.clear();
                        this.conflict_three_way_query_segments_cache.clear();
                        cx.notify();
                    }
                });
            },
        )
        .detach();
    }

    pub(in crate::view) fn clear_diff_text_query_overlay_cache(&mut self) {
        self.diff_text_query_segments_cache.clear();
        self.diff_text_query_cache_query = SharedString::default();
        self.diff_text_query_cache_options = Default::default();
        self.diff_text_query_cache_matcher = None;
        self.diff_text_query_cache_generation =
            self.diff_text_query_cache_generation.wrapping_add(1);
    }

    pub(in crate::view) fn invalidate_diff_text_query_overlay_cache(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        if self.diff_text_query_cache_query.as_ref() != query
            || self.diff_text_query_cache_options != options
        {
            self.diff_text_query_cache_query = query.to_string().into();
            self.diff_text_query_cache_options = options;
            self.diff_text_query_cache_matcher = (!query.is_empty())
                .then(|| super::diff_search::DiffSearchMatcher::new(query, options));
            self.diff_text_query_cache_generation =
                self.diff_text_query_cache_generation.wrapping_add(1);
        }
    }

    pub(in crate::view) fn sync_diff_text_query_overlay_cache(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        self.invalidate_diff_text_query_overlay_cache(query, options);
    }

    pub(in crate::view) fn clear_diff_text_style_caches(&mut self) {
        self.diff_text_segments_cache.clear();
        self.clear_diff_text_query_overlay_cache();
    }

    pub(in crate::view) fn clear_worktree_preview_segments_cache(&mut self) {
        self.worktree_preview_segments_cache.clear();
        self.worktree_preview_cache_write_blocked_until_rev = None;
    }

    pub(in crate::view) fn clear_conflict_diff_query_overlay_caches(&mut self) {
        self.conflict_diff_query_segments_cache_split.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.conflict_diff_query_cache_query = SharedString::default();
        self.conflict_diff_query_cache_options = Default::default();
    }

    pub(in crate::view) fn clear_conflict_diff_style_caches_preserving_query(&mut self) {
        self.conflict_diff_segments_cache_split.clear();
        self.conflict_diff_query_segments_cache_split.clear();
        self.conflict_three_way_query_segments_cache.clear();
    }

    pub(in crate::view) fn sync_conflict_diff_query_overlay_caches(
        &mut self,
        query: &str,
        options: super::diff_search::DiffSearchOptions,
    ) {
        if self.conflict_diff_query_cache_query.as_ref() != query
            || self.conflict_diff_query_cache_options != options
        {
            self.conflict_diff_query_cache_query = query.to_string().into();
            self.conflict_diff_query_cache_options = options;
            self.conflict_diff_query_segments_cache_split.clear();
            self.conflict_three_way_query_segments_cache.clear();
        }
    }

    pub(in crate::view) fn clear_conflict_diff_style_caches(&mut self) {
        self.clear_conflict_diff_style_caches_preserving_query();
        self.conflict_diff_query_cache_query = SharedString::default();
        self.conflict_diff_query_cache_options = Default::default();
    }

    pub(in crate::view::panes::main) fn conflict_resolver_invalidate_resolved_outline(&mut self) {
        self.conflict_resolver.resolver_pending_recompute_seq = self
            .conflict_resolver
            .resolver_pending_recompute_seq
            .wrapping_add(1);
        self.conflict_resolved_preview_path = None;
        self.conflict_resolved_preview_source_revision = None;
        self.conflict_resolved_output_projection = None;
        self.conflict_resolved_preview_text = TextModelSnapshot::default();
        self.conflict_resolved_preview_syntax_language = None;
        self.conflict_resolved_preview_line_count = 0;
        self.conflict_resolved_preview_line_starts = Arc::default();
        self.conflict_resolved_output_measure_row = 0;
        self.conflict_resolved_outline_stash = None;
        self.conflict_three_way_prepared_syntax_documents = ThreeWaySides::default();
        self.conflict_three_way_syntax_inflight = ThreeWaySides::default();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.conflict_resolver.resolved_outline = ResolvedOutlineData::default();
        self.conflict_resolver.resolved_output_visible_dirty = true;
    }

    fn resolved_outline_source_view(&self) -> ResolvedOutlineSourceView<'_> {
        match self.conflict_resolver.view_mode {
            ConflictResolverViewMode::ThreeWay => ResolvedOutlineSourceView::ThreeWay {
                base_text: &self.conflict_resolver.three_way_text.base,
                base_line_starts: self
                    .conflict_resolver
                    .three_way_line_starts_ref(ThreeWayColumn::Base),
                ours_text: &self.conflict_resolver.three_way_text.ours,
                ours_line_starts: self
                    .conflict_resolver
                    .three_way_line_starts_ref(ThreeWayColumn::Ours),
                theirs_text: &self.conflict_resolver.three_way_text.theirs,
                theirs_line_starts: self
                    .conflict_resolver
                    .three_way_line_starts_ref(ThreeWayColumn::Theirs),
            },
            ConflictResolverViewMode::TwoWayDiff => ResolvedOutlineSourceView::TwoWay {
                ours_text: &self.conflict_resolver.three_way_text.ours,
                ours_line_starts: self
                    .conflict_resolver
                    .three_way_line_starts_ref(ThreeWayColumn::Ours),
                theirs_text: &self.conflict_resolver.three_way_text.theirs,
                theirs_line_starts: self
                    .conflict_resolver
                    .three_way_line_starts_ref(ThreeWayColumn::Theirs),
            },
        }
    }

    /// Snapshot everything the outline recompute needs, so it can run detached.
    ///
    /// This materializes the output, and unlike the syntax path it is not an
    /// artifact worth removing: the outline assigns a
    /// provenance to *every* row by comparing its text against the three source
    /// sides, so the work is O(document) whatever it reads through, and the copy
    /// is a small constant beside it.
    ///
    /// What keeps that off the keystroke path is *where* it is called from, not
    /// its cost: both the production task and the synchronous test arm build the
    /// request only once the debounce
    /// (`CONFLICT_RESOLVED_OUTLINE_DEBOUNCE_MS`) has settled and the recompute
    /// is going to run. Hoisting this call above that check charges every
    /// keystroke for a copy of the document that is then discarded.
    fn background_resolved_outline_recompute_request(
        &self,
        output_snapshot: &TextModelSnapshot,
    ) -> BackgroundResolvedOutlineRecomputeRequest {
        let output_text: Arc<str> = output_snapshot.as_shared_string().into();
        let output_line_count = output_snapshot.shared_line_starts().len().max(1);
        let sources = match self.conflict_resolver.view_mode {
            ConflictResolverViewMode::ThreeWay => OwnedResolvedOutlineSourceData::ThreeWay {
                base_text: self.conflict_resolver.three_way_text.base.clone().into(),
                base_line_starts: self
                    .conflict_resolver
                    .three_way_shared_line_starts(ThreeWayColumn::Base),
                ours_text: self.conflict_resolver.three_way_text.ours.clone().into(),
                ours_line_starts: self
                    .conflict_resolver
                    .three_way_shared_line_starts(ThreeWayColumn::Ours),
                theirs_text: self.conflict_resolver.three_way_text.theirs.clone().into(),
                theirs_line_starts: self
                    .conflict_resolver
                    .three_way_shared_line_starts(ThreeWayColumn::Theirs),
            },
            ConflictResolverViewMode::TwoWayDiff => OwnedResolvedOutlineSourceData::TwoWay {
                ours_text: self.conflict_resolver.three_way_text.ours.clone().into(),
                ours_line_starts: self
                    .conflict_resolver
                    .three_way_shared_line_starts(ThreeWayColumn::Ours),
                theirs_text: self.conflict_resolver.three_way_text.theirs.clone().into(),
                theirs_line_starts: self
                    .conflict_resolver
                    .three_way_shared_line_starts(ThreeWayColumn::Theirs),
            },
        };

        BackgroundResolvedOutlineRecomputeRequest {
            output_text,
            output_line_count,
            marker_segments: self.conflict_resolver.marker_segments.clone(),
            block_map: self.conflict_resolved_output_block_map.clone(),
            sources,
        }
    }

    fn stash_current_conflict_resolved_outline_state(&mut self) {
        let line_count = self.conflict_resolved_preview_line_count;
        if line_count == 0
            || self.conflict_resolver.resolved_outline.meta.len() != line_count
            || self.conflict_resolver.resolved_outline.markers.len() != line_count
        {
            return;
        }

        self.conflict_resolved_outline_stash = Some(StashedResolvedOutlineState {
            text: self.conflict_resolved_preview_text.clone(),
            line_starts: self.conflict_resolved_preview_line_starts.clone(),
            marker_segments: self.conflict_resolver.marker_segments.clone(),
            view_mode: self.conflict_resolver.view_mode,
            outline: self.conflict_resolver.resolved_outline.clone(),
        });
    }

    fn resolved_outline_incremental_base(&self) -> Option<ResolvedOutlineIncrementalBase<'_>> {
        if self.conflict_resolved_output_is_streamed() {
            return None;
        }
        if let Some(stash) = self.conflict_resolved_outline_stash.as_ref() {
            return Some(ResolvedOutlineIncrementalBase {
                text: &stash.text,
                line_starts: &stash.line_starts,
                marker_segments: &stash.marker_segments,
                view_mode: stash.view_mode,
            });
        }

        let line_count = self.conflict_resolved_preview_line_count;
        if line_count == 0
            || self.conflict_resolver.resolved_outline.meta.len() != line_count
            || self.conflict_resolver.resolved_outline.markers.len() != line_count
        {
            return None;
        }

        Some(ResolvedOutlineIncrementalBase {
            text: &self.conflict_resolved_preview_text,
            line_starts: &self.conflict_resolved_preview_line_starts,
            marker_segments: &self.conflict_resolver.marker_segments,
            view_mode: self.conflict_resolver.view_mode,
        })
    }

    fn sync_conflict_resolved_preview_snapshot(
        &mut self,
        output_snapshot: &TextModelSnapshot,
        path: Option<&std::path::PathBuf>,
        clear_outline: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if clear_outline {
            self.stash_current_conflict_resolved_outline_state();
        }
        self.conflict_resolved_preview_source_revision =
            Some(ResolvedOutputSourceRevision::from_snapshot(output_snapshot));
        self.conflict_resolved_preview_line_starts = output_snapshot.shared_line_starts();
        self.conflict_resolved_preview_syntax_language =
            path.and_then(rows::diff_syntax_language_for_path);
        self.conflict_resolved_preview_line_count = output_snapshot.line_count().max(1);
        self.conflict_resolved_output_measure_row = resolved_output_measure_row(output_snapshot);
        // Syntax no longer *waits* on this debounce — it tracks the buffer on
        // the keystroke, in the `cx.observe` on `conflict_resolver_input`. The
        // call stays because this method is also how the language arrives
        // (from `path`) and how a wholesale text replacement lands, neither of
        // which produces edit deltas. It reparses only if the buffer actually
        // differs from what the tree already describes, so on the common path
        // it is a version bump and nothing more.
        self.refresh_conflict_resolved_output_syntax(output_snapshot, None, cx);
        self.conflict_resolved_preview_text = output_snapshot.clone();

        if clear_outline {
            self.conflict_resolver.resolved_outline = ResolvedOutlineData::default();
            self.conflict_resolver.resolved_output_visible_dirty = true;
            self.conflict_resolver.resolved_outline_gutter_rows.clear();
        }
    }

    fn apply_resolved_outline_computation(
        &mut self,
        path: Option<&std::path::PathBuf>,
        trace_started: Instant,
        computed: ResolvedOutlineComputation,
    ) {
        self.conflict_resolved_outline_stash = None;
        self.conflict_resolver.resolved_outline = computed.outline;
        self.conflict_resolver.resolved_output_visible_dirty = true;
        self.conflict_resolver.resolved_outline_gutter_rows.clear();
        record_resolved_outline_trace(path, trace_started, self, computed.output_line_count);
    }

    pub(in crate::view::panes::main) fn recompute_conflict_resolved_outline_and_provenance(
        &mut self,
        path: Option<&std::path::PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_is_streamed() {
            let _ = cx;
            self.refresh_streamed_resolved_output_preview_from_markers(path);
            return;
        }
        let _perf_scope = perf::span(ViewPerfSpan::RecomputeResolvedOutline);
        let trace_started = Instant::now();
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        let output_text = output_snapshot.as_ref();
        let output_line_count = output_snapshot.shared_line_starts().len().max(1);
        let computed = compute_resolved_outline_computation(
            output_text,
            output_line_count,
            &self.conflict_resolver.marker_segments,
            &self.conflict_resolved_output_block_map,
            self.resolved_outline_source_view(),
        );
        self.sync_conflict_resolved_preview_snapshot(&output_snapshot, path, false, cx);
        self.apply_resolved_outline_computation(path, trace_started, computed);
    }

    fn recompute_conflict_resolved_outline_and_provenance_incremental(
        &mut self,
        path: Option<&std::path::PathBuf>,
        delta: ResolvedOutlineDelta,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.conflict_resolved_output_is_streamed() {
            let _ = path;
            let _ = delta;
            let _ = cx;
            return false;
        }
        let Some(base) = self.resolved_outline_incremental_base() else {
            return false;
        };
        let old_text_snapshot = base.text.clone();
        let old_text = old_text_snapshot.as_ref();
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        let output_text = output_snapshot.as_ref();
        let old_line_starts = base.line_starts.clone();
        let old_line_count = old_line_starts.len().max(1);
        let new_line_starts = output_snapshot.shared_line_starts();
        let new_line_count = new_line_starts.len().max(1);
        if old_line_starts.is_empty() {
            return false;
        }
        let used_stash = self.conflict_resolved_outline_stash.is_some();
        let delta = if used_stash {
            resolved_outline_delta_between_texts(old_text, output_text)
        } else {
            Some(delta)
        };
        let Some(delta) = delta else {
            return false;
        };
        if delta.old_range.start > delta.old_range.end
            || delta.new_range.start > delta.new_range.end
            || delta.old_range.end > old_text.len()
            || delta.new_range.end > output_text.len()
        {
            return false;
        }

        let old_dirty_lines = dirty_byte_range_to_line_range(
            old_line_starts.as_ref(),
            old_text.len(),
            delta.old_range.clone(),
        );
        let new_dirty_lines = dirty_byte_range_to_line_range(
            new_line_starts.as_ref(),
            output_text.len(),
            delta.new_range.clone(),
        );
        let mut old_affected = old_dirty_lines.clone();
        let mut new_affected = new_dirty_lines.clone();
        old_affected.start = old_affected.start.saturating_sub(1);
        old_affected.end = old_affected.end.saturating_add(1).min(old_line_count);
        new_affected.start = new_affected.start.saturating_sub(1);
        new_affected.end = new_affected.end.saturating_add(1).min(new_line_count);

        let Some(old_block_ranges) =
            resolved_output_conflict_block_ranges_in_text(base.marker_segments, old_text)
        else {
            return false;
        };
        let new_block_ranges = match resolved_output_conflict_block_line_ranges(
            &self.conflict_resolver.marker_segments,
            output_text,
            &self.conflict_resolved_output_block_map,
        ) {
            Some(ranges) if ranges.len() == old_block_ranges.len() => ranges,
            _ => remap_resolved_output_conflict_block_ranges_for_delta(
                old_block_ranges.as_slice(),
                old_dirty_lines.clone(),
                new_dirty_lines.clone(),
                new_line_count,
            ),
        };
        if old_block_ranges.len() != new_block_ranges.len() {
            return false;
        }

        let mut touched_conflicts: FxHashSet<usize> = FxHashSet::default();
        for (conflict_ix, range) in old_block_ranges.iter().enumerate() {
            if line_ranges_intersect(range, &old_affected) {
                touched_conflicts.insert(conflict_ix);
            }
        }
        for (conflict_ix, range) in new_block_ranges.iter().enumerate() {
            if line_ranges_intersect(range, &new_affected) {
                touched_conflicts.insert(conflict_ix);
            }
        }
        for conflict_ix in &touched_conflicts {
            if let Some(old_range) = old_block_ranges.get(*conflict_ix) {
                old_affected.start = old_affected.start.min(old_range.start);
                old_affected.end = old_affected.end.max(old_range.end).min(old_line_count);
            }
            if let Some(new_range) = new_block_ranges.get(*conflict_ix) {
                new_affected.start = new_affected.start.min(new_range.start);
                new_affected.end = new_affected.end.max(new_range.end).min(new_line_count);
            }
        }

        let mut recompute_conflicts = Vec::new();
        for (conflict_ix, new_range) in new_block_ranges.iter().enumerate() {
            if line_ranges_intersect(new_range, &new_affected) {
                recompute_conflicts.push(conflict_ix);
                if let Some(old_range) = old_block_ranges.get(conflict_ix) {
                    old_affected.start = old_affected.start.min(old_range.start);
                    old_affected.end = old_affected.end.max(old_range.end).min(old_line_count);
                }
                new_affected.start = new_affected.start.min(new_range.start);
                new_affected.end = new_affected.end.max(new_range.end).min(new_line_count);
            }
        }
        if old_affected.start != new_affected.start {
            return false;
        }

        let old_view_mode = base.view_mode;
        let new_view_mode = self.conflict_resolver.view_mode;
        let middle_meta = {
            let mut source_lookup: FxHashMap<
                &str,
                (conflict_resolver::ResolvedLineSource, Option<u32>),
            > = FxHashMap::default();
            match new_view_mode {
                ConflictResolverViewMode::ThreeWay => {
                    insert_lookup_from_indexed_text(
                        &mut source_lookup,
                        conflict_resolver::ResolvedLineSource::C,
                        &self.conflict_resolver.three_way_text.theirs,
                        self.conflict_resolver
                            .three_way_line_starts_ref(ThreeWayColumn::Theirs),
                    );
                    insert_lookup_from_indexed_text(
                        &mut source_lookup,
                        conflict_resolver::ResolvedLineSource::B,
                        &self.conflict_resolver.three_way_text.ours,
                        self.conflict_resolver
                            .three_way_line_starts_ref(ThreeWayColumn::Ours),
                    );
                    insert_lookup_from_indexed_text(
                        &mut source_lookup,
                        conflict_resolver::ResolvedLineSource::A,
                        &self.conflict_resolver.three_way_text.base,
                        self.conflict_resolver
                            .three_way_line_starts_ref(ThreeWayColumn::Base),
                    );
                }
                ConflictResolverViewMode::TwoWayDiff => {
                    insert_lookup_from_indexed_text(
                        &mut source_lookup,
                        conflict_resolver::ResolvedLineSource::B,
                        &self.conflict_resolver.three_way_text.theirs,
                        self.conflict_resolver
                            .three_way_line_starts_ref(ThreeWayColumn::Theirs),
                    );
                    insert_lookup_from_indexed_text(
                        &mut source_lookup,
                        conflict_resolver::ResolvedLineSource::A,
                        &self.conflict_resolver.three_way_text.ours,
                        self.conflict_resolver
                            .three_way_line_starts_ref(ThreeWayColumn::Ours),
                    );
                }
            }

            let mut middle_meta = Vec::with_capacity(new_affected.len());
            for line_ix in new_affected.clone() {
                let output_line =
                    rows::resolved_output_line_text(output_text, new_line_starts.as_ref(), line_ix);
                let (mut source, mut input_line) = source_lookup
                    .get(output_line)
                    .copied()
                    .unwrap_or((conflict_resolver::ResolvedLineSource::Manual, None));
                if new_dirty_lines.contains(&line_ix) {
                    source = conflict_resolver::ResolvedLineSource::Manual;
                    input_line = None;
                }
                middle_meta.push(conflict_resolver::ResolvedLineMeta {
                    output_line: u32::try_from(line_ix).unwrap_or(u32::MAX),
                    source,
                    input_line,
                });
            }
            middle_meta
        };

        let old_outline = if used_stash {
            self.conflict_resolved_outline_stash
                .as_ref()
                .map(|stash| stash.outline.clone())
                .unwrap_or_default()
        } else {
            std::mem::take(&mut self.conflict_resolver.resolved_outline)
        };
        let old_meta = old_outline.meta;
        let old_markers = old_outline.markers;
        let mut next_sources_index = old_outline.sources_index;
        let line_delta = new_affected.len() as isize - old_affected.len() as isize;

        let mut next_meta = Vec::with_capacity(new_line_count);
        next_meta.extend(
            old_meta
                .iter()
                .take(old_affected.start.min(old_meta.len()))
                .cloned(),
        );
        next_meta.extend(middle_meta);
        for entry in old_meta.iter().skip(old_affected.end.min(old_meta.len())) {
            let mut shifted = entry.clone();
            shifted.output_line =
                u32::try_from(shifted_line_index(entry.output_line as usize, line_delta))
                    .unwrap_or(u32::MAX);
            next_meta.push(shifted);
        }
        apply_conflict_choice_provenance_hints(
            &mut next_meta,
            &self.conflict_resolver.marker_segments,
            output_text,
            new_view_mode,
        );

        let mut next_markers = vec![None; new_line_count];
        for (line_ix, marker) in old_markers
            .iter()
            .copied()
            .enumerate()
            .take(old_affected.start.min(old_markers.len()))
        {
            if line_ix < new_line_count {
                next_markers[line_ix] = marker;
            }
        }
        for (old_line_ix, marker) in old_markers
            .iter()
            .copied()
            .enumerate()
            .skip(old_affected.end.min(old_markers.len()))
        {
            let Some(marker) = marker else {
                continue;
            };
            let new_line_ix = shifted_line_index(old_line_ix, line_delta);
            if new_line_ix < new_line_count {
                next_markers[new_line_ix] = Some(shift_resolved_output_marker(marker, line_delta));
            }
        }
        let blocks: Vec<&conflict_resolver::ConflictBlock> = self
            .conflict_resolver
            .marker_segments
            .iter()
            .filter_map(|seg| match seg {
                conflict_resolver::ConflictSegment::Block(block) => Some(block),
                _ => None,
            })
            .collect();
        for conflict_ix in recompute_conflicts {
            let block = blocks[conflict_ix];
            let range = new_block_ranges[conflict_ix].clone();
            let marker_ranges = conflict_marker_ranges_for_block(block, range);
            write_conflict_markers_for_ranges(
                &mut next_markers,
                conflict_ix,
                !block.resolved,
                marker_ranges.as_slice(),
            );
        }

        update_line_sources_index_for_range(
            &mut next_sources_index,
            old_view_mode,
            old_meta.as_slice(),
            old_text,
            old_line_starts.as_ref(),
            old_affected.clone(),
            false,
        );
        update_line_sources_index_for_range(
            &mut next_sources_index,
            new_view_mode,
            next_meta.as_slice(),
            output_text,
            new_line_starts.as_ref(),
            new_affected.clone(),
            true,
        );

        self.conflict_resolved_preview_syntax_language =
            path.and_then(rows::diff_syntax_language_for_path);
        self.conflict_resolved_preview_source_revision = Some(
            ResolvedOutputSourceRevision::from_snapshot(&output_snapshot),
        );
        self.conflict_resolved_preview_line_count = new_line_count;
        self.conflict_resolved_preview_line_starts = new_line_starts;
        self.conflict_resolved_output_measure_row = resolved_output_measure_row(&output_snapshot);
        // The text already reached the live tree on the keystroke. This call is
        // here for what the outline recompute itself changed: the language (the
        // path may have only just resolved) and the unresolved-conflict overlay,
        // which is derived from the marker segments this delta rewrote. It
        // reparses only if the buffer really is different.
        self.refresh_conflict_resolved_output_syntax(&output_snapshot, None, cx);
        self.conflict_resolved_outline_stash = None;
        self.conflict_resolver.resolved_outline = ResolvedOutlineData {
            meta: next_meta,
            markers: next_markers,
            sources_index: next_sources_index,
        };
        self.conflict_resolver.resolved_output_visible_dirty = true;
        self.conflict_resolver.resolved_outline_gutter_rows.clear();
        self.conflict_resolved_preview_text = output_snapshot;
        true
    }

    pub(in crate::view::panes::main) fn conflict_resolver_scroll_resolved_output_to_line(
        &self,
        target_line_ix: usize,
        line_count: usize,
    ) {
        if line_count == 0 {
            return;
        }
        // Deferred item scrolls apply at the next layout pass, so they work
        // before the lists have ever laid out (initial open) and cannot be
        // clamped against stale bounds. Scrolling the gutter and output
        // lists together leaves the per-frame offset sync nothing to
        // arbitrate, which previously ping-ponged the output back to the
        // top of the file.
        let target_line = target_line_ix.min(line_count.saturating_sub(1));
        // Collapsed context mode: the output lists are in fold-projected row
        // space, so address the row showing the line (or its fold).
        let target = self.resolved_output_visible_ix_for_line(target_line);
        self.conflict_resolved_preview_scroll
            .scroll_to_item_strict(target, gpui::ScrollStrategy::Center);
        self.conflict_resolved_preview_gutter_scroll
            .scroll_to_item_strict(target, gpui::ScrollStrategy::Center);
    }

    pub(in crate::view::panes::main) fn conflict_resolver_scroll_resolved_output_to_line_in_text(
        &self,
        target_line_ix: usize,
        output_text: &str,
    ) {
        let line_count = count_newlines(output_text).saturating_add(1);
        self.conflict_resolver_scroll_resolved_output_to_line(target_line_ix, line_count);
    }

    pub(in crate::view::panes::main) fn schedule_conflict_resolved_outline_recompute(
        &mut self,
        path: Option<std::path::PathBuf>,
        source_revision: ResolvedOutputSourceRevision,
        delta: Option<ResolvedOutlineDelta>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_is_streamed() {
            let _ = source_revision;
            let _ = delta;
            self.refresh_streamed_resolved_output_preview_from_markers(path.as_ref());
            cx.notify();
            return;
        }
        self.conflict_resolver.resolver_pending_recompute_seq = self
            .conflict_resolver
            .resolver_pending_recompute_seq
            .wrapping_add(1);
        let seq = self.conflict_resolver.resolver_pending_recompute_seq;

        #[cfg(test)]
        {
            let did_incremental = delta.clone().is_some_and(|delta| {
                self.recompute_conflict_resolved_outline_and_provenance_incremental(
                    path.as_ref(),
                    delta,
                    cx,
                )
            });
            if did_incremental {
                cx.notify();
                return;
            }

            let trace_started = Instant::now();
            let output_snapshot = self
                .conflict_resolver_input
                .read_with(cx, |input, _| input.text_snapshot());
            let background_delay = self
                .conflict_resolved_outline_background_delay_override
                .unwrap_or_default();
            self.sync_conflict_resolved_preview_snapshot(&output_snapshot, path.as_ref(), true, cx);

            if background_delay.is_zero()
                && self.conflict_resolver.resolver_pending_recompute_seq == seq
                && self.conflict_resolved_preview_source_revision == Some(source_revision)
                && self.conflict_resolved_preview_path.as_ref() == path.as_ref()
            {
                // Built here rather than above so this arm matches production,
                // where the request is assembled inside the debounced task. It
                // copies the document, so hoisting it would charge every
                // keystroke for an outline that only runs once per burst.
                let request = self.background_resolved_outline_recompute_request(&output_snapshot);
                let computed = compute_resolved_outline_computation(
                    request.output_text.as_ref(),
                    request.output_line_count,
                    &request.marker_segments,
                    &request.block_map,
                    request.sources.as_view(),
                );
                self.apply_resolved_outline_computation(path.as_ref(), trace_started, computed);
            }

            cx.notify();
        }

        #[cfg(not(test))]
        {
            cx.spawn(
                async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                    smol::Timer::after(Duration::from_millis(
                        CONFLICT_RESOLVED_OUTLINE_DEBOUNCE_MS,
                    ))
                    .await;
                    let request = view.update(cx, |this, cx| {
                        if this.conflict_resolver.resolver_pending_recompute_seq != seq {
                            return None;
                        }
                        if this.conflict_resolved_preview_source_revision != Some(source_revision)
                            || this.conflict_resolved_preview_path.as_ref() != path.as_ref()
                        {
                            return None;
                        }
                        let did_incremental = delta.clone().is_some_and(|delta| {
                            this.recompute_conflict_resolved_outline_and_provenance_incremental(
                                path.as_ref(),
                                delta,
                                cx,
                            )
                        });
                        if !did_incremental {
                            let trace_started = Instant::now();
                            let output_snapshot = this
                                .conflict_resolver_input
                                .read_with(cx, |input, _| input.text_snapshot());
                            let request = this
                                .background_resolved_outline_recompute_request(&output_snapshot);
                            let background_delay = Duration::default();
                            this.sync_conflict_resolved_preview_snapshot(
                                &output_snapshot,
                                path.as_ref(),
                                true,
                                cx,
                            );
                            cx.notify();
                            return Some((request, trace_started, background_delay));
                        }

                        cx.notify();
                        None
                    });
                    let Some((request, trace_started, background_delay)) = request.ok().flatten()
                    else {
                        return;
                    };

                    if !background_delay.is_zero() {
                        smol::Timer::after(background_delay).await;
                    }

                    let compute_outline = move || {
                        compute_resolved_outline_computation(
                            request.output_text.as_ref(),
                            request.output_line_count,
                            &request.marker_segments,
                            &request.block_map,
                            request.sources.as_view(),
                        )
                    };
                    let computed = smol::unblock(compute_outline).await;

                    let _ = view.update(cx, |this, cx| {
                        if this.conflict_resolver.resolver_pending_recompute_seq != seq {
                            return;
                        }
                        if this.conflict_resolved_preview_source_revision != Some(source_revision)
                            || this.conflict_resolved_preview_path.as_ref() != path.as_ref()
                        {
                            return;
                        }

                        this.apply_resolved_outline_computation(
                            path.as_ref(),
                            trace_started,
                            computed,
                        );
                        cx.notify();
                    });
                },
            )
            .detach();
        }
    }

    #[cfg(test)]
    pub(in crate::view) fn recompute_conflict_resolved_outline_for_tests(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let path = self.conflict_resolver.path.clone();
        self.recompute_conflict_resolved_outline_and_provenance(path.as_ref(), cx);
    }

    #[cfg(test)]
    pub(in crate::view) fn set_conflict_resolved_outline_background_delay_override_for_tests(
        &mut self,
        delay: Duration,
    ) {
        self.conflict_resolved_outline_background_delay_override = Some(delay);
    }
}
