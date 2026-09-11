//! Diff row and anchor data types shared by the plan, align and UI layers.

use super::line_text::{FileDiffEofNewline, FileDiffLineText, FileDiffRowKind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDiffRow {
    pub kind: FileDiffRowKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub old: Option<FileDiffLineText>,
    pub new: Option<FileDiffLineText>,
    pub eof_newline: Option<FileDiffEofNewline>,
}

/// Stable anchor metadata for a rendered side-by-side diff row.
///
/// `region_id`/`ordinal_in_region` are only populated for non-context rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileDiffRowAnchor {
    pub row_index: usize,
    pub region_id: Option<u32>,
    pub ordinal_in_region: Option<u32>,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

/// Stable anchor metadata for one contiguous changed region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileDiffRegionAnchor {
    pub region_id: u32,
    pub row_start: usize,
    pub row_end_exclusive: usize,
    pub old_start_line: Option<u32>,
    pub old_end_line: Option<u32>,
    pub new_start_line: Option<u32>,
    pub new_end_line: Option<u32>,
}

/// Anchors for all rows and change regions in a side-by-side diff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDiffAnchors {
    pub row_anchors: Vec<FileDiffRowAnchor>,
    pub region_anchors: Vec<FileDiffRegionAnchor>,
}

/// Side-by-side diff rows along with stable row/region anchors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDiffRowsWithAnchors {
    pub rows: Vec<FileDiffRow>,
    pub anchors: FileDiffAnchors,
}
