use super::*;

#[derive(Clone, Debug)]
pub(super) enum PopoverAnchor {
    Point(Point<Pixels>),
    Bounds(Bounds<Pixels>),
    Centered,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::view) struct PopoverWidthSpec {
    preferred: f32,
    min: f32,
    max: f32,
}

impl PopoverWidthSpec {
    pub(in crate::view) const fn fixed(width: f32) -> Self {
        Self {
            preferred: width,
            min: width,
            max: width,
        }
    }

    pub(in crate::view) const fn range(preferred: f32, min: f32, max: f32) -> Self {
        Self {
            preferred,
            min,
            max,
        }
    }

    pub(in crate::view) fn preferred_px(self, ui_scale: ui_scale::UiScale) -> Pixels {
        ui_scale.px(self.preferred)
    }

    pub(in crate::view) fn min_px(self, ui_scale: ui_scale::UiScale) -> Pixels {
        ui_scale.px(self.min)
    }

    pub(in crate::view) fn max_px(self, ui_scale: ui_scale::UiScale) -> Pixels {
        ui_scale.px(self.max)
    }
}

pub(super) const DEFAULT_CONTEXT_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(260.0, 180.0, 380.0);
pub(super) const NARROW_CONTEXT_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(220.0, 160.0, 220.0);
pub(super) const REBASE_ACTION_MENU_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(110.0);
pub(super) const REBASE_AUTOSQUASH_MENU_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(190.0);
pub(super) const CHANGE_TRACKING_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(220.0, 220.0, 320.0);
pub(super) const DIFF_ACTION_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(240.0, 200.0, 320.0);
pub(super) const MERGETOOL_SETTINGS_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(320.0, 280.0, 420.0);
pub(super) const DIFF_EDITOR_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(260.0, 200.0, 340.0);
pub(super) const CONFLICT_INPUT_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(220.0, 180.0, 280.0);
pub(super) const CONFLICT_CHUNK_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(320.0, 220.0, 360.0);
pub(super) const CONFLICT_OUTPUT_MENU_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(240.0, 200.0, 300.0);
pub(super) const STASH_MENU_WIDTH: PopoverWidthSpec = PopoverWidthSpec::range(220.0, 180.0, 360.0);
/// Wider than the sibling column menus: it carries a search box, and author
/// names run long — "Firstname Middlename Lastname" truncates at the menu
/// default.
pub(super) const HISTORY_AUTHOR_FILTER_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(320.0, 240.0, 420.0);
/// Remote branch rows carry `remote/feature/…` names, which run longer than
/// local ones; the range lets narrow windows take the scroll instead of
/// truncating every row.
pub(super) const HISTORY_REF_FILTER_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(420.0, 320.0, 540.0);
pub(super) const REPO_TAB_MENU_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(360.0);
pub(super) const PICKER_WIDTH: PopoverWidthSpec = PopoverWidthSpec::range(420.0, 420.0, 820.0);
pub(super) const LARGE_PICKER_WIDTH: PopoverWidthSpec =
    PopoverWidthSpec::range(520.0, 520.0, 820.0);
pub(super) const DIALOG_320_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(320.0);
pub(super) const DIALOG_360_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(360.0);
pub(super) const DIALOG_380_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(380.0);
pub(super) const DIALOG_420_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(420.0);
pub(super) const DIALOG_440_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(440.0);
pub(super) const DIALOG_460_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(460.0);
pub(super) const DIALOG_540_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(540.0);
pub(super) const DIALOG_640_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(640.0);
// Leaves enough room for “Open in code editor” and its three-key shortcut
// badge to remain on one line on non-macOS platforms.
pub(super) const APP_MENU_WIDTH: PopoverWidthSpec = PopoverWidthSpec::fixed(320.0);

pub(in crate::view) fn popover_ui_scale(cx: &mut gpui::Context<PopoverHost>) -> ui_scale::UiScale {
    ui_scale::UiScale::current(cx)
}

pub(in crate::view) fn popover_ui_scale_percent(cx: &mut gpui::Context<PopoverHost>) -> u32 {
    popover_ui_scale(cx).percent()
}

pub(in crate::view) fn popover_scaled_px(
    value: f32,
    ui_scale: impl Into<ui_scale::UiScale>,
) -> Pixels {
    ui_scale.into().px(value)
}

pub(in crate::view) fn popover_scaled_px_from_percent(value: f32, ui_scale_percent: u32) -> Pixels {
    popover_scaled_px(value, ui_scale_percent)
}

/// One-line replacement for the per-panel `ui_scale_percent` + closure
/// preamble: returns a copyable `f32 -> Pixels` scaler for the current
/// UI scale.
pub(in crate::view::panels) fn popover_scaled_px_fn(
    cx: &mut gpui::Context<PopoverHost>,
) -> impl Fn(f32) -> Pixels + Copy + use<> {
    let ui_scale = popover_ui_scale(cx);
    move |value: f32| ui_scale.px(value)
}

