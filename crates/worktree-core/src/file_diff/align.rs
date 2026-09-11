//! Replacement preparation and alignment, plus the histogram / patience /
//! Myers sequence-alignment backends.

use super::levenshtein::LevenshteinScratch;
use super::line_text::{FileDiffEofNewline, FileDiffLineText, FileDiffRowKind};
use super::plan::{
    DiffRowMeta, Edit, EditKind, FileDiffPlan, FileDiffPlanRun, PlannedReplacementOp,
    PreparedReplacementLine, ReplacementAlignStep,
};
use super::rows_anchors::FileDiffRow;
use rustc_hash::FxHashMap;
use std::sync::Arc;

const REPLACEMENT_ALIGN_CELL_BUDGET: usize = 50_000;
const REPLACEMENT_GAP_COST: u32 = 100;
const REPLACEMENT_PAIR_BASE_COST: u32 = 80;
const REPLACEMENT_PAIR_SCALE_COST: u32 = 120;
const REPLACEMENT_DISSIMILAR_PENALTY_COST: u32 = 40;
const REPLACEMENT_DISSIMILAR_PENALTY_MIN_LEN: usize = 4;

const SIDE_BY_SIDE_HISTOGRAM_LINE_THRESHOLD: usize = 1_024;
const SIDE_BY_SIDE_LINEAR_FALLBACK_LINE_THRESHOLD: usize = 100_000;
const PATIENCE_POSITIONAL_FALLBACK_LINE_THRESHOLD: usize = 2_048;
const SIDE_BY_SIDE_SPARSE_POSITIONAL_MAX_CHANGED_RATIO_DENOMINATOR: usize = 4;
const SIDE_BY_SIDE_SPARSE_POSITIONAL_MAX_BLOCK_LEN: usize = 1;

pub(super) fn prepare_replacement_lines<'a>(lines: &[&'a str]) -> Vec<PreparedReplacementLine<'a>> {
    lines
        .iter()
        .map(|line| PreparedReplacementLine::new(line))
        .collect()
}

pub(super) struct ReplacementTextCacheIds {
    pub(super) ids: Vec<usize>,
    pub(super) unique_texts: usize,
    pub(super) has_duplicates: bool,
}

