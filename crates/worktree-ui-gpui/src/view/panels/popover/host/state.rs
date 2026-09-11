//! Per-picker state: the fields each popover keeps between renders.

use super::super::*;
use super::kinds::{
    AutosquashMode, PopoverKind, RemotePopoverKind, RepoPopoverKind, SubmodulePopoverKind,
    WorktreePopoverKind,
};

impl AutosquashMode {
    pub(in crate::view) fn label(self) -> &'static str {
        match self {
            AutosquashMode::ToTop => crate::i18n::tr_str("ui.label.autosquash.to_top_commit"),
            AutosquashMode::Neighbor => {
                crate::i18n::tr_str("ui.label.autosquash.neighboring_commit")
            }
            AutosquashMode::ToBottom => crate::i18n::tr_str("ui.label.autosquash.to_bottom_commit"),
        }
    }
}

impl PopoverKind {
    pub(in crate::view) fn remote(repo_id: RepoId, kind: RemotePopoverKind) -> Self {
        Self::Repo {
            repo_id,
            kind: RepoPopoverKind::Remote(kind),
        }
    }

    pub(in crate::view) fn worktree(repo_id: RepoId, kind: WorktreePopoverKind) -> Self {
        Self::Repo {
            repo_id,
            kind: RepoPopoverKind::Worktree(kind),
        }
    }

    pub(in crate::view) fn submodule(repo_id: RepoId, kind: SubmodulePopoverKind) -> Self {
        Self::Repo {
            repo_id,
            kind: RepoPopoverKind::Submodule(kind),
        }
    }
}

/// Per-popover state for the context menu domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct ContextMenuState {
    pub(in crate::view::panels::popover) context_menu_selected_ix: Option<usize>,
    /// Submenu groups currently expanded in the open context menu, keyed by
    /// the group's stable id. Selection indices shift when a group opens or
    /// closes, so both are cleared together.
    pub(in crate::view::panels::popover) context_menu_open_submenus: FxHashSet<SharedString>,
}

