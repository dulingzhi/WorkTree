//! Pane diff caches: the row/patch builders, collapsed-hunk projection, prepared
//! syntax documents, and the worktree preview cache.
//!
//! Every item is re-exported below, so `diff_cache::X` still names the same thing it always did.

mod helpers;
mod impl_cache;
mod impl_collapsed;
mod impl_patch;
mod impl_preview;
mod impl_rows;
mod impl_syntax;

mod file_diff;
mod image_cache;
mod patch_diff;
#[cfg(test)]
mod tests;
mod word_highlight;

#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use self::file_diff::build_file_diff_cache_rebuild;
pub(in crate::view) use self::file_diff::{PagedFileDiffInlineRows, PagedFileDiffRows};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use self::image_cache::render_svg_image_diff_preview;
pub(in crate::view) use self::patch_diff::{
    PagedPatchDiffRows, PagedPatchSplitRows, PatchInlineVisibleMap,
};
use super::*;
// The pre-existing siblings name the two below through `use super::*;`,
// and only under `#[cfg(test)]` — so the import carries that cfg.
#[cfg(test)]
use crate::view::markdown_preview;
#[cfg(test)]
use crate::view::panes::main::helpers::FileDiffStyleCacheEpochs;
use crate::view::rows;
#[cfg(test)]
use worktree_core::domain::DiffRowProvider;

use helpers::append_non_whitespace;
use helpers::{FILE_DIFF_MAX_CACHED_PAGES, FILE_DIFF_PAGE_SIZE};
