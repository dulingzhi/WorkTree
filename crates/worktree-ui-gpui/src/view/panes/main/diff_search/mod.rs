//! Diff and file-editor search: query state, match recomputation, the scan
//! backends, and navigation between matches.
//!
//! Every item is re-exported below, so `diff_search::X` still names the same thing it always did.

mod conflict;
mod consts;
mod impl_navigate;
mod impl_query;
mod impl_recompute;
mod impl_scan;
mod inline_patch;
mod row_text;
mod stream;
#[cfg(test)]
mod tests;
mod trigram;

use super::*;
pub(in crate::view) use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};

#[cfg(test)]
use crate::kit::text_search::{
    AsciiCaseInsensitiveNeedle, DiffSearchQueryReuse, diff_search_query_reuse,
};
#[cfg(test)]
pub(in crate::view) use conflict::diff_search_split_row_texts_match_query;
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
use conflict::{
    ConflictResolverSearchContext, ConflictResolverSearchTwoWayRows,
    ConflictResolverSearchVisibleRows, conflict_resolver_visible_match_indices_with_matcher,
};
#[cfg(test)]
use conflict::{
    conflict_resolver_visible_match_indices, identity_three_way_aligned,
    three_way_visible_item_matches_query,
};
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
use consts::{FILE_EDITOR_SEARCH_MAX_MATCHES, FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES};
#[cfg(test)]
pub(in crate::view) use row_text::file_editor_search_ranges;
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
use row_text::{StreamMatchCollectionMode, diff_search_resume_match_ix};
#[cfg(test)]
use row_text::{contains_ascii_case_insensitive, empty_conflict_resolver_search_two_way_rows};
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
#[cfg(test)]
use stream::{
    collect_file_diff_line_text_stream_match_visible_rows, collect_split_stream_match_visible_rows,
    collect_stream_match_visible_rows, collect_stream_match_visible_rows_with_mode,
};
pub(in crate::view) use trigram::DiffSearchVisibleTrigramIndex;
#[cfg(test)]
use trigram::diff_search_inline_patch_query_uses_trigram_index;
