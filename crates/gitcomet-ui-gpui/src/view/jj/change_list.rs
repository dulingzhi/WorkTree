//! The change list: one row per change, newest first, with the working
//! copy (@) pinned above the list instead of inside it.
//!
//! The row view-model is pure (`change_row_vms`) so selection and caches
//! can key on `ChangeId` — unlike git commit hashes, jj change ids are
//! stable across history rewrites, which is exactly why later caches
//! (#79 blame, selection) must use them and not `CommitId`.

use super::*;

use crate::view::date_time::format_relative_time;
use gitcomet_jj_core::{ChangeId, JjChange};

/// One rendered row of the change list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ChangeRowVm {
    pub change_id: ChangeId,
    pub commit_id: String,
    pub author: String,
    /// Pre-formatted relative time ("3 hours ago" style).
    pub time_text: String,
    /// First line of the description; empty when undescribed.
    pub description_line: String,
    pub divergent: bool,
    pub conflicted: bool,
    pub bookmarks: String,
}

impl ChangeRowVm {
    /// Marker chips shown after the change id: divergence and conflicts.
    pub(super) fn flag_labels(&self) -> Vec<SharedString> {
        let mut labels = Vec::new();
        if self.divergent {
            labels.push(crate::i18n::tr("jj.working_copy.divergent"));
        }
        if self.conflicted {
            labels.push(crate::i18n::tr("jj.working_copy.conflicted"));
        }
        labels
    }
}

/// Build the list rows for a repo's loaded changes. The working copy row is
/// skipped here — the describe bar pins it above the list — so the list
/// itself never shifts when @ moves.
pub(super) fn change_row_vms(
    repo: &gitcomet_state::jj_store::JjRepoState,
    now: std::time::SystemTime,
) -> Vec<ChangeRowVm> {
    repo.changes
        .iter()
        .filter(|change| !change.is_working_copy)
        .map(|change| change_row_vm(change, now))
        .collect()
}

pub(super) fn change_row_vm(change: &JjChange, now: std::time::SystemTime) -> ChangeRowVm {
    ChangeRowVm {
        change_id: change.change_id.clone(),
        commit_id: change.commit_id.0.clone(),
        author: change.author_name.clone(),
        time_text: format_relative_time(change.committed_at_unix, now),
        description_line: change.description.lines().next().unwrap_or("").to_string(),
        divergent: change.divergent,
        conflicted: change.conflicted,
        bookmarks: change.bookmarks.join(", "),
    }
}

/// Render one row. Selection compares by `ChangeId` (not row index) so
/// paging — which only appends — never reselects a different change.
pub(super) fn render_change_row(
    row: &ChangeRowVm,
    selected: bool,
    theme: AppTheme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let mut id_row = div()
        .flex()
        .items_baseline()
        .gap_2()
        .min_w_0()
        .child(
            div()
                .text_sm()
                .text_color(if selected {
                    theme.colors.foreground.emphasis
                } else {
                    theme.colors.foreground.primary
                })
                .child(row.change_id.0.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(row.commit_id.clone()),
        );
    for label in row.flag_labels() {
        id_row = id_row.child(
            div()
                .text_xs()
                .rounded(px(theme.radii.control))
                .border_1()
                .border_color(theme.colors.status.warning.border)
                .px_1()
                .text_color(theme.colors.status.warning.foreground)
                .child(label),
        );
    }
    if !row.bookmarks.is_empty() {
        id_row = id_row.child(
            div()
                .text_xs()
                .rounded(px(theme.radii.control))
                .border_1()
                .border_color(theme.colors.stroke.subtle)
                .px_1()
                .text_color(theme.colors.foreground.secondary)
                .child(row.bookmarks.clone()),
        );
    }

    div()
        .id(ElementId::Name(
            format!("jj_change_row_{}", row.change_id.0).into(),
        ))
        .debug_selector(|| format!("jj_change_row_{}", row.change_id.0))
        .flex()
        .flex_col()
        .gap_0p5()
        .px_3()
        .py_2()
        .when(selected, |d| {
            d.bg(theme.colors.interaction.selected_background)
        })
        .border_b_1()
        .border_color(theme.colors.stroke.subtle)
        .hover(|d| d.bg(theme.colors.interaction.hover_background))
        .on_click(move |event, window, cx| on_click(event, window, cx))
        .child(id_row)
        .child(
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground.primary)
                        .truncate()
                        .child(if row.description_line.is_empty() {
                            crate::i18n::tr("jj.working_copy.empty_description").to_string()
                        } else {
                            row.description_line.clone()
                        }),
                )
                .child(
                    div()
                        .ml_auto()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(row.time_text.clone()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_jj_core::JjCommitId;

    fn change(name: &str) -> JjChange {
        JjChange {
            change_id: ChangeId(name.to_string()),
            commit_id: JjCommitId(format!("c{name}")),
            parent_ids: Vec::new(),
            divergent: false,
            conflicted: false,
            is_working_copy: false,
            bookmarks: Vec::new(),
            author_name: "A".to_string(),
            author_email: "a@a".to_string(),
            committed_at_unix: 1,
            description: name.to_string(),
        }
    }

    #[test]
    fn change_rows_skip_the_working_copy_and_keep_order() {
        let at = JjChange {
            is_working_copy: true,
            ..change("at")
        };
        let base = change("base");
        let other = change("other");
        let mut repo = super::super::test_jj_repo_state(1, "/tmp/jj-list");
        repo.changes = vec![at, base.clone(), other.clone()];
        let now = std::time::SystemTime::now();

        let rows = change_row_vms(&repo, now);

        assert_eq!(
            rows.iter()
                .map(|row| row.change_id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["base", "other"],
            "@ lives in the describe bar, never in the list"
        );
        assert_eq!(rows[0].commit_id, base.commit_id.0);
        assert_eq!(rows[1].author, "A");
    }

    #[test]
    fn change_row_maps_description_flags_and_bookmarks() {
        let described = JjChange {
            divergent: true,
            conflicted: true,
            bookmarks: vec!["main".to_string(), "main@origin".to_string()],
            description: "first line\nsecond line".to_string(),
            ..change("zxy")
        };
        let now = std::time::SystemTime::now();

        let row = change_row_vm(&described, now);

        assert_eq!(row.description_line, "first line");
        assert!(row.divergent);
        assert!(row.conflicted);
        assert_eq!(row.bookmarks, "main, main@origin");
        assert!(!row.time_text.is_empty());
        // The label chips derive from the flags, in a fixed order.
        assert_eq!(row.flag_labels().len(), 2);
    }

    #[test]
    fn undescribed_changes_show_an_empty_first_line() {
        let undescribed = JjChange {
            description: String::new(),
            ..change("mzn")
        };
        let row = change_row_vm(&undescribed, std::time::SystemTime::now());

        assert_eq!(row.description_line, "");
    }
}
