//! Side-by-side diff plan types and the entry points that build them.

use super::align::{
    assign_line_to_row, build_side_by_side_plan_with_pair_cost, for_each_plan_row_meta,
    mark_changed_line, materialize_rows_from_plan, materialize_rows_from_plan_into,
    one_based_line_number, replacement_pair_cost, split_lines,
};
use super::line_text::{FileDiffEofNewline, FileDiffRowKind};
use super::rows_anchors::{
    FileDiffAnchors, FileDiffRegionAnchor, FileDiffRow, FileDiffRowAnchor, FileDiffRowsWithAnchors,
};
use std::cell::OnceCell;
use std::sync::Arc;

/// Compact plan for a streamed side-by-side diff.
///
/// Runs carry only line-index spans into the old/new source documents. UI code
/// can materialize rows page-by-page without cloning the entire file into a
/// `Vec<FileDiffRow>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDiffPlanRun {
    Context {
        old_start: usize,
        new_start: usize,
        len: usize,
    },
    Remove {
        old_start: usize,
        len: usize,
    },
    Add {
        new_start: usize,
        len: usize,
    },
    Modify {
        old_start: usize,
        new_start: usize,
        len: usize,
    },
}

impl FileDiffPlanRun {
    pub fn row_len(&self) -> usize {
        match self {
            Self::Context { len, .. }
            | Self::Remove { len, .. }
            | Self::Add { len, .. }
            | Self::Modify { len, .. } => *len,
        }
    }

    pub fn inline_row_len(&self) -> usize {
        match self {
            Self::Modify { len, .. } => len.saturating_mul(2),
            _ => self.row_len(),
        }
    }

    pub fn kind(&self) -> FileDiffRowKind {
        match self {
            Self::Context { .. } => FileDiffRowKind::Context,
            Self::Remove { .. } => FileDiffRowKind::Remove,
            Self::Add { .. } => FileDiffRowKind::Add,
            Self::Modify { .. } => FileDiffRowKind::Modify,
        }
    }
}

/// Compact whole-file plan used by the streamed UI runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDiffPlan {
    pub runs: Vec<FileDiffPlanRun>,
    pub row_count: usize,
    pub inline_row_count: usize,
    pub eof_newline: Option<FileDiffEofNewline>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EditKind {
    Equal,
    Insert,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Edit<'a> {
    pub(crate) kind: EditKind,
    pub(crate) old: Option<&'a str>,
    pub(crate) new: Option<&'a str>,
}

/// A contiguous edit span relative to base lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiffHunk<T> {
    pub(crate) base_start: usize,
    pub(crate) base_end: usize,
    pub(crate) new_lines: Vec<T>,
}