/// Which corner of the popover is placed on its anchor.
///
/// Most menus hang off a button on the right of their row, so they open
/// leftwards. The link menus are the exception: their anchor is the box of a
/// span of text, and a menu that reads as belonging to that span has to start
/// where the span starts.
pub(super) fn popover_anchor_corner(kind: &PopoverKind) -> Anchor {
    match kind {
        PopoverKind::PullPicker
        | PopoverKind::PushPicker
        | PopoverKind::CreateBranchFromRefPrompt { .. }
        | PopoverKind::RenameBranchPrompt { .. }
        | PopoverKind::StashPrompt { .. }
        | PopoverKind::StashDropConfirm { .. }
        | PopoverKind::CloneRepo
        | PopoverKind::ResetPrompt { .. }
        | PopoverKind::CreateTagPrompt { .. }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Remote(
                    RemotePopoverKind::AddPrompt
                    | RemotePopoverKind::EditUrlPrompt { .. }
                    | RemotePopoverKind::RemoveConfirm { .. }
                    | RemotePopoverKind::SshKeyPrompt { .. },
                ),
            ..
        }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Worktree(
                    WorktreePopoverKind::AddPrompt
                    | WorktreePopoverKind::OpenPicker
                    | WorktreePopoverKind::RemovePicker
                    | WorktreePopoverKind::RemoveConfirm { .. },
                ),
            ..
        }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Submodule(
                    SubmodulePopoverKind::AddPrompt
                    | SubmodulePopoverKind::ChangePointerPrompt { .. }
                    | SubmodulePopoverKind::TrustConfirm
                    | SubmodulePopoverKind::OpenPicker
                    | SubmodulePopoverKind::RemovePicker
                    | SubmodulePopoverKind::RemoveConfirm { .. },
                ),
            ..
        }
        | PopoverKind::PushSetUpstreamPrompt { .. }
        | PopoverKind::ForcePushConfirm { .. }
        | PopoverKind::MergeRequestPushPrompt { .. }
        | PopoverKind::UndoLastActionPrompt { .. }
        | PopoverKind::CherryPickCommitConfirm { .. }
        | PopoverKind::MergeCommitConfirm { .. }
        | PopoverKind::MergeAbortConfirm { .. }
        | PopoverKind::ForceDeleteBranchConfirm { .. }
        | PopoverKind::ForceRemoveWorktreeConfirm { .. }
        | PopoverKind::PullReconcilePrompt { .. }
        | PopoverKind::RebaseOntoConfirm { .. }
        | PopoverKind::RebaseReword { .. }
        | PopoverKind::CommitOptionsMenu { .. }
        | PopoverKind::PreviousCommitMessagesMenu { .. }
        | PopoverKind::RepoTabMenu { .. }
        | PopoverKind::DiffActionMenu
        | PopoverKind::MergetoolSettingsMenu
        | PopoverKind::HistoryBranchFilter { .. }
        | PopoverKind::HistoryAuthorFilter { .. }
        | PopoverKind::HistoryRefFilter { .. }
        | PopoverKind::DiffContentModeSettings
        | PopoverKind::ChangeTrackingSettings
        | PopoverKind::TerminalMenu { .. }
        | PopoverKind::UiScalePicker => Anchor::TopRight,
        _ => Anchor::TopLeft,
    }
}

