//! Resolved-line provenance: classifying output lines as A/B/C/Manual,
//! the packed gutter row, and the dedupe key index for plus-icon gating.

use super::*;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

/// Source provenance for a resolved output line.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResolvedLineSource {
    /// Line matches source A (Base in three-way, Ours in two-way).
    A,
    /// Line matches source B (Ours in three-way, Theirs in two-way).
    B,
    /// Line matches source C (Theirs in three-way; not used in two-way).
    C,
    /// Line was manually edited or does not match any source.
    Manual,
}

impl ResolvedLineSource {
    #[cfg(test)]
    /// Compact single-character label for UI badges.
    pub fn badge_char(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
            Self::C => 'C',
            Self::Manual => 'M',
        }
    }
}

/// Packed per-line gutter state for resolved-output preview rows.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) struct ResolvedOutputGutterRow(u32);

impl ResolvedOutputGutterRow {
    const SOURCE_MASK: u32 = 0b11;
    const SOURCE_A: u32 = 0;
    const SOURCE_B: u32 = 1;
    const SOURCE_C: u32 = 2;
    const SOURCE_MANUAL: u32 = 3;
    const IS_START_FLAG: u32 = 1 << 2;
    const IS_END_FLAG: u32 = 1 << 3;
    const UNRESOLVED_FLAG: u32 = 1 << 4;
    /// The row renders an unresolved-conflict placeholder.
    ///
    /// kdiff3 reads the `?`, the conflict color and the placeholder string off
    /// one field (`srcSelect`, `mergeresultwindow.cpp:1515`), so its gutter can
    /// never disagree with its text. Our marker array is built separately and
    /// can go stale across incremental edits, so this flag lets the row fall
    /// back to the text's own verdict.
    const PLACEHOLDER_FLAG: u32 = 1 << 5;
    const CONFLICT_SHIFT: u32 = 6;
    const CONFLICT_VALUE_MASK: u32 = u32::MAX >> Self::CONFLICT_SHIFT;

    pub(in crate::view) fn new(
        source: ResolvedLineSource,
        marker_conflict_ix: Option<usize>,
        is_start: bool,
        is_end: bool,
        unresolved: bool,
    ) -> Self {
        let mut bits = match source {
            ResolvedLineSource::A => Self::SOURCE_A,
            ResolvedLineSource::B => Self::SOURCE_B,
            ResolvedLineSource::C => Self::SOURCE_C,
            ResolvedLineSource::Manual => Self::SOURCE_MANUAL,
        };

        if let Some(conflict_ix) = marker_conflict_ix {
            if is_start {
                bits |= Self::IS_START_FLAG;
            }
            if is_end {
                bits |= Self::IS_END_FLAG;
            }
            if unresolved {
                bits |= Self::UNRESOLVED_FLAG;
            }
            let encoded_conflict = u32::try_from(conflict_ix)
                .ok()
                .and_then(|ix| ix.checked_add(1))
                .unwrap_or(Self::CONFLICT_VALUE_MASK)
                .min(Self::CONFLICT_VALUE_MASK);
            bits |= encoded_conflict << Self::CONFLICT_SHIFT;
        }

        Self(bits)
    }

    /// Mark the row as rendering an unresolved-conflict placeholder.
    ///
    /// Standing alone it reads as an unresolved marker that both starts and ends
    /// on this row, which is what the fallback needs when the marker array has
    /// no entry for it. A block wider than one row names only its first row, so
    /// where the marker array *does* cover the row it owns the bracket ends —
    /// otherwise a multi-row block would close its bracket on the named row.
    #[inline(always)]
    pub(in crate::view) fn with_unresolved_placeholder(self) -> Self {
        Self(self.0 | Self::PLACEHOLDER_FLAG)
    }

    #[inline(always)]
    fn is_placeholder(self) -> bool {
        (self.0 & Self::PLACEHOLDER_FLAG) != 0
    }

    /// A placeholder row the marker array does not cover — the only case where
    /// the row's own text has to stand in for marker bracket ends.
    #[inline(always)]
    fn is_unmarked_placeholder(self) -> bool {
        self.is_placeholder() && (self.0 >> Self::CONFLICT_SHIFT) == 0
    }

