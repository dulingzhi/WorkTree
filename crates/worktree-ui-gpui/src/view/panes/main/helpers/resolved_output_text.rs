use super::super::*;
use crate::kit::rope::Rope;
use crate::kit::text_model::TextModelSnapshot;
use crate::kit::{HighlightProvider, HighlightProviderResult};
use palette::IntoColor;
use rustc_hash::FxHasher;
use std::collections::HashSet;

/// Heuristic highlights for the rows overlapping `byte_range`.
///
/// Windowing this is *exact*, not an approximation: the heuristic tokenizer is
/// line-local, so a row's tokens do not depend on anything above it. (A
/// tree-sitter query is the opposite — it needs the enclosing tree, which is why
/// that path queries a range of a whole-document parse instead.)
///
/// Reading rows through the rope keeps the cost proportional to the viewport.
/// The previous shape tokenized the entire document and handed the result to
/// `set_highlights` on every keystroke, which is the one thing this arm — the
/// arm reached by the *largest* buffers — could least afford.
pub(in crate::view) fn resolved_output_heuristic_highlights_for_range(
    theme: AppTheme,
    output_text: &Rope,
    language: rows::DiffSyntaxLanguage,
    byte_range: Range<usize>,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    let len = output_text.len();
    let start = byte_range.start.min(len);
    let end = byte_range.end.min(len).max(start);
    if start == end {
        return Vec::new();
    }

    let first_row = output_text.offset_to_point(start).row;
    let last_row = output_text.offset_to_point(end).row;
    let mut highlights = Vec::new();
    for row in first_row..=last_row {
        let line_range = output_text.line_range(row);
        if line_range.start >= len && row > first_row {
            break;
        }
        let line = output_text.line_text(row);
        for (range, style) in rows::syntax_highlights_for_line(
            theme,
            &line,
            language,
            rows::DiffSyntaxMode::HeuristicOnly,
        ) {
            highlights.push((
                (line_range.start + range.start)..(line_range.start + range.end),
                style,
            ));
        }
    }
    highlights
}

/// The fallback counterpart to [`resolved_output_live_highlight_provider`], for
/// buffers with no live tree — no wired grammar, or past the parse ceiling.
///
/// Same contract: answers whatever window the input asks for, never reports
/// pending, and carries the unresolved-conflict overlay on top.
pub(in crate::view) fn resolved_output_heuristic_highlight_provider(
    theme: AppTheme,
    output_text: Rope,
    language: Option<rows::DiffSyntaxLanguage>,
    unresolved_spans: ResolvedOutputUnresolvedSpans,
) -> HighlightProvider {
    let unresolved_style = resolved_output_unresolved_highlight_style(theme);
    let active_unresolved_style = resolved_output_active_unresolved_highlight_style(theme);
    HighlightProvider::with_pending(
        move |byte_range: Range<usize>| HighlightProviderResult {
            highlights: apply_resolved_output_unresolved_highlights(
                language
                    .map(|language| {
                        resolved_output_heuristic_highlights_for_range(
                            theme,
                            &output_text,
                            language,
                            byte_range.clone(),
                        )
                    })
                    .unwrap_or_default(),
                &unresolved_spans,
                byte_range,
                unresolved_style,
                active_unresolved_style,
            ),
            pending: false,
        },
        || 0,
        || false,
    )
}

/// Binding key for the heuristic provider.
///
/// The live provider keys on its tree's version; this one has no tree, so it
/// keys on the buffer revision the closure captured, plus the theme and the
/// overlay. Distinct from the live key space so the two can never collide on a
/// buffer that switches arms.
pub(in crate::view) fn resolved_output_heuristic_provider_binding_key(
    revision: ResolvedOutputSourceRevision,
    theme_epoch: u64,
    unresolved_spans: &ResolvedOutputUnresolvedSpans,
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = FxHasher::default();
    "heuristic".hash(&mut hasher);
    revision.model_id.hash(&mut hasher);
    revision.revision.hash(&mut hasher);
    theme_epoch.hash(&mut hasher);
    unresolved_spans.all.hash(&mut hasher);
    unresolved_spans.active.hash(&mut hasher);
    hasher.finish()
}

