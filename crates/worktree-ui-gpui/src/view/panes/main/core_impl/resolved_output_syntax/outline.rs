//! Provenance outline computation: the source views, the incremental base, and
//! the recompute entry points.

use crate::kit::text_model::TextModelSnapshot;
use crate::view::conflict_resolver;
use crate::view::conflict_resolver::ConflictResolverViewMode;
use crate::view::conflict_resolver::ResolvedOutlineData;
use crate::view::conflict_resolver::ResolvedOutputConflictMarker;
use crate::view::panes::main::helpers::apply_conflict_choice_provenance_hints;
use crate::view::panes::main::helpers::apply_conflict_choice_provenance_hints_for_ranges;
use crate::view::panes::main::helpers::build_resolved_output_conflict_markers;
use crate::view::panes::main::helpers::build_resolved_output_conflict_markers_from_block_ranges;
use crate::view::panes::main::helpers::indexed_line_count;
use crate::view::panes::main::helpers::shifted_line_index;
use crate::view::panes::main::helpers::should_skip_resolved_outline_provenance;
use crate::view::panes::main::state::MainPaneView;
use crate::view::rows;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;
use worktree_core::mergetool_trace;
use worktree_core::mergetool_trace::MergetoolTraceEvent;
use worktree_core::mergetool_trace::MergetoolTraceSideStats;
use worktree_core::mergetool_trace::MergetoolTraceStage;
pub(super) fn line_ranges_intersect(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

pub(super) fn shift_resolved_output_marker(
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

pub(super) fn record_resolved_outline_trace(
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

pub(super) struct ResolvedOutlineComputation {
    pub(super) output_line_count: usize,
    pub(super) outline: ResolvedOutlineData,
}

pub(super) enum ResolvedOutlineSourceView<'a> {
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
pub(super) enum OwnedResolvedOutlineSourceData {
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
    pub(super) fn as_view(&self) -> ResolvedOutlineSourceView<'_> {
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
pub(super) struct BackgroundResolvedOutlineRecomputeRequest {
    pub(super) output_text: Arc<str>,
    pub(super) output_line_count: usize,
    pub(super) marker_segments: Vec<conflict_resolver::ConflictSegment>,
    pub(super) block_map: conflict_resolver::ResolvedOutputBlockMap,
    pub(super) sources: OwnedResolvedOutlineSourceData,
}

pub(super) struct ResolvedOutlineIncrementalBase<'a> {
    pub(super) text: &'a TextModelSnapshot,
    pub(super) line_starts: &'a Arc<[usize]>,
    pub(super) marker_segments: &'a [conflict_resolver::ConflictSegment],
    pub(super) view_mode: ConflictResolverViewMode,
}

pub(super) fn compute_resolved_outline_computation(
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

pub(super) fn compute_resolved_outline_computation_from_projection(
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

pub(super) fn insert_lookup_from_indexed_text<'a>(
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

pub(super) fn update_line_sources_index_for_range(
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
