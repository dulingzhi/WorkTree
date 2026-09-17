use super::repo_management::{self, append_cancel_repo_loads_effect_for_repo};
use super::util::{
    self, DiffReloadMode, SelectedConflictTarget, append_diff_reload_effects,
    append_refresh_full_effects, append_refresh_primary_effects,
    append_start_conflict_target_reload, append_start_current_conflict_target_reload,
    append_targeted_status_refresh, apply_selected_diff_load_plan_state,
    apply_selected_diff_load_plan_state_with_reload_mode, clear_banner_error_for_repo,
    format_failure_summary, push_action_log, push_command_log, push_failure_needs_pull_retry,
    refresh_full_effect_capacity, refresh_primary_effect_capacity, selected_conflict_target,
    selected_diff_load_plan,
};
use super::{
    ReduceOutcome, actions_emit_effects, auth_prompt_for_commit, auth_prompt_for_repo_command,
    auth_prompt_for_safe_push_after_commit, begin_commit_action, begin_head_changing_local_action,
    begin_local_action, normalize_repo_relative_path, start_submodule_add_progress,
};
use crate::model::{
    AppState, BannerErrorState, InteractiveCherryPickSetup, InteractiveRebaseSetup, Loadable,
    PendingCommitRetry, RepoId, RepoLoadsInFlight, RepoState, SubmoduleTrustCheckOperation,
    SubmoduleTrustCheckState, SubmoduleTrustPromptOperation, SubmoduleTrustPromptState,
};
use crate::msg::{Effect, Msg, RepoChange, RepoCommandKind, RepoPathList};
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;
use worktree_core::auth::StagedGitAuth;
use worktree_core::conflict_session::{ConflictRegionResolution, ConflictResolverStrategy};
use worktree_core::domain::{CommitId, DiffTarget, FileConflictKind};
use worktree_core::error::Error;
use worktree_core::external_merge_tool::ExternalMergeToolSelection;
use worktree_core::services::{
    CommandOutput, GitRepository, InteractiveRebaseEntry, PullMode, RemoteUrlKind, ResetMode,
    SafePushAfterCommitContext, SafePushAfterCommitTarget,
};

pub(super) fn checkout_branch(repo_id: RepoId, name: String) -> Vec<Effect> {
    vec![Effect::CheckoutBranch { repo_id, name }]
}

pub(super) fn checkout_remote_branch(
    repo_id: RepoId,
    remote: String,
    branch: String,
    local_branch: String,
) -> Vec<Effect> {
    vec![Effect::CheckoutRemoteBranch {
        repo_id,
        remote,
        branch,
        local_branch,
    }]
}

pub(super) fn checkout_pull_request(repo_id: RepoId, remote: String, number: u64) -> Vec<Effect> {
    vec![Effect::CheckoutPullRequest {
        repo_id,
        remote,
        number,
    }]
}

pub(super) fn checkout_commit(
    repo_id: RepoId,
    commit_id: worktree_core::domain::CommitId,
) -> Vec<Effect> {
    vec![Effect::CheckoutCommit { repo_id, commit_id }]
}

pub(super) fn cherry_pick_commit(
    repo_id: RepoId,
    commit_id: worktree_core::domain::CommitId,
    commit: bool,
    mainline: Option<usize>,
    summary: String,
) -> Vec<Effect> {
    vec![Effect::CherryPickCommit {
        repo_id,
        commit_id,
        commit,
        mainline,
        summary,
    }]
}

pub(super) fn revert_commit(
    repo_id: RepoId,
    commit_id: worktree_core::domain::CommitId,
) -> Vec<Effect> {
    vec![Effect::RevertCommit { repo_id, commit_id }]
}

pub(super) fn create_branch(repo_id: RepoId, name: String, target: String) -> Vec<Effect> {
    vec![Effect::CreateBranch {
        repo_id,
        name,
        target,
    }]
}

pub(super) fn create_branch_and_checkout(
    repo_id: RepoId,
    name: String,
    target: String,
) -> Vec<Effect> {
    vec![Effect::CreateBranchAndCheckout {
        repo_id,
        name,
        target,
    }]
}

pub(super) fn rename_branch(repo_id: RepoId, old_name: String, new_name: String) -> Vec<Effect> {
    vec![Effect::RenameBranch {
        repo_id,
        old_name,
        new_name,
    }]
}

pub(super) fn delete_branch(repo_id: RepoId, name: String) -> Vec<Effect> {
    vec![Effect::DeleteBranch { repo_id, name }]
}

pub(super) fn force_delete_branch(repo_id: RepoId, name: String) -> Vec<Effect> {
    vec![Effect::ForceDeleteBranch { repo_id, name }]
}

pub(super) fn delete_branches(repo_id: RepoId, names: Vec<String>, force: bool) -> Vec<Effect> {
    vec![Effect::DeleteBranches {
        repo_id,
        names,
        force,
    }]
}

pub(super) fn export_patch(
    repo_id: RepoId,
    commit_id: worktree_core::domain::CommitId,
    dest: PathBuf,
) -> Vec<Effect> {
    vec![Effect::ExportPatch {
        repo_id,
        commit_id,
        dest,
    }]
}

pub(super) fn apply_patch(repo_id: RepoId, patch: PathBuf) -> Vec<Effect> {
    vec![Effect::ApplyPatch { repo_id, patch }]
}

pub(super) fn archive_zip(repo_id: RepoId, revision: String, dest: PathBuf) -> Vec<Effect> {
    vec![Effect::ArchiveZip {
        repo_id,
        revision,
        dest,
    }]
}

pub(super) fn cleanup_repo(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::CleanupRepo { repo_id }]
}

pub(super) fn add_worktree(
    repo_id: RepoId,
    path: PathBuf,
    reference: Option<String>,
) -> Vec<Effect> {
    vec![Effect::AddWorktree {
        repo_id,
        path,
        reference,
    }]
}

pub(super) fn remove_worktree(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::RemoveWorktree { repo_id, path }]
}

pub(super) fn force_remove_worktree(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::ForceRemoveWorktree { repo_id, path }]
}

pub(super) fn add_submodule(
    repo_id: RepoId,
    url: String,
    path: PathBuf,
    branch: Option<String>,
    name: Option<String>,
    force: bool,
    approved_sources: Vec<worktree_core::services::SubmoduleTrustTarget>,
) -> Vec<Effect> {
    vec![Effect::AddSubmodule {
        repo_id,
        url,
        path,
        branch,
        name,
        force,
        approved_sources,
        auth: None,
    }]
}

pub(super) fn update_submodules(
    repo_id: RepoId,
    approved_sources: Vec<worktree_core::services::SubmoduleTrustTarget>,
) -> Vec<Effect> {
    vec![Effect::UpdateSubmodules {
        repo_id,
        approved_sources,
        auth: None,
    }]
}

pub(super) fn load_submodule(
    repo_id: RepoId,
    path: PathBuf,
    approved_sources: Vec<worktree_core::services::SubmoduleTrustTarget>,
) -> Vec<Effect> {
    vec![Effect::LoadSubmodule {
        repo_id,
        path,
        approved_sources,
        auth: None,
    }]
}

pub(super) fn change_submodule_pointer(
    repo_id: RepoId,
    path: PathBuf,
    reference: String,
) -> Vec<Effect> {
    vec![Effect::ChangeSubmodulePointer {
        repo_id,
        path,
        reference,
    }]
}

pub(super) fn remove_submodule(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::RemoveSubmodule { repo_id, path }]
}

pub(super) fn stage_path(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::StagePath { repo_id, path }]
}

pub(super) fn stage_paths(repo_id: RepoId, paths: RepoPathList) -> Vec<Effect> {
    vec![Effect::StagePaths { repo_id, paths }]
}

pub(super) fn unstage_path(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::UnstagePath { repo_id, path }]
}

pub(super) fn unstage_paths(repo_id: RepoId, paths: RepoPathList) -> Vec<Effect> {
    vec![Effect::UnstagePaths { repo_id, paths }]
}

pub(super) fn discard_worktree_changes_path(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::DiscardWorktreeChangesPath { repo_id, path }]
}

pub(super) fn discard_worktree_changes_paths(repo_id: RepoId, paths: Vec<PathBuf>) -> Vec<Effect> {
    vec![Effect::DiscardWorktreeChangesPaths { repo_id, paths }]
}

pub(super) fn save_worktree_file(
    repo_id: RepoId,
    path: PathBuf,
    contents: String,
    stage: bool,
) -> Vec<Effect> {
    vec![Effect::SaveWorktreeFile {
        repo_id,
        path,
        contents,
        stage,
    }]
}

pub(super) fn append_gitignore_patterns(repo_id: RepoId, patterns: Vec<String>) -> Vec<Effect> {
    vec![Effect::AppendGitignorePatterns { repo_id, patterns }]
}

pub(super) fn commit(repo_id: RepoId, message: String) -> Vec<Effect> {
    vec![Effect::Commit {
        repo_id,
        message,
        auth: None,
    }]
}

pub(super) fn commit_amend(repo_id: RepoId, message: String) -> Vec<Effect> {
    vec![Effect::CommitAmend {
        repo_id,
        message,
        auth: None,
    }]
}

/// The subject of `target` from the loaded log page, or `None` when the commit
/// is not in the page. The only caller is reached from a history row, so the
/// target is normally present; a miss means the page moved under the click.
fn commit_subject(state: &AppState, repo_id: RepoId, target: &CommitId) -> Option<String> {
    let repo_state = state.repos.iter().find(|r| r.id == repo_id)?;
    let Loadable::Ready(page) = &repo_state.log else {
        return None;
    };
    page.commits
        .iter()
        .find(|commit| &commit.id == target)
        .map(|commit| commit.summary.to_string())
}

