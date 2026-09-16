//! `PopoverHost` menu models: building an entry model and activating an entry.

use super::super::*;
use super::helpers::{
    CONTEXT_MENU_SUBMENU_INDENT_PX, context_menu_activate_entry_ix, context_menu_entry_action_at,
    context_menu_entry_debug_selector, context_menu_entry_tooltip, context_menu_shortcut_entry_ix,
    interactive_rebase_action_menu_model, interactive_rebase_autosquash_menu_model,
};

use super::{
    branch_group, branch_section, browse_history, change_tracking_settings, commit_file,
    commit_options, commit_sha_link, conflict_resolver_chunk, conflict_resolver_input_row,
    conflict_resolver_output, diff_actions, diff_content_mode_settings, diff_editor, diff_hunk,
    file_browser_file, file_browser_folder, history_branch_filter, mergetool_settings,
    pinned_section, previous_commit_messages, pull, pull_request, push, reflog_entry, remote,
    repo_picker_row, repo_tab, stash, status_file, submodule, submodule_inner_diff,
    submodule_section, tag, terminal, ui_scale_picker, web_link, worktree, worktree_section,
};
use crate::view::panels::popover::context_menu::branch;
use crate::view::panels::popover::context_menu::commit;
// @split-module: impl_menu
impl PopoverHost {
    /// The menu a repository row in the picker offers. Not reachable through
    /// [`Self::context_menu_model`] because it has no popover kind of its own:
    /// it is only ever floated over the picker by
    /// [`super::picker_row_menu`](crate::view::panels::popover), never opened as
    /// a popover in its own right.
    pub(in crate::view::panels::popover) fn repo_picker_row_menu_model(
        &self,
        entry: &repo_picker::RepoPickerEntry,
    ) -> ContextMenuModel {
        repo_picker_row::model(self, entry)
    }

