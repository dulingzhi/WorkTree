use super::util::{push_diagnostic, push_notification};
use crate::model::{AppNotificationKind, AppState, DiagnosticKind, Loadable, RepoId};
use crate::msg::Effect;
use worktree_core::error::Error;
use worktree_core::services::InteractiveRebaseEntry;
use worktree_core::squash::{AutosquashMode, build_autosquash_plan};

/// Handles the `base..HEAD` fold result from `Effect::LoadAutosquashSetup`.
///
/// On success it folds the range with the autosquash rules and stores the plan
/// in `RepoState::autosquash_preview` for the confirmation popover to render.
/// When nothing is eligible it records a notice and clears the preview instead
/// of rewriting history. If HEAD moved between the click and the list landing,
/// the fold is abandoned — rewriting would touch the wrong commits.
///
/// The repo borrow is scoped to the inner block so the trailing notification —
/// which needs `&mut AppState` — never fights it.
pub(in crate::store::reducer) fn autosquash_rebase_setup_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    base: String,
    result: Result<Vec<InteractiveRebaseEntry>, Error>,
) -> Vec<Effect> {
    let notice = {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        match result {
            Ok(entries) => {
                // `base..HEAD` lists oldest-first, so the last entry is the live
                // HEAD. If it no longer matches, history moved while we listed
                // and folding now would rewrite the wrong range — abandon,
                // mirroring the squash guard.
                let head_unchanged = entries.last().is_some_and(|e| {
                    repo_state
                        .head_commit_id()
                        .is_some_and(|h| e.commit_id == h.as_ref())
                });
                if !head_unchanged {
                    repo_state.set_autosquash_preview(Loadable::NotLoaded);
                    Some((
                        AppNotificationKind::Warning,
                        rust_i18n::t!("store.reducer.autosquash_cancelled").to_string(),
                    ))
                } else {
                    match build_autosquash_plan(&entries, AutosquashMode::ToTop, base) {
                        Some(plan) => {
                            repo_state.set_autosquash_preview(Loadable::Ready(plan));
                            None
                        }
                        None => {
                            repo_state.set_autosquash_preview(Loadable::NotLoaded);
                            Some((
                                AppNotificationKind::Info,
                                rust_i18n::t!("store.reducer.autosquash_nothing_to_fold")
                                    .to_string(),
                            ))
                        }
                    }
                }
            }
            Err(e) => {
                push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                repo_state.set_autosquash_preview(Loadable::NotLoaded);
                Some((
                    AppNotificationKind::Error,
                    rust_i18n::t!("store.reducer.autosquash_load_failed", error = e).to_string(),
                ))
            }
        }
    };

    if let Some((kind, message)) = notice {
        push_notification(state, kind, message);
    }
    Vec::new()
}
