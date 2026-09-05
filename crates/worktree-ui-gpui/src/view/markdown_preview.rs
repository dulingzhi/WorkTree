//! Markdown preview engine.
//!
//! One module per domain, under `markdown_preview/`:
//!
//! - `model` — rows, inline spans, images, and the per-row render caches
//! - `wrap` — visual-row wrap plans for the uniform-row surfaces
//! - `parse` — source-to-document entry points and source-line mapping
//! - `flatten` — the pulldown-cmark event walk that builds rows
//! - `html` — classification of the HTML the preview understands
//! - `inline` — inline span machinery: styles, links, whitespace normalisation
//! - `tables` — column alignment for flattened table rows
//! - `diff` — two-sided diff previews: hints, alignment, spacers, inline merge
//!
//! This file is the module root: it declares the domains and re-exports the
//! surface the renderers and panes consume, so `crate::view::markdown_preview`
//! keeps naming the same items it always did.

mod diff;
mod flatten;
mod html;
mod inline;
mod model;
mod parse;
mod tables;
mod wrap;

pub(super) use self::diff::{
    build_markdown_diff_preview, scrollbar_markers_for_diff_preview, scrollbar_markers_for_document,
};
pub(super) use self::model::{
    MAX_DIFF_PREVIEW_SOURCE_BYTES, MAX_FLOWING_PREVIEW_ROWS, MAX_PREVIEW_SOURCE_BYTES,
    MarkdownAlertKind, MarkdownBlock, MarkdownChangeHint, MarkdownInlineImage, MarkdownInlineStyle,
    MarkdownPreviewDiff, MarkdownPreviewDocument, MarkdownPreviewRefusal, MarkdownPreviewRow,
    MarkdownPreviewRowKind, TOO_MANY_ROWS_TO_RENDER_MESSAGE, diff_preview_unavailable_reason,
    markdown_document_blocks, single_preview_unavailable_reason,
};
// Names whose only consumers sit in `#[cfg(test)]` modules or behind the
// `benchmarks` feature: re-exported under the matching cfg so the plain
// library build does not carry unused imports.
#[cfg(any(test, feature = "benchmarks"))]
pub(super) use self::model::MarkdownInlineSpan;
#[cfg(test)]
pub(super) use self::model::{MAX_PREVIEW_ROWS, MarkdownImage};
#[cfg(feature = "benchmarks")]
pub(super) use self::model::{MarkdownPreviewRowStyledTextCache, MarkdownPreviewRowWidthCache};
pub(super) use self::parse::parse_markdown;
pub(super) use self::wrap::{
    MarkdownPreviewVisualRow, MarkdownPreviewWrapPlan, build_markdown_preview_split_wrap_plans,
    build_markdown_preview_wrap_plan,
};

// ── Tests ───────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests;
