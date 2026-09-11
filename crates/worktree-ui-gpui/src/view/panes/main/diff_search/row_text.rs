//! Shared small helpers for normalising row text and folding bytes.

use super::consts::FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES;

#[cfg(test)]
use super::conflict::ConflictResolverSearchTwoWayRows;
use crate::kit::text_model::TextModelSnapshot;
use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::kit::text_search::DiffSearchMatcher;
#[cfg(test)]
use crate::view::conflict_resolver;
use std::ops::Range;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffSearchFinalizeMode {
    ScrollToFirst,
    PreserveCurrent {
        previous_match_ix: Option<usize>,
        previous_visible_ix: Option<usize>,
    },
}

impl DiffSearchFinalizeMode {
    pub(super) fn preserve_current(
        previous_match_ix: Option<usize>,
        previous_visible_ix: Option<usize>,
    ) -> Self {
        Self::PreserveCurrent {
            previous_match_ix,
            previous_visible_ix,
        }
    }
}

pub(in crate::view) enum DiffSearchVisibleCandidates<'a> {
    All,
    Indexed(&'a [u32]),
    None,
}

#[inline]
pub(super) fn diff_search_displayed_text_matches_query(
    query: AsciiCaseInsensitiveNeedle<'_>,
    text: &str,
    expanded_tabs: &mut String,
) -> bool {
    if !text.contains('\t') {
        return query.is_match(text);
    }

    expanded_tabs.clear();
    for ch in text.chars() {
        match ch {
            '\t' => expanded_tabs.push_str("    "),
            _ => expanded_tabs.push(ch),
        }
    }
    query.is_match(expanded_tabs.as_str())
}

pub(super) fn diff_search_resume_match_ix(
    previous_visible_ix: Option<usize>,
    matches: &[usize],
) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }

    let Some(previous_visible_ix) = previous_visible_ix else {
        return Some(0);
    };

    match matches.binary_search(&previous_visible_ix) {
        Ok(ix) => Some(ix),
        Err(insert_ix) => Some(insert_ix.checked_sub(1).unwrap_or(matches.len() - 1)),
    }
}

pub(super) fn resolved_output_line_ix_matches_query(
    raw_text: &worktree_core::file_diff::FileDiffLineText,
    query: AsciiCaseInsensitiveNeedle<'_>,
) -> bool {
    if raw_text.len() <= FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES {
        return query.is_match(raw_text.as_ref());
    }

    let overlap = query.as_bytes().len().saturating_sub(1);
    let mut chunk_start = 0usize;
    while chunk_start < raw_text.len() {
        let scan_start = chunk_start.saturating_sub(overlap);
        let scan_end = chunk_start
            .saturating_add(FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES)
            .min(raw_text.len());
        let slice = raw_text
            .slice_text(scan_start..scan_end)
            .unwrap_or_default();
        if query.is_match(slice.as_ref()) {
            return true;
        }
        if scan_end >= raw_text.len() {
            break;
        }
        chunk_start = scan_end;
    }

    false
}

pub(super) fn retain_refined_visible_matches(
    matches: &mut Vec<usize>,
    candidates: DiffSearchVisibleCandidates<'_>,
    mut visible_ix_matches_query: impl FnMut(usize) -> bool,
) {
    match candidates {
        DiffSearchVisibleCandidates::None => {
            matches.clear();
        }
        DiffSearchVisibleCandidates::All => {
            matches.retain(|&visible_ix| visible_ix_matches_query(visible_ix));
        }
        DiffSearchVisibleCandidates::Indexed(candidate_visible_rows) => {
            if candidate_visible_rows.len() >= matches.len() {
                matches.retain(|&visible_ix| visible_ix_matches_query(visible_ix));
                return;
            }

            let mut read_ix = 0usize;
            let mut write_ix = 0usize;
            let mut candidate_ix = 0usize;

            while read_ix < matches.len() && candidate_ix < candidate_visible_rows.len() {
                let visible_ix = matches[read_ix];
                let candidate_visible_ix = candidate_visible_rows[candidate_ix] as usize;
                if visible_ix < candidate_visible_ix {
                    read_ix += 1;
                    continue;
                }
                if visible_ix > candidate_visible_ix {
                    candidate_ix += 1;
                    continue;
                }

                if visible_ix_matches_query(visible_ix) {
                    matches[write_ix] = visible_ix;
                    write_ix += 1;
                }
                read_ix += 1;
                candidate_ix += 1;
            }

            matches.truncate(write_ix);
        }
    }
}

pub(super) fn normalized_stream_row_text(row_text: &str) -> &str {
    row_text.strip_suffix('\r').unwrap_or(row_text)
}

