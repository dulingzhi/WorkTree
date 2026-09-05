//! The editable resolved output: byte-ownership block maps, placeholder
//! rows, fragment/span projections and the rope-friendly source trait.

use super::*;
use std::ops::Range;
use std::sync::Arc;

/// Byte ownership for the editable output of each displayed conflict block.
///
/// The editor text is authoritative; these ranges only decide which bytes
/// belong to which conflict when deriving session resolutions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedOutputBlockMap {
    ranges: Vec<Range<usize>>,
    pub(super) text_len: usize,
}

impl ResolvedOutputBlockMap {
    pub(in crate::view) fn from_segments(segments: &[ConflictSegment]) -> Self {
        let mut ranges = Vec::with_capacity(conflict_count(segments));
        let mut cursor = 0usize;
        for segment in segments {
            match segment {
                ConflictSegment::Text(text) => {
                    cursor = cursor.saturating_add(text.len());
                }
                ConflictSegment::Block(block) => {
                    let start = cursor;
                    cursor = cursor.saturating_add(editable_conflict_block_len(block));
                    ranges.push(start..cursor);
                }
            }
        }
        Self {
            ranges,
            text_len: cursor,
        }
    }

    pub(in crate::view) fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    pub(in crate::view) fn is_valid_for(
        &self,
        segments: &[ConflictSegment],
        output_text: &(impl ResolvedOutputSource + ?Sized),
    ) -> bool {
        self.text_len == output_text.len()
            && self.ranges.len() == conflict_count(segments)
            && self.ranges.iter().all(|range| {
                range.start <= range.end
                    && range.end <= output_text.len()
                    && output_text.is_char_boundary(range.start)
                    && output_text.is_char_boundary(range.end)
            })
            && self
                .ranges
                .windows(2)
                .all(|ranges| ranges[0].end <= ranges[1].start)
    }

    pub(in crate::view) fn block_slice<'a>(
        &self,
        segments: &[ConflictSegment],
        output_text: &'a str,
        block_index: usize,
    ) -> Option<&'a str> {
        self.is_valid_for(segments, output_text).then_some(())?;
        output_text.get(self.ranges.get(block_index)?.clone())
    }

    fn owner_for_edit_start(&self, start: usize) -> Option<usize> {
        self.ranges
            .iter()
            .position(|range| range.start <= start && start < range.end)
            .or_else(|| {
                self.ranges
                    .iter()
                    .position(|range| range.is_empty() && range.start == start)
            })
    }

    pub(in crate::view) fn apply_edit_delta(
        &mut self,
        old_range: Range<usize>,
        new_range: Range<usize>,
    ) -> bool {
        if old_range.start > old_range.end
            || old_range.end > self.text_len
            || new_range.start != old_range.start
            || new_range.start > new_range.end
        {
            return false;
        }

        let owner = self.owner_for_edit_start(old_range.start);
        let shift = new_range.len() as isize - old_range.len() as isize;
        let shift_offset = |offset: usize| {
            if shift >= 0 {
                offset.saturating_add(shift as usize)
            } else {
                offset.saturating_sub((-shift) as usize)
            }
        };

        for (block_index, range) in self.ranges.iter_mut().enumerate() {
            let previous = range.clone();
            if owner == Some(block_index) {
                let start = if previous.start < old_range.start {
                    previous.start
                } else {
                    old_range.start
                };
                let end = if previous.end > old_range.end {
                    shift_offset(previous.end)
                } else {
                    new_range.end
                };
                *range = start..end.max(start);
                continue;
            }

            if previous.end <= old_range.start {
                continue;
            }
            if previous.start >= old_range.end {
                *range = shift_offset(previous.start)..shift_offset(previous.end);
                continue;
            }

            let keeps_left = previous.start < old_range.start;
            let keeps_right = previous.end > old_range.end;
            *range = match (keeps_left, keeps_right) {
                (true, true) => previous.start..shift_offset(previous.end),
                (true, false) => previous.start..old_range.start,
                (false, true) => new_range.end..shift_offset(previous.end),
                // A later block deleted by an edit owned by an earlier block
                // collapses after the replacement, never into the owner's text.
                (false, false) => new_range.end..new_range.end,
            };
        }

        self.text_len = shift_offset(self.text_len);
        self.ranges
            .windows(2)
            .all(|ranges| ranges[0].end <= ranges[1].start)
            && self
                .ranges
                .iter()
                .all(|range| range.start <= range.end && range.end <= self.text_len)
    }

    pub(in crate::view) fn apply_edit_deltas(
        &mut self,
        deltas: impl IntoIterator<Item = (Range<usize>, Range<usize>)>,
    ) -> bool {
        for (old_range, new_range) in deltas {
            if !self.apply_edit_delta(old_range, new_range) {
                return false;
            }
        }
        true
    }
}

/// KDiff3-compatible text shown for a merge block with no selected sources.
pub(in crate::view) const UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER: &str = "<Merge Conflict>";

/// Same, for a block whose sides differ only in whitespace.
pub(in crate::view) const UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER: &str =
    "<Merge Conflict (Whitespace only)>";

const UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_LF: &str = "<Merge Conflict>\n";
const UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_CRLF: &str = "<Merge Conflict>\r\n";
const UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_CR: &str = "<Merge Conflict>\r";

const UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_LF: &str = "<Merge Conflict (Whitespace only)>\n";
const UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_CRLF: &str =
    "<Merge Conflict (Whitespace only)>\r\n";
const UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_CR: &str = "<Merge Conflict (Whitespace only)>\r";

/// Whether one output line is an unresolved-conflict placeholder row.
///
/// The resolved output is a text document, so a placeholder row is identified
/// by its own content — the same fact the reader sees. This keeps the gutter
/// marker in step with the text however the marker array was built, mirroring
/// how kdiff3 derives both from a single `srcSelect`.
pub(in crate::view) fn line_is_unresolved_conflict_placeholder(line: &str) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    line == UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER
        || line == UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER
}

/// Whether this block renders as a placeholder row rather than as text.
///
/// An unresolved block with no side picked has nothing to show, so the output
/// carries one `<Merge Conflict>` row in its place.
fn uses_unresolved_merge_conflict_placeholder(block: &ConflictBlock) -> bool {
    !block.resolved && block.choice.is_empty()
}

