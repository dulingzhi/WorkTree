use super::*;

impl DetailsPaneView {
    pub(in crate::view) fn render_commit_file_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(repo) = this.active_repo() else {
            return Vec::new();
        };
        let Loadable::Ready(details) = &repo.history_state.commit_details else {
            return Vec::new();
        };

        let theme = this.theme;
        let ui_scale_percent = this.ui_scale_percent;
        let file_row_h = crate::view::components::file_row_height(
            crate::density::current(cx).density,
            ui_scale_percent,
        );
        let scaled_px =
            |value: f32| crate::ui_scale::design_px_from_percent(value, ui_scale_percent);
        let repo_id = repo.id;
        let has_active_menu = this.active_context_menu_invoker.is_some();
        let file_rows = this.cached_commit_file_rows(
            repo_id,
            repo.history_state.commit_details_rev,
            &details.files,
        );
        let visible_signature = this.commit_files_visible_signature(
            repo_id,
            repo.history_state.commit_details_rev,
            &range,
            details.files.len(),
        );
        let path_alignment_group = this
            .commit_files_path_alignment_group
            .visible_rows(visible_signature);

        range
            .filter_map(|ix| {
                details
                    .files
                    .get(ix)
                    .zip(file_rows.get(ix))
                    .map(|(f, row)| (ix, f, row.label.clone(), row.visuals))
            })
            .map(|(ix, f, path_label, visuals)| {
                let commit_id = details.id.clone();
                let icon = Some(visuals.icon);
                let color = visuals.color(&theme);

                let context_menu_active = has_active_menu && {
                    let invoker: SharedString = format!(
                        "commit_file_menu_{}_{}_{}",
                        repo_id.0,
                        commit_id.as_ref(),
                        f.path.display()
                    )
                    .into();
                    this.active_context_menu_invoker.as_ref() == Some(&invoker)
                };
                let selected = repo
                    .diff_state
                    .diff_target
                    .as_ref()
                    .is_some_and(|t| match t {
                        DiffTarget::Commit {
                            commit_id: t_commit_id,
                            path: Some(t_path),
                        } => t_commit_id == &commit_id && t_path == &f.path,
                        _ => false,
                    });
                let commit_id_for_click = commit_id.clone();
                let path_for_click = f.path.clone();
                let commit_id_for_menu = commit_id.clone();
                let path_for_menu = f.path.clone();
                let tooltip = path_label.clone();

                let mut row = div()
                    .id(("commit_file", ix))
                    .debug_selector(move || format!("commit_file_{}_{}", repo_id.0, ix))
                    .h(file_row_h)
                    .flex()
                    .items_center()
                    .gap(scaled_px(8.0))
                    .px(scaled_px(8.0))
                    .w_full()
                    .rounded(px(theme.radii.row))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |s| {
                        if context_menu_active {
                            s.bg(theme.colors.interaction.pressed_background)
                        } else {
                            s.bg(theme.colors.interaction.hover_background)
                        }
                    })
                    .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                    .child(
                        div()
                            .w(scaled_px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(icon, |this, icon| {
                                this.child(svg_icon(icon, color, scaled_px(14.0)))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_sm()
                            .line_height(scaled_px(18.0))
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .child(
                                components::TruncatedText::aligned_path(
                                    path_label,
                                    path_alignment_group.clone(),
                                )
                                .text_sm()
                                .render(cx),
                            ),
                    )
                    .when(f.additions.is_some() || f.deletions.is_some(), |row| {
                        row.child(div().flex_none().child(components::diff_stat(
                            theme,
                            ui_scale_percent,
                            f.additions.unwrap_or(0) as usize,
                            f.deletions.unwrap_or(0) as usize,
                        )))
                    })
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        if !e.standard_click() {
                            return;
                        }
                        let target = DiffTarget::Commit {
                            commit_id: commit_id_for_click.clone(),
                            path: Some(path_for_click.clone()),
                        };
                        let selected = this.active_repo().is_some_and(|repo| {
                            repo.id == repo_id
                                && repo.diff_state.diff_target.as_ref() == Some(&target)
                        });

                        if selected {
                            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
                        } else {
                            this.focus_diff_panel(window, cx);
                            this.store.dispatch(Msg::SelectDiff { repo_id, target });
                        }
                        cx.notify();
                    }))
                    .worktree_tooltip(theme, tooltip.clone());
                row = row.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let invoker: SharedString = format!(
                            "commit_file_menu_{}_{}_{}",
                            repo_id.0,
                            commit_id_for_menu.as_ref(),
                            path_for_menu.display()
                        )
                        .into();
                        this.activate_context_menu_invoker(invoker, cx);
                        this.open_popover_at(
                            PopoverKind::CommitFileMenu {
                                repo_id,
                                commit_id: commit_id_for_menu.clone(),
                                path: path_for_menu.clone(),
                            },
                            e.position,
                            window,
                            cx,
                        );
                        cx.notify();
                    }),
                );

                if selected {
                    row = row.bg(with_alpha(
                        theme.colors.accent.foreground,
                        if theme.is_dark { 0.16 } else { 0.10 },
                    ));
                }
                if context_menu_active {
                    row = row.bg(theme.colors.interaction.pressed_background);
                }

                row.into_any_element()
            })
            .collect()
    }

    /// Render the changed-file rows for an active two-point comparison. Mirrors
    /// [`Self::render_commit_file_rows`] but sources the file list from
    /// `history_state.range_files` and builds `DiffTarget::CommitRange` targets,
    /// so clicking a file loads its diff through the normal diff pipeline.
    /// Changed files of a linked worktree that is not this tab.
    ///
    /// Clicking one opens it through the inline foreign-diff machinery — the
    /// same path submodule diffs take — so the diff renders here rather than
    /// forcing a tab switch.
    pub(in crate::view) fn render_worktree_file_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(repo) = this.active_repo() else {
            return Vec::new();
        };
        let repo_id = repo.id;
        let worktree_dirty_rev = repo.worktree_dirty_rev;
        let Some(summary) = this.selected_worktree_summary() else {
            return Vec::new();
        };
        // Derived once per scan, not per frame: this list is virtualized, but the
        // inputs behind it are one entry per changed file.
        let inputs = this.cached_worktree_file_inputs(repo_id, worktree_dirty_rev, summary);
        let files = &inputs.files;

        let theme = this.theme;
        let ui_scale_percent = this.ui_scale_percent;
        let file_row_h = crate::view::components::file_row_height(
            crate::density::current(cx).density,
            ui_scale_percent,
        );
        let scaled_px =
            |value: f32| crate::ui_scale::design_px_from_percent(value, ui_scale_percent);
        let file_rows =
            this.cached_worktree_file_rows(repo_id, worktree_dirty_rev, &summary.path, files);
        let selected_ix_now = repo
            .diff_state
            .inline_submodule_diff
            .as_ref()
            .filter(|inline| inline.submodule_repo_path == summary.path)
            .map(|inline| inline.selected_ix);
        let visible_signature = this.worktree_files_visible_signature(
            repo_id,
            worktree_dirty_rev,
            &summary.path,
            &range,
            files.len(),
        );
        let path_alignment_group = this
            .worktree_files_path_alignment_group
            .visible_rows(visible_signature);
        let worktree_path = summary.path.clone();
        let origin = worktree_state::model::ForeignDiffOrigin::Worktree {
            branch: summary.branch.clone(),
            detached: summary.detached,
        };

        range
            .filter_map(|ix| {
                files
                    .get(ix)
                    .zip(file_rows.get(ix))
                    .map(|(f, row)| (ix, f.clone(), row.label.clone(), row.visuals))
            })
            .map(|(ix, _f, path_label, visuals)| {
                let color = visuals.color(&theme);
                let selected = selected_ix_now == Some(ix);
                let tooltip = path_label.clone();
                let inputs_for_click = Arc::clone(&inputs);
                let worktree_path_for_click = worktree_path.clone();
                let origin_for_click = origin.clone();

                let mut row = div()
                    .id(("worktree_file", ix))
                    .debug_selector(move || format!("worktree_file_{}_{}", repo_id.0, ix))
                    .h(file_row_h)
                    .flex()
                    .items_center()
                    .gap(scaled_px(8.0))
                    .px(scaled_px(8.0))
                    .w_full()
                    .rounded(px(theme.radii.row))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |s| s.bg(theme.colors.interaction.hover_background))
                    .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                    .child(
                        div()
                            .w(scaled_px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(svg_icon(visuals.icon, color, scaled_px(14.0))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_sm()
                            .line_height(scaled_px(18.0))
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .child(
                                components::TruncatedText::aligned_path(
                                    path_label,
                                    path_alignment_group.clone(),
                                )
                                .text_sm()
                                .render(cx),
                            ),
                    )
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        if !e.standard_click() {
                            return;
                        }
                        this.focus_diff_panel(window, cx);
                        this.store.dispatch(Msg::OpenInlineSubmoduleDiff {
                            repo_id,
                            origin: origin_for_click.clone(),
                            submodule_repo_path: worktree_path_for_click.clone(),
                            parent_submodule_path: worktree_path_for_click.clone(),
                            entries: inputs_for_click.entries.clone(),
                            selected_ix: ix,
                        });
                        cx.notify();
                    }))
                    .worktree_tooltip(theme, tooltip.clone());

                if selected {
                    row = row.bg(with_alpha(
                        theme.colors.accent.foreground,
                        if theme.is_dark { 0.16 } else { 0.10 },
                    ));
                }

                row.into_any_element()
            })
            .collect()
    }

    pub(in crate::view) fn render_range_file_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(repo) = this.active_repo() else {
            return Vec::new();
        };
        let Some(range_selection) = repo.history_state.range_selection.clone() else {
            return Vec::new();
        };
        let Loadable::Ready(files) = &repo.history_state.range_files else {
            return Vec::new();
        };
        let files = files.clone();

        let theme = this.theme;
        let ui_scale_percent = this.ui_scale_percent;
        let file_row_h = crate::view::components::file_row_height(
            crate::density::current(cx).density,
            ui_scale_percent,
        );
        let scaled_px =
            |value: f32| crate::ui_scale::design_px_from_percent(value, ui_scale_percent);
        let repo_id = repo.id;
        let from = range_selection.from.clone();
        let to = range_selection.to.clone();
        let file_rows =
            this.cached_range_file_rows(repo_id, repo.history_state.range_files_rev, &files);
        let visible_signature = this.range_files_visible_signature(
            repo_id,
            repo.history_state.range_files_rev,
            &range,
            files.len(),
        );
        let path_alignment_group = this
            .range_files_path_alignment_group
            .visible_rows(visible_signature);

        range
            .filter_map(|ix| {
                files
                    .get(ix)
                    .zip(file_rows.get(ix))
                    .map(|(f, row)| (ix, f, row.label.clone(), row.visuals))
            })
            .map(|(ix, f, path_label, visuals)| {
                let icon = Some(visuals.icon);
                let color = visuals.color(&theme);
                let target = DiffTarget::CommitRange {
                    from_commit_id: from.clone(),
                    to_commit_id: to.clone(),
                    path: Some(f.path.clone()),
                };
                let selected = repo.diff_state.diff_target.as_ref() == Some(&target);
                let target_for_click = target.clone();
                let tooltip = path_label.clone();

                let mut row = div()
                    .id(("range_file", ix))
                    .debug_selector(move || format!("range_file_{}_{}", repo_id.0, ix))
                    .h(file_row_h)
                    .flex()
                    .items_center()
                    .gap(scaled_px(8.0))
                    .px(scaled_px(8.0))
                    .w_full()
                    .rounded(px(theme.radii.row))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |s| s.bg(theme.colors.interaction.hover_background))
                    .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                    .child(
                        div()
                            .w(scaled_px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(icon, |this, icon| {
                                this.child(svg_icon(icon, color, scaled_px(14.0)))
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_sm()
                            .line_height(scaled_px(18.0))
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .child(
                                components::TruncatedText::aligned_path(
                                    path_label,
                                    path_alignment_group.clone(),
                                )
                                .text_sm()
                                .render(cx),
                            ),
                    )
                    .when(f.additions.is_some() || f.deletions.is_some(), |row| {
                        row.child(div().flex_none().child(components::diff_stat(
                            theme,
                            ui_scale_percent,
                            f.additions.unwrap_or(0) as usize,
                            f.deletions.unwrap_or(0) as usize,
                        )))
                    })
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        if !e.standard_click() {
                            return;
                        }
                        let selected = this.active_repo().is_some_and(|repo| {
                            repo.id == repo_id
                                && repo.diff_state.diff_target.as_ref() == Some(&target_for_click)
                        });
                        if selected {
                            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
                        } else {
                            this.focus_diff_panel(window, cx);
                            this.store.dispatch(Msg::SelectDiff {
                                repo_id,
                                target: target_for_click.clone(),
                            });
                        }
                        cx.notify();
                    }))
                    .worktree_tooltip(theme, tooltip.clone());

                if selected {
                    row = row.bg(with_alpha(
                        theme.colors.accent.foreground,
                        if theme.is_dark { 0.16 } else { 0.10 },
                    ));
                }

                row.into_any_element()
            })
            .collect()
    }
}