pub(super) fn prepare_replacement_text_cache_ids(
    lines: &[PreparedReplacementLine<'_>],
) -> ReplacementTextCacheIds {
    let mut text_ids = FxHashMap::default();
    text_ids.reserve(lines.len());
    let mut ids = Vec::with_capacity(lines.len());

    for line in lines {
        let next_id = text_ids.len();
        let id = *text_ids.entry(line.text).or_insert(next_id);
        ids.push(id);
    }

    let unique_texts = text_ids.len();
    ReplacementTextCacheIds {
        ids,
        unique_texts,
        has_duplicates: unique_texts < lines.len(),
    }
}

#[cfg(test)]
fn replacement_alignment_ops(
    deletes: &[PreparedReplacementLine<'_>],
    inserts: &[PreparedReplacementLine<'_>],
) -> Vec<PlannedReplacementOp> {
    replacement_alignment_ops_with_pair_cost(deletes, inserts, replacement_pair_cost)
}

fn replacement_alignment_ops_with_pair_cost<'a, F>(
    deletes: &[PreparedReplacementLine<'a>],
    inserts: &[PreparedReplacementLine<'a>],
    pair_cost_fn: F,
) -> Vec<PlannedReplacementOp>
where
    F: Copy
        + Fn(
            &PreparedReplacementLine<'a>,
            &PreparedReplacementLine<'a>,
            &mut LevenshteinScratch,
        ) -> u32,
{
    let n = deletes.len();
    let m = inserts.len();
    let width = m + 1;
    let mut prev_costs = vec![0; width];
    let mut curr_costs = vec![0; width];
    let mut step = vec![ReplacementAlignStep::None; (n + 1) * width];
    #[allow(clippy::default_constructed_unit_structs)]
    let mut scratch = LevenshteinScratch::default();
    let delete_text_ids = prepare_replacement_text_cache_ids(deletes);
    let insert_text_ids = prepare_replacement_text_cache_ids(inserts);
    // Cache pair costs only when either side actually repeats line text within
    // this replacement block; otherwise every pair is unique and the cache
    // adds extra hashing/allocation work without any reuse.
    let mut pair_cost_cache = (delete_text_ids.has_duplicates || insert_text_ids.has_duplicates)
        .then(|| vec![u32::MAX; delete_text_ids.unique_texts * insert_text_ids.unique_texts]);
    for j in 1..=m {
        prev_costs[j] = (j as u32) * REPLACEMENT_GAP_COST;
        step[j] = ReplacementAlignStep::Insert;
    }

    for i in 1..=n {
        curr_costs[0] = (i as u32) * REPLACEMENT_GAP_COST;
        step[i * width] = ReplacementAlignStep::Delete;
        for j in 1..=m {
            let idx = i * width + j;
            let pair_cost_value = if let Some(pair_cost_cache) = pair_cost_cache.as_mut() {
                let pair_cache_idx = delete_text_ids.ids[i - 1] * insert_text_ids.unique_texts
                    + insert_text_ids.ids[j - 1];
                let cached_pair_cost = &mut pair_cost_cache[pair_cache_idx];
                if *cached_pair_cost == u32::MAX {
                    let computed = pair_cost_fn(&deletes[i - 1], &inserts[j - 1], &mut scratch);
                    *cached_pair_cost = computed;
                    computed
                } else {
                    *cached_pair_cost
                }
            } else {
                pair_cost_fn(&deletes[i - 1], &inserts[j - 1], &mut scratch)
            };
            let pair_cost = prev_costs[j - 1].saturating_add(pair_cost_value);
            let insert_cost = curr_costs[j - 1].saturating_add(REPLACEMENT_GAP_COST);
            let delete_cost = prev_costs[j].saturating_add(REPLACEMENT_GAP_COST);

            let mut best_cost = pair_cost;
            let mut best_step = ReplacementAlignStep::Pair;

            if insert_cost < best_cost {
                best_cost = insert_cost;
                best_step = ReplacementAlignStep::Insert;
            }
            if delete_cost < best_cost {
                best_cost = delete_cost;
                best_step = ReplacementAlignStep::Delete;
            }

            curr_costs[j] = best_cost;
            step[idx] = best_step;
        }
        std::mem::swap(&mut prev_costs, &mut curr_costs);
    }

    let mut i = n;
    let mut j = m;
    let mut aligned_rev = Vec::with_capacity(n + m);
    while i > 0 || j > 0 {
        let idx = i * width + j;
        match step[idx] {
            ReplacementAlignStep::Pair if i > 0 && j > 0 => {
                aligned_rev.push(PlannedReplacementOp::Pair);
                i -= 1;
                j -= 1;
            }
            ReplacementAlignStep::Insert if j > 0 => {
                aligned_rev.push(PlannedReplacementOp::Insert);
                j -= 1;
            }
            ReplacementAlignStep::Delete if i > 0 => {
                aligned_rev.push(PlannedReplacementOp::Delete);
                i -= 1;
            }
            _ if j > 0 => {
                aligned_rev.push(PlannedReplacementOp::Insert);
                j -= 1;
            }
            _ if i > 0 => {
                aligned_rev.push(PlannedReplacementOp::Delete);
                i -= 1;
            }
            _ => break,
        }
    }

    aligned_rev.reverse();
    aligned_rev
}

fn select_side_by_side_edits<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    let combined = old.len().saturating_add(new.len());
    if combined >= SIDE_BY_SIDE_LINEAR_FALLBACK_LINE_THRESHOLD {
        return myers_fallback_edits(old, new);
    }
    if combined >= SIDE_BY_SIDE_HISTOGRAM_LINE_THRESHOLD {
        return histogram_edits(old, new);
    }
    myers_edits(old, new)
}

fn push_plan_run(runs: &mut Vec<FileDiffPlanRun>, run: FileDiffPlanRun) {
    let len = run.row_len();
    if len == 0 {
        return;
    }

    let merged = match (runs.last_mut(), &run) {
        (
            Some(FileDiffPlanRun::Context {
                old_start: last_old_start,
                new_start: last_new_start,
                len: last_len,
            }),
            FileDiffPlanRun::Context {
                old_start,
                new_start,
                len,
            },
        ) if last_old_start.saturating_add(*last_len) == *old_start
            && last_new_start.saturating_add(*last_len) == *new_start =>
        {
            *last_len = last_len.saturating_add(*len);
            true
        }
        (
            Some(FileDiffPlanRun::Remove {
                old_start: last_old_start,
                len: last_len,
            }),
            FileDiffPlanRun::Remove { old_start, len },
        ) if last_old_start.saturating_add(*last_len) == *old_start => {
            *last_len = last_len.saturating_add(*len);
            true
        }
        (
            Some(FileDiffPlanRun::Add {
                new_start: last_new_start,
                len: last_len,
            }),
            FileDiffPlanRun::Add { new_start, len },
        ) if last_new_start.saturating_add(*last_len) == *new_start => {
            *last_len = last_len.saturating_add(*len);
            true
        }
        (
            Some(FileDiffPlanRun::Modify {
                old_start: last_old_start,
                new_start: last_new_start,
                len: last_len,
            }),
            FileDiffPlanRun::Modify {
                old_start,
                new_start,
                len,
            },
        ) if last_old_start.saturating_add(*last_len) == *old_start
            && last_new_start.saturating_add(*last_len) == *new_start =>
        {
            *last_len = last_len.saturating_add(*len);
            true
        }
        _ => false,
    };

    if !merged {
        runs.push(run);
    }
}

fn push_plan_run_with_counts(
    runs: &mut Vec<FileDiffPlanRun>,
    row_count: &mut usize,
    inline_row_count: &mut usize,
    run: FileDiffPlanRun,
) {
    let len = run.row_len();
    if len == 0 {
        return;
    }
    *row_count = row_count.saturating_add(len);
    *inline_row_count = inline_row_count.saturating_add(run.inline_row_len());
    push_plan_run(runs, run);
}

fn apply_eof_newline_to_plan(
    runs: &mut Vec<FileDiffPlanRun>,
    eof_newline: Option<FileDiffEofNewline>,
) -> bool {
    if eof_newline.is_none() {
        return false;
    }

    let Some(last_run) = runs.pop() else {
        return false;
    };
    match last_run {
        FileDiffPlanRun::Context {
            old_start,
            new_start,
            len,
        } if len > 1 => {
            runs.push(FileDiffPlanRun::Context {
                old_start,
                new_start,
                len: len.saturating_sub(1),
            });
            runs.push(FileDiffPlanRun::Modify {
                old_start: old_start.saturating_add(len.saturating_sub(1)),
                new_start: new_start.saturating_add(len.saturating_sub(1)),
                len: 1,
            });
            true
        }
        FileDiffPlanRun::Context {
            old_start,
            new_start,
            ..
        } => {
            runs.push(FileDiffPlanRun::Modify {
                old_start,
                new_start,
                len: 1,
            });
            true
        }
        other => {
            runs.push(other);
            false
        }
    }
}

fn build_sparse_positional_side_by_side_plan(
    old_text: &str,
    new_text: &str,
    old_lines: &[&str],
    new_lines: &[&str],
) -> Option<FileDiffPlan> {
    if old_lines.len() != new_lines.len() {
        return None;
    }
    let line_count = old_lines.len();
    if line_count == 0
        || line_count.saturating_add(line_count) < SIDE_BY_SIDE_HISTOGRAM_LINE_THRESHOLD
    {
        return None;
    }

    let mut runs = Vec::with_capacity(line_count / 8 + 1);
    let mut row_count = 0usize;
    let mut inline_row_count = 0usize;
    let mut changed_lines = 0usize;
    let mut ix = 0usize;

    while ix < line_count {
        if old_lines[ix] == new_lines[ix] {
            let start = ix;
            ix += 1;
            while ix < line_count && old_lines[ix] == new_lines[ix] {
                ix += 1;
            }
            push_plan_run_with_counts(
                &mut runs,
                &mut row_count,
                &mut inline_row_count,
                FileDiffPlanRun::Context {
                    old_start: start,
                    new_start: start,
                    len: ix.saturating_sub(start),
                },
            );
            continue;
        }

        let block_start = ix;
        ix += 1;
        while ix < line_count && old_lines[ix] != new_lines[ix] {
            ix += 1;
        }
        let block_len = ix.saturating_sub(block_start);
        changed_lines = changed_lines.saturating_add(block_len);
        if block_len > SIDE_BY_SIDE_SPARSE_POSITIONAL_MAX_BLOCK_LEN
            || changed_lines
                .saturating_mul(SIDE_BY_SIDE_SPARSE_POSITIONAL_MAX_CHANGED_RATIO_DENOMINATOR)
                > line_count
        {
            return None;
        }

        push_plan_run_with_counts(
            &mut runs,
            &mut row_count,
            &mut inline_row_count,
            FileDiffPlanRun::Modify {
                old_start: block_start,
                new_start: block_start,
                len: block_len,
            },
        );
    }

    let eof_newline = eof_newline_delta(old_text, new_text);
    if apply_eof_newline_to_plan(&mut runs, eof_newline) {
        inline_row_count = inline_row_count.saturating_add(1);
    }

    Some(FileDiffPlan {
        runs,
        row_count,
        inline_row_count,
        eof_newline,
    })
}

fn push_paired_replacement_runs_by_position_to_plan(
    old_start: usize,
    old_len: usize,
    new_start: usize,
    new_len: usize,
    runs: &mut Vec<FileDiffPlanRun>,
    row_count: &mut usize,
    inline_row_count: &mut usize,
) {
    let paired = old_len.min(new_len);
    if paired > 0 {
        push_plan_run_with_counts(
            runs,
            row_count,
            inline_row_count,
            FileDiffPlanRun::Modify {
                old_start,
                new_start,
                len: paired,
            },
        );
    }
    if old_len > paired {
        push_plan_run_with_counts(
            runs,
            row_count,
            inline_row_count,
            FileDiffPlanRun::Remove {
                old_start: old_start.saturating_add(paired),
                len: old_len.saturating_sub(paired),
            },
        );
    }
    if new_len > paired {
        push_plan_run_with_counts(
            runs,
            row_count,
            inline_row_count,
            FileDiffPlanRun::Add {
                new_start: new_start.saturating_add(paired),
                len: new_len.saturating_sub(paired),
            },
        );
    }
}

fn push_aligned_replacement_runs_to_plan_with_pair_cost<F>(
    old_lines: &[&str],
    new_lines: &[&str],
    old_range: std::ops::Range<usize>,
    new_range: std::ops::Range<usize>,
    runs: &mut Vec<FileDiffPlanRun>,
    row_count: &mut usize,
    inline_row_count: &mut usize,
    pair_cost_fn: F,
) where
    F: Copy
        + for<'a> Fn(
            &PreparedReplacementLine<'a>,
            &PreparedReplacementLine<'a>,
            &mut LevenshteinScratch,
        ) -> u32,
{
    let old_start = old_range.start;
    let new_start = new_range.start;
    let deletes = &old_lines[old_range.start..old_range.end];
    let inserts = &new_lines[new_range.start..new_range.end];

    if deletes.is_empty() {
        push_plan_run_with_counts(
            runs,
            row_count,
            inline_row_count,
            FileDiffPlanRun::Add {
                new_start,
                len: inserts.len(),
            },
        );
        return;
    }
    if inserts.is_empty() {
        push_plan_run_with_counts(
            runs,
            row_count,
            inline_row_count,
            FileDiffPlanRun::Remove {
                old_start,
                len: deletes.len(),
            },
        );
        return;
    }

    if deletes.len().saturating_mul(inserts.len()) > REPLACEMENT_ALIGN_CELL_BUDGET {
        push_paired_replacement_runs_by_position_to_plan(
            old_start,
            deletes.len(),
            new_start,
            inserts.len(),
            runs,
            row_count,
            inline_row_count,
        );
        return;
    }

    let deletes = prepare_replacement_lines(deletes);
    let inserts = prepare_replacement_lines(inserts);

    let mut local_old = 0usize;
    let mut local_new = 0usize;
    for op in replacement_alignment_ops_with_pair_cost(&deletes, &inserts, pair_cost_fn) {
        match op {
            PlannedReplacementOp::Pair => {
                push_plan_run_with_counts(
                    runs,
                    row_count,
                    inline_row_count,
                    FileDiffPlanRun::Modify {
                        old_start: old_start.saturating_add(local_old),
                        new_start: new_start.saturating_add(local_new),
                        len: 1,
                    },
                );
                local_old += 1;
                local_new += 1;
            }
            PlannedReplacementOp::Delete => {
                push_plan_run_with_counts(
                    runs,
                    row_count,
                    inline_row_count,
                    FileDiffPlanRun::Remove {
                        old_start: old_start.saturating_add(local_old),
                        len: 1,
                    },
                );
                local_old += 1;
            }
            PlannedReplacementOp::Insert => {
                push_plan_run_with_counts(
                    runs,
                    row_count,
                    inline_row_count,
                    FileDiffPlanRun::Add {
                        new_start: new_start.saturating_add(local_new),
                        len: 1,
                    },
                );
                local_new += 1;
            }
        }
    }
}

pub(super) fn build_linear_fallback_side_by_side_plan_with_pair_cost<F>(
    old_text: &str,
    new_text: &str,
    old_lines: &[&str],
    new_lines: &[&str],
    pair_cost_fn: F,
) -> FileDiffPlan
where
    F: Copy
        + for<'a> Fn(
            &PreparedReplacementLine<'a>,
            &PreparedReplacementLine<'a>,
            &mut LevenshteinScratch,
        ) -> u32,
{
    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while prefix + suffix < old_lines.len()
        && prefix + suffix < new_lines.len()
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid_end = old_lines.len().saturating_sub(suffix);
    let new_mid_end = new_lines.len().saturating_sub(suffix);
    let mut runs = Vec::with_capacity(3);
    let mut row_count = 0usize;
    let mut inline_row_count = 0usize;

    push_plan_run_with_counts(
        &mut runs,
        &mut row_count,
        &mut inline_row_count,
        FileDiffPlanRun::Context {
            old_start: 0,
            new_start: 0,
            len: prefix,
        },
    );

    push_aligned_replacement_runs_to_plan_with_pair_cost(
        old_lines,
        new_lines,
        prefix..old_mid_end,
        prefix..new_mid_end,
        &mut runs,
        &mut row_count,
        &mut inline_row_count,
        pair_cost_fn,
    );

    push_plan_run_with_counts(
        &mut runs,
        &mut row_count,
        &mut inline_row_count,
        FileDiffPlanRun::Context {
            old_start: old_mid_end,
            new_start: new_mid_end,
            len: suffix,
        },
    );

    let eof_newline = eof_newline_delta(old_text, new_text);
    if apply_eof_newline_to_plan(&mut runs, eof_newline) {
        inline_row_count = inline_row_count.saturating_add(1);
    }
    FileDiffPlan {
        runs,
        row_count,
        inline_row_count,
        eof_newline,
    }
}

pub(super) fn build_side_by_side_plan_with_pair_cost<F>(
    old_text: &str,
    new_text: &str,
    old_lines: &[&str],
    new_lines: &[&str],
    pair_cost_fn: F,
) -> FileDiffPlan
where
    F: Copy
        + for<'a> Fn(
            &PreparedReplacementLine<'a>,
            &PreparedReplacementLine<'a>,
            &mut LevenshteinScratch,
        ) -> u32,
{
    if old_lines.len().saturating_add(new_lines.len())
        >= SIDE_BY_SIDE_LINEAR_FALLBACK_LINE_THRESHOLD
    {
        return build_linear_fallback_side_by_side_plan_with_pair_cost(
            old_text,
            new_text,
            old_lines,
            new_lines,
            pair_cost_fn,
        );
    }
    if let Some(plan) =
        build_sparse_positional_side_by_side_plan(old_text, new_text, old_lines, new_lines)
    {
        return plan;
    }

    let edits = select_side_by_side_edits(old_lines, new_lines);
    let mut runs = Vec::with_capacity(edits.len());
    let mut row_count = 0usize;
    let mut inline_row_count = 0usize;
    let mut old_ix = 0usize;
    let mut new_ix = 0usize;
    let mut i = 0usize;

    while i < edits.len() {
        match edits[i].kind {
            EditKind::Equal => {
                let run_old_start = old_ix;
                let run_new_start = new_ix;
                while i < edits.len() && edits[i].kind == EditKind::Equal {
                    old_ix += 1;
                    new_ix += 1;
                    i += 1;
                }
                push_plan_run_with_counts(
                    &mut runs,
                    &mut row_count,
                    &mut inline_row_count,
                    FileDiffPlanRun::Context {
                        old_start: run_old_start,
                        new_start: run_new_start,
                        len: old_ix.saturating_sub(run_old_start),
                    },
                );
            }
            EditKind::Delete => {
                let delete_start = old_ix;
                while i < edits.len() && edits[i].kind == EditKind::Delete {
                    old_ix += 1;
                    i += 1;
                }

                let insert_start = new_ix;
                while i < edits.len() && edits[i].kind == EditKind::Insert {
                    new_ix += 1;
                    i += 1;
                }

                if insert_start == new_ix {
                    push_plan_run_with_counts(
                        &mut runs,
                        &mut row_count,
                        &mut inline_row_count,
                        FileDiffPlanRun::Remove {
                            old_start: delete_start,
                            len: old_ix.saturating_sub(delete_start),
                        },
                    );
                } else {
                    push_aligned_replacement_runs_to_plan_with_pair_cost(
                        old_lines,
                        new_lines,
                        delete_start..old_ix,
                        insert_start..new_ix,
                        &mut runs,
                        &mut row_count,
                        &mut inline_row_count,
                        pair_cost_fn,
                    );
                }
            }
            EditKind::Insert => {
                let insert_start = new_ix;
                while i < edits.len() && edits[i].kind == EditKind::Insert {
                    new_ix += 1;
                    i += 1;
                }
                push_plan_run_with_counts(
                    &mut runs,
                    &mut row_count,
                    &mut inline_row_count,
                    FileDiffPlanRun::Add {
                        new_start: insert_start,
                        len: new_ix.saturating_sub(insert_start),
                    },
                );
            }
        }
    }

    let eof_newline = eof_newline_delta(old_text, new_text);
    if apply_eof_newline_to_plan(&mut runs, eof_newline) {
        inline_row_count = inline_row_count.saturating_add(1);
    }
    FileDiffPlan {
        runs,
        row_count,
        inline_row_count,
        eof_newline,
    }
}