/// Convert an edit script into base-relative change hunks.
pub(crate) fn edits_to_hunks_with<'a, T, F>(
    edits: &[Edit<'a>],
    mut map_insert: F,
) -> Vec<DiffHunk<T>>
where
    F: FnMut(&'a str) -> T,
{
    let mut hunks = Vec::new();
    let mut base_ix = 0usize;
    let mut i = 0usize;

    while i < edits.len() {
        if edits[i].kind == EditKind::Equal {
            base_ix += 1;
            i += 1;
            continue;
        }

        let hunk_base_start = base_ix;
        let mut new_lines = Vec::new();

        while i < edits.len() && edits[i].kind != EditKind::Equal {
            match edits[i].kind {
                EditKind::Delete => {
                    base_ix += 1;
                }
                EditKind::Insert => {
                    new_lines.push(map_insert(edits[i].new.unwrap_or_default()));
                }
                EditKind::Equal => unreachable!(),
            }
            i += 1;
        }

        hunks.push(DiffHunk {
            base_start: hunk_base_start,
            base_end: base_ix,
            new_lines,
        });
    }

    hunks
}

/// Reconstruct one side's sequence for a base range by applying hunks.
pub(crate) fn reconstruct_side_with<'a, T, FBase>(
    base_lines: &'a [&'a str],
    range: std::ops::Range<usize>,
    hunks: &[DiffHunk<T>],
    output: &mut Vec<T>,
    mut map_base_line: FBase,
) where
    T: Clone,
    FBase: FnMut(&'a str) -> T,
{
    let range_end = range.end.min(base_lines.len());
    let mut pos = range.start.min(range_end);

    for hunk in hunks {
        let base_limit = hunk.base_start.min(range_end).max(pos);
        for &line in &base_lines[pos..base_limit] {
            output.push(map_base_line(line));
        }
        output.extend(hunk.new_lines.iter().cloned());
        pos = hunk.base_end.min(range_end).max(pos);
    }

    for &line in &base_lines[pos..range_end] {
        output.push(map_base_line(line));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReplacementAlignStep {
    None,
    Pair,
    Delete,
    Insert,
}

pub fn side_by_side_plan(old: &str, new: &str) -> FileDiffPlan {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    side_by_side_plan_from_lines(old, new, old_lines.as_slice(), new_lines.as_slice())
}

/// Build a side-by-side diff plan from precomputed `str::lines()` slices.
///
/// Callers must ensure `old_lines` and `new_lines` were derived from
/// `old_text`/`new_text` using the same line-splitting semantics as
/// [`str::lines`], because EOF newline handling still comes from the full
/// source texts.
pub fn side_by_side_plan_from_lines(
    old_text: &str,
    new_text: &str,
    old_lines: &[&str],
    new_lines: &[&str],
) -> FileDiffPlan {
    build_side_by_side_plan_with_pair_cost(
        old_text,
        new_text,
        old_lines,
        new_lines,
        replacement_pair_cost,
    )
}

pub fn side_by_side_rows(old: &str, new: &str) -> Vec<FileDiffRow> {
    let old_text: Arc<str> = Arc::from(old);
    let new_text: Arc<str> = Arc::from(new);
    let old_lines = split_lines(old_text.as_ref());
    let new_lines = split_lines(new_text.as_ref());
    let plan = build_side_by_side_plan_with_pair_cost(
        old_text.as_ref(),
        new_text.as_ref(),
        old_lines.as_slice(),
        new_lines.as_slice(),
        replacement_pair_cost,
    );
    materialize_rows_from_plan(
        &plan,
        &old_text,
        old_lines.as_slice(),
        &new_text,
        new_lines.as_slice(),
    )
}

pub fn append_side_by_side_rows_with_offsets(
    rows: &mut Vec<FileDiffRow>,
    old: &str,
    new: &str,
    old_line_offset: u32,
    new_line_offset: u32,
) {
    let old_text: Arc<str> = Arc::from(old);
    let new_text: Arc<str> = Arc::from(new);
    let old_lines = split_lines(old_text.as_ref());
    let new_lines = split_lines(new_text.as_ref());
    let plan = build_side_by_side_plan_with_pair_cost(
        old_text.as_ref(),
        new_text.as_ref(),
        old_lines.as_slice(),
        new_lines.as_slice(),
        replacement_pair_cost,
    );
    materialize_rows_from_plan_into(
        rows,
        &plan,
        &old_text,
        old_lines.as_slice(),
        &new_text,
        new_lines.as_slice(),
        old_line_offset,
        new_line_offset,
    )
}

pub fn side_by_side_rows_with_anchors(old: &str, new: &str) -> FileDiffRowsWithAnchors {
    let rows = side_by_side_rows(old, new);
    let anchors = compute_row_region_anchors(&rows);
    FileDiffRowsWithAnchors { rows, anchors }
}

pub fn plan_row_region_anchors(plan: &FileDiffPlan) -> FileDiffAnchors {
    let mut builder = FileDiffAnchorBuilder::with_capacity(plan.row_count);
    for_each_plan_row_meta(plan, |row_index, row| builder.push(row_index, row));
    builder.finish(plan.row_count)
}

pub fn plan_emitted_line_prefix_counts(plan: &FileDiffPlan) -> (Vec<usize>, Vec<usize>) {
    let mut old_prefix = Vec::with_capacity(plan.row_count.saturating_add(1));
    let mut new_prefix = Vec::with_capacity(plan.row_count.saturating_add(1));
    let mut old_count = 0usize;
    let mut new_count = 0usize;
    old_prefix.push(0);
    new_prefix.push(0);

    for_each_plan_row_meta(plan, |_row_index, row| {
        if row.old_line.is_some() {
            old_count = old_count.saturating_add(1);
        }
        if row.new_line.is_some() {
            new_count = new_count.saturating_add(1);
        }
        old_prefix.push(old_count);
        new_prefix.push(new_count);
    });

    (old_prefix, new_prefix)
}

pub fn plan_changed_line_masks(
    plan: &FileDiffPlan,
    old_line_count: usize,
    new_line_count: usize,
) -> (Vec<bool>, Vec<bool>) {
    let mut old_mask = vec![false; old_line_count];
    let mut new_mask = vec![false; new_line_count];

    for_each_plan_row_meta(plan, |_row_index, row| match row.kind {
        FileDiffRowKind::Context => {}
        FileDiffRowKind::Remove => mark_changed_line(old_mask.as_mut_slice(), row.old_line),
        FileDiffRowKind::Add => mark_changed_line(new_mask.as_mut_slice(), row.new_line),
        FileDiffRowKind::Modify => {
            mark_changed_line(old_mask.as_mut_slice(), row.old_line);
            mark_changed_line(new_mask.as_mut_slice(), row.new_line);
        }
    });

    (old_mask, new_mask)
}

pub fn plan_line_to_row_maps(
    plan: &FileDiffPlan,
    old_line_count: usize,
    new_line_count: usize,
) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut old_line_to_row = vec![None; old_line_count];
    let mut new_line_to_row = vec![None; new_line_count];

    for_each_plan_row_meta(plan, |row_index, row| {
        assign_line_to_row(old_line_to_row.as_mut_slice(), row.old_line, row_index);
        assign_line_to_row(new_line_to_row.as_mut_slice(), row.new_line, row_index);
    });

    (old_line_to_row, new_line_to_row)
}

/// A borrowed view of a single side-by-side diff row.
///
/// This is the zero-allocation equivalent of iterating `side_by_side_rows()`:
/// text references point directly into the source line slices instead of being
/// cloned into owned `String`s.
#[derive(Clone, Copy, Debug)]
pub enum PlanRowView<'a> {
    Context {
        old_line: u32,
        new_line: u32,
        text: &'a str,
    },
    Remove {
        old_line: u32,
        text: &'a str,
    },
    Add {
        new_line: u32,
        text: &'a str,
    },
    Modify {
        old_line: u32,
        new_line: u32,
        old_text: &'a str,
        new_text: &'a str,
    },
}

impl PlanRowView<'_> {
    pub fn kind(&self) -> FileDiffRowKind {
        match self {
            Self::Context { .. } => FileDiffRowKind::Context,
            Self::Remove { .. } => FileDiffRowKind::Remove,
            Self::Add { .. } => FileDiffRowKind::Add,
            Self::Modify { .. } => FileDiffRowKind::Modify,
        }
    }
}