    #[inline(always)]
    pub(in crate::view) fn source(self) -> ResolvedLineSource {
        match self.0 & Self::SOURCE_MASK {
            Self::SOURCE_A => ResolvedLineSource::A,
            Self::SOURCE_B => ResolvedLineSource::B,
            Self::SOURCE_C => ResolvedLineSource::C,
            _ => ResolvedLineSource::Manual,
        }
    }

    #[inline(always)]
    pub(in crate::view) fn badge_char(self) -> char {
        if self.has_marker() && self.unresolved() {
            return '?';
        }
        match self.0 & Self::SOURCE_MASK {
            Self::SOURCE_A => 'A',
            Self::SOURCE_B => 'B',
            Self::SOURCE_C => 'C',
            _ => 'M',
        }
    }

    #[inline(always)]
    pub(in crate::view) fn has_marker(self) -> bool {
        self.is_placeholder() || (self.0 >> Self::CONFLICT_SHIFT) != 0
    }

    #[inline(always)]
    pub(in crate::view) fn marker_conflict_ix(self) -> Option<usize> {
        let encoded_conflict = self.0 >> Self::CONFLICT_SHIFT;
        (encoded_conflict != 0).then(|| (encoded_conflict - 1) as usize)
    }

    #[inline(always)]
    pub(in crate::view) fn is_start(self) -> bool {
        self.is_unmarked_placeholder() || (self.0 & Self::IS_START_FLAG) != 0
    }

    #[inline(always)]
    pub(in crate::view) fn is_end(self) -> bool {
        self.is_unmarked_placeholder() || (self.0 & Self::IS_END_FLAG) != 0
    }

    #[inline(always)]
    pub(in crate::view) fn unresolved(self) -> bool {
        self.is_placeholder() || (self.0 & Self::UNRESOLVED_FLAG) != 0
    }

    #[inline(always)]
    pub(in crate::view) fn manual_without_marker(self) -> bool {
        !self.has_marker() && (self.0 & Self::SOURCE_MASK) == Self::SOURCE_MANUAL
    }
}

impl Default for ResolvedOutputGutterRow {
    fn default() -> Self {
        Self(Self::SOURCE_MANUAL)
    }
}

/// Per-line provenance metadata for the resolved output outline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedLineMeta {
    /// 0-based line index in the resolved output.
    pub output_line: u32,
    /// Which source this line came from (or Manual).
    pub source: ResolvedLineSource,
    /// If source is A/B/C, the 1-based line number in that source pane.
    pub input_line: Option<u32>,
}

/// Key identifying a specific source line for dedupe gating (plus-icon visibility).
///
/// Two source lines with the same key are considered "the same row" for purposes
/// of preventing duplicate insertion into the resolved output.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SourceLineKey {
    pub view_mode: ConflictResolverViewMode,
    pub side: ResolvedLineSource,
    /// 1-based line number in the source pane.
    pub line_no: u32,
    /// Hash of the line's text content for fast equality checks.
    pub content_hash: u64,
}

impl SourceLineKey {
    pub fn new(
        view_mode: ConflictResolverViewMode,
        side: ResolvedLineSource,
        line_no: u32,
        content: &str,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = FxHasher::default();
        content.hash(&mut hasher);
        Self {
            view_mode,
            side,
            line_no,
            content_hash: hasher.finish(),
        }
    }
}

/// Row count for a materialized output. Production reads this off
/// [`ResolvedOutputSource::row_count`] instead, which the rope answers from its
/// summary; this remains for tests and benchmarks that hold a `String`.
#[cfg(any(test, feature = "benchmarks"))]
pub fn resolved_output_outline_line_count(output: &str) -> usize {
    memchr::memchr_iter(b'\n', output.as_bytes())
        .count()
        .saturating_add(1)
}

