//! The change-details card (#82): the selected change's file list (`jj
//! diff --summary`) and, for a clicked file, its unified diff (`jj diff
//! --git`). Row view-models and the diff-line split stay pure like the
//! other panels, so they test without a window; the interactive affordance
//! (which file is expanded) lives in `JjRepoView`.

use super::*;

use gitcomet_jj_core::{JjFileStat, JjFileStatus};
use gitcomet_state::jj_store::JjRepoState;

/// Diff lines beyond this are dropped with a truncation note — a generated
/// file or a lockfile can be tens of thousands of lines, and the card is
/// inline, not a scrolling diff surface like the focused-diff window.
const MAX_DIFF_LINES: usize = 2000;

/// One rendered file row of the details card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FileRowVm {
    pub path: String,
    /// The single status letter (A/M/D/R/C), as jj spells it.
    pub status_label: &'static str,
    /// The pre-rename path, for `R {old => new}` rows.
    pub rename_from: Option<String>,
}

pub(super) fn file_row_vm(stat: &JjFileStat) -> FileRowVm {
    FileRowVm {
        path: stat.path.clone(),
        status_label: stat.status.label(),
        rename_from: match &stat.status {
            JjFileStatus::Renamed { from } => Some(from.clone()),
            _ => None,
        },
    }
}

pub(super) fn file_row_vms(repo: &JjRepoState) -> Vec<FileRowVm> {
    repo.details.files.iter().map(file_row_vm).collect()
}

/// The status chip's color class: adds and removes lean on the theme's
/// diff palette, conflicts on the warning palette, the rest on plain
/// foreground emphasis with no chip fill.
fn status_chip_colors(status_label: &str, theme: &AppTheme) -> (gpui::Rgba, Option<gpui::Rgba>) {
    match status_label {
        "A" => (
            theme.colors.diff.added.foreground,
            Some(theme.colors.diff.added.background),
        ),
        "D" => (
            theme.colors.diff.removed.foreground,
            Some(theme.colors.diff.removed.background),
        ),
        "C" => (
            theme.colors.status.warning.foreground,
            Some(theme.colors.status.warning.background),
        ),
        _ => (theme.colors.foreground.emphasis, None),
    }
}

/// One file row: status chip, path, and the rename source when jj reported
/// `R {old => new}`. Clicking expands the file's diff below the list.
pub(super) fn render_file_row(
    row: &FileRowVm,
    expanded: bool,
    theme: AppTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (chip_foreground, chip_background) = status_chip_colors(row.status_label, &theme);
    div()
        .id(ElementId::Name(format!("jj_file_row_{}", row.path).into()))
        .debug_selector(move || format!("jj_file_row_{}", row.path))
        .flex()
        .items_baseline()
        .gap_2()
        .min_w_0()
        .px_3()
        .py_1()
        .when(expanded, |d| {
            d.bg(theme.colors.interaction.selected_background)
        })
        .hover(|d| d.bg(theme.colors.interaction.hover_background))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(
            div()
                .flex_none()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .rounded(px(theme.radii.control))
                .px_1()
                .text_color(chip_foreground)
                .when_some(chip_background, |d, bg| d.bg(bg))
                .child(row.status_label),
        )
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.primary)
                .truncate()
                .child(row.path.clone()),
        )
        .when_some(row.rename_from.clone(), |d, from| {
            d.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::t!("jj.details.renamed_from", from = from)),
            )
        })
}

/// How one diff line is colored. Mirrors the focused-diff window's
/// classification for the subset of unified-diff syntax jj prints.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffLineKind {
    Header,
    Hunk,
    Add,
    Remove,
    Context,
}

pub(super) fn diff_line_kind(line: &str) -> DiffLineKind {
    if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
    {
        DiffLineKind::Header
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        // `+++ b/…` was already classified as a header above; any other
        // `+` prefix (including the empty `+` line) is an add.
        DiffLineKind::Add
    } else if line.starts_with('-') {
        DiffLineKind::Remove
    } else {
        DiffLineKind::Context
    }
}

