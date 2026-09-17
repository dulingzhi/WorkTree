use super::{
    ReduceOutcome, begin_local_action, conflict_interactions, diff_selection, loaded_results, util,
};
use crate::model::{AppState, Loadable};
use crate::msg::Msg;

mod autosquash;
mod conflict;
mod history_loads;
mod merge_preview;
mod selection;
mod sidebar_browser;
mod squash;
mod status_refs;
mod worktrees;

pub(super) use conflict::{conflict_file_loaded, load_conflict_file};

pub(super) use autosquash::autosquash_rebase_setup_loaded;
pub(super) use history_loads::{
    ai_commit_context_loaded, author_emails_loaded, blame_loaded, commits_searched,
    file_history_loaded, hover_commit_message_loaded, load_ai_commit_context, load_blame,
    load_file_history, load_hover_commit_message, load_recent_commit_messages, load_reflog,
    load_stashes, recent_commit_messages_loaded, reflog_loaded, search_commits, stashes_loaded,
};
pub(super) use merge_preview::merge_preview_loaded;
pub(super) use selection::{
    ComparisonSource, clear_commit_selection, clear_comparison, clear_comparison_mark,
    commit_details_loaded, commit_reveal_resolved, compare_range, compare_with_marked,
    finish_commit_reveal, mark_for_comparison, range_files_loaded, reveal_commit, select_commit,
    select_commit_and_load_details, select_commit_multi,
};
pub(super) use sidebar_browser::{
    append_ensure_sidebar_data_effects, browse_repository_at_commit, ensure_sidebar_data,
    file_browser_loaded, load_file_browser, request_file_browser_load, reset_browse_to_live,
    reveal_file_browser_path, set_file_browser_dir_expanded_recursive, set_file_browser_search,
    set_file_browser_source, set_sidebar_mode, toggle_file_browser_dir,
};
pub(super) use squash::{
    prepare_squash, squash_message_preview_loaded, squash_plan_for_repo, squash_rebase_setup_loaded,
};

pub(super) use status_refs::{
    assume_unchanged_list_loaded, branches_loaded, head_branch_loaded, load_ref_metadata,
    load_remote_tags, load_repo_statistics, load_submodules, load_tags, ref_metadata_loaded,
    refresh_branches, remote_branches_loaded, remote_tags_loaded, remotes_loaded,
    repo_statistics_loaded, staged_status_loaded, status_for_paths_loaded, status_loaded,
    submodules_loaded, tags_loaded, upstream_divergence_loaded, worktree_status_loaded,
};
pub(super) use worktrees::{
    load_worktree_dirty, load_worktrees, request_worktree_dirty_effect,
    retire_orphaned_worktree_diffs, select_working_tree_summary, select_worktree_uncommitted,
    worktree_dirty_loaded, worktrees_loaded,
};

