//! Diff row rendering: styled-text specs, the `MainPaneView` row builders,
//! and the unified/split row bodies.
//!
//! Every item is re-exported below, so `rows::X` still names the same thing it always did.

mod impl_context;
mod impl_render;
mod row_builders;
mod text_spec;

use super::*;

#[cfg(test)]
pub(in crate::view::rows::diff) use row_builders::coverage_gutter_color;
pub(in crate::view) use row_builders::should_hide_unified_diff_header_line;
#[cfg(test)]
pub(in crate::view::rows::diff) use text_spec::focused_diff_line_bg;
