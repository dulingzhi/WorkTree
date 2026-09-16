mod actions_emit_effects;
mod conflict_interactions;
mod diff_selection;
mod external_and_history;
mod loaded_results;
mod repo_change;
mod repo_management;
mod util;

use crate::model::{
    AppState, AuthPromptState, AuthRetryOperation, BannerErrorState, PendingCommitRetry, RepoId,
    SubmoduleAddProgressState,
};
use crate::msg::{ConflictRegionChoice, Effect, Msg, RepoCommandKind, RepoPath, RepoPathList};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use worktree_core::auth::StagedGitAuth;
use worktree_core::services::{GitRepository, SafePushAfterCommitContext};

#[cfg(feature = "benchmarks")]
pub(crate) use diff_selection::SelectDiffEffects;
pub(crate) use repo_management::{ReorderRepoTabsEffects, SetActiveRepoEffects};

pub(crate) const SINGLE_PATH_ACTION_INLINE_EFFECT_CAPACITY: usize = 1;
pub(crate) type SinglePathActionEffects =
    SmallVec<[Effect; SINGLE_PATH_ACTION_INLINE_EFFECT_CAPACITY]>;
pub(crate) type BatchPathActionEffects =
    SmallVec<[Effect; SINGLE_PATH_ACTION_INLINE_EFFECT_CAPACITY]>;

// Dispatch passes unhandled messages through the domain chain by value; boxing
// `NotHandled` would allocate once per message on the hot reducer path.
#[allow(clippy::large_enum_variant)]
enum ReduceOutcome {
    Handled(Vec<Effect>),
    NotHandled(Msg),
}

#[cfg(test)]
pub(super) fn normalize_repo_path(path: std::path::PathBuf) -> std::path::PathBuf {
    util::normalize_repo_path(path)
}

fn normalize_repo_relative_path(
    repo_workdir: &std::path::Path,
    path: std::path::PathBuf,
) -> std::path::PathBuf {
    let path = if path.is_relative() {
        repo_workdir.join(path)
    } else {
        path
    };
    util::canonicalize_path(path)
}

#[inline]
fn begin_local_action(state: &mut AppState, repo_id: RepoId) {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.local_actions_in_flight = repo_state.local_actions_in_flight.saturating_add(1);
        repo_state.bump_ops_rev();
    }
}

fn begin_commit_action(state: &mut AppState, repo_id: RepoId) {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.local_actions_in_flight = repo_state.local_actions_in_flight.saturating_add(1);
        repo_state.commit_in_flight = repo_state.commit_in_flight.saturating_add(1);
        repo_state.pending_force_push_lease = None;
        repo_state.bump_ops_rev();
    }
}

fn begin_head_changing_local_action(state: &mut AppState, repo_id: RepoId) {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.local_actions_in_flight = repo_state.local_actions_in_flight.saturating_add(1);
        repo_state.clear_head_dependent_cached_state();
        repo_state.bump_ops_rev();
    }
}

fn start_submodule_add_progress(
    state: &mut AppState,
    repo_id: RepoId,
    url: &str,
    path: &std::path::Path,
) {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.submodule_add_in_flight = Some(SubmoduleAddProgressState {
            url: url.to_string(),
            path: path.to_path_buf(),
        });
    }
}

pub(crate) fn msg_requires_available_git(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::OpenRepo(_)
            | Msg::RestoreSession { .. }
            | Msg::ReloadRepo { .. }
            | Msg::RepoActivated { .. }
            | Msg::RepoExternallyChanged { .. }
            | Msg::SetHistoryScope { .. }
            | Msg::SetHistoryAuthorFilter { .. }
            | Msg::LoadMoreHistory { .. }
            | Msg::SelectCommit { .. }
            | Msg::SelectWorkingTreeSummary { .. }
            | Msg::CompareCommitRange { .. }
            | Msg::CompareWithMarked { .. }
            | Msg::CompareWithWorkingTree { .. }
            | Msg::SelectDiff { .. }
            | Msg::SelectConflictDiff { .. }
            | Msg::SelectWorktreeUncommitted { .. }
            | Msg::LoadStashes { .. }
            | Msg::LoadRepoStatistics { .. }
            | Msg::LoadConflictFile { .. }
            | Msg::LoadReflog { .. }
            | Msg::LoadRecentCommitMessages { .. }
            | Msg::SearchCommits { .. }
            | Msg::LoadAiCommitContext { .. }
            | Msg::LoadHoverCommitMessage { .. }
            | Msg::LoadFileHistory { .. }
            | Msg::LoadBlame { .. }
            | Msg::LoadWorktrees { .. }
            | Msg::LoadWorktreeDirty { .. }
            | Msg::LoadRefMetadata { .. }
            | Msg::LoadSubmodules { .. }
            | Msg::LoadSubmodule { .. }
            | Msg::LoadTags { .. }
            | Msg::LoadRemoteTags { .. }
            | Msg::RefreshBranches { .. }
            | Msg::LoadFileBrowser { .. }
            | Msg::OpenFileContent { .. }
            | Msg::OpenFileEditor { .. }
            | Msg::OpenFileAtCommitParent { .. }
            | Msg::OpenFileAtCommit { .. }
            | Msg::BrowseRepositoryAtCommit { .. }
            | Msg::RevealCommit { .. }
            | Msg::ResetBrowseToLive { .. }
            | Msg::ViewerNavBack { .. }
            | Msg::ViewerNavForward { .. }
            | Msg::GlobalNavBack { .. }
            | Msg::GlobalNavForward { .. }
            | Msg::StageHunk { .. }
            | Msg::UnstageHunk { .. }
            | Msg::ApplyWorktreePatch { .. }
            | Msg::CheckoutBranch { .. }
            | Msg::CheckoutRemoteBranch { .. }
            | Msg::CheckoutCommit { .. }
            | Msg::CherryPickCommit { .. }
            | Msg::RevertCommit { .. }
            | Msg::CreateBranch { .. }
            | Msg::CreateBranchAndCheckout { .. }
            | Msg::RenameBranch { .. }
            | Msg::DeleteBranch { .. }
            | Msg::ForceDeleteBranch { .. }
            | Msg::DeleteBranches { .. }
            | Msg::CloneRepo { .. }
            | Msg::ExportPatch { .. }
            | Msg::ArchiveZip { .. }
            | Msg::CleanupRepo { .. }
            | Msg::ApplyPatch { .. }
            | Msg::AddWorktree { .. }
            | Msg::RemoveWorktree { .. }
            | Msg::ForceRemoveWorktree { .. }
            | Msg::AddSubmodule { .. }
            | Msg::UpdateSubmodules { .. }
            | Msg::ChangeSubmodulePointer { .. }
            | Msg::RemoveSubmodule { .. }
            | Msg::StagePath { .. }
            | Msg::StagePaths { .. }
            | Msg::UnstagePath { .. }
            | Msg::UnstagePaths { .. }
            | Msg::DiscardWorktreeChangesPath { .. }
            | Msg::DiscardWorktreeChangesPaths { .. }
            | Msg::SaveWorktreeFile { .. }
            | Msg::AppendGitignorePatterns { .. }
            | Msg::Commit { .. }
            | Msg::CommitAmend { .. }
            | Msg::SafePushAfterCommit { .. }
            | Msg::FetchAll { .. }
            | Msg::AutoFetchAll { .. }
            | Msg::PruneMergedBranches { .. }
            | Msg::PruneLocalTags { .. }
            | Msg::Pull { .. }
            | Msg::PullBranch { .. }
            | Msg::MergeRef { .. }
            | Msg::SquashRef { .. }
            | Msg::Push { .. }
            | Msg::PushAfterCommit { .. }
            | Msg::ForcePush { .. }
            | Msg::ForcePushWithLease { .. }
            | Msg::PushMergeRequest { .. }
            | Msg::PushSetUpstream { .. }
            | Msg::SetUpstreamBranch { .. }
            | Msg::UnsetUpstreamBranch { .. }
            | Msg::FastForwardBranch { .. }
            | Msg::DeleteRemoteBranch { .. }
            | Msg::DeleteRemoteBranches { .. }
            | Msg::Reset { .. }
            | Msg::PrepareSquash { .. }
            | Msg::SquashCommits { .. }
            | Msg::Rebase { .. }
            | Msg::RebaseContinue { .. }
            | Msg::RebaseAbort { .. }
            | Msg::BisectStart { .. }
            | Msg::BisectMark { .. }
            | Msg::BisectReset { .. }
            | Msg::InteractiveRebase { .. }
            | Msg::InteractiveCherryPick { .. }
            | Msg::MergeAbort { .. }
            | Msg::CreateTag { .. }
            | Msg::DeleteTag { .. }
            | Msg::PushTag { .. }
            | Msg::DeleteRemoteTag { .. }
            | Msg::AddRemote { .. }
            | Msg::RemoveRemote { .. }
            | Msg::SetRemoteUrl { .. }
            | Msg::SetRemoteSshKey { .. }
            | Msg::CheckoutConflictSide { .. }
            | Msg::AcceptConflictDeletion { .. }
            | Msg::CheckoutConflictBase { .. }
            | Msg::LaunchMergetool { .. }
            | Msg::Stash { .. }
            | Msg::ApplyStash { .. }
            | Msg::PopStash { .. }
            | Msg::DropStash { .. }
            | Msg::StashBranch { .. }
            | Msg::SetAssumeUnchanged { .. }
    )
}