pub(in crate::view) fn resolved_output_unresolved_highlight_style(
    theme: AppTheme,
) -> gpui::HighlightStyle {
    gpui::HighlightStyle {
        color: Some(theme.colors.status.danger.foreground.into_color()),
        ..gpui::HighlightStyle::default()
    }
}

/// The unresolved treatment for the conflict the resolver is parked on: the same
/// danger text over a yellow wash, so the output says which of several open
/// `<Merge Conflict>` rows the picks and the source columns are about.
pub(in crate::view) fn resolved_output_active_unresolved_highlight_style(
    theme: AppTheme,
) -> gpui::HighlightStyle {
    gpui::HighlightStyle {
        background_color: Some(resolved_output_active_conflict_background(theme).into_color()),
        ..resolved_output_unresolved_highlight_style(theme)
    }
}

/// The yellow the active conflict's row is washed with, shared by the editable
/// output's text highlight, its gutter row and the streamed read-only rows so
/// one row reads as one band across all three.
pub(in crate::view) fn resolved_output_active_conflict_background(theme: AppTheme) -> gpui::Rgba {
    with_alpha(
        theme.colors.status.warning.foreground,
        if theme.is_dark { 0.30 } else { 0.34 },
    )
}

/// The still-unresolved output rows, split into every one of them and the subset
/// belonging to the conflict the resolver is parked on.
///
/// Both are derived in one pass because this runs on the keystroke path, and
/// `active` is always a subset of `all` — the two can never disagree about where
/// a row starts and ends.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct ResolvedOutputUnresolvedSpans {
    pub(in crate::view) all: Arc<[Range<usize>]>,
    pub(in crate::view) active: Arc<[Range<usize>]>,
}

impl ResolvedOutputUnresolvedSpans {
    fn is_active(&self, range: &Range<usize>) -> bool {
        self.active.iter().any(|active| active == range)
    }
}