pub(super) fn one_based_line_number(line_ix: usize) -> Option<u32> {
    line_ix
        .checked_add(1)
        .and_then(|line| u32::try_from(line).ok())
}

pub(super) fn mark_changed_line(mask: &mut [bool], line: Option<u32>) {
    let Some(line) = line else {
        return;
    };
    let line_ix = line.saturating_sub(1) as usize;
    if let Some(slot) = mask.get_mut(line_ix) {
        *slot = true;
    }
}

pub(super) fn assign_line_to_row(
    line_to_row: &mut [Option<usize>],
    line: Option<u32>,
    row_index: usize,
) {
    let Some(line) = line else {
        return;
    };
    let line_ix = line.saturating_sub(1) as usize;
    if let Some(slot) = line_to_row.get_mut(line_ix) {
        *slot = Some(row_index);
    }
}

pub(super) fn for_each_plan_row_meta(plan: &FileDiffPlan, mut f: impl FnMut(usize, DiffRowMeta)) {
    let mut row_index = 0usize;

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
                    f(
                        row_index,
                        DiffRowMeta {
                            kind: FileDiffRowKind::Context,
                            old_line: one_based_line_number(old_ix),
                            new_line: one_based_line_number(new_ix),
                        },
                    );
                    row_index = row_index.saturating_add(1);
                }
            }
            FileDiffPlanRun::Remove { old_start, len } => {
                for offset in 0..len {
                    let old_ix = old_start.saturating_add(offset);
                    f(
                        row_index,
                        DiffRowMeta {
                            kind: FileDiffRowKind::Remove,
                            old_line: one_based_line_number(old_ix),
                            new_line: None,
                        },
                    );
                    row_index = row_index.saturating_add(1);
                }
            }
            FileDiffPlanRun::Add { new_start, len } => {
                for offset in 0..len {
                    let new_ix = new_start.saturating_add(offset);
                    f(
                        row_index,
                        DiffRowMeta {
                            kind: FileDiffRowKind::Add,
                            old_line: None,
                            new_line: one_based_line_number(new_ix),
                        },
                    );
                    row_index = row_index.saturating_add(1);
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
                    f(
                        row_index,
                        DiffRowMeta {
                            kind: FileDiffRowKind::Modify,
                            old_line: one_based_line_number(old_ix),
                            new_line: one_based_line_number(new_ix),
                        },
                    );
                    row_index = row_index.saturating_add(1);
                }
            }
        }
    }

    debug_assert_eq!(row_index, plan.row_count);
}

