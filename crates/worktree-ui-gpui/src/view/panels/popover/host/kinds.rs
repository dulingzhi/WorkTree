//! Popover and picker kinds: which popover is open and what the palette's pickers
//! are being used for.

use super::super::*;

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
/// the others being folded (fixup) into it. Defined in core (the folding rules
/// it selects between are domain rules the reducer also needs) and re-exported
/// here so the view layer's existing imports keep working.
pub(in crate::view) use worktree_core::squash::AutosquashMode;

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
    /// Confirm an autosquash: folds every `fixup!`/`squash!` commit in
    /// `base..HEAD` into its matching target commit. `base` is the commit the
    /// user right-clicked ("Autosquash from here"); `repo_id` identifies the
    /// repo whose `autosquash_preview` the panel renders.
    AutosquashConfirm {
        repo_id: RepoId,
        base: String,
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