/// Commit the staged changes as a `fixup!` commit for `target`, so the next
/// autosquash folds it in.
///
/// The message comes from core's [`worktree_core::squash::fixup_message`],
/// which reproduces `git commit --fixup=<target>` exactly — so this rides the
/// ordinary commit path rather than adding a backend operation. Everything
/// downstream (the in-flight bookkeeping, the auth-retry replay, the
/// post-commit push) is the same as a plain commit.
pub(super) fn commit_fixup(
    state: &mut AppState,
    repo_id: RepoId,
    target: CommitId,
    push_after_commit: bool,
) -> Vec<Effect> {
    let Some(subject) = commit_subject(state, repo_id, &target) else {
        super::util::push_notification(
            state,
            crate::model::AppNotificationKind::Warning,
            rust_i18n::t!("store.reducer.fixup_target_not_loaded").to_string(),
        );
        return Vec::new();
    };

    let message = worktree_core::squash::fixup_message(&subject);
    begin_commit_action(state, repo_id);
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.pending_commit_retry = Some(PendingCommitRetry {
            message: message.clone(),
            amend: false,
            push_after_commit,
        });
    }
    commit(repo_id, message)
}

pub(super) fn safe_push_after_commit(
    repo_id: RepoId,
    context: worktree_core::services::SafePushAfterCommitContext,
) -> Vec<Effect> {
    vec![Effect::SafePushAfterCommit {
        repo_id,
        context,
        auth: None,
    }]
}

enum InFlightKind {
    Pull,
    Push,
}

fn bump_in_flight(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    kind: InFlightKind,
) {
    if !repos.contains_key(&repo_id) {
        return;
    }
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        match kind {
            InFlightKind::Pull => {
                repo_state.pull_in_flight = repo_state.pull_in_flight.saturating_add(1);
            }
            InFlightKind::Push => {
                repo_state.push_in_flight = repo_state.push_in_flight.saturating_add(1);
            }
        }
        repo_state.bump_ops_rev();
    }
}

pub(super) fn fetch_all(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let prune = fetch_prune_setting(state, repo_id);
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::FetchAll {
        repo_id,
        prune,
        auth: None,
    }]
}

/// Same command as [`fetch_all`], but reported as the quiet `AutoFetchAll`
/// kind: the store's activation handler dispatches this after a tab's local
/// refresh, so remote changes arrive without toasts or failure banners.
pub(super) fn auto_fetch_all(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    let prune = fetch_prune_setting(state, repo_id);
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::AutoFetchAll {
        repo_id,
        prune,
        auth: None,
    }]
}

fn fetch_prune_setting(state: &AppState, repo_id: RepoId) -> bool {
    state
        .repos
        .iter()
        .find(|r| r.id == repo_id)
        .is_some_and(|repo_state| repo_state.fetch_prune_deleted_remote_tracking_branches)
}

pub(super) fn prune_merged_branches(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::PruneMergedBranches { repo_id }]
}

pub(super) fn prune_local_tags(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::PruneLocalTags { repo_id }]
}

pub(super) fn pull(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    mode: PullMode,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::Pull {
        repo_id,
        mode,
        auth: None,
    }]
}

pub(super) fn pull_branch(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    branch: String,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Pull);
    vec![Effect::PullBranch {
        repo_id,
        remote,
        branch,
        auth: None,
    }]
}

pub(super) fn merge_ref(repo_id: RepoId, reference: String) -> Vec<Effect> {
    vec![Effect::MergeRef { repo_id, reference }]
}

pub(super) fn squash_ref(repo_id: RepoId, reference: String) -> Vec<Effect> {
    vec![Effect::SquashRef { repo_id, reference }]
}

pub(super) fn push(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    pull_retry: bool,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    if let Some(repo_state) = state.repos.iter_mut().find(|repo| repo.id == repo_id) {
        repo_state.push_pull_retry_armed = pull_retry;
    }
    vec![Effect::Push {
        repo_id,
        auth: None,
    }]
}

/// What the push pull-retry state machine wants to do once a repo command
/// finishes. Planned before `repo_command_finished` consumes the result
/// (`Error` is not cloneable) and applied after it, so a rejected push's
/// command-log entry can be rewritten as a retry-in-progress instead of an
/// announced failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PushPullRetryPlan {
    /// The push was rejected because the remote is ahead and the retry option
    /// was on: convert the failure into `git pull --rebase` and re-push once
    /// the pull succeeds.
    PullAndRetry,
    /// The retry pull succeeded: push again, unarmed.
    RetryPush,
    Nothing,
}

pub(super) fn push_pull_retry_plan(
    state: &AppState,
    repo_id: RepoId,
    command: &RepoCommandKind,
    result: &std::result::Result<CommandOutput, Error>,
) -> PushPullRetryPlan {
    let Some(repo_state) = state.repos.iter().find(|r| r.id == repo_id) else {
        return PushPullRetryPlan::Nothing;
    };
    match (command, result) {
        (RepoCommandKind::Push, Err(error)) => {
            if repo_state.push_pull_retry_armed && push_failure_needs_pull_retry(error) {
                PushPullRetryPlan::PullAndRetry
            } else {
                PushPullRetryPlan::Nothing
            }
        }
        (RepoCommandKind::Pull { .. }, Ok(_)) if repo_state.push_pull_retry_pending => {
            PushPullRetryPlan::RetryPush
        }
        _ => PushPullRetryPlan::Nothing,
    }
}

pub(super) fn apply_push_pull_retry(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    command: &RepoCommandKind,
    plan: PushPullRetryPlan,
) -> Vec<Effect> {
    {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        match command {
            RepoCommandKind::Push => {
                // One-shot: consumed on every push finish, retry or not.
                repo_state.push_pull_retry_armed = false;
                if plan == PushPullRetryPlan::PullAndRetry {
                    repo_state.push_pull_retry_pending = true;
                    if let Some(entry) = repo_state.command_log.last_mut() {
                        entry.announce_failure = false;
                        entry.summary =
                            rust_i18n::t!("store.reducer.push_pull_retry_started").to_string();
                    }
                    // Keep the suppressed failure from surfacing elsewhere.
                    repo_state.last_error = None;
                }
            }
            RepoCommandKind::Pull { .. } => {
                // Cleared on any pull finish so a failed rebase (conflicts)
                // never leaves a stale trigger behind.
                repo_state.push_pull_retry_pending = false;
            }
            _ => {}
        }
    }
    match plan {
        PushPullRetryPlan::PullAndRetry => pull(repos, state, repo_id, PullMode::Rebase),
        PushPullRetryPlan::RetryPush => push(repos, state, repo_id, false),
        PushPullRetryPlan::Nothing => Vec::new(),
    }
}

pub(super) fn push_after_commit(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    target: SafePushAfterCommitTarget,
    set_upstream: bool,
) -> Vec<Effect> {
    push_after_commit_with_auth(repos, state, repo_id, target, set_upstream, None)
}

fn push_after_commit_with_auth(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    target: SafePushAfterCommitTarget,
    set_upstream: bool,
    auth: Option<StagedGitAuth>,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::PushAfterCommit {
        repo_id,
        target,
        set_upstream,
        auth,
    }]
}

pub(super) fn force_push(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::ForcePush {
        repo_id,
        auth: None,
    }]
}

pub(super) fn force_push_with_lease(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    lease: worktree_core::services::ForcePushLease,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::ForcePushWithLease {
        repo_id,
        lease,
        auth: None,
    }]
}

/// Deliberately outside the push pull-retry state machine: a retry would
/// re-dispatch a plain `Msg::Push` and lose the merge-request options. A
/// rejected merge-request push just reports; the user pulls, then pushes the
/// merge request again.
pub(super) fn push_merge_request(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    options: worktree_core::services::MergeRequestPushOptions,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::PushMergeRequest {
        repo_id,
        options,
        auth: None,
    }]
}

pub(super) fn push_set_upstream(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    branch: String,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::PushSetUpstream {
        repo_id,
        remote,
        branch,
        auth: None,
    }]
}

pub(super) fn set_upstream_branch(
    repo_id: RepoId,
    branch: String,
    upstream: String,
) -> Vec<Effect> {
    vec![Effect::SetUpstreamBranch {
        repo_id,
        branch,
        upstream,
    }]
}

pub(super) fn unset_upstream_branch(repo_id: RepoId, branch: String) -> Vec<Effect> {
    vec![Effect::UnsetUpstreamBranch { repo_id, branch }]
}

pub(super) fn fast_forward_branch(repo_id: RepoId, branch: String) -> Vec<Effect> {
    vec![Effect::FastForwardBranch { repo_id, branch }]
}

pub(super) fn delete_remote_branch(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    branch: String,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::DeleteRemoteBranch {
        repo_id,
        remote,
        branch,
        auth: None,
    }]
}

pub(super) fn delete_remote_branches(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    branches: Vec<String>,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::DeleteRemoteBranches {
        repo_id,
        remote,
        branches,
        auth: None,
    }]
}

pub(super) fn reset(repo_id: RepoId, target: String, mode: ResetMode) -> Vec<Effect> {
    vec![Effect::Reset {
        repo_id,
        target,
        mode,
    }]
}

pub(super) fn squash_commits(
    state: &mut AppState,
    repo_id: RepoId,
    oldest: worktree_core::domain::CommitId,
    expected_head: worktree_core::domain::CommitId,
    message: String,
    count: usize,
) -> Vec<Effect> {
    // Re-validate against the current selection and log: both may have
    // changed between opening the prompt and confirming.
    let plan = state
        .repos
        .iter()
        .find(|r| r.id == repo_id)
        .and_then(super::loaded_results::squash_plan_for_repo);
    let still_valid = plan
        .as_ref()
        .is_some_and(|p| p.oldest == oldest && p.head == expected_head);
    if !still_valid || message.trim().is_empty() {
        super::util::push_notification(
            state,
            crate::model::AppNotificationKind::Warning,
            rust_i18n::t!("store.reducer.squash_cancelled").to_string(),
        );
        return Vec::new();
    }
    let plan = plan.unwrap();

    // Range ends at HEAD: use the fast commit-tree + update-ref path that
    // does not touch the worktree or index.
    if plan.head == plan.actual_head {
        super::begin_local_action(state, repo_id);
        return vec![Effect::SquashCommits {
            repo_id,
            oldest,
            expected_head,
            message,
            count,
        }];
    }

    // Intermediate range: load the full commit list from base..HEAD so we
    // can build a rebase todo that squashes only the selected commits.
    vec![Effect::LoadSquashRebaseSetup {
        repo_id,
        base: plan.oldest_parent,
        actual_head: plan.actual_head,
        selected_ids: plan.ordered_ids,
        reword_id: oldest,
        message,
        count,
    }]
}