/// Replace syntax styles inside unresolved output ranges with one plain danger
/// style — the active conflict's rows with the washed variant of it. The
/// returned ranges are non-overlapping with the unresolved spans, so the text
/// input's later-highlight precedence cannot reveal syntax colours through the
/// conflict treatment.
pub(in crate::view) fn apply_resolved_output_unresolved_highlights(
    mut syntax_highlights: Vec<(Range<usize>, gpui::HighlightStyle)>,
    unresolved_spans: &ResolvedOutputUnresolvedSpans,
    requested_range: Range<usize>,
    unresolved_style: gpui::HighlightStyle,
    active_unresolved_style: gpui::HighlightStyle,
) -> Vec<(Range<usize>, gpui::HighlightStyle)> {
    let unresolved_ranges = unresolved_spans.all.as_ref();
    if unresolved_ranges.is_empty() || requested_range.is_empty() {
        return syntax_highlights;
    }

    let mut highlights = Vec::with_capacity(
        syntax_highlights
            .len()
            .saturating_add(unresolved_ranges.len()),
    );
    for (syntax_range, style) in syntax_highlights.drain(..) {
        if syntax_range.is_empty() {
            continue;
        }

        let mut cursor = syntax_range.start;
        let first_unresolved =
            unresolved_ranges.partition_point(|range| range.end <= syntax_range.start);
        for unresolved in unresolved_ranges.iter().skip(first_unresolved) {
            let unresolved_start = unresolved.start.max(requested_range.start);
            let unresolved_end = unresolved.end.min(requested_range.end);
            if unresolved_start >= syntax_range.end {
                break;
            }
            if unresolved_end <= cursor || unresolved_start >= unresolved_end {
                continue;
            }
            if cursor < unresolved_start {
                highlights.push((cursor..unresolved_start.min(syntax_range.end), style));
            }
            cursor = cursor.max(unresolved_end);
            if cursor >= syntax_range.end {
                break;
            }
        }
        if cursor < syntax_range.end {
            highlights.push((cursor..syntax_range.end, style));
        }
    }

    for unresolved in unresolved_ranges {
        let start = unresolved.start.max(requested_range.start);
        let end = unresolved.end.min(requested_range.end);
        if start < end {
            let style = if unresolved_spans.is_active(unresolved) {
                active_unresolved_style
            } else {
                unresolved_style
            };
            highlights.push((start..end, style));
        }
    }
    highlights.sort_by(|(left, _), (right, _)| {
        left.start.cmp(&right.start).then(left.end.cmp(&right.end))
    });

    let mut merged: Vec<(Range<usize>, gpui::HighlightStyle)> =
        Vec::with_capacity(highlights.len());
    for (range, style) in highlights {
        if let Some((previous_range, previous_style)) = merged.last_mut()
            && previous_range.end == range.start
            && *previous_style == style
        {
            previous_range.end = range.end;
        } else {
            merged.push((range, style));
        }
    }
    merged
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct ResolvedOutputSourceRevision {
    pub(in crate::view) model_id: u64,
    pub(in crate::view) revision: u64,
}

impl ResolvedOutputSourceRevision {
    pub(in crate::view) fn from_snapshot(snapshot: &TextModelSnapshot) -> Self {
        Self {
            model_id: snapshot.model_id(),
            revision: snapshot.revision(),
        }
    }
}

pub(in crate::view) fn resolved_output_snapshot_is_modified(
    saved: Option<&TextModelSnapshot>,
    current: &TextModelSnapshot,
) -> bool {
    saved.is_some_and(|saved| current != saved)
}

/// Whether the worktree payload must be kept as opaque user output instead of
/// replacing it with the stage-derived marker projection.
///
/// The question this answers is "would projecting throw away work someone did by
/// hand?", and the only usable evidence is the document's own content: every
/// line of an untouched conflict document comes from one of the three stages,
/// because git assembled it out of them. A hand resolution types something new,
/// and that line belongs to no stage.
///
/// The comparison is deliberately *not* against the projection or the stage
/// blobs. Two correct three-way merges of the same stages may place their
/// conflict boundaries in entirely different places — ours anchors differently
/// than git's `xdiff` does, and the contributor-alignment pass moves boundaries
/// again — so the two documents interleave the same lines in different orders
/// and neither reconstructs to the other's sides nor to a whole stage blob.
/// Demanding either equality protected essentially every real merge, which left
/// the resolver inert: no marker geometry, every pick a silent no-op, and no way
/// out but *Reset conflict markers*.
///
/// The cost of the weaker test is that a hand resolution built purely by
/// *deleting* lines — picking a side in an editor, markers and all — reads as
/// untouched. Nothing is lost on disk either way: the resolver only rewrites its
/// own buffer, and the worktree file stands until an explicit Save.
pub(in crate::view) fn worktree_output_requires_protection(
    current: Option<&str>,
    marker_projection: Option<&str>,
    base: Option<&str>,
    ours: Option<&str>,
    theirs: Option<&str>,
) -> bool {
    let Some(current) = current else {
        return false;
    };
    if marker_projection == Some(current) {
        return false;
    }
    // The projection renders every line with one detected ending, so a document
    // that mixes CRLF and LF cannot be reproduced from it even when every line
    // of it comes from a stage. Keep the worktree bytes rather than rewriting
    // terminators the user never touched.
    if worktree_core::text_utils::text_has_mixed_line_endings(current) {
        return true;
    }

    let marker_ranges = worktree_core::conflict_session::parse_conflict_marker_ranges(current);
    if !marker_ranges.iter().any(|segment| {
        matches!(
            segment,
            worktree_core::conflict_session::ParsedConflictSegmentRanges::Conflict(_)
        )
    }) {
        return true;
    }

    let (Some(ours), Some(theirs)) = (ours, theirs) else {
        return true;
    };
    // `std`'s seeded `HashSet`, not `FxHashSet`: the keys are raw file lines
    // off disk, i.e. content an untrusted repository controls.
    let stage_lines: HashSet<&str> = base
        .into_iter()
        .chain([ours, theirs])
        .flat_map(str::lines)
        .collect();
    !conflict_document_content_lines(current, &marker_ranges).all(|line| stage_lines.contains(line))
}

/// The document's lines with the four marker lines left out, so a comparison
/// against stage content is not defeated by the labels git wrote.
fn conflict_document_content_lines<'a>(
    current: &'a str,
    marker_ranges: &'a [worktree_core::conflict_session::ParsedConflictSegmentRanges],
) -> impl Iterator<Item = &'a str> {
    use worktree_core::conflict_session::ParsedConflictSegmentRanges as Segment;

    marker_ranges
        .iter()
        .flat_map(move |segment| {
            let ranges = match segment {
                Segment::Text(range) => vec![range.clone()],
                Segment::Conflict(block) => [Some(block.ours.clone()), block.base.clone()]
                    .into_iter()
                    .flatten()
                    .chain([block.theirs.clone()])
                    .collect(),
            };
            ranges.into_iter()
        })
        .flat_map(move |range| current.get(range).unwrap_or_default().lines())
}