pub(in crate::view) fn popover_width_spec(kind: &PopoverKind) -> Option<PopoverWidthSpec> {
    match kind {
        PopoverKind::RepoPicker
        | PopoverKind::BranchPicker {
            purpose:
                BranchPickerPurpose::Delete
                | BranchPickerPurpose::Merge
                | BranchPickerPurpose::RebaseOnto,
        }
        | PopoverKind::RemotePicker { .. }
        | PopoverKind::DeleteTagPicker { .. }
        | PopoverKind::CommitSearchPicker { .. }
        | PopoverKind::UpstreamPicker { .. } => Some(PICKER_WIDTH),
        PopoverKind::BranchPicker {
            purpose: BranchPickerPurpose::Checkout,
        } => Some(LARGE_PICKER_WIDTH),
        PopoverKind::StashPrompt { .. }
        | PopoverKind::StashBranchPrompt { .. }
        | PopoverKind::CommitPrompt { .. }
        | PopoverKind::StashPickerPrompt { .. }
        | PopoverKind::CloneRepo
        | PopoverKind::CreateTagPrompt { .. }
        | PopoverKind::SquashPrompt { .. } => Some(DIALOG_420_WIDTH),
        PopoverKind::MergeRequestPushPrompt { .. } => Some(DIALOG_440_WIDTH),
        PopoverKind::AgentSessions { .. } => Some(DIALOG_440_WIDTH),
        PopoverKind::RepoSettingsPrompt { .. } => Some(DIALOG_440_WIDTH),
        PopoverKind::UndoLastActionPrompt { .. } => Some(DIALOG_440_WIDTH),
        PopoverKind::AssumeUnchangedManager { .. } => Some(DIALOG_540_WIDTH),
        PopoverKind::Statistics { .. } => Some(DIALOG_540_WIDTH),
        // The explanation reads like prose, not a form; give it the wide
        // dialog so a normal paragraph wraps once, not three times.
        PopoverKind::HunkExplanation { .. } => Some(DIALOG_540_WIDTH),
        PopoverKind::CreateBranchFromRefPrompt { .. }
        | PopoverKind::RenameBranchPrompt { .. }
        | PopoverKind::CheckoutRemoteBranchPrompt { .. } => Some(DIALOG_540_WIDTH),
        PopoverKind::StashDropConfirm { .. }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Remote(
                    RemotePopoverKind::RemoveConfirm { .. }
                    | RemotePopoverKind::DeleteBranchConfirm { .. },
                ),
            ..
        }
        | PopoverKind::Repo {
            kind: RepoPopoverKind::Worktree(WorktreePopoverKind::RemoveConfirm { .. }),
            ..
        }
        | PopoverKind::Repo {
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::RemoveConfirm { .. }),
            ..
        }
        | PopoverKind::ForcePushConfirm { .. }
        | PopoverKind::ForceDeleteBranchConfirm { .. }
        | PopoverKind::DeleteBranchesConfirm { .. }
        | PopoverKind::DiscardChangesConfirm { .. }
        | PopoverKind::DiscardAllConfirm { .. }
        | PopoverKind::StageConflictMarkersConfirm { .. } => Some(DIALOG_420_WIDTH),
        PopoverKind::PushSetUpstreamPrompt { .. } => Some(DIALOG_320_WIDTH),
        PopoverKind::ResetPrompt { .. }
        | PopoverKind::RebaseOntoConfirm { .. }
        | PopoverKind::CherryPickCommitConfirm { .. }
        | PopoverKind::MergeCommitConfirm { .. } => Some(DIALOG_380_WIDTH),
        PopoverKind::MergeAbortConfirm { .. } => Some(DIALOG_360_WIDTH),
        PopoverKind::ForceRemoveWorktreeConfirm { .. } => Some(DIALOG_460_WIDTH),
        PopoverKind::PullReconcilePrompt { .. } | PopoverKind::AddToGitignorePrompt { .. } => {
            Some(DIALOG_440_WIDTH)
        }
        PopoverKind::Repo {
            kind:
                RepoPopoverKind::Remote(
                    RemotePopoverKind::AddPrompt
                    | RemotePopoverKind::EditUrlPrompt { .. }
                    | RemotePopoverKind::SshKeyPrompt { .. },
                ),
            ..
        }
        | PopoverKind::Repo {
            kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
            ..
        }
        | PopoverKind::Repo {
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
            ..
        } => Some(DIALOG_640_WIDTH),
        PopoverKind::Repo {
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::TrustConfirm),
            ..
        } => Some(DIALOG_460_WIDTH),
        PopoverKind::Repo {
            kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::ChangePointerPrompt { .. }),
            ..
        } => Some(DIALOG_420_WIDTH),
        PopoverKind::Repo {
            kind:
                RepoPopoverKind::Worktree(
                    WorktreePopoverKind::OpenPicker
                    | WorktreePopoverKind::RemovePicker
                    | WorktreePopoverKind::BadgePicker,
                ),
            ..
        }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Submodule(
                    SubmodulePopoverKind::OpenPicker | SubmodulePopoverKind::RemovePicker,
                ),
            ..
        }
        | PopoverKind::FileHistory { .. } => Some(LARGE_PICKER_WIDTH),
        PopoverKind::AppMenu => Some(APP_MENU_WIDTH),
        PopoverKind::AddRepoMenu => Some(DEFAULT_CONTEXT_MENU_WIDTH),
        PopoverKind::TerminalShutdownConfirm(_) | PopoverKind::UnsavedFileEditsConfirm(_) => {
            Some(DIALOG_440_WIDTH)
        }
        PopoverKind::TerminalMenu { .. } => Some(DEFAULT_CONTEXT_MENU_WIDTH),
        PopoverKind::WebLinkMenu { .. } | PopoverKind::DiffActionMenu => {
            Some(DIFF_ACTION_MENU_WIDTH)
        }
        // Shares "Browse repository at this point" with the commit menu, and so
        // needs the same extra room.
        PopoverKind::CommitShaLinkMenu { .. } => Some(PopoverWidthSpec::range(300.0, 220.0, 400.0)),
        // "Browse repository at this point" needs more room than the default
        // context-menu width.
        PopoverKind::CommitMenu { .. } => Some(PopoverWidthSpec::range(300.0, 220.0, 400.0)),
        // Resolver settings have substantially longer labels than diff actions.
        // A dedicated preferred width also feeds the shared anchor-side chooser,
        // allowing the menu to flip toward the side where the full label fits.
        PopoverKind::MergetoolSettingsMenu => Some(MERGETOOL_SETTINGS_MENU_WIDTH),
        PopoverKind::PullPicker
        | PopoverKind::PushPicker
        | PopoverKind::CommitOptionsMenu { .. }
        | PopoverKind::PreviousCommitMessagesMenu { .. }
        | PopoverKind::TagMenu { .. }
        | PopoverKind::TagRefMenu { .. }
        | PopoverKind::PullRequestMenu { .. }
        | PopoverKind::StatusFileMenu { .. }
        | PopoverKind::BranchMenu { .. }
        | PopoverKind::BranchSectionMenu { .. }
        | PopoverKind::SubmoduleInnerDiffMenu { .. }
        | PopoverKind::Repo {
            kind: RepoPopoverKind::Remote(RemotePopoverKind::Menu { .. }),
            ..
        }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Worktree(
                    WorktreePopoverKind::SectionMenu | WorktreePopoverKind::Menu { .. },
                ),
            ..
        }
        | PopoverKind::Repo {
            kind:
                RepoPopoverKind::Submodule(
                    SubmodulePopoverKind::SectionMenu | SubmodulePopoverKind::Menu { .. },
                ),
            ..
        }
        | PopoverKind::CommitFileMenu { .. }
        | PopoverKind::FileBrowserFileMenu { .. }
        | PopoverKind::FileBrowserFolderMenu { .. }
        | PopoverKind::BranchGroupMenu { .. }
        | PopoverKind::PinnedSectionMenu { .. }
        | PopoverKind::ReflogEntryMenu { .. }
        | PopoverKind::BrowseHistoryMenu { .. } => Some(DEFAULT_CONTEXT_MENU_WIDTH),
        PopoverKind::RepoTabMenu { .. } => Some(REPO_TAB_MENU_WIDTH),
        PopoverKind::HistoryBranchFilter { .. }
        | PopoverKind::DiffContentModeSettings
        | PopoverKind::UiScalePicker
        | PopoverKind::DiffHunkMenu { .. } => Some(NARROW_CONTEXT_MENU_WIDTH),
        PopoverKind::HistoryAuthorFilter { .. } => Some(HISTORY_AUTHOR_FILTER_WIDTH),
        PopoverKind::HistoryRefFilter { .. } => Some(HISTORY_REF_FILTER_WIDTH),
        PopoverKind::ChangeTrackingSettings => Some(CHANGE_TRACKING_MENU_WIDTH),
        PopoverKind::DiffEditorMenu { .. } => Some(DIFF_EDITOR_MENU_WIDTH),
        PopoverKind::ConflictResolverInputRowMenu { .. } => Some(CONFLICT_INPUT_MENU_WIDTH),
        PopoverKind::ConflictResolverChunkMenu { .. } => Some(CONFLICT_CHUNK_MENU_WIDTH),
        PopoverKind::ConflictResolverOutputMenu { .. } => Some(CONFLICT_OUTPUT_MENU_WIDTH),
        PopoverKind::StashMenu { .. } => Some(STASH_MENU_WIDTH),
        PopoverKind::RebaseReword { .. } => Some(DIALOG_440_WIDTH),
        PopoverKind::InteractiveRebaseActionMenu { .. } => Some(REBASE_ACTION_MENU_WIDTH),
        PopoverKind::InteractiveRebaseAutosquashMenu => Some(REBASE_AUTOSQUASH_MENU_WIDTH),
    }
}

pub(super) fn popover_preferred_anchor_width(
    kind: &PopoverKind,
    ui_scale: ui_scale::UiScale,
) -> Pixels {
    popover_width_spec(kind)
        .map(|spec| spec.preferred_px(ui_scale).max(spec.min_px(ui_scale)))
        .unwrap_or_else(|| ui_scale.px(640.0))
}

pub(super) fn choose_popover_anchor_corner(
    anchor_corner: Anchor,
    space_left: Pixels,
    space_right: Pixels,
    preferred_width: Pixels,
) -> Anchor {
    match anchor_corner {
        Anchor::TopRight if space_left < preferred_width && space_right > space_left => {
            Anchor::TopLeft
        }
        Anchor::BottomRight if space_left < preferred_width && space_right > space_left => {
            Anchor::BottomLeft
        }
        Anchor::TopLeft if space_right < preferred_width && space_left > space_right => {
            Anchor::TopRight
        }
        Anchor::BottomLeft if space_right < preferred_width && space_left > space_right => {
            Anchor::BottomRight
        }
        _ => anchor_corner,
    }
}
