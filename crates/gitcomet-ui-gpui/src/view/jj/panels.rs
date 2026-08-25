//! The bookmark card and the operation-log card — the two store-backed
//! panels of the jj workspace (#79). Row view-models are pure like
//! `change_list`'s, so tests never need a window; interactive affordances
//! (the delete button) are built by `JjRepoView` where a listener context
//! exists and passed in as children.

use super::*;

use crate::view::date_time::format_relative_time;
use gitcomet_jj_core::{JjBookmark, JjOp};

/// One remote half of a bookmark pair, rendered as a chip on the local row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BookmarkRemoteVm {
    pub remote: String,
    pub target_commit_id: String,
    /// The remote ref points at the same commit as the local ref.
    pub synced: bool,
    pub conflicted: bool,
}

/// One rendered bookmark row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BookmarkRowVm {
    /// The bare bookmark name, as mutation messages spell it.
    pub name: String,
    /// jj's rendering: `main` locally, `main@origin` for remote-only rows.
    pub display_name: String,
    /// The local half of a local/remote pair — the only half this workspace
    /// renames or deletes.
    pub is_local: bool,
    /// A remote ref with no local counterpart — its only affordance is
    /// Track, which creates the local half pointing at it.
    pub remote_only: bool,
    /// The remote this row belongs to on remote-only rows.
    pub remote: Option<String>,
    pub target_commit_id: String,
    pub conflicted: bool,
    /// The local row's remote halves, rendered as chips.
    pub remotes: Vec<BookmarkRemoteVm>,
}

pub(super) fn bookmark_row_vm(bookmark: &JjBookmark) -> BookmarkRowVm {
    BookmarkRowVm {
        name: bookmark.name.clone(),
        display_name: match &bookmark.remote {
            None => bookmark.name.clone(),
            Some(remote) => format!("{}@{remote}", bookmark.name),
        },
        is_local: bookmark.is_local(),
        remote_only: !bookmark.is_local(),
        remote: bookmark.remote.clone(),
        target_commit_id: bookmark.target_commit_id.0.clone(),
        conflicted: bookmark.conflicted,
        remotes: Vec::new(),
    }
}

/// One row per bare name (#85): a bookmark's remote halves fold into chips
/// on its local row, and remote refs with no local half become standalone
/// rows whose only affordance is Track.
pub(super) fn bookmark_row_vms(repo: &gitcomet_state::jj_store::JjRepoState) -> Vec<BookmarkRowVm> {
    let mut rows: Vec<BookmarkRowVm> = repo
        .bookmarks
        .iter()
        .filter(|bookmark| bookmark.is_local())
        .map(|local| {
            let remotes: Vec<BookmarkRemoteVm> = repo
                .bookmarks
                .iter()
                .filter(|other| other.name == local.name && !other.is_local())
                .map(|other| BookmarkRemoteVm {
                    remote: other.remote.clone().unwrap_or_default(),
                    synced: other.target_commit_id == local.target_commit_id,
                    target_commit_id: other.target_commit_id.0.clone(),
                    conflicted: other.conflicted,
                })
                .collect();
            let mut vm = bookmark_row_vm(local);
            vm.remotes = remotes;
            vm
        })
        .collect();
    for bookmark in &repo.bookmarks {
        if bookmark.is_local() || rows.iter().any(|row| row.name == bookmark.name) {
            continue;
        }
        rows.push(bookmark_row_vm(bookmark));
    }
    rows
}

/// One rendered operation row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct OpRowVm {
    pub op_id: String,
    pub description: String,
    pub user: String,
    pub time_text: String,
}

pub(super) fn op_row_vm(op: &JjOp, now: std::time::SystemTime) -> OpRowVm {
    OpRowVm {
        op_id: op.op_id.clone(),
        description: op.description.clone(),
        user: op.user.clone(),
        time_text: format_relative_time(op.started_at_unix, now),
    }
}

pub(super) fn op_row_vms(
    repo: &gitcomet_state::jj_store::JjRepoState,
    now: std::time::SystemTime,
) -> Vec<OpRowVm> {
    repo.ops.iter().map(|op| op_row_vm(op, now)).collect()
}

/// The remote chips for a local row: `@origin` per remote, dim while the
/// remote points at the local target, warning while it diverged, danger
/// while the ref itself conflicts.
fn remote_chips(row: &BookmarkRowVm, theme: AppTheme) -> Vec<impl IntoElement> {
    row.remotes
        .iter()
        .map(|remote| {
            let (border, foreground) = if remote.conflicted {
                (
                    theme.colors.status.danger.border,
                    theme.colors.status.danger.foreground,
                )
            } else if remote.synced {
                (
                    theme.colors.stroke.subtle,
                    theme.colors.foreground.secondary,
                )
            } else {
                (
                    theme.colors.status.warning.border,
                    theme.colors.status.warning.foreground,
                )
            };
            div()
                .text_xs()
                .rounded(px(theme.radii.control))
                .border_1()
                .border_color(border)
                .px_1()
                .text_color(foreground)
                .child(format!("@{}", remote.remote))
        })
        .collect()
}