/// The single `<Merge Conflict>` row an unresolved block occupies, kdiff3's
/// `MergeBlockList::buildFromDiff3` (one `MergeEditLine` per block).
///
/// A block that spans several aligned source rows still collapses to this one
/// row, so the output's line numbers count the output's own lines rather than
/// the source columns'.
fn unresolved_merge_conflict_placeholder_text(block: &ConflictBlock) -> &'static str {
    use worktree_core::conflict_output::{
        ConflictOutputBlockRef, detect_conflict_block_line_ending,
    };

    let line_ending = detect_conflict_block_line_ending(ConflictOutputBlockRef {
        base: block.base.as_deref(),
        ours: &block.ours,
        theirs: &block.theirs,
        choice: block.choice,
        resolved: block.resolved,
    });
    // kdiff3 mergeresultwindow.cpp: a block whose sides differ only in
    // whitespace names itself, so the trivial ones can be told apart from real
    // clashes without opening them.
    if block.whitespace_only {
        return match line_ending {
            "\r\n" => UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_CRLF,
            "\r" => UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_CR,
            _ => UNRESOLVED_WHITESPACE_CONFLICT_PLACEHOLDER_LF,
        };
    }
    match line_ending {
        "\r\n" => UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_CRLF,
        "\r" => UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_CR,
        _ => UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER_LF,
    }
}

fn editable_conflict_block_len(block: &ConflictBlock) -> usize {
    use worktree_core::conflict_output::ConflictOutputSource;

    if uses_unresolved_merge_conflict_placeholder(block) {
        return unresolved_merge_conflict_placeholder_text(block).len();
    }
    block.choice.iter().fold(0usize, |len, source| {
        len.saturating_add(match source {
            ConflictOutputSource::Base => block.base.as_ref().map_or(0, ConflictText::len),
            ConflictOutputSource::Ours => block.ours.len(),
            ConflictOutputSource::Theirs => block.theirs.len(),
        })
    })
}

/// Generate the editable merge-output projection.
///
/// A truly unresolved block has no selected sources and occupies one named
/// KDiff3-style placeholder row, however many aligned rows it spans in the
/// source columns. Resolved blocks retain their ordered source selection, while
/// marker-preserving save/export use [`generate_resolved_text_with_options`]
/// directly.
pub fn generate_resolved_text(segments: &[ConflictSegment]) -> String {
    use worktree_core::conflict_output::ConflictOutputSource;

    let mut output = String::new();
    for segment in segments {
        match segment {
            ConflictSegment::Text(text) => output.push_str(text),
            ConflictSegment::Block(block) if uses_unresolved_merge_conflict_placeholder(block) => {
                output.push_str(unresolved_merge_conflict_placeholder_text(block));
            }
            ConflictSegment::Block(block) => {
                for source in block.choice.iter() {
                    match source {
                        ConflictOutputSource::Base => {
                            if let Some(base) = block.base.as_deref() {
                                output.push_str(base);
                            }
                        }
                        ConflictOutputSource::Ours => output.push_str(&block.ours),
                        ConflictOutputSource::Theirs => output.push_str(&block.theirs),
                    }
                }
            }
        }
    }
    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedOutputText {
    Shared(Arc<str>),
    Owned(String),
}

impl ResolvedOutputText {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Shared(text) => text.as_ref(),
            Self::Owned(text) => text.as_str(),
        }
    }

    pub fn line_count(&self) -> usize {
        text_line_count_usize(self.as_str())
    }

    pub fn into_shared_string(self) -> gpui::SharedString {
        match self {
            Self::Shared(text) => text.into(),
            Self::Owned(text) => text.into(),
        }
    }
}

pub fn bootstrap_resolved_output_text(
    segments: &[ConflictSegment],
    current_text: Option<&Arc<str>>,
    ours_text: Option<&Arc<str>>,
    theirs_text: Option<&Arc<str>>,
) -> ResolvedOutputText {
    if segments.is_empty() {
        return current_text
            .or(ours_text)
            .or(theirs_text)
            .cloned()
            .map(ResolvedOutputText::Shared)
            .unwrap_or_else(|| ResolvedOutputText::Owned(String::new()));
    }

    ResolvedOutputText::Owned(generate_resolved_text(segments))
}