/// Per-popover state for the repo picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct RepoPickerState {
    pub(in crate::view::panels::popover) repo_picker_selected_index: Option<usize>,
    /// Session recent repositories snapshotted when a repository picker opens,
    /// so the list can't shift under the user mid-interaction.
    pub(in crate::view::panels::popover) cached_recent_repos: Vec<std::path::PathBuf>,
    /// Session pins snapshotted alongside `cached_recent_repos`. Held apart from
    /// the recents so a pin outlives the recents cap.
    pub(in crate::view::panels::popover) cached_pinned_repos: Vec<std::path::PathBuf>,
    /// Storage keys of the repository picker sections the user folded away.
    pub(in crate::view::panels::popover) cached_collapsed_picker_sections:
        std::collections::BTreeSet<String>,
    pub(in crate::view::panels::popover) repo_picker_sort: repo_picker::RepoPickerSort,
    pub(in crate::view::panels::popover) repo_picker_sort_menu_open: bool,

    pub(in crate::view::panels::popover) repo_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) repo_picker_rows_cache:
        rows_cache::RowsCache<repo_picker::RepoPickerEntry>,
    pub(in crate::view::panels::popover) _repo_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the branch picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct BranchPickerState {
    pub(in crate::view::panels::popover) branch_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) branch_picker_search_input:
        Option<Entity<components::TextInput>>,
    /// Row models for the pickers that build one row per repository, ref or
    /// worktree, rebuilt only when the data behind them changes rather than on
    /// every frame. See [`rows_cache`] — a hover moving between rows re-renders
    /// this whole view.
    pub(in crate::view::panels::popover) branch_picker_rows_cache:
        rows_cache::RowsCache<branch_picker::BranchPickerNavTarget>,
    pub(in crate::view::panels::popover) branch_ref_rows_cache: rows_cache::RowsCache<String>,
    pub(in crate::view::panels::popover) _branch_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the worktree picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct WorktreePickerState {
    pub(in crate::view::panels::popover) worktree_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) worktree_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) worktree_picker_rows_cache:
        rows_cache::RowsCache<std::path::PathBuf>,
    pub(in crate::view::panels::popover) _worktree_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the workspace picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct WorkspacePickerState {
    pub(in crate::view::panels::popover) workspace_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) workspace_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) workspace_picker_rows_cache:
        rows_cache::RowsCache<workspace_picker::WorkspaceRow>,
    pub(in crate::view::panels::popover) _workspace_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the upstream picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct UpstreamPickerState {
    pub(in crate::view::panels::popover) upstream_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) upstream_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) upstream_picker_rows_cache:
        rows_cache::RowsCache<upstream_picker::UpstreamRow>,
    pub(in crate::view::panels::popover) _upstream_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the submodule picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct SubmodulePickerState {
    pub(in crate::view::panels::popover) submodule_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) submodule_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) submodule_picker_rows_cache:
        rows_cache::RowsCache<std::path::PathBuf>,
    pub(in crate::view::panels::popover) _submodule_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the remote picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct RemotePickerState {
    pub(in crate::view::panels::popover) remote_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) remote_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) remote_picker_rows_cache:
        rows_cache::RowsCache<remote_picker::RemotePickerRow>,
    pub(in crate::view::panels::popover) _remote_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the tag picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct TagPickerState {
    pub(in crate::view::panels::popover) tag_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) tag_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) tag_picker_rows_cache: rows_cache::RowsCache<String>,
    pub(in crate::view::panels::popover) _tag_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the commit search picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct CommitSearchPickerState {
    pub(in crate::view::panels::popover) commit_search_picker_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) commit_search_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) commit_search_picker_rows_cache:
        rows_cache::RowsCache<commit_search_picker::CommitSearchPickerRow>,
    pub(in crate::view::panels::popover) _commit_search_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the stash picker domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct StashPickerState {
    pub(in crate::view::panels::popover) stash_picker_prompt_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) stash_picker_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) stash_picker_rows_cache:
        rows_cache::RowsCache<stash_picker_prompt::StashRow>,
    pub(in crate::view::panels::popover) _stash_picker_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the file history domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct FileHistoryState {
    pub(in crate::view::panels::popover) file_history_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) file_history_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) file_history_rows_cache: rows_cache::RowsCache<CommitId>,
    pub(in crate::view::panels::popover) _file_history_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the history author filter domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct HistoryAuthorFilterState {
    pub(in crate::view::panels::popover) history_author_filter_selected_index: Option<usize>,
    pub(in crate::view::panels::popover) history_author_filter_search_input:
        Option<Entity<components::TextInput>>,
    /// Author suggestions for the history author filter, keyed by repository and
    /// the log revision they were collected from. Collecting them walks the
    /// whole accumulated log, and the popover re-renders on every mouse move
    /// over it, so the result has to outlive the frame. See
    /// [`author_filter::suggestions`].
    pub(in crate::view::panels::popover) history_author_suggestions:
        Option<(RepoId, u64, std::sync::Arc<[SharedString]>)>,
    pub(in crate::view::panels::popover) _history_author_filter_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the history ref filter domain, grouped off the host's top level.
#[derive(Default)]
pub(in crate::view::panels::popover) struct HistoryRefFilterState {
    /// The ref-filter popover's plain query box: narrows the three ref
    /// sections live, with no Enter semantics — clicks still toggle filters.
    pub(in crate::view::panels::popover) history_ref_filter_search_input:
        Option<Entity<components::TextInput>>,
    pub(in crate::view::panels::popover) _history_ref_filter_search_input_subscription:
        Option<gpui::Subscription>,
}

/// Per-popover state for the clone repo domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct CloneRepoState {
    pub(in crate::view::panels::popover) clone_repo_url_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) clone_repo_parent_dir_input: Entity<components::TextInput>,
    /// Optional per-clone SSH key path: rides the clone as
    /// `core.sshCommand` and persists onto the new `origin` remote.
    pub(in crate::view::panels::popover) clone_ssh_key_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) clone_repo_focus: DialogFocus,
    pub(in crate::view::panels::popover) clone_repo_browse_focus_handle: FocusHandle,
}

