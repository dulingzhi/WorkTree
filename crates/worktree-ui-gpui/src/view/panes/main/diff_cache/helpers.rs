//! Free helpers the caches are built from: signatures, content text and the
//! patch visual-line kinds.

use super::super::helpers::PreparedSyntaxDocumentKey;
use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use crate::view::markdown_preview;
use crate::view::rows;
use rustc_hash::FxHasher;

pub(super) const PREPARED_SYNTAX_DOCUMENT_CACHE_MAX_ENTRIES: usize = 256;
pub(super) const FILE_DIFF_PAGE_SIZE: usize = 256;
pub(super) const FILE_DIFF_MAX_CACHED_PAGES: usize = 64;
pub(super) const COLLAPSED_DIFF_REVEAL_STEP: usize = 20;

// Full-document views (file diff, worktree preview) always attempt prepared
// syntax and fall back to plain/heuristic rendering until it is ready.
pub(super) const FULL_DOCUMENT_SYNTAX_MODE: rows::DiffSyntaxMode = rows::DiffSyntaxMode::Auto;

pub(super) fn patch_diff_content_signature(diff: &worktree_core::domain::Diff) -> u64 {
    use std::hash::Hasher;

    let mut hasher = FxHasher::default();
    hasher.write_usize(diff.lines.len());
    for line in diff.lines.iter() {
        let kind = match line.kind {
            worktree_core::domain::DiffLineKind::Header => 0,
            worktree_core::domain::DiffLineKind::Hunk => 1,
            worktree_core::domain::DiffLineKind::Add => 2,
            worktree_core::domain::DiffLineKind::Remove => 3,
            worktree_core::domain::DiffLineKind::Context => 4,
        };
        hasher.write_u8(kind);
        hasher.write_usize(line.text.len());
        hasher.write(line.text.as_ref().as_bytes());
    }
    hasher.finish()
}

pub(super) fn append_non_whitespace(text: &str, out: &mut String) {
    out.extend(text.chars().filter(|ch| !ch.is_whitespace()));
}

pub(super) fn diff_line_content_text(line: &worktree_core::domain::DiffLine) -> &str {
    match line.kind {
        worktree_core::domain::DiffLineKind::Add => {
            line.text.strip_prefix('+').unwrap_or(&line.text)
        }
        worktree_core::domain::DiffLineKind::Remove => {
            line.text.strip_prefix('-').unwrap_or(&line.text)
        }
        worktree_core::domain::DiffLineKind::Context => {
            line.text.strip_prefix(' ').unwrap_or(&line.text)
        }
        worktree_core::domain::DiffLineKind::Header | worktree_core::domain::DiffLineKind::Hunk => {
            &line.text
        }
    }
}

pub(super) fn is_unified_no_newline_marker(text: &str) -> bool {
    text.starts_with("\\ No newline")
}

pub(super) fn is_patch_diff_whitespace_group_line(line: &worktree_core::domain::DiffLine) -> bool {
    matches!(
        line.kind,
        worktree_core::domain::DiffLineKind::Remove | worktree_core::domain::DiffLineKind::Add
    ) || is_unified_no_newline_marker(&line.text)
}

pub(super) fn visual_line_kinds_for_patch_diff(
    diff: &worktree_core::domain::Diff,
    mode: DiffWhitespaceMode,
) -> Vec<worktree_core::domain::DiffLineKind> {
    use worktree_core::domain::DiffLineKind as DK;

    let mut visual = diff.lines.iter().map(|line| line.kind).collect::<Vec<_>>();
    if mode == DiffWhitespaceMode::Show {
        return visual;
    }

    let mut ix = 0usize;
    while ix < diff.lines.len() {
        if !matches!(diff.lines[ix].kind, DK::Remove | DK::Add) {
            ix += 1;
            continue;
        }

        let group_start = ix;
        let mut old_stripped = String::new();
        let mut new_stripped = String::new();
        while ix < diff.lines.len() && is_patch_diff_whitespace_group_line(&diff.lines[ix]) {
            let line = &diff.lines[ix];
            match line.kind {
                DK::Remove => {
                    append_non_whitespace(diff_line_content_text(line), &mut old_stripped)
                }
                DK::Add => append_non_whitespace(diff_line_content_text(line), &mut new_stripped),
                DK::Context | DK::Header | DK::Hunk => {}
            }
            ix += 1;
        }

        if old_stripped == new_stripped {
            for kind in &mut visual[group_start..ix] {
                *kind = DK::Context;
            }
        }
    }

    visual
}

pub(super) fn file_diff_text_is_source_backed(file: &worktree_core::domain::FileDiffText) -> bool {
    file.old_source.is_some() || file.new_source.is_some()
}

pub(super) fn file_diff_markdown_source_len(
    source: Option<&worktree_core::domain::FileDiffTextSource>,
    legacy_text: Option<&Arc<str>>,
) -> usize {
    if let Some(text) = legacy_text {
        return text.len();
    }
    source
        .and_then(|source| std::fs::metadata(&source.path).ok())
        .and_then(|metadata| usize::try_from(metadata.len()).ok())
        .unwrap_or(0)
}