pub fn generate_resolved_text_with_options(
    segments: &[ConflictSegment],
    options: worktree_core::conflict_output::GenerateResolvedTextOptions<'_>,
) -> String {
    use worktree_core::conflict_output::{
        ConflictOutputBlockRef, ConflictOutputSegmentRef,
        generate_resolved_text as generate_core_resolved_text,
    };

    let core_segments: Vec<ConflictOutputSegmentRef<'_>> = segments
        .iter()
        .map(|segment| match segment {
            ConflictSegment::Text(text) => ConflictOutputSegmentRef::Text(text),
            ConflictSegment::Block(block) => {
                ConflictOutputSegmentRef::Block(ConflictOutputBlockRef {
                    base: block.base.as_deref(),
                    ours: &block.ours,
                    theirs: &block.theirs,
                    choice: block.choice,
                    resolved: block.resolved,
                })
            }
        })
        .collect();

    generate_core_resolved_text(&core_segments, options)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ResolvedOutputFragmentSource {
    TextSegment { segment_ix: usize },
    BlockBase { segment_ix: usize },
    BlockOurs { segment_ix: usize },
    BlockTheirs { segment_ix: usize },
    UnresolvedPlaceholder { text: &'static str },
}

fn resolved_output_block_source_fragment(
    segment_ix: usize,
    block: &ConflictBlock,
    source: worktree_core::conflict_output::ConflictOutputSource,
) -> Option<(ResolvedOutputFragmentSource, &str)> {
    use worktree_core::conflict_output::ConflictOutputSource;

    match source {
        ConflictOutputSource::Base => block
            .base
            .as_deref()
            .map(|base| (ResolvedOutputFragmentSource::BlockBase { segment_ix }, base)),
        ConflictOutputSource::Ours => Some((
            ResolvedOutputFragmentSource::BlockOurs { segment_ix },
            block.ours.as_str(),
        )),
        ConflictOutputSource::Theirs => Some((
            ResolvedOutputFragmentSource::BlockTheirs { segment_ix },
            block.theirs.as_str(),
        )),
    }
}

const RESOLVED_OUTPUT_SPARSE_LINE_INDEX_MIN_LINES: usize = LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES;
const RESOLVED_OUTPUT_SPAN_CHECKPOINT_STRIDE: usize = 256;

#[derive(Clone, Debug)]
enum ResolvedOutputFragmentLineIndex {
    SingleLine,
    Dense(Arc<[usize]>),
    Sparse(SparseLineIndex),
}

impl ResolvedOutputFragmentLineIndex {
    fn line_text<'a>(&self, text: &'a str, line_ix: usize) -> Option<&'a str> {
        match self {
            Self::SingleLine => (line_ix == 0).then_some(text.strip_suffix('\n').unwrap_or(text)),
            Self::Dense(line_starts) => {
                Some(line_text_from_starts(text, line_starts.as_ref(), line_ix))
            }
            Self::Sparse(line_index) => line_index.line_text(text, line_ix),
        }
    }

    fn for_each_line_text<'a>(
        &self,
        text: &'a str,
        range: Range<usize>,
        mut visit: impl FnMut(usize, &'a str),
    ) {
        if range.start >= range.end {
            return;
        }

        match self {
            Self::SingleLine => {
                if range.start == 0 {
                    visit(0, text.strip_suffix('\n').unwrap_or(text));
                }
            }
            Self::Dense(line_starts) => {
                let line_starts = line_starts.as_ref();
                for line_ix in range {
                    visit(line_ix, line_text_from_starts(text, line_starts, line_ix));
                }
            }
            Self::Sparse(line_index) => {
                for line_ix in range {
                    if let Some(line) = line_index.line_text(text, line_ix) {
                        visit(line_ix, line);
                    }
                }
            }
        }
    }

    #[cfg(all(test, feature = "benchmarks"))]
    fn metadata_byte_size(&self) -> usize {
        match self {
            Self::SingleLine => 0,
            Self::Dense(line_starts) => line_starts.len() * std::mem::size_of::<usize>(),
            Self::Sparse(line_index) => line_index.metadata_byte_size(),
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedOutputFragment {
    source: ResolvedOutputFragmentSource,
    line_index: ResolvedOutputFragmentLineIndex,
    newline_count: usize,
    ends_with_newline: bool,
    line_count: usize,
    widest_line_ix: usize,
    widest_line_len: usize,
}

impl ResolvedOutputFragment {
    fn source_text<'a>(&self, segments: &'a [ConflictSegment]) -> Option<&'a str> {
        match self.source {
            ResolvedOutputFragmentSource::TextSegment { segment_ix } => {
                match segments.get(segment_ix) {
                    Some(ConflictSegment::Text(text)) => Some(text.as_str()),
                    _ => None,
                }
            }
            ResolvedOutputFragmentSource::BlockBase { segment_ix } => {
                match segments.get(segment_ix) {
                    Some(ConflictSegment::Block(block)) => {
                        Some(block.base.as_deref().unwrap_or(""))
                    }
                    _ => None,
                }
            }
            ResolvedOutputFragmentSource::BlockOurs { segment_ix } => {
                match segments.get(segment_ix) {
                    Some(ConflictSegment::Block(block)) => Some(block.ours.as_str()),
                    _ => None,
                }
            }
            ResolvedOutputFragmentSource::BlockTheirs { segment_ix } => {
                match segments.get(segment_ix) {
                    Some(ConflictSegment::Block(block)) => Some(block.theirs.as_str()),
                    _ => None,
                }
            }
            ResolvedOutputFragmentSource::UnresolvedPlaceholder { text } => Some(text),
        }
    }

    fn line_text<'a>(&self, segments: &'a [ConflictSegment], line_ix: usize) -> Option<&'a str> {
        let text = self.source_text(segments)?;
        if line_ix < self.line_count {
            self.line_index.line_text(text, line_ix)
        } else {
            None
        }
    }

    fn for_each_line_text<'a>(
        &self,
        segments: &'a [ConflictSegment],
        range: Range<usize>,
        visit: impl FnMut(usize, &'a str),
    ) {
        let Some(text) = self.source_text(segments) else {
            return;
        };
        let start = range.start.min(self.line_count);
        let end = range.end.min(self.line_count);
        if start >= end {
            return;
        }
        self.line_index.for_each_line_text(text, start..end, visit);
    }

    fn widest_line(&self) -> Option<(usize, usize)> {
        (self.line_count > 0).then_some((self.widest_line_ix, self.widest_line_len))
    }

    #[cfg(all(test, feature = "benchmarks"))]
    fn metadata_byte_size(&self) -> usize {
        self.line_index.metadata_byte_size()
    }
}

#[derive(Clone, Debug)]
enum ResolvedOutputSpan {
    SourceLines {
        visible_start: usize,
        len: usize,
        fragment_ix: usize,
        fragment_line_start: usize,
    },
    MergedLine {
        visible_index: usize,
        text: String,
    },
}

