//! Conflict resolver search: the two-way/three-way row models and their predicates.

#[cfg(test)]
use super::row_text::contains_ascii_case_insensitive;
use super::row_text::{diff_search_displayed_text_matches_query, line_text};
use super::stream::{collect_split_stream_match_visible_rows, collect_stream_match_visible_rows};

use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::kit::text_search::DiffSearchMatcher;
use crate::view::conflict_resolver;
use crate::view::conflict_resolver::ConflictResolverUiState;
use crate::view::conflict_resolver::ConflictResolverViewMode;
use crate::view::conflict_resolver::ThreeWayColumn;
use std::borrow::Cow;
pub(in crate::view) fn diff_search_split_row_texts_match_query(
    query: AsciiCaseInsensitiveNeedle<'_>,
    left: Option<&str>,
    right: Option<&str>,
    expanded_tabs: &mut String,
) -> bool {
    if let Some(text) = left
        && diff_search_displayed_text_matches_query(query, text, expanded_tabs)
    {
        return true;
    }

    right.is_some_and(|text| diff_search_displayed_text_matches_query(query, text, expanded_tabs))
}

#[derive(Clone, Copy)]
pub(super) enum ConflictResolverSearchVisibleRows<'a> {
    Projection(&'a conflict_resolver::ThreeWayVisibleProjection),
}

impl<'a> ConflictResolverSearchVisibleRows<'a> {
    pub(super) fn from_conflict_resolver(
        conflict_resolver: &'a ConflictResolverUiState,
    ) -> ConflictResolverSearchVisibleRows<'a> {
        Self::Projection(conflict_resolver.three_way_visible_projection())
    }

    #[cfg(test)]
    fn len(self) -> usize {
        match self {
            Self::Projection(projection) => projection.len(),
        }
    }

    #[cfg(test)]
    fn get(self, visible_ix: usize) -> Option<conflict_resolver::ThreeWayVisibleItem> {
        match self {
            Self::Projection(projection) => projection.get(visible_ix),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ConflictResolverSearchTwoWayRows<'a> {
    Streamed {
        split_row_index: &'a conflict_resolver::ConflictSplitRowIndex,
        two_way_split_projection: &'a conflict_resolver::TwoWaySplitProjection,
    },
}

impl<'a> ConflictResolverSearchTwoWayRows<'a> {
    pub(super) fn from_conflict_resolver(
        conflict_resolver: &'a ConflictResolverUiState,
    ) -> ConflictResolverSearchTwoWayRows<'a> {
        let split_row_index = conflict_resolver
            .split_row_index()
            .expect("streamed conflict resolver must always expose split row index");
        let two_way_split_projection = conflict_resolver
            .two_way_split_projection()
            .expect("streamed conflict resolver must always expose split projection");
        Self::Streamed {
            split_row_index,
            two_way_split_projection,
        }
    }
}

pub(super) struct ConflictResolverSearchContext<'a> {
    pub(super) view_mode: ConflictResolverViewMode,
    pub(super) marker_segments: &'a [conflict_resolver::ConflictSegment],
    pub(super) three_way_visible: ConflictResolverSearchVisibleRows<'a>,
    pub(super) three_way_base_text: &'a str,
    pub(super) three_way_base_line_starts: &'a [usize],
    pub(super) three_way_ours_text: &'a str,
    pub(super) three_way_ours_line_starts: &'a [usize],
    pub(super) three_way_theirs_text: &'a str,
    pub(super) three_way_theirs_line_starts: &'a [usize],
    pub(super) three_way_aligned: &'a conflict_resolver::ThreeWayAlignedMap,
    pub(super) two_way_rows: ConflictResolverSearchTwoWayRows<'a>,
}