/// Iterate over side-by-side diff rows with borrowed text, avoiding the
/// `Vec<FileDiffRow>` materialization that `side_by_side_rows()` performs.
///
/// Internally computes the diff plan and walks it, yielding `PlanRowView`
/// references into the source texts.
pub fn for_each_side_by_side_row<'a>(
    old: &'a str,
    new: &'a str,
    mut f: impl FnMut(PlanRowView<'a>),
) {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let plan = build_side_by_side_plan_with_pair_cost(
        old,
        new,
        old_lines.as_slice(),
        new_lines.as_slice(),
        replacement_pair_cost,
    );

    for run in &plan.runs {
        match *run {
            FileDiffPlanRun::Context {
                old_start,
                new_start,
                len,
            } => {
                for offset in 0..len {
                    let old_ix = old_start.saturating_add(offset);
                    let new_ix = new_start.saturating_add(offset);
                    if let (Some(ol), Some(nl)) =
                        (one_based_line_number(old_ix), one_based_line_number(new_ix))
                    {
                        let text = old_lines.get(old_ix).copied().unwrap_or_default();
                        f(PlanRowView::Context {
                            old_line: ol,
                            new_line: nl,
                            text,
                        });
                    }
                }
            }
            FileDiffPlanRun::Remove { old_start, len } => {
                for offset in 0..len {
                    let old_ix = old_start.saturating_add(offset);
                    if let Some(ol) = one_based_line_number(old_ix) {
                        let text = old_lines.get(old_ix).copied().unwrap_or_default();
                        f(PlanRowView::Remove { old_line: ol, text });
                    }
                }
            }
            FileDiffPlanRun::Add { new_start, len } => {
                for offset in 0..len {
                    let new_ix = new_start.saturating_add(offset);
                    if let Some(nl) = one_based_line_number(new_ix) {
                        let text = new_lines.get(new_ix).copied().unwrap_or_default();
                        f(PlanRowView::Add { new_line: nl, text });
                    }
                }
            }
            FileDiffPlanRun::Modify {
                old_start,
                new_start,
                len,
            } => {
                for offset in 0..len {
                    let old_ix = old_start.saturating_add(offset);
                    let new_ix = new_start.saturating_add(offset);
                    if let (Some(ol), Some(nl)) =
                        (one_based_line_number(old_ix), one_based_line_number(new_ix))
                    {
                        let old_text = old_lines.get(old_ix).copied().unwrap_or_default();
                        let new_text = new_lines.get(new_ix).copied().unwrap_or_default();
                        f(PlanRowView::Modify {
                            old_line: ol,
                            new_line: nl,
                            old_text,
                            new_text,
                        });
                    }
                }
            }
        }
    }
}