/// Per-popover state for the repo settings domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct RepoSettingsState {
    /// Repo-settings prompt state: two inputs plus the tri-state signing
    /// override and the config snapshot read on open.
    pub(in crate::view::panels::popover) repo_settings_user_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) repo_settings_email_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) repo_settings_sign_commits: Option<bool>,
    pub(in crate::view::panels::popover) repo_settings_error: Option<SharedString>,
    /// The local/global snapshot consumed by the next panel render; taken by
    /// the panel so it is re-read on every open, never between renders.
    pub(in crate::view::panels::popover) repo_settings_current:
        Option<crate::view::panels::popover::repo_settings::RepoSettingsCurrent>,
    pub(in crate::view::panels::popover) repo_settings_focus: DialogFocus,
    /// Test seam: how many times the repo-settings config snapshot was read
    /// from disk. An open and an apply each read once; renders must read
    /// never — a render-time read is five git spawns per keystroke.
    #[cfg(test)]
    pub(in crate::view::panels::popover) repo_settings_test_loads: usize,
}

/// Per-popover state for the rebase onto domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct RebaseOntoState {
    pub(in crate::view::panels::popover) rebase_onto_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) rebase_onto_submit_focus_handle: FocusHandle,
}

/// Per-popover state for the create tag domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct CreateTagState {
    pub(in crate::view::panels::popover) create_tag_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) create_tag_message_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) create_tag_message_scroll: ScrollHandle,
    pub(in crate::view::panels::popover) create_tag_annotated: bool,
    pub(in crate::view::panels::popover) create_tag_focus: DialogFocus,
    pub(in crate::view::panels::popover) create_tag_annotated_focus_handle: FocusHandle,
}

/// Per-popover state for the gitignore domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct GitignoreState {
    /// One `.gitignore` line per row. Multiline so a multi-file selection and a
    /// single file share one code path, and so the field reads like the file it
    /// is about to become.
    pub(in crate::view::panels::popover) gitignore_patterns_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) gitignore_patterns_scroll: ScrollHandle,
    /// Which scope's patterns the input was last prefilled with. Only a prefill
    /// shortcut — submit reads the input, never this.
    pub(in crate::view::panels::popover) gitignore_scope: worktree_core::gitignore::GitignoreScope,
    /// Computed once when the dialog opens, so a status refresh arriving
    /// mid-edit cannot change the offered scopes under the user.
    pub(in crate::view::panels::popover) gitignore_suggestions:
        Option<worktree_core::gitignore::GitignoreSuggestions>,
    /// The paths the dialog is about, for the "Ignore <file>" body text.
    pub(in crate::view::panels::popover) gitignore_paths: Vec<std::path::PathBuf>,
}

/// Per-popover state for the squash domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct SquashState {
    pub(in crate::view::panels::popover) squash_message_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) squash_description_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) squash_description_scroll: ScrollHandle,
    /// The `(oldest, head)` range the squash prompt's message inputs were last
    /// prefilled for. Prevents re-prefilling the same range (so a user who
    /// clears the fields keeps them cleared) and, together with the empty-input
    /// check, prevents clobbering text the user typed while the preview loaded.
    pub(in crate::view::panels::popover) squash_prompt_prefilled_range: Option<(
        worktree_core::domain::CommitId,
        worktree_core::domain::CommitId,
    )>,
    pub(in crate::view::panels::popover) squash_cancel_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) squash_submit_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) _squash_message_input_subscription: gpui::Subscription,
    pub(in crate::view::panels::popover) _squash_description_input_subscription: gpui::Subscription,
}

/// Per-popover state for the remote prompts domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct RemotePromptsState {
    pub(in crate::view::panels::popover) remote_name_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) remote_url_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) remote_url_edit_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) remote_ssh_key_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) remote_add_focus: DialogFocus,
    pub(in crate::view::panels::popover) remote_edit_focus: DialogFocus,
    pub(in crate::view::panels::popover) remote_ssh_key_focus: DialogFocus,
    pub(in crate::view::panels::popover) remote_ssh_key_clear_focus: FocusHandle,
}

/// Per-popover state for the create branch domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct CreateBranchState {
    pub(in crate::view::panels::popover) create_branch_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) create_branch_checkout_enabled: bool,
    pub(in crate::view::panels::popover) create_branch_source_target: String,
    pub(in crate::view::panels::popover) create_branch_from_ref_checkout_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) create_branch_from_ref_focus: DialogFocus,
}