pub(super) fn reduce_loaded_results(msg: Msg, state: &mut AppState) -> ReduceOutcome {
    let effects = match msg {
        Msg::SelectCommit { repo_id, commit_id } => {
            loaded_results::select_commit(state, repo_id, commit_id)
        }
        Msg::SelectCommitMulti {
            repo_id,
            commit_id,
            mode,
            clicked_index,
            visible_order,
        } => loaded_results::select_commit_multi(
            state,
            repo_id,
            commit_id,
            mode,
            clicked_index,
            visible_order,
        ),
        Msg::ClearCommitSelection { repo_id } => {
            loaded_results::clear_commit_selection(state, repo_id)
        }
        Msg::CompareCommitRange {
            repo_id,
            from,
            to,
            from_label,
            to_label,
        } => loaded_results::compare_range(
            state,
            repo_id,
            from,
            Some(to),
            from_label,
            to_label,
            loaded_results::ComparisonSource::Explicit,
        ),
        Msg::CompareWithWorkingTree {
            repo_id,
            from,
            from_label,
        } => loaded_results::compare_range(
            state,
            repo_id,
            from,
            None,
            from_label,
            rust_i18n::t!("store.reducer.label_working_tree").to_string(),
            loaded_results::ComparisonSource::Explicit,
        ),
        Msg::ClearComparison { repo_id } => loaded_results::clear_comparison(state, repo_id),
        Msg::MarkForComparison {
            repo_id,
            commit_id,
            label,
        } => loaded_results::mark_for_comparison(state, repo_id, commit_id, label),
        Msg::CompareWithMarked {
            repo_id,
            commit_id,
            label,
        } => loaded_results::compare_with_marked(state, repo_id, commit_id, label),
        Msg::ClearComparisonMark { repo_id } => {
            loaded_results::clear_comparison_mark(state, repo_id)
        }
        Msg::EnsureSidebarData { repo_id, request } => {
            loaded_results::ensure_sidebar_data(state, repo_id, request)
        }
        Msg::LoadStashes { repo_id } => loaded_results::load_stashes(state, repo_id),
        Msg::LoadConflictFile {
            repo_id,
            path,
            mode,
        } => loaded_results::load_conflict_file(state, repo_id, path, mode),
        Msg::LoadReflog { repo_id } => loaded_results::load_reflog(state, repo_id),
        Msg::LoadHoverCommitMessage { repo_id, commit_id } => {
            loaded_results::load_hover_commit_message(state, repo_id, commit_id)
        }
        Msg::LoadRecentCommitMessages { repo_id, limit } => {
            loaded_results::load_recent_commit_messages(state, repo_id, limit)
        }
        Msg::SearchCommits { repo_id, query } => {
            loaded_results::search_commits(state, repo_id, query)
        }
        Msg::LoadAiCommitContext { repo_id } => {
            loaded_results::load_ai_commit_context(state, repo_id)
        }
        Msg::LoadFileHistory {
            repo_id,
            path,
            limit,
        } => loaded_results::load_file_history(state, repo_id, path, limit),
        Msg::LoadBlame {
            repo_id,
            path,
            source,
        } => loaded_results::load_blame(state, repo_id, path, source),
        Msg::LoadWorktrees { repo_id } => loaded_results::load_worktrees(state, repo_id),
        Msg::LoadWorktreeDirty { repo_id } => loaded_results::load_worktree_dirty(state, repo_id),
        Msg::SelectWorktreeUncommitted { repo_id, path } => {
            loaded_results::select_worktree_uncommitted(state, repo_id, path)
        }
        Msg::SelectWorkingTreeSummary { repo_id } => {
            loaded_results::select_working_tree_summary(state, repo_id)
        }
        Msg::LoadRefMetadata { repo_id } => loaded_results::load_ref_metadata(state, repo_id),
        Msg::LoadSubmodules { repo_id } => loaded_results::load_submodules(state, repo_id),
        // The GitHub API call itself is spawned by the UI (the store has no
        // HTTP); this arm only flips the loadable so the section renders its
        // loading state and re-loads are not double-spawned.
        Msg::LoadPullRequests { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_pull_requests(Loadable::Loading);
                worktree_core::applog_info!(
                    "pull-request load state: loading (repo_id={})",
                    repo_id.0
                );
            }
            Vec::new()
        }
        Msg::LoadTags { repo_id } => loaded_results::load_tags(state, repo_id),
        Msg::LoadRemoteTags { repo_id } => loaded_results::load_remote_tags(state, repo_id),
        Msg::RefreshBranches { repo_id } => loaded_results::refresh_branches(state, repo_id),
        Msg::LoadFileBrowser { repo_id, source } => {
            loaded_results::load_file_browser(state, repo_id, source)
        }
        Msg::ToggleFileBrowserDir { repo_id, path } => {
            loaded_results::toggle_file_browser_dir(state, repo_id, path)
        }
        Msg::SetFileBrowserDirExpandedRecursive {
            repo_id,
            path,
            expanded,
        } => {
            loaded_results::set_file_browser_dir_expanded_recursive(state, repo_id, path, expanded)
        }
        Msg::SetFileBrowserSearch { repo_id, query } => {
            loaded_results::set_file_browser_search(state, repo_id, query)
        }
        Msg::RevealFileBrowserPath { repo_id, path } => {
            loaded_results::reveal_file_browser_path(state, repo_id, path)
        }
        Msg::SetFileBrowserSource { repo_id, source } => {
            loaded_results::set_file_browser_source(state, repo_id, source)
        }
        Msg::BrowseRepositoryAtCommit { repo_id, commit_id } => {
            loaded_results::browse_repository_at_commit(state, repo_id, commit_id)
        }
        Msg::RevealCommit { repo_id, reference } => {
            loaded_results::reveal_commit(state, repo_id, reference)
        }
        Msg::FinishCommitReveal { repo_id } => loaded_results::finish_commit_reveal(state, repo_id),
        Msg::ResetBrowseToLive { repo_id } => loaded_results::reset_browse_to_live(state, repo_id),
        Msg::SetSidebarMode { mode } => loaded_results::set_sidebar_mode(state, mode),
        Msg::SetCoverage { repo_id, report } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_coverage(Some(report));
            }
            Vec::new()
        }
        Msg::ClearCoverage { repo_id } => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_coverage(None);
            }
            Vec::new()
        }
        Msg::PrepareSquash { repo_id } => loaded_results::prepare_squash(state, repo_id),
        Msg::LoadRepoStatistics { repo_id } => loaded_results::load_repo_statistics(state, repo_id),
        Msg::Internal(crate::msg::InternalMsg::BranchesLoaded { repo_id, result }) => {
            loaded_results::branches_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemotesLoaded { repo_id, result }) => {
            loaded_results::remotes_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemoteBranchesLoaded { repo_id, result }) => {
            loaded_results::remote_branches_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded { repo_id, result }) => {
            loaded_results::worktree_status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded { repo_id, result }) => {
            loaded_results::staged_status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StatusLoaded { repo_id, result }) => {
            loaded_results::status_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result,
        }) => loaded_results::status_for_paths_loaded(state, repo_id, paths, result),
        Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded { repo_id, result }) => {
            loaded_results::head_branch_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::UpstreamDivergenceLoaded { repo_id, result }) => {
            loaded_results::upstream_divergence_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::TagsLoaded { repo_id, result }) => {
            loaded_results::tags_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RemoteTagsLoaded { repo_id, result }) => {
            loaded_results::remote_tags_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::StashesLoaded { repo_id, result }) => {
            loaded_results::stashes_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::AssumeUnchangedListLoaded { repo_id, result }) => {
            loaded_results::assume_unchanged_list_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RepoStatisticsLoaded { repo_id, result }) => {
            loaded_results::repo_statistics_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::ReflogLoaded { repo_id, result }) => {
            loaded_results::reflog_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::HoverCommitMessageLoaded {
            repo_id,
            commit_id,
            result,
        }) => loaded_results::hover_commit_message_loaded(state, repo_id, commit_id, result),
        Msg::Internal(crate::msg::InternalMsg::FileHistoryLoaded {
            repo_id,
            path,
            result,
        }) => loaded_results::file_history_loaded(state, repo_id, path, result),
        Msg::Internal(crate::msg::InternalMsg::AuthorEmailsLoaded { repo_id, result }) => {
            loaded_results::author_emails_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::BlameLoaded {
            repo_id,
            path,
            source,
            result,
        }) => loaded_results::blame_loaded(state, repo_id, path, source, result),
        Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
            repo_id,
            path,
            result,
            conflict_session,
        }) => loaded_results::conflict_file_loaded(state, repo_id, path, *result, conflict_session),
        Msg::Internal(crate::msg::InternalMsg::WorktreesLoaded { repo_id, result }) => {
            loaded_results::worktrees_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::WorktreeDirtyLoaded { repo_id, result }) => {
            loaded_results::worktree_dirty_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::RefMetadataLoaded { repo_id, result }) => {
            loaded_results::ref_metadata_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::SubmodulesLoaded { repo_id, result }) => {
            loaded_results::submodules_loaded(state, repo_id, result)
        }
        Msg::Internal(crate::msg::InternalMsg::FileBrowserLoaded {
            repo_id,
            source,
            result,
        }) => loaded_results::file_browser_loaded(state, repo_id, source, result),
        Msg::Internal(crate::msg::InternalMsg::CommitDetailsLoaded {
            repo_id,
            commit_id,
            result,
        }) => loaded_results::commit_details_loaded(state, repo_id, commit_id, result),
        Msg::Internal(crate::msg::InternalMsg::CommitRevealResolved {
            repo_id,
            reference,
            result,
        }) => loaded_results::commit_reveal_resolved(state, repo_id, reference, result),
        Msg::Internal(crate::msg::InternalMsg::RangeFilesLoaded {
            repo_id,
            from,
            to,
            request,
            result,
        }) => loaded_results::range_files_loaded(state, repo_id, from, to, request, result),
        Msg::Internal(crate::msg::InternalMsg::SquashMessagePreviewLoaded {
            repo_id,
            oldest,
            head,
            result,
        }) => loaded_results::squash_message_preview_loaded(state, repo_id, oldest, head, result),
        Msg::Internal(crate::msg::InternalMsg::SquashRebaseSetupLoaded {
            repo_id,
            base,
            actual_head,
            selected_ids,
            reword_id,
            message,
            count,
            result,
        }) => loaded_results::squash_rebase_setup_loaded(
            state,
            repo_id,
            base,
            actual_head,
            selected_ids,
            reword_id,
            message,
            count,
            result,
        ),
        Msg::Internal(crate::msg::InternalMsg::AutosquashSetupLoaded {
            repo_id,
            base,
            result,
        }) => loaded_results::autosquash_rebase_setup_loaded(state, repo_id, base, result),
        Msg::Internal(crate::msg::InternalMsg::MergePreviewLoaded {
            repo_id,
            head,
            result,
        }) => loaded_results::merge_preview_loaded(state, repo_id, head, result),
        Msg::Internal(crate::msg::InternalMsg::RecentCommitMessagesLoaded {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::recent_commit_messages_loaded(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::CommitsSearched {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::commits_searched(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::AiCommitContextLoaded {
            repo_id,
            request_rev,
            result,
        }) => loaded_results::ai_commit_context_loaded(state, repo_id, request_rev, result),
        Msg::Internal(crate::msg::InternalMsg::PullRequestsLoaded { repo_id, result }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                match &result {
                    Ok(pull_requests) => worktree_core::applog_info!(
                        "pull-request load state: {} pull requests stored (repo_id={})",
                        pull_requests.len(),
                        repo_id.0
                    ),
                    Err(error) => worktree_core::applog_warn!(
                        "pull-request load state: error stored: {error} (repo_id={})",
                        repo_id.0
                    ),
                }
                repo_state.set_pull_requests(match result {
                    Ok(pull_requests) => Loadable::Ready(pull_requests),
                    Err(error) => Loadable::Error(error.to_string()),
                });
            }
            Vec::new()
        }
        Msg::Internal(crate::msg::InternalMsg::PullRequestChecksLoaded {
            repo_id,
            number,
            checks,
        }) => {
            if let Some(repo_state) = state.repos.iter_mut().find(|r| r.id == repo_id) {
                repo_state.set_pull_request_checks(number, checks);
            }
            Vec::new()
        }
        other => return ReduceOutcome::NotHandled(other),
    };
    ReduceOutcome::Handled(effects)
}

#[cfg(test)]
mod file_browser_filter_tests;

#[cfg(test)]
mod tests;