#[cfg(test)]
pub(super) fn push_diagnostic(
    repo_state: &mut crate::model::RepoState,
    kind: crate::model::DiagnosticKind,
    message: String,
) {
    util::push_diagnostic(repo_state, kind, message)
}

#[cfg(test)]
pub(super) fn handle_session_persist_result(
    state: &mut crate::model::AppState,
    repo_id: Option<crate::model::RepoId>,
    action: &'static str,
    result: std::io::Result<()>,
) {
    util::handle_session_persist_result(state, repo_id, action, result)
}

fn auth_prompt_for_repo_command(
    repo_id: RepoId,
    command: &RepoCommandKind,
    error: &worktree_core::error::Error,
) -> Option<AuthPromptState> {
    let kind = util::detect_auth_prompt_kind(error)?;
    let operation = AuthRetryOperation::RepoCommand {
        repo_id,
        command: command.clone(),
    };
    retry_msg_for_auth_operation(operation.clone())?;
    Some(AuthPromptState {
        kind,
        reason: util::format_error_for_user(error),
        operation,
    })
}

fn auth_prompt_for_safe_push_after_commit(
    repo_id: RepoId,
    context: SafePushAfterCommitContext,
    error: &worktree_core::error::Error,
) -> Option<AuthPromptState> {
    let kind = util::detect_auth_prompt_kind(error)?;
    Some(AuthPromptState {
        kind,
        reason: util::format_error_for_user(error),
        operation: AuthRetryOperation::SafePushAfterCommit { repo_id, context },
    })
}

fn auth_prompt_for_commit(
    repo_id: RepoId,
    pending: Option<PendingCommitRetry>,
    error: &worktree_core::error::Error,
) -> Option<AuthPromptState> {
    let kind = util::detect_auth_prompt_kind(error)?;
    let pending = pending?;
    Some(AuthPromptState {
        kind,
        reason: util::format_error_for_user(error),
        operation: AuthRetryOperation::Commit {
            repo_id,
            message: pending.message,
            amend: pending.amend,
            push_after_commit: pending.push_after_commit,
        },
    })
}

fn auth_prompt_for_clone(
    url: &str,
    dest: &std::path::Path,
    ssh_key: Option<&str>,
    error: &worktree_core::error::Error,
) -> Option<AuthPromptState> {
    let kind = util::detect_auth_prompt_kind(error)?;
    Some(AuthPromptState {
        kind,
        reason: util::format_error_for_user(error),
        operation: AuthRetryOperation::Clone {
            url: url.to_string(),
            dest: dest.to_path_buf(),
            ssh_key: ssh_key.map(str::to_string),
        },
    })
}

fn retry_msg_for_auth_operation(operation: AuthRetryOperation) -> Option<Msg> {
    match operation {
        AuthRetryOperation::RepoCommand { repo_id, command } => {
            retry_msg_for_repo_command(repo_id, command)
        }
        AuthRetryOperation::SafePushAfterCommit { repo_id, context } => {
            Some(Msg::SafePushAfterCommit { repo_id, context })
        }
        AuthRetryOperation::Commit {
            repo_id,
            message,
            amend,
            push_after_commit,
        } => Some(if amend {
            Msg::CommitAmend {
                repo_id,
                message,
                push_after_commit,
            }
        } else {
            Msg::Commit {
                repo_id,
                message,
                push_after_commit,
            }
        }),
        AuthRetryOperation::Clone { url, dest, ssh_key } => {
            Some(Msg::CloneRepo { url, dest, ssh_key })
        }
    }
}

fn clear_banner_error_for_auth_operation(state: &mut AppState, operation: &AuthRetryOperation) {
    match operation {
        AuthRetryOperation::RepoCommand { repo_id, .. }
        | AuthRetryOperation::SafePushAfterCommit { repo_id, .. }
        | AuthRetryOperation::Commit { repo_id, .. } => {
            util::clear_banner_error_for_repo(state, *repo_id);
        }
        AuthRetryOperation::Clone { .. } => clear_stale_clone_banner_error(state),
    }
}

fn clear_stale_clone_banner_error(state: &mut AppState) {
    if state
        .banner_error
        .as_ref()
        .is_some_and(|banner| banner.message.starts_with("Clone failed"))
    {
        state.banner_error = None;
    }
}

