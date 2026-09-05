//! Blame annotation plumbing: the render context snapshot, local-change
//! classification, and the row paint builders.

use super::*;

/// Snapshot of blame data needed to render the annotation column for one diff
/// render pass. Owns its data (an `Arc` clone of the loaded blame plus the
/// recency range and a single `now` timestamp) so it does not borrow the view.
#[derive(Clone)]
pub(in crate::view) struct BlameRenderCtx {
    pub(super) lines: std::sync::Arc<Vec<worktree_core::services::BlameLine>>,
    pub(super) range: Option<(i64, i64)>,
    pub(super) now: std::time::SystemTime,
    pub(super) path: std::sync::Arc<std::path::Path>,
    /// The commit currently being viewed (when blaming a specific revision), used
    /// to hide the "view file at this commit" action on lines from that commit.
    pub(super) viewed_commit: Option<std::sync::Arc<str>>,
    /// The working-tree area being blamed (`Some` for staged/unstaged diffs),
    /// used to classify uncommitted lines as staged vs unstaged. `None` when
    /// blaming a committed revision, where that distinction has no meaning.
    pub(super) area: Option<worktree_core::domain::DiffArea>,
}

impl BlameRenderCtx {
    /// How many lines the blamed revision has.
    ///
    /// The editor compares this against its buffer to work out how far unsaved
    /// edits have slid the attribution below them.
    pub(in crate::view) fn line_count(&self) -> usize {
        self.lines.len()
    }
}

/// Staged vs unstaged classification for an uncommitted blame row when blaming a
/// working-tree area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LocalChange {
    Staged,
    Unstaged,
}

/// Classify an uncommitted (or removal) blame row as staged vs unstaged from
/// whether it is a genuine *context* line and the blamed [`DiffArea`].
///
/// `Staged` area: every local line is staged. `Unstaged` area: an unchanged
/// *context* line (identical in index and worktree but differing from `HEAD`) is
/// a *staged* change; any actual change on the new side — an add or a
/// modification — is an *unstaged* change, which therefore overrides staged when
/// a line carries both staged and unstaged edits. Removals are also changes and
/// are classified with `is_context == false`. Views without diff sidedness (full
/// file content) pass `is_context == false`, yielding the area default
/// (`Staged → Staged`, `Unstaged → Unstaged`).
pub(super) fn classify_local_change(
    area: worktree_core::domain::DiffArea,
    is_context: bool,
) -> LocalChange {
    use worktree_core::domain::DiffArea;
    match area {
        DiffArea::Staged => LocalChange::Staged,
        DiffArea::Unstaged if is_context => LocalChange::Staged,
        DiffArea::Unstaged => LocalChange::Unstaged,
    }
}

/// Run-grouping identity for a blamed row. Consecutive rows in the same group
/// collapse into one attribution run (the textual label is painted only on the
/// run start). Local lines all share the all-zero commit id, so the
/// staged/unstaged kind is part of the group to break a run at a staged↔unstaged
/// boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BlameGroup {
    Committed,
    /// A local change; `None` when blaming a revision (legacy "Now" rendering).
    Local(Option<LocalChange>),
}

/// Previous rendered blamed row's state, threaded across a row sequence so run
/// starts are computed against the previously *rendered* line and group.
#[derive(Clone, Copy, Default)]
pub(super) struct BlamePrev {
    new_line: Option<u32>,
    group: Option<BlameGroup>,
}

/// Border color for a local-change group, reusing the diff add/remove palette.
fn local_change_color(theme: AppTheme, local: Option<LocalChange>) -> gpui::Rgba {
    match local {
        Some(LocalChange::Staged) => crate::theme::blame_staged_color(theme),
        Some(LocalChange::Unstaged) => crate::theme::blame_unstaged_color(theme),
        None => crate::theme::blame_local_change_color(theme.is_dark),
    }
}

/// Run-start label for a local-change group.
fn local_change_label(local: Option<LocalChange>) -> &'static str {
    match local {
        Some(LocalChange::Staged) => "Staged",
        Some(LocalChange::Unstaged) => "Unstaged",
        None => "Now",
    }
}