    pub(in super::super::super) fn context_menu_model(
        &self,
        kind: &PopoverKind,
        cx: &gpui::Context<Self>,
    ) -> Option<ContextMenuModel> {
        match kind {
            PopoverKind::AppMenu => Some(app_menu::model(self)),
            PopoverKind::AddRepoMenu => Some(add_repo_menu::model()),
            PopoverKind::PullPicker => Some(pull::model(self)),
            PopoverKind::PushPicker => Some(push::model(self)),
            PopoverKind::CommitOptionsMenu { repo_id } => {
                Some(commit_options::model(self, *repo_id))
            }
            PopoverKind::PreviousCommitMessagesMenu { repo_id } => {
                Some(previous_commit_messages::model(self, *repo_id))
            }
            PopoverKind::RepoTabMenu { repo_id } => Some(repo_tab::model(self, *repo_id)),
            PopoverKind::CommitMenu { repo_id, commit_id } => {
                Some(commit::model(self, *repo_id, commit_id))
            }
            PopoverKind::ReflogEntryMenu {
                repo_id,
                target,
                selector,
            } => Some(reflog_entry::model(*repo_id, selector, target)),
            PopoverKind::TagMenu { repo_id, commit_id } => {
                Some(tag::model(self, *repo_id, commit_id))
            }
            PopoverKind::TagRefMenu {
                repo_id,
                commit_id,
                name,
            } => Some(tag::model_for_tag(self, *repo_id, commit_id, name)),
            PopoverKind::PullRequestMenu { repo_id, number } => {
                Some(pull_request::model(self, *repo_id, *number))
            }
            PopoverKind::StatusFileMenu {
                repo_id,
                area,
                path,
            } => Some(status_file::model(self, *repo_id, *area, path, cx)),
            PopoverKind::BranchMenu {
                repo_id,
                section,
                name,
            } => Some(branch::model(self, *repo_id, *section, name)),
            PopoverKind::BranchSectionMenu { repo_id, section } => {
                Some(branch_section::model(self, *repo_id, *section))
            }
            PopoverKind::Repo {
                repo_id,
                kind: RepoPopoverKind::Remote(RemotePopoverKind::Menu { name }),
            } => Some(remote::model(self, *repo_id, name)),
            PopoverKind::WebLinkMenu { url } => Some(web_link::model(url)),
            PopoverKind::CommitShaLinkMenu {
                repo_id,
                commit_id,
                allow_navigate,
            } => Some(commit_sha_link::model(*repo_id, commit_id, *allow_navigate)),
            PopoverKind::StashMenu {
                repo_id,
                index,
                message,
            } => Some(stash::model(*repo_id, *index, message)),
            PopoverKind::Repo {
                repo_id,
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::SectionMenu),
            } => Some(worktree_section::model(*repo_id)),
            PopoverKind::Repo {
                repo_id,
                kind: RepoPopoverKind::Worktree(WorktreePopoverKind::Menu { path, branch }),
            } => Some(worktree::model(*repo_id, path, branch.as_deref())),
            PopoverKind::Repo {
                repo_id,
                kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::SectionMenu),
            } => Some(submodule_section::model(*repo_id)),
            PopoverKind::Repo {
                repo_id,
                kind: RepoPopoverKind::Submodule(SubmodulePopoverKind::Menu { path }),
            } => Some(submodule::model(self, *repo_id, path)),
            PopoverKind::CommitFileMenu {
                repo_id,
                commit_id,
                path,
            } => Some(commit_file::model(self, *repo_id, commit_id, path)),
            PopoverKind::FileBrowserFileMenu { repo_id, path } => {
                Some(file_browser_file::model(self, *repo_id, path, cx))
            }
            PopoverKind::FileBrowserFolderMenu { repo_id, path } => {
                Some(file_browser_folder::model(self, *repo_id, path))
            }
            PopoverKind::BranchGroupMenu {
                repo_id,
                section,
                remote,
                path,
            } => Some(branch_group::model(
                self,
                *repo_id,
                *section,
                remote.as_deref(),
                path,
            )),
            PopoverKind::PinnedSectionMenu { repo_id, section } => {
                Some(pinned_section::model(self, *repo_id, *section))
            }
            PopoverKind::BrowseHistoryMenu { repo_id } => {
                Some(browse_history::model(self, *repo_id))
            }
            PopoverKind::SubmoduleInnerDiffMenu {
                repo_id,
                submodule_repo_path,
                target,
            } => Some(submodule_inner_diff::model(
                *repo_id,
                submodule_repo_path,
                target,
            )),
            PopoverKind::DiffHunkMenu { repo_id, src_ix } => {
                Some(diff_hunk::model(self, *repo_id, *src_ix))
            }
            PopoverKind::DiffEditorMenu {
                repo_id,
                area,
                path,
                hunk_patch,
                hunks_count,
                lines_patch,
                discard_lines_patch,
                lines_count,
                copy_text,
                copy_target,
            } => Some(diff_editor::model(
                *repo_id,
                *area,
                path,
                hunk_patch,
                *hunks_count,
                lines_patch,
                discard_lines_patch,
                *lines_count,
                copy_text,
                *copy_target,
            )),
            PopoverKind::ConflictResolverInputRowMenu {
                line_label,
                line_target,
                chunk_label,
                chunk_target,
            } => Some(conflict_resolver_input_row::model(
                line_label,
                line_target,
                chunk_label,
                chunk_target,
            )),
            PopoverKind::ConflictResolverChunkMenu {
                conflict_ix,
                has_base,
                is_three_way,
                selected_choices,
                output_line_ix,
                split_selection_rows,
                join_previous_region,
                join_next_region,
                alignment_marked_columns,
                has_manual_alignments,
                output_is_protected,
            } => Some(conflict_resolver_chunk::model(
                *conflict_ix,
                *has_base,
                *is_three_way,
                selected_choices,
                *output_line_ix,
                *split_selection_rows,
                join_previous_region.clone(),
                join_next_region.clone(),
                *alignment_marked_columns,
                *has_manual_alignments,
                *output_is_protected,
            )),
            PopoverKind::ConflictResolverOutputMenu {
                cursor_line,
                selected_text,
                has_source_a,
                has_source_b,
                has_source_c,
                is_three_way,
            } => Some(conflict_resolver_output::model(
                *cursor_line,
                selected_text,
                *has_source_a,
                *has_source_b,
                *has_source_c,
                *is_three_way,
            )),
            PopoverKind::HistoryBranchFilter { repo_id } => {
                Some(history_branch_filter::model(self, *repo_id))
            }
            PopoverKind::DiffActionMenu => Some(diff_actions::model(self)),
            PopoverKind::MergetoolSettingsMenu => Some(mergetool_settings::model(self, cx)),
            PopoverKind::DiffContentModeSettings => Some(diff_content_mode_settings::model(self)),
            PopoverKind::ChangeTrackingSettings => Some(change_tracking_settings::model(self)),
            PopoverKind::UiScalePicker => Some(ui_scale_picker::model(cx)),
            PopoverKind::InteractiveRebaseActionMenu {
                ix,
                can_squash,
                can_drop,
            } => {
                let pick_locked = self
                    .main_pane
                    .read_with(cx, |pane, _| pane.active_entry_pick_locked(*ix));
                Some(interactive_rebase_action_menu_model(
                    *ix,
                    *can_squash,
                    *can_drop,
                    pick_locked,
                ))
            }
            PopoverKind::InteractiveRebaseAutosquashMenu => {
                Some(interactive_rebase_autosquash_menu_model())
            }
            PopoverKind::TerminalMenu { repo_id, context } => {
                Some(terminal::model(*repo_id, *context, cx))
            }
            _ => None,
        }
    }

    pub(in crate::view) fn context_menu_activate_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let mut close_after_action = true;
        let mut restore_diff_panel_focus_after_action = false;
        match action {
            ContextMenuAction::AppMenu(action) => {
                app_menu::activate(self, action, window, cx);
                return;
            }
            ContextMenuAction::AddRepoMenu(action) => {
                add_repo_menu::activate(self, action, window, cx);
                return;
            }
            ContextMenuAction::SelectDiff { repo_id, target } => {
                self.store.dispatch(Msg::SelectDiff { repo_id, target });
            }
            ContextMenuAction::OpenFileContent {
                repo_id,
                source,
                path,
            } => {
                self.store.dispatch(Msg::OpenFileContent {
                    repo_id,
                    source,
                    path,
                });
            }
            ContextMenuAction::EditFile { repo_id, path } => {
                self.store.dispatch(Msg::OpenFileEditor { repo_id, path });
            }
            ContextMenuAction::DiscardFileEdits { repo_id, path } => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.discard_file_edits_for(repo_id, &path, cx);
                });
            }
            ContextMenuAction::BrowseRepositoryAtCommit { repo_id, commit_id } => {
                self.store
                    .dispatch(Msg::BrowseRepositoryAtCommit { repo_id, commit_id });
            }
            ContextMenuAction::RevealHistoryCommit { repo_id, commit_id } => {
                self.main_pane.update(cx, |main, cx| {
                    main.reveal_history_commit(
                        repo_id,
                        commit_id,
                        Some(worktree_core::domain::LogScope::AllBranches),
                        cx,
                    );
                });
            }
            ContextMenuAction::ResetBrowseToLive { repo_id } => {
                self.store.dispatch(Msg::ResetBrowseToLive { repo_id });
            }
            ContextMenuAction::ToggleFileBrowserDir { repo_id, path } => {
                self.store
                    .dispatch(Msg::ToggleFileBrowserDir { repo_id, path });
            }
            ContextMenuAction::CompareDirectory { repo_id, path } => {
                let range = self
                    .state
                    .repos
                    .iter()
                    .find(|r| r.id == repo_id)
                    .and_then(|repo| repo.diff_state.diff_target.clone())
                    .and_then(|target| match target {
                        worktree_core::domain::DiffTarget::CommitRange {
                            from_commit_id,
                            to_commit_id,
                            ..
                        } => Some((from_commit_id, to_commit_id)),
                        _ => None,
                    });
                match range {
                    Some((from_commit_id, to_commit_id)) => {
                        self.store.dispatch(Msg::RequestDirectoryDiff {
                            repo_id,
                            target: worktree_core::domain::DiffTarget::CommitRange {
                                from_commit_id,
                                to_commit_id,
                                path: Some(path),
                            },
                        });
                    }
                    None => {
                        self.push_toast(
                            components::ToastKind::Error,
                            crate::i18n::tr_str("palette.cmd.compare-directory-needs-range")
                                .to_string(),
                            cx,
                        );
                    }
                }
            }
            // The branch tree's collapse state is view-owned rather than a
            // store message, so these four go through the sidebar pane.
            ContextMenuAction::ToggleSidebarCollapseKey { collapse_key } => {
                self.sidebar_pane.update(cx, |pane, cx| {
                    pane.toggle_active_repo_collapse_key(collapse_key, cx);
                });
            }
            ContextMenuAction::SetSidebarCollapseKey {
                collapse_key,
                collapsed,
            } => {
                self.sidebar_pane.update(cx, |pane, cx| {
                    pane.set_active_repo_collapse_key(collapse_key, collapsed, cx);
                });
            }
            ContextMenuAction::SetBranchGroupCollapsedRecursive {
                section,
                remote,
                path,
                collapsed,
            } => {
                self.sidebar_pane.update(cx, |pane, cx| {
                    pane.set_branch_group_collapsed_recursive(section, remote, path, collapsed, cx);
                });
            }
            ContextMenuAction::UnpinAllBranches { repo_id, section } => {
                self.sidebar_pane.update(cx, |pane, cx| {
                    pane.unpin_all_branches(repo_id, section, cx);
                });
            }
            ContextMenuAction::ConfirmDeleteBranchGroup {
                repo_id,
                section,
                remote,
                path,
                group_label,
            } => {
                let names = branch_group::deletable_branches(
                    self,
                    repo_id,
                    section,
                    remote.as_deref(),
                    &path,
                );
                // The entry is disabled at zero, so this only fires if the group
                // emptied between the last repaint and the click.
                if names.is_empty() {
                    self.close_popover(cx);
                    return;
                }
                let anchor = self.popover_anchor_point();
                self.open_popover_at(
                    PopoverKind::DeleteBranchesConfirm {
                        repo_id,
                        section,
                        remote,
                        group_label,
                        names,
                    },
                    anchor,
                    window,
                    cx,
                );
                return;
            }
            ContextMenuAction::SetFileBrowserDirExpandedRecursive {
                repo_id,
                path,
                expanded,
            } => {
                self.store
                    .dispatch(Msg::SetFileBrowserDirExpandedRecursive {
                        repo_id,
                        path,
                        expanded,
                    });
            }
            ContextMenuAction::SelectConflictDiff { repo_id, path } => {
                self.store
                    .dispatch(Msg::SelectConflictDiff { repo_id, path });
            }
            ContextMenuAction::OpenFile { repo_id, path } => {
                let full_path = match self.resolve_workdir_path(repo_id, &path) {
                    Ok(path) => path,
                    Err(err) => {
                        self.push_toast(components::ToastKind::Error, err, cx);
                        self.close_popover(cx);
                        return;
                    }
                };

                if !full_path.exists() {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Path not found: {}", full_path.display()),
                        cx,
                    );
                } else if let Err(err) = self.open_path_default(&full_path) {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Failed to open: {err}"),
                        cx,
                    );
                }
            }
            ContextMenuAction::OpenFileLocation { repo_id, path } => {
                let full_path = match self.resolve_workdir_path(repo_id, &path) {
                    Ok(path) => path,
                    Err(err) => {
                        self.push_toast(components::ToastKind::Error, err, cx);
                        self.close_popover(cx);
                        return;
                    }
                };

                let fallback = self.workdir_for_repo(repo_id);
                self.reveal_path_in_file_manager(full_path, fallback, cx);
            }
            ContextMenuAction::OpenRepositoryLocation { path } => {
                self.reveal_path_in_file_manager(path, None, cx);
            }
            ContextMenuAction::OpenInCodeEditor { repo_id, path } => {
                let full_path = match repo_id {
                    Some(repo_id) => match self.resolve_workdir_path(repo_id, &path) {
                        Ok(path) => path,
                        Err(err) => {
                            self.push_toast(components::ToastKind::Error, err, cx);
                            self.close_popover(cx);
                            return;
                        }
                    },
                    None => path,
                };

                if !full_path.exists() {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Path not found: {}", full_path.display()),
                        cx,
                    );
                } else if let Err(err) =
                    crate::external_editor::launch_configured_editor(&full_path)
                {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Failed to open in code editor: {err}"),
                        cx,
                    );
                }
            }
            ContextMenuAction::OpenInDetectedEditor {
                repo_id,
                path,
                id,
                editor_path,
            } => {
                let full_path = match repo_id {
                    Some(repo_id) => match self.resolve_workdir_path(repo_id, &path) {
                        Ok(path) => path,
                        Err(err) => {
                            self.push_toast(components::ToastKind::Error, err, cx);
                            self.close_popover(cx);
                            return;
                        }
                    },
                    None => path,
                };
                let setting = worktree_state::session::ExternalCodeEditorSetting::Detected {
                    id,
                    path: editor_path,
                };

                if !full_path.exists() {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Path not found: {}", full_path.display()),
                        cx,
                    );
                } else if let Err(err) = crate::external_editor::launch_editor(&setting, &full_path)
                {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!(
                            "Failed to open in {}: {err}",
                            crate::external_editor::label_for_setting(Some(&setting))
                        ),
                        cx,
                    );
                }
            }
            ContextMenuAction::OpenRepo { path } => {
                self.store.dispatch(Msg::OpenRepo(path));
            }
            ContextMenuAction::ActivateRepo { repo_id } => {
                if !self.repo_is_open(repo_id) {
                    self.warn_repository_gone(cx);
                    return;
                }
                self.store.dispatch(Msg::SetActiveRepo { repo_id });
            }
            ContextMenuAction::CloseRepo { repo_id } if !self.repo_is_open(repo_id) => {
                // The row this came from went stale — a concurrent close from a
                // repo tab, say. Dispatching would be a no-op the user cannot
                // see, and the menu is already down by now.
                self.warn_repository_gone(cx);
                return;
            }
            ContextMenuAction::CloseRepo { repo_id } => {
                // The reducer records the close as a recent; this keeps the
                // repository picker's own snapshot of that list in step, cap
                // included, for the frames before it next reads the session.
                if let Some(workdir) = self.workdir_for_repo(repo_id) {
                    session::promote_recent_repo(
                        &mut self.repo_picker.cached_recent_repos,
                        &workdir,
                    );
                }
                self.store.dispatch(Msg::CloseRepo { repo_id });
            }
            ContextMenuAction::PinRepository { path } => {
                let _ = session::persist_pinned_repo(&path);
                if !self.repo_picker.cached_pinned_repos.contains(&path) {
                    self.repo_picker.cached_pinned_repos.push(path);
                }
                // Pinning is bookkeeping, not navigation: the menu that offered
                // it goes, but the list it was over stays.
                close_after_action = false;
            }
            ContextMenuAction::UnpinRepository { path } => {
                let _ = session::remove_pinned_repo(&path);
                self.repo_picker
                    .cached_pinned_repos
                    .retain(|pin| pin != &path);
                close_after_action = false;
            }
            ContextMenuAction::ForgetRecentRepository { path } => {
                // Open and pinned repositories have no entry for this, and the
                // guard keeps it that way: a pin is what keeps a closed
                // repository listed, so forgetting one would strand it.
                if !self.repo_picker.cached_pinned_repos.contains(&path) {
                    let _ = session::remove_recent_repo(&path);
                    self.repo_picker
                        .cached_recent_repos
                        .retain(|recent| recent != &path);
                }
                close_after_action = false;
            }
            ContextMenuAction::CloseRepos {
                repo_ids,
                activate_after,
            } => {
                self.store.dispatch(Msg::CloseRepos {
                    repo_ids,
                    activate_after,
                });
            }
            ContextMenuAction::OpenSubmoduleDiffInTab { path, target } => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.open_submodule_inner_diff(path, target, cx);
                });
            }
            ContextMenuAction::ExportPatch { repo_id, commit_id } => {
                cx.stop_propagation();
                let view = cx.weak_entity();
                let sha = commit_id.as_ref();
                let short = sha.get(0..8).unwrap_or(sha).to_string();
                let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some(crate::i18n::tr("ui.prompt.export_patch_folder")),
                });
                window
                    .spawn(cx, async move |cx| {
                        let result = rx.await;
                        let paths = match result {
                            Ok(Ok(Some(paths))) => paths,
                            Ok(Ok(None)) => return,
                            Ok(Err(_)) | Err(_) => return,
                        };
                        let Some(folder) = paths.into_iter().next() else {
                            return;
                        };
                        let dest = folder.join(format!("commit-{short}.patch"));
                        let _ = view.update(cx, |this, cx| {
                            this.store.dispatch(Msg::ExportPatch {
                                repo_id,
                                commit_id: commit_id.clone(),
                                dest,
                            });
                            cx.notify();
                        });
                    })
                    .detach();
                self.close_popover(cx);
                return;
            }
            ContextMenuAction::ArchiveZip {
                repo_id,
                revision,
                suggested_name,
            } => {
                cx.stop_propagation();
                // The platform save dialog doubles as the confirmation step:
                // cancelling it cancels the export. It starts in the repo's
                // workdir with the suggested archive name pre-filled.
                let Some(workdir) = self
                    .state
                    .repos
                    .iter()
                    .find(|repo| repo.id == repo_id)
                    .map(|repo| repo.spec.workdir.clone())
                else {
                    return;
                };
                let view = cx.weak_entity();
                let rx = cx.prompt_for_new_path(&workdir, Some(&suggested_name));
                window
                    .spawn(cx, async move |cx| {
                        let result = rx.await;
                        let path = match result {
                            Ok(Ok(Some(path))) => path,
                            Ok(Ok(None)) | Ok(Err(_)) | Err(_) => return,
                        };
                        // `--format=zip` decides the content; make the name
                        // agree when the dialog did not append an extension.
                        let mut dest = path;
                        if dest.extension().is_none() {
                            dest.set_extension("zip");
                        }
                        let _ = view.update(cx, |this, cx| {
                            this.store.dispatch(Msg::ArchiveZip {
                                repo_id,
                                revision,
                                dest,
                            });
                            cx.notify();
                        });
                    })
                    .detach();
                self.close_popover(cx);
                return;
            }
            ContextMenuAction::CheckoutCommit { repo_id, commit_id } => {
                self.store
                    .dispatch(Msg::CheckoutCommit { repo_id, commit_id });
            }
            ContextMenuAction::BisectStartAt {
                repo_id,
                bad,
                goods,
            } => {
                self.store.dispatch(Msg::BisectStart {
                    repo_id,
                    bad,
                    goods,
                });
            }
            ContextMenuAction::BisectMarkCommit {
                repo_id,
                verdict,
                commit,
            } => {
                self.store.dispatch(Msg::BisectMark {
                    repo_id,
                    verdict,
                    commit: Some(commit),
                });
            }
            ContextMenuAction::MarkForComparison {
                repo_id,
                commit_id,
                label,
            } => {
                self.store.dispatch(Msg::MarkForComparison {
                    repo_id,
                    commit_id,
                    label,
                });
            }
            ContextMenuAction::CompareWithMarked {
                repo_id,
                commit_id,
                label,
            } => {
                self.store.dispatch(Msg::CompareWithMarked {
                    repo_id,
                    commit_id,
                    label,
                });
            }
            ContextMenuAction::CompareWithWorkingTree {
                repo_id,
                commit_id,
                label,
            } => {
                self.store.dispatch(Msg::CompareWithWorkingTree {
                    repo_id,
                    from: commit_id,
                    from_label: label,
                });
            }
            ContextMenuAction::ClearComparisonMark { repo_id } => {
                self.store.dispatch(Msg::ClearComparisonMark { repo_id });
            }
            ContextMenuAction::CherryPickCommit { repo_id, commit_id } => {
                let anchor = self.popover_anchor_point();
                self.open_popover_at(
                    PopoverKind::CherryPickCommitConfirm { repo_id, commit_id },
                    anchor,
                    window,
                    cx,
                );
                return;
            }
            ContextMenuAction::RevertCommit { repo_id, commit_id } => {
                self.store
                    .dispatch(Msg::RevertCommit { repo_id, commit_id });
            }
            ContextMenuAction::SquashSelectedCommits { repo_id } => {
                // PrepareSquash and the eventual SquashCommits are both
                // discarded silently when the git runtime is unavailable, which
                // would leave the prompt stuck on "Building combined message…".
                // Don't open it in that state.
                if !self.state.git_runtime.is_available() {
                    self.close_popover(cx);
                    return;
                }
                // Kick off the combined-message preview, then swap the menu
                // for the confirmation prompt.
                self.store.dispatch(Msg::PrepareSquash { repo_id });
                let anchor = self.popover_anchor_point();
                self.open_popover_at(PopoverKind::SquashPrompt { repo_id }, anchor, window, cx);
                return;
            }
            ContextMenuAction::CheckoutBranch { repo_id, name } => {
                self.store.dispatch(Msg::CheckoutBranch { repo_id, name });
            }
            ContextMenuAction::CheckoutPullRequest {
                repo_id,
                remote,
                number,
            } => {
                self.store.dispatch(Msg::CheckoutPullRequest {
                    repo_id,
                    remote,
                    number,
                });
            }
            ContextMenuAction::DeleteBranch { repo_id, name } => {
                let _ = self.root_view.update(cx, |root, _| {
                    root.pending_force_delete_branch_centered = false;
                });
                self.store.dispatch(Msg::DeleteBranch { repo_id, name });
            }
            ContextMenuAction::ToggleBranchPin {
                repo_id,
                section,
                name,
            } => {
                self.sidebar_pane.update(cx, |pane, cx| {
                    pane.toggle_pinned_branch(repo_id, section, &name, cx);
                });
            }
            ContextMenuAction::SetHistoryScope { repo_id, scope } => {
                self.store.dispatch(Msg::SetHistoryScope { repo_id, scope });
            }
            ContextMenuAction::SetDiffContentMode { mode } => {
                self.diff_content_mode = mode;
                let main_pane = self.main_pane.clone();
                cx.defer(move |cx| {
                    main_pane.update(cx, |pane, cx| {
                        pane.set_diff_content_mode_and_persist(mode, cx);
                    });
                });
            }
            ContextMenuAction::SetDiffWhitespaceMode { mode } => {
                close_after_action = false;
                restore_diff_panel_focus_after_action = true;
                self.diff_whitespace_mode = mode;
                let main_pane = self.main_pane.clone();
                cx.defer(move |cx| {
                    main_pane.update(cx, |pane, cx| {
                        pane.set_diff_whitespace_mode_and_persist(mode, cx);
                    });
                });
            }
            ContextMenuAction::SetDiffRevealWhitespaceChars { enabled } => {
                close_after_action = false;
                restore_diff_panel_focus_after_action = true;
                self.diff_reveal_whitespace_chars = enabled;
                let main_pane = self.main_pane.clone();
                cx.defer(move |cx| {
                    main_pane.update(cx, |pane, cx| {
                        pane.set_diff_reveal_whitespace_chars_and_persist(enabled, cx);
                    });
                });
            }
            ContextMenuAction::SetDiffWordWrap { enabled } => {
                close_after_action = false;
                restore_diff_panel_focus_after_action = true;
                self.diff_word_wrap = enabled;
                let main_pane = self.main_pane.clone();
                cx.defer(move |cx| {
                    main_pane.update(cx, |pane, cx| {
                        pane.set_diff_word_wrap_and_persist(enabled, cx);
                    });
                });
            }
            ContextMenuAction::SetDiffShowLineNumbers { enabled } => {
                close_after_action = false;
                restore_diff_panel_focus_after_action = true;
                self.diff_show_line_numbers = enabled;
                let main_pane = self.main_pane.clone();
                cx.defer(move |cx| {
                    main_pane.update(cx, |pane, cx| {
                        pane.set_diff_show_line_numbers_and_persist(enabled, cx);
                    });
                });
            }
            ContextMenuAction::SetChangeTrackingView { view } => {
                self.change_tracking_view = view;
                let root_view = self.root_view.clone();
                cx.defer(move |cx| {
                    let _ = root_view.update(cx, |root, cx| {
                        root.set_change_tracking_view(view, cx);
                    });
                });
            }
            ContextMenuAction::SetCommitAmendEnabled { enabled } => {
                close_after_action = false;
                self.commit_amend_enabled = enabled;
                let root_view = self.root_view.clone();
                cx.defer(move |cx| {
                    let _ = root_view.update(cx, |root, cx| {
                        root.set_commit_amend_enabled(enabled, cx);
                    });
                });
            }
            ContextMenuAction::SetCommitPushAfterEnabled { enabled } => {
                close_after_action = false;
                self.commit_push_after_enabled = enabled;
                let root_view = self.root_view.clone();
                cx.defer(move |cx| {
                    let _ = root_view.update(cx, |root, cx| {
                        root.set_commit_push_after_enabled(enabled, cx);
                    });
                });
            }
            ContextMenuAction::SetPushPullRetryEnabled { enabled } => {
                close_after_action = false;
                self.push_pull_retry_enabled = enabled;
                let root_view = self.root_view.clone();
                cx.defer(move |cx| {
                    let _ = root_view.update(cx, |root, cx| {
                        root.set_push_pull_retry_enabled(enabled, cx);
                    });
                });
            }
            ContextMenuAction::UseCommitMessage { message } => {
                self.details_pane.update(cx, |pane, cx| {
                    pane.set_commit_message_from_history(message, window, cx);
                });
            }
            ContextMenuAction::StageSelectionOrPath {
                repo_id,
                area,
                path,
            } => {
                // Staging is what marks a conflict resolved, so confirm first if
                // any of these files still has conflict markers in the worktree.
                // Resolved without consuming the selection, which the dialog
                // takes over responsibility for.
                let (paths, used_selection) =
                    self.status_paths_for_action(repo_id, area, &path, cx);
                if let Some(confirm) = crate::view::conflict_markers::stage_confirm_popover(
                    &self.state,
                    repo_id,
                    paths.clone(),
                    used_selection,
                ) {
                    let anchor = crate::view::conflict_markers::centered_dialog_anchor(window);
                    self.open_popover_at(confirm, anchor, window, cx);
                    return;
                }
                if used_selection {
                    self.clear_status_multi_selection(repo_id, cx);
                    self.store.dispatch(Msg::ClearDiffSelection { repo_id });
                    self.store.dispatch(Msg::StagePaths {
                        repo_id,
                        paths: paths.into(),
                    });
                } else {
                    self.store.dispatch(Msg::SelectDiff {
                        repo_id,
                        target: DiffTarget::WorkingTree {
                            path: path.clone(),
                            area,
                        },
                    });
                    self.store.dispatch(Msg::StagePath { repo_id, path });
                }
            }
            ContextMenuAction::UnstageSelectionOrPath {
                repo_id,
                area,
                path,
            } => {
                let (paths, used_selection) =
                    self.take_status_paths_for_action(repo_id, area, &path, cx);
                if used_selection {
                    self.store.dispatch(Msg::ClearDiffSelection { repo_id });
                    self.store.dispatch(Msg::UnstagePaths {
                        repo_id,
                        paths: paths.into(),
                    });
                } else {
                    self.store.dispatch(Msg::SelectDiff {
                        repo_id,
                        target: DiffTarget::WorkingTree {
                            path: path.clone(),
                            area,
                        },
                    });
                    self.store.dispatch(Msg::UnstagePath { repo_id, path });
                }
            }
            ContextMenuAction::DiscardWorktreeChangesSelectionOrPath {
                repo_id,
                area,
                path,
            } => {
                let anchor = self.popover_anchor_point();
                self.open_popover_at(
                    PopoverKind::DiscardChangesConfirm {
                        repo_id,
                        area,
                        path: Some(path),
                    },
                    anchor,
                    window,
                    cx,
                );
                return;
            }
            ContextMenuAction::AddToGitignoreSelectionOrPath {
                repo_id,
                area,
                path,
            } => {
                let anchor = self.popover_anchor_point();
                // Deliberately does not consume the row selection: the dialog
                // can still be cancelled, and `submit_add_to_gitignore` is what
                // clears it once the action is committed.
                self.open_popover_at(
                    PopoverKind::AddToGitignorePrompt {
                        repo_id,
                        area,
                        path,
                    },
                    anchor,
                    window,
                    cx,
                );
                return;
            }
            ContextMenuAction::StashSelectionOrPath {
                repo_id,
                area,
                path,
            } => {
                let anchor = self.popover_anchor_point();
                // Deliberately does not consume the row selection: the prompt
                // can still be cancelled, and `submit_stash` is the point of
                // no return. The prompt carries the resolved paths so it stays
                // correct even if the selection changes while it is open.
                let (paths, _) = self.status_paths_for_action(repo_id, area, &path, cx);
                self.open_popover_at(PopoverKind::StashPrompt { paths }, anchor, window, cx);
                return;
            }
            ContextMenuAction::CheckoutConflictSideSelectionOrPath {
                repo_id,
                area,
                path,
                side,
            } => {
                let (paths, _) = self.take_status_paths_for_action(repo_id, area, &path, cx);
                self.details_pane.update(cx, |pane, cx| {
                    pane.status_multi_selection.remove(&repo_id);
                    cx.notify();
                });
                self.store.dispatch(Msg::ClearDiffSelection { repo_id });
                for path in paths {
                    self.store.dispatch(Msg::CheckoutConflictSide {
                        repo_id,
                        path,
                        side,
                    });
                }
            }
            ContextMenuAction::LaunchMergetool { repo_id, path } => {
                // Snapshot the app-level preference here: the store worker
                // has no session state, so the Msg carries it explicitly.
                self.store.dispatch(Msg::LaunchMergetool {
                    repo_id,
                    path,
                    preference: worktree_core::external_merge_tool::current_external_merge_tool(),
                });
            }
            ContextMenuAction::SetAssumeUnchangedPath { repo_id, path } => {
                self.store.dispatch(Msg::SetAssumeUnchanged {
                    repo_id,
                    path,
                    enable: true,
                });
            }
            ContextMenuAction::FetchAll { repo_id } => {
                self.store.dispatch(Msg::FetchAll { repo_id });
            }
            ContextMenuAction::PruneMergedBranches { repo_id } => {
                self.store.dispatch(Msg::PruneMergedBranches { repo_id });
            }
            ContextMenuAction::PruneLocalTags { repo_id } => {
                self.store.dispatch(Msg::PruneLocalTags { repo_id });
            }
            ContextMenuAction::UpdateSubmodules { repo_id } => {
                self.store.dispatch(Msg::UpdateSubmodules { repo_id });
            }
            ContextMenuAction::LoadSubmodule { repo_id, path } => {
                self.store.dispatch(Msg::LoadSubmodule { repo_id, path });
            }
            ContextMenuAction::LoadWorktrees { repo_id } => {
                self.store.dispatch(Msg::LoadWorktrees { repo_id });
            }
            ContextMenuAction::Pull { repo_id, mode } => {
                self.store.dispatch(Msg::Pull { repo_id, mode });
            }
            ContextMenuAction::PullBranch {
                repo_id,
                remote,
                branch,
            } => {
                self.store.dispatch(Msg::PullBranch {
                    repo_id,
                    remote,
                    branch,
                });
            }
            ContextMenuAction::MergeRef { repo_id, reference } => {
                self.store.dispatch(Msg::MergeRef { repo_id, reference });
            }
            ContextMenuAction::SquashRef { repo_id, reference } => {
                self.store.dispatch(Msg::SquashRef { repo_id, reference });
            }
            ContextMenuAction::ApplyStash { repo_id, index } => {
                self.store.dispatch(Msg::ApplyStash { repo_id, index });
            }
            ContextMenuAction::PopStash { repo_id, index } => {
                self.store.dispatch(Msg::PopStash { repo_id, index });
            }
            ContextMenuAction::DropStashConfirm {
                repo_id,
                index,
                message,
            } => {
                let anchor = self.popover_anchor_point();
                self.open_popover_at(
                    PopoverKind::StashDropConfirm {
                        repo_id,
                        index,
                        message,
                    },
                    anchor,
                    window,
                    cx,
                );
                return;
            }
            ContextMenuAction::Push { repo_id } => {
                self.store.dispatch(Msg::Push {
                    repo_id,
                    pull_retry: self.push_pull_retry_enabled,
                });
            }
            ContextMenuAction::SetUpstreamBranch {
                repo_id,
                branch,
                upstream,
            } => {
                self.store.dispatch(Msg::SetUpstreamBranch {
                    repo_id,
                    branch,
                    upstream,
                });
            }
            ContextMenuAction::UnsetUpstreamBranch { repo_id, branch } => {
                self.store
                    .dispatch(Msg::UnsetUpstreamBranch { repo_id, branch });
            }
            ContextMenuAction::FastForwardBranch { repo_id, branch } => {
                self.store
                    .dispatch(Msg::FastForwardBranch { repo_id, branch });
            }
            ContextMenuAction::SetUiScale { percent } => {
                cx.defer(move |cx| {
                    crate::app::set_app_ui_scale_percent(cx, percent);
                });
            }
            ContextMenuAction::LoadInteractiveRebaseSetup { repo_id, base } => {
                self.store
                    .dispatch(Msg::LoadInteractiveRebaseSetup { repo_id, base });
            }
            ContextMenuAction::OpenInteractiveCherryPickSetup {
                repo_id,
                entries,
                source_colors,
            } => {
                self.store.dispatch(Msg::OpenInteractiveCherryPickSetup {
                    repo_id,
                    entries,
                    source_colors,
                });
            }
            ContextMenuAction::SetInteractiveRebaseAction { ix, action } => {
                let root_view = self.root_view.clone();
                let was_reword = action == InteractiveRebaseAction::Reword;
                let reword_state = if was_reword {
                    self.main_pane.read_with(cx, |pane, _| {
                        pane.active_irebase().and_then(|st| {
                            let action = st.entries.get(ix)?.action;
                            let msg = worktree_core::squash::reword_seed_message(&st.entries, ix);
                            Some((action, msg))
                        })
                    })
                } else {
                    None
                };
                self.main_pane.update(cx, |pane, cx| {
                    pane.set_rebase_action(ix, action, cx);
                });
                if let Some((original_action, msg)) = reword_state {
                    let wh = window.window_handle();
                    cx.defer(move |cx| {
                        let _ = wh.update(cx, |_, window, cx| {
                            let _ = root_view.update(cx, |root, cx| {
                                root.open_popover_centered(
                                    PopoverKind::RebaseReword {
                                        ix,
                                        original_action,
                                        original_message: msg,
                                    },
                                    window,
                                    cx,
                                );
                            });
                        });
                    });
                }
            }
            ContextMenuAction::SetInteractiveRebaseAutosquashMode { mode } => {
                let applied = self
                    .main_pane
                    .update(cx, |pane, cx| pane.apply_autosquash_mode(mode, cx));
                if !applied {
                    self.push_toast(
                        components::ToastKind::Warning,
                        "No automatic squashable commits found. Auto Squash searches for \
                         commits with identical messages and amend-commits them."
                            .to_string(),
                        cx,
                    );
                }
            }
            ContextMenuAction::OpenPopover { kind } => {
                let anchor = self.popover_anchor_point();
                self.open_popover_at(kind, anchor, window, cx);
                return;
            }
            ContextMenuAction::ConflictResolverPick { target } => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_apply_pick_target(target, cx);
                });
            }
            ContextMenuAction::ConflictResolverUnresolve { conflict_ix } => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_select_conflict(conflict_ix, cx);
                    pane.conflict_resolver_unresolve_active_conflict(cx);
                });
            }
            ContextMenuAction::ConflictResolverSplitSelection => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_split_selection(cx);
                });
            }
            ContextMenuAction::ConflictResolverAlignManually => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_align_manually(cx);
                });
            }
            ContextMenuAction::ConflictResolverClearManualAlignments => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_clear_manual_alignments(cx);
                });
            }
            ContextMenuAction::ConflictResolverJoinRegions { target } => {
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_join_regions(target, cx);
                });
            }
            ContextMenuAction::SetMergetoolAutoAdvance { enabled } => {
                close_after_action = false;
                self.main_pane.update(cx, |pane, cx| {
                    pane.set_mergetool_auto_advance_and_persist(enabled, cx);
                });
                cx.notify();
            }
            ContextMenuAction::ToggleMergetoolCollapseUnchanged => {
                close_after_action = false;
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_toggle_collapse_context(cx);
                });
                cx.notify();
            }
            ContextMenuAction::SetMergetoolOutputScrollSync { enabled } => {
                close_after_action = false;
                self.main_pane.update(cx, |pane, cx| {
                    pane.set_mergetool_output_scroll_sync_and_persist(enabled, cx);
                });
                cx.notify();
            }
            ContextMenuAction::SetMergetoolShowLineNumbers { enabled } => {
                close_after_action = false;
                self.main_pane.update(cx, |pane, cx| {
                    pane.set_mergetool_show_line_numbers_and_persist(enabled, cx);
                });
                cx.notify();
            }
            ContextMenuAction::SetMergetoolThreeWayView { enabled } => {
                close_after_action = false;
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_set_view_mode(
                        if enabled {
                            ConflictResolverViewMode::ThreeWay
                        } else {
                            ConflictResolverViewMode::TwoWayDiff
                        },
                        cx,
                    );
                });
                cx.notify();
            }
            ContextMenuAction::ConflictResolverOutputCut { text } => {
                crate::clipboard::write_text(cx, text, crate::clipboard::CopySource::ContextMenu);
                self.main_pane.update(cx, |pane, cx| {
                    pane.conflict_resolver_output_delete_selection(cx);
                });
            }
            ContextMenuAction::ConflictResolverOutputPaste => {
                if let Some(text) = crate::clipboard::read_text(cx) {
                    self.main_pane.update(cx, |pane, cx| {
                        pane.conflict_resolver_output_paste_text(&text, cx);
                    });
                }
            }
            ContextMenuAction::CopyText { text } => {
                window.activate_window();
                crate::clipboard::write_text(cx, text, crate::clipboard::CopySource::ContextMenu);
            }
            ContextMenuAction::CopyLinkAddress { url } => {
                window.activate_window();
                crate::clipboard::write_text(cx, url, crate::clipboard::CopySource::ContextMenu);
                self.push_toast(
                    components::ToastKind::Success,
                    crate::i18n::t!("toast.context_menu.link_copied").into_owned(),
                    cx,
                );
            }
            ContextMenuAction::CreateWebRequestPage => {
                let _ = self.root_view.update(cx, |root, cx| {
                    root.open_create_request_page(cx);
                });
            }
            ContextMenuAction::OpenWebUrl { url } => {
                if let Err(err) = crate::view::platform_open::open_url(&url) {
                    self.push_toast(
                        components::ToastKind::Error,
                        format!("Failed to open link: {err}"),
                        cx,
                    );
                }
            }
            ContextMenuAction::CopyDiffSelection { text } => {
                window.activate_window();
                crate::clipboard::write_text(
                    cx,
                    text,
                    crate::clipboard::CopySource::DiffContextMenu,
                );
            }
            ContextMenuAction::CopyDiffText { visible_ix, region } => {
                window.activate_window();
                self.main_pane.update(cx, |pane, cx| {
                    pane.copy_diff_text_for_context_menu_to_clipboard(visible_ix, region, cx);
                });
            }
            ContextMenuAction::TerminalCopy { repo_id } => {
                window.activate_window();
                let _ = self.root_view.update(cx, |root, cx| {
                    root.copy_terminal_selection_for_repo(repo_id, window, cx);
                });
            }
            ContextMenuAction::TerminalPaste { repo_id } => {
                let _ = self.root_view.update(cx, |root, cx| {
                    root.paste_terminal_clipboard_for_repo(repo_id, window, cx);
                });
            }
            ContextMenuAction::TerminalSelectAll { repo_id } => {
                let _ = self.root_view.update(cx, |root, cx| {
                    root.select_all_terminal_for_repo(repo_id, window, cx);
                });
            }
            ContextMenuAction::TerminalClear { repo_id } => {
                let _ = self.root_view.update(cx, |root, cx| {
                    root.clear_terminal_for_repo(repo_id, window, cx);
                });
            }
            ContextMenuAction::TerminalOpenExternal { repo_id } => {
                let _ = self.root_view.update(cx, |root, cx| {
                    root.open_external_terminal_from_menu(repo_id, window, cx);
                });
            }
            ContextMenuAction::ApplyIndexPatch {
                repo_id,
                patch,
                reverse,
            } => {
                if patch.trim().is_empty() {
                    self.push_toast(
                        components::ToastKind::Error,
                        crate::i18n::t!("toast.context_menu.patch_empty").into_owned(),
                        cx,
                    );
                } else if reverse {
                    self.store.dispatch(Msg::UnstageHunk { repo_id, patch });
                } else {
                    self.store.dispatch(Msg::StageHunk { repo_id, patch });
                }
            }
            ContextMenuAction::ApplyWorktreePatch {
                repo_id,
                patch,
                reverse,
            } => {
                if patch.trim().is_empty() {
                    self.push_toast(
                        components::ToastKind::Error,
                        crate::i18n::t!("toast.context_menu.patch_empty").into_owned(),
                        cx,
                    );
                } else {
                    self.store.dispatch(Msg::ApplyWorktreePatch {
                        repo_id,
                        patch,
                        reverse,
                    });
                }
            }
            ContextMenuAction::StageHunk { repo_id, src_ix } => {
                if let Some(patch) = self.build_unified_patch_for_hunk_src_ix(repo_id, src_ix) {
                    self.store.dispatch(Msg::StageHunk { repo_id, patch });
                } else {
                    self.push_toast(
                        components::ToastKind::Error,
                        crate::i18n::t!("toast.context_menu.patch_build_failed").into_owned(),
                        cx,
                    );
                }
            }
            ContextMenuAction::UnstageHunk { repo_id, src_ix } => {
                if let Some(patch) = self.build_unified_patch_for_hunk_src_ix(repo_id, src_ix) {
                    self.store.dispatch(Msg::UnstageHunk { repo_id, patch });
                } else {
                    self.push_toast(
                        components::ToastKind::Error,
                        crate::i18n::t!("toast.context_menu.patch_build_failed").into_owned(),
                        cx,
                    );
                }
            }
            ContextMenuAction::ExplainHunk { repo_id, src_ix } => {
                // start_ opens the explanation popover in the menu's place, so
                // it takes over the close-path; when it only warned (no patch,
                // or no configured source) the menu closes as any action's
                // aftermath does.
                if !crate::ai_commit::current().is_configured() {
                    self.push_toast(
                        components::ToastKind::Warning,
                        crate::i18n::t!("misc.ai_commit.not_configured").into_owned(),
                        cx,
                    );
                } else if self.start_hunk_explanation(repo_id, src_ix, window, cx) {
                    return;
                }
            }
            ContextMenuAction::DeleteTag { repo_id, name } => {
                self.store.dispatch(Msg::DeleteTag { repo_id, name });
            }
            ContextMenuAction::PushTag {
                repo_id,
                remote,
                name,
            } => {
                self.store.dispatch(Msg::PushTag {
                    repo_id,
                    remote,
                    name,
                });
            }
            ContextMenuAction::DeleteRemoteTag {
                repo_id,
                remote,
                name,
            } => {
                self.store.dispatch(Msg::DeleteRemoteTag {
                    repo_id,
                    remote,
                    name,
                });
            }
        }
        // A menu floating over a picker is not the popover: it closed itself on
        // the way in, and whether the picker underneath survives is the picker's
        // call, not the action's.
        if close_after_action && !self.suppress_popover_close_after_action {
            self.close_popover_and_restore_focus(window, cx);
        } else {
            if restore_diff_panel_focus_after_action {
                let focus = self.main_pane.read(cx).diff_panel_focus_handle.clone();
                window.focus(&focus, cx);
            }
            cx.notify();
        }
    }

    fn context_menu_activate_model_entry(
        &mut self,
        rows: &ContextMenuRows,
        ix: usize,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        match rows.get(ix) {
            Some((ContextMenuItem::Submenu { id, .. }, _)) => {
                self.toggle_context_menu_submenu(id.clone());
                cx.notify();
            }
            _ => {
                if let Some(action) = context_menu_entry_action_at(rows, ix) {
                    self.context_menu_activate_action(action, window, cx);
                }
            }
        }
    }

    /// Open or close one submenu group. Row positions shift with the group's
    /// children, so the keyboard selection cannot survive the toggle.
    fn toggle_context_menu_submenu(&mut self, id: SharedString) {
        self.context_menu.context_menu_selected_ix = None;
        if !self.context_menu.context_menu_open_submenus.remove(&id) {
            self.context_menu.context_menu_open_submenus.insert(id);
        }
    }

    pub(in crate::view::panels::popover) fn context_menu_view(
        &mut self,
        kind: PopoverKind,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let theme = self.theme;
        let ui_scale = super::popover_ui_scale(cx);
        let width = super::popover_width_spec(&kind).unwrap_or(super::DEFAULT_CONTEXT_MENU_WIDTH);
        let model = self
            .context_menu_model(&kind, cx)
            .unwrap_or_else(|| ContextMenuModel::new(vec![]));
        let tooltip_host = self.tooltip_host.clone();
        let entry_tooltips = model.entry_tooltips.clone();
        let entry_debug_selectors = model.entry_debug_selectors.clone();
        let shortcut_keycaps = model.shortcut_keycaps;
        // Everything below indexes the flattened rows — submenu children only
        // exist as rows while their group is open.
        let rows =
            ContextMenuRows::from_model(&model, &self.context_menu.context_menu_open_submenus);
        let rows_for_keys = rows.clone();
        let rows_for_mouse = rows.clone();
        let open_submenus_render = self.context_menu.context_menu_open_submenus.clone();

        let focus = self.context_menu_focus_handle.clone();
        // No fallback highlight: the menu opens with nothing selected (like
        // native menus), and hovering a disabled entry parks the selection on
        // it, which renders as no highlight at all rather than jumping to the
        // first selectable row.
        let current_selected = self.context_menu.context_menu_selected_ix;
        let selected_for_render = current_selected.filter(|&ix| rows.is_selectable(ix));

        // Keep labels aligned across entries when only some of them (e.g. the
        // checked option) carry an icon; icon-less menus stay compact.
        let reserve_icon_column = rows.iter().any(|(item, _)| match item {
            ContextMenuItem::Entry { icon: Some(_), .. } => true,
            ContextMenuItem::Submenu { icon: Some(_), .. } => true,
            _ => false,
        });

        div()
            .flex()
            .flex_col()
            .items_stretch()
            .text_color(theme.colors.foreground.primary)
            .min_w(width.min_px(ui_scale))
            .max_w(width.max_px(ui_scale))
            .track_focus(&focus)
            .key_context("ContextMenu")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, window, cx| {
                    window.focus(&this.context_menu_focus_handle, cx);
                }),
            )
            .on_key_down(
                cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                    let key = e.keystroke.key.as_str();
                    let mods = e.keystroke.modifiers;
                    if mods.control || mods.platform || mods.alt || mods.function {
                        return;
                    }

                    match key {
                        "escape" => {
                            cx.stop_propagation();
                            this.close_popover_and_restore_focus(window, cx);
                        }
                        "up" => {
                            cx.stop_propagation();
                            let next = rows_for_keys
                                .next_selectable(this.context_menu.context_menu_selected_ix, -1);
                            this.context_menu.context_menu_selected_ix = next;
                            cx.notify();
                        }
                        "down" => {
                            cx.stop_propagation();
                            let next = rows_for_keys
                                .next_selectable(this.context_menu.context_menu_selected_ix, 1);
                            this.context_menu.context_menu_selected_ix = next;
                            cx.notify();
                        }
                        "tab" => {
                            cx.stop_propagation();
                            let direction = if mods.shift { -1 } else { 1 };
                            this.context_menu.context_menu_selected_ix = rows_for_keys
                                .next_selectable(
                                    this.context_menu.context_menu_selected_ix,
                                    direction,
                                );
                            cx.notify();
                        }
                        "home" => {
                            cx.stop_propagation();
                            this.context_menu.context_menu_selected_ix =
                                rows_for_keys.first_selectable();
                            cx.notify();
                        }
                        "end" => {
                            cx.stop_propagation();
                            this.context_menu.context_menu_selected_ix =
                                rows_for_keys.last_selectable();
                            cx.notify();
                        }
                        "enter" | "space" => {
                            let Some(ix) = context_menu_activate_entry_ix(
                                &rows_for_keys,
                                this.context_menu.context_menu_selected_ix,
                            ) else {
                                return;
                            };
                            cx.stop_propagation();
                            this.context_menu_activate_model_entry(&rows_for_keys, ix, window, cx);
                        }
                        _ => {
                            if let Some(ix) = context_menu_shortcut_entry_ix(&rows_for_keys, key) {
                                cx.stop_propagation();
                                this.context_menu_activate_model_entry(
                                    &rows_for_keys,
                                    ix,
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                }),
            )
            .children(
                rows.into_iter()
                    .enumerate()
                    .map(move |(ix, (item, depth))| {
                        let indent = |row: gpui::Stateful<gpui::Div>| {
                            row.when(depth > 0, |row| {
                                row.pl(
                                    ui_scale.px(CONTEXT_MENU_SUBMENU_INDENT_PX * f32::from(depth))
                                )
                            })
                        };
                        match item {
                            ContextMenuItem::Separator => {
                                components::context_menu_separator(theme, ui_scale)
                                    .id(("context_menu_sep", ix))
                                    .into_any_element()
                            }
                            ContextMenuItem::Header(title) => components::context_menu_header(
                                theme,
                                ui_scale,
                                title.localized(),
                                Some(tooltip_host.clone()),
                                cx,
                            )
                            .id(("context_menu_header", ix))
                            .into_any_element(),
                            ContextMenuItem::Description(text) => {
                                components::context_menu_description(
                                    theme,
                                    ui_scale,
                                    text.localized(),
                                    Some(tooltip_host.clone()),
                                    cx,
                                )
                                .id(("context_menu_description", ix))
                                .into_any_element()
                            }
                            ContextMenuItem::Label(text) => components::context_menu_label(
                                theme,
                                ui_scale,
                                text.localized(),
                                Some(tooltip_host.clone()),
                                cx,
                            )
                            .id(("context_menu_label", ix))
                            .into_any_element(),
                            ContextMenuItem::Segmented { label, segments } => {
                                // Same construction as the toolbar's Inline/Split style
                                // toggles: one bordered pill, dividers between segments,
                                // the active one filled.
                                let mut control = div()
                                    .id(("context_menu_segmented", ix))
                                    .flex()
                                    .items_center()
                                    .h(components::control_height(ui_scale))
                                    .rounded(px(theme.radii.row))
                                    .border_1()
                                    .border_color(theme.colors.stroke.default)
                                    .overflow_hidden()
                                    .p(px(1.0));
                                for (seg_ix, segment) in segments.into_iter().enumerate() {
                                    if seg_ix > 0 {
                                        control = control.child(
                                            div()
                                                .h_full()
                                                .w(px(1.0))
                                                .bg(theme.colors.stroke.default),
                                        );
                                    }
                                    let ContextMenuSegment {
                                        id,
                                        label,
                                        tooltip,
                                        selected,
                                        action,
                                    } = segment;
                                    let debug_selector = id.clone();
                                    let label = crate::i18n::tr_en(label.as_ref());
                                    let mut button = components::Button::new(id, label)
                                        .borderless()
                                        .style(components::ButtonStyle::Subtle)
                                        .selected(selected)
                                        .selected_bg(theme.colors.interaction.pressed_background)
                                        .on_click(theme, cx, move |this, _e, window, cx| {
                                            this.context_menu_activate_action(
                                                action.clone(),
                                                window,
                                                cx,
                                            );
                                        })
                                        .debug_selector(move || debug_selector.to_string());
                                    if let Some(tooltip) = tooltip {
                                        button = button.worktree_tooltip(
                                            theme,
                                            crate::i18n::tr_en(tooltip.as_ref()),
                                        );
                                    }
                                    control = control.child(button);
                                }
                                components::context_menu_label(
                                    theme,
                                    ui_scale,
                                    crate::i18n::tr_en(label.as_ref()),
                                    Some(tooltip_host.clone()),
                                    cx,
                                )
                                .id(("context_menu_segmented_row", ix))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(control)
                                .into_any_element()
                            }
                            ContextMenuItem::Entry {
                                label,
                                icon,
                                shortcut,
                                disabled,
                                action,
                            } => {
                                let selected = selected_for_render == Some(ix);
                                let debug_selector = entry_debug_selectors
                                    .get(&ix)
                                    .map(|selector| selector.to_string())
                                    .unwrap_or_else(|| {
                                        context_menu_entry_debug_selector(label.as_ref())
                                    });
                                let tooltip_text = entry_tooltips
                                    .get(&ix)
                                    .cloned()
                                    .or_else(|| context_menu_entry_tooltip(action.as_ref()));
                                let tooltip_host_for_move = tooltip_host.clone();
                                let tooltip_text_for_move = tooltip_text.clone();
                                let tooltip_host_for_hover = tooltip_host.clone();
                                let activate_on_left_release = rows_for_mouse.clone();
                                let activate_on_right_release = rows_for_mouse.clone();
                                let icon_slot = match icon {
                                    Some(icon) => components::ContextMenuIconSlot::Icon(icon),
                                    None if reserve_icon_column => {
                                        components::ContextMenuIconSlot::Reserved
                                    }
                                    None => components::ContextMenuIconSlot::None,
                                };
                                let row = components::ContextMenuEntry::new(
                                    ("context_menu_entry", ix),
                                    label,
                                )
                                .icon(icon_slot)
                                .shortcut(shortcut)
                                .shortcut_keycaps(shortcut_keycaps)
                                .selected(selected)
                                .disabled(disabled)
                                .tooltip_host(tooltip_host.clone())
                                .render(theme, ui_scale, cx)
                                .debug_selector(move || debug_selector.clone());
                                let row = indent(row);

                                row.on_mouse_move(cx.listener(
                                    move |this, event: &MouseMoveEvent, _w, cx| {
                                        this.context_menu.context_menu_selected_ix = Some(ix);
                                        if let Some(tooltip_text) = tooltip_text_for_move.as_ref() {
                                            let _ = tooltip_host_for_move.update(cx, |host, cx| {
                                                host.on_mouse_moved(event.position, cx);
                                                host.set_tooltip_text_if_changed(
                                                    Some(tooltip_text.clone()),
                                                    cx,
                                                );
                                            });
                                        }
                                        cx.notify();
                                    },
                                ))
                                .on_hover(cx.listener(move |this, hovering: &bool, _w, cx| {
                                    if *hovering {
                                        this.context_menu.context_menu_selected_ix = Some(ix);
                                        cx.notify();
                                    } else if let Some(tooltip_text) = tooltip_text.as_ref() {
                                        let _ = tooltip_host_for_hover.update(cx, |host, cx| {
                                            host.clear_tooltip_if_matches(tooltip_text, cx);
                                        });
                                    }
                                }))
                                .when(!disabled, |row| {
                                    row.on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.context_menu_activate_model_entry(
                                                &activate_on_left_release,
                                                ix,
                                                window,
                                                cx,
                                            );
                                        }),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Right,
                                        cx.listener(move |this, _e: &MouseUpEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.context_menu_activate_model_entry(
                                                &activate_on_right_release,
                                                ix,
                                                window,
                                                cx,
                                            );
                                        }),
                                    )
                                })
                                .into_any_element()
                            }
                            ContextMenuItem::Submenu {
                                id, label, icon, ..
                            } => {
                                let selected = selected_for_render == Some(ix);
                                let is_open = open_submenus_render.contains(&id);
                                let debug_selector =
                                    context_menu_entry_debug_selector(label.as_ref());
                                let icon_slot = match icon {
                                    Some(icon) => components::ContextMenuIconSlot::Icon(icon),
                                    None if reserve_icon_column => {
                                        components::ContextMenuIconSlot::Reserved
                                    }
                                    None => components::ContextMenuIconSlot::None,
                                };
                                let chevron = if is_open {
                                    "icons/chevron_down.svg"
                                } else {
                                    "icons/chevron_right.svg"
                                };
                                let toggle_on_left = rows_for_mouse.clone();
                                let toggle_on_right = rows_for_mouse.clone();
                                let row = indent(
                                    components::ContextMenuEntry::new(
                                        ("context_menu_submenu", ix),
                                        label,
                                    )
                                    .icon(icon_slot)
                                    .selected(selected)
                                    .trailing_icon(chevron)
                                    .tooltip_host(tooltip_host.clone())
                                    .render(theme, ui_scale, cx)
                                    .debug_selector(move || debug_selector.clone()),
                                );

                                row.on_mouse_move(cx.listener(
                                    move |this, _e: &MouseMoveEvent, _w, cx| {
                                        this.context_menu.context_menu_selected_ix = Some(ix);
                                        cx.notify();
                                    },
                                ))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(move |this, _e: &MouseUpEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.context_menu_activate_model_entry(
                                            &toggle_on_left,
                                            ix,
                                            _window,
                                            cx,
                                        );
                                    }),
                                )
                                .on_mouse_up(
                                    MouseButton::Right,
                                    cx.listener(move |this, _e: &MouseUpEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.context_menu_activate_model_entry(
                                            &toggle_on_right,
                                            ix,
                                            _window,
                                            cx,
                                        );
                                    }),
                                )
                                .into_any_element()
                            }
                        }
                    }),
            )
    }
}
