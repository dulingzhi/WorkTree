//! The diff pane toolbar: the controls cluster assembled from the
//! computed `DiffViewIntents` — content-mode menu, hunk and file
//! navigation, the inline/split toggle, edit/blame buttons, the
//! rendered-preview switch and the action-menu/close pair.
use super::*;

impl MainPaneView {
    pub(super) fn diff_controls(
        &mut self,
        intents: &DiffViewIntents,
        repo_id: Option<RepoId>,
        theme: AppTheme,
        ui_scale_percent: u32,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let (prev_file_btn, next_file_btn) = if show_diff_file_navigation(self.view_mode) {
            self.diff_prev_next_file_buttons(repo_id, intents.is_conflict_resolver, theme, cx)
        } else {
            (None, None)
        };

        let mut controls = div().flex().items_center().gap_1();
        if self.is_inline_submodule_diff_active()
            && let Some(repo_id) = repo_id
        {
            controls = controls.child(
                components::Button::new(
                    "inline_submodule_back",
                    crate::i18n::tr("diff.toolbar.back"),
                )
                .separated_end_slot(Self::diff_nav_hotkey_hint(theme, "Esc"))
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, cx| {
                    this.store
                        .dispatch(Msg::CloseInlineSubmoduleDiff { repo_id });
                    cx.notify();
                }),
            );
        }
        // Path-level reject for the agent compare: restore the file on
        // screen from the session baseline, inside the agent worktree.
        // Resolved through the root view because the session is keyed by
        // the main repo while this pane is showing the worktree's tab.
        let agent_restore = self.root_view.upgrade().and_then(|root| {
            let root_view = root.read(cx);
            let active_id = root_view.active_repo_id()?;
            let session = root_view
                .agent_sessions
                .values()
                .find(|session| session.worktree_repo_id == Some(active_id))?;
            let target = root_view
                .state
                .repos
                .iter()
                .find(|repo| repo.id == active_id)?
                .diff_state
                .diff_target
                .clone()?;
            let (worktree, path) = agent_workbench::agent_restore_context(session, Some(&target))?;
            Some((active_id, worktree, session.baseline.clone(), path))
        });
        if let Some((repo_id, worktree, baseline, path)) = agent_restore {
            controls = controls.child(
                components::Button::new(
                    "agent_restore_file",
                    crate::i18n::tr("chrome.agent.restore_file"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, cx| {
                    this.restore_agent_file_from_baseline(
                        repo_id,
                        worktree.clone(),
                        baseline.clone(),
                        path.clone(),
                        cx,
                    );
                }),
            );
        }
        if intents.is_conflict_resolver && intents.is_simple_conflict_strategy {
            controls = self.conflict_toolbar_simple_controls(
                controls,
                prev_file_btn,
                next_file_btn,
                theme,
            );
        } else if intents.is_conflict_resolver {
            controls = self.conflict_toolbar_full_controls(
                controls,
                prev_file_btn,
                next_file_btn,
                intents.conflict_rendered_preview_active,
                repo_id,
                &intents.conflict_target_path,
                theme,
                cx,
            );
        } else if !intents.is_file_preview && !intents.is_file_editor {
            let view_toggle_selected_bg = with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.26 } else { 0.20 },
            );
            let view_toggle_border = with_alpha(
                theme.colors.foreground.secondary,
                if theme.is_dark { 0.38 } else { 0.28 },
            );
            let view_toggle_divider = with_alpha(view_toggle_border, 0.90);