#[derive(Clone, Debug)]
pub(in crate::view) struct StashedResolvedOutlineState {
    pub(in crate::view) text: TextModelSnapshot,
    pub(in crate::view) line_starts: Arc<[usize]>,
    pub(in crate::view) marker_segments: Vec<conflict_resolver::ConflictSegment>,
    pub(in crate::view) view_mode: ConflictResolverViewMode,
    pub(in crate::view) outline: ResolvedOutlineData,
}

pub(in crate::view) fn count_newlines(text: &str) -> usize {
    text.as_bytes().iter().filter(|&&b| b == b'\n').count()
}

pub(in crate::view) fn build_line_starts(text: &str) -> Vec<usize> {
    build_line_starts_with_count(text).0
}

pub(in crate::view) fn build_line_starts_with_count(text: &str) -> (Vec<usize>, usize) {
    let mut line_starts = Vec::with_capacity(text.len().saturating_div(64).saturating_add(1));
    line_starts.push(0usize);
    for (ix, byte) in text.as_bytes().iter().enumerate() {
        if *byte == b'\n' {
            line_starts.push(ix.saturating_add(1));
        }
    }
    let line_count = if text.is_empty() {
        0
    } else {
        line_starts.len()
    };
    (line_starts, line_count)
}

#[cfg(test)]
pub(in crate::view) fn preview_source_text_from_lines(
    lines: &[String],
    source_len: usize,
) -> SharedString {
    let mut source = lines.join("\n");
    if source.len() < source_len {
        source.push('\n');
    }
    debug_assert_eq!(
        source.len(),
        source_len,
        "preview lines/source length should only differ by an optional trailing newline",
    );
    source.into()
}

pub(in crate::view) fn preview_source_text_and_line_starts_from_lines(
    lines: &[String],
    source_len: usize,
) -> (SharedString, Arc<[usize]>) {
    if lines.is_empty() {
        debug_assert_eq!(
            source_len, 0,
            "empty preview lines should only produce empty source text",
        );
        return (SharedString::default(), Arc::default());
    }

    let mut text = String::with_capacity(source_len);
    let mut line_starts = Vec::with_capacity(lines.len().saturating_add(1));
    line_starts.push(0);
    for (ix, line) in lines.iter().enumerate() {
        text.push_str(line);
        let has_more_lines = ix + 1 < lines.len();
        let needs_trailing_newline = !has_more_lines && text.len() < source_len;
        if has_more_lines || needs_trailing_newline {
            text.push('\n');
            line_starts.push(text.len());
        }
    }
    debug_assert_eq!(
        text.len(),
        source_len,
        "preview lines/source length should only differ by an optional trailing newline",
    );
    (text.into(), Arc::from(line_starts))
}

const PREVIEW_LINE_FLAG_ASCII_ONLY: u8 = 0b01;
const PREVIEW_LINE_FLAG_HAS_TABS: u8 = 0b10;

#[inline]
pub(in crate::view) fn preview_line_flags_for_text(text: &str) -> u8 {
    preview_line_flags_from_bools(text.is_ascii(), text.contains('\t'))
}

#[inline]
pub(in crate::view) fn preview_line_flags_from_bools(ascii_only: bool, has_tabs: bool) -> u8 {
    let mut flags = 0u8;
    if ascii_only {
        flags |= PREVIEW_LINE_FLAG_ASCII_ONLY;
    }
    if has_tabs {
        flags |= PREVIEW_LINE_FLAG_HAS_TABS;
    }
    flags
}

