use super::util::{push_diagnostic, push_notification};
use crate::model::{AppNotificationKind, AppState, DiagnosticKind, Loadable, RepoId, RepoState};
use crate::msg::Effect;
use rustc_hash::FxHashSet;
use worktree_core::domain::CommitId;
use worktree_core::error::Error;
use worktree_core::services::{InteractiveRebaseAction, InteractiveRebaseEntry};

/// Validates the current multi-selection against the loaded log and HEAD.
/// This is the single reducer-side gate for every squash entry point.
pub(in crate::store::reducer) fn squash_plan_for_repo(
    repo_state: &RepoState,
) -> Option<worktree_core::squash::SquashPlan> {
    let Loadable::Ready(page) = &repo_state.log else {
        return None;
    };
    let head = repo_state.head_commit_id()?;
    worktree_core::squash::squash_eligibility(
        &page.commits,
        &repo_state.history_state.multi_selection.commits,
        &head,
    )
}

pub(in crate::store::reducer) fn prepare_squash(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    let Some(plan) = squash_plan_for_repo(repo_state) else {
        repo_state.history_state.squash_preview_pending = None;
        repo_state.set_squash_preview(Loadable::NotLoaded);
        return Vec::new();
    };

    repo_state.history_state.squash_preview_pending =
        Some((plan.oldest.clone(), plan.head.clone()));
    repo_state.set_squash_preview(Loadable::Loading);
    vec![Effect::LoadSquashMessagePreview {
        repo_id,
        oldest: plan.oldest,
        head: plan.head,
    }]
}

pub(in crate::store::reducer) fn squash_message_preview_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    oldest: CommitId,
    head: CommitId,
    result: std::result::Result<String, Error>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        // Accept the result only if it still matches the range we last asked
        // for. Keying off the recorded request (not the live plan) means a
        // transiently-invalid plan — e.g. HEAD momentarily unresolved during a
        // concurrent reload — does not drop the result and strand the preview
        // on Loading forever.
        let matches_request = repo_state.history_state.squash_preview_pending.as_ref()
            == Some(&(oldest.clone(), head.clone()));
        if matches_request {
            repo_state.history_state.squash_preview_pending = None;
            let value = match result {
                Ok(message) => {
                    let (subject, body) = worktree_core::squash::split_subject_body(&message);
                    Loadable::Ready(crate::model::SquashPreview {
                        oldest,
                        head,
                        subject,
                        body,
                    })
                }
                Err(e) => {
                    push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
                    Loadable::Error(e.to_string())
                }
            };
            repo_state.set_squash_preview(value);
        }
    }
    Vec::new()
}

pub(in crate::store::reducer) fn squash_rebase_setup_loaded(
    state: &mut AppState,
    repo_id: RepoId,
    base: String,
    actual_head: CommitId,
    selected_ids: Vec<CommitId>,
    reword_id: CommitId,
    message: String,
    count: usize,
    result: std::result::Result<Vec<InteractiveRebaseEntry>, Error>,
) -> Vec<Effect> {
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    let entries = match result {
        Ok(entries) => entries,
        Err(e) => {
            push_diagnostic(repo_state, DiagnosticKind::Error, e.to_string());
            push_notification(
                state,
                AppNotificationKind::Error,
                rust_i18n::t!("store.reducer.squash_rebase_load_failed", error = e).to_string(),
            );
            return Vec::new();
        }
    };

    let selected_strs: FxHashSet<&str> = selected_ids.iter().map(|id| id.as_ref()).collect();

    // The list loaded asynchronously, so re-validate it against the plan the
    // user confirmed before rewriting history. `git log --reverse base..HEAD`
    // yields commits oldest-first, so the last entry is the live HEAD.
    let head_unchanged = entries
        .last()
        .is_some_and(|e| e.commit_id == actual_head.as_ref());

    let mut matched = 0usize;
    let mut reword_found = false;
    let todo: Vec<InteractiveRebaseEntry> = entries
        .into_iter()
        .map(|mut entry| {
            if entry.commit_id == reword_id.as_ref() {
                entry.action = InteractiveRebaseAction::Reword;
                entry.new_message = Some(message.clone());
                reword_found = true;
                matched += 1;
            } else if selected_strs.contains(entry.commit_id.as_str()) {
                entry.action = InteractiveRebaseAction::Fixup;
                matched += 1;
            }
            entry
        })
        .collect();

    // Every selected commit must appear exactly once in the live range and the
    // oldest must have become the reword anchor; otherwise HEAD moved or the
    // range drifted between confirmation and now, and rewriting would either be
    // a silent no-op or touch the wrong commits. `matched == count` also
    // catches a selection count that disagrees with what was actually planned.
    if !head_unchanged || !reword_found || matched != count {
        push_notification(
            state,
            AppNotificationKind::Warning,
            rust_i18n::t!("store.reducer.squash_cancelled").to_string(),
        );
        return Vec::new();
    }

    super::begin_local_action(state, repo_id);
    vec![Effect::InteractiveRebase {
        repo_id,
        base,
        entries: todo,
        // Automated squash rebase — no editor window; reports as "Rebase".
        interactive: false,
    }]
}