pub(super) fn rebase(repo_id: RepoId, onto: String) -> Vec<Effect> {
    vec![Effect::Rebase { repo_id, onto }]
}

pub(super) fn rebase_continue(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::RebaseContinue {
        repo_id,
        auth: None,
    }]
}

pub(super) fn rebase_abort(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::RebaseAbort { repo_id }]
}

pub(super) fn bisect_start(
    repo_id: RepoId,
    bad: Option<String>,
    goods: Vec<String>,
) -> Vec<Effect> {
    vec![Effect::BisectStart {
        repo_id,
        bad,
        goods,
    }]
}

pub(super) fn bisect_mark(
    repo_id: RepoId,
    verdict: worktree_core::services::BisectVerdict,
    commit: Option<String>,
) -> Vec<Effect> {
    vec![Effect::BisectMark {
        repo_id,
        verdict,
        commit,
    }]
}

pub(super) fn bisect_reset(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::BisectReset { repo_id }]
}

pub(super) fn merge_abort(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::MergeAbort { repo_id }]
}

pub(super) fn load_interactive_rebase_setup(
    state: &mut AppState,
    repo_id: RepoId,
    base: String,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.interactive_rebase_setup = Some(InteractiveRebaseSetup {
            base: base.clone(),
            entries: Loadable::Loading,
        });
    }
    vec![Effect::LoadInteractiveRebaseSetup { repo_id, base }]
}

pub(super) fn interactive_rebase(
    repo_id: RepoId,
    base: String,
    entries: Vec<InteractiveRebaseEntry>,
) -> Vec<Effect> {
    vec![Effect::InteractiveRebase {
        repo_id,
        base,
        entries,
        interactive: true,
    }]
}

pub(super) fn open_interactive_cherry_pick_setup(
    state: &mut AppState,
    repo_id: RepoId,
    entries: Vec<InteractiveRebaseEntry>,
    source_colors: Vec<(String, u8)>,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.interactive_rebase_setup = None;
        // The entries arrive seeded with subjects only (the log page carries
        // no bodies); load the full messages so a reword edit doesn't start
        // from — and then silently commit — a body-less seed.
        let ids = entries
            .iter()
            .map(|entry| entry.commit_id.clone())
            .collect();
        repo_state.interactive_cherry_pick_setup = Some(InteractiveCherryPickSetup {
            entries,
            source_colors,
            full_messages: Loadable::Loading,
        });
        return vec![Effect::LoadInteractiveCherryPickMessages { repo_id, ids }];
    }
    vec![]
}

pub(super) fn interactive_cherry_pick(
    repo_id: RepoId,
    entries: Vec<InteractiveRebaseEntry>,
) -> Vec<Effect> {
    vec![Effect::InteractiveCherryPick { repo_id, entries }]
}

pub(super) fn cancel_interactive_rebase_setup(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.interactive_rebase_setup = None;
    }
    vec![]
}

pub(super) fn cancel_interactive_cherry_pick_setup(
    state: &mut AppState,
    repo_id: RepoId,
) -> Vec<Effect> {
    if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
        repo_state.interactive_cherry_pick_setup = None;
    }
    vec![]
}

pub(super) fn create_tag(
    repo_id: RepoId,
    name: String,
    target: String,
    message: Option<String>,
    annotated: bool,
) -> Vec<Effect> {
    vec![Effect::CreateTag {
        repo_id,
        name,
        target,
        message,
        annotated,
    }]
}

pub(super) fn delete_tag(repo_id: RepoId, name: String) -> Vec<Effect> {
    vec![Effect::DeleteTag { repo_id, name }]
}

pub(super) fn push_tag(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    name: String,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::PushTag {
        repo_id,
        remote,
        name,
        auth: None,
    }]
}

pub(super) fn delete_remote_tag(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    remote: String,
    name: String,
) -> Vec<Effect> {
    bump_in_flight(repos, state, repo_id, InFlightKind::Push);
    vec![Effect::DeleteRemoteTag {
        repo_id,
        remote,
        name,
        auth: None,
    }]
}

pub(super) fn add_remote(repo_id: RepoId, name: String, url: String) -> Vec<Effect> {
    vec![Effect::AddRemote { repo_id, name, url }]
}

pub(super) fn remove_remote(repo_id: RepoId, name: String) -> Vec<Effect> {
    vec![Effect::RemoveRemote { repo_id, name }]
}

pub(super) fn set_remote_url(
    repo_id: RepoId,
    name: String,
    url: String,
    kind: RemoteUrlKind,
) -> Vec<Effect> {
    vec![Effect::SetRemoteUrl {
        repo_id,
        name,
        url,
        kind,
    }]
}

pub(super) fn set_remote_ssh_key(
    repo_id: RepoId,
    remote: String,
    key: Option<String>,
) -> Vec<Effect> {
    vec![Effect::SetRemoteSshKey {
        repo_id,
        remote,
        key,
    }]
}

pub(super) fn checkout_conflict_side(
    repo_id: RepoId,
    path: PathBuf,
    side: worktree_core::services::ConflictSide,
) -> Vec<Effect> {
    vec![Effect::CheckoutConflictSide {
        repo_id,
        path,
        side,
    }]
}

pub(super) fn accept_conflict_deletion(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::AcceptConflictDeletion { repo_id, path }]
}

pub(super) fn checkout_conflict_base(repo_id: RepoId, path: PathBuf) -> Vec<Effect> {
    vec![Effect::CheckoutConflictBase { repo_id, path }]
}

pub(super) fn launch_mergetool(
    repo_id: RepoId,
    path: PathBuf,
    preference: ExternalMergeToolSelection,
) -> Vec<Effect> {
    vec![Effect::LaunchMergetool {
        repo_id,
        path,
        preference,
    }]
}

pub(super) fn stash(
    repo_id: RepoId,
    message: String,
    include_untracked: bool,
    keep_index: bool,
    paths: RepoPathList,
) -> Vec<Effect> {
    vec![Effect::Stash {
        repo_id,
        message,
        include_untracked,
        keep_index,
        paths,
    }]
}

pub(super) fn apply_stash(repo_id: RepoId, index: usize) -> Vec<Effect> {
    vec![Effect::ApplyStash { repo_id, index }]
}

pub(super) fn pop_stash(repo_id: RepoId, index: usize) -> Vec<Effect> {
    vec![Effect::PopStash { repo_id, index }]
}

pub(super) fn drop_stash(repo_id: RepoId, index: usize) -> Vec<Effect> {
    vec![Effect::DropStash { repo_id, index }]
}

pub(super) fn stash_branch(repo_id: RepoId, index: usize, branch: String) -> Vec<Effect> {
    vec![Effect::StashBranch {
        repo_id,
        index,
        branch,
    }]
}

pub(super) fn set_assume_unchanged(repo_id: RepoId, path: PathBuf, enable: bool) -> Vec<Effect> {
    vec![Effect::SetAssumeUnchanged {
        repo_id,
        path,
        enable,
    }]
}

pub(super) fn load_assume_unchanged(repo_id: RepoId) -> Vec<Effect> {
    vec![Effect::LoadAssumeUnchanged { repo_id }]
}

/// Drop any loaded blame once the content it describes is known to be stale —
/// after a working-tree mutation (stage/unstage/apply patch/commit), after a
/// reload whose result actually differed (`diff_loaded`/`diff_file_loaded`), or
/// after a git-state event that may have moved HEAD. The blame annotation column
/// is derived from the same content the diff shows; leaving blame `Ready` would
/// make `request_blame_for_current_target` treat the target as already attempted
/// and keep painting stale attribution and staged/unstaged labels. `blame_path`
/// and `blame_source` are intentionally preserved so the view reloads the same
/// target's blame against the new content.
pub(super) fn invalidate_loaded_blame(repo_state: &mut RepoState) {
    if !matches!(repo_state.history_state.blame, Loadable::NotLoaded) {
        // Keep the outgoing annotations available to the view so the column
        // stays painted across the reload; the target is unchanged, so they
        // still describe the right file.
        repo_state.retain_blame_while_loading();
        repo_state.history_state.blame = Loadable::NotLoaded;
    }
}

pub(super) fn commit_finished(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<(), Error>,
) -> Vec<Effect> {
    let mut clear_banner = false;
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // Coarse "repo changed" ping so UI can uniformly sense commits.
    repo_state.bump_content_rev();
    // Collected before any state below changes: the (pre-commit) staged
    // entries are exactly the paths the commit clears from the staged lane.
    let committed_paths: Vec<PathBuf> = repo_state
        .staged_status_entries()
        .map(|entries| entries.iter().map(|entry| entry.path.clone()).collect())
        .unwrap_or_default();
    repo_state.local_actions_in_flight = repo_state.local_actions_in_flight.saturating_sub(1);
    repo_state.commit_in_flight = repo_state.commit_in_flight.saturating_sub(1);
    repo_state.bump_ops_rev();
    match result {
        Ok(()) => {
            repo_state.last_error = None;
            clear_banner = true;
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
            repo_state.set_diff_target(None);
            repo_state.diff_state.diff = Loadable::NotLoaded;
            repo_state.diff_state.diff_file = Loadable::NotLoaded;
            repo_state.diff_state.diff_file_lfs = Loadable::NotLoaded;
            repo_state.diff_state.diff_preview_text_file = Loadable::NotLoaded;
            repo_state.diff_state.submodule_summary = Loadable::NotLoaded;
            repo_state.diff_state.inline_submodule_diff = None;
            repo_state.diff_state.diff_file_image = Loadable::NotLoaded;
            repo_state.bump_diff_state_rev();
            invalidate_loaded_blame(repo_state);
            push_action_log(
                repo_state,
                true,
                rust_i18n::t!("store.reducer.label_commit").to_string(),
                rust_i18n::t!("store.reducer.commit_done").to_string(),
                None,
            );
        }
        Err(e) => {
            let summary = format_failure_summary(&rust_i18n::t!("store.reducer.label_commit"), &e);
            repo_state.last_error = Some(summary.clone());
            push_action_log(
                repo_state,
                false,
                rust_i18n::t!("store.reducer.label_commit").to_string(),
                summary,
                Some(&e),
            );
        }
    }
    if !clear_banner {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        let mut effects = Vec::with_capacity(refresh_primary_effect_capacity());
        append_refresh_primary_effects(repo_state, &mut effects);
        return effects;
    }
    // A finished commit mutated the repo like any other action, so the loads
    // issued before it are stale (the file watcher usually has a full status
    // scan in flight over the index it just rewrote): cancel them, then let
    // the committed paths answer through the targeted scan so the staged
    // panel empties after one pathspec `git status` rather than that walk.
    let mut effects: Vec<Effect> = Vec::new();
    append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects);
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return effects;
    };
    append_targeted_status_refresh(repo_state, &mut effects, &committed_paths);
    append_refresh_primary_effects(repo_state, &mut effects);
    clear_banner_error_for_repo(state, repo_id);
    effects
}