impl ResolvedOutputSpan {
    fn visible_start(&self) -> usize {
        match self {
            Self::SourceLines { visible_start, .. } => *visible_start,
            Self::MergedLine { visible_index, .. } => *visible_index,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::SourceLines { len, .. } => *len,
            Self::MergedLine { .. } => 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ResolvedOutputProjection {
    fragments: Vec<ResolvedOutputFragment>,
    spans: Vec<ResolvedOutputSpan>,
    span_checkpoints: Vec<usize>,
    conflict_line_ranges: Vec<std::ops::Range<usize>>,
    line_count: usize,
    widest_line_ix: usize,
}

impl ResolvedOutputProjection {
    pub fn from_segments(segments: &[ConflictSegment]) -> Self {
        #[derive(Clone, Debug)]
        enum PendingLine {
            Empty,
            Source {
                fragment_ix: usize,
                line_ix: usize,
                conflict_ix: Option<usize>,
            },
            Composed {
                text: String,
                conflict_ix: Option<usize>,
            },
        }

        impl PendingLine {
            fn conflict_ix(&self) -> Option<usize> {
                match self {
                    Self::Empty => None,
                    Self::Source { conflict_ix, .. } | Self::Composed { conflict_ix, .. } => {
                        *conflict_ix
                    }
                }
            }
        }

        fn dense_line_starts_and_widest_line(text: &str) -> (Arc<[usize]>, usize, usize, usize) {
            let bytes = text.as_bytes();
            let mut starts = Vec::with_capacity(bytes.len().saturating_div(64).saturating_add(1));
            starts.push(0usize);
            let mut line_count = 0usize;
            let mut line_start = 0usize;
            let mut widest_line_ix = 0usize;
            let mut widest_line_len = 0usize;

            for pos in memchr::memchr_iter(b'\n', bytes) {
                let line_len = pos.saturating_sub(line_start);
                if line_len > widest_line_len {
                    widest_line_len = line_len;
                    widest_line_ix = line_count;
                }
                line_count = line_count.saturating_add(1);
                line_start = pos.saturating_add(1);
                starts.push(line_start);
            }

            if line_start < bytes.len() {
                let line_len = bytes.len().saturating_sub(line_start);
                if line_len > widest_line_len {
                    widest_line_len = line_len;
                    widest_line_ix = line_count;
                }
                line_count = line_count.saturating_add(1);
            }

            (starts.into(), line_count, widest_line_ix, widest_line_len)
        }

        fn fragment_line_stats(
            text: &str,
        ) -> (
            ResolvedOutputFragmentLineIndex,
            usize,
            bool,
            usize,
            usize,
            usize,
        ) {
            let bytes = text.as_bytes();
            let ends_with_newline = bytes.last().copied() == Some(b'\n');
            let (line_index, line_count, widest_line_ix, widest_line_len) = if ends_with_newline
                && bytes
                    .iter()
                    .take(bytes.len().saturating_sub(1))
                    .all(|&b| b != b'\n')
            {
                (
                    ResolvedOutputFragmentLineIndex::SingleLine,
                    1,
                    0,
                    bytes.len() - 1,
                )
            } else if !ends_with_newline && bytes.iter().all(|&b| b != b'\n') {
                (
                    ResolvedOutputFragmentLineIndex::SingleLine,
                    1,
                    0,
                    bytes.len(),
                )
            } else {
                let (dense_line_starts, line_count, widest_line_ix, widest_line_len) =
                    dense_line_starts_and_widest_line(text);
                if line_count >= RESOLVED_OUTPUT_SPARSE_LINE_INDEX_MIN_LINES {
                    let line_index = SparseLineIndex::for_text(text);
                    let (widest_line_ix, widest_line_len) =
                        line_index.widest_line().unwrap_or((0, 0));
                    (
                        ResolvedOutputFragmentLineIndex::Sparse(line_index),
                        line_count,
                        widest_line_ix,
                        widest_line_len,
                    )
                } else {
                    (
                        ResolvedOutputFragmentLineIndex::Dense(dense_line_starts),
                        line_count,
                        widest_line_ix,
                        widest_line_len,
                    )
                }
            };
            let newline_count = if ends_with_newline {
                line_count
            } else {
                line_count.saturating_sub(1)
            };
            (
                line_index,
                newline_count,
                ends_with_newline,
                line_count,
                widest_line_ix,
                widest_line_len,
            )
        }

        fn push_source_span(
            spans: &mut Vec<ResolvedOutputSpan>,
            visible_start: usize,
            fragment_ix: usize,
            fragment_line_start: usize,
            len: usize,
        ) {
            if len == 0 {
                return;
            }
            if let Some(ResolvedOutputSpan::SourceLines {
                visible_start: prev_visible_start,
                len: prev_len,
                fragment_ix: prev_fragment_ix,
                fragment_line_start: prev_fragment_line_start,
            }) = spans.last_mut()
                && *prev_fragment_ix == fragment_ix
                && prev_visible_start.saturating_add(*prev_len) == visible_start
                && prev_fragment_line_start.saturating_add(*prev_len) == fragment_line_start
            {
                *prev_len = prev_len.saturating_add(len);
                return;
            }
            spans.push(ResolvedOutputSpan::SourceLines {
                visible_start,
                len,
                fragment_ix,
                fragment_line_start,
            });
        }

        fn push_merged_line(
            spans: &mut Vec<ResolvedOutputSpan>,
            visible_index: usize,
            text: String,
        ) {
            spans.push(ResolvedOutputSpan::MergedLine {
                visible_index,
                text,
            });
        }

        fn merge_conflict_ix(current: Option<usize>, next: Option<usize>) -> Option<usize> {
            match (current, next) {
                (None, other) | (other, None) => other,
                (Some(left), Some(right)) => {
                    debug_assert_eq!(
                        left, right,
                        "resolved output line should not span multiple conflict blocks"
                    );
                    Some(left)
                }
            }
        }

        fn build_span_checkpoints(spans: &[ResolvedOutputSpan], line_count: usize) -> Vec<usize> {
            if spans.is_empty() || line_count == 0 {
                return Vec::new();
            }

            let checkpoint_count = line_count
                .saturating_add(RESOLVED_OUTPUT_SPAN_CHECKPOINT_STRIDE - 1)
                / RESOLVED_OUTPUT_SPAN_CHECKPOINT_STRIDE;
            let mut checkpoints = Vec::with_capacity(checkpoint_count);
            let mut span_ix = 0usize;

            for checkpoint_ix in 0..checkpoint_count {
                let visible_line = checkpoint_ix * RESOLVED_OUTPUT_SPAN_CHECKPOINT_STRIDE;
                while span_ix + 1 < spans.len()
                    && spans[span_ix]
                        .visible_start()
                        .saturating_add(spans[span_ix].len())
                        <= visible_line
                {
                    span_ix = span_ix.saturating_add(1);
                }
                checkpoints.push(span_ix);
            }

            checkpoints
        }

        fn extend_conflict_line_range(
            ranges: &mut [Option<std::ops::Range<usize>>],
            conflict_ix: Option<usize>,
            line_ix: usize,
        ) {
            let Some(conflict_ix) = conflict_ix else {
                return;
            };
            let Some(slot) = ranges.get_mut(conflict_ix) else {
                return;
            };
            match slot {
                Some(range) => {
                    range.start = range.start.min(line_ix);
                    range.end = range.end.max(line_ix.saturating_add(1));
                }
                None => {
                    *slot = Some(line_ix..line_ix.saturating_add(1));
                }
            }
        }

        fn finalize_pending_line(
            pending: &mut PendingLine,
            fragments: &[ResolvedOutputFragment],
            segments: &[ConflictSegment],
            spans: &mut Vec<ResolvedOutputSpan>,
            visible_line: &mut usize,
            conflict_ranges: &mut [Option<std::ops::Range<usize>>],
            widest_visible_line: &mut (usize, usize),
        ) {
            let line_conflict = pending.conflict_ix();
            let line_len = match pending {
                PendingLine::Empty => 0,
                PendingLine::Source {
                    fragment_ix,
                    line_ix,
                    ..
                } => fragments
                    .get(*fragment_ix)
                    .and_then(|fragment| fragment.line_text(segments, *line_ix))
                    .map_or(0, str::len),
                PendingLine::Composed { text, .. } => text.len(),
            };
            if line_len > widest_visible_line.1 {
                *widest_visible_line = (*visible_line, line_len);
            }
            match pending {
                PendingLine::Empty => {
                    push_merged_line(spans, *visible_line, String::new());
                }
                PendingLine::Source {
                    fragment_ix,
                    line_ix,
                    ..
                } => {
                    push_source_span(spans, *visible_line, *fragment_ix, *line_ix, 1);
                }
                PendingLine::Composed { text, .. } => {
                    push_merged_line(spans, *visible_line, std::mem::take(text));
                }
            }
            extend_conflict_line_range(conflict_ranges, line_conflict, *visible_line);
            *visible_line = visible_line.saturating_add(1);
            *pending = PendingLine::Empty;
        }

        fn update_widest_from_source_span(
            widest_visible_line: &mut (usize, usize),
            fragments: &[ResolvedOutputFragment],
            visible_start: usize,
            fragment_ix: usize,
            fragment_line_start: usize,
            len: usize,
        ) {
            let Some(fragment) = fragments.get(fragment_ix) else {
                return;
            };
            let Some((widest_line_ix, widest_line_len)) = fragment.widest_line() else {
                return;
            };
            let fragment_line_end = fragment_line_start.saturating_add(len);
            if widest_line_ix < fragment_line_start || widest_line_ix >= fragment_line_end {
                return;
            }

            let visible_ix =
                visible_start.saturating_add(widest_line_ix.saturating_sub(fragment_line_start));
            if widest_line_len > widest_visible_line.1 {
                *widest_visible_line = (visible_ix, widest_line_len);
            }
        }

        fn append_source_piece_to_pending(
            pending: &mut PendingLine,
            fragments: &[ResolvedOutputFragment],
            segments: &[ConflictSegment],
            fragment_ix: usize,
            line_ix: usize,
            conflict_ix: Option<usize>,
        ) {
            let piece_text = fragments
                .get(fragment_ix)
                .and_then(|fragment| fragment.line_text(segments, line_ix))
                .unwrap_or("");
            match pending {
                PendingLine::Empty => {
                    if piece_text.is_empty() {
                        return;
                    }
                    *pending = PendingLine::Source {
                        fragment_ix,
                        line_ix,
                        conflict_ix,
                    };
                }
                PendingLine::Source {
                    fragment_ix: existing_fragment_ix,
                    line_ix: existing_line_ix,
                    conflict_ix: existing_conflict_ix,
                } => {
                    let existing_text = fragments
                        .get(*existing_fragment_ix)
                        .and_then(|fragment| fragment.line_text(segments, *existing_line_ix))
                        .unwrap_or("");
                    let mut composed =
                        String::with_capacity(existing_text.len().saturating_add(piece_text.len()));
                    composed.push_str(existing_text);
                    composed.push_str(piece_text);
                    *pending = PendingLine::Composed {
                        text: composed,
                        conflict_ix: merge_conflict_ix(*existing_conflict_ix, conflict_ix),
                    };
                }
                PendingLine::Composed {
                    text,
                    conflict_ix: existing_conflict_ix,
                } => {
                    text.push_str(piece_text);
                    *existing_conflict_ix = merge_conflict_ix(*existing_conflict_ix, conflict_ix);
                }
            }
        }

        let conflict_total = conflict_count(segments);
        let projected_fragment_count = segments
            .iter()
            .map(|segment| match segment {
                ConflictSegment::Text(text) => usize::from(!text.is_empty()),
                ConflictSegment::Block(block)
                    if uses_unresolved_merge_conflict_placeholder(block) =>
                {
                    1
                }
                ConflictSegment::Block(block) => block
                    .choice
                    .iter()
                    .filter(|source| match source {
                        worktree_core::conflict_output::ConflictOutputSource::Base => {
                            block.base.as_ref().is_some_and(|base| !base.is_empty())
                        }
                        worktree_core::conflict_output::ConflictOutputSource::Ours => {
                            !block.ours.is_empty()
                        }
                        worktree_core::conflict_output::ConflictOutputSource::Theirs => {
                            !block.theirs.is_empty()
                        }
                    })
                    .count(),
            })
            .sum();
        let mut conflict_ranges: Vec<Option<std::ops::Range<usize>>> = vec![None; conflict_total];
        let mut conflict_line_anchors = vec![0usize; conflict_total];
        let mut fragments = Vec::with_capacity(projected_fragment_count);
        let mut spans = Vec::with_capacity(projected_fragment_count.saturating_add(conflict_total));
        let mut pending = PendingLine::Empty;
        let mut visible_line = 0usize;
        let mut block_ix = 0usize;
        let mut widest_visible_line = (0usize, 0usize);

        fn push_fragment(
            fragments: &mut Vec<ResolvedOutputFragment>,
            source: ResolvedOutputFragmentSource,
            text: &str,
        ) -> Option<usize> {
            if text.is_empty() {
                return None;
            }
            let (
                line_index,
                newline_count,
                ends_with_newline,
                line_count,
                widest_line_ix,
                widest_line_len,
            ) = fragment_line_stats(text);
            let fragment_ix = fragments.len();
            fragments.push(ResolvedOutputFragment {
                source,
                line_index,
                newline_count,
                ends_with_newline,
                line_count,
                widest_line_ix,
                widest_line_len,
            });
            Some(fragment_ix)
        }

        for (segment_ix, segment) in segments.iter().enumerate() {
            match segment {
                ConflictSegment::Text(text) => {
                    let Some(fragment_ix) = push_fragment(
                        &mut fragments,
                        ResolvedOutputFragmentSource::TextSegment { segment_ix },
                        text.as_str(),
                    ) else {
                        continue;
                    };
                    let fragment = &fragments[fragment_ix];
                    if fragment.newline_count == 0 {
                        append_source_piece_to_pending(
                            &mut pending,
                            &fragments,
                            segments,
                            fragment_ix,
                            0,
                            None,
                        );
                        continue;
                    }

                    if !matches!(pending, PendingLine::Empty) {
                        append_source_piece_to_pending(
                            &mut pending,
                            &fragments,
                            segments,
                            fragment_ix,
                            0,
                            None,
                        );
                        finalize_pending_line(
                            &mut pending,
                            &fragments,
                            segments,
                            &mut spans,
                            &mut visible_line,
                            &mut conflict_ranges,
                            &mut widest_visible_line,
                        );
                        if fragment.newline_count > 1 {
                            push_source_span(
                                &mut spans,
                                visible_line,
                                fragment_ix,
                                1,
                                fragment.newline_count - 1,
                            );
                            update_widest_from_source_span(
                                &mut widest_visible_line,
                                &fragments,
                                visible_line,
                                fragment_ix,
                                1,
                                fragment.newline_count - 1,
                            );
                            visible_line = visible_line.saturating_add(fragment.newline_count - 1);
                        }
                    } else {
                        push_source_span(
                            &mut spans,
                            visible_line,
                            fragment_ix,
                            0,
                            fragment.newline_count,
                        );
                        update_widest_from_source_span(
                            &mut widest_visible_line,
                            &fragments,
                            visible_line,
                            fragment_ix,
                            0,
                            fragment.newline_count,
                        );
                        visible_line = visible_line.saturating_add(fragment.newline_count);
                    }

                    if !fragment.ends_with_newline {
                        pending = PendingLine::Source {
                            fragment_ix,
                            line_ix: fragment.newline_count,
                            conflict_ix: None,
                        };
                    }
                }
                ConflictSegment::Block(block) => {
                    let conflict_ix = block_ix;
                    block_ix = block_ix.saturating_add(1);
                    if let Some(anchor) = conflict_line_anchors.get_mut(conflict_ix) {
                        *anchor = visible_line;
                    }

                    let fragment_sources: Vec<_> =
                        if uses_unresolved_merge_conflict_placeholder(block) {
                            let text = unresolved_merge_conflict_placeholder_text(block);
                            vec![(
                                ResolvedOutputFragmentSource::UnresolvedPlaceholder { text },
                                text,
                            )]
                        } else {
                            block
                                .choice
                                .iter()
                                .filter_map(|source| {
                                    resolved_output_block_source_fragment(segment_ix, block, source)
                                })
                                .collect()
                        };

                    for (source, text) in fragment_sources {
                        let Some(fragment_ix) = push_fragment(&mut fragments, source, text) else {
                            continue;
                        };
                        let fragment = &fragments[fragment_ix];
                        if fragment.newline_count == 0 {
                            append_source_piece_to_pending(
                                &mut pending,
                                &fragments,
                                segments,
                                fragment_ix,
                                0,
                                Some(conflict_ix),
                            );
                            continue;
                        }

                        if !matches!(pending, PendingLine::Empty) {
                            append_source_piece_to_pending(
                                &mut pending,
                                &fragments,
                                segments,
                                fragment_ix,
                                0,
                                Some(conflict_ix),
                            );
                            finalize_pending_line(
                                &mut pending,
                                &fragments,
                                segments,
                                &mut spans,
                                &mut visible_line,
                                &mut conflict_ranges,
                                &mut widest_visible_line,
                            );
                            if fragment.newline_count > 1 {
                                let middle_len = fragment.newline_count - 1;
                                push_source_span(
                                    &mut spans,
                                    visible_line,
                                    fragment_ix,
                                    1,
                                    middle_len,
                                );
                                update_widest_from_source_span(
                                    &mut widest_visible_line,
                                    &fragments,
                                    visible_line,
                                    fragment_ix,
                                    1,
                                    middle_len,
                                );
                                for offset in 0..middle_len {
                                    extend_conflict_line_range(
                                        &mut conflict_ranges,
                                        Some(conflict_ix),
                                        visible_line.saturating_add(offset),
                                    );
                                }
                                visible_line = visible_line.saturating_add(middle_len);
                            }
                        } else {
                            push_source_span(
                                &mut spans,
                                visible_line,
                                fragment_ix,
                                0,
                                fragment.newline_count,
                            );
                            update_widest_from_source_span(
                                &mut widest_visible_line,
                                &fragments,
                                visible_line,
                                fragment_ix,
                                0,
                                fragment.newline_count,
                            );
                            for offset in 0..fragment.newline_count {
                                extend_conflict_line_range(
                                    &mut conflict_ranges,
                                    Some(conflict_ix),
                                    visible_line.saturating_add(offset),
                                );
                            }
                            visible_line = visible_line.saturating_add(fragment.newline_count);
                        }

                        if !fragment.ends_with_newline {
                            pending = PendingLine::Source {
                                fragment_ix,
                                line_ix: fragment.newline_count,
                                conflict_ix: Some(conflict_ix),
                            };
                        }
                    }
                }
            }
        }

        finalize_pending_line(
            &mut pending,
            &fragments,
            segments,
            &mut spans,
            &mut visible_line,
            &mut conflict_ranges,
            &mut widest_visible_line,
        );

        let conflict_line_ranges: Vec<std::ops::Range<usize>> = conflict_ranges
            .into_iter()
            .enumerate()
            .map(|(conflict_ix, range)| {
                range.unwrap_or_else(|| {
                    let anchor = conflict_line_anchors
                        .get(conflict_ix)
                        .copied()
                        .unwrap_or_default()
                        .min(visible_line);
                    anchor..anchor
                })
            })
            .collect();
        let line_count = visible_line.max(1);
        let span_checkpoints = build_span_checkpoints(&spans, line_count);

        Self {
            fragments,
            spans,
            span_checkpoints,
            conflict_line_ranges,
            line_count,
            widest_line_ix: widest_visible_line.0,
        }
    }

    pub fn len(&self) -> usize {
        self.line_count
    }

    pub fn widest_line_ix(&self) -> usize {
        self.widest_line_ix
    }

    /// Approximate heap bytes used by projection metadata, excluding the
    /// underlying segment texts which are shared with the resolver state.
    #[cfg(all(test, feature = "benchmarks"))]
    pub fn metadata_byte_size(&self) -> usize {
        let fragments = self.fragments.len() * std::mem::size_of::<ResolvedOutputFragment>()
            + self
                .fragments
                .iter()
                .map(ResolvedOutputFragment::metadata_byte_size)
                .sum::<usize>();
        let spans = self.spans.len() * std::mem::size_of::<ResolvedOutputSpan>()
            + self
                .spans
                .iter()
                .map(|span| match span {
                    ResolvedOutputSpan::SourceLines { .. } => 0,
                    ResolvedOutputSpan::MergedLine { text, .. } => text.capacity(),
                })
                .sum::<usize>();
        let span_checkpoints = self.span_checkpoints.len() * std::mem::size_of::<usize>();
        let conflict_ranges =
            self.conflict_line_ranges.len() * std::mem::size_of::<std::ops::Range<usize>>();
        fragments + spans + span_checkpoints + conflict_ranges
    }

    fn span_ix_for_visible_line(&self, line_ix: usize) -> Option<usize> {
        if self.spans.is_empty() || line_ix >= self.line_count {
            return None;
        }

        let checkpoint_ix = line_ix / RESOLVED_OUTPUT_SPAN_CHECKPOINT_STRIDE;
        let mut span_ix = self
            .span_checkpoints
            .get(checkpoint_ix)
            .copied()
            .unwrap_or_default();

        while let Some(span) = self.spans.get(span_ix) {
            let span_start = span.visible_start();
            let span_end = span_start.saturating_add(span.len());
            if line_ix < span_end {
                return (line_ix >= span_start).then_some(span_ix);
            }
            span_ix = span_ix.saturating_add(1);
        }

        None
    }

    pub fn conflict_line_range(&self, conflict_ix: usize) -> Option<std::ops::Range<usize>> {
        self.conflict_line_ranges.get(conflict_ix).cloned()
    }

    pub fn conflict_line_ranges(&self) -> &[std::ops::Range<usize>] {
        self.conflict_line_ranges.as_slice()
    }

    pub fn for_each_line_text_in_range<'a>(
        &'a self,
        segments: &'a [ConflictSegment],
        range: Range<usize>,
        mut visit: impl FnMut(usize, &'a str),
    ) {
        if range.start >= range.end || range.start >= self.line_count || self.spans.is_empty() {
            return;
        }

        let end = range.end.min(self.line_count);
        let mut line_ix = range.start;
        let mut span_ix = match self.span_ix_for_visible_line(line_ix) {
            Some(span_ix) => span_ix,
            None => return,
        };

        while line_ix < end {
            let Some(span) = self.spans.get(span_ix) else {
                break;
            };
            let span_start = span.visible_start();
            let span_end = span_start.saturating_add(span.len());
            if line_ix < span_start {
                line_ix = span_start;
                if line_ix >= end {
                    break;
                }
            }

            let visit_end = end.min(span_end);
            match span {
                ResolvedOutputSpan::SourceLines {
                    visible_start,
                    fragment_ix,
                    fragment_line_start,
                    ..
                } => {
                    let Some(fragment) = self.fragments.get(*fragment_ix) else {
                        break;
                    };
                    let fragment_range_start =
                        fragment_line_start.saturating_add(line_ix.saturating_sub(*visible_start));
                    let fragment_range_end =
                        fragment_range_start.saturating_add(visit_end.saturating_sub(line_ix));
                    fragment.for_each_line_text(
                        segments,
                        fragment_range_start..fragment_range_end,
                        |fragment_line_ix, line| {
                            let visible_ix = visible_start.saturating_add(
                                fragment_line_ix.saturating_sub(*fragment_line_start),
                            );
                            visit(visible_ix, line);
                        },
                    );
                }
                ResolvedOutputSpan::MergedLine {
                    visible_index,
                    text,
                } => {
                    if line_ix == *visible_index {
                        visit(*visible_index, text.as_str());
                    }
                }
            }

            line_ix = visit_end;
            span_ix = span_ix.saturating_add(1);
        }
    }

    pub fn line_text<'a>(
        &'a self,
        segments: &'a [ConflictSegment],
        line_ix: usize,
    ) -> Option<std::borrow::Cow<'a, str>> {
        let span_ix = self.span_ix_for_visible_line(line_ix)?;
        let span = self.spans.get(span_ix)?;
        if line_ix >= span.visible_start().saturating_add(span.len()) {
            return None;
        }
        match span {
            ResolvedOutputSpan::SourceLines {
                visible_start,
                fragment_ix,
                fragment_line_start,
                ..
            } => {
                let fragment = self.fragments.get(*fragment_ix)?;
                let line_ix_in_fragment =
                    fragment_line_start.saturating_add(line_ix.saturating_sub(*visible_start));
                fragment
                    .line_text(segments, line_ix_in_fragment)
                    .map(std::borrow::Cow::Borrowed)
            }
            ResolvedOutputSpan::MergedLine { text, .. } => {
                Some(std::borrow::Cow::Borrowed(text.as_str()))
            }
        }
    }
}

/// What the resolved-output analysis needs to read from the buffer.
///
/// Implemented for `str` and for [`Rope`], so the same code serves callers that
/// already hold a materialized string (a freshly generated output, a test
/// fixture) and the editable buffer, which must not be flattened just to be
/// scanned. Implementing it on `str` rather than `&str` is what keeps every
/// existing caller compiling unchanged.
pub(in crate::view) trait ResolvedOutputSource {
    fn len(&self) -> usize;
    fn is_char_boundary(&self, offset: usize) -> bool;
    /// Whether the text at `offset` begins with `needle`.
    fn starts_with_at(&self, offset: usize, needle: &str) -> bool;
    fn byte_at(&self, offset: usize) -> Option<u8>;
    fn count_newlines_in(&self, range: Range<usize>) -> usize;
    /// Rows, counting the empty one a trailing newline leaves behind.
    fn row_count(&self) -> usize {
        self.count_newlines_in(0..self.len()).saturating_add(1)
    }