            if intents.supports_diff_content_toggle {
                let diff_mode_invoker: SharedString = "diff_content_mode_header".into();
                let diff_mode_active = self
                    .active_context_menu_invoker
                    .as_ref()
                    .is_some_and(|id| id == &diff_mode_invoker);
                let diff_mode_label = self.diff_content_mode.label();

                controls = controls.child(
                    div()
                        .id("diff_content_mode_header")
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_1()
                        .h(components::control_height(ui_scale_percent))
                        .rounded(px(theme.radii.row))
                        .when(diff_mode_active, |d| {
                            d.bg(theme.colors.interaction.pressed_background)
                        })
                        .hover(move |s| {
                            if diff_mode_active {
                                s.bg(theme.colors.interaction.pressed_background)
                            } else {
                                s.bg(with_alpha(theme.colors.interaction.hover_background, 0.55))
                            }
                        })
                        .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                        .cursor(CursorStyle::PointingHand)
                        .child(
                            div()
                                .min_w(px(0.0))
                                .line_clamp(1)
                                .whitespace_nowrap()
                                .text_sm()
                                .child(diff_mode_label),
                        )
                        .child(svg_icon(
                            "icons/chevron_down.svg",
                            theme.colors.foreground.secondary,
                            px(12.0),
                        ))
                        .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                            this.activate_context_menu_invoker(diff_mode_invoker.clone(), cx);
                            this.open_popover_at(
                                PopoverKind::DiffContentModeSettings,
                                e.position(),
                                window,
                                cx,
                            );
                        })),
                );
            }

            controls = controls.when_some(prev_file_btn, |d, btn| d.child(btn));

            if !intents.is_image_diff_view {
                let nav_entries = self.diff_nav_entries();
                let can_nav_prev = diff_navigation::diff_nav_prev_target(
                    &nav_entries,
                    self.diff_nav_prev_current_ix(),
                )
                .is_some();
                let can_nav_next = diff_navigation::diff_nav_next_target(
                    &nav_entries,
                    self.diff_nav_next_current_ix(),
                )
                .is_some();

                let prev_hunk_btn = components::Button::new("diff_prev_hunk", "")
                    .start_slot(svg_icon(
                        "icons/arrow_up.svg",
                        theme.colors.foreground.primary,
                        px(14.0),
                    ))
                    .style(components::ButtonStyle::Outlined)
                    .disabled(!can_nav_prev)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.diff_jump_prev();
                        cx.notify();
                    })
                    .worktree_tooltip(
                        theme,
                        crate::i18n::t!(
                            "diff.tooltip.prev_change",
                            shortcut = crate::view::shortcut_labels::alt_shortcut("Up")
                        )
                        .to_string()
                        .into(),
                    );

                let next_hunk_btn = components::Button::new("diff_next_hunk", "")
                    .start_slot(svg_icon(
                        "icons/arrow_down.svg",
                        theme.colors.foreground.primary,
                        px(14.0),
                    ))
                    .style(components::ButtonStyle::Outlined)
                    .disabled(!can_nav_next)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.diff_jump_next();
                        cx.notify();
                    })
                    .worktree_tooltip(
                        theme,
                        crate::i18n::t!(
                            "diff.tooltip.next_change",
                            shortcut = crate::view::shortcut_labels::alt_shortcut("Down")
                        )
                        .to_string()
                        .into(),
                    );

                let diff_inline_btn =
                    components::Button::new("diff_inline", crate::i18n::tr("diff.toolbar.inline"))
                        .borderless()
                        .rounded_left()
                        .style(components::ButtonStyle::Subtle)
                        .selected(self.diff_view == DiffViewMode::Inline)
                        .selected_bg(view_toggle_selected_bg)
                        .on_click(theme, cx, |this, _e, window, cx| {
                            this.set_diff_view_mode(DiffViewMode::Inline, cx);
                            this.restore_diff_panel_focus_after_toolbar_action(window, cx);
                            let root_view = this.root_view.clone();
                            cx.defer(move |cx| {
                                if let Some(root) = root_view.upgrade() {
                                    root.update(cx, |root, cx| {
                                        root.set_diff_view_mode(DiffViewMode::Inline, cx);
                                    });
                                }
                            });
                            cx.notify();
                        })
                        .debug_selector(|| "diff_inline".to_string())
                        .worktree_tooltip(
                            theme,
                            crate::i18n::t!(
                                "diff.tooltip.inline_view",
                                shortcut = crate::view::shortcut_labels::alt_shortcut("I")
                            )
                            .to_string()
                            .into(),
                        );

                let diff_split_btn =
                    components::Button::new("diff_split", crate::i18n::tr("diff.toolbar.split"))
                        .borderless()
                        .rounded_right()
                        .style(components::ButtonStyle::Subtle)
                        .selected(self.diff_view == DiffViewMode::Split)
                        .selected_bg(view_toggle_selected_bg)
                        .on_click(theme, cx, |this, _e, window, cx| {
                            this.set_diff_view_mode(DiffViewMode::Split, cx);
                            this.restore_diff_panel_focus_after_toolbar_action(window, cx);
                            let root_view = this.root_view.clone();
                            cx.defer(move |cx| {
                                if let Some(root) = root_view.upgrade() {
                                    root.update(cx, |root, cx| {
                                        root.set_diff_view_mode(DiffViewMode::Split, cx);
                                    });
                                }
                            });
                            cx.notify();
                        })
                        .debug_selector(|| "diff_split".to_string())
                        .worktree_tooltip(
                            theme,
                            crate::i18n::t!(
                                "diff.tooltip.split_view",
                                shortcut = crate::view::shortcut_labels::alt_shortcut("S")
                            )
                            .to_string()
                            .into(),
                        );

                let diff_edit_btn = self
                    .file_edit_toggle_button(
                        theme,
                        view_toggle_selected_bg,
                        intents.is_file_editor,
                        cx,
                    )
                    .into_any_element();
                let diff_annotate_btn =
                    self.diff_annotate_toggle_button(theme, view_toggle_selected_bg, cx);

                let view_toggle = div()
                    .id("diff_view_toggle")
                    .debug_selector(|| "diff_view_toggle".to_string())
                    .flex()
                    .items_center()
                    .h(components::control_height(ui_scale_percent))
                    .rounded(px(theme.radii.row))
                    .border_1()
                    .border_color(view_toggle_border)
                    .bg(gpui::rgba(0x00000000))
                    .overflow_hidden()
                    .child(diff_inline_btn)
                    .child(div().h_full().w(px(1.0)).bg(view_toggle_divider))
                    .child(diff_split_btn);

                controls = controls
                    .child(prev_hunk_btn)
                    .child(next_hunk_btn)
                    .when_some(next_file_btn, |d, btn| d.child(btn))
                    .child(view_toggle)
                    .child(diff_edit_btn)
                    .child(diff_annotate_btn)
                    // `intents.is_file_editor`, not `is_file_editor_active`: edit mode
                    // can be on while a submodule summary or an
                    // untracked-directory notice owns the body, and a Save
                    // control over a body with no buffer is a trap.
                    // Discard sits before Save, so the pair reads as the two
                    // ways out of an unsaved buffer in the order they are meant.
                    .when(intents.is_file_editor && !self.auto_save_file_edits, |d| {
                        d.child(self.file_editor_discard_button(theme, cx))
                            .child(self.file_editor_save_button(theme, cx))
                    });
            } else {
                controls = controls.when_some(next_file_btn, |d, btn| d.child(btn));
            }
        } else {
            // File content view (e.g. a file shown at a commit): expose the
            // Blame toggle here too so annotations can be walked through history.
            let annotate_selected_bg = with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.26 } else { 0.20 },
            );
            // Reached by the file-content view *and* by the editor, including
            // for a file the preview declines: an editable buffer must never sit
            // under Inline/Split and hunk arrows that navigate a diff which is
            // not on screen.
            controls = controls
                .when_some(prev_file_btn, |d, btn| d.child(btn))
                .when_some(next_file_btn, |d, btn| d.child(btn))
                .child(self.file_edit_toggle_button(
                    theme,
                    annotate_selected_bg,
                    intents.is_file_editor,
                    cx,
                ))
                .child(self.diff_annotate_toggle_button(theme, annotate_selected_bg, cx))
                // Saving is explicit only when auto-save is off; with it on the
                // button would never be enabled long enough to click, and
                // neither would the Discard beside it.
                .when(intents.is_file_editor && !self.auto_save_file_edits, |d| {
                    d.child(self.file_editor_discard_button(theme, cx))
                        .child(self.file_editor_save_button(theme, cx))
                });
        }

        if !intents.is_conflict_resolver
            && let Some(preview_kind) = intents.rendered_view_toggle_kind
        {
            let preview_mode = self.rendered_preview_modes.get(preview_kind);
            controls = controls.child(
                div()
                    .id(preview_kind.toggle_id())
                    .debug_selector(move || preview_kind.toggle_id().to_string())
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        components::Button::new(
                            preview_kind.rendered_button_id(),
                            preview_kind.rendered_label(),
                        )
                        .style(if preview_mode == RenderedPreviewMode::Rendered {
                            components::ButtonStyle::Filled
                        } else {
                            components::ButtonStyle::Outlined
                        })
                        .on_click(
                            theme,
                            cx,
                            move |this, _e, window, cx| {
                                this.rendered_preview_modes
                                    .set(preview_kind, RenderedPreviewMode::Rendered);
                                // Rendered rows and source lines are different
                                // row spaces, so an open search has to rescan
                                // rather than keep indices into the old one.
                                this.diff_search_recompute_matches();
                                this.restore_diff_panel_focus_after_toolbar_action(window, cx);
                                cx.notify();
                            },
                        ),
                    )
                    .child(
                        components::Button::new(
                            preview_kind.source_button_id(),
                            preview_kind.source_label(),
                        )
                        .style(if preview_mode == RenderedPreviewMode::Source {
                            components::ButtonStyle::Filled
                        } else {
                            components::ButtonStyle::Outlined
                        })
                        .on_click(
                            theme,
                            cx,
                            move |this, _e, window, cx| {
                                this.rendered_preview_modes
                                    .set(preview_kind, RenderedPreviewMode::Source);
                                this.diff_search_recompute_matches();
                                this.restore_diff_panel_focus_after_toolbar_action(window, cx);
                                cx.notify();
                            },
                        ),
                    ),
            );
        }

        if let Some(repo_id) = repo_id {
            // The full text resolver gets its own settings menu under the cog
            // (section 30); everything else keeps the diff actions menu.
            let resolver_settings_active =
                intents.is_conflict_resolver && !intents.is_simple_conflict_strategy;
            let (cog_id, cog_kind, cog_tooltip): (&'static str, PopoverKind, &'static str) =
                if resolver_settings_active {
                    (
                        "mergetool_settings_menu",
                        PopoverKind::MergetoolSettingsMenu,
                        crate::i18n::tr_str("diff.tooltip.mergetool_settings"),
                    )
                } else {
                    (
                        "diff_action_menu",
                        PopoverKind::DiffActionMenu,
                        crate::i18n::tr_str("diff.tooltip.diff_actions"),
                    )
                };
            let diff_action_invoker: SharedString = cog_id.into();
            let diff_action_active = self
                .active_context_menu_invoker
                .as_ref()
                .is_some_and(|id| id == &diff_action_invoker);
            controls = controls.child(
                components::Button::new(cog_id, "")
                    .start_slot(svg_icon(
                        "icons/cog.svg",
                        theme.colors.foreground.secondary,
                        px(14.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .selected(diff_action_active)
                    .selected_bg(theme.colors.interaction.pressed_background)
                    .on_click(theme, cx, move |this, e, window, cx| {
                        this.activate_context_menu_invoker(diff_action_invoker.clone(), cx);
                        this.open_popover_at(cog_kind.clone(), e.position(), window, cx);
                    })
                    .debug_selector(move || cog_id.to_string())
                    .worktree_tooltip(theme, cog_tooltip.into()),
            );
            controls = controls.child(
                components::Button::new("diff_close", "")
                    .start_slot(svg_icon(
                        "icons/generic_close.svg",
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        this.clear_status_multi_selection(repo_id, cx);
                        this.clear_diff_selection_or_exit(repo_id, cx);
                        cx.notify();
                    })
                    .debug_selector(|| "diff_close".to_string())
                    .worktree_tooltip(theme, crate::i18n::tr("diff.tooltip.close_diff")),
            );
        }

        controls
    }
}