fn retry_msg_for_repo_command(repo_id: RepoId, command: RepoCommandKind) -> Option<Msg> {
    Some(match command {
        // Automatic fetches never open the auth prompt, so there is nothing to
        // retry with; the next activation fetches again on its own.
        RepoCommandKind::AutoFetchAll => return None,
        RepoCommandKind::FetchAll => Msg::FetchAll { repo_id },
        RepoCommandKind::PruneMergedBranches => Msg::PruneMergedBranches { repo_id },
        RepoCommandKind::PruneLocalTags => Msg::PruneLocalTags { repo_id },
        RepoCommandKind::Pull { mode } => Msg::Pull { repo_id, mode },
        RepoCommandKind::PullBranch { remote, branch } => Msg::PullBranch {
            repo_id,
            remote,
            branch,
        },
        RepoCommandKind::MergeRef { reference } => Msg::MergeRef { repo_id, reference },
        RepoCommandKind::SquashRef { reference } => Msg::SquashRef { repo_id, reference },
        // Auth retries never carry pull-retry intent: a credential failure is
        // never a behind-remote rejection, and re-dispatching unarmed keeps
        // `push_pull_retry_armed` from leaking into an unrelated push.
        RepoCommandKind::Push => Msg::Push {
            repo_id,
            pull_retry: false,
        },
        RepoCommandKind::PushAfterCommit {
            target,
            set_upstream,
        } => Msg::PushAfterCommit {
            repo_id,
            target,
            set_upstream,
        },
        RepoCommandKind::ForcePush => Msg::ForcePush { repo_id },
        RepoCommandKind::ForcePushWithLease { lease } => Msg::ForcePushWithLease { repo_id, lease },
        RepoCommandKind::PushMergeRequest { options } => Msg::PushMergeRequest { repo_id, options },
        RepoCommandKind::PushSetUpstream { remote, branch } => Msg::PushSetUpstream {
            repo_id,
            remote,
            branch,
        },
        RepoCommandKind::SetUpstreamBranch { branch, upstream } => Msg::SetUpstreamBranch {
            repo_id,
            branch,
            upstream,
        },
        RepoCommandKind::UnsetUpstreamBranch { branch } => {
            Msg::UnsetUpstreamBranch { repo_id, branch }
        }
        RepoCommandKind::FastForwardBranch { branch } => Msg::FastForwardBranch { repo_id, branch },
        RepoCommandKind::DeleteRemoteBranch { remote, branch } => Msg::DeleteRemoteBranch {
            repo_id,
            remote,
            branch,
        },
        RepoCommandKind::DeleteRemoteBranches { remote, branches } => Msg::DeleteRemoteBranches {
            repo_id,
            remote,
            branches,
        },
        RepoCommandKind::Reset { mode, target } => Msg::Reset {
            repo_id,
            target,
            mode,
        },
        RepoCommandKind::SquashCommits {
            oldest,
            expected_head,
            message,
            count,
        } => Msg::SquashCommits {
            repo_id,
            oldest,
            expected_head,
            message,
            count,
        },
        RepoCommandKind::Rebase { onto } => Msg::Rebase { repo_id, onto },
        RepoCommandKind::RebaseContinue => Msg::RebaseContinue { repo_id },
        RepoCommandKind::RebaseAbort => Msg::RebaseAbort { repo_id },
        // Bisect commands never need auth, so this replay path never fires for
        // them — but a faithful mapping keeps the match exhaustive.
        RepoCommandKind::BisectStart { bad, goods } => Msg::BisectStart {
            repo_id,
            bad,
            goods,
        },
        RepoCommandKind::BisectMark { verdict, commit } => Msg::BisectMark {
            repo_id,
            verdict,
            commit,
        },
        RepoCommandKind::BisectReset => Msg::BisectReset { repo_id },
        // Sequencer commands only reach an auth prompt through a signing
        // passphrase failure, and by then git has already left cherry-pick
        // or rebase state on disk: replaying the original plan would be
        // rejected as already in progress (and its effect has no auth slot).
        // Continue the paused sequencer with the staged auth instead.
        RepoCommandKind::InteractiveCherryPick { .. } => Msg::RebaseContinue { repo_id },
        RepoCommandKind::CherryPick {
            commit_id,
            commit,
            mainline,
            summary,
        } => {
            if commit {
                Msg::RebaseContinue { repo_id }
            } else {
                // `--no-commit` picks never sign, so an auth prompt here is
                // not a paused sequencer; replay the command itself.
                Msg::CherryPickCommit {
                    repo_id,
                    commit_id,
                    commit,
                    mainline,
                    summary,
                }
            }
        }
        RepoCommandKind::MergeAbort => Msg::MergeAbort { repo_id },
        RepoCommandKind::CreateTag {
            name,
            target,
            message,
            annotated,
        } => Msg::CreateTag {
            repo_id,
            name,
            target,
            message,
            annotated,
        },
        RepoCommandKind::DeleteTag { name } => Msg::DeleteTag { repo_id, name },
        RepoCommandKind::PushTag { remote, name } => Msg::PushTag {
            repo_id,
            remote,
            name,
        },
        RepoCommandKind::DeleteRemoteTag { remote, name } => Msg::DeleteRemoteTag {
            repo_id,
            remote,
            name,
        },
        RepoCommandKind::AddRemote { name, url } => Msg::AddRemote { repo_id, name, url },
        RepoCommandKind::RemoveRemote { name } => Msg::RemoveRemote { repo_id, name },
        RepoCommandKind::SetRemoteUrl { name, url, kind } => Msg::SetRemoteUrl {
            repo_id,
            name,
            url,
            kind,
        },
        RepoCommandKind::SetRemoteSshKey { remote, key } => Msg::SetRemoteSshKey {
            repo_id,
            remote,
            key,
        },
        RepoCommandKind::CheckoutConflict { path, side } => Msg::CheckoutConflictSide {
            repo_id,
            path,
            side,
        },
        RepoCommandKind::AcceptConflictDeletion { path } => {
            Msg::AcceptConflictDeletion { repo_id, path }
        }
        RepoCommandKind::CheckoutConflictBase { path } => {
            Msg::CheckoutConflictBase { repo_id, path }
        }
        RepoCommandKind::LaunchMergetool { path, preference } => Msg::LaunchMergetool {
            repo_id,
            path,
            preference,
        },
        RepoCommandKind::ExportPatch { commit_id, dest } => Msg::ExportPatch {
            repo_id,
            commit_id,
            dest,
        },
        RepoCommandKind::Cleanup => Msg::CleanupRepo { repo_id },
        RepoCommandKind::ArchiveZip { revision, dest } => Msg::ArchiveZip {
            repo_id,
            revision,
            dest,
        },
        RepoCommandKind::ApplyPatch { patch } => Msg::ApplyPatch { repo_id, patch },
        RepoCommandKind::AddWorktree { path, reference } => Msg::AddWorktree {
            repo_id,
            path,
            reference,
        },
        RepoCommandKind::RemoveWorktree { path } => Msg::RemoveWorktree { repo_id, path },
        RepoCommandKind::ForceRemoveWorktree { path } => Msg::ForceRemoveWorktree { repo_id, path },
        RepoCommandKind::AddSubmodule {
            url,
            path,
            branch,
            name,
            force,
            approved_sources,
        } => Msg::AddSubmoduleTrusted {
            repo_id,
            url,
            path,
            branch,
            name,
            force,
            approved_sources,
        },
        RepoCommandKind::UpdateSubmodules { approved_sources } => Msg::UpdateSubmodulesTrusted {
            repo_id,
            approved_sources,
        },
        RepoCommandKind::LoadSubmodule {
            path,
            approved_sources,
        } => Msg::LoadSubmoduleTrusted {
            repo_id,
            path,
            approved_sources,
        },
        RepoCommandKind::ChangeSubmodulePointer { path, reference } => {
            Msg::ChangeSubmodulePointer {
                repo_id,
                path,
                reference,
            }
        }
        RepoCommandKind::RemoveSubmodule { path } => Msg::RemoveSubmodule { repo_id, path },
        // A signing failure mid-rebase leaves git's state (and WorkTree's
        // persisted reword messages) on disk; continue it with the staged
        // auth like the cherry-pick commands above.
        RepoCommandKind::InteractiveRebase { .. } => Msg::RebaseContinue { repo_id },
        // Writes `.gitignore` on the local filesystem, so it never fails for
        // want of credentials — and this replay path exists only to re-run a
        // command after an auth prompt. Retaining `patterns` would make a replay
        // possible; there is just nothing here that an auth prompt could fix.
        RepoCommandKind::AppendGitignorePatterns { .. } => return None,
        // Not replayable because command metadata does not retain original content.
        RepoCommandKind::SaveWorktreeFile { .. }
        | RepoCommandKind::StageHunk
        | RepoCommandKind::UnstageHunk
        | RepoCommandKind::ApplyWorktreePatch { .. } => return None,
    })
}

fn attach_git_auth_to_effects(mut effects: Vec<Effect>, auth: StagedGitAuth) -> Vec<Effect> {
    let Some(first) = effects.first_mut() else {
        return effects;
    };

    match first {
        Effect::CloneRepo { auth: slot, .. }
        | Effect::AddSubmodule { auth: slot, .. }
        | Effect::UpdateSubmodules { auth: slot, .. }
        | Effect::LoadSubmodule { auth: slot, .. }
        | Effect::Commit { auth: slot, .. }
        | Effect::CommitAmend { auth: slot, .. }
        | Effect::SafePushAfterCommit { auth: slot, .. }
        | Effect::FetchAll { auth: slot, .. }
        | Effect::Pull { auth: slot, .. }
        | Effect::PullBranch { auth: slot, .. }
        | Effect::Push { auth: slot, .. }
        | Effect::PushAfterCommit { auth: slot, .. }
        | Effect::ForcePush { auth: slot, .. }
        | Effect::ForcePushWithLease { auth: slot, .. }
        | Effect::PushMergeRequest { auth: slot, .. }
        | Effect::PushSetUpstream { auth: slot, .. }
        | Effect::DeleteRemoteBranch { auth: slot, .. }
        | Effect::DeleteRemoteBranches { auth: slot, .. }
        | Effect::PushTag { auth: slot, .. }
        | Effect::DeleteRemoteTag { auth: slot, .. }
        | Effect::RebaseContinue { auth: slot, .. } => {
            *slot = Some(auth);
        }
        _ => {}
    }

    effects
}

