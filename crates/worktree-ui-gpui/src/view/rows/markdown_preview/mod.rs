//! Uniform-row markdown renderer for the diff/list preview surfaces, split into
//! submodules.
//!
//! Every item is re-exported below, so `markdown_preview::X` still names the same thing it always did.

mod chrome;
mod images;
mod metrics;
mod render;
mod wrap;

use super::diff_text::*;
use super::*;
use crate::kit::text_search::DiffSearchMatcher;
use crate::view::markdown_preview::{
    MarkdownAlertKind, MarkdownChangeHint, MarkdownInlineImage, MarkdownInlineStyle,
    MarkdownPreviewDocument, MarkdownPreviewRow, MarkdownPreviewRowKind, MarkdownPreviewVisualRow,
    MarkdownPreviewWrapPlan,
};
use crate::view::perf::{self, ViewPerfRenderLane, ViewPerfSpan};
use palette::IntoColor;

pub(in crate::view::rows) use chrome::worktree_preview_bar_color;
pub(in crate::view) use chrome::{
    markdown_preview_alert_bar_color, markdown_preview_alert_label,
    markdown_preview_row_background, worktree_markdown_preview_bar_color,
};
#[cfg(test)]
pub(in crate::view::rows) use chrome::{
    markdown_preview_alert_title_label, markdown_preview_inline_highlight,
    markdown_preview_row_marker, markdown_preview_row_styled_text,
};
pub(in crate::view) use images::{
    MarkdownPreviewImageSource, MarkdownPreviewPictureSizes, markdown_preview_flow_image,
    markdown_preview_image_source, markdown_preview_inline_image,
};
#[cfg(test)]
pub(in crate::view::rows) use images::{
    markdown_preview_no_picture_sizes, markdown_preview_picture_skeleton,
};
pub(in crate::view::rows) use metrics::{
    MARKDOWN_PREVIEW_BASE_FONT_PX, MARKDOWN_PREVIEW_BLOCKQUOTE_BAR_WIDTH_PX,
    MARKDOWN_PREVIEW_INDENT_STEP_PX, MARKDOWN_PREVIEW_LIST_MARKER_GAP_PX,
    MARKDOWN_PREVIEW_LIST_MARKER_MIN_WIDTH_PX, MARKDOWN_PREVIEW_SHELL_PAD_X_PX,
    markdown_preview_font_family_hash,
};
#[cfg(test)]
pub(in crate::view::rows) use metrics::{
    MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX, markdown_preview_row_height,
    markdown_preview_row_horizontal_padding, markdown_preview_row_layout,
    markdown_preview_row_typography,
};
pub(in crate::view) use metrics::{
    MARKDOWN_PREVIEW_CONTENT_PAD_X_PX, MARKDOWN_PREVIEW_INLINE_IMAGE_GAP_PX,
};
pub(in crate::view) use render::{
    MarkdownPreviewQuery, MarkdownPreviewRevealRequest, markdown_preview_highlighted_text,
    markdown_preview_marker_label, markdown_preview_reveal_offset_y, markdown_preview_row_extent,
    markdown_preview_styled_row_with_query,
};
pub(in crate::view::rows) use render::{
    MarkdownPreviewRenderContext, render_markdown_preview_document_rows,
};
#[cfg(test)]
pub(in crate::view::rows) use wrap::markdown_preview_expanded_slice_range;
pub(in crate::view::rows) use wrap::{
    MarkdownPreviewWrapMeasure, markdown_preview_row_required_width,
};