/// Build the per-row annotation paint data for a diff row, given its old-side
/// and new-side (1-based) line numbers. Returns `None` for rows that carry no
/// annotation (e.g. headers, or pure deletions outside a working-tree blame).
///
/// `is_context` is `true` only for genuinely unchanged lines (so a staged-only
/// edit, which appears as context in the unstaged diff, is labeled "Staged"); any
/// add/modify/removal passes `false`. `old_line` is used only to recognize a pure
/// removal worth a bar. Full file-content views without diff sidedness pass
/// `is_context = false`, falling back to the area default.
pub(in crate::view) fn build_row_blame_paint(
    ctx: &BlameRenderCtx,
    is_context: bool,
    old_line: Option<u32>,
    new_line: Option<u32>,
    prev_new_line: Option<u32>,
    theme: AppTheme,
) -> Option<diff_canvas::RowBlamePaint> {
    let prev = BlamePrev {
        new_line: prev_new_line,
        group: None,
    };
    build_row_blame_paint_inner(ctx, is_context, old_line, new_line, prev, theme)
        .map(|(paint, _)| paint)
}

/// Core blame-paint builder shared by the tracked and untracked entry points.
/// Returns the paint plus the row's [`BlameGroup`] so the caller can thread it
/// into the next row's run-start decision.
pub(super) fn build_row_blame_paint_inner(
    ctx: &BlameRenderCtx,
    is_context: bool,
    old_line: Option<u32>,
    new_line: Option<u32>,
    prev: BlamePrev,
    theme: AppTheme,
) -> Option<(diff_canvas::RowBlamePaint, BlameGroup)> {
    // A pure removal (no new-side line) has no `BlameLine` to attribute, but in a
    // working-tree blame it is still a local change worth a colored bar so a
    // deleted chunk is visible in the annotation column. A removal is a change,
    // so it never counts as context.
    if new_line.is_none() {
        let area = ctx.area?;
        old_line?;
        let local = classify_local_change(area, false);
        let group = BlameGroup::Local(Some(local));
        let is_run_start = prev.group != Some(group);
        return Some((removal_blame_paint(ctx, local, is_run_start, theme), group));
    }

    let annotation = super::super::blame::blame_for_new_line(&ctx.lines, new_line, prev.new_line)?;
    let line = annotation.line;
    // Working-tree blame surfaces not-yet-committed lines with an empty or
    // all-zero object id. These render as a distinct local-change row: a
    // staged/unstaged-colored bar, the matching label, no author initials, no
    // summary, and no action icons.
    let uncommitted = worktree_core::domain::is_uncommitted_commit_id(&line.commit_id);
    let group = if uncommitted {
        BlameGroup::Local(ctx.area.map(|area| classify_local_change(area, is_context)))
    } else {
        BlameGroup::Committed
    };
    // Break a run when the classification group changes from the previous
    // rendered row, even when the underlying commit id is identical (all local
    // lines share the all-zero id), so a staged chunk and an adjacent unstaged
    // chunk each get their own label. Only force this when a previous group is
    // actually known (the tracked diff path); the untracked file-content path
    // leaves `prev.group == None` and relies on `annotation.is_run_start` alone.
    let is_run_start =
        annotation.is_run_start || (prev.group.is_some() && prev.group != Some(group));

    let border = match group {
        BlameGroup::Local(local) => local_change_color(theme, local),
        BlameGroup::Committed => {
            let t = match (line.author_time_unix, ctx.range) {
                (Some(ts), Some(range)) => super::super::blame::blame_recency_t(ts, range),
                _ => 1.0,
            };
            crate::theme::blame_heat_color(theme.is_dark, t)
        }
    };
    let (when, initials, summary, body) = if !is_run_start {
        (
            SharedString::default(),
            SharedString::default(),
            SharedString::default(),
            None,
        )
    } else if let BlameGroup::Local(local) = group {
        (
            SharedString::from(local_change_label(local)),
            SharedString::default(),
            SharedString::default(),
            None,
        )
    } else {
        let when = line
            .author_time_unix
            .map(|ts| crate::view::date_time::format_relative_time(ts, ctx.now))
            .unwrap_or_else(|| "unknown".to_string());
        (
            SharedString::from(when),
            SharedString::from(super::super::blame::author_initials(&line.author)),
            // `Arc<str>` -> `SharedString` is a refcount bump, not a byte copy,
            // so this avoids re-allocating the summary/body every render pass.
            SharedString::from(line.summary.clone()),
            line.body.clone().map(SharedString::from),
        )
    };
    let paint = diff_canvas::RowBlamePaint {
        border,
        show_text: is_run_start,
        when,
        initials,
        summary,
        body,
        commit_id: worktree_core::domain::CommitId(line.commit_id.clone()),
        path: std::sync::Arc::clone(&ctx.path),
        source_path: line.source_path.as_deref().map(std::sync::Arc::from),
        prior_exists: line.prior_exists,
        prior_commit: line
            .prior_commit
            .clone()
            .map(worktree_core::domain::CommitId),
        is_viewed_commit: ctx.viewed_commit.as_deref() == Some(line.commit_id.as_ref()),
    };
    Some((paint, group))
}

