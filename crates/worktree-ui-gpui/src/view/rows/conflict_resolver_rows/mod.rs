//! Conflict resolver row rendering: the styled-text cache the rows share, and the
//! `MainPaneView` builders for the three-way, two-way and resolved-output bodies.
//!
//! Every item is re-exported below, so `conflict_resolver_rows::X` still names the same thing it always did.

mod impl_diff;
mod impl_resolved;
mod impl_three_way;
mod styled_text;

#[cfg(test)]
mod tests;

// The pieces below import what they need from their own glob chain, so this
// block is what remains that is still named here: the parent's glob.
use super::*;
// The tests build queries through these, so they stay in the namespace
// under the same cfg the test module carries.
#[cfg(test)]
use crate::kit::text_search::DiffSearchOptions;

pub(in crate::view) use styled_text::{
    resolved_output_gutter_width, resolved_output_line_no_width,
};
// The tests read these through `use super::*;`, so the re-export carries the
// same cfg their consumers do.