pub(crate) fn fill_set_active_repo_inline(
    state: &mut AppState,
    repo_id: RepoId,
    effects: &mut SetActiveRepoEffects,
) {
    repo_management::fill_set_active_repo_inline(state, repo_id, effects)
}

pub(crate) fn fill_reorder_repo_tabs_inline(
    state: &mut AppState,
    repo_id: RepoId,
    insert_before: Option<RepoId>,
    effects: &mut ReorderRepoTabsEffects,
) {
    repo_management::fill_reorder_repo_tabs_inline(state, repo_id, insert_before, effects)
}

// The only non-benchmark consumers of `fill_select_diff_inline` live inside
// the reducer submodule (via the unconditional `pub(super)` definition in
// `diff_selection.rs`). This public re-export exists solely for the benchmark
// helper in `store/mod.rs` so that the inline reduce path can be measured.
#[cfg(feature = "benchmarks")]
pub(crate) fn fill_select_diff_inline(
    state: &mut AppState,
    repo_id: RepoId,
    target: worktree_core::domain::DiffTarget,
    content_preview: bool,
    effects: &mut SelectDiffEffects,
) {
    let mode = if content_preview {
        diff_selection::ContentViewMode::Preview
    } else {
        diff_selection::ContentViewMode::Diff
    };
    diff_selection::fill_select_diff_inline(state, repo_id, target, mode, effects)
}

#[inline]
pub(crate) fn fill_stage_path_inline(
    state: &mut AppState,
    repo_id: RepoId,
    path: std::path::PathBuf,
    effects: &mut SinglePathActionEffects,
) {
    begin_local_action(state, repo_id);
    effects.push(Effect::StagePath { repo_id, path });
}

#[inline]
pub(crate) fn fill_stage_paths_inline(
    state: &mut AppState,
    repo_id: RepoId,
    paths: RepoPathList,
    effects: &mut BatchPathActionEffects,
) {
    begin_local_action(state, repo_id);
    effects.push(Effect::StagePaths { repo_id, paths });
}

#[inline]
pub(crate) fn fill_unstage_path_inline(
    state: &mut AppState,
    repo_id: RepoId,
    path: std::path::PathBuf,
    effects: &mut SinglePathActionEffects,
) {
    begin_local_action(state, repo_id);
    effects.push(Effect::UnstagePath { repo_id, path });
}

#[inline]
pub(crate) fn fill_unstage_paths_inline(
    state: &mut AppState,
    repo_id: RepoId,
    paths: RepoPathList,
    effects: &mut BatchPathActionEffects,
) {
    begin_local_action(state, repo_id);
    effects.push(Effect::UnstagePaths { repo_id, paths });
}

#[inline]
pub(crate) fn set_conflict_region_choice_inline(
    state: &mut AppState,
    repo_id: RepoId,
    path: RepoPath,
    region_index: usize,
    choice: ConflictRegionChoice,
) {
    conflict_interactions::set_region_choice_inline(state, repo_id, path, region_index, choice);
}

#[inline]
pub(crate) fn reset_conflict_resolutions_inline(
    state: &mut AppState,
    repo_id: RepoId,
    path: RepoPath,
) {
    conflict_interactions::reset_resolutions_inline(state, repo_id, path);
}

fn submit_auth_prompt(
    repos: &mut FxHashMap<RepoId, Arc<dyn GitRepository>>,
    id_alloc: &AtomicU64,
    state: &mut AppState,
    username: Option<String>,
    secret: String,
) -> Vec<Effect> {
    let Some(prompt) = state.auth_prompt.take() else {
        return Vec::new();
    };

    let username = username
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let auth = match util::prepare_staged_git_auth(prompt.kind, username.as_deref(), &secret) {
        Ok(auth) => auth,
        Err(err) => {
            state.auth_prompt = Some(prompt);
            return if let Some(repo_state) = state
                .active_repo
                .and_then(|repo_id| state.repos.iter_mut().find(|r| r.id == repo_id))
            {
                util::push_diagnostic(
                    repo_state,
                    crate::model::DiagnosticKind::Error,
                    util::format_error_for_user(&err),
                );
                Vec::new()
            } else {
                Vec::new()
            };
        }
    };

    clear_banner_error_for_auth_operation(state, &prompt.operation);

    match retry_msg_for_auth_operation(prompt.operation) {
        Some(msg) => attach_git_auth_to_effects(reduce(repos, id_alloc, state, msg), auth),
        None => Vec::new(),
    }
}

pub(super) fn reduce(
    repos: &mut FxHashMap<RepoId, Arc<dyn GitRepository>>,
    id_alloc: &AtomicU64,
    state: &mut AppState,
    msg: Msg,
) -> Vec<Effect> {
    let reconcile = !matches!(
        msg,
        Msg::GlobalNavBack { .. } | Msg::GlobalNavForward { .. }
    );
    let push = is_view_navigation(&msg);

    if reconcile {
        reconcile_active_nav_history(state, false);
    }

    let effects = reduce_inner(repos, id_alloc, state, msg);

    // Enforced here rather than at each of the four places a worktree selection
    // can end; see the helper.
    loaded_results::retire_orphaned_worktree_diffs(state);

    if reconcile {
        reconcile_active_nav_history(state, push);
    }

    effects
}

/// Whether `msg` is a user-initiated navigation that should create a new global
/// back/forward step (as opposed to a background change folded into the current
/// step). `GlobalNav*` replays are handled separately and never reach here as a
/// "push".
fn is_view_navigation(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::SelectDiff { .. }
            | Msg::SelectConflictDiff { .. }
            | Msg::SelectCommit { .. }
            // Selecting a linked-worktree row is a destination like any other
            // history selection; it just is not a commit.
            | Msg::SelectWorktreeUncommitted { .. }
            // So is selecting this checkout's uncommitted-changes row.
            | Msg::SelectWorkingTreeSummary { .. }
            | Msg::CompareCommitRange { .. }
            | Msg::CompareWithMarked { .. }
            | Msg::CompareWithWorkingTree { .. }
            | Msg::OpenFileContent { .. }
            | Msg::OpenFileEditor { .. }
            // Leaving the editor is a destination of its own, so Back returns to
            // the editor rather than skipping past it to whatever preceded it.
            | Msg::ExitDiffEditMode { .. }
            | Msg::OpenFileAtCommit { .. }
            | Msg::BrowseRepositoryAtCommit { .. }
            // A reveal moves the main view when its reference resolves, not
            // when it is asked for.
            | Msg::Internal(crate::msg::InternalMsg::CommitRevealResolved { .. })
            | Msg::ResetBrowseToLive { .. }
            | Msg::OpenInlineSubmoduleDiff { .. }
            | Msg::SelectInlineSubmoduleDiff { .. }
    )
}

/// Sync the active repo's global navigation history against the current
/// main-view snapshot. See [`crate::model::NavStack::reconcile`].
fn reconcile_active_nav_history(state: &mut AppState, push: bool) {
    let Some(repo_id) = state.active_repo else {
        return;
    };
    let Some(repo) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return;
    };
    // Hot path: most messages don't move the main view, so the snapshot still
    // matches the current entry and `reconcile` would no-op. Compare by borrow
    // first and bail before cloning a `MainViewSnapshot` (which owns a `PathBuf`)
    // — this runs twice per dispatched message.
    let cursor = repo.nav_history.cursor;
    if let Some(current) = repo.nav_history.entries.get(cursor)
        && repo.main_view_snapshot_matches(current)
    {
        return;
    }
    let cur = repo.main_view_snapshot();
    repo.nav_history.reconcile(cur, push);
}