impl<'a> ConflictResolverSearchContext<'a> {
    pub(super) fn from_conflict_resolver(conflict_resolver: &'a ConflictResolverUiState) -> Self {
        let (three_way_base_line_starts, three_way_ours_line_starts, three_way_theirs_line_starts) =
            if conflict_resolver.view_mode == ConflictResolverViewMode::ThreeWay {
                (
                    conflict_resolver.three_way_line_starts_ref(ThreeWayColumn::Base),
                    conflict_resolver.three_way_line_starts_ref(ThreeWayColumn::Ours),
                    conflict_resolver.three_way_line_starts_ref(ThreeWayColumn::Theirs),
                )
            } else {
                (&[][..], &[][..], &[][..])
            };
        Self {
            view_mode: conflict_resolver.view_mode,
            marker_segments: &conflict_resolver.marker_segments,
            three_way_visible: ConflictResolverSearchVisibleRows::from_conflict_resolver(
                conflict_resolver,
            ),
            three_way_base_text: &conflict_resolver.three_way_text.base,
            three_way_base_line_starts,
            three_way_ours_text: &conflict_resolver.three_way_text.ours,
            three_way_ours_line_starts,
            three_way_theirs_text: &conflict_resolver.three_way_text.theirs,
            three_way_theirs_line_starts,
            three_way_aligned: &conflict_resolver.three_way_aligned,
            two_way_rows: ConflictResolverSearchTwoWayRows::from_conflict_resolver(
                conflict_resolver,
            ),
        }
    }

    #[cfg(test)]
    pub(super) fn three_way_visible_len(&self) -> usize {
        self.three_way_visible.len()
    }

    #[cfg(test)]
    pub(super) fn three_way_visible_item(
        &self,
        visible_ix: usize,
    ) -> Option<conflict_resolver::ThreeWayVisibleItem> {
        self.three_way_visible.get(visible_ix)
    }
}

#[cfg(test)]
pub(super) fn conflict_resolver_visible_match_indices(
    query: &str,
    ctx: &ConflictResolverSearchContext<'_>,
) -> Vec<usize> {
    let Some(query) = AsciiCaseInsensitiveNeedle::new(query) else {
        return Vec::new();
    };
    conflict_resolver_visible_match_indices_with_needle(query, ctx)
}

pub(super) fn conflict_resolver_visible_match_indices_with_needle(
    query: AsciiCaseInsensitiveNeedle<'_>,
    ctx: &ConflictResolverSearchContext<'_>,
) -> Vec<usize> {
    let mut out = Vec::new();
    match ctx.view_mode {
        ConflictResolverViewMode::ThreeWay => {
            let ConflictResolverSearchVisibleRows::Projection(projection) = ctx.three_way_visible;
            search_three_way_via_spans(projection, ctx, query, &mut out);
        }
        ConflictResolverViewMode::TwoWayDiff => {
            let ConflictResolverSearchTwoWayRows::Streamed {
                split_row_index,
                two_way_split_projection,
            } = ctx.two_way_rows;
            let matching_rows = split_row_index
                .search_ascii_case_insensitive_matching_rows(ctx.marker_segments, query.as_bytes());
            for source_row in matching_rows {
                if let Some(vis) = two_way_split_projection.source_to_visible(source_row) {
                    out.push(vis);
                }
            }
        }
    }
    out
}