/// Split diff text into colored lines, capped at [`MAX_DIFF_LINES`]; the
/// flag reports whether anything was dropped.
pub(super) fn diff_lines_capped(text: &str) -> (Vec<(DiffLineKind, String)>, bool) {
    let mut lines: Vec<(DiffLineKind, String)> = text
        .lines()
        .map(|line| (diff_line_kind(line), line.to_string()))
        .collect();
    let truncated = lines.len() > MAX_DIFF_LINES;
    lines.truncate(MAX_DIFF_LINES);
    (lines, truncated)
}

/// The inline diff surface: one colored row per line (the same palette as
/// the focused-diff window) and a truncation note when capped.
pub(super) fn render_file_diff(text: &str, theme: AppTheme) -> impl IntoElement {
    let (lines, truncated) = diff_lines_capped(text);
    let mut list = div().flex().flex_col();
    for (kind, line) in &lines {
        let (foreground, background) = match kind {
            DiffLineKind::Header => (theme.colors.foreground.secondary, None),
            DiffLineKind::Hunk => (theme.colors.accent.foreground, None),
            DiffLineKind::Add => (
                theme.colors.diff.added.foreground,
                Some(theme.colors.diff.added.background),
            ),
            DiffLineKind::Remove => (
                theme.colors.diff.removed.foreground,
                Some(theme.colors.diff.removed.background),
            ),
            DiffLineKind::Context => (theme.colors.foreground.primary, None),
        };
        list = list.child(
            div()
                .w_full()
                .flex()
                .flex_row()
                .text_xs()
                .text_color(foreground)
                .when_some(background, |d, bg| d.bg(bg))
                .child(
                    div()
                        .whitespace_nowrap()
                        .child(SharedString::from(line.clone())),
                ),
        );
    }
    if truncated {
        list = list.child(
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::t!(
                    "jj.details.truncated",
                    lines = MAX_DIFF_LINES
                )),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_jj_core::JjFileStat;

    #[test]
    fn file_rows_map_status_labels_and_rename_sources() {
        let mut repo = super::super::test_jj_repo_state(1, "/tmp/jj-details");
        repo.details.files = vec![
            JjFileStat {
                path: "added.txt".to_string(),
                status: JjFileStatus::Added,
            },
            JjFileStat {
                path: "moved.txt".to_string(),
                status: JjFileStatus::Renamed {
                    from: "tracked.txt".to_string(),
                },
            },
            JjFileStat {
                path: "merged.txt".to_string(),
                status: JjFileStatus::Conflict,
            },
        ];

        let rows = file_row_vms(&repo);

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].status_label, "A");
        assert_eq!(rows[0].rename_from, None);
        assert_eq!(rows[1].status_label, "R");
        assert_eq!(rows[1].rename_from.as_deref(), Some("tracked.txt"));
        assert_eq!(rows[2].status_label, "C");
    }

    #[test]
    fn diff_lines_classify_the_unified_diff_syntax() {
        let text = "diff --git a/f b/f\nindex 123..456 100644\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n context\n-removed\n+added\n+";
        let (lines, truncated) = diff_lines_capped(text);

        assert!(!truncated);
        let kinds: Vec<DiffLineKind> = lines.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Header,
                DiffLineKind::Hunk,
                DiffLineKind::Context,
                DiffLineKind::Remove,
                DiffLineKind::Add,
                DiffLineKind::Add,
            ]
        );
        assert_eq!(lines.last().unwrap().1, "+");
    }

    #[test]
    fn oversized_diffs_are_capped_with_a_truncation_flag() {
        let text = (0..MAX_DIFF_LINES + 500)
            .map(|ix| format!("+line {ix}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, truncated) = diff_lines_capped(&text);

        assert!(truncated);
        assert_eq!(lines.len(), MAX_DIFF_LINES);
        assert_eq!(
            lines.last().unwrap().1,
            format!("+line {}", MAX_DIFF_LINES - 1)
        );
    }

    #[test]
    fn empty_diff_text_yields_no_lines() {
        let (lines, truncated) = diff_lines_capped("");
        assert!(lines.is_empty());
        assert!(!truncated);
    }
}