fn reduce_inner(
    repos: &mut FxHashMap<RepoId, Arc<dyn GitRepository>>,
    id_alloc: &AtomicU64,
    state: &mut AppState,
    msg: Msg,
) -> Vec<Effect> {
    if msg_requires_available_git(&msg) && !state.git_runtime.is_available() {
        return Vec::new();
    }

    let msg = match repo_management::reduce_repo_management(msg, repos, id_alloc, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    let msg = match external_and_history::reduce_external_and_history(msg, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    let msg = match loaded_results::reduce_loaded_results(msg, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    let msg = match diff_selection::reduce_diff_selection(msg, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    let msg = match actions_emit_effects::reduce_actions_emit_effects(msg, repos, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    let msg = match conflict_interactions::reduce_conflict_interactions(msg, state) {
        ReduceOutcome::Handled(effects) => return effects,
        ReduceOutcome::NotHandled(msg) => msg,
    };

    match msg {
        Msg::ShowBannerError { repo_id, message } => {
            if !message.trim().is_empty() {
                state.banner_error = Some(BannerErrorState { repo_id, message });
            }
            Vec::new()
        }
        Msg::DismissBannerError => {
            state.banner_error = None;
            Vec::new()
        }
        Msg::DismissRepoError { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.last_error = None;
            }
            util::clear_banner_error_for_repo(state, repo_id);
            Vec::new()
        }
        Msg::SubmitAuthPrompt { username, secret } => {
            submit_auth_prompt(repos, id_alloc, state, username, secret)
        }
        Msg::CancelAuthPrompt => {
            state.auth_prompt = None;
            util::clear_staged_git_auth_env();
            Vec::new()
        }
        Msg::SetGitRuntimeState(runtime) => {
            state.git_runtime = runtime;
            Vec::new()
        }
        Msg::SetGitLogSettings {
            show_history_tags,
            tag_fetch_mode,
        } => {
            state.git_log_settings.show_history_tags = show_history_tags;
            state.git_log_settings.tag_fetch_mode = tag_fetch_mode;
            Vec::new()
        }
        Msg::SetDefaultTagType(tag_type) => {
            state.default_tag_type = tag_type;
            Vec::new()
        }
        other => unreachable!("reduce_inner dispatch chain covers every Msg variant: {other:?}"),
    }
}

#[cfg(test)]
mod nav_history_tests {
    use super::*;
    use crate::model::{AppState, RepoState};
    use std::sync::atomic::AtomicU64;
    use worktree_core::domain::{CommitId, DiffArea, DiffTarget, RepoSpec};
    use worktree_core::process::{
        GitExecutableAvailability, GitExecutablePreference, GitRuntimeState,
    };

    fn available_state_with_repo(repo_id: RepoId) -> AppState {
        let mut state = AppState::default();
        state.git_runtime = GitRuntimeState {
            preference: GitExecutablePreference::SystemPath,
            availability: GitExecutableAvailability::Available {
                version_output: "git version 2.0.0".to_string(),
            },
        };
        state.repos.push(RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: std::path::PathBuf::from("/tmp/repo"),
            },
        ));
        state.active_repo = Some(repo_id);
        state
    }

    fn dispatch(state: &mut AppState, msg: Msg) {
        let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
        let id_alloc = AtomicU64::new(99);
        let _ = reduce(&mut repos, &id_alloc, state, msg);
    }

    fn repo(state: &AppState, repo_id: RepoId) -> &RepoState {
        state.repos.iter().find(|r| r.id == repo_id).unwrap()
    }

    #[test]
    fn repo_watch_degraded_pushes_warning_notification() {
        let mut state = AppState::default();
        dispatch(
            &mut state,
            Msg::RepoWatchDegraded {
                repo_id: RepoId(1),
                reason: crate::msg::RepoWatchDegradedReason::TooManyFolders { dir_count: 9000 },
            },
        );
        assert_eq!(state.notifications.len(), 1);
        let note = &state.notifications[0];
        assert_eq!(note.kind, crate::model::AppNotificationKind::Warning);
        assert!(
            note.message.contains("9000"),
            "warning should mention the folder count: {}",
            note.message
        );

        // A partial watch failure surfaces a (distinct) warning too — not just the stderr log.
        dispatch(
            &mut state,
            Msg::RepoWatchDegraded {
                repo_id: RepoId(1),
                reason: crate::msg::RepoWatchDegradedReason::WatchLimitReached {
                    unwatched_dirs: 42,
                },
            },
        );
        assert_eq!(state.notifications.len(), 2);
        let note = &state.notifications[1];
        assert_eq!(note.kind, crate::model::AppNotificationKind::Warning);
        assert!(
            note.message.contains("42"),
            "partial-watch warning should mention the unwatched count: {}",
            note.message
        );
    }

    /// A linked-worktree row is a third kind of history selection, and selecting
    /// one clears the commit selection. Left out of the navigation machinery it
    /// read as "the view went back to the log": the entry for the commit the user
    /// came from was overwritten in place, so Back skipped it, and no snapshot
    /// could reproduce the worktree row on the way forward.
    #[test]
    fn selecting_a_worktree_row_is_a_navigation_step_of_its_own() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit = CommitId("abc".into());
        let worktree = std::path::PathBuf::from("/tmp/wt/a");

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectWorktreeUncommitted {
                repo_id,
                path: worktree.clone(),
            },
        );
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit,
            None,
            "the worktree row displaces the commit selection"
        );

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit.as_ref(),
            Some(&commit),
            "back must return to the commit the worktree row was selected from"
        );
        assert_eq!(repo(&state, repo_id).history_state.worktree_selection, None);

        dispatch(&mut state, Msg::GlobalNavForward { repo_id });
        assert_eq!(
            repo(&state, repo_id)
                .history_state
                .worktree_selection
                .as_ref(),
            Some(&worktree),
            "forward must reproduce the worktree row, not just clear the commit"
        );
        assert_eq!(repo(&state, repo_id).history_state.selected_commit, None);
    }

    /// The uncommitted-changes row is a fourth kind of history selection. Like
    /// the worktree row, it must be a navigation step of its own — otherwise
    /// Back would skip the commit the user came from, and Forward could not
    /// reproduce the working-tree review.
    #[test]
    fn selecting_the_working_tree_row_is_a_navigation_step_of_its_own() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit = CommitId("abc".into());

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit.clone(),
            },
        );
        dispatch(&mut state, Msg::SelectWorkingTreeSummary { repo_id });
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit,
            Some(CommitId::uncommitted()),
            "the working-tree row displaces the commit selection"
        );

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit.as_ref(),
            Some(&commit),
            "back must return to the commit the working-tree row was selected from"
        );

        dispatch(&mut state, Msg::GlobalNavForward { repo_id });
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit,
            Some(CommitId::uncommitted()),
            "forward must reproduce the working-tree review"
        );
    }

    #[test]
    fn opening_a_file_diff_is_recorded_and_back_restores_the_log() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let target = DiffTarget::WorkingTree {
            path: std::path::PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        };

        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: target.clone(),
            },
        );
        assert_eq!(repo(&state, repo_id).diff_state.diff_target, Some(target));
        // Origin (history log) seeded + the diff.
        assert_eq!(repo(&state, repo_id).nav_history.entries.len(), 2);

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        assert_eq!(
            repo(&state, repo_id).diff_state.diff_target,
            None,
            "back closes the file diff and shows the history log"
        );

        dispatch(&mut state, Msg::GlobalNavForward { repo_id });
        assert!(
            repo(&state, repo_id).diff_state.diff_target.is_some(),
            "forward reopens the file diff"
        );
    }

    #[test]
    fn commit_then_file_diffs_are_all_remembered() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit_a = CommitId("aaa".into());
        let file1 = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("file1.rs")),
        };
        let file2 = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("file2.rs")),
        };

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit_a.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file1.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file2.clone(),
            },
        );

        let entries = &repo(&state, repo_id).nav_history.entries;
        assert!(entries.iter().any(|e| e.diff_target == Some(file1.clone())));
        assert!(entries.iter().any(|e| e.diff_target == Some(file2.clone())));

        // Back must step one-by-one: file2 diff -> file1 diff -> commit details
        // (commit selected, no diff) -> history log.
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        assert_eq!(
            repo(&state, repo_id).diff_state.diff_target,
            Some(file1.clone())
        );

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(r.diff_state.diff_target, None, "should show commit details");
        assert_eq!(
            r.history_state.selected_commit.as_ref(),
            Some(&commit_a),
            "commit should still be selected at the details step"
        );

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit,
            None,
            "final back returns to the history log with no commit selected"
        );
    }

    #[test]
    fn view_navigation_messages_push_others_fold_in_place() {
        // User navigations create a new global back/forward step.
        assert!(is_view_navigation(&Msg::SelectDiff {
            repo_id: RepoId(1),
            target: DiffTarget::WorkingTree {
                path: std::path::PathBuf::from("a.txt"),
                area: DiffArea::Unstaged,
            },
        }));
        assert!(is_view_navigation(&Msg::SelectCommit {
            repo_id: RepoId(1),
            commit_id: CommitId("a".into()),
        }));
        // The file-content viewer's own back/forward does NOT land a global
        // step — it operates on a separate viewer-level stack so it does not
        // pollute the global back/forward history.
        assert!(!is_view_navigation(&Msg::ViewerNavBack {
            repo_id: RepoId(1)
        }));
        // Background / non-navigation messages do not push a step (they are
        // folded into the current entry in place, so they can't pollute history).
        assert!(!is_view_navigation(&Msg::DismissBannerError));
    }

    #[test]
    fn closure_and_replay_messages_are_not_view_navigations() {
        assert!(!is_view_navigation(&Msg::ClearDiffSelection {
            repo_id: RepoId(1),
        }));
        assert!(!is_view_navigation(&Msg::ClearCommitSelection {
            repo_id: RepoId(1),
        }));
        assert!(!is_view_navigation(&Msg::ViewerNavBack {
            repo_id: RepoId(1),
        }));
        assert!(!is_view_navigation(&Msg::ViewerNavForward {
            repo_id: RepoId(1),
        }));
        assert!(!is_view_navigation(&Msg::CloseInlineSubmoduleDiff {
            repo_id: RepoId(1),
        }));
        assert!(is_view_navigation(&Msg::OpenInlineSubmoduleDiff {
            origin: crate::model::ForeignDiffOrigin::Submodule,
            repo_id: RepoId(1),
            submodule_repo_path: std::path::PathBuf::from("/tmp/sub"),
            parent_submodule_path: std::path::PathBuf::from("sub"),
            entries: vec![],
            selected_ix: 0,
        }));
    }

    #[test]
    fn close_inline_submodule_diff_folds_in_place_and_does_not_bloat_nav_history() {
        // Closing a sub-view must fold in-place: if the snapshot after
        // closing matches a previous entry, it should collapse back to that
        // entry rather than pushing a duplicate.
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);

        // Seed: select a working tree diff (entries: [origin, diff], cursor=1).
        let target = DiffTarget::WorkingTree {
            path: std::path::PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        };
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: target.clone(),
            },
        );
        assert_eq!(repo(&state, repo_id).nav_history.entries.len(), 2);
        assert_eq!(repo(&state, repo_id).nav_history.cursor, 1);

        // Open inline submodule diff.
        dispatch(
            &mut state,
            Msg::OpenInlineSubmoduleDiff {
                origin: crate::model::ForeignDiffOrigin::Submodule,
                repo_id,
                submodule_repo_path: std::path::PathBuf::from("/tmp/repo/vendor/first"),
                parent_submodule_path: std::path::PathBuf::from("vendor/first"),
                entries: vec![],
                selected_ix: 0,
            },
        );

        // Close inline submodule diff — must fold, not push.
        dispatch(&mut state, Msg::CloseInlineSubmoduleDiff { repo_id });
        assert_eq!(
            repo(&state, repo_id).nav_history.entries.len(),
            2,
            "close must not add a new nav entry"
        );
        assert_eq!(
            repo(&state, repo_id).nav_history.cursor,
            1,
            "cursor must not advance past the parent diff"
        );
    }

    #[test]
    fn clearing_diff_folds_in_place_and_single_back_goes_to_commit_details() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit_a = CommitId("aaa".into());
        let file = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("file1.rs")),
        };

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit_a.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file.clone(),
            },
        );
        // User clicks the same committed file again, which dispatches
        // ClearDiffSelection to close the diff view.
        dispatch(&mut state, Msg::ClearDiffSelection { repo_id });

        let entries = &repo(&state, repo_id).nav_history.entries;
        // After folding in-place, no duplicate entry remains—the file
        // diff entry is collapsed back into the commit-details entry.
        assert_eq!(
            entries.len(),
            2,
            "fold-and-collapse must not create a new entry"
        );
        assert_eq!(
            repo(&state, repo_id).nav_history.cursor,
            1,
            "cursor should be back at the commit-details step"
        );

        // One GlobalNavBack from the commit-details view goes to the
        // history log (origin), confirming the stack did not bloat.
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(r.diff_state.diff_target, None);
        assert_eq!(r.history_state.selected_commit, None);
        assert!(!r.nav_history.can_back());
    }

    #[test]
    fn clearing_diff_without_folding_previous_allows_correct_back() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit_a = CommitId("aaa".into());
        let commit_b = CommitId("bbb".into());
        let file = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("file1.rs")),
        };

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit_a.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file.clone(),
            },
        );
        // Switch to a different commit (no fold-collapse because the
        // new state differs from the previous entry).
        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit_b.clone(),
            },
        );

        let r = repo(&state, repo_id);
        assert_eq!(
            r.nav_history.entries.len(),
            4,
            "select-commit pushes a new entry when the commit changes"
        );
        assert_eq!(r.nav_history.cursor, 3);
        assert_eq!(r.history_state.selected_commit.as_ref(), Some(&commit_b));

        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(
            r.diff_state.diff_target,
            Some(file),
            "back should reopen the file diff"
        );
        assert_eq!(r.history_state.selected_commit.as_ref(), Some(&commit_a));
    }

    #[test]
    fn browsing_committed_files_within_a_commit_keeps_commit_selected_on_back() {
        let repo_id = RepoId(1);
        let mut state = available_state_with_repo(repo_id);
        let commit_a = CommitId("aaa".into());
        let file_a = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("src/a.rs")),
        };
        let file_b = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("src/b.rs")),
        };
        let file_c = DiffTarget::Commit {
            commit_id: commit_a.clone(),
            path: Some(std::path::PathBuf::from("src/c.rs")),
        };

        dispatch(
            &mut state,
            Msg::SelectCommit {
                repo_id,
                commit_id: commit_a.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file_a.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file_b.clone(),
            },
        );
        dispatch(
            &mut state,
            Msg::SelectDiff {
                repo_id,
                target: file_c.clone(),
            },
        );

        let r = repo(&state, repo_id);
        // Origin + commit details + three file diffs = 5 entries.
        assert_eq!(
            r.nav_history.entries.len(),
            5,
            "each file selection must push a distinct history entry"
        );
        assert_eq!(r.nav_history.cursor, 4);
        assert_eq!(r.diff_state.diff_target, Some(file_c.clone()));

        // ── Back 1: file_c → file_b ──
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(
            r.diff_state.diff_target,
            Some(file_b.clone()),
            "first back must return to the previously viewed file (b)"
        );
        assert_eq!(
            r.history_state.selected_commit.as_ref(),
            Some(&commit_a),
            "commit must remain selected while browsing files"
        );
        assert_eq!(r.nav_history.cursor, 3);

        // ── Back 2: file_b → file_a ──
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(
            r.diff_state.diff_target,
            Some(file_a.clone()),
            "second back must return to the first opened file (a)"
        );
        assert_eq!(r.history_state.selected_commit.as_ref(), Some(&commit_a));
        assert_eq!(r.nav_history.cursor, 2);

        // ── Back 3: file_a → commit details (no diff, commit still selected) ──
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(
            r.diff_state.diff_target, None,
            "third back closes the last file diff and shows commit details"
        );
        assert_eq!(
            r.history_state.selected_commit.as_ref(),
            Some(&commit_a),
            "commit must still be selected — back must not deselect the commit"
        );
        assert_eq!(r.nav_history.cursor, 1);

        // ── Back 4: commit details → history log ──
        dispatch(&mut state, Msg::GlobalNavBack { repo_id });
        let r = repo(&state, repo_id);
        assert_eq!(r.diff_state.diff_target, None);
        assert_eq!(
            r.history_state.selected_commit, None,
            "only the fourth back returns to the history log"
        );
        assert_eq!(r.nav_history.cursor, 0);
        assert!(!r.nav_history.can_back());
    }
}

