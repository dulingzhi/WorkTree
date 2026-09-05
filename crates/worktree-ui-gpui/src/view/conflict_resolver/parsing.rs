//! Marker-text parsing: segments from conflict markers and ancestor-base
//! population for two-way markers with an available git ancestor.

use super::*;
use std::sync::Arc;

pub fn parse_conflict_markers(text: &str) -> Vec<ConflictSegment> {
    parse_conflict_markers_shared(Arc::<str>::from(text))
}

pub fn parse_conflict_markers_shared(text: Arc<str>) -> Vec<ConflictSegment> {
    worktree_core::conflict_session::parse_conflict_marker_ranges(text.as_ref())
        .into_iter()
        .map(|segment| match segment {
            worktree_core::conflict_session::ParsedConflictSegmentRanges::Text(range) => {
                ConflictSegment::Text(ConflictText::shared_slice(Arc::clone(&text), range))
            }
            worktree_core::conflict_session::ParsedConflictSegmentRanges::Conflict(block) => {
                ConflictSegment::Block(ConflictBlock {
                    base: block
                        .base
                        .map(|range| ConflictText::shared_slice(Arc::clone(&text), range)),
                    ours: ConflictText::shared_slice(Arc::clone(&text), block.ours),
                    theirs: ConflictText::shared_slice(Arc::clone(&text), block.theirs),
                    choice: ConflictChoice::empty(),
                    resolved: false,
                    whitespace_only: false,
                })
            }
        })
        .collect()
}

/// Parse marker segments only when the text plausibly contains conflict
/// markers, returning an empty segment list for clean inputs.
pub fn parse_conflict_markers_shared_nonempty(text: Arc<str>) -> Vec<ConflictSegment> {
    if memchr::memmem::find(text.as_bytes(), b"<<<<<<<").is_none() {
        return Vec::new();
    }

    let segments = parse_conflict_markers_shared(text);
    if conflict_count(&segments) == 0 {
        Vec::new()
    } else {
        segments
    }
}

pub(super) fn append_text_segment(
    segments: &mut Vec<ConflictSegment>,
    text: impl Into<ConflictText>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(ConflictSegment::Text(prev)) = segments.last_mut() {
        prev.push_str(text.as_str());
        return;
    }
    segments.push(ConflictSegment::Text(text));
}

/// When conflict markers use 2-way style (no `|||||||` base section), `block.base`
/// will be `None` even though the git ancestor content (index stage :1:) is available.
/// This function populates `block.base` by using the Text segments as anchors to
/// locate the corresponding base content in the ancestor file.
fn populate_block_bases_from_ancestor_impl(
    segments: &mut [ConflictSegment],
    ancestor_text: &str,
    shared_ancestor_text: Option<&Arc<str>>,
) {
    if ancestor_text.is_empty() {
        return;
    }
    let any_missing = segments
        .iter()
        .any(|s| matches!(s, ConflictSegment::Block(b) if b.base.is_none()));
    if !any_missing {
        return;
    }

    // Find each Text segment's byte position in the ancestor file.
    // Text segments are the non-conflicting parts that exist in all three versions.
    let mut text_byte_ranges: Vec<std::ops::Range<usize>> =
        Vec::with_capacity(segments.len().saturating_add(1) / 2);
    let mut cursor = 0usize;
    for seg in segments.iter() {
        if let ConflictSegment::Text(text) = seg {
            if let Some(rel) = ancestor_text[cursor..].find(text.as_str()) {
                let start = cursor + rel;
                let end = start + text.len();
                text_byte_ranges.push(start..end);
                cursor = end;
            } else {
                // Text not found in ancestor – bail out.
                return;
            }
        }
    }

    // Extract base content for each block from the gaps between text positions.
    let mut text_idx = 0usize;
    let mut prev_end = 0usize;
    for seg in segments.iter_mut() {
        match seg {
            ConflictSegment::Text(_) => {
                prev_end = text_byte_ranges[text_idx].end;
                text_idx += 1;
            }
            ConflictSegment::Block(block) => {
                if block.base.is_some() {
                    continue;
                }
                let next_start = text_byte_ranges
                    .get(text_idx)
                    .map(|r| r.start)
                    .unwrap_or(ancestor_text.len());
                block.base = Some(if let Some(shared_ancestor_text) = shared_ancestor_text {
                    ConflictText::shared_slice(
                        Arc::clone(shared_ancestor_text),
                        prev_end..next_start,
                    )
                } else {
                    ancestor_text[prev_end..next_start].to_string().into()
                });
            }
        }
    }
}

#[cfg(test)]
pub fn populate_block_bases_from_ancestor(segments: &mut [ConflictSegment], ancestor_text: &str) {
    populate_block_bases_from_ancestor_impl(segments, ancestor_text, None);
}

pub fn populate_block_bases_from_shared_ancestor(
    segments: &mut [ConflictSegment],
    ancestor_text: Arc<str>,
) {
    populate_block_bases_from_ancestor_impl(segments, ancestor_text.as_ref(), Some(&ancestor_text));
}

/// Check whether the given text still contains a complete git conflict-marker
/// block. Marker-looking content on its own (for example a Markdown `=======`
/// Setext underline) is not enough to block Save.
pub fn text_contains_conflict_markers(text: &str) -> bool {
    #[derive(Clone, Copy)]
    enum MarkerState {
        Outside,
        Ours,
        Theirs,
    }

    let mut state = MarkerState::Outside;
    for line in text.lines() {
        if line.starts_with("<<<<<<<") {
            state = MarkerState::Ours;
            continue;
        }
        state = match (state, line) {
            (MarkerState::Ours, line) if line.starts_with("=======") => MarkerState::Theirs,
            (MarkerState::Theirs, line) if line.starts_with(">>>>>>>") => return true,
            (current, _) => current,
        };
    }
    false
}
