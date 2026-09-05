use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum BranchPickerPurpose {
    Checkout,
    Delete,
    Merge,
    RebaseOnto,
}

/// What the palette's remote picker does with the remote (or remote branch)
/// row the user activates. Decides both the row source — remotes for
/// `RemoveRemote`/`EditUrl`, `remote/branch` refs for `DeleteBranch` — and the
/// popover or menu activation opens.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum RemotePickerPurpose {
    DeleteBranch,
    RemoveRemote,
    EditUrl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum StashPickerPurpose {
    Pop,
    Apply,
    Drop,
    Branch,
}

/// Auto-squash strategy: which commit in each identical-message group survives,
/// the others being folded (fixup) into it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum AutosquashMode {
    /// Fold each duplicate group into its newest (top) commit.
    ToTop,
    /// Only merge duplicates that are already adjacent in the list.
    Neighbor,
    /// Fold each duplicate group into its oldest (bottom) commit.
    ToBottom,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum PopoverKind {
    RepoPicker,
    BranchPicker {
        purpose: BranchPickerPurpose,
    },
    /// Pick (or clear) the remote-tracking branch a local branch follows.
    /// Opened from the local branch menu's "Change tracking upstream…".
    UpstreamPicker {
        repo_id: RepoId,
        branch: String,
    },
    CreateBranchFromRefPrompt {
        repo_id: RepoId,
        target: String,
        source_selectable: bool,
        /// Text the name field opens with, so "create a branch in this group"
        /// can hand over `feat/` and leave the user typing only the leaf.
        ///
        /// Carried on the kind rather than kept beside it on the host because
        /// two prompts differing only by prefix are different popovers; sharing
        /// a value would make them compare equal.
        name_prefix: String,
    },
    RenameBranchPrompt {
        repo_id: RepoId,
        name: String,
        is_current_branch: bool,
    },
    CheckoutRemoteBranchPrompt {
        repo_id: RepoId,
        remote: String,
        branch: String,
    },
    CommitPrompt {
        repo_id: RepoId,
    },
    /// Stash prompt. `paths` restricts the stash to a selection; empty stashes
    /// the whole worktree.
    StashPrompt {
        paths: Vec<std::path::PathBuf>,
    },
    StashDropConfirm {
        repo_id: RepoId,
        index: usize,
        message: String,
    },
    StashPickerPrompt {
        repo_id: RepoId,
        purpose: StashPickerPurpose,
    },
    StashMenu {
        repo_id: RepoId,
        index: usize,
        message: String,
    },
    /// Asks for a branch name, then runs `git stash branch` on the stash.
    StashBranchPrompt {
        repo_id: RepoId,
        index: usize,
    },
    /// Lists the paths marked assume-unchanged in the index and offers a
    /// per-row restore. The list itself is loaded on open (and reloaded after
    /// every toggle) through `Msg::LoadAssumeUnchanged`.
    AssumeUnchangedManager {
        repo_id: RepoId,
    },
    /// Commit statistics over the current week / month / year: weekday /
    /// day-of-month / month bars plus contributor rankings. The commit list
    /// is loaded on open through `Msg::LoadRepoStatistics` and bucketed in
    /// the user's display timezone at render time.
    Statistics {
        repo_id: RepoId,
    },
    /// Undoes the repository's last operation. An in-progress merge or rebase
    /// is aborted; a completed reset / merge / pull / rebase / commit is
    /// reversed by resetting the branch back to the reflog-recorded
    /// position, with the reset mode (and its safety note) chosen in the
    /// preview. The reflog is loaded on open when missing.
    UndoLastActionPrompt {
        repo_id: RepoId,
    },
    CloneRepo,
    ResetPrompt {
        repo_id: RepoId,
        target: String,
        mode: ResetMode,
    },
    SquashPrompt {
        repo_id: RepoId,
    },
    CreateTagPrompt {
        repo_id: RepoId,
        target: String,
    },
    Repo {
        repo_id: RepoId,
        kind: RepoPopoverKind,
    },
    FileHistory {
        repo_id: RepoId,
        path: std::path::PathBuf,
        /// The path is a directory: the popover lists the folder's history and
        /// rows open the commit's changes *under* the folder instead of a file
        /// version.
        is_dir: bool,
    },
    /// Right-click menu on a reflog panel row: the same reset actions the
    /// history log's commit context menu offers, targeting the commit the
    /// clicked reflog entry points at.
    ReflogEntryMenu {
        repo_id: RepoId,
        target: CommitId,
        selector: SharedString,
    },
    PushSetUpstreamPrompt {
        repo_id: RepoId,
        remote: String,
    },
    ForcePushConfirm {
        repo_id: RepoId,
    },
    /// Push HEAD carrying `git push -o merge_request.*` options so GitLab
    /// opens the merge request from the push itself. The four options
    /// (target branch, merge on green pipeline, remove source branch, push
    /// to `MR/<branch>`) mirror the C# client's push dialog.
    MergeRequestPushPrompt {
        repo_id: RepoId,
    },
    CherryPickCommitConfirm {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    MergeCommitConfirm {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    MergeAbortConfirm {
        repo_id: RepoId,
    },
    ForceDeleteBranchConfirm {
        repo_id: RepoId,
        name: String,
    },
    ForceRemoveWorktreeConfirm {
        repo_id: RepoId,
        path: std::path::PathBuf,
        branch: Option<String>,
    },
    DiscardChangesConfirm {
        repo_id: RepoId,
        area: DiffArea,
        path: Option<std::path::PathBuf>,
    },
    /// Palette "Discard All Changes": every path with staged or unstaged
    /// modifications in one confirm, unlike [`PopoverKind::DiscardChangesConfirm`]
    /// which resolves its paths from the status selection at open time.
    DiscardAllConfirm {
        repo_id: RepoId,
    },
    /// Palette remote commands. One picker, three destinations: activating a
    /// row opens the matching existing confirm or menu — the
    /// [`PopoverKind::Repo`] remote kinds carry the rest of each flow.
    RemotePicker {
        repo_id: RepoId,
        purpose: RemotePickerPurpose,
    },
    /// Palette "Delete Tag": the tag list; activation deletes without a
    /// confirm, mirroring the tag context menu.
    DeleteTagPicker {
        repo_id: RepoId,
    },
    /// Palette "Search Commits" (and the Ctrl+F fallback when no diff is
    /// open): a two-tier picker — matches from the already-loaded log page
    /// appear instantly, and a trailing action row runs the cross-history
    /// `git log --all` search whose results join the list when they land.
    /// Activation reveals the commit in the history view.
    CommitSearchPicker {
        repo_id: RepoId,
    },
    /// Add the clicked status path — or its folder, or its extension — to the
    /// repo-root `.gitignore`.
    ///
    /// `path` is the clicked row only. The multi-selection it may stand for is
    /// re-derived when the dialog opens and consumed only on submit, so
    /// cancelling leaves the selection intact.
    AddToGitignorePrompt {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    /// Staging would mark files resolved that still contain conflict markers.
    /// `paths` is the stage request as issued (empty means everything);
    /// `unresolved` is what the user is being warned about.
    ///
    /// `clear_selection` says whether `paths` came out of the status row
    /// selection. The selection is deliberately left intact while this dialog is
    /// up — cancelling must not cost it — so going ahead is what consumes it.
    StageConflictMarkersConfirm {
        repo_id: RepoId,
        paths: Vec<std::path::PathBuf>,
        unresolved: Vec<std::path::PathBuf>,
        clear_selection: bool,
    },
    PullReconcilePrompt {
        repo_id: RepoId,
    },
    PullPicker,
    PushPicker,
    CommitOptionsMenu {
        repo_id: RepoId,
    },
    PreviousCommitMessagesMenu {
        repo_id: RepoId,
    },
    RepoTabMenu {
        repo_id: RepoId,
    },
    AppMenu,
    AddRepoMenu,
    TerminalShutdownConfirm(TerminalShutdownPrompt),
    UnsavedFileEditsConfirm(UnsavedFileEditsPrompt),
    TerminalMenu {
        repo_id: RepoId,
        context: TerminalMenuContext,
    },
    DiffActionMenu,
    MergetoolSettingsMenu,
    DiffHunkMenu {
        repo_id: RepoId,
        src_ix: usize,
    },
    /// The diff view's "Explain this change" answer for one hunk, held open
    /// while the AI request runs and once it lands.
    HunkExplanation {
        repo_id: RepoId,
        src_ix: usize,
    },
    /// Actions for a web link clicked in the rendered markdown preview or in a
    /// commit message.
    WebLinkMenu {
        url: SharedString,
    },
    /// Actions for a commit id clicked in a commit message or a SHA field.
    CommitShaLinkMenu {
        repo_id: RepoId,
        commit_id: CommitId,
        /// A commit's own SHA field cannot navigate to itself.
        allow_navigate: bool,
    },
    DiffEditorMenu {
        repo_id: RepoId,
        area: DiffArea,
        path: Option<std::path::PathBuf>,
        hunk_patch: Option<String>,
        hunks_count: usize,
        lines_patch: Option<String>,
        discard_lines_patch: Option<String>,
        lines_count: usize,
        copy_text: Option<String>,
        copy_target: Option<(usize, DiffTextRegion)>,
    },
    ConflictResolverInputRowMenu {
        line_label: SharedString,
        line_target: ResolverPickTarget,
        chunk_label: SharedString,
        chunk_target: ResolverPickTarget,
    },
    ConflictResolverChunkMenu {
        conflict_ix: usize,
        has_base: bool,
        is_three_way: bool,
        selected_choices: Vec<conflict_resolver::ConflictChoice>,
        output_line_ix: Option<usize>,
        /// section 30 split: row count of a valid split selection in this block, or
        /// `None` when there is no splittable selection (hides the entry).
        split_selection_rows: Option<usize>,
        /// Revision-bound target for joining this chunk with its previous
        /// neighbour, when one exists.
        join_previous_region: Option<ConflictResolverJoinTarget>,
        /// Revision-bound target for joining this chunk with its next
        /// neighbour, when one exists.
        join_next_region: Option<ConflictResolverJoinTarget>,
        /// kdiff3 manual diff help: how many source columns carry a pending
        /// alignment mark. Zero hides the "align" entry.
        alignment_marked_columns: usize,
        /// Whether this file already has pinned alignments to clear.
        has_manual_alignments: bool,
        /// Whether the merged output is the untouched worktree payload rather
        /// than our projection. Every resolution action refuses to run in that
        /// state, so the entries grey out instead of silently doing nothing —
        /// the toolbar already gates the same picks this way.
        output_is_protected: bool,
    },
    ConflictResolverOutputMenu {
        cursor_line: usize,
        selected_text: Option<String>,
        has_source_a: bool,
        has_source_b: bool,
        has_source_c: bool,
        is_three_way: bool,
    },
    CommitMenu {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    StatusFileMenu {
        repo_id: RepoId,
        area: DiffArea,
        path: std::path::PathBuf,
    },
    BranchMenu {
        repo_id: RepoId,
        section: BranchSection,
        name: String,
    },
    BranchSectionMenu {
        repo_id: RepoId,
        section: BranchSection,
    },
    /// Menu for a `/`-prefix group row in the branch tree (`feat/`).
    BranchGroupMenu {
        repo_id: RepoId,
        section: BranchSection,
        /// The owning remote for a remote group; `None` for a local one.
        remote: Option<String>,
        /// Full slash path with no trailing separator (`feat`, `feat/sub`).
        path: String,
    },
    /// Menu for the "Pinned Local/Remote Branches" header row.
    PinnedSectionMenu {
        repo_id: RepoId,
        section: BranchSection,
    },
    /// Confirms deleting every branch in a group. Carries the resolved member
    /// list so the dialog names what it is about to remove, rather than
    /// re-deriving it and risking a different answer than the menu showed.
    DeleteBranchesConfirm {
        repo_id: RepoId,
        section: BranchSection,
        remote: Option<String>,
        group_label: String,
        names: Vec<String>,
    },
    CommitFileMenu {
        repo_id: RepoId,
        commit_id: CommitId,
        path: std::path::PathBuf,
    },
    FileBrowserFileMenu {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    FileBrowserFolderMenu {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    BrowseHistoryMenu {
        repo_id: RepoId,
    },
    SubmoduleInnerDiffMenu {
        repo_id: RepoId,
        submodule_repo_path: std::path::PathBuf,
        target: DiffTarget,
    },
    #[allow(dead_code)]
    TagMenu {
        repo_id: RepoId,
        commit_id: CommitId,
    },
    TagRefMenu {
        repo_id: RepoId,
        commit_id: CommitId,
        name: String,
    },
    PullRequestMenu {
        repo_id: RepoId,
        number: u64,
    },
    /// The agent workbench's sessions roster: one card for the running
    /// session (view changes / stop) plus start entries for each agent.
    AgentSessions {
        repo_id: RepoId,
    },
    /// Per-repository settings (local git config overrides): user name,
    /// email, and commit signing. Empty fields inherit the global config.
    RepoSettingsPrompt {
        repo_id: RepoId,
    },
    HistoryBranchFilter {
        repo_id: RepoId,
    },
    HistoryAuthorFilter {
        repo_id: RepoId,
    },
    HistoryRefFilter {
        repo_id: RepoId,
    },
    DiffContentModeSettings,
    ChangeTrackingSettings,
    UiScalePicker,
    RebaseOntoConfirm {
        repo_id: RepoId,
        onto: String,
    },
    RebaseReword {
        ix: usize,
        original_action: InteractiveRebaseAction,
        original_message: String,
    },
    InteractiveRebaseActionMenu {
        ix: usize,
        can_squash: bool,
        can_drop: bool,
    },
    InteractiveRebaseAutosquashMenu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum RepoPopoverKind {
    Remote(RemotePopoverKind),
    Worktree(WorktreePopoverKind),
    Submodule(SubmodulePopoverKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum RemotePopoverKind {
    AddPrompt,
    EditUrlPrompt { name: String, kind: RemoteUrlKind },
    RemoveConfirm { name: String },
    Menu { name: String },
    DeleteBranchConfirm { remote: String, branch: String },
    SshKeyPrompt { name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum WorktreePopoverKind {
    SectionMenu,
    Menu {
        path: std::path::PathBuf,
        branch: Option<String>,
    },
    AddPrompt,
    OpenPicker,
    RemovePicker,
    /// The action bar's workspace badge picker: every worktree including the
    /// current one, plus a create row. Distinct from `OpenPicker`, which hides
    /// the current worktree and has no create affordance.
    BadgePicker,
    RemoveConfirm {
        path: std::path::PathBuf,
        branch: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum SubmodulePopoverKind {
    SectionMenu,
    Menu { path: std::path::PathBuf },
    AddPrompt,
    ChangePointerPrompt { path: std::path::PathBuf },
    TrustConfirm,
    OpenPicker,
    RemovePicker,
    RemoveConfirm { path: std::path::PathBuf },
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

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::view) enum RemoteRow {
    Header(String),
    Branch { remote: String, name: String },
}

/// Per-popover state for the context menu domain, grouped off the host's top level.
pub(super) struct ContextMenuState {
    pub(super) context_menu_selected_ix: Option<usize>,
    /// Submenu groups currently expanded in the open context menu, keyed by
    /// the group's stable id. Selection indices shift when a group opens or
    /// closes, so both are cleared together.
    pub(super) context_menu_open_submenus: FxHashSet<SharedString>,
}

/// Per-popover state for the repo picker domain, grouped off the host's top level.
pub(super) struct RepoPickerState {
    pub(super) repo_picker_selected_index: Option<usize>,
    /// Session recent repositories snapshotted when a repository picker opens,
    /// so the list can't shift under the user mid-interaction.
    pub(super) cached_recent_repos: Vec<std::path::PathBuf>,
    /// Session pins snapshotted alongside `cached_recent_repos`. Held apart from
    /// the recents so a pin outlives the recents cap.
    pub(super) cached_pinned_repos: Vec<std::path::PathBuf>,
    /// Storage keys of the repository picker sections the user folded away.
    pub(super) cached_collapsed_picker_sections: std::collections::BTreeSet<String>,
    pub(super) repo_picker_sort: repo_picker::RepoPickerSort,
    pub(super) repo_picker_sort_menu_open: bool,

    pub(super) repo_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) repo_picker_rows_cache: rows_cache::RowsCache<repo_picker::RepoPickerEntry>,
    pub(super) _repo_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the branch picker domain, grouped off the host's top level.
pub(super) struct BranchPickerState {
    pub(super) branch_picker_selected_index: Option<usize>,
    pub(super) branch_picker_search_input: Option<Entity<components::TextInput>>,
    /// Row models for the pickers that build one row per repository, ref or
    /// worktree, rebuilt only when the data behind them changes rather than on
    /// every frame. See [`rows_cache`] — a hover moving between rows re-renders
    /// this whole view.
    pub(super) branch_picker_rows_cache:
        rows_cache::RowsCache<branch_picker::BranchPickerNavTarget>,
    pub(super) branch_ref_rows_cache: rows_cache::RowsCache<String>,
    pub(super) _branch_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the worktree picker domain, grouped off the host's top level.
pub(super) struct WorktreePickerState {
    pub(super) worktree_picker_selected_index: Option<usize>,
    pub(super) worktree_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) worktree_picker_rows_cache: rows_cache::RowsCache<std::path::PathBuf>,
    pub(super) _worktree_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the workspace picker domain, grouped off the host's top level.
pub(super) struct WorkspacePickerState {
    pub(super) workspace_picker_selected_index: Option<usize>,
    pub(super) workspace_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) workspace_picker_rows_cache: rows_cache::RowsCache<workspace_picker::WorkspaceRow>,
    pub(super) _workspace_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the upstream picker domain, grouped off the host's top level.
pub(super) struct UpstreamPickerState {
    pub(super) upstream_picker_selected_index: Option<usize>,
    pub(super) upstream_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) upstream_picker_rows_cache: rows_cache::RowsCache<upstream_picker::UpstreamRow>,
    pub(super) _upstream_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the submodule picker domain, grouped off the host's top level.
pub(super) struct SubmodulePickerState {
    pub(super) submodule_picker_selected_index: Option<usize>,
    pub(super) submodule_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) submodule_picker_rows_cache: rows_cache::RowsCache<std::path::PathBuf>,
    pub(super) _submodule_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the remote picker domain, grouped off the host's top level.
pub(super) struct RemotePickerState {
    pub(super) remote_picker_selected_index: Option<usize>,
    pub(super) remote_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) remote_picker_rows_cache: rows_cache::RowsCache<remote_picker::RemotePickerRow>,
    pub(super) _remote_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the tag picker domain, grouped off the host's top level.
pub(super) struct TagPickerState {
    pub(super) tag_picker_selected_index: Option<usize>,
    pub(super) tag_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) tag_picker_rows_cache: rows_cache::RowsCache<String>,
    pub(super) _tag_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the commit search picker domain, grouped off the host's top level.
pub(super) struct CommitSearchPickerState {
    pub(super) commit_search_picker_selected_index: Option<usize>,
    pub(super) commit_search_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) commit_search_picker_rows_cache:
        rows_cache::RowsCache<commit_search_picker::CommitSearchPickerRow>,
    pub(super) _commit_search_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the stash picker domain, grouped off the host's top level.
pub(super) struct StashPickerState {
    pub(super) stash_picker_prompt_selected_index: Option<usize>,
    pub(super) stash_picker_search_input: Option<Entity<components::TextInput>>,
    pub(super) stash_picker_rows_cache: rows_cache::RowsCache<stash_picker_prompt::StashRow>,
    pub(super) _stash_picker_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the file history domain, grouped off the host's top level.
pub(super) struct FileHistoryState {
    pub(super) file_history_selected_index: Option<usize>,
    pub(super) file_history_search_input: Option<Entity<components::TextInput>>,
    pub(super) file_history_rows_cache: rows_cache::RowsCache<CommitId>,
    pub(super) _file_history_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the history author filter domain, grouped off the host's top level.
pub(super) struct HistoryAuthorFilterState {
    pub(super) history_author_filter_selected_index: Option<usize>,
    pub(super) history_author_filter_search_input: Option<Entity<components::TextInput>>,
    /// Author suggestions for the history author filter, keyed by repository and
    /// the log revision they were collected from. Collecting them walks the
    /// whole accumulated log, and the popover re-renders on every mouse move
    /// over it, so the result has to outlive the frame. See
    /// [`author_filter::suggestions`].
    pub(super) history_author_suggestions: Option<(RepoId, u64, std::sync::Arc<[SharedString]>)>,
    pub(super) _history_author_filter_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the history ref filter domain, grouped off the host's top level.
pub(super) struct HistoryRefFilterState {
    /// The ref-filter popover's plain query box: narrows the three ref
    /// sections live, with no Enter semantics — clicks still toggle filters.
    pub(super) history_ref_filter_search_input: Option<Entity<components::TextInput>>,
    pub(super) _history_ref_filter_search_input_subscription: Option<gpui::Subscription>,
}

/// Per-popover state for the clone repo domain, grouped off the host's top level.
pub(super) struct CloneRepoState {
    pub(super) clone_repo_url_input: Entity<components::TextInput>,
    pub(super) clone_repo_parent_dir_input: Entity<components::TextInput>,
    /// Optional per-clone SSH key path: rides the clone as
    /// `core.sshCommand` and persists onto the new `origin` remote.
    pub(super) clone_ssh_key_input: Entity<components::TextInput>,
    pub(super) clone_repo_focus: DialogFocus,
    pub(super) clone_repo_browse_focus_handle: FocusHandle,
}

/// Per-popover state for the repo settings domain, grouped off the host's top level.
pub(super) struct RepoSettingsState {
    /// Repo-settings prompt state: two inputs plus the tri-state signing
    /// override and the config snapshot read on open.
    pub(super) repo_settings_user_input: Entity<components::TextInput>,
    pub(super) repo_settings_email_input: Entity<components::TextInput>,
    pub(super) repo_settings_sign_commits: Option<bool>,
    pub(super) repo_settings_error: Option<SharedString>,
    /// The local/global snapshot consumed by the next panel render; taken by
    /// the panel so it is re-read on every open, never between renders.
    pub(super) repo_settings_current:
        Option<crate::view::panels::popover::repo_settings::RepoSettingsCurrent>,
    pub(super) repo_settings_focus: DialogFocus,
    /// Test seam: how many times the repo-settings config snapshot was read
    /// from disk. An open and an apply each read once; renders must read
    /// never — a render-time read is five git spawns per keystroke.
    #[cfg(test)]
    pub(super) repo_settings_test_loads: usize,
}

/// Per-popover state for the rebase onto domain, grouped off the host's top level.
pub(super) struct RebaseOntoState {
    pub(super) rebase_onto_input: Entity<components::TextInput>,
    pub(super) rebase_onto_submit_focus_handle: FocusHandle,
}

/// Per-popover state for the create tag domain, grouped off the host's top level.
pub(super) struct CreateTagState {
    pub(super) create_tag_input: Entity<components::TextInput>,
    pub(super) create_tag_message_input: Entity<components::TextInput>,
    pub(super) create_tag_message_scroll: ScrollHandle,
    pub(super) create_tag_annotated: bool,
    pub(super) create_tag_focus: DialogFocus,
    pub(super) create_tag_annotated_focus_handle: FocusHandle,
}

/// Per-popover state for the gitignore domain, grouped off the host's top level.
pub(super) struct GitignoreState {
    /// One `.gitignore` line per row. Multiline so a multi-file selection and a
    /// single file share one code path, and so the field reads like the file it
    /// is about to become.
    pub(super) gitignore_patterns_input: Entity<components::TextInput>,
    pub(super) gitignore_patterns_scroll: ScrollHandle,
    /// Which scope's patterns the input was last prefilled with. Only a prefill
    /// shortcut — submit reads the input, never this.
    pub(super) gitignore_scope: worktree_core::gitignore::GitignoreScope,
    /// Computed once when the dialog opens, so a status refresh arriving
    /// mid-edit cannot change the offered scopes under the user.
    pub(super) gitignore_suggestions: Option<worktree_core::gitignore::GitignoreSuggestions>,
    /// The paths the dialog is about, for the "Ignore <file>" body text.
    pub(super) gitignore_paths: Vec<std::path::PathBuf>,
}

/// Per-popover state for the squash domain, grouped off the host's top level.
pub(super) struct SquashState {
    pub(super) squash_message_input: Entity<components::TextInput>,
    pub(super) squash_description_input: Entity<components::TextInput>,
    pub(super) squash_description_scroll: ScrollHandle,
    /// The `(oldest, head)` range the squash prompt's message inputs were last
    /// prefilled for. Prevents re-prefilling the same range (so a user who
    /// clears the fields keeps them cleared) and, together with the empty-input
    /// check, prevents clobbering text the user typed while the preview loaded.
    pub(super) squash_prompt_prefilled_range: Option<(
        worktree_core::domain::CommitId,
        worktree_core::domain::CommitId,
    )>,
    pub(super) squash_cancel_focus_handle: FocusHandle,
    pub(super) squash_submit_focus_handle: FocusHandle,
    pub(super) _squash_message_input_subscription: gpui::Subscription,
    pub(super) _squash_description_input_subscription: gpui::Subscription,
}

/// Per-popover state for the remote prompts domain, grouped off the host's top level.
pub(super) struct RemotePromptsState {
    pub(super) remote_name_input: Entity<components::TextInput>,
    pub(super) remote_url_input: Entity<components::TextInput>,
    pub(super) remote_url_edit_input: Entity<components::TextInput>,
    pub(super) remote_ssh_key_input: Entity<components::TextInput>,
    pub(super) remote_add_focus: DialogFocus,
    pub(super) remote_edit_focus: DialogFocus,
    pub(super) remote_ssh_key_focus: DialogFocus,
    pub(super) remote_ssh_key_clear_focus: FocusHandle,
}

/// Per-popover state for the create branch domain, grouped off the host's top level.
pub(super) struct CreateBranchState {
    pub(super) create_branch_input: Entity<components::TextInput>,
    pub(super) create_branch_checkout_enabled: bool,
    pub(super) create_branch_source_target: String,
    pub(super) create_branch_from_ref_checkout_focus_handle: FocusHandle,
    pub(super) create_branch_from_ref_focus: DialogFocus,
}

/// Per-popover state for the stash domain, grouped off the host's top level.
pub(super) struct StashState {
    pub(super) stash_message_input: Entity<components::TextInput>,
    pub(super) stash_focus: DialogFocus,
    /// Stash prompt option rows. Defaults re-applied every time the prompt
    /// opens, so an earlier stash never leak its options into the next one.
    pub(super) stash_include_untracked: bool,
    pub(super) stash_keep_index: bool,
    pub(super) stash_include_untracked_focus_handle: FocusHandle,
    pub(super) stash_keep_index_focus_handle: FocusHandle,
}

/// Per-popover state for the mr push domain, grouped off the host's top level.
pub(super) struct MrPushState {
    /// Merge-request push prompt state. Like the stash options, defaults are
    /// re-applied every time the prompt opens; `merge_request.create` itself
    /// is implicit — the dialog exists to create one.
    pub(super) mr_push_target_input: Entity<components::TextInput>,
    /// The AI-generated (or hand-written) MR description. Prepare-and-copy
    /// only: GitLab push options carry no reliable multiline description.
    pub(super) mr_push_description_input: Entity<components::TextInput>,
    pub(super) mr_push_description_generating: bool,
    pub(super) mr_push_description_error: Option<SharedString>,
    pub(super) mr_push_merge_when_pipeline_succeeds: bool,
    pub(super) mr_push_remove_source_branch: bool,
    pub(super) mr_push_push_to_mr_branch: bool,
    pub(super) mr_push_pipeline_focus_handle: FocusHandle,
    pub(super) mr_push_remove_source_focus_handle: FocusHandle,
    pub(super) mr_push_mr_branch_focus_handle: FocusHandle,
}

/// Per-popover state for the commit prompt domain, grouped off the host's top level.
pub(super) struct CommitPromptState {
    pub(super) commit_prompt_message_drafts: FxHashMap<RepoId, SharedString>,
    pub(super) commit_prompt_message_input: Entity<components::TextInput>,
    pub(super) commit_prompt_message_scroll: ScrollHandle,
    pub(super) commit_prompt_focus: DialogFocus,
}

/// Per-popover state for the worktree add domain, grouped off the host's top level.
pub(super) struct WorktreeAddState {
    pub(super) worktree_ref_source_target: String,
    pub(super) suppress_worktree_submit_after_ref_enter: bool,
    /// Path/reference the workspace badge's create row hands to the Add-worktree
    /// dialog. Consumed (and cleared) when that dialog opens, so a later
    /// open from elsewhere still starts blank.
    pub(super) pending_worktree_add_prefill: Option<(String, String)>,
    pub(super) worktree_path_input: Entity<components::TextInput>,
    pub(super) worktree_ref_input: Entity<components::TextInput>,
    pub(super) worktree_focus: DialogFocus,
    pub(super) worktree_browse_focus_handle: FocusHandle,
}

/// Per-popover state for the submodule add domain, grouped off the host's top level.
pub(super) struct SubmoduleAddState {
    pub(super) submodule_url_input: Entity<components::TextInput>,
    pub(super) submodule_path_input: Entity<components::TextInput>,
    pub(super) submodule_ref_input: Entity<components::TextInput>,
    pub(super) submodule_branch_input: Entity<components::TextInput>,
    pub(super) submodule_name_input: Entity<components::TextInput>,
    pub(super) submodule_add_advanced_expanded: bool,
    pub(super) submodule_force_enabled: bool,
    pub(super) submodule_focus: DialogFocus,
    pub(super) submodule_advanced_focus_handle: FocusHandle,
    pub(super) submodule_force_focus_handle: FocusHandle,
}

/// Per-popover state for the push upstream domain, grouped off the host's top level.
pub(super) struct PushUpstreamState {
    pub(super) push_upstream_branch_input: Entity<components::TextInput>,
    pub(super) push_upstream_focus: DialogFocus,
}

/// Per-popover state for the rebase reword domain, grouped off the host's top level.
pub(super) struct RebaseRewordState {
    pub(super) rebase_reword_input: Entity<components::TextInput>,
    pub(super) rebase_reword_description_input: Entity<components::TextInput>,
    pub(super) rebase_reword_description_scroll: ScrollHandle,
}
pub(in crate::view) struct PopoverHost {
    pub(super) store: Arc<AppStore>,

    pub(super) state: Arc<AppState>,

    pub(super) theme: AppTheme,

    pub(super) theme_mode: ThemeMode,

    pub(super) date_time_format: DateTimeFormat,

    pub(super) timezone: Timezone,

    pub(super) show_timezone: bool,

    pub(super) change_tracking_view: ChangeTrackingView,

    pub(super) commit_amend_enabled: bool,

    pub(super) commit_push_after_enabled: bool,

    pub(super) push_pull_retry_enabled: bool,

    pub(super) diff_content_mode: DiffContentMode,

    pub(super) diff_whitespace_mode: DiffWhitespaceMode,

    pub(super) diff_reveal_whitespace_chars: bool,

    pub(super) diff_word_wrap: bool,

    pub(super) diff_show_line_numbers: bool,

    pub(super) _ui_model_subscription: gpui::Subscription,

    pub(super) repo_picker: RepoPickerState,

    pub(super) branch_picker: BranchPickerState,

    pub(super) worktree_picker: WorktreePickerState,

    pub(super) workspace_picker: WorkspacePickerState,

    pub(super) upstream_picker: UpstreamPickerState,

    pub(super) submodule_picker: SubmodulePickerState,

    pub(super) remote_picker: RemotePickerState,

    pub(super) tag_picker: TagPickerState,

    pub(super) commit_search_picker: CommitSearchPickerState,

    pub(super) file_history: FileHistoryState,

    pub(super) history_author_filter: HistoryAuthorFilterState,

    pub(super) history_ref_filter: HistoryRefFilterState,

    pub(super) squash: SquashState,

    pub(super) _prompt_input_subscriptions: Vec<gpui::Subscription>,

    pub(super) notify_fingerprint: u64,

    pub(super) root_view: WeakEntity<WorkTreeView>,

    /// Mirror of the root view's mode, which is fixed for the window's lifetime.
    /// Held here because menu models are built while the root view's update
    /// borrow is active, so its entity can't be read at that point.
    pub(super) root_view_mode: WorkTreeViewMode,

    pub(super) tooltip_host: WeakEntity<TooltipHost>,

    pub(super) main_pane: Entity<MainPaneView>,

    pub(super) details_pane: Entity<DetailsPaneView>,

    pub(super) reflog_pane: Entity<ReflogPaneView>,

    pub(super) sidebar_pane: Entity<SidebarPaneView>,

    /// Mirror of the sidebar pane's pinned branches, keyed by repository
    /// workdir. Kept here because context menus are built from click handlers
    /// that already hold the sidebar pane's update borrow, so its entity can't
    /// be read at that point.
    pub(super) pinned_branches_by_repo:
        std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,

    /// Mirror of the sidebar's collapse set, kept here for the same reason as
    /// [`Self::pinned_branches_by_repo`]: the branch group menu is built while
    /// the sidebar pane's update borrow is already held.
    pub(super) collapsed_items_by_repo:
        std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,

    /// Mirror of the sidebar's branch filter, for the same reason.
    pub(super) branch_filter_query: String,

    pub(super) popover: Option<PopoverKind>,

    pub(super) popover_anchor: Option<PopoverAnchor>,

    /// Explicit 1-based mainline selected for the currently open single
    /// merge-commit cherry-pick confirmation. Reset every time that dialog
    /// opens; drafts are intentionally session-local.
    pub(super) cherry_pick_mainline: Option<usize>,

    /// Period tab shown by the statistics popover. View-local rather than a
    /// `PopoverKind` field so switching tabs doesn't reopen the popover (which
    /// would refocus and re-request data); reset to Week on open.
    pub(super) statistics_period: statistics::StatisticsPeriod,

    /// Reset mode chosen in the undo prompt, once the user departs from the
    /// plan's suggestion. `None` until then; reset on open.
    pub(super) undo_reset_mode: Option<ResetMode>,

    /// The diff view's pending (or landed) AI explanation, paired with the
    /// hunk snapshot it was requested against. Reset on open; every open
    /// starts a fresh request.
    pub(super) hunk_explanation: Option<hunk_explanation::HunkExplanation>,

    /// Test seams standing in for the network call test builds cannot make:
    /// how many explanation requests were driven, and the patch the last one
    /// carried.
    #[cfg(test)]
    pub(super) hunk_explanation_test_requests: usize,

    #[cfg(test)]
    pub(super) hunk_explanation_test_last_patch: Option<String>,

    pub(super) context_menu_focus_handle: FocusHandle,

    /// Focus held by the App/Add Repository menu invoker, restored when that
    /// menu is dismissed without replacing it with another prompt.
    pub(super) menu_invoker_focus: Option<FocusHandle>,

    /// Whether the open popover was invoked from inside the diff panel.
    ///
    /// Some menus — the web link menu above all — can be raised from either the
    /// diff panel or the commit details pane, and only the former should hand
    /// focus back to the diff panel when it closes.
    pub(super) popover_opened_from_diff_panel: bool,

    pub(super) prompt_tab_group_focus_handle: FocusHandle,

    pub(super) prompt_tab_wrap_end_focus_handle: FocusHandle,

    pub(super) context_menu: ContextMenuState,

    /// Repository row whose context menu floats over the picker, and the window
    /// position it was invoked at. The picker stays open underneath it.
    pub(super) picker_row_menu: Option<picker_row_menu::PickerRowMenu>,

    pub(super) worktree_add: WorktreeAddState,

    pub(super) stash_picker: StashPickerState,

    pub(super) picker_prompt_scroll: ScrollHandle,

    pub(super) clone_repo: CloneRepoState,

    pub(super) repo_settings: RepoSettingsState,

    pub(super) rebase_onto: RebaseOntoState,

    pub(super) create_tag: CreateTagState,

    pub(super) gitignore: GitignoreState,

    pub(super) remote_prompts: RemotePromptsState,

    pub(super) create_branch: CreateBranchState,

    /// Set while a row menu floating over a picker runs one of its entries. The
    /// menu has already closed itself by then, and the popover underneath is the
    /// picker — which stays up so the next row can be acted on.
    pub(super) suppress_popover_close_after_action: bool,

    pub(super) checkout_remote_branch_focus: DialogFocus,

    pub(super) stash: StashState,

    pub(super) mr_push: MrPushState,

    pub(super) stash_branch_focus: DialogFocus,

    pub(super) commit_prompt: CommitPromptState,

    pub(super) push_upstream: PushUpstreamState,

    pub(super) submodule_add: SubmoduleAddState,

    pub(super) rebase_reword: RebaseRewordState,
}

/// Rows the branch badge's checkout picker would show for `query`, for the
/// picker benchmarks. The builder is a pure function of the repository, so the
/// benchmark measures exactly what a frame used to rebuild.
#[cfg(feature = "benchmarks")]
pub(in crate::view) fn benchmark_branch_checkout_rows(
    repo: &RepoState,
    query: &str,
    now: std::time::SystemTime,
) -> Vec<components::PickerPromptItem> {
    branch_picker::rows(repo, query, now).items
}

/// Rows the workspace badge's picker would show for `query`, for the picker
/// benchmarks.
#[cfg(feature = "benchmarks")]
pub(in crate::view) fn benchmark_workspace_rows(
    repo: &RepoState,
    query: &str,
) -> Vec<components::PickerPromptItem> {
    workspace_picker::rows(repo, query).items
}

impl PopoverHost {
    #[cfg(test)]
    pub(in crate::view) fn create_branch_input_focus_handle_for_test(
        &self,
        app: &App,
    ) -> FocusHandle {
        self.create_branch
            .create_branch_input
            .read(app)
            .focus_handle()
    }

    /// The history author filter's search box, once its popover has opened it.
    #[cfg(test)]
    pub(in crate::view) fn history_author_filter_search_input_for_test(
        &self,
    ) -> Option<&Entity<components::TextInput>> {
        self.history_author_filter
            .history_author_filter_search_input
            .as_ref()
    }

    /// Scrolls the author dropdown to a displayed row exactly as its keyboard
    /// navigation does.
    #[cfg(test)]
    pub(in crate::view) fn scroll_history_author_filter_to_item_for_test(
        &mut self,
        ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        self.scroll_history_author_filter_to_row(ix, cx);
    }

    pub(super) fn sync_titlebar_app_menu_state(&self, cx: &mut gpui::Context<Self>) {
        let root_view = self.root_view.clone();
        let app_menu_open = matches!(self.popover, Some(PopoverKind::AppMenu));
        let repo_picker_open = matches!(self.popover, Some(PopoverKind::RepoPicker));
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.title_bar.update(cx, |title_bar, cx| {
                    title_bar.set_app_menu_open(app_menu_open, cx);
                    title_bar.set_repo_picker_open(repo_picker_open, cx);
                });
            });
        });
    }

    pub(super) fn clear_active_context_menu_invoker(&self, cx: &mut gpui::Context<Self>) {
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.set_active_context_menu_invoker(None, cx);
            });
        });
    }

    pub(super) fn history_refs_menu_active(&self, cx: &mut gpui::Context<Self>) -> bool {
        self.root_view
            .update(cx, |root, _cx| {
                root.active_context_menu_invoker
                    .as_ref()
                    .is_some_and(|invoker| {
                        invoker.as_ref().starts_with(
                            crate::view::history_refs_hover::HISTORY_REFS_HOVER_MENU_INVOKER_PREFIX,
                        )
                    })
            })
            .unwrap_or(false)
    }

    /// Subscription that submits a prompt when Enter is pressed in one of its
    /// inputs. Escape is consumed here; prompt dismissal is handled by the
    /// PopoverPrompt key context.
    pub(super) fn prompt_enter_subscription(
        input: &Entity<components::TextInput>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        is_active: fn(&Self) -> bool,
        submit: fn(&mut Self, &mut Window, &mut gpui::Context<Self>),
    ) -> gpui::Subscription {
        cx.observe_in(input, window, move |this, input, window, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            let _ = input.update(cx, |input, _| input.take_escape_pressed());

            if !is_active(this) {
                return;
            }

            if enter_pressed {
                submit(this, window, cx);
                return;
            }

            cx.notify();
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::view) fn new(
        store: Arc<AppStore>,
        ui_model: Entity<AppUiModel>,
        theme: AppTheme,
        theme_mode: ThemeMode,
        date_time_format: DateTimeFormat,
        timezone: Timezone,
        show_timezone: bool,
        change_tracking_view: ChangeTrackingView,
        commit_push_after_enabled: bool,
        push_pull_retry_enabled: bool,
        diff_content_mode: DiffContentMode,
        diff_whitespace_mode: DiffWhitespaceMode,
        diff_reveal_whitespace_chars: bool,
        diff_word_wrap: bool,
        diff_show_line_numbers: bool,
        root_view: WeakEntity<WorkTreeView>,
        root_view_mode: WorkTreeViewMode,
        tooltip_host: WeakEntity<TooltipHost>,
        main_pane: Entity<MainPaneView>,
        details_pane: Entity<DetailsPaneView>,
        reflog_pane: Entity<ReflogPaneView>,
        sidebar_pane: Entity<SidebarPaneView>,
        pinned_branches_by_repo: std::collections::BTreeMap<
            std::path::PathBuf,
            std::collections::BTreeSet<String>,
        >,
        collapsed_items_by_repo: std::collections::BTreeMap<
            std::path::PathBuf,
            std::collections::BTreeSet<String>,
        >,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let state = Arc::clone(&ui_model.read(cx).state);
        let subscription = cx.observe(&ui_model, |this, model, cx| {
            this.state = Arc::clone(&model.read(cx).state);
            this.commit_prompt
                .commit_prompt_message_drafts
                .retain(|repo_id, _| this.state.repos.iter().any(|repo| repo.id == *repo_id));

            // Prefill the squash prompt from the message preview when it lands,
            // rather than in the render path, so the generated message never
            // clobbers text the user typed while it was loading.
            this.sync_squash_prompt_prefill(cx);

            let Some(popover) = this.popover.as_ref() else {
                return;
            };

            let next_fingerprint = fingerprint::notify_fingerprint(&this.state, popover);
            if next_fingerprint != this.notify_fingerprint {
                this.notify_fingerprint = next_fingerprint;
                cx.notify();
            }
        });

        let clone_repo_url_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "https://example.com/org/repo.git".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let clone_repo_parent_dir_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "/path/to/parent/folder".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let clone_ssh_key_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.clone_ssh_key"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let repo_settings_user_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.repo_settings_user"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let repo_settings_email_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.repo_settings_email"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let rebase_onto_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "origin/main".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let create_tag_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "v1.0.0".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let create_tag_message_scroll = ScrollHandle::new();
        let create_tag_message_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.annotation_message").into(),
                    multiline: true,
                    soft_wrap: true,
                    min_lines: 3,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_vertical_scroll_handle(Some(create_tag_message_scroll.clone()));
            input
        });

        let gitignore_patterns_scroll = ScrollHandle::new();
        let gitignore_patterns_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "/path/to/file".into(),
                    multiline: true,
                    soft_wrap: true,
                    min_lines: 3,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_vertical_scroll_handle(Some(gitignore_patterns_scroll.clone()));
            input
        });

        let squash_message_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.commit_message"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let squash_description_scroll = ScrollHandle::new();
        let squash_description_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.description_optional"),
                    multiline: true,
                    soft_wrap: true,
                    min_lines: 4,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_vertical_scroll_handle(Some(squash_description_scroll.clone()));
            input
        });

        let remote_name_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "origin".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let remote_url_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "https://example.com/org/repo.git".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let remote_url_edit_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "https://example.com/org/repo.git".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let remote_ssh_key_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "~/.ssh/id_ed25519".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let create_branch_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "branch-name".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let stash_message_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.stash_message"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let mr_push_target_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.mr_push_target"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let mr_push_description_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.mr_push_description"),
                    multiline: true,
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        // The subject input re-renders the host on every keystroke so the
        // Squash button's disabled state (driven by whether the message is
        // empty) stays current, and submits on Enter.
        let squash_message_input_subscription =
            cx.observe(&squash_message_input, |this, input, cx| {
                let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
                let _ = input.update(cx, |input, _| input.take_escape_pressed());

                if !matches!(this.popover, Some(PopoverKind::SquashPrompt { .. })) {
                    return;
                }

                if enter_pressed {
                    this.submit_squash(cx);
                    return;
                }

                cx.notify();
            });

        // The multiline description input only needs to re-render the host (it
        // does not affect the button state, and Enter inserts a newline).
        let squash_description_input_subscription =
            cx.observe(&squash_description_input, |this, _input, cx| {
                if !matches!(this.popover, Some(PopoverKind::SquashPrompt { .. })) {
                    return;
                }
                cx.notify();
            });

        let commit_prompt_message_scroll = ScrollHandle::new();
        let commit_prompt_message_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.commit_message"),
                    multiline: true,
                    soft_wrap: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_vertical_scroll_handle(Some(commit_prompt_message_scroll.clone()));
            input
        });

        let push_upstream_branch_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "branch-name".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let worktree_path_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "/path/to/worktree".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let worktree_ref_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "branch-or-commit".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let submodule_url_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "https://example.com/org/repo.git".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let submodule_path_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "path/in/repo".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let submodule_ref_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "branch-or-commit".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let submodule_name_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "submodule-logical-name".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let submodule_branch_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "feature".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });

        let rebase_reword_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.commit_subject"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let rebase_reword_description_scroll = ScrollHandle::new();
        let rebase_reword_description_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: crate::i18n::tr("ui.placeholder.description_optional"),
                    multiline: true,
                    soft_wrap: true,
                    min_lines: 4,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_vertical_scroll_handle(Some(rebase_reword_description_scroll.clone()));
            input
        });

        let mut prompt_input_subscriptions = Vec::new();
        prompt_input_subscriptions.push(cx.observe(
            &commit_prompt_message_input,
            |this, _input, cx| {
                if matches!(this.popover, Some(PopoverKind::CommitPrompt { .. })) {
                    cx.notify();
                }
            },
        ));
        for input in [&repo_settings_user_input, &repo_settings_email_input] {
            prompt_input_subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| matches!(this.popover, Some(PopoverKind::RepoSettingsPrompt { .. })),
                |this, window, cx| this.submit_repo_settings_open(window, cx),
            ));
        }
        for input in [
            &clone_repo_url_input,
            &clone_repo_parent_dir_input,
            &clone_ssh_key_input,
        ] {
            prompt_input_subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| matches!(this.popover, Some(PopoverKind::CloneRepo)),
                |this, _window, cx| this.submit_clone_repo(cx),
            ));
        }
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &create_tag_input,
            window,
            cx,
            |this| matches!(this.popover, Some(PopoverKind::CreateTagPrompt { .. })),
            |this, _window, cx| this.submit_create_tag(cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &create_branch_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::CreateBranchFromRefPrompt { .. })
                        | Some(PopoverKind::RenameBranchPrompt { .. })
                        | Some(PopoverKind::CheckoutRemoteBranchPrompt { .. })
                        | Some(PopoverKind::StashBranchPrompt { .. })
                )
            },
            |this, window, cx| {
                if matches!(
                    this.popover,
                    Some(PopoverKind::CreateBranchFromRefPrompt { .. })
                ) {
                    this.submit_create_branch(window, cx);
                } else if matches!(this.popover, Some(PopoverKind::RenameBranchPrompt { .. })) {
                    this.submit_rename_branch(window, cx);
                } else if matches!(this.popover, Some(PopoverKind::StashBranchPrompt { .. })) {
                    this.submit_stash_branch(window, cx);
                } else {
                    this.submit_checkout_remote_branch(cx);
                }
            },
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &stash_message_input,
            window,
            cx,
            |this| matches!(this.popover, Some(PopoverKind::StashPrompt { .. })),
            |this, window, cx| this.submit_stash(window, cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &mr_push_target_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::MergeRequestPushPrompt { .. })
                )
            },
            |this, window, cx| this.submit_mr_push(window, cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &submodule_ref_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::Repo {
                        kind: RepoPopoverKind::Submodule(
                            SubmodulePopoverKind::ChangePointerPrompt { .. }
                        ),
                        ..
                    })
                )
            },
            |this, window, cx| this.submit_submodule_change_pointer(window, cx),
        ));
        for input in [&remote_name_input, &remote_url_input] {
            prompt_input_subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| {
                    matches!(
                        this.popover,
                        Some(PopoverKind::Repo {
                            kind: RepoPopoverKind::Remote(RemotePopoverKind::AddPrompt),
                            ..
                        })
                    )
                },
                |this, _window, cx| this.submit_remote_add(cx),
            ));
        }
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &remote_url_edit_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::Repo {
                        kind: RepoPopoverKind::Remote(RemotePopoverKind::EditUrlPrompt { .. }),
                        ..
                    })
                )
            },
            |this, _window, cx| this.submit_remote_edit_url(cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &remote_ssh_key_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::Repo {
                        kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { .. }),
                        ..
                    })
                )
            },
            |this, _window, cx| this.submit_remote_ssh_key(cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &push_upstream_branch_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::PushSetUpstreamPrompt { .. })
                )
            },
            |this, _window, cx| this.submit_push_set_upstream(cx),
        ));
        prompt_input_subscriptions.push(Self::prompt_enter_subscription(
            &worktree_path_input,
            window,
            cx,
            |this| {
                matches!(
                    this.popover,
                    Some(PopoverKind::Repo {
                        kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                        ..
                    })
                )
            },
            |this, _window, cx| this.submit_worktree_add(cx),
        ));
        for input in [
            &submodule_url_input,
            &submodule_path_input,
            &submodule_branch_input,
            &submodule_name_input,
        ] {
            prompt_input_subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| {
                    matches!(
                        this.popover,
                        Some(PopoverKind::Repo {
                            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
                            ..
                        })
                    )
                },
                |this, _window, cx| this.submit_submodule_add(cx),
            ));
        }

        let context_menu_focus_handle = cx.focus_handle().tab_index(0).tab_stop(false);
        let prompt_tab_group_focus_handle = cx.focus_handle().tab_index(0).tab_stop(false);
        let prompt_tab_wrap_end_focus_handle = cx.focus_handle().tab_index(1).tab_stop(false);
        let create_branch_from_ref_checkout_focus_handle =
            cx.focus_handle().tab_index(0).tab_stop(true);
        let create_branch_from_ref_focus = DialogFocus::new(cx);
        let checkout_remote_branch_focus = DialogFocus::new(cx);
        let stash_focus = DialogFocus::new(cx);
        let stash_include_untracked_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let stash_keep_index_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let mr_push_pipeline_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let mr_push_remove_source_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let mr_push_mr_branch_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let stash_branch_focus = DialogFocus::new(cx);
        let commit_prompt_focus = DialogFocus::new(cx);
        let clone_repo_browse_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let squash_cancel_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let squash_submit_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let rebase_onto_submit_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let clone_repo_focus = DialogFocus::new(cx);
        let repo_settings_focus = DialogFocus::new(cx);
        let create_tag_focus = DialogFocus::new(cx);
        let create_tag_annotated_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let remote_add_focus = DialogFocus::new(cx);
        let remote_edit_focus = DialogFocus::new(cx);
        let remote_ssh_key_clear_focus = cx.focus_handle().tab_index(0).tab_stop(true);
        let remote_ssh_key_focus = DialogFocus::new(cx);
        let push_upstream_focus = DialogFocus::new(cx);
        let worktree_browse_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let worktree_focus = DialogFocus::new(cx);
        let submodule_advanced_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let submodule_force_focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        let submodule_focus = DialogFocus::new(cx);

        Self {
            store,
            state,
            theme,
            theme_mode,
            date_time_format,
            timezone,
            show_timezone,
            change_tracking_view,
            commit_amend_enabled: false,
            commit_push_after_enabled,
            push_pull_retry_enabled,
            diff_content_mode,
            diff_whitespace_mode,
            diff_reveal_whitespace_chars,
            diff_word_wrap,
            diff_show_line_numbers,
            _ui_model_subscription: subscription,
            repo_picker: RepoPickerState {
                _repo_picker_search_input_subscription: None,
                repo_picker_selected_index: None,
                cached_recent_repos: Vec::new(),
                cached_pinned_repos: Vec::new(),
                cached_collapsed_picker_sections: std::collections::BTreeSet::new(),
                repo_picker_sort: repo_picker::RepoPickerSort::default(),
                repo_picker_sort_menu_open: false,
                repo_picker_rows_cache: rows_cache::RowsCache::default(),
                repo_picker_search_input: None,
            },
            branch_picker: BranchPickerState {
                _branch_picker_search_input_subscription: None,
                branch_picker_selected_index: None,
                branch_picker_rows_cache: rows_cache::RowsCache::default(),
                branch_ref_rows_cache: rows_cache::RowsCache::default(),
                branch_picker_search_input: None,
            },
            worktree_picker: WorktreePickerState {
                _worktree_picker_search_input_subscription: None,
                worktree_picker_selected_index: None,
                worktree_picker_rows_cache: rows_cache::RowsCache::default(),
                worktree_picker_search_input: None,
            },
            workspace_picker: WorkspacePickerState {
                _workspace_picker_search_input_subscription: None,
                workspace_picker_selected_index: None,
                workspace_picker_rows_cache: rows_cache::RowsCache::default(),
                workspace_picker_search_input: None,
            },
            upstream_picker: UpstreamPickerState {
                _upstream_picker_search_input_subscription: None,
                upstream_picker_selected_index: None,
                upstream_picker_rows_cache: rows_cache::RowsCache::default(),
                upstream_picker_search_input: None,
            },
            submodule_picker: SubmodulePickerState {
                _submodule_picker_search_input_subscription: None,
                submodule_picker_selected_index: None,
                submodule_picker_rows_cache: rows_cache::RowsCache::default(),
                submodule_picker_search_input: None,
            },
            remote_picker: RemotePickerState {
                _remote_picker_search_input_subscription: None,
                remote_picker_selected_index: None,
                remote_picker_rows_cache: rows_cache::RowsCache::default(),
                remote_picker_search_input: None,
            },
            tag_picker: TagPickerState {
                _tag_picker_search_input_subscription: None,
                tag_picker_selected_index: None,
                tag_picker_rows_cache: rows_cache::RowsCache::default(),
                tag_picker_search_input: None,
            },
            commit_search_picker: CommitSearchPickerState {
                _commit_search_picker_search_input_subscription: None,
                commit_search_picker_selected_index: None,
                commit_search_picker_rows_cache: rows_cache::RowsCache::default(),
                commit_search_picker_search_input: None,
            },
            file_history: FileHistoryState {
                _file_history_search_input_subscription: None,
                file_history_selected_index: None,
                file_history_rows_cache: rows_cache::RowsCache::default(),
                file_history_search_input: None,
            },
            history_author_filter: HistoryAuthorFilterState {
                _history_author_filter_search_input_subscription: None,
                history_author_filter_selected_index: None,
                history_author_suggestions: None,
                history_author_filter_search_input: None,
            },
            history_ref_filter: HistoryRefFilterState {
                _history_ref_filter_search_input_subscription: None,
                history_ref_filter_search_input: None,
            },
            stash_picker: StashPickerState {
                _stash_picker_search_input_subscription: None,
                stash_picker_rows_cache: rows_cache::RowsCache::default(),
                stash_picker_prompt_selected_index: None,
                stash_picker_search_input: None,
            },
            squash: SquashState {
                _squash_message_input_subscription: squash_message_input_subscription,
                _squash_description_input_subscription: squash_description_input_subscription,
                squash_message_input,
                squash_description_input,
                squash_description_scroll,
                squash_prompt_prefilled_range: None,
                squash_cancel_focus_handle,
                squash_submit_focus_handle,
            },
            _prompt_input_subscriptions: prompt_input_subscriptions,
            notify_fingerprint: 0,
            root_view,
            root_view_mode,
            tooltip_host,
            main_pane,
            details_pane,
            reflog_pane,
            sidebar_pane,
            pinned_branches_by_repo,
            collapsed_items_by_repo,
            branch_filter_query: String::new(),
            popover: None,
            popover_anchor: None,
            cherry_pick_mainline: None,
            statistics_period: statistics::StatisticsPeriod::default(),
            undo_reset_mode: None,
            hunk_explanation: None,
            #[cfg(test)]
            hunk_explanation_test_requests: 0,
            #[cfg(test)]
            hunk_explanation_test_last_patch: None,
            context_menu_focus_handle,
            menu_invoker_focus: None,
            popover_opened_from_diff_panel: false,
            prompt_tab_group_focus_handle,
            prompt_tab_wrap_end_focus_handle,
            context_menu: ContextMenuState {
                context_menu_selected_ix: None,
                context_menu_open_submenus: FxHashSet::default(),
            },
            picker_row_menu: None,
            worktree_add: WorktreeAddState {
                pending_worktree_add_prefill: None,
                worktree_ref_source_target: String::new(),
                suppress_worktree_submit_after_ref_enter: false,
                worktree_browse_focus_handle,
                worktree_focus,
                worktree_path_input,
                worktree_ref_input,
            },
            picker_prompt_scroll: ScrollHandle::new(),
            clone_repo: CloneRepoState {
                clone_repo_url_input,
                clone_repo_parent_dir_input,
                clone_ssh_key_input,
                clone_repo_browse_focus_handle,
                clone_repo_focus,
            },
            repo_settings: RepoSettingsState {
                repo_settings_user_input,
                repo_settings_email_input,
                repo_settings_sign_commits: None,
                repo_settings_error: None,
                repo_settings_current: None,
                #[cfg(test)]
                repo_settings_test_loads: 0,
                repo_settings_focus,
            },
            rebase_onto: RebaseOntoState {
                rebase_onto_input,
                rebase_onto_submit_focus_handle,
            },
            create_tag: CreateTagState {
                create_tag_input,
                create_tag_message_input,
                create_tag_message_scroll,
                create_tag_annotated: false,
                create_tag_annotated_focus_handle,
                create_tag_focus,
            },
            gitignore: GitignoreState {
                gitignore_patterns_input,
                gitignore_patterns_scroll,
                gitignore_scope: worktree_core::gitignore::GitignoreScope::File,
                gitignore_suggestions: None,
                gitignore_paths: Vec::new(),
            },
            remote_prompts: RemotePromptsState {
                remote_name_input,
                remote_url_input,
                remote_url_edit_input,
                remote_ssh_key_input,
                remote_add_focus,
                remote_edit_focus,
                remote_ssh_key_clear_focus,
                remote_ssh_key_focus,
            },
            create_branch: CreateBranchState {
                create_branch_input,
                create_branch_checkout_enabled: true,
                create_branch_source_target: String::new(),
                create_branch_from_ref_checkout_focus_handle,
                create_branch_from_ref_focus,
            },
            suppress_popover_close_after_action: false,
            checkout_remote_branch_focus,
            stash: StashState {
                stash_message_input,
                stash_focus,
                stash_include_untracked: true,
                stash_keep_index: false,
                stash_include_untracked_focus_handle,
                stash_keep_index_focus_handle,
            },
            mr_push: MrPushState {
                mr_push_target_input,
                mr_push_description_input,
                mr_push_description_generating: false,
                mr_push_description_error: None,
                mr_push_merge_when_pipeline_succeeds: false,
                // GitLab-side default from the C# client's push dialog.
                mr_push_remove_source_branch: true,
                mr_push_push_to_mr_branch: false,
                mr_push_pipeline_focus_handle,
                mr_push_remove_source_focus_handle,
                mr_push_mr_branch_focus_handle,
            },
            stash_branch_focus,
            commit_prompt: CommitPromptState {
                commit_prompt_message_drafts: FxHashMap::default(),
                commit_prompt_message_input,
                commit_prompt_message_scroll,
                commit_prompt_focus,
            },
            push_upstream: PushUpstreamState {
                push_upstream_focus,
                push_upstream_branch_input,
            },
            submodule_add: SubmoduleAddState {
                submodule_advanced_focus_handle,
                submodule_force_focus_handle,
                submodule_focus,
                submodule_url_input,
                submodule_path_input,
                submodule_ref_input,
                submodule_branch_input,
                submodule_name_input,
                submodule_add_advanced_expanded: false,
                submodule_force_enabled: false,
            },
            rebase_reword: RebaseRewordState {
                rebase_reword_input,
                rebase_reword_description_input,
                rebase_reword_description_scroll,
            },
        }
    }

    /// Every text input owned by the host, including the lazily created
    /// picker search inputs that currently exist.
    pub(super) fn all_text_inputs(&self) -> impl Iterator<Item = &Entity<components::TextInput>> {
        [
            &self.clone_repo.clone_repo_url_input,
            &self.clone_repo.clone_repo_parent_dir_input,
            &self.rebase_onto.rebase_onto_input,
            &self.create_tag.create_tag_input,
            &self.create_tag.create_tag_message_input,
            &self.gitignore.gitignore_patterns_input,
            &self.squash.squash_message_input,
            &self.squash.squash_description_input,
            &self.remote_prompts.remote_name_input,
            &self.remote_prompts.remote_url_input,
            &self.remote_prompts.remote_url_edit_input,
            &self.remote_prompts.remote_ssh_key_input,
            &self.create_branch.create_branch_input,
            &self.stash.stash_message_input,
            &self.commit_prompt.commit_prompt_message_input,
            &self.push_upstream.push_upstream_branch_input,
            &self.worktree_add.worktree_path_input,
            &self.worktree_add.worktree_ref_input,
            &self.submodule_add.submodule_url_input,
            &self.submodule_add.submodule_path_input,
            &self.submodule_add.submodule_ref_input,
            &self.submodule_add.submodule_branch_input,
            &self.submodule_add.submodule_name_input,
            &self.rebase_reword.rebase_reword_input,
            &self.rebase_reword.rebase_reword_description_input,
        ]
        .into_iter()
        .chain(
            [
                &self.repo_picker.repo_picker_search_input,
                &self.branch_picker.branch_picker_search_input,
                &self.remote_picker.remote_picker_search_input,
                &self.file_history.file_history_search_input,
                &self
                    .history_author_filter
                    .history_author_filter_search_input,
                &self.history_ref_filter.history_ref_filter_search_input,
                &self.worktree_picker.worktree_picker_search_input,
                &self.workspace_picker.workspace_picker_search_input,
                &self.upstream_picker.upstream_picker_search_input,
                &self.submodule_picker.submodule_picker_search_input,
                &self.tag_picker.tag_picker_search_input,
                &self.commit_search_picker.commit_search_picker_search_input,
                &self.stash_picker.stash_picker_search_input,
            ]
            .into_iter()
            .flatten(),
        )
    }

    pub(in crate::view) fn set_theme(&mut self, theme: AppTheme, cx: &mut gpui::Context<Self>) {
        self.theme = theme;

        let inputs: Vec<_> = self.all_text_inputs().cloned().collect();
        for input in inputs {
            input.update(cx, |input, cx| input.set_theme(theme, cx));
        }

        cx.notify();
    }

    pub(in crate::view) fn is_kind_open(&self, kind: &PopoverKind) -> bool {
        self.popover.as_ref() == Some(kind)
    }

    #[cfg(test)]
    pub(in crate::view) fn popover_kind_for_tests(&self) -> Option<PopoverKind> {
        self.popover.clone()
    }

    #[cfg(test)]
    pub(in crate::view) fn popover_opened_from_diff_panel_for_tests(&self) -> bool {
        self.popover_opened_from_diff_panel
    }

    /// The box the open popover hangs off, when it was anchored to one.
    #[cfg(test)]
    pub(in crate::view) fn popover_anchor_bounds_for_tests(&self) -> Option<Bounds<Pixels>> {
        match self.popover_anchor {
            Some(PopoverAnchor::Bounds(bounds)) => Some(bounds),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(in crate::view) fn worktree_path_input_text_for_tests(&self, app: &gpui::App) -> String {
        self.worktree_add
            .worktree_path_input
            .read(app)
            .text()
            .to_string()
    }

    #[cfg(test)]
    pub(in crate::view) fn worktree_ref_source_target_for_tests(&self) -> &str {
        &self.worktree_add.worktree_ref_source_target
    }

    /// Whether the unsaved-edits confirmation is the popover on screen.
    ///
    /// Asked by the close/quit path instead of a mirrored bool: that dialog
    /// blocks every further close while it is up, and a mirror that missed a
    /// dismissal wedged the window shut for the rest of the session.
    pub(in crate::view) fn showing_unsaved_file_edits_prompt(&self) -> bool {
        matches!(self.popover, Some(PopoverKind::UnsavedFileEditsConfirm(_)))
    }

    pub(super) fn active_repo_id(&self) -> Option<RepoId> {
        self.state.active_repo
    }

    pub(super) fn active_repo(&self) -> Option<&RepoState> {
        let repo_id = self.active_repo_id()?;
        self.state.repos.iter().find(|r| r.id == repo_id)
    }

    pub(in crate::view) fn set_pinned_branches(
        &mut self,
        pinned: std::collections::BTreeMap<std::path::PathBuf, std::collections::BTreeSet<String>>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.pinned_branches_by_repo == pinned {
            return;
        }
        self.pinned_branches_by_repo = pinned;
        cx.notify();
    }

    pub(in crate::view) fn set_branch_filter_query(
        &mut self,
        query: String,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.branch_filter_query == query {
            return;
        }
        self.branch_filter_query = query;
        cx.notify();
    }

    /// The active branch filter, or `None` when it matches everything.
    ///
    /// Mirrors `matches_branch_filter`, which treats a blank query as "no
    /// filter" — so a lone space must not read as a filter that hides
    /// everything.
    pub(in crate::view) fn active_branch_filter(&self) -> Option<&str> {
        let query = self.branch_filter_query.trim();
        (!query.is_empty()).then_some(query)
    }

    pub(in crate::view) fn set_collapsed_items(
        &mut self,
        collapsed: std::collections::BTreeMap<
            std::path::PathBuf,
            std::collections::BTreeSet<String>,
        >,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.collapsed_items_by_repo == collapsed {
            return;
        }
        self.collapsed_items_by_repo = collapsed;
        cx.notify();
    }

    /// Whether a sidebar collapse key is currently collapsed, going through
    /// `branch_sidebar::is_collapsed` so default-collapsed sections and the
    /// inverted `expanded:` storage are read the same way the tree reads them.
    pub(in crate::view) fn sidebar_collapse_key_is_collapsed(
        &self,
        repo_id: RepoId,
        collapse_key: &str,
    ) -> bool {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        // A repo with nothing stored reads the same as one with an empty set:
        // `is_collapsed` answers from the key's own default in both cases.
        static EMPTY: std::sync::LazyLock<std::collections::BTreeSet<String>> =
            std::sync::LazyLock::new(std::collections::BTreeSet::new);
        let items = self
            .collapsed_items_by_repo
            .get(&repo.spec.workdir)
            .unwrap_or(&EMPTY);
        crate::view::branch_sidebar::is_collapsed(items, collapse_key)
    }

    /// How many pinned branches the section is actually showing, for the pinned
    /// header's "Unpin all (N)".
    ///
    /// Counting raw pin keys would overcount: the row builder skips a pin whose
    /// branch no longer exists, and skips one filtered out by the branch
    /// filter, so "Unpin all (3)" could sit above a single row.
    pub(in crate::view) fn pinned_branch_count(
        &self,
        repo_id: RepoId,
        section: BranchSection,
    ) -> usize {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return 0;
        };
        let filter = self.active_branch_filter().unwrap_or_default();
        self.pinned_branches_by_repo
            .get(&repo.spec.workdir)
            .map_or(0, |items| {
                items
                    .iter()
                    .filter(|key| {
                        crate::view::branch_sidebar::pinned_branch_renders(
                            repo, key, section, filter,
                        )
                    })
                    .count()
            })
    }

    pub(in crate::view) fn is_branch_pinned(
        &self,
        repo_id: RepoId,
        section: BranchSection,
        name: &str,
    ) -> bool {
        let Some(repo) = self.state.repos.iter().find(|r| r.id == repo_id) else {
            return false;
        };
        let key = crate::view::branch_sidebar::branch_pin_storage_key(section, name);
        self.pinned_branches_by_repo
            .get(&repo.spec.workdir)
            .is_some_and(|items| items.contains(&key))
    }

    pub(in crate::view) fn set_date_time_format(
        &mut self,
        next: DateTimeFormat,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.date_time_format == next {
            return;
        }
        self.date_time_format = next;
        self.main_pane
            .update(cx, |pane, cx| pane.set_date_time_format(next, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(in crate::view) fn set_timezone(&mut self, next: Timezone, cx: &mut gpui::Context<Self>) {
        if self.timezone == next {
            return;
        }
        self.timezone = next;
        self.main_pane
            .update(cx, |pane, cx| pane.set_timezone(next, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(in crate::view) fn set_show_timezone(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.show_timezone == enabled {
            return;
        }
        self.show_timezone = enabled;
        self.main_pane
            .update(cx, |pane, cx| pane.set_show_timezone(enabled, cx));
        self.sync_pane_date_settings(cx);
        self.schedule_ui_settings_persist(cx);
    }

    pub(super) fn sync_pane_date_settings(&mut self, cx: &mut gpui::Context<Self>) {
        let (format, timezone, show_timezone) =
            (self.date_time_format, self.timezone, self.show_timezone);
        self.details_pane.update(cx, |pane, cx| {
            pane.set_date_settings(format, timezone, show_timezone, cx);
        });
        self.reflog_pane.update(cx, |pane, cx| {
            pane.set_date_settings(format, timezone, show_timezone, cx);
        });
    }

    pub(in crate::view) fn set_theme_mode(
        &mut self,
        next: ThemeMode,
        appearance: gpui::WindowAppearance,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.theme_mode == next {
            return;
        }

        self.theme_mode = next.clone();
        self.set_theme(next.resolve_theme(appearance), cx);
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.set_theme_mode(next.clone(), appearance, cx);
            });
        });
    }

    pub(super) fn schedule_ui_settings_persist(&mut self, cx: &mut gpui::Context<Self>) {
        let mode = self.theme_mode.clone();
        let fmt = self.date_time_format;
        let tz = self.timezone;
        let show_tz = self.show_timezone;
        let root_view = self.root_view.clone();
        cx.spawn(
            async move |_host: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                let _ = root_view.update(cx, |root, cx| {
                    root.theme_mode = mode;
                    root.date_time_format = fmt;
                    root.timezone = tz;
                    root.show_timezone = show_tz;
                    root.schedule_ui_settings_persist(cx);
                });
            },
        )
        .detach();
    }

    pub(in crate::view) fn sync_change_tracking_view(
        &mut self,
        next: ChangeTrackingView,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.change_tracking_view == next {
            return;
        }

        self.change_tracking_view = next;
        if matches!(self.popover, Some(PopoverKind::ChangeTrackingSettings)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_commit_push_after_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.commit_push_after_enabled == enabled {
            return;
        }

        self.commit_push_after_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::CommitOptionsMenu { .. })) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_push_pull_retry_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.push_pull_retry_enabled == enabled {
            return;
        }

        self.push_pull_retry_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::PushPicker)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_commit_amend_enabled(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.commit_amend_enabled == enabled {
            return;
        }

        self.commit_amend_enabled = enabled;
        if matches!(self.popover, Some(PopoverKind::CommitOptionsMenu { .. })) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_content_mode(
        &mut self,
        next: DiffContentMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_content_mode == next {
            return;
        }

        self.diff_content_mode = next;
        if matches!(self.popover, Some(PopoverKind::DiffContentModeSettings)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_whitespace_mode(
        &mut self,
        next: DiffWhitespaceMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_whitespace_mode == next {
            return;
        }

        self.diff_whitespace_mode = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_reveal_whitespace_chars(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_reveal_whitespace_chars == next {
            return;
        }

        self.diff_reveal_whitespace_chars = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_word_wrap(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap == next {
            return;
        }

        self.diff_word_wrap = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    pub(in crate::view) fn sync_diff_show_line_numbers(
        &mut self,
        next: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_show_line_numbers == next {
            return;
        }

        self.diff_show_line_numbers = next;
        if matches!(self.popover, Some(PopoverKind::DiffActionMenu)) {
            cx.notify();
        }
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub(super) fn install_linux_desktop_integration(&mut self, cx: &mut gpui::Context<Self>) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.install_linux_desktop_integration(cx);
        });
    }

    pub(super) fn push_toast(
        &mut self,
        kind: components::ToastKind,
        message: String,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.push_toast(kind, message, cx);
        });
    }
}