#[cfg(test)]
mod comparison_tests {
    use super::*;
    use crate::model::{AppState, Loadable, RepoState};
    use crate::msg::{CommitSelectMode, Effect};
    use std::sync::atomic::AtomicU64;
    use worktree_core::domain::{
        Commit, CommitFileChange, CommitId, FileStatusKind, LogPage, RepoSpec,
    };
    use worktree_core::process::{
        GitExecutableAvailability, GitExecutablePreference, GitRuntimeState,
    };

    fn commit(id: &str, parent: &str) -> Commit {
        Commit {
            signed: false,
            id: CommitId(id.into()),
            parent_ids: smallvec::smallvec![CommitId(parent.into())],
            summary: id.into(),
            author: "Tester".into(),
            time: std::time::SystemTime::UNIX_EPOCH,
        }
    }

    /// A repo whose loaded log is newest-first `c3, c2, c1` (so `c1` is oldest).
    /// Every commit has a parent — including the oldest, whose parent `c0` is
    /// simply older than the loaded page — so the merged-diff base is a real
    /// parent rather than the root-commit fallback.
    fn state_with_log(repo_id: RepoId) -> AppState {
        let mut state = AppState::default();
        state.git_runtime = GitRuntimeState {
            preference: GitExecutablePreference::SystemPath,
            availability: GitExecutableAvailability::Available {
                version_output: "git version 2.0.0".to_string(),
            },
        };
        let mut repo_state = RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: std::path::PathBuf::from("/tmp/repo"),
            },
        );
        repo_state.history_state.log = Loadable::Ready(Arc::new(LogPage {
            commits: vec![commit("c3", "c2"), commit("c2", "c1"), commit("c1", "c0")],
            next_cursor: None,
        }));
        state.repos.push(repo_state);
        state.active_repo = Some(repo_id);
        state
    }

    fn dispatch_effects(state: &mut AppState, msg: Msg) -> Vec<Effect> {
        let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
        let id_alloc = AtomicU64::new(99);
        reduce(&mut repos, &id_alloc, state, msg)
    }

    fn repo(state: &AppState, repo_id: RepoId) -> &RepoState {
        state.repos.iter().find(|r| r.id == repo_id).unwrap()
    }

    fn select(
        state: &mut AppState,
        repo_id: RepoId,
        id: &str,
        mode: CommitSelectMode,
    ) -> Vec<Effect> {
        dispatch_effects(
            state,
            Msg::SelectCommitMulti {
                repo_id,
                commit_id: CommitId(id.into()),
                mode,
                clicked_index: None,
                visible_order: None,
            },
        )
    }

    #[test]
    fn selecting_two_commits_enters_ordered_range_comparison() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        let c0 = CommitId("c0".into());
        let c3 = CommitId("c3".into());

        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        let effects = select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);

        let range = repo(&state, repo_id)
            .history_state
            .range_selection
            .clone()
            .expect("two selected commits should start a comparison");
        // The base is the *parent* of the oldest selected commit, regardless of
        // click order, so the merged diff includes that commit's own changes.
        assert_eq!(range.from, c0);
        assert_eq!(range.to, Some(c3.clone()));

        // The diff pane stays empty: the comparison presents the file
        // side-selection first, and the user opens a file to view its diff.
        assert_eq!(repo(&state, repo_id).diff_state.diff_target, None);
        assert!(matches!(
            repo(&state, repo_id).history_state.range_files,
            Loadable::Loading
        ));
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::LoadRangeFiles { from, to, .. } if *from == c0 && *to == Some(c3.clone())
            )),
            "a LoadRangeFiles effect for c0->c3 should be issued"
        );
    }

    #[test]
    fn range_files_loaded_populates_only_the_current_comparison() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        let effects = select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        let request = effects
            .iter()
            .find_map(|e| match e {
                Effect::LoadRangeFiles { request, .. } => Some(*request),
                _ => None,
            })
            .expect("a range-file load should be issued");

        let files = vec![CommitFileChange {
            path: std::path::PathBuf::from("a.rs"),
            kind: FileStatusKind::Modified,
            is_submodule: false,
            additions: Some(1),
            deletions: Some(0),
        }];

        // A stale result (wrong `from`) is dropped.
        dispatch_effects(
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
                repo_id,
                from: CommitId("c9".into()),
                to: Some(CommitId("c3".into())),
                request,
                result: Ok(files.clone()),
            }),
        );
        assert!(matches!(
            repo(&state, repo_id).history_state.range_files,
            Loadable::Loading
        ));

        // A reply from an *overtaken* load for the very same endpoints is
        // dropped too. This is the case `(from, to)` cannot catch: a
        // commit↔working-tree comparison keeps its pair across every refresh, so
        // only the request id distinguishes a current reply from a late one.
        dispatch_effects(
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
                repo_id,
                from: CommitId("c0".into()),
                to: Some(CommitId("c3".into())),
                request: request.wrapping_sub(1),
                result: Ok(files.clone()),
            }),
        );
        assert!(matches!(
            repo(&state, repo_id).history_state.range_files,
            Loadable::Loading
        ));

        // The matching result populates the list.
        dispatch_effects(
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
                repo_id,
                from: CommitId("c0".into()),
                to: Some(CommitId("c3".into())),
                request,
                result: Ok(files.clone()),
            }),
        );
        match &repo(&state, repo_id).history_state.range_files {
            Loadable::Ready(loaded) => assert_eq!(loaded.as_ref(), &files),
            other => panic!("expected loaded range files, got {other:?}"),
        }
    }

    #[test]
    fn single_selection_clears_an_active_comparison() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_some()
        );

        select(&mut state, repo_id, "c2", CommitSelectMode::Single);
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_none(),
            "collapsing to a single commit ends the comparison"
        );
    }

    fn loaded_details(id: &str) -> worktree_core::domain::CommitDetails {
        worktree_core::domain::CommitDetails {
            id: CommitId(id.into()),
            message: format!("{id} message"),
            author_name: "Tester".into(),
            author_email: "t@example.com".into(),
            authored_at_unix: 0,
            committed_at: String::new(),
            committed_at_unix: 0,
            parent_ids: Vec::new(),
            files: Vec::new(),
            signed: false,
        }
    }

    /// Entering a comparison moves `selected_commit` to the focused commit
    /// without loading its details — the comparison view owns the pane, so that
    /// load would be wasted. Leaving the comparison is therefore the moment the
    /// details pane has to be put back in sync, or it keeps rendering whichever
    /// commit's details were loaded last under a different commit's selection.
    #[test]
    fn closing_a_comparison_reloads_the_focused_commits_details() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);

        // c3 selected, its details loaded.
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        state.repos[0].history_state.commit_details =
            Loadable::Ready(Arc::new(loaded_details("c3")));

        // Ctrl-click c1: comparison mode, focus moves to c1, details stay c3's.
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_some()
        );
        assert_eq!(
            repo(&state, repo_id).history_state.selected_commit,
            Some(CommitId("c1".into()))
        );

        let effects = dispatch_effects(&mut state, Msg::ClearComparison { repo_id });

        let r = repo(&state, repo_id);
        assert!(
            !matches!(&r.history_state.commit_details, Loadable::Ready(d) if d.id == CommitId("c3".into())),
            "c3's details must not stay on screen under c1's selection"
        );
        assert!(
            effects.iter().any(|e| matches!(
                e,
                Effect::LoadCommitDetails { commit_id, .. } if *commit_id == CommitId("c1".into())
            )),
            "closing the comparison should load the still-selected commit's details"
        );
    }

    /// Every plain history click leaves a commit in `multi_selection`, so a
    /// comparison started from a context menu finds a stale one sitting there.
    /// It describes a different comparison, so it must not survive to name this
    /// one or supply its preview cards.
    #[test]
    fn an_explicit_comparison_drops_a_stale_multi_selection() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        assert!(
            repo(&state, repo_id)
                .history_state
                .multi_selection
                .is_multi(),
            "precondition: a multi-selection comparison is active"
        );

        dispatch_effects(
            &mut state,
            Msg::CompareWithWorkingTree {
                repo_id,
                from: CommitId("c2".into()),
                from_label: "main".into(),
            },
        );

        let r = repo(&state, repo_id);
        assert!(
            r.history_state.multi_selection.commits.is_empty(),
            "the previous selection is not part of this comparison"
        );
        let range = r
            .history_state
            .range_selection
            .clone()
            .expect("the explicit comparison replaces the previous one");
        assert_eq!(range.from, CommitId("c2".into()));
        assert_eq!(range.to, None);
    }

    /// A multi-selection comparison keeps its selection: there, the selection
    /// *is* what is being compared, and the UI names the comparison after it.
    #[test]
    fn a_multi_selection_comparison_keeps_its_selection() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);

        let r = repo(&state, repo_id);
        assert!(r.history_state.range_selection.is_some());
        assert!(r.history_state.multi_selection.is_multi());
    }

    #[test]
    fn clear_comparison_dismisses_selection_and_diff() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_some()
        );

        dispatch_effects(&mut state, Msg::ClearComparison { repo_id });
        let r = repo(&state, repo_id);
        assert!(r.history_state.range_selection.is_none());
        assert!(!r.history_state.multi_selection.is_multi());
        assert_eq!(r.diff_state.diff_target, None);
    }

    #[test]
    fn mark_then_compare_with_marked_builds_the_range() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        let c1 = CommitId("c1".into());
        let c3 = CommitId("c3".into());

        // Nothing marked yet: comparing is a no-op.
        let effects = dispatch_effects(
            &mut state,
            Msg::CompareWithMarked {
                repo_id,
                commit_id: c3.clone(),
                label: "c3".into(),
            },
        );
        assert!(effects.is_empty());
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_none()
        );

        // Mark c1 (base), then compare c3 against it.
        dispatch_effects(
            &mut state,
            Msg::MarkForComparison {
                repo_id,
                commit_id: c1.clone(),
                label: "main".into(),
            },
        );
        dispatch_effects(
            &mut state,
            Msg::CompareWithMarked {
                repo_id,
                commit_id: c3.clone(),
                label: "feature".into(),
            },
        );
        let range = repo(&state, repo_id)
            .history_state
            .range_selection
            .clone()
            .expect("compare with marked should start a comparison");
        assert_eq!(range.from, c1);
        assert_eq!(range.to, Some(c3.clone()));
        assert_eq!(range.from_label, "main");
        assert_eq!(range.to_label, "feature");
    }

    #[test]
    fn compare_commit_range_message_orders_via_labels() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        let effects = dispatch_effects(
            &mut state,
            Msg::CompareCommitRange {
                repo_id,
                from: CommitId("c1".into()),
                to: CommitId("c3".into()),
                from_label: "main".into(),
                to_label: "feature".into(),
            },
        );
        let range = repo(&state, repo_id)
            .history_state
            .range_selection
            .clone()
            .expect("explicit compare should set a comparison");
        assert_eq!(range.from_label, "main");
        assert_eq!(range.to_label, "feature");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadRangeFiles { .. }))
        );
    }

    #[test]
    fn compare_with_working_tree_starts_a_worktree_comparison() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        let from = CommitId("c2".into());

        let effects = dispatch_effects(
            &mut state,
            Msg::CompareWithWorkingTree {
                repo_id,
                from: from.clone(),
                from_label: "main".into(),
            },
        );

        let range = repo(&state, repo_id)
            .history_state
            .range_selection
            .clone()
            .expect("compare with working tree should start a comparison");
        assert_eq!(range.from, from);
        // The tip is the working tree, not a commit.
        assert_eq!(range.to, None);
        assert_eq!(range.to_label, "Working tree");
        // A worktree-tip file list load is issued, and the diff pane is cleared.
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::LoadRangeFiles { from: f, to: None, .. } if *f == from
        )));
        assert_eq!(repo(&state, repo_id).diff_state.diff_target, None);
    }

    /// A refresh means two full-tree `git diff` calls, so changes arriving while
    /// one is running must fold into it rather than each starting their own —
    /// and the fold must still end with a run that sees the final state.
    #[test]
    fn external_worktree_changes_refresh_a_worktree_comparison_one_at_a_time() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        let from = CommitId("c2".into());
        let effects = dispatch_effects(
            &mut state,
            Msg::CompareWithWorkingTree {
                repo_id,
                from: from.clone(),
                from_label: "main".into(),
            },
        );
        let load_request = |effects: &[Effect]| {
            effects.iter().find_map(|e| match e {
                Effect::LoadRangeFiles {
                    from: f,
                    to: None,
                    request,
                    ..
                } if *f == from => Some(*request),
                _ => None,
            })
        };
        let first = load_request(&effects).expect("the comparison issues a file-list load");

        // Two changes land while that load is still running: neither starts its
        // own, they collapse into the one already in flight.
        for _ in 0..2 {
            let effects = dispatch_effects(
                &mut state,
                Msg::RepoExternallyChanged {
                    repo_id,
                    change: crate::msg::RepoExternalChange::Worktree,
                    worktree_paths: None,
                },
            );
            assert!(
                load_request(&effects).is_none(),
                "a refresh must not stack on top of one already in flight"
            );
        }

        // When it lands, the folded changes are honoured by exactly one re-run,
        // so the list ends up describing the worktree as it is now.
        let effects = dispatch_effects(
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
                repo_id,
                from: from.clone(),
                to: None,
                request: first,
                result: Ok(Vec::new()),
            }),
        );
        let second = load_request(&effects).expect("the folded refresh runs once the load lands");
        assert_ne!(first, second, "the re-run is a new request, not a replay");

        // Nothing further is queued, so a quiet worktree stops the chain.
        let effects = dispatch_effects(
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
                repo_id,
                from: from.clone(),
                to: None,
                request: second,
                result: Ok(Vec::new()),
            }),
        );
        assert!(load_request(&effects).is_none());

        // And with nothing in flight, the next change refreshes immediately.
        let effects = dispatch_effects(
            &mut state,
            Msg::RepoExternallyChanged {
                repo_id,
                change: crate::msg::RepoExternalChange::Worktree,
                worktree_paths: None,
            },
        );
        assert!(
            load_request(&effects).is_some(),
            "expected the worktree comparison file list to refresh"
        );
    }

    #[test]
    fn external_change_does_not_refresh_a_commit_comparison() {
        let repo_id = RepoId(1);
        let mut state = state_with_log(repo_id);
        // Two-commit (immutable) comparison.
        select(&mut state, repo_id, "c3", CommitSelectMode::Single);
        select(&mut state, repo_id, "c1", CommitSelectMode::Toggle);
        assert!(
            repo(&state, repo_id)
                .history_state
                .range_selection
                .is_some()
        );

        let effects = dispatch_effects(
            &mut state,
            Msg::RepoExternallyChanged {
                repo_id,
                change: crate::msg::RepoExternalChange::Worktree,
                worktree_paths: None,
            },
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::LoadRangeFiles { .. })),
            "a commit↔commit comparison is immutable and must not refresh"
        );
    }
}