/// Split resolved output into one logical row per newline for outline rendering.
///
/// Uses `split('\n')` so trailing newlines are preserved as a final empty row.
#[cfg(any(test, feature = "benchmarks"))]
pub fn split_output_lines_for_outline(output: &str) -> Vec<String> {
    let mut lines = Vec::with_capacity(resolved_output_outline_line_count(output));
    lines.extend(output.split('\n').map(str::to_string));
    lines
}

#[cfg(test)]
pub fn append_lines_to_output(output: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return output.to_string();
    }

    let needs_leading_nl = !output.is_empty() && !output.ends_with('\n');
    let extra_len: usize =
        lines.iter().map(|l| l.len()).sum::<usize>() + lines.len() + usize::from(needs_leading_nl);
    let mut out = String::with_capacity(output.len() + extra_len);
    out.push_str(output);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// Provenance mapping: classify resolved output lines as A/B/C/Manual
// ---------------------------------------------------------------------------

/// Source lines from the three input panes, used for provenance matching.
///
/// In three-way mode: A = Base, B = Ours, C = Theirs.
/// In two-way mode: A = Ours (old), B = Theirs (new), C is empty.
#[cfg(any(test, feature = "benchmarks"))]
pub struct SourceLines<'a> {
    pub a: &'a [gpui::SharedString],
    pub b: &'a [gpui::SharedString],
    pub c: &'a [gpui::SharedString],
}

#[cfg(any(test, feature = "benchmarks"))]
fn build_source_line_lookup<'a>(
    sources: &'a SourceLines<'a>,
) -> FxHashMap<&'a str, (ResolvedLineSource, u32)> {
    let mut lookup = FxHashMap::default();

    // Insert in reverse order so duplicates keep the first line number within a side.
    // Later sides overwrite earlier ones to enforce priority A > B > C.
    for (ix, line) in sources.c.iter().enumerate().rev() {
        lookup.insert(
            line.as_ref(),
            (
                ResolvedLineSource::C,
                u32::try_from(ix + 1).unwrap_or(u32::MAX),
            ),
        );
    }
    for (ix, line) in sources.b.iter().enumerate().rev() {
        lookup.insert(
            line.as_ref(),
            (
                ResolvedLineSource::B,
                u32::try_from(ix + 1).unwrap_or(u32::MAX),
            ),
        );
    }
    for (ix, line) in sources.a.iter().enumerate().rev() {
        lookup.insert(
            line.as_ref(),
            (
                ResolvedLineSource::A,
                u32::try_from(ix + 1).unwrap_or(u32::MAX),
            ),
        );
    }

    lookup
}

fn compute_resolved_line_provenance_from_iter<'a>(
    output_lines: impl Iterator<Item = &'a str>,
    lookup: &FxHashMap<&str, (ResolvedLineSource, u32)>,
) -> Vec<ResolvedLineMeta> {
    let mut result = Vec::new();
    for (out_ix, out_line) in output_lines.enumerate() {
        let (source, input_line) = match lookup.get(out_line).copied() {
            Some((src, line_no)) => (src, Some(line_no)),
            None => (ResolvedLineSource::Manual, None),
        };
        result.push(ResolvedLineMeta {
            output_line: out_ix as u32,
            source,
            input_line,
        });
    }
    result
}

/// Compute per-line provenance metadata for the resolved output.
///
/// Each output line is compared (exact text equality) against every source line
/// in A, B, C. The first match found (priority: A, B, C) wins; if none match
/// the line is labeled `Manual`.
#[cfg(any(test, feature = "benchmarks"))]
pub fn compute_resolved_line_provenance(
    output_lines: &[String],
    sources: &SourceLines<'_>,
) -> Vec<ResolvedLineMeta> {
    let lookup = build_source_line_lookup(sources);
    compute_resolved_line_provenance_from_iter(output_lines.iter().map(String::as_str), &lookup)
}

fn insert_indexed_source_lines<'a>(
    lookup: &mut FxHashMap<&'a str, (ResolvedLineSource, u32)>,
    source: ResolvedLineSource,
    text: &'a str,
    line_starts: &[usize],
) {
    let line_count = indexed_line_count(text, line_starts);
    for line_ix in (0..line_count).rev() {
        if let Some(line) = indexed_line_text(text, line_starts, line_ix) {
            lookup.insert(
                line,
                (
                    source,
                    u32::try_from(line_ix.saturating_add(1)).unwrap_or(u32::MAX),
                ),
            );
        }
    }
}