/// Per-popover state for the stash domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct StashState {
    pub(in crate::view::panels::popover) stash_message_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) stash_focus: DialogFocus,
    /// Stash prompt option rows. Defaults re-applied every time the prompt
    /// opens, so an earlier stash never leak its options into the next one.
    pub(in crate::view::panels::popover) stash_include_untracked: bool,
    pub(in crate::view::panels::popover) stash_keep_index: bool,
    pub(in crate::view::panels::popover) stash_include_untracked_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) stash_keep_index_focus_handle: FocusHandle,
}

/// Per-popover state for the mr push domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct MrPushState {
    /// Merge-request push prompt state. Like the stash options, defaults are
    /// re-applied every time the prompt opens; `merge_request.create` itself
    /// is implicit — the dialog exists to create one.
    pub(in crate::view::panels::popover) mr_push_target_input: Entity<components::TextInput>,
    /// The AI-generated (or hand-written) MR description. Prepare-and-copy
    /// only: GitLab push options carry no reliable multiline description.
    pub(in crate::view::panels::popover) mr_push_description_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) mr_push_description_generating: bool,
    pub(in crate::view::panels::popover) mr_push_description_error: Option<SharedString>,
    pub(in crate::view::panels::popover) mr_push_merge_when_pipeline_succeeds: bool,
    pub(in crate::view::panels::popover) mr_push_remove_source_branch: bool,
    pub(in crate::view::panels::popover) mr_push_push_to_mr_branch: bool,
    pub(in crate::view::panels::popover) mr_push_pipeline_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) mr_push_remove_source_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) mr_push_mr_branch_focus_handle: FocusHandle,
}

/// Per-popover state for the commit prompt domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct CommitPromptState {
    pub(in crate::view::panels::popover) commit_prompt_message_drafts:
        FxHashMap<RepoId, SharedString>,
    pub(in crate::view::panels::popover) commit_prompt_message_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) commit_prompt_message_scroll: ScrollHandle,
    pub(in crate::view::panels::popover) commit_prompt_focus: DialogFocus,
}

/// Per-popover state for the worktree add domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct WorktreeAddState {
    pub(in crate::view::panels::popover) worktree_ref_source_target: String,
    pub(in crate::view::panels::popover) suppress_worktree_submit_after_ref_enter: bool,
    /// Path/reference the workspace badge's create row hands to the Add-worktree
    /// dialog. Consumed (and cleared) when that dialog opens, so a later
    /// open from elsewhere still starts blank.
    pub(in crate::view::panels::popover) pending_worktree_add_prefill: Option<(String, String)>,
    pub(in crate::view::panels::popover) worktree_path_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) worktree_ref_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) worktree_focus: DialogFocus,
    pub(in crate::view::panels::popover) worktree_browse_focus_handle: FocusHandle,
}

/// Per-popover state for the submodule add domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct SubmoduleAddState {
    pub(in crate::view::panels::popover) submodule_url_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) submodule_path_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) submodule_ref_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) submodule_branch_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) submodule_name_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) submodule_add_advanced_expanded: bool,
    pub(in crate::view::panels::popover) submodule_force_enabled: bool,
    pub(in crate::view::panels::popover) submodule_focus: DialogFocus,
    pub(in crate::view::panels::popover) submodule_advanced_focus_handle: FocusHandle,
    pub(in crate::view::panels::popover) submodule_force_focus_handle: FocusHandle,
}

/// Per-popover state for the push upstream domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct PushUpstreamState {
    pub(in crate::view::panels::popover) push_upstream_branch_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) push_upstream_focus: DialogFocus,
}

/// Per-popover state for the rebase reword domain, grouped off the host's top level.
pub(in crate::view::panels::popover) struct RebaseRewordState {
    pub(in crate::view::panels::popover) rebase_reword_input: Entity<components::TextInput>,
    pub(in crate::view::panels::popover) rebase_reword_description_input:
        Entity<components::TextInput>,
    pub(in crate::view::panels::popover) rebase_reword_description_scroll: ScrollHandle,
}
