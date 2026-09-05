use super::*;

pub(super) fn popover_is_context_menu(kind: &PopoverKind) -> bool {
    matches!(
        kind,
        PopoverKind::AppMenu
            | PopoverKind::AddRepoMenu
            | PopoverKind::PullPicker
            | PopoverKind::PushPicker
            | PopoverKind::CommitOptionsMenu { .. }
            | PopoverKind::PreviousCommitMessagesMenu { .. }
            | PopoverKind::RepoTabMenu { .. }
            | PopoverKind::WebLinkMenu { .. }
            | PopoverKind::CommitShaLinkMenu { .. }
            | PopoverKind::DiffActionMenu
            | PopoverKind::InteractiveRebaseActionMenu { .. }
            | PopoverKind::InteractiveRebaseAutosquashMenu
            | PopoverKind::MergetoolSettingsMenu
            | PopoverKind::HistoryBranchFilter { .. }
            | PopoverKind::DiffContentModeSettings
            | PopoverKind::ChangeTrackingSettings
            | PopoverKind::UiScalePicker
            | PopoverKind::TerminalMenu { .. }
            | PopoverKind::DiffHunkMenu { .. }
            | PopoverKind::DiffEditorMenu { .. }
            | PopoverKind::ConflictResolverInputRowMenu { .. }
            | PopoverKind::ConflictResolverChunkMenu { .. }
            | PopoverKind::ConflictResolverOutputMenu { .. }
            | PopoverKind::CommitMenu { .. }
            | PopoverKind::ReflogEntryMenu { .. }
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
            | PopoverKind::StashMenu { .. }
            | PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(
                    WorktreePopoverKind::SectionMenu | WorktreePopoverKind::Menu { .. },
                ),
                ..
            }
            | PopoverKind::Repo {
                kind: RepoPopoverKind::Submodule(
                    SubmodulePopoverKind::SectionMenu | SubmodulePopoverKind::Menu { .. },
                ),
                ..
            }
            | PopoverKind::CommitFileMenu { .. }
            | PopoverKind::FileBrowserFileMenu { .. }
            | PopoverKind::FileBrowserFolderMenu { .. }
            | PopoverKind::BranchGroupMenu { .. }
            | PopoverKind::PinnedSectionMenu { .. }
            | PopoverKind::BrowseHistoryMenu { .. }
    )
}