pub fn compute_resolved_line_provenance_from_text_with_indexed_sources(
    output_text: &str,
    a_text: &str,
    a_line_starts: &[usize],
    b_text: &str,
    b_line_starts: &[usize],
    c_text: &str,
    c_line_starts: &[usize],
) -> Vec<ResolvedLineMeta> {
    let mut lookup = FxHashMap::default();
    insert_indexed_source_lines(&mut lookup, ResolvedLineSource::C, c_text, c_line_starts);
    insert_indexed_source_lines(&mut lookup, ResolvedLineSource::B, b_text, b_line_starts);
    insert_indexed_source_lines(&mut lookup, ResolvedLineSource::A, a_text, a_line_starts);
    compute_resolved_line_provenance_from_iter(output_text.split('\n'), &lookup)
}

pub fn compute_resolved_line_provenance_from_text_two_way_indexed_sources(
    output_text: &str,
    ours_text: &str,
    ours_line_starts: &[usize],
    theirs_text: &str,
    theirs_line_starts: &[usize],
) -> Vec<ResolvedLineMeta> {
    let mut lookup = FxHashMap::default();
    insert_indexed_source_lines(
        &mut lookup,
        ResolvedLineSource::B,
        theirs_text,
        theirs_line_starts,
    );
    insert_indexed_source_lines(
        &mut lookup,
        ResolvedLineSource::A,
        ours_text,
        ours_line_starts,
    );
    compute_resolved_line_provenance_from_iter(output_text.split('\n'), &lookup)
}

// ---------------------------------------------------------------------------
// Dedupe key index: tracks which source lines are present in resolved output
// ---------------------------------------------------------------------------

/// Build the set of `SourceLineKey`s currently represented in the resolved output.
///
/// Used to gate the plus-icon: a source row's plus-icon is hidden when its key
/// is already in this set (preventing duplicate insertion).
#[cfg(test)]
pub fn build_resolved_output_line_sources_index(
    meta: &[ResolvedLineMeta],
    output_lines: &[String],
    view_mode: ConflictResolverViewMode,
) -> FxHashSet<SourceLineKey> {
    let mut index = FxHashSet::with_capacity_and_hasher(meta.len(), Default::default());
    for m in meta {
        if m.source == ResolvedLineSource::Manual {
            continue;
        }
        let Some(line_no) = m.input_line else {
            continue;
        };
        let content = output_lines
            .get(m.output_line as usize)
            .map(|s| s.as_str())
            .unwrap_or("");
        index.insert(SourceLineKey::new(view_mode, m.source, line_no, content));
    }
    index
}

pub fn build_resolved_output_line_sources_index_from_text(
    meta: &[ResolvedLineMeta],
    output_text: &str,
    view_mode: ConflictResolverViewMode,
) -> FxHashSet<SourceLineKey> {
    let mut index = FxHashSet::with_capacity_and_hasher(meta.len(), Default::default());
    for (ix, line) in output_text.split('\n').enumerate() {
        let Some(m) = meta.get(ix) else {
            break;
        };
        if m.source == ResolvedLineSource::Manual {
            continue;
        }
        let Some(line_no) = m.input_line else {
            continue;
        };
        index.insert(SourceLineKey::new(view_mode, m.source, line_no, line));
    }
    index
}

/// Check whether a given source line is already present in the resolved output.
///
/// Returns `true` if the source line's key is in the dedupe index — meaning
/// the plus-icon for that row should be hidden.
#[cfg(test)]
pub fn is_source_line_in_output(
    index: &FxHashSet<SourceLineKey>,
    view_mode: ConflictResolverViewMode,
    side: ResolvedLineSource,
    line_no: u32,
    content: &str,
) -> bool {
    let key = SourceLineKey::new(view_mode, side, line_no, content);
    index.contains(&key)
}
