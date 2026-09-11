//! Inline-patch diff search: source/visible index mapping and match collection.

use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use crate::view::diff_utils::DiffClickKind;
use gpui::SharedString;
use rustc_hash::FxHashMap;
use std::borrow::Cow;
use worktree_core::domain::Diff;
pub(super) fn inline_patch_diff_search_text<'a>(
    diff: &'a Diff,
    diff_click_kinds: &[DiffClickKind],
    diff_header_display_cache: &'a FxHashMap<usize, SharedString>,
    src_ix: usize,
) -> Option<Cow<'a, str>> {
    let line = diff.lines.get(src_ix)?;
    let click_kind = diff_click_kinds
        .get(src_ix)
        .copied()
        .unwrap_or(DiffClickKind::Line);
    if matches!(
        click_kind,
        DiffClickKind::HunkHeader | DiffClickKind::FileHeader
    ) && let Some(display) = diff_header_display_cache.get(&src_ix)
    {
        return Some(Cow::Borrowed(display.as_ref()));
    }

    if !line.text.contains('\t') {
        return Some(Cow::Borrowed(line.text.as_ref()));
    }

    let mut expanded = String::with_capacity(line.text.len());
    for ch in line.text.chars() {
        match ch {
            '\t' => expanded.push_str("    "),
            _ => expanded.push(ch),
        }
    }
    Some(Cow::Owned(expanded))
}

fn inline_patch_diff_src_ix_for_visible_ix(
    diff_visible_inline_map: Option<&super::diff_cache::PatchInlineVisibleMap>,
    diff_visible_indices: &[usize],
    visible_ix: usize,
) -> Option<usize> {
    if let Some(map) = diff_visible_inline_map {
        return map.src_ix_for_visible_ix(visible_ix);
    }
    diff_visible_indices.get(visible_ix).copied()
}

pub(super) fn inline_patch_diff_visible_ix_matches_query(
    diff: &Diff,
    diff_click_kinds: &[DiffClickKind],
    diff_header_display_cache: &FxHashMap<usize, SharedString>,
    diff_visible_inline_map: Option<&super::diff_cache::PatchInlineVisibleMap>,
    diff_visible_indices: &[usize],
    query: AsciiCaseInsensitiveNeedle<'_>,
    visible_ix: usize,
) -> bool {
    let Some(src_ix) = inline_patch_diff_src_ix_for_visible_ix(
        diff_visible_inline_map,
        diff_visible_indices,
        visible_ix,
    ) else {
        return false;
    };
    inline_patch_diff_search_text(diff, diff_click_kinds, diff_header_display_cache, src_ix)
        .is_some_and(|text| query.is_match(text.as_ref()))
}

pub(super) fn collect_inline_patch_diff_visible_matches_with_needle(
    diff: &Diff,
    diff_click_kinds: &[DiffClickKind],
    diff_header_display_cache: &FxHashMap<usize, SharedString>,
    diff_visible_inline_map: Option<&super::diff_cache::PatchInlineVisibleMap>,
    diff_visible_indices: &[usize],
    query: AsciiCaseInsensitiveNeedle<'_>,
    out: &mut Vec<usize>,
) {
    let total = diff_visible_inline_map
        .map(super::diff_cache::PatchInlineVisibleMap::visible_len)
        .unwrap_or(diff_visible_indices.len());
    for visible_ix in 0..total {
        if inline_patch_diff_visible_ix_matches_query(
            diff,
            diff_click_kinds,
            diff_header_display_cache,
            diff_visible_inline_map,
            diff_visible_indices,
            query,
            visible_ix,
        ) {
            out.push(visible_ix);
        }
    }
}