pub(super) fn materialize_rows_from_plan(
    plan: &FileDiffPlan,
    old_text: &Arc<str>,
    old_lines: &[&str],
    new_text: &Arc<str>,
    new_lines: &[&str],
) -> Vec<FileDiffRow> {
    let mut rows = Vec::with_capacity(plan.row_count);
    materialize_rows_from_plan_into(
        &mut rows, plan, old_text, old_lines, new_text, new_lines, 0, 0,
    );
    rows
}

fn shared_line_text(text: &Arc<str>, line: &str) -> FileDiffLineText {
    let base_ptr = text.as_ptr() as usize;
    let line_ptr = line.as_ptr() as usize;
    let start = line_ptr.saturating_sub(base_ptr);
    let end = start.saturating_add(line.len());
    debug_assert_eq!(text.get(start..end), Some(line));
    FileDiffLineText::shared_slice(Arc::clone(text), start..end)
}

pub(super) fn materialize_rows_from_plan_into(
    rows: &mut Vec<FileDiffRow>,
    plan: &FileDiffPlan,
    old_text: &Arc<str>,
    old_lines: &[&str],
    new_text: &Arc<str>,
    new_lines: &[&str],
    old_line_offset: u32,
    new_line_offset: u32,
) {
    let old_line_delta = old_line_offset.saturating_sub(1);
    let new_line_delta = new_line_offset.saturating_sub(1);
    rows.reserve(plan.row_count);
    let row_start = rows.len();

    for run in &plan.runs {
        match run {
            FileDiffPlanRun::Context {
                old_start,
                new_start,
                len,
            } => {
                for offset in 0..*len {
                    let old_ix = old_start.saturating_add(offset);
                    let new_ix = new_start.saturating_add(offset);
                    let text = shared_line_text(
                        old_text,
                        old_lines.get(old_ix).copied().unwrap_or_default(),
                    );
                    rows.push(FileDiffRow {
                        kind: FileDiffRowKind::Context,
                        old_line: one_based_line_number(old_ix)
                            .map(|line| line.saturating_add(old_line_delta)),
                        new_line: one_based_line_number(new_ix)
                            .map(|line| line.saturating_add(new_line_delta)),
                        old: Some(text.clone()),
                        new: Some(text),
                        eof_newline: None,
                    });
                }
            }
            FileDiffPlanRun::Remove { old_start, len } => {
                for offset in 0..*len {
                    let old_ix = old_start.saturating_add(offset);
                    rows.push(FileDiffRow {
                        kind: FileDiffRowKind::Remove,
                        old_line: one_based_line_number(old_ix)
                            .map(|line| line.saturating_add(old_line_delta)),
                        new_line: None,
                        old: Some(shared_line_text(
                            old_text,
                            old_lines.get(old_ix).copied().unwrap_or_default(),
                        )),
                        new: None,
                        eof_newline: None,
                    });
                }
            }
            FileDiffPlanRun::Add { new_start, len } => {
                for offset in 0..*len {
                    let new_ix = new_start.saturating_add(offset);
                    rows.push(FileDiffRow {
                        kind: FileDiffRowKind::Add,
                        old_line: None,
                        new_line: one_based_line_number(new_ix)
                            .map(|line| line.saturating_add(new_line_delta)),
                        old: None,
                        new: Some(shared_line_text(
                            new_text,
                            new_lines.get(new_ix).copied().unwrap_or_default(),
                        )),
                        eof_newline: None,
                    });
                }
            }
            FileDiffPlanRun::Modify {
                old_start,
                new_start,
                len,
            } => {
                for offset in 0..*len {
                    let old_ix = old_start.saturating_add(offset);
                    let new_ix = new_start.saturating_add(offset);
                    rows.push(FileDiffRow {
                        kind: FileDiffRowKind::Modify,
                        old_line: one_based_line_number(old_ix)
                            .map(|line| line.saturating_add(old_line_delta)),
                        new_line: one_based_line_number(new_ix)
                            .map(|line| line.saturating_add(new_line_delta)),
                        old: Some(shared_line_text(
                            old_text,
                            old_lines.get(old_ix).copied().unwrap_or_default(),
                        )),
                        new: Some(shared_line_text(
                            new_text,
                            new_lines.get(new_ix).copied().unwrap_or_default(),
                        )),
                        eof_newline: None,
                    });
                }
            }
        }
    }

    if let Some(marker) = plan.eof_newline {
        if let Some(last) = rows[row_start..].last_mut() {
            last.eof_newline = Some(marker);
        } else {
            rows.push(FileDiffRow {
                kind: FileDiffRowKind::Modify,
                old_line: None,
                new_line: None,
                old: None,
                new: None,
                eof_newline: Some(marker),
            });
        }
    }
}

