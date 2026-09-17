use super::util::{push_diagnostic, push_notification};
use crate::model::{AppNotificationKind, AppState, DiagnosticKind, Loadable, RepoId};
use crate::msg::Effect;
use worktree_core::domain::CommitId;
use worktree_core::error::Error;
use worktree_core::services::MergeTreePreview;

/// Handles the read-only merge preview from `Effect::LoadMergePreview`.
///
/// On success the tree the merge would produce (plus any conflicts) is stored
/// in `RepoState::merge_preview` for the `MergePreview` popover. If HEAD moved
/// between the click and the result landing, the preview is abandoned —
/// showing a merge into a different commit would mislead. Errors are surfaced
/// as a diagnostic + notification.
///
/// The repo borrow is scoped to the inner block so the trailing notification —
/// which needs `&mut AppState` — never fights it.
pub(in crate::store::reducer) fn merge_preview_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    head: CommitId,
    result: Result<MergeTreePreview, Error>,
) -> Vec<Effect> {
    let notice = {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        let head_unchanged = repo_state
            .head_commit_id()
            .is_some_and(|h| h.as_ref() == head.as_ref());
        if !head_unchanged {
            repo_state.set_merge_preview(Loadable::NotLoaded);
            Some((
                AppNotificationKind::Warning,
                rust_i18n::t!("store.reducer.merge_preview_cancelled").to_string(),
            ))
        } else {
            match result {
                Ok(preview) => {
                    repo_state.set_merge_preview(Loadable::Ready(preview));
                    None
                }
                Err(e) => {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    repo_state.set_merge_preview(Loadable::NotLoaded);
                    Some((
                        AppNotificationKind::Error,
                        rust_i18n::t!("store.reducer.merge_preview_failed", error = e).to_string(),
                    ))
                }
            }
        }
    };

    if let Some((kind, message)) = notice {
        push_notification(state, kind, message);
    }
    Vec::new()
}