pub(super) fn conflict_resolver_visible_match_indices_with_matcher(
    matcher: &DiffSearchMatcher,
    ctx: &ConflictResolverSearchContext<'_>,
) -> Vec<usize> {
    let mut out = Vec::new();
    match ctx.view_mode {
        ConflictResolverViewMode::ThreeWay => {
            let ConflictResolverSearchVisibleRows::Projection(projection) = ctx.three_way_visible;
            search_three_way_via_spans_with_matcher(projection, ctx, matcher, &mut out);
        }
        ConflictResolverViewMode::TwoWayDiff => {
            let ConflictResolverSearchTwoWayRows::Streamed {
                split_row_index,
                two_way_split_projection,
            } = ctx.two_way_rows;
            let rows = (0..two_way_split_projection.visible_len()).filter_map(|visible_ix| {
                let (source_row, _conflict_ix) = two_way_split_projection.get(visible_ix)?;
                let row = split_row_index.row_at(ctx.marker_segments, source_row)?;
                Some((
                    visible_ix,
                    row.old
                        .as_ref()
                        .map(|text| Cow::Owned(text.as_ref().to_string())),
                    row.new
                        .as_ref()
                        .map(|text| Cow::Owned(text.as_ref().to_string())),
                ))
            });
            collect_split_stream_match_visible_rows(rows, matcher, &mut out);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Search three-way source texts by iterating projection spans directly.
///
/// This avoids the per-visible-item O(log spans) projection lookup by walking
/// spans sequentially and extracting line text from the three source texts.
fn search_three_way_via_spans(
    projection: &conflict_resolver::ThreeWayVisibleProjection,
    ctx: &ConflictResolverSearchContext<'_>,
    query: AsciiCaseInsensitiveNeedle<'_>,
    out: &mut Vec<usize>,
) {
    for span in projection.spans() {
        match *span {
            conflict_resolver::ThreeWayVisibleSpan::Lines {
                visible_start,
                source_line_start,
                len,
            } => {
                for i in 0..len {
                    // section 30: rows are aligned; each side renders its own line
                    // there (padding rows have no text).
                    let row = source_line_start + i;
                    let aligned = ctx.three_way_aligned;
                    let base = aligned.side_line_for_row(0, row).map_or("", |l| {
                        line_text(ctx.three_way_base_text, ctx.three_way_base_line_starts, l)
                    });
                    let ours = aligned.side_line_for_row(1, row).map_or("", |l| {
                        line_text(ctx.three_way_ours_text, ctx.three_way_ours_line_starts, l)
                    });
                    let theirs = aligned.side_line_for_row(2, row).map_or("", |l| {
                        line_text(
                            ctx.three_way_theirs_text,
                            ctx.three_way_theirs_line_starts,
                            l,
                        )
                    });
                    if query.is_match(base) || query.is_match(ours) || query.is_match(theirs) {
                        out.push(visible_start + i);
                    }
                }
            }
            conflict_resolver::ThreeWayVisibleSpan::CollapsedResolvedBlock {
                visible_index,
                conflict_ix,
            } => {
                let choice_label = conflict_choice_for_index(ctx.marker_segments, conflict_ix)
                    .map(conflict_choice_label)
                    .unwrap_or("?");
                let summary = format!("Resolved: picked {choice_label}");
                if query.is_match(&summary) {
                    out.push(visible_index);
                }
            }
            // Folded context lines are not visible, so they are not searched.
            conflict_resolver::ThreeWayVisibleSpan::CollapsedContext { .. } => {}
        }
    }
}

fn search_three_way_via_spans_with_matcher(
    projection: &conflict_resolver::ThreeWayVisibleProjection,
    ctx: &ConflictResolverSearchContext<'_>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    let mut base_rows = Vec::new();
    let mut ours_rows = Vec::new();
    let mut theirs_rows = Vec::new();
    let mut summary_rows = Vec::new();

    for span in projection.spans() {
        match *span {
            conflict_resolver::ThreeWayVisibleSpan::Lines {
                visible_start,
                source_line_start,
                len,
            } => {
                for i in 0..len {
                    let visible_ix = visible_start + i;
                    // section 30: rows are aligned; translate per side.
                    let row = source_line_start + i;
                    let aligned = ctx.three_way_aligned;
                    base_rows.push((
                        visible_ix,
                        Cow::Borrowed(aligned.side_line_for_row(0, row).map_or("", |l| {
                            line_text(ctx.three_way_base_text, ctx.three_way_base_line_starts, l)
                        })),
                    ));
                    ours_rows.push((
                        visible_ix,
                        Cow::Borrowed(aligned.side_line_for_row(1, row).map_or("", |l| {
                            line_text(ctx.three_way_ours_text, ctx.three_way_ours_line_starts, l)
                        })),
                    ));
                    theirs_rows.push((
                        visible_ix,
                        Cow::Borrowed(aligned.side_line_for_row(2, row).map_or("", |l| {
                            line_text(
                                ctx.three_way_theirs_text,
                                ctx.three_way_theirs_line_starts,
                                l,
                            )
                        })),
                    ));
                }
            }
            conflict_resolver::ThreeWayVisibleSpan::CollapsedResolvedBlock {
                visible_index,
                conflict_ix,
            } => {
                let choice_label = conflict_choice_for_index(ctx.marker_segments, conflict_ix)
                    .map(conflict_choice_label)
                    .unwrap_or("?");
                summary_rows.push((
                    visible_index,
                    Cow::Owned(format!("Resolved: picked {choice_label}")),
                ));
            }
            // Folded context lines are not visible, so they are not searched.
            conflict_resolver::ThreeWayVisibleSpan::CollapsedContext { .. } => {}
        }
    }

    collect_stream_match_visible_rows(base_rows, matcher, out);
    collect_stream_match_visible_rows(ours_rows, matcher, out);
    collect_stream_match_visible_rows(theirs_rows, matcher, out);
    collect_stream_match_visible_rows(summary_rows, matcher, out);
}

fn conflict_choice_for_index(
    segments: &[conflict_resolver::ConflictSegment],
    conflict_ix: usize,
) -> Option<conflict_resolver::ConflictChoice> {
    segments
        .iter()
        .filter_map(|seg| match seg {
            conflict_resolver::ConflictSegment::Block(block) => Some(block.choice),
            _ => None,
        })
        .nth(conflict_ix)
}

fn conflict_choice_label(choice: conflict_resolver::ConflictChoice) -> &'static str {
    match choice {
        conflict_resolver::ConflictChoice::Base => "Base (A)",
        conflict_resolver::ConflictChoice::Ours => "Local (B)",
        conflict_resolver::ConflictChoice::Theirs => "Remote (C)",
        conflict_resolver::ConflictChoice::Both => "Local+Remote (B+C)",
        _ => "Ordered source selection",
    }
}

#[cfg(test)]
pub(super) fn three_way_visible_item_matches_query(
    item: conflict_resolver::ThreeWayVisibleItem,
    ctx: &ConflictResolverSearchContext<'_>,
    query: &str,
) -> bool {
    match item {
        conflict_resolver::ThreeWayVisibleItem::Line(ix) => {
            // section 30: `ix` is an aligned row; translate per side.
            let aligned = ctx.three_way_aligned;
            let base = aligned.side_line_for_row(0, ix).map_or("", |l| {
                line_text(ctx.three_way_base_text, ctx.three_way_base_line_starts, l)
            });
            let ours = aligned.side_line_for_row(1, ix).map_or("", |l| {
                line_text(ctx.three_way_ours_text, ctx.three_way_ours_line_starts, l)
            });
            let theirs = aligned.side_line_for_row(2, ix).map_or("", |l| {
                line_text(
                    ctx.three_way_theirs_text,
                    ctx.three_way_theirs_line_starts,
                    l,
                )
            });

            contains_ascii_case_insensitive(base, query)
                || contains_ascii_case_insensitive(ours, query)
                || contains_ascii_case_insensitive(theirs, query)
        }
        conflict_resolver::ThreeWayVisibleItem::CollapsedBlock(conflict_ix) => {
            let choice_label = conflict_choice_for_index(ctx.marker_segments, conflict_ix)
                .map(conflict_choice_label)
                .unwrap_or("?");
            let summary = format!("Resolved: picked {choice_label}");
            contains_ascii_case_insensitive(&summary, query)
        }
        // Folded context lines are not visible, so they are not searched.
        conflict_resolver::ThreeWayVisibleItem::CollapsedContext { .. } => false,
    }
}

#[cfg(test)]
pub(super) fn identity_three_way_aligned() -> &'static conflict_resolver::ThreeWayAlignedMap {
    use std::sync::OnceLock;
    static MAP: OnceLock<conflict_resolver::ThreeWayAlignedMap> = OnceLock::new();
    MAP.get_or_init(conflict_resolver::ThreeWayAlignedMap::default)
}