#[cfg(test)]
pub(super) fn pair_replacements(rows: Vec<FileDiffRow>) -> Vec<FileDiffRow> {
    let mut out = Vec::with_capacity(rows.len());
    let mut ix = 0usize;

    while ix < rows.len() {
        if rows[ix].kind != FileDiffRowKind::Remove {
            out.push(rows[ix].clone());
            ix += 1;
            continue;
        }

        let del_start = ix;
        while ix < rows.len() && rows[ix].kind == FileDiffRowKind::Remove {
            ix += 1;
        }
        let del_end = ix;

        let ins_start = ix;
        while ix < rows.len() && rows[ix].kind == FileDiffRowKind::Add {
            ix += 1;
        }
        let ins_end = ix;

        if ins_start == ins_end {
            out.extend(rows[del_start..del_end].iter().cloned());
            continue;
        }

        out.extend(align_replacement_runs(
            &rows[del_start..del_end],
            &rows[ins_start..ins_end],
        ));
    }

    out
}

#[cfg(test)]
fn align_replacement_runs(deletes: &[FileDiffRow], inserts: &[FileDiffRow]) -> Vec<FileDiffRow> {
    if deletes.is_empty() {
        return inserts.to_vec();
    }
    if inserts.is_empty() {
        return deletes.to_vec();
    }

    if deletes.len().saturating_mul(inserts.len()) > REPLACEMENT_ALIGN_CELL_BUDGET {
        return pair_replacement_runs_by_position(deletes, inserts);
    }

    let delete_meta: Vec<_> = deletes
        .iter()
        .map(|row| PreparedReplacementLine::new(row.old.as_deref().unwrap_or_default()))
        .collect();
    let insert_meta: Vec<_> = inserts
        .iter()
        .map(|row| PreparedReplacementLine::new(row.new.as_deref().unwrap_or_default()))
        .collect();

    let mut out = Vec::with_capacity(deletes.len() + inserts.len());
    let mut delete_ix = 0usize;
    let mut insert_ix = 0usize;
    for op in replacement_alignment_ops(&delete_meta, &insert_meta) {
        match op {
            PlannedReplacementOp::Pair => {
                out.push(make_modify_row(&deletes[delete_ix], &inserts[insert_ix]));
                delete_ix += 1;
                insert_ix += 1;
            }
            PlannedReplacementOp::Insert => {
                out.push(inserts[insert_ix].clone());
                insert_ix += 1;
            }
            PlannedReplacementOp::Delete => {
                out.push(deletes[delete_ix].clone());
                delete_ix += 1;
            }
        }
    }

    out
}

#[cfg(test)]
fn pair_replacement_runs_by_position(
    deletes: &[FileDiffRow],
    inserts: &[FileDiffRow],
) -> Vec<FileDiffRow> {
    let paired = deletes.len().min(inserts.len());
    let mut out = Vec::with_capacity(deletes.len() + inserts.len());

    for i in 0..paired {
        out.push(make_modify_row(&deletes[i], &inserts[i]));
    }
    if deletes.len() > paired {
        out.extend(deletes[paired..].iter().cloned());
    }
    if inserts.len() > paired {
        out.extend(inserts[paired..].iter().cloned());
    }
    out
}

#[cfg(test)]
fn make_modify_row(delete: &FileDiffRow, insert: &FileDiffRow) -> FileDiffRow {
    FileDiffRow {
        kind: FileDiffRowKind::Modify,
        old_line: delete.old_line,
        new_line: insert.new_line,
        old: delete.old.clone(),
        new: insert.new.clone(),
        eof_newline: None,
    }
}

