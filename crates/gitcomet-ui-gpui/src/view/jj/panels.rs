//! The bookmark card and the operation-log card — the two store-backed
//! panels of the jj workspace (#79). Row view-models are pure like
//! `change_list`'s, so tests never need a window; interactive affordances
//! (the delete button) are built by `JjRepoView` where a listener context
//! exists and passed in as children.

use super::*;

use crate::view::date_time::format_relative_time;
use gitcomet_jj_core::{JjBookmark, JjOp};

/// One rendered bookmark row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BookmarkRowVm {
    /// The bare bookmark name, as mutation messages spell it.
    pub name: String,
    /// jj's rendering: `main` locally, `main@origin` for remote refs.
    pub display_name: String,
    /// The local half of a local/remote pair — the only half deletable
    /// from this workspace.
    pub is_local: bool,
    pub target_commit_id: String,
    pub conflicted: bool,
}

pub(super) fn bookmark_row_vm(bookmark: &JjBookmark) -> BookmarkRowVm {
    BookmarkRowVm {
        name: bookmark.name.clone(),
        display_name: match &bookmark.remote {
            None => bookmark.name.clone(),
            Some(remote) => format!("{}@{remote}", bookmark.name),
        },
        is_local: bookmark.is_local(),
        target_commit_id: bookmark.target_commit_id.0.clone(),
        conflicted: bookmark.conflicted,
    }
}

pub(super) fn bookmark_row_vms(repo: &gitcomet_state::jj_store::JjRepoState) -> Vec<BookmarkRowVm> {
    repo.bookmarks.iter().map(bookmark_row_vm).collect()
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

/// One bookmark row: display name, target commit, a conflicted chip, and
/// the caller-supplied delete affordance (local bookmarks only). The
/// button arrives as a rendered element because `Button` is generic over
/// the view its click listener mutates.
pub(super) fn render_bookmark_row(
    row: &BookmarkRowVm,
    theme: AppTheme,
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
        .child(div().ml_auto().flex_none().children(delete_button))
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
        assert_eq!(local_vm.name, "main");

        let remote_vm = bookmark_row_vm(&remote);
        assert_eq!(remote_vm.display_name, "main@origin");
        assert!(!remote_vm.is_local);
        // The bare name is what mutation messages spell, remote or not.
        assert_eq!(remote_vm.name, "main");
        assert!(remote_vm.conflicted);
        assert_eq!(remote_vm.target_commit_id, "cbbbb");
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