pub(super) fn read_file_diff_markdown_source(
    source: Option<&worktree_core::domain::FileDiffTextSource>,
    legacy_text: Option<&Arc<str>>,
) -> std::result::Result<String, String> {
    if let Some(text) = legacy_text {
        return Ok(text.to_string());
    }
    let Some(source) = source else {
        return Ok(String::new());
    };
    std::fs::read_to_string(&source.path).map_err(|err| err.to_string())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FileDiffPreparedSyntaxApplyResult {
    pub(super) split_left: bool,
    pub(super) split_right: bool,
}

impl FileDiffPreparedSyntaxApplyResult {
    pub(super) fn any(self) -> bool {
        self.split_left || self.split_right
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SyncFileDiffPreparedSyntaxApplyResult {
    pub(super) inserted: bool,
    pub(super) needs_background_prepare: bool,
}

#[cfg(test)]
pub(super) fn preview_lines_source_len(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| line.len())
        .sum::<usize>()
        .saturating_add(lines.len().saturating_sub(1))
}

pub(super) fn build_single_markdown_preview_document(
    source: &str,
) -> Result<Arc<markdown_preview::MarkdownPreviewDocument>, markdown_preview::MarkdownPreviewRefusal>
{
    use markdown_preview::MarkdownPreviewRefusal;

    if source.len() > markdown_preview::MAX_PREVIEW_SOURCE_BYTES {
        return Err(MarkdownPreviewRefusal::Unavailable(
            markdown_preview::single_preview_unavailable_reason(source.len()).to_owned(),
        ));
    }

    let document = markdown_preview::parse_markdown(source).ok_or_else(|| {
        MarkdownPreviewRefusal::Unavailable(
            markdown_preview::single_preview_unavailable_reason(source.len()).to_owned(),
        )
    })?;
    // The single-document preview lays every row out on every frame, so its
    // budget is tighter than the parser's. This one is recoverable: the source
    // is still readable, so the reader is sent there instead of to an error.
    if document.rows.len() > markdown_preview::MAX_FLOWING_PREVIEW_ROWS {
        return Err(MarkdownPreviewRefusal::TooManyRowsToRender);
    }

    Ok(Arc::new(document))
}

/// The pixel size of every picture in `document` that can be measured without
/// decoding it, keyed by the source the document wrote.
///
/// A picture's box is not known until its file has been read, and reading a GIF
/// means decoding every frame — seconds of work for a long one. Its header says
/// how big it is in a few bytes, which is enough to hold the right amount of
/// room open in the meantime. Runs beside the parse, off the UI thread, so a
/// document that carries no pictures pays nothing.
pub(super) fn measure_markdown_preview_pictures(
    document: &markdown_preview::MarkdownPreviewDocument,
    image_base_dir: Option<&std::path::Path>,
) -> rows::MarkdownPreviewPictureSizes {
    let mut sizes: FxHashMap<SharedString, (u32, u32)> = FxHashMap::default();
    let mut measure = |source: &SharedString| {
        if sizes.contains_key(source) {
            return;
        }
        // Only a local file can be measured this cheaply. A remote picture
        // would have to be fetched, which is the expensive half anyway, and
        // `gpui` is already fetching it.
        let Some(rows::MarkdownPreviewImageSource::File(path)) =
            rows::markdown_preview_image_source(image_base_dir, source.as_ref())
        else {
            return;
        };
        if let Ok((width, height)) = image::image_dimensions(&path)
            && width > 0
            && height > 0
        {
            sizes.insert(source.clone(), (width, height));
        }
    };

    for row in document.rows.iter() {
        if let Some(image) = row.image.as_ref() {
            measure(&image.source);
        }
        for inline in row.inline_images.iter() {
            measure(&inline.image.source);
        }
    }

    Arc::new(sizes)
}

#[derive(Clone, Debug, Default)]
pub(super) struct FileDiffBackgroundPreparedSyntaxDocuments {
    pub(super) split_left: Option<rows::BackgroundPreparedDiffSyntaxDocument>,
    pub(super) split_right: Option<rows::BackgroundPreparedDiffSyntaxDocument>,
}

pub(super) fn prepared_syntax_document_key(
    repo_id: RepoId,
    target_rev: u64,
    file_path: &std::path::Path,
    view_mode: PreparedSyntaxViewMode,
) -> PreparedSyntaxDocumentKey {
    PreparedSyntaxDocumentKey {
        repo_id,
        target_rev,
        file_path: file_path.to_path_buf(),
        view_mode,
    }
}

pub(super) fn diff_syntax_edit_from_text_change(
    old: &str,
    new: &str,
) -> Option<rows::DiffSyntaxEdit> {
    if old == new {
        return None;
    }

    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    let mut prefix = 0usize;
    let max_prefix = old_bytes.len().min(new_bytes.len());
    while prefix < max_prefix && old_bytes[prefix] == new_bytes[prefix] {
        prefix += 1;
    }

    let mut old_suffix_start = old_bytes.len();
    let mut new_suffix_start = new_bytes.len();
    while old_suffix_start > prefix
        && new_suffix_start > prefix
        && old_bytes[old_suffix_start - 1] == new_bytes[new_suffix_start - 1]
    {
        old_suffix_start -= 1;
        new_suffix_start -= 1;
    }

    Some(rows::DiffSyntaxEdit {
        old_range: prefix..old_suffix_start,
        new_range: prefix..new_suffix_start,
    })
}