pub(super) fn replacement_pair_cost(
    old: &PreparedReplacementLine<'_>,
    new: &PreparedReplacementLine<'_>,
    scratch: &mut LevenshteinScratch,
) -> u32 {
    if old.text == new.text {
        return 0;
    }

    if let (Some(old_bytes), Some(new_bytes)) = (old.ascii_bytes(), new.ascii_bytes()) {
        let (shared_prefix, shared_suffix) = shared_boundary_bytes(old_bytes, new_bytes);
        return replacement_pair_cost_with_shared_boundary(
            old_bytes,
            new_bytes,
            shared_prefix,
            shared_suffix,
            |old_trimmed, new_trimmed| scratch.distance_bytes(old_trimmed, new_trimmed) as u32,
        );
    }

    replacement_pair_cost_with_distance(old.chars(), new.chars(), |old_trimmed, new_trimmed| {
        scratch.distance(old_trimmed, new_trimmed) as u32
    })
}

pub(super) fn replacement_pair_cost_with_distance<T: Eq>(
    old_units: &[T],
    new_units: &[T],
    distance_fn: impl FnOnce(&[T], &[T]) -> u32,
) -> u32 {
    let (shared_prefix, shared_suffix) = shared_boundary_units(old_units, new_units);
    replacement_pair_cost_with_shared_boundary(
        old_units,
        new_units,
        shared_prefix,
        shared_suffix,
        distance_fn,
    )
}

pub(super) fn replacement_pair_cost_with_shared_boundary<T: Eq>(
    old_units: &[T],
    new_units: &[T],
    shared_prefix: usize,
    shared_suffix: usize,
    distance_fn: impl FnOnce(&[T], &[T]) -> u32,
) -> u32 {
    let max_len_usize = old_units.len().max(new_units.len()).max(1);
    let max_len = max_len_usize as u32;
    let old_trimmed = &old_units[shared_prefix..old_units.len().saturating_sub(shared_suffix)];
    let new_trimmed = &new_units[shared_prefix..new_units.len().saturating_sub(shared_suffix)];
    let trimmed_cells = old_trimmed.len().saturating_mul(new_trimmed.len());

    // Fast path: if either trimmed side is empty, the distance is exactly
    // the length of the other side — skip the O(n*m) Levenshtein DP.
    let distance = if old_trimmed.is_empty() || new_trimmed.is_empty() {
        (old_trimmed.len() + new_trimmed.len()) as u32
    } else if trimmed_cells > REPLACEMENT_ALIGN_CELL_BUDGET {
        u32::try_from(old_trimmed.len().max(new_trimmed.len())).unwrap_or(u32::MAX)
    } else {
        distance_fn(old_trimmed, new_trimmed)
    };

    let mut cost = REPLACEMENT_PAIR_BASE_COST
        + distance
            .min(max_len)
            .saturating_mul(REPLACEMENT_PAIR_SCALE_COST)
            / max_len;
    if shared_prefix == 0
        && shared_suffix == 0
        && max_len_usize >= REPLACEMENT_DISSIMILAR_PENALTY_MIN_LEN
    {
        cost = cost.saturating_add(REPLACEMENT_DISSIMILAR_PENALTY_COST);
    }

    cost
}

pub(super) fn shared_boundary_bytes(a: &[u8], b: &[u8]) -> (usize, usize) {
    let min_len = a.len().min(b.len());
    let word_bytes = std::mem::size_of::<usize>();
    let mut prefix = 0usize;
    while prefix.saturating_add(word_bytes) <= min_len
        && a[prefix..prefix + word_bytes] == b[prefix..prefix + word_bytes]
    {
        prefix += word_bytes;
    }
    while prefix < min_len && a[prefix] == b[prefix] {
        prefix += 1;
    }

    let max_suffix = min_len.saturating_sub(prefix);
    let mut suffix = 0usize;
    while suffix < max_suffix && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix] {
        suffix += 1;
    }

    (prefix, suffix)
}

pub(super) fn shared_boundary_units<T: Eq>(a: &[T], b: &[T]) -> (usize, usize) {
    let mut prefix = 0usize;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }

    let max_suffix = a.len().min(b.len()).saturating_sub(prefix);
    let mut suffix = 0usize;
    while suffix < max_suffix && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix] {
        suffix += 1;
    }

    (prefix, suffix)
}

#[cfg(test)]
pub(super) fn shared_boundary_chars(a: &[char], b: &[char]) -> (usize, usize) {
    shared_boundary_units(a, b)
}

/// Patience/histogram diff algorithm.
///
/// Uses unique lines as anchors via the longest increasing subsequence,
/// then recursively diffs the regions between anchors. Falls back to
/// Myers for regions with no unique lines. This produces cleaner diffs
/// for code with repetitive structural tokens (braces, returns, etc.)
/// by preferring semantically unique lines (function signatures) as
/// alignment points.
/// Maximum recursion depth for histogram/patience diff before falling back to Myers.
const PATIENCE_MAX_DEPTH: usize = 32;

pub(crate) fn histogram_edits<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    patience_recurse(old, new, 0, old.len(), 0, new.len(), 0)
}