/// One bookmark row: display name, target commit, remote chips, a
/// conflicted chip, and the caller-supplied affordances — rename and
/// delete on local rows, track on remote-only rows. The buttons arrive as
/// rendered elements because `Button` is generic over the view its click
/// listener mutates.
pub(super) fn render_bookmark_row(
    row: &BookmarkRowVm,
    theme: AppTheme,
    rename_button: Option<AnyElement>,
    track_button: Option<AnyElement>,
    delete_button: Option<AnyElement>,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(
            format!("jj_bookmark_row_{}", row.display_name).into(),
        ))
        .debug_selector(|| format!("jj_bookmark_row_{}", row.display_name))
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.colors.stroke.subtle)
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.primary)
                .child(row.display_name.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(row.target_commit_id.clone()),
        )
        .when(!row.remotes.is_empty(), |d| {
            d.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .children(remote_chips(row, theme)),
            )
        })
        .when(row.conflicted, |d| {
            d.child(
                div()
                    .text_xs()
                    .rounded(px(theme.radii.control))
                    .border_1()
                    .border_color(theme.colors.status.warning.border)
                    .px_1()
                    .text_color(theme.colors.status.warning.foreground)
                    .child(crate::i18n::tr("jj.working_copy.conflicted")),
            )
        })
        .child(
            div()
                .ml_auto()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .children(rename_button)
                .children(track_button)
                .children(delete_button),
        )
}

/// One operation row: description, user, relative time.
pub(super) fn render_op_row(row: &OpRowVm, theme: AppTheme) -> impl IntoElement {
    div()
        .id(ElementId::Name(format!("jj_op_row_{}", row.op_id).into()))
        .debug_selector(|| format!("jj_op_row_{}", row.op_id))
        .flex()
        .items_baseline()
        .gap_2()
        .min_w_0()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.colors.stroke.subtle)
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.primary)
                .truncate()
                .child(if row.description.is_empty() {
                    crate::i18n::tr("jj.op_log.unnamed").to_string()
                } else {
                    row.description.clone()
                }),
        )
        .child(
            div()
                .ml_auto()
                .flex_none()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(row.user.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(row.time_text.clone()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_jj_core::JjCommitId;

    #[test]
    fn bookmark_rows_render_local_and_remote_names() {
        let local = JjBookmark {
            name: "main".to_string(),
            remote: None,
            target_commit_id: JjCommitId("caaaa".to_string()),
            conflicted: false,
        };
        let remote = JjBookmark {
            name: "main".to_string(),
            remote: Some("origin".to_string()),
            target_commit_id: JjCommitId("cbbbb".to_string()),
            conflicted: true,
        };

        let local_vm = bookmark_row_vm(&local);
        assert_eq!(local_vm.display_name, "main");
        assert!(local_vm.is_local);
        assert!(!local_vm.remote_only);
        assert_eq!(local_vm.name, "main");

        let remote_vm = bookmark_row_vm(&remote);
        assert_eq!(remote_vm.display_name, "main@origin");
        assert!(!remote_vm.is_local);
        assert!(remote_vm.remote_only);
        assert_eq!(remote_vm.remote.as_deref(), Some("origin"));
        // The bare name is what mutation messages spell, remote or not.
        assert_eq!(remote_vm.name, "main");
        assert!(remote_vm.conflicted);
        assert_eq!(remote_vm.target_commit_id, "cbbbb");
    }

    /// The grouped view (#85): a paired remote folds into a chip on the
    /// local row (flagging divergence), and a remote-only ref stays its own
    /// row.
    #[test]
    fn bookmark_rows_fold_paired_remotes_into_local_rows() {
        let mut repo = super::super::test_jj_repo_state(1, "/tmp/jj-bookmarks");
        repo.bookmarks = vec![
            JjBookmark {
                name: "main".to_string(),
                remote: None,
                target_commit_id: JjCommitId("caaaa".to_string()),
                conflicted: false,
            },
            JjBookmark {
                name: "main".to_string(),
                remote: Some("origin".to_string()),
                target_commit_id: JjCommitId("caaaa".to_string()),
                conflicted: false,
            },
            JjBookmark {
                name: "feature".to_string(),
                remote: None,
                target_commit_id: JjCommitId("ccccc".to_string()),
                conflicted: false,
            },
            JjBookmark {
                name: "feature".to_string(),
                remote: Some("origin".to_string()),
                target_commit_id: JjCommitId("cdddd".to_string()),
                conflicted: false,
            },
            JjBookmark {
                name: "solo".to_string(),
                remote: Some("upstream".to_string()),
                target_commit_id: JjCommitId("ceeee".to_string()),
                conflicted: false,
            },
        ];

        let rows = bookmark_row_vms(&repo);
        assert_eq!(rows.len(), 3, "one row per bare name: {rows:?}");

        let main = &rows[0];
        assert_eq!(main.name, "main");
        assert!(main.is_local);
        assert_eq!(main.remotes.len(), 1);
        assert!(main.remotes[0].synced, "origin points at the local target");

        let feature = &rows[1];
        assert_eq!(feature.remotes.len(), 1);
        assert!(!feature.remotes[0].synced, "origin diverged from local");
        assert_eq!(feature.remotes[0].target_commit_id, "cdddd");

        let solo = &rows[2];
        assert!(solo.remote_only);
        assert_eq!(solo.display_name, "solo@upstream");
        assert!(solo.remotes.is_empty());
    }

    #[test]
    fn op_rows_map_description_user_and_time() {
        let mut repo = super::super::test_jj_repo_state(1, "/tmp/jj-ops");
        repo.ops = vec![JjOp {
            op_id: "op9".to_string(),
            description: "describe change zzz".to_string(),
            user: "A <a@a>".to_string(),
            started_at_unix: 1,
        }];

        let rows = op_row_vms(&repo, std::time::SystemTime::now());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].op_id, "op9");
        assert_eq!(rows[0].description, "describe change zzz");
        assert_eq!(rows[0].user, "A <a@a>");
        assert!(!rows[0].time_text.is_empty());
    }
}