pub(crate) fn compute_row_region_anchors(rows: &[FileDiffRow]) -> FileDiffAnchors {
    let mut builder = FileDiffAnchorBuilder::with_capacity(rows.len());
    for (row_index, row) in rows.iter().enumerate() {
        builder.push(row_index, row.into());
    }
    builder.finish(rows.len())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DiffRowMeta {
    pub(super) kind: FileDiffRowKind,
    pub(super) old_line: Option<u32>,
    pub(super) new_line: Option<u32>,
}

impl From<&FileDiffRow> for DiffRowMeta {
    fn from(row: &FileDiffRow) -> Self {
        Self {
            kind: row.kind,
            old_line: row.old_line,
            new_line: row.new_line,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveRegion {
    region_id: u32,
    row_start: usize,
    old_start_line: Option<u32>,
    old_end_line: Option<u32>,
    new_start_line: Option<u32>,
    new_end_line: Option<u32>,
    next_ordinal: u32,
}

impl ActiveRegion {
    fn new(region_id: u32, row_start: usize) -> Self {
        Self {
            region_id,
            row_start,
            old_start_line: None,
            old_end_line: None,
            new_start_line: None,
            new_end_line: None,
            next_ordinal: 0,
        }
    }

    fn update_lines(&mut self, row: DiffRowMeta) {
        if let Some(old_line) = row.old_line {
            self.old_start_line = Some(
                self.old_start_line
                    .map_or(old_line, |line| line.min(old_line)),
            );
            self.old_end_line = Some(
                self.old_end_line
                    .map_or(old_line, |line| line.max(old_line)),
            );
        }
        if let Some(new_line) = row.new_line {
            self.new_start_line = Some(
                self.new_start_line
                    .map_or(new_line, |line| line.min(new_line)),
            );
            self.new_end_line = Some(
                self.new_end_line
                    .map_or(new_line, |line| line.max(new_line)),
            );
        }
    }

    fn as_region_anchor(self, row_end_exclusive: usize) -> FileDiffRegionAnchor {
        FileDiffRegionAnchor {
            region_id: self.region_id,
            row_start: self.row_start,
            row_end_exclusive,
            old_start_line: self.old_start_line,
            old_end_line: self.old_end_line,
            new_start_line: self.new_start_line,
            new_end_line: self.new_end_line,
        }
    }
}

struct FileDiffAnchorBuilder {
    row_anchors: Vec<FileDiffRowAnchor>,
    region_anchors: Vec<FileDiffRegionAnchor>,
    active_region: Option<ActiveRegion>,
}

impl FileDiffAnchorBuilder {
    fn with_capacity(row_count_hint: usize) -> Self {
        Self {
            row_anchors: Vec::with_capacity(row_count_hint),
            region_anchors: Vec::new(),
            active_region: None,
        }
    }

    fn push(&mut self, row_index: usize, row: DiffRowMeta) {
        if row.kind == FileDiffRowKind::Context {
            if let Some(region) = self.active_region.take() {
                self.region_anchors.push(region.as_region_anchor(row_index));
            }
            self.row_anchors.push(FileDiffRowAnchor {
                row_index,
                region_id: None,
                ordinal_in_region: None,
                old_line: row.old_line,
                new_line: row.new_line,
            });
            return;
        }

        let region = self.active_region.get_or_insert_with(|| {
            let region_id = self.region_anchors.len() as u32;
            ActiveRegion::new(region_id, row_index)
        });
        region.update_lines(row);
        let ordinal_in_region = region.next_ordinal;
        region.next_ordinal = region.next_ordinal.saturating_add(1);

        self.row_anchors.push(FileDiffRowAnchor {
            row_index,
            region_id: Some(region.region_id),
            ordinal_in_region: Some(ordinal_in_region),
            old_line: row.old_line,
            new_line: row.new_line,
        });
    }

    fn finish(mut self, row_count: usize) -> FileDiffAnchors {
        if let Some(region) = self.active_region.take() {
            self.region_anchors.push(region.as_region_anchor(row_count));
        }
        FileDiffAnchors {
            row_anchors: self.row_anchors,
            region_anchors: self.region_anchors,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PlannedReplacementOp {
    Pair,
    Delete,
    Insert,
}

pub(super) struct PreparedReplacementLine<'a> {
    pub(super) text: &'a str,
    ascii_bytes: Option<&'a [u8]>,
    pub(super) chars: OnceCell<Box<[char]>>,
}

impl<'a> PreparedReplacementLine<'a> {
    pub(super) fn new(text: &'a str) -> Self {
        if text.is_ascii() {
            Self {
                text,
                ascii_bytes: Some(text.as_bytes()),
                chars: OnceCell::new(),
            }
        } else {
            let prepared_chars = text.chars().collect::<Vec<_>>().into_boxed_slice();
            let chars = OnceCell::new();
            assert!(
                chars.set(prepared_chars).is_ok(),
                "fresh OnceCell should accept prepared chars"
            );
            Self {
                text,
                ascii_bytes: None,
                chars,
            }
        }
    }

    pub(super) fn ascii_bytes(&self) -> Option<&[u8]> {
        self.ascii_bytes
    }

    pub(super) fn chars(&self) -> &[char] {
        self.chars
            .get_or_init(|| self.text.chars().collect::<Vec<_>>().into_boxed_slice())
            .as_ref()
    }
}