#[inline]
pub(in crate::view) fn preview_line_is_ascii_without_loading(flags: u8) -> bool {
    (flags & PREVIEW_LINE_FLAG_ASCII_ONLY) != 0
}

#[inline]
pub(in crate::view) fn preview_line_has_tabs_without_loading(flags: u8) -> bool {
    (flags & PREVIEW_LINE_FLAG_HAS_TABS) != 0
}

pub(in crate::view) fn preview_line_flags_from_source(
    text: &str,
    line_starts: &[usize],
) -> Arc<[u8]> {
    let line_count = indexed_line_count_from_len(text.len(), line_starts);
    let mut flags = Vec::with_capacity(line_count);
    for line_ix in 0..line_count {
        let range = indexed_line_byte_range(line_starts, text.len(), line_ix)
            .unwrap_or(text.len()..text.len());
        flags.push(preview_line_flags_for_text(
            text.get(range).unwrap_or_default(),
        ));
    }
    Arc::from(flags)
}

pub(in crate::view) fn line_start_offset_for_index(
    line_starts: &[usize],
    text_len: usize,
    line_ix: usize,
) -> usize {
    line_starts.get(line_ix).copied().unwrap_or(text_len)
}

pub(in crate::view) fn source_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

/// Number of logical rows represented by precomputed line starts.
///
/// Uses `split('\n')` row semantics for non-empty text, so a trailing newline
/// preserves a final empty row.
pub(in crate::view) fn indexed_line_count_from_len(
    source_len: usize,
    line_starts: &[usize],
) -> usize {
    if source_len == 0 {
        0
    } else {
        line_starts.len().max(1)
    }
}

pub(in crate::view) fn indexed_line_count(text: &str, line_starts: &[usize]) -> usize {
    indexed_line_count_from_len(text.len(), line_starts)
}

pub(in crate::view) fn indexed_line_byte_range(
    line_starts: &[usize],
    source_len: usize,
    line_ix: usize,
) -> Option<Range<usize>> {
    let line_count = indexed_line_count_from_len(source_len, line_starts);
    if line_ix >= line_count {
        return None;
    }

    let start = line_starts
        .get(line_ix)
        .copied()
        .unwrap_or(source_len)
        .min(source_len);
    let end = line_starts
        .get(line_ix.saturating_add(1))
        .copied()
        .map(|next| next.saturating_sub(1))
        .unwrap_or(source_len)
        .min(source_len)
        .max(start);
    Some(start..end)
}

/// Number of logical rows produced by `split('\n')` (always at least 1).
pub(in crate::view) fn split_line_count(text: &str) -> usize {
    count_newlines(text).saturating_add(1)
}

/// Full resolved-output provenance is much more expensive in three-way mode,
/// because it builds source-line lookups across all three full documents.
pub(in crate::view) const LARGE_RESOLVED_OUTLINE_THREE_WAY_PROVENANCE_MAX_LINES: usize = 50_000;
/// Two-way mode still needs a cap, because the source-index alone scales with
/// output-line count even when the diff-row lookup is small.
pub(in crate::view) const LARGE_RESOLVED_OUTLINE_TWO_WAY_PROVENANCE_MAX_LINES: usize = 200_000;

pub(in crate::view) fn should_skip_resolved_outline_provenance(
    view_mode: ConflictResolverViewMode,
    output_line_count: usize,
) -> bool {
    match view_mode {
        ConflictResolverViewMode::ThreeWay => {
            output_line_count > LARGE_RESOLVED_OUTLINE_THREE_WAY_PROVENANCE_MAX_LINES
        }
        ConflictResolverViewMode::TwoWayDiff => {
            output_line_count > LARGE_RESOLVED_OUTLINE_TWO_WAY_PROVENANCE_MAX_LINES
        }
    }
}

