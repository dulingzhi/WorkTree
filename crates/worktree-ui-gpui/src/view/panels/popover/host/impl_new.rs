//! `PopoverHost` construction.

use super::super::*;
use super::host::PopoverHost;
use super::kinds::{
    PopoverKind, RemotePopoverKind, RepoPopoverKind, SubmodulePopoverKind, WorktreePopoverKind,
};
use super::state::{
    BranchPickerState, CloneRepoState, CommitPromptState, CommitSearchPickerState,
    ContextMenuState, CreateBranchState, CreateTagState, FileHistoryState, GitignoreState,
    HistoryAuthorFilterState, HistoryRefFilterState, MrPushState, PushUpstreamState,
    RebaseOntoState, RebaseRewordState, RemotePickerState, RemotePromptsState, RepoPickerState,
    RepoSettingsState, SquashState, StashPickerState, StashState, SubmoduleAddState,
    SubmodulePickerState, TagPickerState, UpstreamPickerState, WorkspacePickerState,
    WorktreeAddState, WorktreePickerState,
};

// @split-module: impl_new
impl PopoverHost {
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

        // Focus handles and dialog focus pairs are created up front, in the same
        // order as before the split, because tab order follows handle creation
        // order; each domain builder below receives its handles as parameters.
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

        let mut prompt_input_subscriptions = Vec::new();
        let clone_repo = Self::new_clone_repo_state(
            clone_repo_focus,
            clone_repo_browse_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let repo_settings = Self::new_repo_settings_state(
            repo_settings_focus,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let rebase_onto = Self::new_rebase_onto_state(rebase_onto_submit_focus_handle, window, cx);
        let create_tag = Self::new_create_tag_state(
            create_tag_focus,
            create_tag_annotated_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let gitignore = Self::new_gitignore_state(window, cx);
        let squash = Self::new_squash_state(
            squash_cancel_focus_handle,
            squash_submit_focus_handle,
            window,
            cx,
        );
        let remote_prompts = Self::new_remote_prompts_state(
            remote_add_focus,
            remote_edit_focus,
            remote_ssh_key_focus,
            remote_ssh_key_clear_focus,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let create_branch = Self::new_create_branch_state(
            create_branch_from_ref_checkout_focus_handle,
            create_branch_from_ref_focus,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let stash = Self::new_stash_state(
            stash_focus,
            stash_include_untracked_focus_handle,
            stash_keep_index_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let mr_push = Self::new_mr_push_state(
            mr_push_pipeline_focus_handle,
            mr_push_remove_source_focus_handle,
            mr_push_mr_branch_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let commit_prompt = Self::new_commit_prompt_state(
            commit_prompt_focus,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let push_upstream = Self::new_push_upstream_state(
            push_upstream_focus,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let worktree_add = Self::new_worktree_add_state(
            worktree_focus,
            worktree_browse_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let submodule_add = Self::new_submodule_add_state(
            submodule_focus,
            submodule_advanced_focus_handle,
            submodule_force_focus_handle,
            window,
            cx,
            &mut prompt_input_subscriptions,
        );
        let rebase_reword = Self::new_rebase_reword_state(window, cx);

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
            picker_row_menu: None,
            picker_prompt_scroll: ScrollHandle::new(),
            suppress_popover_close_after_action: false,
            checkout_remote_branch_focus,
            stash_branch_focus,
            remote_picker: RemotePickerState::default(),
            history_ref_filter: HistoryRefFilterState::default(),
            workspace_picker: WorkspacePickerState::default(),
            branch_picker: BranchPickerState::default(),
            file_history: FileHistoryState::default(),
            upstream_picker: UpstreamPickerState::default(),
            stash_picker: StashPickerState::default(),
            submodule_picker: SubmodulePickerState::default(),
            commit_search_picker: CommitSearchPickerState::default(),
            repo_picker: RepoPickerState::default(),
            worktree_picker: WorktreePickerState::default(),
            tag_picker: TagPickerState::default(),
            history_author_filter: HistoryAuthorFilterState::default(),
            context_menu: ContextMenuState::default(),
            clone_repo,
            repo_settings,
            rebase_onto,
            create_tag,
            gitignore,
            squash,
            remote_prompts,
            create_branch,
            stash,
            mr_push,
            commit_prompt,
            push_upstream,
            worktree_add,
            submodule_add,
            rebase_reword,
        }
    }

    fn new_clone_repo_state(
        clone_repo_focus: DialogFocus,
        clone_repo_browse_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> CloneRepoState {
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

        for input in [
            &clone_repo_url_input,
            &clone_repo_parent_dir_input,
            &clone_ssh_key_input,
        ] {
            subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| matches!(this.popover, Some(PopoverKind::CloneRepo)),
                |this, _window, cx| this.submit_clone_repo(cx),
            ));
        }

        CloneRepoState {
            clone_repo_url_input,
            clone_repo_parent_dir_input,
            clone_ssh_key_input,
            clone_repo_browse_focus_handle,
            clone_repo_focus,
        }
    }

    fn new_repo_settings_state(
        repo_settings_focus: DialogFocus,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> RepoSettingsState {
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

        for input in [&repo_settings_user_input, &repo_settings_email_input] {
            subscriptions.push(Self::prompt_enter_subscription(
                input,
                window,
                cx,
                |this| matches!(this.popover, Some(PopoverKind::RepoSettingsPrompt { .. })),
                |this, window, cx| this.submit_repo_settings_open(window, cx),
            ));
        }

        RepoSettingsState {
            repo_settings_user_input,
            repo_settings_email_input,
            repo_settings_sign_commits: None,
            repo_settings_error: None,
            repo_settings_current: None,
            #[cfg(test)]
            repo_settings_test_loads: 0,
            repo_settings_focus,
        }
    }

    fn new_rebase_onto_state(
        rebase_onto_submit_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> RebaseOntoState {
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

        RebaseOntoState {
            rebase_onto_input,
            rebase_onto_submit_focus_handle,
        }
    }

    fn new_create_tag_state(
        create_tag_focus: DialogFocus,
        create_tag_annotated_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> CreateTagState {
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

        subscriptions.push(Self::prompt_enter_subscription(
            &create_tag_input,
            window,
            cx,
            |this| matches!(this.popover, Some(PopoverKind::CreateTagPrompt { .. })),
            |this, _window, cx| this.submit_create_tag(cx),
        ));

        CreateTagState {
            create_tag_input,
            create_tag_message_input,
            create_tag_message_scroll,
            create_tag_annotated: false,
            create_tag_annotated_focus_handle,
            create_tag_focus,
        }
    }

    fn new_gitignore_state(window: &mut Window, cx: &mut gpui::Context<Self>) -> GitignoreState {
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

        GitignoreState {
            gitignore_patterns_input,
            gitignore_patterns_scroll,
            gitignore_scope: worktree_core::gitignore::GitignoreScope::File,
            gitignore_suggestions: None,
            gitignore_paths: Vec::new(),
        }
    }

    fn new_squash_state(
        squash_cancel_focus_handle: FocusHandle,
        squash_submit_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> SquashState {
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

        SquashState {
            _squash_message_input_subscription: squash_message_input_subscription,
            _squash_description_input_subscription: squash_description_input_subscription,
            squash_message_input,
            squash_description_input,
            squash_description_scroll,
            squash_prompt_prefilled_range: None,
            squash_cancel_focus_handle,
            squash_submit_focus_handle,
        }
    }

    fn new_remote_prompts_state(
        remote_add_focus: DialogFocus,
        remote_edit_focus: DialogFocus,
        remote_ssh_key_focus: DialogFocus,
        remote_ssh_key_clear_focus: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> RemotePromptsState {
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

        for input in [&remote_name_input, &remote_url_input] {
            subscriptions.push(Self::prompt_enter_subscription(
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        RemotePromptsState {
            remote_name_input,
            remote_url_input,
            remote_url_edit_input,
            remote_ssh_key_input,
            remote_add_focus,
            remote_edit_focus,
            remote_ssh_key_clear_focus,
            remote_ssh_key_focus,
        }
    }

    fn new_create_branch_state(
        create_branch_from_ref_checkout_focus_handle: FocusHandle,
        create_branch_from_ref_focus: DialogFocus,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> CreateBranchState {
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        CreateBranchState {
            create_branch_input,
            create_branch_checkout_enabled: true,
            create_branch_source_target: String::new(),
            create_branch_from_ref_checkout_focus_handle,
            create_branch_from_ref_focus,
        }
    }

    fn new_stash_state(
        stash_focus: DialogFocus,
        stash_include_untracked_focus_handle: FocusHandle,
        stash_keep_index_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> StashState {
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

        subscriptions.push(Self::prompt_enter_subscription(
            &stash_message_input,
            window,
            cx,
            |this| matches!(this.popover, Some(PopoverKind::StashPrompt { .. })),
            |this, window, cx| this.submit_stash(window, cx),
        ));

        StashState {
            stash_message_input,
            stash_focus,
            stash_include_untracked: true,
            stash_keep_index: false,
            stash_include_untracked_focus_handle,
            stash_keep_index_focus_handle,
        }
    }

    fn new_mr_push_state(
        mr_push_pipeline_focus_handle: FocusHandle,
        mr_push_remove_source_focus_handle: FocusHandle,
        mr_push_mr_branch_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> MrPushState {
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        MrPushState {
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
        }
    }

    fn new_commit_prompt_state(
        commit_prompt_focus: DialogFocus,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> CommitPromptState {
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

        subscriptions.push(
            cx.observe(&commit_prompt_message_input, |this, _input, cx| {
                if matches!(this.popover, Some(PopoverKind::CommitPrompt { .. })) {
                    cx.notify();
                }
            }),
        );

        CommitPromptState {
            commit_prompt_message_drafts: FxHashMap::default(),
            commit_prompt_message_input,
            commit_prompt_message_scroll,
            commit_prompt_focus,
        }
    }

    fn new_push_upstream_state(
        push_upstream_focus: DialogFocus,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> PushUpstreamState {
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        PushUpstreamState {
            push_upstream_focus,
            push_upstream_branch_input,
        }
    }

    fn new_worktree_add_state(
        worktree_focus: DialogFocus,
        worktree_browse_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> WorktreeAddState {
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        WorktreeAddState {
            pending_worktree_add_prefill: None,
            worktree_ref_source_target: String::new(),
            suppress_worktree_submit_after_ref_enter: false,
            worktree_browse_focus_handle,
            worktree_focus,
            worktree_path_input,
            worktree_ref_input,
        }
    }

    fn new_submodule_add_state(
        submodule_focus: DialogFocus,
        submodule_advanced_focus_handle: FocusHandle,
        submodule_force_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        subscriptions: &mut Vec<gpui::Subscription>,
    ) -> SubmoduleAddState {
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

        subscriptions.push(Self::prompt_enter_subscription(
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

        for input in [
            &submodule_url_input,
            &submodule_path_input,
            &submodule_branch_input,
            &submodule_name_input,
        ] {
            subscriptions.push(Self::prompt_enter_subscription(
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

        SubmoduleAddState {
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
        }
    }

    fn new_rebase_reword_state(
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> RebaseRewordState {
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

        RebaseRewordState {
            rebase_reword_input,
            rebase_reword_description_input,
            rebase_reword_description_scroll,
        }
    }
}