pub(super) fn normalized_file_diff_line_text_len(
    raw_text: &worktree_core::file_diff::FileDiffLineText,
) -> usize {
    let len = raw_text.len();
    if len == 0 {
        return 0;
    }

    if raw_text
        .slice_bytes(len - 1..len)
        .is_some_and(|bytes| bytes.as_ref() == b"\r")
    {
        len - 1
    } else {
        len
    }
}

#[inline]
pub(super) fn folded_search_byte(byte: u8, match_case: bool) -> u8 {
    if match_case {
        byte
    } else {
        byte.to_ascii_lowercase()
    }
}

pub(super) fn build_literal_search_prefix_table(needle: &[u8]) -> Vec<usize> {
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0usize;

    for ix in 1..needle.len() {
        while matched > 0 && needle[ix] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[ix] == needle[matched] {
            matched += 1;
            prefix[ix] = matched;
        }
    }

    prefix
}

pub(super) fn visible_ix_for_stream_abs(
    row_starts: &[(usize, usize)],
    stream_abs: usize,
) -> Option<usize> {
    if row_starts.is_empty() {
        return None;
    }

    let row_ix = match row_starts.binary_search_by_key(&stream_abs, |(start, _)| *start) {
        Ok(ix) => ix,
        Err(ix) => ix.saturating_sub(1),
    };
    row_starts.get(row_ix).map(|(_, visible_ix)| *visible_ix)
}

pub(super) fn next_row_start_after_stream_abs(
    row_starts: &[(usize, usize)],
    stream_abs: usize,
) -> Option<usize> {
    let ix = row_starts.partition_point(|(start, _)| *start <= stream_abs);
    row_starts.get(ix).map(|(start, _)| *start)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StreamMatchCollectionMode {
    SingleRowLiteral,
    MaterializedRows,
}

/// Byte ranges of every occurrence of `matcher` in `snapshot`, capped at
/// `max_matches`.
///
/// A line at a time, so the rope is never flattened. The split costs nothing in
/// correctness: a line edge already reads as a non-word character for
/// `whole_word`, and the matcher builds its regex with `multi_line(true)`, so
/// `^`/`$` mean line anchors either way. A query containing a newline is the one
/// shape this cannot answer, and the only path that flattens.
pub(in crate::view) fn file_editor_search_ranges(
    snapshot: &TextModelSnapshot,
    matcher: &DiffSearchMatcher,
    max_matches: usize,
) -> Vec<Range<usize>> {
    let mut found: Vec<Range<usize>> = Vec::new();
    if matcher.is_empty() || matcher.regex_error().is_some() || max_matches == 0 {
        return found;
    }

    if matcher.query().contains('\n') {
        matcher.find_ranges_into(
            snapshot.slice(0..snapshot.len()).as_ref(),
            &mut found,
            max_matches,
        );
        return found;
    }

    let mut line_matches: Vec<Range<usize>> = Vec::new();
    for row in 0..snapshot.line_count() {
        let remaining = max_matches - found.len();
        if remaining == 0 {
            break;
        }
        let line_range = snapshot.line_range(row);
        let line = snapshot.slice(line_range.clone());
        matcher.find_ranges_into(line.as_ref(), &mut line_matches, remaining);
        found.extend(
            line_matches
                .iter()
                .map(|range| line_range.start + range.start..line_range.start + range.end),
        );
    }
    found
}

#[cfg(test)]
pub(super) fn contains_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    match AsciiCaseInsensitiveNeedle::new(needle) {
        Some(needle) => needle.is_match(haystack),
        None => true,
    }
}

#[cfg(test)]
pub(super) fn empty_conflict_resolver_search_two_way_rows()
-> ConflictResolverSearchTwoWayRows<'static> {
    static EMPTY_INDEX: std::sync::LazyLock<conflict_resolver::ConflictSplitRowIndex> =
        std::sync::LazyLock::new(conflict_resolver::ConflictSplitRowIndex::default);
    static EMPTY_PROJECTION: std::sync::LazyLock<conflict_resolver::TwoWaySplitProjection> =
        std::sync::LazyLock::new(conflict_resolver::TwoWaySplitProjection::default);
    ConflictResolverSearchTwoWayRows::Streamed {
        split_row_index: &EMPTY_INDEX,
        two_way_split_projection: &EMPTY_PROJECTION,
    }
}

/// Line `line_ix` of `text` without its trailing newline, shared by the
/// three-way search walks below (the needle and matcher variants) and their
/// tests. Out-of-range indices read as empty so padding rows never match.
pub(super) fn line_text<'a>(text: &'a str, line_starts: &[usize], line_ix: usize) -> &'a str {
    if text.is_empty() {
        return "";
    }
    let text_len = text.len();
    let start = line_starts.get(line_ix).copied().unwrap_or(text_len);
    if start >= text_len {
        return "";
    }
    let mut end = line_starts
        .get(line_ix.saturating_add(1))
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    if end > start && text.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n') {
        end = end.saturating_sub(1);
    }
    text.get(start..end).unwrap_or("")
}