/// Byte range of line content at `line_ix` (without trailing newline).
///
/// Uses `split('\n')` row semantics, so trailing newline creates a final empty row.
pub(in crate::view) fn line_content_byte_range_for_index(
    text: &str,
    line_ix: usize,
) -> Option<Range<usize>> {
    let line_count = split_line_count(text);
    if line_ix >= line_count {
        return None;
    }
    let line_starts = build_line_starts(text);
    let text_len = text.len();
    let start = line_starts.get(line_ix).copied().unwrap_or(text_len);
    let mut end = line_starts
        .get(line_ix.saturating_add(1))
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    if end > start && text.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n') {
        end = end.saturating_sub(1);
    }
    Some(start..end)
}

/// Build insertion text for appending one logical line to output.
pub(in crate::view) fn append_line_insertion_text(existing: &str, line: &str) -> String {
    let needs_leading_newline = !existing.is_empty() && !existing.ends_with('\n');
    let mut out = String::with_capacity(
        line.len()
            .saturating_add(1)
            .saturating_add(usize::from(needs_leading_newline)),
    );
    if needs_leading_newline {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct ResolvedOutlineDelta {
    pub(in crate::view) old_range: Range<usize>,
    pub(in crate::view) new_range: Range<usize>,
}

pub(in crate::view) fn resolved_outline_delta_between_texts(
    old_text: &str,
    new_text: &str,
) -> Option<ResolvedOutlineDelta> {
    if old_text == new_text {
        return None;
    }

    let old = old_text.as_bytes();
    let new = new_text.as_bytes();
    let old_len = old.len();
    let new_len = new.len();

    let mut prefix = 0usize;
    let prefix_max = old_len.min(new_len);
    while prefix < prefix_max && old[prefix] == new[prefix] {
        prefix = prefix.saturating_add(1);
    }
    while prefix > 0 && (!old_text.is_char_boundary(prefix) || !new_text.is_char_boundary(prefix)) {
        prefix = prefix.saturating_sub(1);
    }

    let mut suffix = 0usize;
    while suffix < old_len.saturating_sub(prefix)
        && suffix < new_len.saturating_sub(prefix)
        && old[old_len.saturating_sub(1 + suffix)] == new[new_len.saturating_sub(1 + suffix)]
    {
        suffix = suffix.saturating_add(1);
    }
    while suffix > 0
        && (!old_text.is_char_boundary(old_len.saturating_sub(suffix))
            || !new_text.is_char_boundary(new_len.saturating_sub(suffix)))
    {
        suffix = suffix.saturating_sub(1);
    }

    Some(ResolvedOutlineDelta {
        old_range: prefix..old_len.saturating_sub(suffix),
        new_range: prefix..new_len.saturating_sub(suffix),
    })
}

pub(in crate::view) fn resolved_outline_delta_for_snapshot_transition(
    old_snapshot: &TextModelSnapshot,
    new_snapshot: &TextModelSnapshot,
    recent_edit_delta: Option<(Range<usize>, Range<usize>)>,
) -> Option<ResolvedOutlineDelta> {
    if old_snapshot.model_id() == new_snapshot.model_id()
        && new_snapshot.revision() == old_snapshot.revision().saturating_add(1)
        && let Some((old_range, new_range)) = recent_edit_delta
    {
        return Some(ResolvedOutlineDelta {
            old_range,
            new_range,
        });
    }

    // Do not materialize and compare both documents on the immediate input
    // notification path. If observer delivery coalesced several revisions, the
    // surviving debounced task will perform the full outline recompute instead.
    None
}

fn line_index_for_byte_offset(line_starts: &[usize], byte_offset: usize) -> usize {
    if line_starts.is_empty() {
        return 0;
    }
    line_starts
        .partition_point(|&start| start <= byte_offset)
        .saturating_sub(1)
}

pub(in crate::view) fn dirty_byte_range_to_line_range(
    line_starts: &[usize],
    text_len: usize,
    dirty_range: Range<usize>,
) -> Range<usize> {
    let line_count = line_starts.len().max(1);
    let start_byte = dirty_range.start.min(text_len);
    let end_byte = dirty_range.end.min(text_len);
    let start_line = line_index_for_byte_offset(line_starts, start_byte).min(line_count - 1);
    let end_line_exclusive = if dirty_range.is_empty() {
        start_line.saturating_add(1)
    } else {
        line_index_for_byte_offset(line_starts, end_byte).saturating_add(1)
    }
    .clamp(start_line.saturating_add(1), line_count);
    start_line..end_line_exclusive
}

pub(in crate::view) fn shifted_line_index(ix: usize, delta: isize) -> usize {
    if delta >= 0 {
        ix.saturating_add(delta as usize)
    } else {
        ix.saturating_sub((-delta) as usize)
    }
}

pub(in crate::view) fn remap_resolved_output_conflict_block_ranges_for_delta(
    old_block_ranges: &[Range<usize>],
    old_range: Range<usize>,
    new_range: Range<usize>,
    new_line_count: usize,
) -> Vec<Range<usize>> {
    let line_delta = new_range.len() as isize - old_range.len() as isize;
    old_block_ranges
        .iter()
        .map(|range| {
            let remapped = if range.end <= old_range.start {
                range.clone()
            } else if range.start >= old_range.end {
                shifted_line_index(range.start, line_delta)
                    ..shifted_line_index(range.end, line_delta)
            } else {
                let start = if old_range.start <= range.start {
                    new_range.start
                } else {
                    range.start
                };
                let end = if range.end <= old_range.end {
                    new_range.end
                } else {
                    shifted_line_index(range.end, line_delta)
                };
                start..end
            };
            remapped.start.min(new_line_count)..remapped.end.min(new_line_count)
        })
        .map(|range| range.start..range.end.max(range.start))
        .collect()
}

/// Identity of everything the resolved-output highlight provider closes over.
///
/// This has to be *stable* when nothing changed, not merely unique. Installing a
/// provider notifies the input, which re-enters the `cx.observe` that installed
/// it; an always-fresh key would rebind on that re-entry, notify again, and spin
/// forever. `set_highlight_provider_with_key` early-returns on an unchanged key
/// without notifying, which is what terminates the cycle.
///
/// The document version covers the text and the tree; the theme and the
/// unresolved-conflict spans are the other two things baked into the closure.
pub(in crate::view) fn resolved_output_live_provider_binding_key(
    document_version: u64,
    theme_epoch: u64,
    unresolved_spans: &ResolvedOutputUnresolvedSpans,
) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = FxHasher::default();
    document_version.hash(&mut hasher);
    theme_epoch.hash(&mut hasher);
    unresolved_spans.all.hash(&mut hasher);
    // Navigating between conflicts moves only this half, and it is what decides
    // which row wears the active wash — leave it out and the provider stays
    // bound to the previous conflict's highlight.
    unresolved_spans.active.hash(&mut hasher);
    hasher.finish()
}

/// Highlights for the resolved output, straight off the live tree.
///
/// Unlike the prepared-document provider this replaced, it is always exact for
/// the text it was built over and so never reports `pending`: the document is
/// re-synced on the keystroke and the provider rebound with it. That is what
/// keeps `TextInput`'s interpolation and superseded-source machinery dormant
/// here — they exist to cover a recompute lag this path does not have.
pub(in crate::view) fn resolved_output_live_highlight_provider(
    theme: AppTheme,
    snapshot: rows::LiveSyntaxSnapshot,
    unresolved_spans: ResolvedOutputUnresolvedSpans,
) -> HighlightProvider {
    let unresolved_style = resolved_output_unresolved_highlight_style(theme);
    let active_unresolved_style = resolved_output_active_unresolved_highlight_style(theme);
    HighlightProvider::with_pending(
        move |byte_range: Range<usize>| HighlightProviderResult {
            highlights: apply_resolved_output_unresolved_highlights(
                snapshot.highlights_for_byte_range(byte_range.clone()),
                &unresolved_spans,
                byte_range,
                unresolved_style,
                active_unresolved_style,
            ),
            pending: false,
        },
        || 0,
        || false,
    )
}

/// Fold a batch of edits into the single `(replaced, inserted)` span that covers
/// them all, in the coordinates `LiveSyntaxDocument::sync` expects.
///
/// Each delta is expressed against the buffer as it stood when that delta was
/// applied, and only the final line starts survive to this point, so translating
/// them individually would compute positions against the wrong text. One wider
/// edit is always sound — it just reparses a little more than strictly needed —
/// and GPUI coalesces notifications, so in practice the batch is one delta.
///
/// Mirrors the union arithmetic in `HighlightInterpolation::record_edit`.
pub(in crate::view) fn coalesce_resolved_output_edit_deltas(
    deltas: &[(Range<usize>, Range<usize>)],
) -> Option<(Range<usize>, Range<usize>)> {
    let mut folded: Option<(usize, usize, usize)> = None; // (start, old_len, new_len)
    for (replaced, inserted) in deltas {
        folded = Some(match folded {
            None => (
                replaced.start,
                replaced.end.saturating_sub(replaced.start),
                inserted.end.saturating_sub(inserted.start),
            ),
            Some((start, old_len, new_len)) => {
                let union_start = start.min(replaced.start);
                let union_right = start.saturating_add(new_len).max(replaced.end);
                let source_right = union_right - new_len + old_len;
                let live_right =
                    union_right - (replaced.end - replaced.start) + (inserted.end - inserted.start);
                (
                    union_start,
                    source_right.saturating_sub(union_start),
                    live_right.saturating_sub(union_start),
                )
            }
        });
    }
    folded.map(|(start, old_len, new_len)| (start..start + old_len, start..start + new_len))
}

pub(in crate::view) fn line_index_for_offset(content: &str, offset: usize) -> usize {
    content[..offset.min(content.len())].matches('\n').count()
}

pub(in crate::view) fn conflict_resolver_output_context_line(
    content: &str,
    cursor_offset: usize,
    clicked_offset: Option<usize>,
) -> usize {
    clicked_offset
        .map(|offset| line_index_for_offset(content, offset))
        .unwrap_or_else(|| line_index_for_offset(content, cursor_offset))
}

pub(in crate::view) fn slice_text_by_line_range(text: &str, line_range: Range<usize>) -> String {
    if line_range.start >= line_range.end || text.is_empty() {
        return String::new();
    }

    let line_starts = build_line_starts(text);

    let start_byte = line_starts
        .get(line_range.start)
        .copied()
        .unwrap_or(text.len());
    let end_byte = line_starts
        .get(line_range.end)
        .copied()
        .unwrap_or(text.len());
    if start_byte >= end_byte || start_byte >= text.len() {
        return String::new();
    }
    text[start_byte..end_byte.min(text.len())].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_line_count_returns_zero_for_empty_text() {
        assert_eq!(indexed_line_count("", &[]), 0);
    }

    #[test]
    fn indexed_line_count_matches_nonempty_line_starts() {
        let text = "alpha\nbeta";
        let (line_starts, line_count) = build_line_starts_with_count(text);

        assert_eq!(line_count, 2);
        assert_eq!(indexed_line_count(text, &line_starts), 2);
    }

    #[test]
    fn indexed_line_count_preserves_trailing_empty_row() {
        let text = "alpha\nbeta\n";
        let (line_starts, line_count) = build_line_starts_with_count(text);

        assert_eq!(line_count, 3);
        assert_eq!(line_starts, vec![0, 6, 11]);
        assert_eq!(indexed_line_count(text, &line_starts), 3);
    }

    #[test]
    fn resolved_outline_provenance_skip_thresholds_match_view_mode() {
        assert!(!should_skip_resolved_outline_provenance(
            ConflictResolverViewMode::ThreeWay,
            LARGE_RESOLVED_OUTLINE_THREE_WAY_PROVENANCE_MAX_LINES,
        ));
        assert!(should_skip_resolved_outline_provenance(
            ConflictResolverViewMode::ThreeWay,
            LARGE_RESOLVED_OUTLINE_THREE_WAY_PROVENANCE_MAX_LINES + 1,
        ));
        assert!(!should_skip_resolved_outline_provenance(
            ConflictResolverViewMode::TwoWayDiff,
            LARGE_RESOLVED_OUTLINE_TWO_WAY_PROVENANCE_MAX_LINES,
        ));
        assert!(should_skip_resolved_outline_provenance(
            ConflictResolverViewMode::TwoWayDiff,
            LARGE_RESOLVED_OUTLINE_TWO_WAY_PROVENANCE_MAX_LINES + 1,
        ));
    }
}