pub(super) fn commit_amend_finished(
    state: &mut AppState,
    repo_id: RepoId,
    result: std::result::Result<(), Error>,
) -> Vec<Effect> {
    let mut clear_banner = false;
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };
    // The pre-amend staged entries, collected before anything below mutates:
    // an amend commits the index just like a plain commit.
    let committed_paths: Vec<PathBuf> = repo_state
        .staged_status_entries()
        .map(|entries| entries.iter().map(|entry| entry.path.clone()).collect())
        .unwrap_or_default();
    repo_state.local_actions_in_flight = repo_state.local_actions_in_flight.saturating_sub(1);
    repo_state.commit_in_flight = repo_state.commit_in_flight.saturating_sub(1);
    repo_state.bump_ops_rev();
    match result {
        Ok(()) => {
            repo_state.last_error = None;
            clear_banner = true;
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
            repo_state.set_diff_target(None);
            repo_state.diff_state.diff = Loadable::NotLoaded;
            repo_state.diff_state.diff_file = Loadable::NotLoaded;
            repo_state.diff_state.diff_file_lfs = Loadable::NotLoaded;
            repo_state.diff_state.diff_preview_text_file = Loadable::NotLoaded;
            repo_state.diff_state.submodule_summary = Loadable::NotLoaded;
            repo_state.diff_state.inline_submodule_diff = None;
            repo_state.diff_state.diff_file_image = Loadable::NotLoaded;
            repo_state.bump_diff_state_rev();
            invalidate_loaded_blame(repo_state);
            push_action_log(
                repo_state,
                true,
                rust_i18n::t!("store.reducer.label_amend").to_string(),
                rust_i18n::t!("store.reducer.amend_done").to_string(),
                None,
            );
        }
        Err(e) => {
            let summary = format_failure_summary(&rust_i18n::t!("store.reducer.label_amend"), &e);
            repo_state.last_error = Some(summary.clone());
            push_action_log(
                repo_state,
                false,
                rust_i18n::t!("store.reducer.label_amend").to_string(),
                summary,
                Some(&e),
            );
        }
    }
    if !clear_banner {
        let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
            return Vec::new();
        };
        let mut effects = Vec::with_capacity(refresh_primary_effect_capacity());
        append_refresh_primary_effects(repo_state, &mut effects);
        return effects;
    }
    // Same shape as a plain commit: cancel the now-stale loads, answer the
    // committed paths through the targeted scan, and let the primary refresh
    // cover the head-anchored panes.
    let mut effects: Vec<Effect> = Vec::new();
    append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects);
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return effects;
    };
    append_targeted_status_refresh(repo_state, &mut effects, &committed_paths);
    append_refresh_primary_effects(repo_state, &mut effects);
    clear_banner_error_for_repo(state, repo_id);
    effects
}

pub(super) fn safe_push_after_commit_finished(
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
    repo_id: RepoId,
    auth: Option<StagedGitAuth>,
    result: std::result::Result<worktree_core::services::SafePushAfterCommitDecision, Error>,
) -> Vec<Effect> {
    match result {
        Ok(worktree_core::services::SafePushAfterCommitDecision::Push { target }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_force_push_lease = None;
            }
            push_after_commit_with_auth(repos, state, repo_id, target, false, auth)
        }
        Ok(worktree_core::services::SafePushAfterCommitDecision::PushSetUpstream { target }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_force_push_lease = None;
            }
            push_after_commit_with_auth(repos, state, repo_id, target, true, auth)
        }
        Ok(worktree_core::services::SafePushAfterCommitDecision::Blocked { summary, lease }) => {
            let git_log_settings = state.git_log_settings;
            let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
                return Vec::new();
            };
            let full_summary =
                rust_i18n::t!("store.reducer.push_after_commit_blocked", summary = summary)
                    .to_string();
            repo_state.pending_force_push_lease = lease;
            repo_state.last_error = Some(full_summary.clone());
            push_action_log(
                repo_state,
                false,
                rust_i18n::t!("store.reducer.label_push_after_commit").to_string(),
                full_summary,
                None,
            );
            let mut effects = Vec::with_capacity(refresh_full_effect_capacity());
            append_refresh_full_effects(repo_state, git_log_settings, &mut effects);
            effects
        }
        Err(e) => {
            let git_log_settings = state.git_log_settings;
            let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
                return Vec::new();
            };
            repo_state.pending_force_push_lease = None;
            let summary =
                format_failure_summary(&rust_i18n::t!("store.reducer.label_push_after_commit"), &e);
            repo_state.last_error = Some(summary.clone());
            push_action_log(
                repo_state,
                false,
                rust_i18n::t!("store.reducer.label_push_after_commit").to_string(),
                summary,
                Some(&e),
            );
            let mut effects = Vec::with_capacity(refresh_full_effect_capacity());
            append_refresh_full_effects(repo_state, git_log_settings, &mut effects);
            effects
        }
    }
}

fn tracks_local_actions_in_flight(command: &RepoCommandKind) -> bool {
    matches!(
        command,
        RepoCommandKind::MergeRef { .. }
            | RepoCommandKind::SquashRef { .. }
            | RepoCommandKind::Reset { .. }
            | RepoCommandKind::SquashCommits { .. }
            | RepoCommandKind::Rebase { .. }
            | RepoCommandKind::RebaseContinue
            | RepoCommandKind::RebaseAbort
            | RepoCommandKind::BisectStart { .. }
            | RepoCommandKind::BisectMark { .. }
            | RepoCommandKind::BisectReset
            | RepoCommandKind::InteractiveRebase { .. }
            | RepoCommandKind::InteractiveCherryPick { .. }
            | RepoCommandKind::CherryPick { .. }
            | RepoCommandKind::MergeAbort
            | RepoCommandKind::CreateTag { .. }
            | RepoCommandKind::DeleteTag { .. }
            | RepoCommandKind::AddRemote { .. }
            | RepoCommandKind::RemoveRemote { .. }
            | RepoCommandKind::SetRemoteUrl { .. }
            | RepoCommandKind::SetRemoteSshKey { .. }
            | RepoCommandKind::SetUpstreamBranch { .. }
            | RepoCommandKind::UnsetUpstreamBranch { .. }
            | RepoCommandKind::FastForwardBranch { .. }
            | RepoCommandKind::CheckoutConflict { .. }
            | RepoCommandKind::AcceptConflictDeletion { .. }
            | RepoCommandKind::CheckoutConflictBase { .. }
            | RepoCommandKind::LaunchMergetool { .. }
            | RepoCommandKind::SaveWorktreeFile { .. }
            | RepoCommandKind::AppendGitignorePatterns { .. }
            | RepoCommandKind::ExportPatch { .. }
            | RepoCommandKind::ArchiveZip { .. }
            | RepoCommandKind::Cleanup
            | RepoCommandKind::ApplyPatch { .. }
            | RepoCommandKind::AddSubmodule { .. }
            | RepoCommandKind::UpdateSubmodules { .. }
            | RepoCommandKind::LoadSubmodule { .. }
            | RepoCommandKind::ChangeSubmodulePointer { .. }
            | RepoCommandKind::RemoveSubmodule { .. }
            | RepoCommandKind::StageHunk
            | RepoCommandKind::UnstageHunk
            | RepoCommandKind::ApplyWorktreePatch { .. }
    )
}

fn command_clears_pending_force_push_lease(command: &RepoCommandKind) -> bool {
    matches!(
        command,
        RepoCommandKind::Pull { .. }
            | RepoCommandKind::PullBranch { .. }
            | RepoCommandKind::MergeRef { .. }
            | RepoCommandKind::SquashRef { .. }
            | RepoCommandKind::Push
            | RepoCommandKind::PushAfterCommit { .. }
            | RepoCommandKind::ForcePush
            | RepoCommandKind::ForcePushWithLease { .. }
            | RepoCommandKind::PushMergeRequest { .. }
            | RepoCommandKind::PushSetUpstream { .. }
            | RepoCommandKind::Reset { .. }
            | RepoCommandKind::Rebase { .. }
            | RepoCommandKind::RebaseContinue
            | RepoCommandKind::RebaseAbort
            | RepoCommandKind::InteractiveRebase { .. }
            | RepoCommandKind::InteractiveCherryPick { .. }
            | RepoCommandKind::CherryPick { .. }
            | RepoCommandKind::MergeAbort
    )
}

fn changed_submodule_path(command: &RepoCommandKind) -> Option<&std::path::Path> {
    match command {
        RepoCommandKind::LoadSubmodule { path, .. }
        | RepoCommandKind::ChangeSubmodulePointer { path, .. } => Some(path.as_path()),
        _ => None,
    }
}