    /// Visit every row as `(byte range including its terminator, text)`.
    ///
    /// One pass over the text, so a scan for marker rows costs the document
    /// once rather than a seek per row.
    fn for_each_row_with_terminator(&self, visit: &mut dyn FnMut(Range<usize>, &str));
}

impl ResolvedOutputSource for str {
    fn len(&self) -> usize {
        str::len(self)
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        str::is_char_boundary(self, offset)
    }

    fn starts_with_at(&self, offset: usize, needle: &str) -> bool {
        self.get(offset..)
            .is_some_and(|tail| tail.starts_with(needle))
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.as_bytes().get(offset).copied()
    }

    fn count_newlines_in(&self, range: Range<usize>) -> usize {
        // Clamped rather than `get(range)`, which answers `None` — and so 0 —
        // for a span that runs past the end or starts inside a character. Zero
        // is the dangerous answer: callers feed this into block line ranges, so
        // "no newlines here" silently places conflict markers on the wrong
        // rows. [`Rope`] clamps and counts what is really there; the two
        // implementations are used against the same buffers and have to agree.
        let len = str::len(self);
        let mut start = range.start.min(len);
        let mut end = range.end.min(len).max(start);
        // Widening to character boundaries cannot change the count: a newline
        // is ASCII, so it never sits inside a multi-byte character.
        while start > 0 && !self.is_char_boundary(start) {
            start -= 1;
        }
        while end < len && !self.is_char_boundary(end) {
            end += 1;
        }
        memchr::memchr_iter(b'\n', &self.as_bytes()[start..end]).count()
    }