pub(super) fn popover_is_confirm_dialog(kind: &PopoverKind) -> bool {
    matches!(
        kind,
        PopoverKind::StashDropConfirm { .. }
            | PopoverKind::ForcePushConfirm { .. }
            | PopoverKind::CherryPickCommitConfirm { .. }
            | PopoverKind::MergeCommitConfirm { .. }
            | PopoverKind::MergeAbortConfirm { .. }
            | PopoverKind::RebaseOntoConfirm { .. }
            | PopoverKind::RebaseReword { .. }
            | PopoverKind::ForceDeleteBranchConfirm { .. }
            | PopoverKind::DeleteBranchesConfirm { .. }
            | PopoverKind::ForceRemoveWorktreeConfirm { .. }
            | PopoverKind::DiscardChangesConfirm { .. }
            | PopoverKind::DiscardAllConfirm { .. }
            | PopoverKind::AddToGitignorePrompt { .. }
            | PopoverKind::StageConflictMarkersConfirm { .. }
            | PopoverKind::ResetPrompt { .. }
            | PopoverKind::UndoLastActionPrompt { .. }
            | PopoverKind::PullReconcilePrompt { .. }
            | PopoverKind::TerminalShutdownConfirm(_)
            | PopoverKind::UnsavedFileEditsConfirm(_)
            | PopoverKind::Repo {
                kind: RepoPopoverKind::Remote(RemotePopoverKind::RemoveConfirm { .. }),
                ..
            }
            | PopoverKind::Repo {
                kind: RepoPopoverKind::Remote(RemotePopoverKind::DeleteBranchConfirm { .. }),
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
    )
}

impl PopoverHost {
    pub(in crate::view) fn close_popover(&mut self, cx: &mut gpui::Context<Self>) {
        let dismissing_unsaved_prompt = self.showing_unsaved_file_edits_prompt();
        self.save_commit_prompt_draft(cx);
        self.clear_truncated_tooltip(cx);
        crate::view::tooltip::set_tooltips_suppressed_by_overlay(false, cx);
        self.popover = None;
        self.popover_anchor = None;
        self.context_menu.context_menu_selected_ix = None;
        self.context_menu.context_menu_open_submenus.clear();
        self.picker_row_menu = None;
        self.menu_invoker_focus = None;
        self.notify_fingerprint = 0;
        self.sync_titlebar_app_menu_state(cx);
        self.clear_active_context_menu_invoker(cx);
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                if dismissing_unsaved_prompt {
                    root.clear_pending_unsaved_file_edits_prompt(cx);
                }
                root.set_history_refs_hover_item_menu_open(false, cx);
            });
        });
        cx.notify();
    }

    pub(in crate::view) fn close_popover_and_restore_focus(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let menu_invoker_focus = self.menu_invoker_focus.take();
        let restore_diff_panel_focus = matches!(
            self.popover,
            Some(
                PopoverKind::ChangeTrackingSettings
                    | PopoverKind::DiffContentModeSettings
                    | PopoverKind::WebLinkMenu { .. }
                    | PopoverKind::DiffActionMenu
                    | PopoverKind::MergetoolSettingsMenu
                    | PopoverKind::DiffHunkMenu { .. }
                    | PopoverKind::DiffEditorMenu { .. }
            ) // A web link menu can also be opened from a commit message in the
              // details pane, and handing that click's focus to the diff panel would
              // move the keyboard somewhere the user never was.
        ) && self.popover_opened_from_diff_panel;
        self.close_popover(cx);
        if restore_diff_panel_focus {
            let focus = self.main_pane.read(cx).diff_panel_focus_handle.clone();
            window.focus(&focus, cx);
        } else if let Some(focus) = menu_invoker_focus {
            window.focus(&focus, cx);
        }
    }

    pub(in crate::view) fn is_open(&self) -> bool {
        self.popover.is_some()
    }

    pub(in crate::view) fn dismiss_prompt_popover(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.popover.as_ref().is_some_and(popover_is_confirm_dialog) {
            self.close_popover(cx);
            return;
        }
        match self.popover.as_ref() {
            Some(PopoverKind::CreateBranchFromRefPrompt { .. })
            | Some(PopoverKind::RenameBranchPrompt { .. })
            | Some(PopoverKind::StashPrompt { .. })
            | Some(PopoverKind::StashBranchPrompt { .. })
            | Some(PopoverKind::CommitPrompt { .. })
            | Some(PopoverKind::StashPickerPrompt { .. })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::ChangePointerPrompt { .. }),
                ..
            }) => self.dismiss_inline_popover(window, cx),
            Some(PopoverKind::CloneRepo)
            | Some(PopoverKind::CreateTagPrompt { .. })
            | Some(PopoverKind::SquashPrompt { .. })
            | Some(PopoverKind::CheckoutRemoteBranchPrompt { .. })
            | Some(PopoverKind::PushSetUpstreamPrompt { .. })
            | Some(PopoverKind::RepoSettingsPrompt { .. })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Remote(RemotePopoverKind::AddPrompt),
                ..
            })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Remote(RemotePopoverKind::EditUrlPrompt { .. }),
                ..
            })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { .. }),
                ..
            })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                ..
            })
            | Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
                ..
            }) => self.close_popover(cx),
            _ => {}
        }
    }

    pub(super) fn dismiss_prompt(
        &mut self,
        _: &crate::view::PopoverPromptDismiss,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.prompt_tab_navigation_enabled() {
            return;
        }

        self.dismiss_prompt_popover(window, cx);
        cx.stop_propagation();
    }

    pub(super) fn dismiss_inline_popover(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.save_commit_prompt_draft(cx);
        self.clear_truncated_tooltip(cx);
        self.popover = None;
        self.popover_anchor = None;
        self.clear_active_context_menu_invoker(cx);
        let root_view = self.root_view.clone();
        cx.defer(move |cx| {
            let _ = root_view.update(cx, |root, cx| {
                root.set_history_refs_hover_item_menu_open(false, cx);
            });
        });
        let focus = self.main_pane.read(cx).diff_panel_focus_handle.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn clear_truncated_tooltip(&self, cx: &mut gpui::Context<Self>) {
        let _ = self.tooltip_host.update(cx, |host, cx| {
            host.clear_tooltip(cx);
        });
    }

    pub(super) fn inline_branch_picker_active(&self) -> bool {
        matches!(
            self.popover,
            Some(PopoverKind::BranchPicker { .. })
                | Some(PopoverKind::CreateBranchFromRefPrompt {
                    source_selectable: true,
                    ..
                })
                | Some(PopoverKind::Repo {
                    kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                    ..
                })
        )
    }

    pub(super) fn handle_inline_branch_picker_escape(&mut self, cx: &mut gpui::Context<Self>) {
        match &self.popover {
            Some(PopoverKind::CreateBranchFromRefPrompt { .. }) => {
                self.branch_picker.branch_picker_selected_index = None;
                if let Some(input) = &self.branch_picker.branch_picker_search_input {
                    let target = self.create_branch.create_branch_source_target.clone();
                    let theme = self.theme;
                    input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text(target, cx);
                        cx.notify();
                    });
                }
                cx.notify();
            }
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                ..
            }) => {
                self.branch_picker.branch_picker_selected_index = None;
                if let Some(input) = &self.branch_picker.branch_picker_search_input {
                    let target = self.worktree_add.worktree_ref_source_target.clone();
                    let theme = self.theme;
                    input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text(target, cx);
                        cx.notify();
                    });
                }
                cx.notify();
            }
            _ => {
                self.close_popover(cx);
            }
        }
    }

    pub(super) fn handle_inline_branch_picker_select(
        &mut self,
        name: String,
        repo_id: RepoId,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        match &self.popover {
            Some(PopoverKind::CreateBranchFromRefPrompt { .. }) => {
                self.create_branch.create_branch_source_target = name;
                if let Some(input) = &self.branch_picker.branch_picker_search_input {
                    let theme = self.theme;
                    input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text(self.create_branch.create_branch_source_target.clone(), cx);
                        cx.notify();
                    });
                }
                self.branch_picker.branch_picker_selected_index = None;
                cx.defer_in(window, |this, window, cx| {
                    if matches!(
                        this.popover,
                        Some(PopoverKind::CreateBranchFromRefPrompt { .. })
                    ) {
                        let focus = this
                            .create_branch
                            .create_branch_input
                            .read_with(cx, |input, _| input.focus_handle());
                        window.focus(&focus, cx);
                        cx.notify();
                    }
                });
                cx.notify();
            }
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                ..
            }) => {
                self.worktree_add.worktree_ref_source_target = name;
                if let Some(input) = &self.branch_picker.branch_picker_search_input {
                    let theme = self.theme;
                    input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text(self.worktree_add.worktree_ref_source_target.clone(), cx);
                        cx.notify();
                    });
                }
                self.branch_picker.branch_picker_selected_index = None;
                // Hand focus to Add once the keystroke that picked the ref has
                // finished dispatching, so it cannot land on the button it just
                // moved to; `suppress_worktree_submit_after_ref_enter` covers
                // the same Enter until the next frame is on screen.
                cx.defer_in(window, |this, window, cx| {
                    if matches!(
                        this.popover,
                        Some(PopoverKind::Repo {
                            kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                            ..
                        })
                    ) {
                        let focus = if this.can_submit_worktree_add(cx) {
                            this.worktree_add.worktree_focus.submit.clone()
                        } else {
                            this.worktree_add
                                .worktree_path_input
                                .read_with(cx, |input, _| input.focus_handle())
                        };
                        window.focus(&focus, cx);
                        cx.notify();
                    }
                    cx.on_next_frame(window, |this, _window, cx| {
                        this.worktree_add.suppress_worktree_submit_after_ref_enter = false;
                        cx.notify();
                    });
                });
                cx.notify();
            }
            Some(PopoverKind::BranchPicker {
                purpose: BranchPickerPurpose::Delete,
            }) => {
                let is_centered = matches!(self.popover_anchor, Some(PopoverAnchor::Centered));
                let _ = self.root_view.update(cx, |root, _| {
                    root.pending_force_delete_branch_centered = is_centered;
                });
                self.store.dispatch(Msg::DeleteBranch { repo_id, name });
                self.close_popover(cx);
            }
            Some(PopoverKind::BranchPicker {
                purpose: BranchPickerPurpose::RebaseOnto,
            }) => {
                self.open_popover_centered(
                    PopoverKind::RebaseOntoConfirm {
                        repo_id,
                        onto: name,
                    },
                    window,
                    cx,
                );
            }
            Some(PopoverKind::BranchPicker {
                purpose: BranchPickerPurpose::Merge,
            }) => {
                // The branch context menu merges refs without a confirm; the
                // picker matches it. An unwanted merge is abortable.
                self.store.dispatch(Msg::MergeRef {
                    repo_id,
                    reference: name,
                });
                self.close_popover(cx);
            }
            _ => {
                self.store.dispatch(Msg::CheckoutBranch { repo_id, name });
                self.close_popover(cx);
            }
        }
    }

    pub(in crate::view) fn open_popover_at(
        &mut self,
        kind: PopoverKind,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.open_popover(kind, PopoverAnchor::Point(anchor), window, cx);
    }

    pub(in crate::view) fn open_popover_centered(
        &mut self,
        kind: PopoverKind,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.open_popover(kind, PopoverAnchor::Centered, window, cx);
    }

    pub(in crate::view) fn open_popover_for_bounds(
        &mut self,
        kind: PopoverKind,
        anchor_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.open_popover(kind, PopoverAnchor::Bounds(anchor_bounds), window, cx);
    }

    pub(super) fn request_lazy_popover_repo_data(&self, kind: &PopoverKind) {
        let repo_id = match kind {
            PopoverKind::TagMenu { repo_id, .. } | PopoverKind::TagRefMenu { repo_id, .. } => {
                Some(*repo_id)
            }
            PopoverKind::PreviousCommitMessagesMenu { repo_id } => Some(*repo_id),
            PopoverKind::CommitOptionsMenu { repo_id } => Some(*repo_id),
            PopoverKind::AssumeUnchangedManager { repo_id } => Some(*repo_id),
            PopoverKind::Statistics { repo_id } => Some(*repo_id),
            PopoverKind::UndoLastActionPrompt { repo_id } => Some(*repo_id),
            PopoverKind::BranchPicker { .. } => self.state.active_repo,
            _ => None,
        };
        let Some(repo_id) = repo_id else {
            return;
        };
        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            return;
        };

        if matches!(kind, PopoverKind::AssumeUnchangedManager { .. }) {
            // Rows come from the loaded list; a NotLoaded or failed load is
            // retried on open. A successful load stays — toggles reload it
            // through the action-finished effect, not through this path.
            if matches!(
                repo.assume_unchanged,
                Loadable::NotLoaded | Loadable::Error(_)
            ) {
                self.store.dispatch(Msg::LoadAssumeUnchanged { repo_id });
            }
            return;
        }

        if matches!(kind, PopoverKind::Statistics { .. }) {
            // The chart reads the loaded commit list; retry a failed load on
            // reopen, keep a successful one (it covers 400 days, so a stale
            // window only matters across month boundaries and the popover is
            // short-lived).
            if matches!(repo.statistics, Loadable::NotLoaded | Loadable::Error(_)) {
                self.store.dispatch(Msg::LoadRepoStatistics { repo_id });
            }
            return;
        }

        if matches!(kind, PopoverKind::UndoLastActionPrompt { .. }) {
            // The preview classifies the newest reflog entries; a missing or
            // failed reflog is (re)requested so the prompt can fill itself
            // in. A stale-but-loaded reflog still shows — undo targets the
            // recorded past, and the newest entry only changes when the user
            // just ran another operation, which reopens this prompt anyway.
            if matches!(repo.reflog, Loadable::NotLoaded | Loadable::Error(_)) {
                self.store.dispatch(Msg::LoadReflog { repo_id });
            }
            return;
        }

        if matches!(kind, PopoverKind::BranchPicker { .. }) {
            // Decorates the checkout picker's rows; load once, retry on error.
            if matches!(repo.ref_metadata, Loadable::NotLoaded | Loadable::Error(_)) {
                self.store.dispatch(Msg::LoadRefMetadata { repo_id });
            }
            // Remote branches arrive with the repo's normal refresh; the picker
            // just omits the Remote section until they do.
            return;
        }

        if matches!(
            kind,
            PopoverKind::PreviousCommitMessagesMenu { .. } | PopoverKind::CommitOptionsMenu { .. }
        ) {
            if matches!(
                repo.recent_commit_messages,
                Loadable::NotLoaded | Loadable::Error(_)
            ) {
                self.store
                    .dispatch(Msg::LoadRecentCommitMessages { repo_id, limit: 10 });
            }
            return;
        }

        if matches!(repo.tags, Loadable::NotLoaded | Loadable::Error(_)) {
            self.store.dispatch(Msg::LoadTags { repo_id });
        }
        if matches!(repo.remote_tags, Loadable::NotLoaded | Loadable::Error(_)) {
            self.store.dispatch(Msg::LoadRemoteTags { repo_id });
        }
    }

    pub(super) fn open_popover(
        &mut self,
        kind: PopoverKind,
        anchor: PopoverAnchor,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.save_commit_prompt_draft(cx);
        self.clear_truncated_tooltip(cx);
        // The anchor stays hovered behind the opened surface; keep its
        // tooltip from re-showing on top of the popover.
        crate::view::tooltip::set_tooltips_suppressed_by_overlay(true, cx);
        self.request_lazy_popover_repo_data(&kind);
        if matches!(&kind, PopoverKind::CherryPickCommitConfirm { .. }) {
            self.cherry_pick_mainline = None;
        }
        if matches!(&kind, PopoverKind::Statistics { .. }) {
            self.statistics_period = statistics::StatisticsPeriod::default();
        }
        if matches!(&kind, PopoverKind::UndoLastActionPrompt { .. }) {
            // Back to the plan's suggested mode; the user may have softened
            // it in an earlier visit to the prompt.
            self.undo_reset_mode = None;
        }
        if matches!(&kind, PopoverKind::HunkExplanation { .. }) {
            // Every open starts a fresh request; a previous answer (or error)
            // never carries over.
            self.hunk_explanation = None;
        }
        self.menu_invoker_focus =
            if matches!(&kind, PopoverKind::AppMenu | PopoverKind::AddRepoMenu) {
                window.focused(cx)
            } else {
                None
            };
        // The diff panel takes focus on any left press inside it, so its focus
        // state at open time is a faithful record of where the click landed.
        self.popover_opened_from_diff_panel = self
            .main_pane
            .read(cx)
            .diff_panel_focus_handle
            .is_focused(window);
        let is_context_menu = popover_is_context_menu(&kind);
        let keep_active_invoker = is_context_menu
            || matches!(
                &kind,
                PopoverKind::CreateBranchFromRefPrompt { .. }
                    | PopoverKind::RenameBranchPrompt { .. }
                    | PopoverKind::StashPrompt { .. }
                    | PopoverKind::CommitPrompt { .. }
                    | PopoverKind::StashPickerPrompt { .. }
                    // Opened from the AUTHOR column header, which stays
                    // highlighted while its dropdown is up.
                    | PopoverKind::HistoryAuthorFilter { .. }
                    // Same for the branch column's ref-filter funnel.
                    | PopoverKind::HistoryRefFilter { .. }
                    // Action-bar badges stay lit while their picker is open.
                    // Scoped to Checkout: the Delete picker is opened from the
                    // sidebar context menu, whose invoker must still be cleared.
                    | PopoverKind::BranchPicker {
                        purpose: BranchPickerPurpose::Checkout,
                    }
                    | PopoverKind::Repo {
                        kind: RepoPopoverKind::Worktree(WorktreePopoverKind::BadgePicker),
                        ..
                    }
            );
        if !keep_active_invoker {
            self.clear_active_context_menu_invoker(cx);
        }

        self.popover_anchor = Some(anchor);
        self.context_menu.context_menu_selected_ix = None;
        self.context_menu.context_menu_open_submenus.clear();
        self.repo_picker.repo_picker_selected_index = None;
        // Belongs with the reset above, not with the RepoPicker arm below: every
        // popover kind draws `row_menu_layer`, so a menu left over from a closed
        // picker would spread its occluding scrim over an unrelated popover.
        self.picker_row_menu = None;
        self.branch_picker.branch_picker_selected_index = None;
        self.worktree_picker.worktree_picker_selected_index = None;
        self.workspace_picker.workspace_picker_selected_index = None;
        self.upstream_picker.upstream_picker_selected_index = None;
        self.submodule_picker.submodule_picker_selected_index = None;
        self.remote_picker.remote_picker_selected_index = None;
        self.tag_picker.tag_picker_selected_index = None;
        self.commit_search_picker
            .commit_search_picker_selected_index = None;
        self.file_history.file_history_selected_index = None;
        self.history_author_filter
            .history_author_filter_selected_index = None;
        // Rows are keyed by the data they were built from, so a stale slot can
        // only be reused when that data is unchanged. Dropping them on open still
        // keeps the memory from outliving the picker that needed it.
        self.branch_picker.branch_picker_rows_cache.clear();
        self.workspace_picker.workspace_picker_rows_cache.clear();
        self.upstream_picker.upstream_picker_rows_cache.clear();
        self.repo_picker.repo_picker_rows_cache.clear();
        self.stash_picker.stash_picker_rows_cache.clear();
        self.file_history.file_history_rows_cache.clear();
        self.submodule_picker.submodule_picker_rows_cache.clear();
        self.remote_picker.remote_picker_rows_cache.clear();
        self.tag_picker.tag_picker_rows_cache.clear();
        self.commit_search_picker
            .commit_search_picker_rows_cache
            .clear();
        self.worktree_picker.worktree_picker_rows_cache.clear();
        self.branch_picker.branch_ref_rows_cache.clear();
        if is_context_menu {
            self.popover = Some(kind);
            self.context_menu.context_menu_selected_ix = self
                .popover
                .as_ref()
                .and_then(|kind| self.context_menu_model(kind, cx))
                .map(|m| {
                    ContextMenuRows::from_model(&m, &self.context_menu.context_menu_open_submenus)
                })
                .and_then(|rows| rows.first_selectable());
            window.focus(&self.context_menu_focus_handle, cx);
        } else {
            match &kind {
                PopoverKind::RepoPicker => {
                    let ui_session = session::load();
                    self.repo_picker.repo_picker_sort = repo_picker::sort_from_session(&ui_session);
                    self.repo_picker.cached_recent_repos = ui_session.recent_repos;
                    self.repo_picker.cached_pinned_repos = ui_session.pinned_repos;
                    self.repo_picker.cached_collapsed_picker_sections =
                        ui_session.repo_picker_collapsed_sections;
                    self.repo_picker.repo_picker_sort_menu_open = false;
                    let _ = self.ensure_repo_picker_search_input(window, cx);
                }
                PopoverKind::BranchPicker { .. } => {
                    let _ = self.ensure_branch_picker_search_input(window, cx);
                }
                PopoverKind::UpstreamPicker { .. } => {
                    let _ = self.ensure_upstream_picker_search_input(window, cx);
                }
                PopoverKind::RemotePicker { .. } => {
                    let _ = self.ensure_remote_picker_search_input(window, cx);
                }
                PopoverKind::DeleteTagPicker { .. } => {
                    let _ = self.ensure_tag_picker_search_input(window, cx);
                }
                PopoverKind::CommitSearchPicker { .. } => {
                    let _ = self.ensure_commit_search_picker_search_input(window, cx);
                }
                PopoverKind::CreateBranchFromRefPrompt {
                    source_selectable,
                    target,
                    name_prefix,
                    ..
                } => {
                    let theme = self.theme;
                    self.create_branch.create_branch_checkout_enabled = true;
                    self.create_branch.create_branch_source_target = target.clone();
                    if *source_selectable {
                        let _ = self.ensure_branch_picker_search_input(window, cx);
                        if let Some(input) = &self.branch_picker.branch_picker_search_input {
                            input.update(cx, |input, cx| {
                                input.set_text(target.clone(), cx);
                            });
                        }
                    }
                    let name_prefix = name_prefix.clone();
                    self.create_branch
                        .create_branch_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(name_prefix, cx);
                            cx.notify();
                        });
                    let focus = self
                        .create_branch
                        .create_branch_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::RenameBranchPrompt { name, .. } => {
                    let theme = self.theme;
                    self.create_branch
                        .create_branch_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(name.clone(), cx);
                            cx.notify();
                        });
                    let focus = self
                        .create_branch
                        .create_branch_input
                        .read_with(cx, |input, _| input.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::CheckoutRemoteBranchPrompt { branch, .. } => {
                    let theme = self.theme;
                    self.create_branch
                        .create_branch_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(branch.clone(), cx);
                            cx.notify();
                        });
                    let focus = self
                        .create_branch
                        .create_branch_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::StashPrompt { .. } => {
                    let theme = self.theme;
                    self.stash.stash_include_untracked = true;
                    self.stash.stash_keep_index = false;
                    self.stash.stash_message_input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text("", cx);
                        cx.notify();
                    });
                    let focus = self
                        .stash
                        .stash_message_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::MergeRequestPushPrompt { .. } => {
                    let theme = self.theme;
                    self.mr_push.mr_push_merge_when_pipeline_succeeds = false;
                    self.mr_push.mr_push_remove_source_branch = true;
                    self.mr_push.mr_push_push_to_mr_branch = false;
                    self.mr_push.mr_push_description_generating = false;
                    self.mr_push.mr_push_description_error = None;
                    self.mr_push
                        .mr_push_description_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    self.mr_push.mr_push_target_input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text("", cx);
                        cx.notify();
                    });
                    let focus = self
                        .mr_push
                        .mr_push_target_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::StashBranchPrompt { index, .. } => {
                    let theme = self.theme;
                    let suggested = format!("stash-{index}");
                    self.create_branch
                        .create_branch_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(suggested, cx);
                            cx.notify();
                        });
                    let focus = self
                        .create_branch
                        .create_branch_input
                        .read_with(cx, |input, _| input.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::CommitPrompt { repo_id } => {
                    let theme = self.theme;
                    let draft = self
                        .commit_prompt
                        .commit_prompt_message_drafts
                        .get(repo_id)
                        .cloned()
                        .unwrap_or_default();
                    self.commit_prompt
                        .commit_prompt_message_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(draft.to_string(), cx);
                            cx.notify();
                        });
                    self.commit_prompt
                        .commit_prompt_message_scroll
                        .set_offset(point(px(0.0), px(0.0)));
                    let focus = self
                        .commit_prompt
                        .commit_prompt_message_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::StashPickerPrompt { .. } => {
                    let _ = self.ensure_stash_picker_search_input(window, cx);
                    self.stash_picker.stash_picker_prompt_selected_index = Some(0);
                }
                PopoverKind::CloneRepo => {
                    let theme = self.theme;
                    let url_text = self
                        .clone_repo
                        .clone_repo_url_input
                        .read_with(cx, |i, _| i.text().to_string());
                    let parent_text = self
                        .clone_repo
                        .clone_repo_parent_dir_input
                        .read_with(cx, |i, _| i.text().to_string());
                    self.clone_repo
                        .clone_repo_url_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(url_text, cx);
                            cx.notify();
                        });
                    self.clone_repo
                        .clone_repo_parent_dir_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(parent_text, cx);
                            cx.notify();
                        });
                    let focus = self
                        .clone_repo
                        .clone_repo_url_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::SquashPrompt { .. } => {
                    let theme = self.theme;
                    self.squash.squash_prompt_prefilled_range = None;
                    self.squash.squash_message_input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text("", cx);
                        cx.notify();
                    });
                    self.squash
                        .squash_description_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    // The preview may already be Ready (e.g. reopening the same
                    // range); prefill immediately rather than waiting for the
                    // next model update.
                    self.sync_squash_prompt_prefill(cx);
                    let focus = self
                        .squash
                        .squash_message_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::CreateTagPrompt { .. } => {
                    let theme = self.theme;
                    self.create_tag.create_tag_annotated =
                        matches!(self.state.default_tag_type, DefaultTagType::Annotated);
                    self.create_tag.create_tag_input.update(cx, |input, cx| {
                        input.clear_transient_key_presses();
                        input.set_theme(theme, cx);
                        input.set_text("", cx);
                        cx.notify();
                    });
                    self.create_tag
                        .create_tag_message_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    let focus = self
                        .create_tag
                        .create_tag_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::AddPrompt),
                    ..
                } => {
                    let theme = self.theme;
                    self.remote_prompts
                        .remote_name_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    self.remote_prompts
                        .remote_url_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    let focus = self
                        .remote_prompts
                        .remote_name_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    repo_id: _,
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::SshKeyPrompt { .. }),
                } => {
                    let theme = self.theme;
                    self.remote_prompts
                        .remote_ssh_key_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    let focus = self
                        .remote_prompts
                        .remote_ssh_key_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    repo_id,
                    kind: RepoPopoverKind::Remote(RemotePopoverKind::EditUrlPrompt { name, .. }),
                } => {
                    let theme = self.theme;
                    let text = self
                        .state
                        .repos
                        .iter()
                        .find(|r| r.id == *repo_id)
                        .and_then(|r| match &r.remotes {
                            Loadable::Ready(remotes) => remotes
                                .iter()
                                .find(|remote| remote.name.as_str() == name.as_str())
                                .and_then(|remote| remote.url.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    self.remote_prompts
                        .remote_url_edit_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text(text, cx);
                            cx.notify();
                        });
                    let focus = self
                        .remote_prompts
                        .remote_url_edit_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    kind: RepoPopoverKind::Worktree(WorktreePopoverKind::AddPrompt),
                    ..
                } => {
                    let theme = self.theme;
                    let (path_prefill, reference_prefill) = self
                        .worktree_add
                        .pending_worktree_add_prefill
                        .take()
                        .unwrap_or_default();
                    self.worktree_add
                        .worktree_path_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text(path_prefill, cx);
                            cx.notify();
                        });
                    self.worktree_add.worktree_ref_source_target = reference_prefill.clone();
                    self.worktree_add.suppress_worktree_submit_after_ref_enter = false;
                    let ref_input = self.ensure_branch_picker_search_input(window, cx);
                    // `ensure_*` blanks the input, so the prefilled ref has to be
                    // written back afterwards or the box would read empty while
                    // submit still used the reference.
                    if !reference_prefill.is_empty() {
                        ref_input.update(cx, |input, cx| {
                            input.set_text(reference_prefill, cx);
                            cx.notify();
                        });
                    }
                    let focus = self
                        .worktree_add
                        .worktree_path_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    repo_id,
                    kind:
                        RepoPopoverKind::Worktree(
                            WorktreePopoverKind::OpenPicker | WorktreePopoverKind::RemovePicker,
                        ),
                } => {
                    let _ = self.ensure_worktree_picker_search_input(window, cx);
                    self.store
                        .dispatch(Msg::LoadWorktrees { repo_id: *repo_id });
                }
                PopoverKind::Repo {
                    repo_id,
                    kind: RepoPopoverKind::Worktree(WorktreePopoverKind::BadgePicker),
                } => {
                    let _ = self.ensure_workspace_picker_search_input(window, cx);
                    self.store
                        .dispatch(Msg::LoadWorktrees { repo_id: *repo_id });
                }
                PopoverKind::Repo {
                    kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::AddPrompt),
                    ..
                } => {
                    let theme = self.theme;
                    self.submodule_add.submodule_add_advanced_expanded = false;
                    self.submodule_add.submodule_force_enabled = false;
                    self.submodule_add
                        .submodule_url_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    self.submodule_add
                        .submodule_path_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    self.submodule_add
                        .submodule_branch_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    self.submodule_add
                        .submodule_name_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    let focus = self
                        .submodule_add
                        .submodule_url_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    kind:
                        RepoPopoverKind::Submodule(SubmodulePopoverKind::ChangePointerPrompt { .. }),
                    ..
                } => {
                    let theme = self.theme;
                    self.submodule_add
                        .submodule_ref_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text("", cx);
                            cx.notify();
                        });
                    let focus = self
                        .submodule_add
                        .submodule_ref_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::Repo {
                    kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::TrustConfirm),
                    ..
                } => {}
                PopoverKind::Repo {
                    repo_id,
                    kind:
                        RepoPopoverKind::Submodule(
                            SubmodulePopoverKind::OpenPicker | SubmodulePopoverKind::RemovePicker,
                        ),
                } => {
                    let _ = self.ensure_submodule_picker_search_input(window, cx);
                    self.store
                        .dispatch(Msg::LoadSubmodules { repo_id: *repo_id });
                }
                PopoverKind::FileHistory { repo_id, path, .. } => {
                    self.ensure_file_history_search_input(window, cx);
                    self.store.dispatch(Msg::LoadFileHistory {
                        repo_id: *repo_id,
                        path: path.clone(),
                        limit: 200,
                    });
                }
                PopoverKind::HistoryAuthorFilter { .. } => {
                    self.ensure_history_author_filter_search_input(window, cx);
                }
                PopoverKind::HistoryRefFilter { .. } => {
                    self.ensure_history_ref_filter_search_input(window, cx);
                }
                PopoverKind::RepoSettingsPrompt { repo_id } => {
                    let theme = self.theme;
                    // Read the snapshot now (the panel consumes it) and seed
                    // the drafts from the local overrides. `self.popover` is
                    // still the previous kind here, so resolve via the arm's.
                    let Some(workdir) = self
                        .state
                        .repos
                        .iter()
                        .find(|repo| repo.id == *repo_id)
                        .map(|repo| repo.spec.workdir.clone())
                    else {
                        // No repo to configure: skip the seeding but still
                        // let the popover open (the panel shows its own
                        // no-repository face).
                        let _ = &theme;
                        return;
                    };
                    let current = self.load_repo_settings_current(&workdir);
                    self.repo_settings
                        .repo_settings_user_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(current.user_name.clone().unwrap_or_default(), cx);
                            cx.notify();
                        });
                    self.repo_settings
                        .repo_settings_email_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(current.user_email.clone().unwrap_or_default(), cx);
                            cx.notify();
                        });
                    self.repo_settings.repo_settings_sign_commits = current.sign_commits;
                    self.repo_settings.repo_settings_error = None;
                    self.repo_settings.repo_settings_current = Some(current);
                    // Land in the first field, like every other prompt dialog.
                    let focus = self
                        .repo_settings
                        .repo_settings_user_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::PushSetUpstreamPrompt { repo_id, .. } => {
                    let theme = self.theme;
                    let current_text = self
                        .push_upstream
                        .push_upstream_branch_input
                        .read_with(cx, |i, _| i.text().to_string());
                    let text = self
                        .state
                        .repos
                        .iter()
                        .find(|r| r.id == *repo_id)
                        .and_then(|repo| match &repo.head_branch {
                            Loadable::Ready(head) if !head.is_empty() => Some(head.clone()),
                            _ => None,
                        })
                        .unwrap_or(current_text);
                    self.push_upstream
                        .push_upstream_branch_input
                        .update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text(text, cx);
                            cx.notify();
                        });
                    let focus = self
                        .push_upstream
                        .push_upstream_branch_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::RebaseReword {
                    ix: _,
                    original_action: _,
                    original_message,
                } => {
                    let theme = self.theme;
                    let (subject, body) = original_message
                        .split_once("\n\n")
                        .map(|(s, b)| (s.to_owned(), b.to_owned()))
                        .unwrap_or_else(|| (original_message.clone(), String::new()));
                    self.rebase_reword
                        .rebase_reword_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(subject, cx);
                            cx.notify();
                        });
                    self.rebase_reword
                        .rebase_reword_description_input
                        .update(cx, |input, cx| {
                            input.clear_transient_key_presses();
                            input.set_theme(theme, cx);
                            input.set_text(body, cx);
                            cx.notify();
                        });
                    self.rebase_reword
                        .rebase_reword_description_scroll
                        .set_offset(point(px(0.0), px(0.0)));
                    let focus = self
                        .rebase_reword
                        .rebase_reword_input
                        .read_with(cx, |i, _| i.focus_handle());
                    window.focus(&focus, cx);
                }
                PopoverKind::RebaseOntoConfirm { .. } => {
                    // Focus the primary (Rebase) button so Enter confirms and
                    // Tab/Esc still reach Cancel.
                    window.focus(&self.rebase_onto.rebase_onto_submit_focus_handle, cx);
                }
                // Must sit above the generic confirm-dialog arm below, which
                // would otherwise swallow it and park focus on the tab group
                // instead of the pattern field.
                PopoverKind::AddToGitignorePrompt {
                    repo_id,
                    area,
                    path,
                } => {
                    let (repo_id, area, path) = (*repo_id, *area, path.clone());
                    self.prepare_add_to_gitignore(repo_id, area, &path, window, cx);
                }
                k if popover_is_confirm_dialog(k) => {
                    window.focus(&self.prompt_tab_group_focus_handle, cx);
                }
                _ => {}
            }
            self.popover = Some(kind);
        }
        if let Some(popover) = self.popover.as_ref() {
            self.notify_fingerprint = fingerprint::notify_fingerprint(&self.state, popover);
        }
        self.sync_titlebar_app_menu_state(cx);
        cx.notify();
    }

    /// The search input of whichever picker is open, so a row menu floating over
    /// it can read the filter without knowing which picker it is over.
    pub(super) fn open_picker_search_input(&self) -> Option<&Entity<components::TextInput>> {
        match &self.popover {
            Some(PopoverKind::RepoPicker) => self.repo_picker.repo_picker_search_input.as_ref(),
            Some(PopoverKind::BranchPicker { .. }) => {
                self.branch_picker.branch_picker_search_input.as_ref()
            }
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::BadgePicker),
                ..
            }) => self.workspace_picker.workspace_picker_search_input.as_ref(),
            _ => None,
        }
    }

    /// The selection index of whichever picker is open. A row menu parks it while
    /// it is up — the arrow keys walk the menu then — and restores it on the way
    /// out. **Every picker kind that can host a row menu has to be here**; a
    /// missing arm parks the wrong picker's selection with nothing on screen to
    /// say so.
    pub(super) fn open_picker_selected_index(&mut self) -> Option<&mut Option<usize>> {
        match &self.popover {
            Some(PopoverKind::RepoPicker) => Some(&mut self.repo_picker.repo_picker_selected_index),
            Some(PopoverKind::BranchPicker { .. }) => {
                Some(&mut self.branch_picker.branch_picker_selected_index)
            }
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::BadgePicker),
                ..
            }) => Some(&mut self.workspace_picker.workspace_picker_selected_index),
            _ => None,
        }
    }

    pub(super) fn open_picker_selected_index_value(&self) -> Option<usize> {
        match &self.popover {
            Some(PopoverKind::RepoPicker) => self.repo_picker.repo_picker_selected_index,
            Some(PopoverKind::BranchPicker { .. }) => {
                self.branch_picker.branch_picker_selected_index
            }
            Some(PopoverKind::Repo {
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::BadgePicker),
                ..
            }) => self.workspace_picker.workspace_picker_selected_index,
            _ => None,
        }
    }
}