fn selected_submodule_target_changed_by_command(
    repo_state: &RepoState,
    command: &RepoCommandKind,
) -> Option<DiffTarget> {
    let target = repo_state.diff_state.diff_target.as_ref()?;
    let DiffTarget::WorkingTree { path, .. } = target else {
        return None;
    };
    if let Some(changed_path) = changed_submodule_path(command) {
        if path.as_path() != changed_path {
            return None;
        }
    } else if !matches!(command, RepoCommandKind::UpdateSubmodules { .. }) {
        return None;
    }

    let summary_is_active = !matches!(repo_state.diff_state.submodule_summary, Loadable::NotLoaded);
    (summary_is_active || selected_diff_load_plan(repo_state, target).load_submodule_summary)
        .then(|| target.clone())
}

pub(super) fn repo_command_finished(
    state: &mut AppState,
    repo_id: RepoId,
    command: RepoCommandKind,
    result: std::result::Result<CommandOutput, Error>,
) -> Vec<Effect> {
    let refresh_worktrees = matches!(
        &command,
        RepoCommandKind::AddWorktree { .. }
            | RepoCommandKind::RemoveWorktree { .. }
            | RepoCommandKind::ForceRemoveWorktree { .. }
    ) && result.is_ok();
    let refresh_submodules = matches!(
        &command,
        RepoCommandKind::AddSubmodule { .. }
            | RepoCommandKind::UpdateSubmodules { .. }
            | RepoCommandKind::LoadSubmodule { .. }
            | RepoCommandKind::ChangeSubmodulePointer { .. }
            | RepoCommandKind::RemoveSubmodule { .. }
    ) && result.is_ok();
    let command_succeeded = result.is_ok();
    // Tag CRUD refreshes the list, and so do the commands that can bring new
    // tags in from a remote — a fetch or pull that adds tags used to leave the
    // sidebar showing the stale list until something rewrote a tag locally.
    let refresh_tags = command_succeeded
        && matches!(
            &command,
            RepoCommandKind::CreateTag { .. }
                | RepoCommandKind::DeleteTag { .. }
                | RepoCommandKind::PruneLocalTags
                | RepoCommandKind::FetchAll
                | RepoCommandKind::AutoFetchAll
                | RepoCommandKind::Pull { .. }
                | RepoCommandKind::PullBranch { .. }
        );
    let mut clear_banner = false;

    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        return Vec::new();
    };

    let mut extra_effects = Vec::new();
    match &command {
        RepoCommandKind::FetchAll
        | RepoCommandKind::AutoFetchAll
        | RepoCommandKind::PruneMergedBranches
        | RepoCommandKind::PruneLocalTags
        | RepoCommandKind::Pull { .. }
        | RepoCommandKind::PullBranch { .. } => {
            repo_state.pull_in_flight = repo_state.pull_in_flight.saturating_sub(1);
            repo_state.bump_ops_rev();
        }
        RepoCommandKind::Push
        | RepoCommandKind::PushAfterCommit { .. }
        | RepoCommandKind::ForcePush
        | RepoCommandKind::ForcePushWithLease { .. }
        | RepoCommandKind::PushMergeRequest { .. }
        | RepoCommandKind::PushSetUpstream { .. }
        | RepoCommandKind::DeleteRemoteBranch { .. }
        | RepoCommandKind::DeleteRemoteBranches { .. }
        | RepoCommandKind::PushTag { .. }
        | RepoCommandKind::DeleteRemoteTag { .. } => {
            repo_state.push_in_flight = repo_state.push_in_flight.saturating_sub(1);
            repo_state.bump_ops_rev();
        }
        RepoCommandKind::AddWorktree { .. }
        | RepoCommandKind::RemoveWorktree { .. }
        | RepoCommandKind::ForceRemoveWorktree { .. } => {
            repo_state.worktrees_in_flight = repo_state.worktrees_in_flight.saturating_sub(1);
        }
        _ if tracks_local_actions_in_flight(&command) => {
            repo_state.local_actions_in_flight =
                repo_state.local_actions_in_flight.saturating_sub(1);
            repo_state.bump_ops_rev();
        }
        _ => {}
    }

    if matches!(&command, RepoCommandKind::AddSubmodule { .. }) {
        repo_state.submodule_add_in_flight = None;
    }

    match result {
        Ok(output) => {
            repo_state.last_error = None;
            clear_banner = true;
            if command_clears_pending_force_push_lease(&command) {
                repo_state.pending_force_push_lease = None;
            }
            repo_state.set_recent_commit_messages(Loadable::NotLoaded);
            if matches!(
                &command,
                RepoCommandKind::Reset { .. }
                    | RepoCommandKind::SquashCommits { .. }
                    | RepoCommandKind::Rebase { .. }
                    | RepoCommandKind::RebaseContinue
                    | RepoCommandKind::RebaseAbort
                    | RepoCommandKind::BisectStart { .. }
                    | RepoCommandKind::BisectMark { .. }
                    | RepoCommandKind::BisectReset
                    | RepoCommandKind::InteractiveRebase { .. }
                    | RepoCommandKind::InteractiveCherryPick { .. }
                    | RepoCommandKind::CherryPick { .. }
                    | RepoCommandKind::MergeAbort
            ) {
                repo_state.set_diff_target(None);
                repo_state.diff_state.diff = Loadable::NotLoaded;
                repo_state.diff_state.diff_file = Loadable::NotLoaded;
                repo_state.diff_state.diff_file_lfs = Loadable::NotLoaded;
                repo_state.diff_state.diff_preview_text_file = Loadable::NotLoaded;
                repo_state.diff_state.submodule_summary = Loadable::NotLoaded;
                repo_state.diff_state.inline_submodule_diff = None;
                repo_state.diff_state.diff_file_image = Loadable::NotLoaded;
                repo_state.bump_diff_state_rev();
            }
            if matches!(
                &command,
                RepoCommandKind::SquashCommits { .. }
                    | RepoCommandKind::InteractiveRebase { .. }
                    | RepoCommandKind::InteractiveCherryPick { .. }
            ) {
                // The squashed/rebased commits may no longer exist; clear the
                // selection and the prompt's preview.
                repo_state.set_selected_commit(None);
                repo_state.set_commit_details(Loadable::NotLoaded);
                repo_state.history_state.squash_preview_pending = None;
                repo_state.set_squash_preview(Loadable::NotLoaded);
            }
            push_command_log(repo_state, true, &command, &output, None);
        }
        Err(e) => {
            push_command_log(
                repo_state,
                false,
                &command,
                &CommandOutput::default(),
                Some(&e),
            );
            repo_state.last_error = repo_state
                .command_log
                .last()
                .map(|entry| entry.summary.clone());
        }
    }
    if command_succeeded && sync_conflict_session_after_resolution_command(repo_state, &command) {
        repo_state.bump_conflict_rev();
    }

    // A completed repo command (fetch/pull/push/...) mutated the repository.
    // Cancel stale in-flight loads first so a slow log walk does not swallow the
    // refresh (the "commit list is stale after a fetch/pull/push" race), then
    // translate the raw trigger into a semantic `RepoChange` and let the single
    // dispatch point decide which panels to refresh. This mirrors the
    // invalidation repo_action_finished performs for local actions.
    //
    // The cancel runs BEFORE the command-specific refreshes below: the cancel's
    // `clear_cancelled_repo_loading` resets worktrees / submodules / the active
    // diff back to `NotLoaded`, and the blocks that follow re-raise those flags
    // for the panels the full refresh does not cover. Running them in the other
    // order would let the cancel wipe the `Loading` flags these blocks set.
    let mut effects = Vec::new();
    append_cancel_repo_loads_effect_for_repo(state, Some(repo_id), &mut effects);
    effects.extend(super::repo_change::dispatch_repo_change(
        state,
        repo_id,
        RepoChange::from_repo_command_kind(&command, command_succeeded),
        None,
    ));

    // Command-specific refreshes the full refresh does not cover (worktrees /
    // submodules / tags / the active diff). These run AFTER the cancel so the
    // cancel does not reset their `Loading` flags.
    let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) else {
        if clear_banner {
            clear_banner_error_for_repo(state, repo_id);
        }
        return effects;
    };
    if refresh_worktrees {
        repo_state.set_worktrees(Loadable::Loading);
        extra_effects.push(Effect::LoadWorktrees { repo_id });
    }
    if command_succeeded
        && let Some(target) = selected_submodule_target_changed_by_command(repo_state, &command)
    {
        let load_plan = selected_diff_load_plan(repo_state, &target);
        apply_selected_diff_load_plan_state(repo_state, load_plan);
        repo_state.diff_state.inline_submodule_diff = None;
        repo_state.bump_diff_state_rev();
        append_diff_reload_effects(&mut extra_effects, repo_state, repo_id, target);
    }
    if refresh_submodules {
        repo_state.set_submodules(Loadable::Loading);
        if repo_state
            .loads_in_flight
            .request(RepoLoadsInFlight::SUBMODULES)
        {
            extra_effects.push(Effect::LoadSubmodules { repo_id });
        }
    }
    if refresh_tags {
        repo_state.set_tags(Loadable::NotLoaded);
        if repo_state.loads_in_flight.request(RepoLoadsInFlight::TAGS) {
            extra_effects.push(Effect::LoadTags { repo_id });
        }
    }
    if matches!(
        &command,
        RepoCommandKind::StageHunk
            | RepoCommandKind::UnstageHunk
            | RepoCommandKind::ApplyWorktreePatch { .. }
    ) && let Some(target) = repo_state.diff_state.diff_target.clone()
    {
        // The annotation column is recomputed from the same content the diff
        // shows, so staging/unstaging/patching must invalidate blame too.
        invalidate_loaded_blame(repo_state);
        if let Some(conflict_target) = selected_conflict_target(repo_state, &target) {
            // Blanked, so there is nothing stale left to guard against.
            repo_state.diff_state.diff_reload_in_flight = false;
            repo_state.diff_state.diff = Loadable::NotLoaded;
            repo_state.diff_state.diff_file = Loadable::NotLoaded;
            repo_state.diff_state.diff_file_lfs = Loadable::NotLoaded;
            repo_state.diff_state.diff_preview_text_file = Loadable::NotLoaded;
            repo_state.diff_state.submodule_summary = Loadable::NotLoaded;
            repo_state.diff_state.inline_submodule_diff = None;
            repo_state.diff_state.diff_file_image = Loadable::NotLoaded;
            repo_state.bump_diff_state_rev();
            match conflict_target {
                SelectedConflictTarget::Current => {
                    append_start_current_conflict_target_reload(&mut extra_effects, repo_state);
                }
                SelectedConflictTarget::Path(path) => {
                    append_start_conflict_target_reload(&mut extra_effects, repo_state, path);
                }
            }
        } else {
            let load_plan = selected_diff_load_plan(repo_state, &target);
            // The diff target did not change — only its contents did — so the
            // reload keeps showing what is already there. Blanking it would make
            // the pane flash "Loading" on every staged hunk or line.
            apply_selected_diff_load_plan_state_with_reload_mode(
                repo_state,
                load_plan,
                DiffReloadMode::KeepLoaded,
            );
            repo_state.bump_diff_state_rev();
            append_diff_reload_effects(&mut extra_effects, repo_state, repo_id, target);
        }
    }
    effects.extend(extra_effects);
    if clear_banner {
        clear_banner_error_for_repo(state, repo_id);
    }
    effects
}