/// Build the annotation paint for a pure-removal row, which has no `BlameLine`:
/// just a colored bar plus the staged/unstaged label on the run start.
fn removal_blame_paint(
    ctx: &BlameRenderCtx,
    local: LocalChange,
    is_run_start: bool,
    theme: AppTheme,
) -> diff_canvas::RowBlamePaint {
    let when = if is_run_start {
        SharedString::from(local_change_label(Some(local)))
    } else {
        SharedString::default()
    };
    diff_canvas::RowBlamePaint {
        border: local_change_color(theme, Some(local)),
        show_text: is_run_start,
        when,
        initials: SharedString::default(),
        summary: SharedString::default(),
        body: None,
        commit_id: worktree_core::domain::CommitId(std::sync::Arc::from("")),
        path: std::sync::Arc::clone(&ctx.path),
        source_path: None,
        prior_exists: false,
        prior_commit: None,
        is_viewed_commit: false,
    }
}

/// Build the blame paint for a row while tracking the previous blamed row's
/// new-side line number and group in `prev`, so run starts are computed against
/// the previously *rendered* line and break at staged↔unstaged boundaries.
/// `prev` must be threaded once per rendered row sequence (one cell per map).
pub(super) fn build_row_blame_paint_tracked(
    ctx: &BlameRenderCtx,
    is_context: bool,
    old_line: Option<u32>,
    new_line: Option<u32>,
    prev: &std::cell::Cell<BlamePrev>,
    wrap: Option<diff_canvas::DiffTextWrapSlice>,
    theme: AppTheme,
) -> Option<diff_canvas::RowBlamePaint> {
    let prev_state = prev.get();
    let result =
        build_row_blame_paint_inner(ctx, is_context, old_line, new_line, prev_state, theme);
    // Advancing `prev` on a wrapped continuation row is harmless: it carries the
    // same `new_line`, so the threaded previous-rendered-line stays put for the
    // next logical line. The run tracker therefore needs no special-casing here.
    prev.set(BlamePrev {
        new_line: new_line.or(prev_state.new_line),
        group: match &result {
            Some((_, group)) => Some(*group),
            None => prev_state.group,
        },
    });
    result.map(|(mut paint, _)| {
        // A wrapped continuation row (wrap_ix > 0) is an extra visual row for the
        // same logical line. It keeps the recency border so the bar stays
        // continuous, but must not repeat the run-start time/author/summary label:
        // `is_run_start` is recomputed as true for every continuation row (the
        // previous rendered line equals this one), so without this guard the
        // annotation text is duplicated down each wrapped line. The gutter line
        // numbers suppress wrap continuations the same way (see `show_row_numbers`).
        if wrap.is_some_and(|w| w.wrap_ix > 0) {
            paint.show_text = false;
        }
        paint
    })
}