    fn for_each_row_with_terminator(&self, visit: &mut dyn FnMut(Range<usize>, &str)) {
        let mut start = 0usize;
        for newline in memchr::memchr_iter(b'\n', self.as_bytes()) {
            let end = newline + 1;
            visit(start..end, &self[start..end]);
            start = end;
        }
        if start <= str::len(self) {
            visit(start..str::len(self), &self[start..]);
        }
    }
}

impl ResolvedOutputSource for crate::kit::rope::Rope {
    fn len(&self) -> usize {
        crate::kit::rope::Rope::len(self)
    }

    fn is_char_boundary(&self, offset: usize) -> bool {
        crate::kit::rope::Rope::is_char_boundary(self, offset)
    }

    fn starts_with_at(&self, offset: usize, needle: &str) -> bool {
        let end = offset.saturating_add(needle.len());
        if end > self.len() {
            return false;
        }
        // Compared as bytes, and over a boundary-widened chunk range, because
        // this is asked precisely when the buffer may have diverged from the
        // segments: `offset` or `end` can land inside a multi-byte character
        // the user typed. A `&str` comparison would panic there instead of
        // reporting the mismatch this exists to find.
        let mut rest = needle.as_bytes();
        let mut skip = offset - self.clip_offset(offset, gpui::sum_tree::Bias::Left);
        for chunk in self.chunks_in_range(offset..end) {
            let bytes = chunk.as_bytes();
            let bytes = if skip >= bytes.len() {
                skip -= bytes.len();
                continue;
            } else {
                let tail = &bytes[skip..];
                skip = 0;
                tail
            };
            let take = bytes.len().min(rest.len());
            if rest[..take] != bytes[..take] {
                return false;
            }
            rest = &rest[take..];
            if rest.is_empty() {
                return true;
            }
        }
        rest.is_empty()
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        if offset >= self.len() {
            return None;
        }
        // A byte read is well defined at every offset, including inside a
        // multi-byte character — trimming a `\r` off a row end lands there
        // whenever the row's last character is not ASCII. Widen to the
        // enclosing character and index the bytes, rather than slicing a `&str`
        // at `offset`, which would panic.
        let start = self.clip_offset(offset, gpui::sum_tree::Bias::Left);
        self.chunks_in_range(
            start..self.clip_offset(offset.saturating_add(1), gpui::sum_tree::Bias::Right),
        )
        .next()
        .and_then(|chunk| chunk.as_bytes().get(offset - start).copied())
    }