fn sync_conflict_session_after_resolution_command(
    repo_state: &mut RepoState,
    command: &RepoCommandKind,
) -> bool {
    let Some(path) = resolution_command_path(command) else {
        return false;
    };

    let tracked_path_matches = repo_state
        .conflict_state
        .conflict_file_path
        .as_ref()
        .is_some_and(|tracked| tracked.as_path() == path.as_path());
    if !tracked_path_matches {
        return false;
    }

    if matches!(command, RepoCommandKind::LaunchMergetool { .. }) {
        clear_conflict_context(repo_state);
        return true;
    }

    let Some(session_view) = repo_state.conflict_state.conflict_session.as_ref() else {
        return false;
    };
    if session_view.path.as_path() != path.as_path() {
        return false;
    }

    if session_view.strategy == ConflictResolverStrategy::BinarySidePick
        && session_view.regions.is_empty()
    {
        clear_conflict_context(repo_state);
        return true;
    }

    let resolution = match command {
        RepoCommandKind::CheckoutConflict { side, .. } => match side {
            worktree_core::services::ConflictSide::Ours => ConflictRegionResolution::PickOurs,
            worktree_core::services::ConflictSide::Theirs => ConflictRegionResolution::PickTheirs,
        },
        RepoCommandKind::CheckoutConflictBase { .. } => ConflictRegionResolution::PickBase,
        RepoCommandKind::AcceptConflictDeletion { .. } => {
            deletion_resolution_for_kind(session_view.conflict_kind)
        }
        _ => return false,
    };

    let Some(session) = repo_state.conflict_state.conflict_session.as_mut() else {
        return false;
    };

    apply_resolution_to_all_regions(session, &resolution) > 0
}

fn resolution_command_path(command: &RepoCommandKind) -> Option<&std::path::PathBuf> {
    match command {
        RepoCommandKind::CheckoutConflict { path, .. }
        | RepoCommandKind::CheckoutConflictBase { path }
        | RepoCommandKind::AcceptConflictDeletion { path }
        | RepoCommandKind::LaunchMergetool { path, .. } => Some(path),
        _ => None,
    }
}

fn clear_conflict_context(repo_state: &mut RepoState) {
    repo_state.conflict_state.conflict_file_path = None;
    repo_state.conflict_state.conflict_file_load_mode =
        crate::model::ConflictFileLoadMode::CurrentOnly;
    repo_state.conflict_state.conflict_file = Loadable::NotLoaded;
    repo_state.conflict_state.session_pending_restore = None;
    repo_state.conflict_state.conflict_session = None;
    repo_state.conflict_state.conflict_hide_resolved = false;
}

fn deletion_resolution_for_kind(conflict_kind: FileConflictKind) -> ConflictRegionResolution {
    match conflict_kind {
        FileConflictKind::DeletedByUs
        | FileConflictKind::AddedByThem
        | FileConflictKind::BothDeleted => ConflictRegionResolution::PickOurs,
        FileConflictKind::DeletedByThem | FileConflictKind::AddedByUs => {
            ConflictRegionResolution::PickTheirs
        }
        FileConflictKind::BothAdded | FileConflictKind::BothModified => {
            ConflictRegionResolution::PickOurs
        }
    }
}

fn apply_resolution_to_all_regions(
    session: &mut worktree_core::conflict_session::ConflictSession,
    resolution: &ConflictRegionResolution,
) -> usize {
    let mut changed = 0usize;
    for region in &mut session.regions {
        if matches!(resolution, ConflictRegionResolution::PickBase) && region.base.is_none() {
            continue;
        }
        if &region.resolution != resolution {
            region.resolution = resolution.clone();
            changed += 1;
        }
    }
    changed
}