fn patience_recurse<'a>(
    old: &[&'a str],
    new: &[&'a str],
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
    depth: usize,
) -> Vec<Edit<'a>> {
    // Fall back to Myers if recursion is too deep.
    if depth >= PATIENCE_MAX_DEPTH {
        return myers_edits(&old[old_start..old_end], &new[new_start..new_end]);
    }

    // Strip common prefix.
    let mut prefix = 0;
    while old_start + prefix < old_end
        && new_start + prefix < new_end
        && old[old_start + prefix] == new[new_start + prefix]
    {
        prefix += 1;
    }

    // Strip common suffix.
    let mut suffix = 0;
    while old_start + prefix + suffix < old_end
        && new_start + prefix + suffix < new_end
        && old[old_end - 1 - suffix] == new[new_end - 1 - suffix]
    {
        suffix += 1;
    }

    let inner_old_start = old_start + prefix;
    let inner_old_end = old_end - suffix;
    let inner_new_start = new_start + prefix;
    let inner_new_end = new_end - suffix;

    let mut edits = Vec::new();

    // Emit prefix equals.
    for i in 0..prefix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[old_start + i]),
            new: Some(new[new_start + i]),
        });
    }

    if inner_old_start == inner_old_end && inner_new_start == inner_new_end {
        // Nothing between prefix and suffix.
    } else if inner_old_start == inner_old_end {
        // Pure insertions.
        for &item in &new[inner_new_start..inner_new_end] {
            edits.push(Edit {
                kind: EditKind::Insert,
                old: None,
                new: Some(item),
            });
        }
    } else if inner_new_start == inner_new_end {
        // Pure deletions.
        for &item in &old[inner_old_start..inner_old_end] {
            edits.push(Edit {
                kind: EditKind::Delete,
                old: Some(item),
                new: None,
            });
        }
    } else {
        // Find unique-line anchors via patience matching.
        let anchors = find_patience_anchors(
            old,
            new,
            inner_old_start,
            inner_old_end,
            inner_new_start,
            inner_new_end,
        );

        if anchors.is_empty() {
            let old_inner = &old[inner_old_start..inner_old_end];
            let new_inner = &new[inner_new_start..inner_new_end];
            // Large anchorless regions with matching line counts tend to be
            // structurally aligned already; preserve same-position context
            // lines linearly instead of paying for a full Myers trace.
            if should_use_patience_positional_fallback(old_inner, new_inner) {
                edits.extend(positional_fallback_edits(old_inner, new_inner));
            } else {
                // No unique anchors — fall back to Myers for this region.
                edits.extend(myers_edits(old_inner, new_inner));
            }
        } else {
            // Recursively diff between anchors.
            let mut oi = inner_old_start;
            let mut ni = inner_new_start;

            for &(old_idx, new_idx) in &anchors {
                if oi < old_idx || ni < new_idx {
                    edits.extend(patience_recurse(
                        old,
                        new,
                        oi,
                        old_idx,
                        ni,
                        new_idx,
                        depth + 1,
                    ));
                }
                edits.push(Edit {
                    kind: EditKind::Equal,
                    old: Some(old[old_idx]),
                    new: Some(new[new_idx]),
                });
                oi = old_idx + 1;
                ni = new_idx + 1;
            }

            // Region after the last anchor.
            if oi < inner_old_end || ni < inner_new_end {
                edits.extend(patience_recurse(
                    old,
                    new,
                    oi,
                    inner_old_end,
                    ni,
                    inner_new_end,
                    depth + 1,
                ));
            }
        }
    }

    // Emit suffix equals.
    for i in 0..suffix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[inner_old_end + i]),
            new: Some(new[inner_new_end + i]),
        });
    }

    edits
}

fn should_use_patience_positional_fallback(old: &[&str], new: &[&str]) -> bool {
    old.len() == new.len()
        && old.len().saturating_add(new.len()) >= PATIENCE_POSITIONAL_FALLBACK_LINE_THRESHOLD
}

/// Find lines that are unique in both old and new within the given ranges,
/// then compute the longest increasing subsequence of their positions to
/// produce patience anchors.
fn find_patience_anchors(
    old: &[&str],
    new: &[&str],
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
) -> Vec<(usize, usize)> {
    // Count occurrences and record position for old lines.
    let mut old_info: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (i, &line) in old.iter().enumerate().take(old_end).skip(old_start) {
        let entry = old_info.entry(line).or_insert((0, i));
        entry.0 += 1;
        entry.1 = i;
    }

    // Count occurrences and record position for new lines.
    let mut new_info: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (j, &line) in new.iter().enumerate().take(new_end).skip(new_start) {
        let entry = new_info.entry(line).or_insert((0, j));
        entry.0 += 1;
        entry.1 = j;
    }

    // Collect lines that appear exactly once in both old and new.
    let mut unique_pairs: Vec<(usize, usize)> = Vec::new();
    for (line, &(old_count, old_idx)) in &old_info {
        if old_count != 1 {
            continue;
        }
        if let Some(&(new_count, new_idx)) = new_info.get(line)
            && new_count == 1
        {
            unique_pairs.push((old_idx, new_idx));
        }
    }

    // Sort by position in old.
    unique_pairs.sort_by_key(|&(oi, _)| oi);

    // Find longest increasing subsequence by new-index.
    patience_lis(&unique_pairs)
}

/// Longest increasing subsequence by the second element (new-index).
pub(super) fn patience_lis(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if pairs.is_empty() {
        return Vec::new();
    }

    let n = pairs.len();
    // `tails[i]` stores the index in `pairs` of the smallest tail element
    // for an increasing subsequence of length `i+1`.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; n];

    for i in 0..n {
        let new_idx = pairs[i].1;
        let pos = tails.partition_point(|&t| pairs[t].1 < new_idx);
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
        if pos > 0 {
            prev[i] = Some(tails[pos - 1]);
        }
    }

    // Reconstruct.
    let mut result = Vec::with_capacity(tails.len());
    // SAFETY: loop above runs at least once (n >= 1) and always pushes to `tails`.
    let mut idx = *tails
        .last()
        .expect("tails is non-empty after processing pairs");
    loop {
        result.push(pairs[idx]);
        match prev[idx] {
            Some(p) => idx = p,
            None => break,
        }
    }
    result.reverse();
    result
}

pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }

    // Keep row tokenization line-oriented; EOF newline deltas are annotated separately.
    text.lines().collect()
}

fn eof_newline_delta(old_text: &str, new_text: &str) -> Option<FileDiffEofNewline> {
    let old_has_newline = old_text.ends_with('\n');
    let new_has_newline = new_text.ends_with('\n');
    match (old_has_newline, new_has_newline) {
        (false, true) => Some(FileDiffEofNewline::MissingInOld),
        (true, false) => Some(FileDiffEofNewline::MissingInNew),
        _ => None,
    }
}

pub(super) fn myers_fallback_edits<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    // Keep fallback linear by only preserving common prefix/suffix and
    // representing the interior as delete/insert spans.
    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while prefix + suffix < old.len()
        && prefix + suffix < new.len()
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid_end = old.len().saturating_sub(suffix);
    let new_mid_end = new.len().saturating_sub(suffix);

    let mut edits = Vec::with_capacity(
        old_mid_end
            .saturating_add(new_mid_end)
            .saturating_sub(prefix)
            .saturating_add(suffix),
    );
    for i in 0..prefix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[i]),
            new: Some(new[i]),
        });
    }
    for &line in &old[prefix..old_mid_end] {
        edits.push(Edit {
            kind: EditKind::Delete,
            old: Some(line),
            new: None,
        });
    }
    for &line in &new[prefix..new_mid_end] {
        edits.push(Edit {
            kind: EditKind::Insert,
            old: None,
            new: Some(line),
        });
    }
    for i in 0..suffix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[old_mid_end + i]),
            new: Some(new[new_mid_end + i]),
        });
    }
    edits
}