    fn count_newlines_in(&self, range: Range<usize>) -> usize {
        // Two summary descents rather than a scan.
        let start = self.offset_to_point(range.start).row;
        let end = self.offset_to_point(range.end.max(range.start)).row;
        end.saturating_sub(start) as usize
    }

    fn row_count(&self) -> usize {
        crate::kit::rope::Rope::line_count(self) as usize
    }

    fn for_each_row_with_terminator(&self, visit: &mut dyn FnMut(Range<usize>, &str)) {
        // One walk over the chunks. A row that straddles a chunk boundary is
        // assembled into `carry` — at most one per chunk, so this allocates in
        // proportion to the chunk count, not the row count.
        let mut row_start = 0usize;
        let mut base = 0usize;
        let mut carry = String::new();
        for chunk in self.chunks() {
            let mut search = 0usize;
            while let Some(found) = memchr::memchr(b'\n', &chunk.as_bytes()[search..]) {
                let newline = search + found;
                let row_end = base + newline + 1;
                if carry.is_empty() {
                    visit(row_start..row_end, &chunk[search..=newline]);
                } else {
                    carry.push_str(&chunk[search..=newline]);
                    visit(row_start..row_end, &carry);
                    carry.clear();
                }
                row_start = row_end;
                search = newline + 1;
            }
            if search < chunk.len() {
                carry.push_str(&chunk[search..]);
            }
            base += chunk.len();
        }
        visit(row_start..base, &carry);
    }
}