pub(super) fn reduce_actions_emit_effects(
    msg: Msg,
    repos: &mut FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: &mut AppState,
) -> ReduceOutcome {
    let effects = match msg {
        Msg::CheckoutBranch { repo_id, name } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_detached_head_commit(None);
            }
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::checkout_branch(repo_id, name)
        }
        Msg::CheckoutRemoteBranch {
            repo_id,
            remote,
            branch,
            local_branch,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_detached_head_commit(None);
            }
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::checkout_remote_branch(repo_id, remote, branch, local_branch)
        }
        Msg::CheckoutPullRequest {
            repo_id,
            remote,
            number,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_detached_head_commit(None);
            }
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::checkout_pull_request(repo_id, remote, number)
        }
        Msg::CheckoutCommit { repo_id, commit_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_detached_head_commit(Some(commit_id.clone()));
            }
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::checkout_commit(repo_id, commit_id)
        }
        Msg::CherryPickCommit {
            repo_id,
            commit_id,
            commit,
            mainline,
            summary,
        } => {
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::cherry_pick_commit(repo_id, commit_id, commit, mainline, summary)
        }
        Msg::RevertCommit { repo_id, commit_id } => {
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::revert_commit(repo_id, commit_id)
        }
        Msg::CreateBranch {
            repo_id,
            name,
            target,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::create_branch(repo_id, name, target)
        }
        Msg::CreateBranchAndCheckout {
            repo_id,
            name,
            target,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_detached_head_commit(None);
            }
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::create_branch_and_checkout(repo_id, name, target)
        }
        Msg::RenameBranch {
            repo_id,
            old_name,
            new_name,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::rename_branch(repo_id, old_name, new_name)
        }
        Msg::DeleteBranch { repo_id, name } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::delete_branch(repo_id, name)
        }
        Msg::ForceDeleteBranch { repo_id, name } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::force_delete_branch(repo_id, name)
        }
        Msg::DeleteBranches {
            repo_id,
            names,
            force,
        } => {
            if names.is_empty() {
                return ReduceOutcome::Handled(Vec::new());
            }
            begin_local_action(state, repo_id);
            actions_emit_effects::delete_branches(repo_id, names, force)
        }
        Msg::ExportPatch {
            repo_id,
            commit_id,
            dest,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::export_patch(repo_id, commit_id, dest)
        }
        Msg::ArchiveZip {
            repo_id,
            revision,
            dest,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::archive_zip(repo_id, revision, dest)
        }
        Msg::CleanupRepo { repo_id } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::cleanup_repo(repo_id)
        }
        Msg::ApplyPatch { repo_id, patch } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::apply_patch(repo_id, patch)
        }
        Msg::AddWorktree {
            repo_id,
            path,
            reference,
        } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.worktrees_in_flight = repo_state.worktrees_in_flight.saturating_add(1);
            }
            actions_emit_effects::add_worktree(repo_id, path, reference)
        }
        Msg::RemoveWorktree { repo_id, path } => {
            let normalized_path = if let Some(repo_state) =
                state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                repo_state.worktrees_in_flight = repo_state.worktrees_in_flight.saturating_add(1);
                normalize_repo_relative_path(&repo_state.spec.workdir, path)
            } else {
                path
            };
            actions_emit_effects::remove_worktree(repo_id, normalized_path)
        }
        Msg::ForceRemoveWorktree { repo_id, path } => {
            let normalized_path = if let Some(repo_state) =
                state.repos.iter_mut().find(|r| r.id == repo_id)
            {
                repo_state.worktrees_in_flight = repo_state.worktrees_in_flight.saturating_add(1);
                normalize_repo_relative_path(&repo_state.spec.workdir, path)
            } else {
                path
            };
            actions_emit_effects::force_remove_worktree(repo_id, normalized_path)
        }
        Msg::AddSubmodule {
            repo_id,
            url,
            path,
            branch,
            name,
            force,
        } => {
            state.submodule_trust_prompt = None;
            state.submodule_trust_check_pending = Some(SubmoduleTrustCheckState {
                repo_id,
                operation: SubmoduleTrustCheckOperation::Add,
            });
            vec![Effect::CheckSubmoduleAddTrust {
                repo_id,
                url,
                path,
                branch,
                name,
                force,
            }]
        }
        Msg::AddSubmoduleTrusted {
            repo_id,
            url,
            path,
            branch,
            name,
            force,
            approved_sources,
        } => {
            begin_local_action(state, repo_id);
            start_submodule_add_progress(state, repo_id, &url, &path);
            actions_emit_effects::add_submodule(
                repo_id,
                url,
                path,
                branch,
                name,
                force,
                approved_sources,
            )
        }
        Msg::UpdateSubmodules { repo_id } => {
            state.submodule_trust_prompt = None;
            state.submodule_trust_check_pending = Some(SubmoduleTrustCheckState {
                repo_id,
                operation: SubmoduleTrustCheckOperation::Update,
            });
            vec![Effect::CheckSubmoduleUpdateTrust { repo_id }]
        }
        Msg::UpdateSubmodulesTrusted {
            repo_id,
            approved_sources,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::update_submodules(repo_id, approved_sources)
        }
        Msg::LoadSubmodule { repo_id, path } => {
            state.submodule_trust_prompt = None;
            state.submodule_trust_check_pending = Some(SubmoduleTrustCheckState {
                repo_id,
                operation: SubmoduleTrustCheckOperation::Load,
            });
            vec![Effect::CheckSubmoduleLoadTrust { repo_id, path }]
        }
        Msg::LoadSubmoduleTrusted {
            repo_id,
            path,
            approved_sources,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::load_submodule(repo_id, path, approved_sources)
        }
        Msg::ConfirmSubmoduleTrustPrompt => {
            let Some(prompt) = state.submodule_trust_prompt.take() else {
                return ReduceOutcome::Handled(Vec::new());
            };
            match prompt.operation {
                SubmoduleTrustPromptOperation::Add {
                    url,
                    path,
                    branch,
                    name,
                    force,
                } => {
                    begin_local_action(state, prompt.repo_id);
                    start_submodule_add_progress(state, prompt.repo_id, &url, &path);
                    actions_emit_effects::add_submodule(
                        prompt.repo_id,
                        url,
                        path,
                        branch,
                        name,
                        force,
                        prompt.sources,
                    )
                }
                SubmoduleTrustPromptOperation::Update => {
                    begin_local_action(state, prompt.repo_id);
                    actions_emit_effects::update_submodules(prompt.repo_id, prompt.sources)
                }
                SubmoduleTrustPromptOperation::Load { path } => {
                    begin_local_action(state, prompt.repo_id);
                    actions_emit_effects::load_submodule(prompt.repo_id, path, prompt.sources)
                }
            }
        }
        Msg::CancelSubmoduleTrustPrompt => {
            state.submodule_trust_prompt = None;
            Vec::new()
        }
        Msg::ChangeSubmodulePointer {
            repo_id,
            path,
            reference,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::change_submodule_pointer(repo_id, path, reference)
        }
        Msg::RemoveSubmodule { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::remove_submodule(repo_id, path)
        }
        Msg::StagePath { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::stage_path(repo_id, path)
        }
        Msg::StagePaths { repo_id, paths } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::stage_paths(repo_id, paths)
        }
        Msg::UnstagePath { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::unstage_path(repo_id, path)
        }
        Msg::UnstagePaths { repo_id, paths } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::unstage_paths(repo_id, paths)
        }
        Msg::DiscardWorktreeChangesPath { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::discard_worktree_changes_path(repo_id, path)
        }
        Msg::DiscardWorktreeChangesPaths { repo_id, paths } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::discard_worktree_changes_paths(repo_id, paths)
        }
        Msg::SaveWorktreeFile {
            repo_id,
            path,
            contents,
            stage,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::save_worktree_file(repo_id, path, contents, stage)
        }
        Msg::AppendGitignorePatterns { repo_id, patterns } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::append_gitignore_patterns(repo_id, patterns)
        }
        Msg::Commit {
            repo_id,
            message,
            push_after_commit,
        } => {
            begin_commit_action(state, repo_id);
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_commit_retry = Some(PendingCommitRetry {
                    message: message.clone(),
                    amend: false,
                    push_after_commit,
                });
            }
            actions_emit_effects::commit(repo_id, message)
        }
        Msg::CommitAmend {
            repo_id,
            message,
            push_after_commit,
        } => {
            begin_commit_action(state, repo_id);
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_commit_retry = Some(PendingCommitRetry {
                    message: message.clone(),
                    amend: true,
                    push_after_commit,
                });
            }
            actions_emit_effects::commit_amend(repo_id, message)
        }
        Msg::CommitFixup {
            repo_id,
            target,
            push_after_commit,
        } => actions_emit_effects::commit_fixup(state, repo_id, target, push_after_commit),
        Msg::SafePushAfterCommit { repo_id, context } => {
            actions_emit_effects::safe_push_after_commit(repo_id, context)
        }
        Msg::FetchAll { repo_id } => actions_emit_effects::fetch_all(repos, state, repo_id),
        Msg::AutoFetchAll { repo_id } => {
            actions_emit_effects::auto_fetch_all(repos, state, repo_id)
        }
        Msg::PruneMergedBranches { repo_id } => {
            actions_emit_effects::prune_merged_branches(repos, state, repo_id)
        }
        Msg::PruneLocalTags { repo_id } => {
            actions_emit_effects::prune_local_tags(repos, state, repo_id)
        }
        Msg::Pull { repo_id, mode } => actions_emit_effects::pull(repos, state, repo_id, mode),
        Msg::PullBranch {
            repo_id,
            remote,
            branch,
        } => actions_emit_effects::pull_branch(repos, state, repo_id, remote, branch),
        Msg::MergeRef { repo_id, reference } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::merge_ref(repo_id, reference)
        }
        Msg::SquashRef { repo_id, reference } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::squash_ref(repo_id, reference)
        }
        Msg::Push {
            repo_id,
            pull_retry,
        } => actions_emit_effects::push(repos, state, repo_id, pull_retry),
        Msg::PushAfterCommit {
            repo_id,
            target,
            set_upstream,
        } => actions_emit_effects::push_after_commit(repos, state, repo_id, target, set_upstream),
        Msg::ForcePush { repo_id } => actions_emit_effects::force_push(repos, state, repo_id),
        Msg::ForcePushWithLease { repo_id, lease } => {
            actions_emit_effects::force_push_with_lease(repos, state, repo_id, lease)
        }
        Msg::PushMergeRequest { repo_id, options } => {
            actions_emit_effects::push_merge_request(repos, state, repo_id, options)
        }
        Msg::PushSetUpstream {
            repo_id,
            remote,
            branch,
        } => actions_emit_effects::push_set_upstream(repos, state, repo_id, remote, branch),
        Msg::SetUpstreamBranch {
            repo_id,
            branch,
            upstream,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::set_upstream_branch(repo_id, branch, upstream)
        }
        Msg::UnsetUpstreamBranch { repo_id, branch } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::unset_upstream_branch(repo_id, branch)
        }
        Msg::FastForwardBranch { repo_id, branch } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::fast_forward_branch(repo_id, branch)
        }
        Msg::DeleteRemoteBranch {
            repo_id,
            remote,
            branch,
        } => actions_emit_effects::delete_remote_branch(repos, state, repo_id, remote, branch),
        Msg::DeleteRemoteBranches {
            repo_id,
            remote,
            branches,
        } => {
            if branches.is_empty() {
                return ReduceOutcome::Handled(Vec::new());
            }
            actions_emit_effects::delete_remote_branches(repos, state, repo_id, remote, branches)
        }
        Msg::Reset {
            repo_id,
            target,
            mode,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::reset(repo_id, target, mode)
        }
        Msg::SquashCommits {
            repo_id,
            oldest,
            expected_head,
            message,
            count,
        } => actions_emit_effects::squash_commits(
            state,
            repo_id,
            oldest,
            expected_head,
            message,
            count,
        ),
        Msg::Rebase { repo_id, onto } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::rebase(repo_id, onto)
        }
        Msg::RebaseContinue { repo_id } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::rebase_continue(repo_id)
        }
        Msg::RebaseAbort { repo_id } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::rebase_abort(repo_id)
        }
        Msg::BisectStart {
            repo_id,
            bad,
            goods,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::bisect_start(repo_id, bad, goods)
        }
        Msg::BisectMark {
            repo_id,
            verdict,
            commit,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::bisect_mark(repo_id, verdict, commit)
        }
        Msg::BisectReset { repo_id } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::bisect_reset(repo_id)
        }
        Msg::LoadInteractiveRebaseSetup { repo_id, base } => {
            actions_emit_effects::load_interactive_rebase_setup(state, repo_id, base)
        }
        Msg::OpenInteractiveCherryPickSetup {
            repo_id,
            entries,
            source_colors,
        } => actions_emit_effects::open_interactive_cherry_pick_setup(
            state,
            repo_id,
            entries,
            source_colors,
        ),
        Msg::InteractiveRebase {
            repo_id,
            base,
            entries,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::interactive_rebase(repo_id, base, entries)
        }
        Msg::InteractiveCherryPick { repo_id, entries } => {
            // A multi-pick can land some commits and then fail (a hook or
            // signer on a later step), so HEAD-dependent caches must be
            // invalidated up front like the single-pick path — the error
            // completion path does not clear them.
            begin_head_changing_local_action(state, repo_id);
            actions_emit_effects::interactive_cherry_pick(repo_id, entries)
        }
        Msg::CancelInteractiveRebaseSetup { repo_id } => {
            actions_emit_effects::cancel_interactive_rebase_setup(state, repo_id)
        }
        Msg::CancelInteractiveCherryPickSetup { repo_id } => {
            actions_emit_effects::cancel_interactive_cherry_pick_setup(state, repo_id)
        }
        Msg::MergeAbort { repo_id } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::merge_abort(repo_id)
        }
        Msg::CreateTag {
            repo_id,
            name,
            target,
            message,
            annotated,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::create_tag(repo_id, name, target, message, annotated)
        }
        Msg::DeleteTag { repo_id, name } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::delete_tag(repo_id, name)
        }
        Msg::PushTag {
            repo_id,
            remote,
            name,
        } => actions_emit_effects::push_tag(repos, state, repo_id, remote, name),
        Msg::DeleteRemoteTag {
            repo_id,
            remote,
            name,
        } => actions_emit_effects::delete_remote_tag(repos, state, repo_id, remote, name),
        Msg::AddRemote { repo_id, name, url } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::add_remote(repo_id, name, url)
        }
        Msg::RemoveRemote { repo_id, name } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::remove_remote(repo_id, name)
        }
        Msg::SetRemoteUrl {
            repo_id,
            name,
            url,
            kind,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::set_remote_url(repo_id, name, url, kind)
        }
        Msg::SetRemoteSshKey {
            repo_id,
            remote,
            key,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::set_remote_ssh_key(repo_id, remote, key)
        }
        Msg::CheckoutConflictSide {
            repo_id,
            path,
            side,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::checkout_conflict_side(repo_id, path, side)
        }
        Msg::AcceptConflictDeletion { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::accept_conflict_deletion(repo_id, path)
        }
        Msg::CheckoutConflictBase { repo_id, path } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::checkout_conflict_base(repo_id, path)
        }
        Msg::LaunchMergetool {
            repo_id,
            path,
            preference,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::launch_mergetool(repo_id, path, preference)
        }
        Msg::Stash {
            repo_id,
            message,
            include_untracked,
            keep_index,
            paths,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::stash(repo_id, message, include_untracked, keep_index, paths)
        }
        Msg::ApplyStash { repo_id, index } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::apply_stash(repo_id, index)
        }
        Msg::PopStash { repo_id, index } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::pop_stash(repo_id, index)
        }
        Msg::DropStash { repo_id, index } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::drop_stash(repo_id, index)
        }
        Msg::SetAssumeUnchanged {
            repo_id,
            path,
            enable,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::set_assume_unchanged(repo_id, path, enable)
        }
        Msg::LoadAssumeUnchanged { repo_id } => {
            actions_emit_effects::load_assume_unchanged(repo_id)
        }
        Msg::StashBranch {
            repo_id,
            index,
            branch,
        } => {
            begin_local_action(state, repo_id);
            actions_emit_effects::stash_branch(repo_id, index, branch)
        }
        Msg::Internal(crate::msg::InternalMsg::SubmoduleAddTrustChecked {
            repo_id,
            url,
            path,
            branch,
            name,
            force,
            result,
        }) => {
            state.submodule_trust_check_pending = None;
            match result {
                Ok(worktree_core::services::SubmoduleTrustDecision::Proceed) => {
                    begin_local_action(state, repo_id);
                    start_submodule_add_progress(state, repo_id, &url, &path);
                    actions_emit_effects::add_submodule(
                        repo_id,
                        url,
                        path,
                        branch,
                        name,
                        force,
                        Vec::new(),
                    )
                }
                Ok(worktree_core::services::SubmoduleTrustDecision::Prompt { sources }) => {
                    state.submodule_trust_prompt = Some(SubmoduleTrustPromptState {
                        repo_id,
                        operation: SubmoduleTrustPromptOperation::Add {
                            url,
                            path,
                            branch,
                            name,
                            force,
                        },
                        sources,
                    });
                    Vec::new()
                }
                Err(error) => {
                    state.banner_error = Some(BannerErrorState {
                        repo_id: Some(repo_id),
                        message: util::format_failure_summary(
                            &rust_i18n::t!("store.reducer.label_submodule_trust_check"),
                            &error,
                        ),
                    });
                    Vec::new()
                }
            }
        }
        Msg::Internal(crate::msg::InternalMsg::SubmoduleUpdateTrustChecked { repo_id, result }) => {
            state.submodule_trust_check_pending = None;
            match result {
                Ok(worktree_core::services::SubmoduleTrustDecision::Proceed) => {
                    begin_local_action(state, repo_id);
                    actions_emit_effects::update_submodules(repo_id, Vec::new())
                }
                Ok(worktree_core::services::SubmoduleTrustDecision::Prompt { sources }) => {
                    state.submodule_trust_prompt = Some(SubmoduleTrustPromptState {
                        repo_id,
                        operation: SubmoduleTrustPromptOperation::Update,
                        sources,
                    });
                    Vec::new()
                }
                Err(error) => {
                    state.banner_error = Some(BannerErrorState {
                        repo_id: Some(repo_id),
                        message: util::format_failure_summary(
                            &rust_i18n::t!("store.reducer.label_submodule_trust_check"),
                            &error,
                        ),
                    });
                    Vec::new()
                }
            }
        }
        Msg::Internal(crate::msg::InternalMsg::SubmoduleLoadTrustChecked {
            repo_id,
            path,
            result,
        }) => {
            state.submodule_trust_check_pending = None;
            match result {
                Ok(worktree_core::services::SubmoduleTrustDecision::Proceed) => {
                    begin_local_action(state, repo_id);
                    actions_emit_effects::load_submodule(repo_id, path, Vec::new())
                }
                Ok(worktree_core::services::SubmoduleTrustDecision::Prompt { sources }) => {
                    state.submodule_trust_prompt = Some(SubmoduleTrustPromptState {
                        repo_id,
                        operation: SubmoduleTrustPromptOperation::Load { path },
                        sources,
                    });
                    Vec::new()
                }
                Err(error) => {
                    state.banner_error = Some(BannerErrorState {
                        repo_id: Some(repo_id),
                        message: util::format_failure_summary(
                            &rust_i18n::t!("store.reducer.label_submodule_trust_check"),
                            &error,
                        ),
                    });
                    Vec::new()
                }
            }
        }
        Msg::Internal(crate::msg::InternalMsg::CommitFinished { repo_id, result }) => {
            let pending_commit = state
                .repos
                .iter()
                .find(|r| r.id == repo_id)
                .and_then(|r| r.pending_commit_retry.clone());
            let outcome = result.as_ref().ok().cloned();
            let push_after_commit = outcome.is_some()
                && pending_commit
                    .as_ref()
                    .is_some_and(|pending| pending.push_after_commit);
            let auth_prompt = result
                .as_ref()
                .err()
                .and_then(|error| auth_prompt_for_commit(repo_id, pending_commit.clone(), error));
            let commit_result = result.map(|_| ());
            let mut effects = actions_emit_effects::commit_finished(state, repo_id, commit_result);
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_commit_retry = None;
            }
            if let Some(prompt) = auth_prompt {
                util::clear_staged_git_auth_env();
                state.auth_prompt = Some(prompt);
            }
            if push_after_commit
                && let (Some(outcome), Some(pending_commit)) = (outcome, pending_commit)
            {
                effects.extend(actions_emit_effects::safe_push_after_commit(
                    repo_id,
                    SafePushAfterCommitContext {
                        amend: pending_commit.amend,
                        local_branch: outcome.local_branch,
                        pre_head: outcome.pre_head,
                        post_head: outcome.post_head,
                    },
                ));
            }
            effects
        }
        Msg::Internal(crate::msg::InternalMsg::CommitAmendFinished { repo_id, result }) => {
            let pending_commit = state
                .repos
                .iter()
                .find(|r| r.id == repo_id)
                .and_then(|r| r.pending_commit_retry.clone());
            let outcome = result.as_ref().ok().cloned();
            let push_after_commit = outcome.is_some()
                && pending_commit
                    .as_ref()
                    .is_some_and(|pending| pending.push_after_commit);
            let auth_prompt = result
                .as_ref()
                .err()
                .and_then(|error| auth_prompt_for_commit(repo_id, pending_commit.clone(), error));
            let commit_result = result.map(|_| ());
            let mut effects =
                actions_emit_effects::commit_amend_finished(state, repo_id, commit_result);
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.pending_commit_retry = None;
            }
            if let Some(prompt) = auth_prompt {
                util::clear_staged_git_auth_env();
                state.auth_prompt = Some(prompt);
            }
            if push_after_commit
                && let (Some(outcome), Some(pending_commit)) = (outcome, pending_commit)
            {
                effects.extend(actions_emit_effects::safe_push_after_commit(
                    repo_id,
                    SafePushAfterCommitContext {
                        amend: pending_commit.amend,
                        local_branch: outcome.local_branch,
                        pre_head: outcome.pre_head,
                        post_head: outcome.post_head,
                    },
                ));
            }
            effects
        }
        Msg::Internal(crate::msg::InternalMsg::SafePushAfterCommitFinished {
            repo_id,
            context,
            auth,
            result,
        }) => {
            let auth_prompt = result.as_ref().err().and_then(|error| {
                auth_prompt_for_safe_push_after_commit(repo_id, context.clone(), error)
            });
            let effects = actions_emit_effects::safe_push_after_commit_finished(
                repos, state, repo_id, auth, result,
            );
            if let Some(prompt) = auth_prompt {
                util::clear_staged_git_auth_env();
                state.auth_prompt = Some(prompt);
            }
            effects
        }
        Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
            repo_id,
            command,
            result,
        }) => {
            // Logged before `repo_command_finished` consumes the result:
            // every git write the app performs lands here exactly once, so
            // the daily log can answer "what did the app do" end to end.
            match &result {
                Ok(_) => worktree_core::applog_info!(
                    "git command finished: {command:?} (repo_id={})",
                    repo_id.0
                ),
                Err(error) => worktree_core::applog_warn!(
                    "git command failed: {command:?} (repo_id={}): {error}",
                    repo_id.0
                ),
            }
            let auth_prompt = result
                .as_ref()
                .err()
                .and_then(|error| auth_prompt_for_repo_command(repo_id, &command, error));
            let removed_worktree_path = match (&command, &result) {
                (RepoCommandKind::RemoveWorktree { path }, Ok(_)) => Some(path.clone()),
                (RepoCommandKind::ForceRemoveWorktree { path }, Ok(_)) => Some(path.clone()),
                _ => None,
            };

            // Planned before `repo_command_finished` consumes `result`
            // (errors are not cloneable), applied after it so a rejected
            // push's log entry can be rewritten as a retry-in-progress.
            let push_pull_retry_plan =
                actions_emit_effects::push_pull_retry_plan(state, repo_id, &command, &result);
            let mut effects = actions_emit_effects::repo_command_finished(
                state,
                repo_id,
                command.clone(),
                result,
            );
            effects.extend(actions_emit_effects::apply_push_pull_retry(
                repos,
                state,
                repo_id,
                &command,
                push_pull_retry_plan,
            ));

            if let Some(path) = removed_worktree_path {
                let repo_ids_to_close = state
                    .repos
                    .iter()
                    .filter(|repo| repo.spec.workdir == path)
                    .map(|repo| repo.id)
                    .collect::<Vec<_>>();
                for repo_id in repo_ids_to_close {
                    let _ = repo_management::close_repo(repos, state, repo_id);
                }
            }

            if let Some(prompt) = auth_prompt {
                util::clear_staged_git_auth_env();
                state.auth_prompt = Some(prompt);
            }

            effects
        }
        other => return ReduceOutcome::NotHandled(other),
    };
    ReduceOutcome::Handled(effects)
}