pub(super) fn positional_fallback_edits<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while prefix + suffix < old.len()
        && prefix + suffix < new.len()
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_mid_end = old.len().saturating_sub(suffix);
    let new_mid_end = new.len().saturating_sub(suffix);
    let old_mid = &old[prefix..old_mid_end];
    let new_mid = &new[prefix..new_mid_end];
    let paired = old_mid.len().min(new_mid.len());

    let mut edits = Vec::with_capacity(
        old_mid_end
            .saturating_add(new_mid_end)
            .saturating_sub(prefix)
            .saturating_add(suffix),
    );
    for i in 0..prefix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[i]),
            new: Some(new[i]),
        });
    }

    for i in 0..paired {
        if old_mid[i] == new_mid[i] {
            edits.push(Edit {
                kind: EditKind::Equal,
                old: Some(old_mid[i]),
                new: Some(new_mid[i]),
            });
        } else {
            edits.push(Edit {
                kind: EditKind::Delete,
                old: Some(old_mid[i]),
                new: None,
            });
            edits.push(Edit {
                kind: EditKind::Insert,
                old: None,
                new: Some(new_mid[i]),
            });
        }
    }

    for &line in &old_mid[paired..] {
        edits.push(Edit {
            kind: EditKind::Delete,
            old: Some(line),
            new: None,
        });
    }
    for &line in &new_mid[paired..] {
        edits.push(Edit {
            kind: EditKind::Insert,
            old: None,
            new: Some(line),
        });
    }

    for i in 0..suffix {
        edits.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[old_mid_end + i]),
            new: Some(new[new_mid_end + i]),
        });
    }

    edits
}

pub(crate) fn myers_edits<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit<'a>> {
    // An empty side has a fully determined edit script, so take GNU diff's
    // "handle simple cases" branch instead of searching for it. The search
    // would find the same answer at depth D = max(n, m) while storing a
    // D*(D+1)/2 trace, which is gigabytes for a file-sized deletion.
    if old.is_empty() || new.is_empty() {
        return myers_fallback_edits(old, new);
    }

    // Guard against overflow: if n + m exceeds isize::MAX, use linear fallback.
    let Some(sum) = old.len().checked_add(new.len()) else {
        return myers_fallback_edits(old, new);
    };
    if sum > isize::MAX as usize {
        return myers_fallback_edits(old, new);
    }
    if sum > u32::MAX as usize {
        return myers_fallback_edits(old, new);
    }

    let n = old.len() as isize;
    let m = new.len() as isize;
    let max = (n + m) as usize;
    let offset = max as isize;

    let Some(v_size) = max.checked_mul(2).and_then(|v| v.checked_add(1)) else {
        return myers_fallback_edits(old, new);
    };
    let mut v = vec![0u32; v_size];

    // Compact trace: store only active diagonals per depth.
    // At depth d, active diagonals are -d, -d+2, ..., d (d+1 values).
    // Depth d starts at flat index d*(d+1)/2.
    // This eliminates per-depth Vec allocations and reduces trace memory from
    // d * (2*(n+m)+1) to d*(d+1)/2 elements.
    let mut trace = Vec::<u32>::new();

    {
        let mut x = 0isize;
        let mut y = 0isize;
        while x < n && y < m && old[x as usize] == new[y as usize] {
            x += 1;
            y += 1;
        }
        v[offset as usize] = x as u32;
    }
    // Store depth 0: only diagonal 0
    trace.push(v[offset as usize]);

    let mut last_d = 0usize;
    if v[offset as usize] >= n as u32 && v[offset as usize] >= m as u32 {
        last_d = 0;
    } else {
        'outer: for d in 1..=max {
            let d_isize = d as isize;

            // Update v in-place: at depth d, writes go to diagonals with
            // the same parity as d, reads come from the opposite parity
            // (depth d-1 values), so there is no aliasing.
            for k in (-d_isize..=d_isize).step_by(2) {
                let k_idx = (offset + k) as usize;

                let x = if k == -d_isize
                    || (k != d_isize && v[(offset + k - 1) as usize] < v[(offset + k + 1) as usize])
                {
                    v[(offset + k + 1) as usize]
                } else {
                    v[(offset + k - 1) as usize] + 1
                };

                let mut x = x as isize;
                let mut y = x - k;
                while x < n && y < m && old[x as usize] == new[y as usize] {
                    x += 1;
                    y += 1;
                }
                v[k_idx] = x as u32;

                if x >= n && y >= m {
                    for k2 in (-d_isize..=d_isize).step_by(2) {
                        trace.push(v[(offset + k2) as usize]);
                    }
                    last_d = d;
                    break 'outer;
                }
            }

            for k2 in (-d_isize..=d_isize).step_by(2) {
                trace.push(v[(offset + k2) as usize]);
            }
        }
    }

    if n == 0 && m == 0 {
        return Vec::new();
    }

    if last_d == 0 && n == m && v[offset as usize] == n as u32 {
        return old
            .iter()
            .map(|&s| Edit {
                kind: EditKind::Equal,
                old: Some(s),
                new: Some(s),
            })
            .collect();
    }

    let mut x = n;
    let mut y = m;
    let mut rev: Vec<Edit<'a>> = Vec::with_capacity(last_d + (n + m) as usize);

    for d in (1..=last_d).rev() {
        let prev_depth = d - 1;
        let d_isize = d as isize;
        let k = x - y;

        let prev_k = if k == -d_isize
            || (k != d_isize
                && compact_trace_get(&trace, prev_depth, k - 1)
                    < compact_trace_get(&trace, prev_depth, k + 1))
        {
            k + 1
        } else {
            k - 1
        };

        let prev_x = compact_trace_get(&trace, prev_depth, prev_k) as isize;
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            rev.push(Edit {
                kind: EditKind::Equal,
                old: Some(old[(x - 1) as usize]),
                new: Some(new[(y - 1) as usize]),
            });
            x -= 1;
            y -= 1;
        }

        if x == prev_x {
            rev.push(Edit {
                kind: EditKind::Insert,
                old: None,
                new: Some(new[(y - 1) as usize]),
            });
            y -= 1;
        } else {
            rev.push(Edit {
                kind: EditKind::Delete,
                old: Some(old[(x - 1) as usize]),
                new: None,
            });
            x -= 1;
        }
    }

    while x > 0 && y > 0 {
        rev.push(Edit {
            kind: EditKind::Equal,
            old: Some(old[(x - 1) as usize]),
            new: Some(new[(y - 1) as usize]),
        });
        x -= 1;
        y -= 1;
    }
    while x > 0 {
        rev.push(Edit {
            kind: EditKind::Delete,
            old: Some(old[(x - 1) as usize]),
            new: None,
        });
        x -= 1;
    }
    while y > 0 {
        rev.push(Edit {
            kind: EditKind::Insert,
            old: None,
            new: Some(new[(y - 1) as usize]),
        });
        y -= 1;
    }

    rev.reverse();
    rev
}

/// Access diagonal `k` at `depth` in the compact trace buffer.
/// At depth d, active diagonals are -d, -d+2, ..., d (d+1 values).
/// Base offset = d*(d+1)/2, index within depth = (k+d)/2.
#[inline]
fn compact_trace_get(trace: &[u32], depth: usize, k: isize) -> u32 {
    let d = depth as isize;
    let base = depth * (depth + 1) / 2;
    let idx = ((k + d) / 2) as usize;
    trace[base + idx]
}
