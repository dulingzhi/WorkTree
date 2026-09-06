//! Diff pane keyboard dispatch: the focus carve-outs and the ordered
//! shortcut chain behind `on_key_down`.
use super::*;

impl MainPaneView {
    pub(crate) fn handle_diff_shortcut(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let key = keystroke.key.as_str();
        let mods = keystroke.modifiers;

        let mut handled = false;

        // While the editable buffer has focus every keystroke belongs to it, with
        // one exception: Ctrl/Cmd+S saves and returns to the originating view.
        // Outside the editor that chord stages the file, and both meanings can
        // coexist precisely because they are separated by focus.
        if self
            .file_editor_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
        {
            if (mods.control || mods.platform)
                && !mods.alt
                && !mods.shift
                && !mods.function
                && key == "s"
            {
                self.save_file_editor_buffer_and_exit(window, cx);
                return true;
            }
            if key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function {
                self.toggle_file_editor(window, cx);
                return true;
            }
            // Deliberately *not* Alt+E: on macOS Option+E is the acute-accent
            // dead key, and swallowing it here would stop the buffer composing
            // `é`. Escape above is the way out from inside the editor; Alt+E
            // only enters it, from a view where nothing is being typed.
            return false;
        }

        // When the editable resolved-output pane is focused the user is typing
        // free text: every text-producing keystroke (space, a/b/c/d, etc.)
        // belongs to that editor, not to the diff/conflict shortcut table.
        // Letting them through here staged the conflict file on the first space
        // typed (StagePath → the file leaves Conflicted → the resolver closes
        // mid-edit). Three deliberate carve-outs:
        //   * Ctrl+1/2/3 pick aliases are chords with no text-input collision,
        //     so they stay live while editing (kdiff3 parity).
        //   * Shift+F2/F3 (previous/next unresolved conflict) likewise: an
        //     F-key produces no text and the editor binds only the unmodified
        //     `f2`/`f3`, so the chord is free here — and jumping to the next
        //     open conflict is exactly what you want *while* editing the
        //     merged result, which is why it is not left outside.
        //   * WorkTree's Ctrl+Home/End resolver bindings are intentionally NOT
        //     handled here, so the editor keeps them for cursor movement.
        if self
            .conflict_resolver_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
        {
            if self.is_conflict_resolver_active()
                && (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                    key,
                    self.conflict_resolver.view_mode,
                )
            {
                if mods.shift {
                    self.conflict_resolver_choose_everywhere(choice, cx);
                    return true;
                }
                if self.conflict_resolver_has_active_pick_target() {
                    self.conflict_resolver_pick_active_conflict(choice, cx);
                    return true;
                }
            }
            if self.is_conflict_resolver_active()
                && matches!(key, "f2" | "f3")
                && mods.shift
                && !mods.control
                && !mods.alt
                && !mods.platform
                && !mods.function
                && !self.conflict_resolver.nav_targets.is_empty()
            {
                if key == "f2" {
                    self.conflict_jump_prev_unresolved(cx);
                } else {
                    self.conflict_jump_next_unresolved(cx);
                }
                return true;
            }
            return false;
        }

        // kdiff3 manual diff help: Escape abandons pending alignment marks
        // before reaching the resolver's other escape behaviors, so a
        // mis-marked line does not cost the user their selection or view.
        if key == "escape"
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
            && self.conflict_resolver_clear_alignment_marks(cx)
        {
            return true;
        }

        if key == "escape" && !mods.control && !mods.alt && !mods.platform && !mods.function {
            if self.diff_search_active {
                self.deactivate_diff_search(window, cx);
                handled = true;
            }
            if !handled
                && self.is_inline_submodule_diff_active()
                && let Some(repo_id) = self.active_repo_id()
            {
                self.store
                    .dispatch(Msg::CloseInlineSubmoduleDiff { repo_id });
                handled = true;
            }
            if !handled && let Some(repo_id) = self.active_repo_id() {
                self.clear_status_multi_selection(repo_id, cx);
                self.clear_diff_selection_or_exit(repo_id, cx);
                handled = true;
            }
        }

        if !handled && mods.secondary() && mods.number_of_modifiers() == 1 && key == "f" {
            handled = self.open_search_for_active_view(window, cx);
        }

        // Shift+F2/F3 step between *unresolved* conflicts — the resolved ones
        // are skipped, which is what separates this from plain F2/F3.
        //
        // It sits ahead of the diff-search block below deliberately: this is a
        // distinct chord, so letting it become "previous/next search match"
        // whenever the search box happens to be open would be surprising. The
        // resolver guard keeps that scoped — outside the conflict resolver
        // Shift+F2/F3 falls through and means exactly what it always did.
        if !handled
            && matches!(key, "f2" | "f3")
            && mods.shift
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
            && self.is_conflict_resolver_active()
            && !self.conflict_resolver.nav_targets.is_empty()
        {
            if key == "f2" {
                self.conflict_jump_prev_unresolved(cx);
            } else {
                self.conflict_jump_next_unresolved(cx);
            }
            handled = true;
        }

        if !handled
            && self.diff_search_active
            && matches!(key, "f2" | "f3")
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
        {
            if key == "f2" {
                self.diff_search_prev_match();
            } else {
                self.diff_search_next_match();
            }
            handled = true;
        }

        if !handled
            && key == "space"
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
            && !self.is_inline_submodule_diff_active()
            && !self
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && !self
                .diff_search_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && let Some(repo_id) = self.active_repo_id()
            && let Some(repo) = self.active_repo()
            && let Some(diff_target) = repo.diff_state.diff_target.clone()
            && let DiffTarget::WorkingTree { path, area } = &diff_target
        {
            let path = path.clone();
            let area = *area;
            let change_tracking_view = self.active_change_tracking_view(cx);
            let next_path_in_section = status_nav::status_navigation_context_for_repo(
                repo,
                &diff_target,
                change_tracking_view,
            )
            .and_then(|navigation| navigation.next_or_prev_path());
            let status_ready = repo.status_entries_for_area(area).is_some();

            // A multi-file status selection wins over the single shown file, so
            // the shortcut matches what the status row button and context menu
            // already do with the same selection.
            if let Some(paths) = self.status_selection_for_shortcut(repo_id, area, &path, cx) {
                if self.confirm_stage_conflict_markers(
                    repo_id,
                    area,
                    paths.clone(),
                    true,
                    window,
                    cx,
                ) {
                    return true;
                }
                self.clear_status_selection_for_shortcut(repo_id, cx);
                self.stage_or_unstage_status_paths(repo_id, area, paths);
                self.rebuild_diff_cache(cx);
                return true;
            }

            if self.confirm_stage_conflict_markers(
                repo_id,
                area,
                vec![path.clone()],
                false,
                window,
                cx,
            ) {
                return true;
            }

            match (status_ready, area) {
                (true, DiffArea::Unstaged) => {
                    self.store.dispatch(Msg::StagePath {
                        repo_id,
                        path: path.clone(),
                    });
                    if let Some(next_path) = next_path_in_section {
                        self.store.dispatch(Msg::SelectDiff {
                            repo_id,
                            target: DiffTarget::WorkingTree {
                                path: next_path,
                                area: DiffArea::Unstaged,
                            },
                        });
                    } else {
                        self.clear_diff_selection_or_exit(repo_id, cx);
                    }
                }
                (true, DiffArea::Staged) => {
                    self.store.dispatch(Msg::UnstagePath {
                        repo_id,
                        path: path.clone(),
                    });
                    if let Some(next_path) = next_path_in_section {
                        self.store.dispatch(Msg::SelectDiff {
                            repo_id,
                            target: DiffTarget::WorkingTree {
                                path: next_path,
                                area: DiffArea::Staged,
                            },
                        });
                    } else {
                        self.clear_diff_selection_or_exit(repo_id, cx);
                    }
                }
                (false, DiffArea::Unstaged) => {
                    self.store.dispatch(Msg::StagePath {
                        repo_id,
                        path: path.clone(),
                    });
                }
                (false, DiffArea::Staged) => {
                    self.store.dispatch(Msg::UnstagePath {
                        repo_id,
                        path: path.clone(),
                    });
                }
            }
            self.rebuild_diff_cache(cx);
            handled = true;
        }

        if !handled
            && (key == "f1" || key == "f4")
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
            && let Some(repo_id) = self.active_repo_id()
        {
            let direction = if key == "f1" { -1 } else { 1 };
            handled = self.try_select_adjacent_diff_file(repo_id, direction, window, cx);
        }

        if !handled
            && !self.is_inline_submodule_diff_active()
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && !self
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && !self
                .diff_search_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && let Some(repo_id) = self.active_repo_id()
            && let Some(repo) = self.active_repo()
            && let Some(diff_target) = repo.diff_state.diff_target.clone()
            && let DiffTarget::WorkingTree { path, area } = &diff_target
        {
            let path = path.clone();
            let area = *area;
            let status_ready = repo.status_entries_for_area(area).is_some();

            match key {
                "s" if area == DiffArea::Unstaged && !mods.shift => {
                    let change_tracking_view = self.active_change_tracking_view(cx);
                    let next_path_in_section = status_nav::status_navigation_context_for_repo(
                        repo,
                        &diff_target,
                        change_tracking_view,
                    )
                    .and_then(|navigation| navigation.next_or_prev_path());

                    // A multi-file status selection wins over the single shown
                    // file, matching the status row button and context menu.
                    // Resolved before confirming, or the dialog would describe —
                    // and then stage — only the shown file out of the selection.
                    if let Some(paths) =
                        self.status_selection_for_shortcut(repo_id, area, &path, cx)
                    {
                        if self.confirm_stage_conflict_markers(
                            repo_id,
                            area,
                            paths.clone(),
                            true,
                            window,
                            cx,
                        ) {
                            return true;
                        }
                        self.clear_status_selection_for_shortcut(repo_id, cx);
                        self.stage_or_unstage_status_paths(repo_id, area, paths);
                        self.rebuild_diff_cache(cx);
                        return true;
                    }

                    if self.confirm_stage_conflict_markers(
                        repo_id,
                        area,
                        vec![path.clone()],
                        false,
                        window,
                        cx,
                    ) {
                        return true;
                    }

                    if status_ready {
                        self.store.dispatch(Msg::StagePath {
                            repo_id,
                            path: path.clone(),
                        });
                        if let Some(next_path) = next_path_in_section {
                            self.store.dispatch(Msg::SelectDiff {
                                repo_id,
                                target: DiffTarget::WorkingTree {
                                    path: next_path,
                                    area: DiffArea::Unstaged,
                                },
                            });
                        } else {
                            self.clear_diff_selection_or_exit(repo_id, cx);
                        }
                    } else {
                        self.store.dispatch(Msg::StagePath {
                            repo_id,
                            path: path.clone(),
                        });
                    }
                    self.rebuild_diff_cache(cx);
                    handled = true;
                }
                "u" if area == DiffArea::Staged && !mods.shift => {
                    let change_tracking_view = self.active_change_tracking_view(cx);
                    let next_path_in_section = status_nav::status_navigation_context_for_repo(
                        repo,
                        &diff_target,
                        change_tracking_view,
                    )
                    .and_then(|navigation| navigation.next_or_prev_path());

                    // A multi-file status selection wins over the single shown
                    // file, matching the status row button and context menu.
                    if let Some(paths) =
                        self.status_selection_for_shortcut(repo_id, area, &path, cx)
                    {
                        if self.confirm_stage_conflict_markers(
                            repo_id,
                            area,
                            paths.clone(),
                            true,
                            window,
                            cx,
                        ) {
                            return true;
                        }
                        self.clear_status_selection_for_shortcut(repo_id, cx);
                        self.stage_or_unstage_status_paths(repo_id, area, paths);
                        self.rebuild_diff_cache(cx);
                        return true;
                    }

                    if status_ready {
                        self.store.dispatch(Msg::UnstagePath {
                            repo_id,
                            path: path.clone(),
                        });
                        if let Some(next_path) = next_path_in_section {
                            self.store.dispatch(Msg::SelectDiff {
                                repo_id,
                                target: DiffTarget::WorkingTree {
                                    path: next_path,
                                    area: DiffArea::Staged,
                                },
                            });
                        } else {
                            self.clear_diff_selection_or_exit(repo_id, cx);
                        }
                    } else {
                        self.store.dispatch(Msg::UnstagePath {
                            repo_id,
                            path: path.clone(),
                        });
                    }
                    self.rebuild_diff_cache(cx);
                    handled = true;
                }
                "d" if !mods.shift => {
                    let bounds = window.window_bounds().get_bounds();
                    let anchor = point(
                        (bounds.size.width * 0.5).max(px(64.0)),
                        (bounds.size.height * 0.25).max(px(24.0)),
                    );
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
                    handled = true;
                }
                "h" if !mods.shift => {
                    let bounds = window.window_bounds().get_bounds();
                    let anchor = point(
                        (bounds.size.width * 0.5).max(px(64.0)),
                        (bounds.size.height * 0.25).max(px(24.0)),
                    );
                    self.open_popover_at(
                        PopoverKind::FileHistory {
                            repo_id,
                            path: path.clone(),
                            is_dir: false,
                        },
                        anchor,
                        window,
                        cx,
                    );
                    handled = true;
                }
                "e" if !mods.shift && crate::external_editor::configured_setting().is_some() => {
                    let full_path = repo.spec.workdir.join(&path);
                    let root_view = self.root_view.clone();
                    let p = full_path;
                    cx.defer(move |cx| {
                        if let Some(root) = root_view.upgrade() {
                            root.update(cx, |root, cx| {
                                root.open_path_in_external_code_editor(p, cx);
                            });
                        }
                    });
                    handled = true;
                }
                "c" if mods.shift => {
                    crate::clipboard::write_text(
                        cx,
                        path.display().to_string(),
                        crate::clipboard::CopySource::FilePathShortcut,
                    );
                    handled = true;
                }
                _ => {}
            }
        }

        if !handled
            && !self.is_inline_submodule_diff_active()
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && !self
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && !self
                .diff_search_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && let Some(_repo_id) = self.active_repo_id()
            && let Some(repo) = self.active_repo()
            && let Some(diff_target) = repo.diff_state.diff_target.clone()
        {
            let path = match &diff_target {
                DiffTarget::WorkingTree { path, .. } => Some(path.clone()),
                DiffTarget::Commit { path, .. } => path.clone(),
                DiffTarget::CommitRange { path, .. } => path.clone(),
            };
            if let Some(path) = path {
                match key {
                    "e" if !mods.shift
                        && crate::external_editor::configured_setting().is_some() =>
                    {
                        let full_path = repo.spec.workdir.join(&path);
                        let root_view = self.root_view.clone();
                        let p = full_path;
                        cx.defer(move |cx| {
                            if let Some(root) = root_view.upgrade() {
                                root.update(cx, |root, cx| {
                                    root.open_path_in_external_code_editor(p, cx);
                                });
                            }
                        });
                        handled = true;
                    }
                    _ => {}
                }
            }
        }

        // Ahead of the file-preview early return below, which would otherwise
        // swallow it: the toggle has to work from the content view as well as
        // from a diff. Behind the focused-editor carve-out above, so it never
        // competes with what the buffer is composing.
        // Not while a text field owns the keyboard: on macOS Option+E is the
        // acute-accent dead key, and the search and raw-diff inputs compose with
        // it exactly as the buffer does. The Ctrl/Cmd branch below excludes the
        // same two inputs for the same reason.
        let text_input_focused = self
            .diff_search_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
            || self
                .diff_raw_input
                .read(cx)
                .focus_handle()
                .is_focused(window);
        if mods.alt
            && !mods.control
            && !mods.platform
            && !mods.function
            && !mods.shift
            && key == "e"
            && !text_input_focused
            && !self.is_conflict_resolver_active()
            && !self.is_markdown_preview_active()
            && self.can_edit_current_target()
        {
            self.toggle_file_editor(window, cx);
            return true;
        }

        let copy_target_is_focused = self
            .diff_raw_input
            .read(cx)
            .focus_handle()
            .is_focused(window);
        let is_file_preview = self.is_file_preview_active();
        if is_file_preview {
            if !handled
                && !copy_target_is_focused
                && (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && !mods.shift
                && key == "c"
                && self.diff_text_has_selection()
            {
                self.copy_selected_diff_text_to_clipboard(cx);
                handled = true;
            }

            if !handled
                && !copy_target_is_focused
                && (mods.control || mods.platform)
                && !mods.alt
                && !mods.function
                && key == "a"
            {
                self.select_all_diff_text();
                handled = true;
            }

            return handled;
        }

        let conflict_resolver_active = self.is_conflict_resolver_active();
        let markdown_preview_active = self.is_markdown_preview_active();
        let conflict_preview_active = self.is_conflict_rendered_preview_active();

        if mods.alt && !mods.control && !mods.platform && !mods.function {
            match key {
                "i" | "s" => {
                    if conflict_resolver_active {
                        handled = false;
                    } else if self.active_conflict_target().is_some() {
                        self.set_diff_view_mode(DiffViewMode::Split, cx);
                        handled = true;
                        let root_view = self.root_view.clone();
                        cx.defer(move |cx| {
                            if let Some(root) = root_view.upgrade() {
                                root.update(cx, |root, cx| {
                                    root.set_diff_view_mode(DiffViewMode::Split, cx);
                                });
                            }
                        });
                    // The markdown diff preview renders both layouts (see
                    // `render_markdown_diff_preview`), so these switch it just
                    // like the text diff. The single-pane file preview has no
                    // old/new pair to split, so it stays excluded.
                    } else if !self.is_file_preview_active() {
                        let new_mode = if key == "i" {
                            DiffViewMode::Inline
                        } else {
                            DiffViewMode::Split
                        };
                        self.set_diff_view_mode(new_mode, cx);
                        handled = true;
                        let root_view = self.root_view.clone();
                        let mode = new_mode;
                        cx.defer(move |cx| {
                            if let Some(root) = root_view.upgrade() {
                                root.update(cx, |root, cx| {
                                    root.set_diff_view_mode(mode, cx);
                                });
                            }
                        });
                    }
                }
                "w" if !markdown_preview_active && !conflict_preview_active => {
                    self.toggle_reveal_whitespace_chars(cx);
                    handled = true;
                }
                "b" if !markdown_preview_active && !conflict_preview_active => {
                    let next = !self.annotate_enabled;
                    handled = true;
                    let root_view = self.root_view.clone();
                    cx.defer(move |cx| {
                        if let Some(root) = root_view.upgrade() {
                            root.update(cx, |root, cx| {
                                root.set_annotate_enabled(next, cx);
                            });
                        }
                    });
                }
                "up" => {
                    handled = self.navigate_prev_diff_change(cx);
                }
                "down" => {
                    handled = self.navigate_next_diff_change(cx);
                }
                "left" => {
                    if let Some(repo_id) = self.active_repo_id() {
                        self.store.dispatch(Msg::GlobalNavBack { repo_id });
                        handled = true;
                    }
                }
                "right" => {
                    if let Some(repo_id) = self.active_repo_id() {
                        self.store.dispatch(Msg::GlobalNavForward { repo_id });
                        handled = true;
                    }
                }
                _ => {}
            }
        }

        if !handled
            && matches!(key, "f2" | "f3" | "f7")
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
        {
            match key {
                "f2" => {
                    let _ = self.navigate_prev_search_match_or_diff_change(cx);
                }
                "f3" => {
                    let _ = self.navigate_next_search_match_or_diff_change(cx);
                }
                "f7" if mods.shift => {
                    let _ = self.navigate_prev_diff_change(cx);
                }
                "f7" => {
                    let _ = self.navigate_next_diff_change(cx);
                }
                _ => {}
            }
            handled = true;
        }

        if !handled
            && conflict_resolver_active
            && !mods.control
            && !mods.alt
            && !mods.platform
            && !mods.function
            && !copy_target_is_focused
            && !self
                .conflict_resolver_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            // Single-letter picks must not swallow characters typed into the
            // search box (e.g. "d" would otherwise pick Both).
            && !self
                .diff_search_input
                .read(cx)
                .focus_handle()
                .is_focused(window)
            && self.conflict_resolver_has_active_pick_target()
        {
            if let Some(choice) = conflict_resolver::conflict_quick_pick_choice_for_key(
                key,
                self.conflict_resolver.view_mode,
            ) {
                self.conflict_resolver_pick_active_conflict(choice, cx);
                handled = true;
            } else if key == "u" {
                // section 30: U un-resolves the active conflict (pick or auto-solve).
                self.conflict_resolver_unresolve_active_conflict(cx);
                handled = true;
            }
        }

        // KDiff3-compatible Ctrl+Shift+1/2/3: choose A/B/C on every delta,
        // including blocks that were selected automatically and have no
        // conflict markers.
        if !handled
            && conflict_resolver_active
            && (mods.control || mods.platform)
            && mods.shift
            && !mods.alt
            && !mods.function
            && let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                key,
                self.conflict_resolver.view_mode,
            )
        {
            self.conflict_resolver_choose_everywhere(choice, cx);
            handled = true;
        }

        // section 30: kdiff3-compatible Ctrl+1/2/3 pick aliases. When the output
        // editor is focused these are handled by the carve-out at the top of this
        // fn; this block covers the case where focus is elsewhere.
        if !handled
            && conflict_resolver_active
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && !mods.shift
            && self.conflict_resolver_has_active_pick_target()
            && let Some(choice) = conflict_resolver::conflict_ctrl_pick_choice_for_key(
                key,
                self.conflict_resolver.view_mode,
            )
        {
            self.conflict_resolver_pick_active_conflict(choice, cx);
            handled = true;
        }

        // kdiff3 manual diff help: Ctrl+Y pins the lines marked in the source
        // columns onto one another; Ctrl+Shift+Y drops every pin and returns
        // the file to its automatic alignment.
        if !handled
            && conflict_resolver_active
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && key == "y"
        {
            handled = if mods.shift {
                self.conflict_resolver_clear_manual_alignments(cx)
            } else {
                self.conflict_resolver_align_manually(cx)
            };
        }

        // WorkTree resolver navigation: Ctrl+Home/End jump to the first/last
        // delta. (Previous/next *unresolved* conflict is Shift+F2/F3, handled
        // above — Ctrl+PgUp/PgDn belongs to the repository tabs.)
        if !handled
            && conflict_resolver_active
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && !mods.shift
            && !self.conflict_resolver.nav_targets.is_empty()
        {
            match key {
                "home" => {
                    self.conflict_jump_first(cx);
                    handled = true;
                }
                "end" => {
                    self.conflict_jump_last(cx);
                    handled = true;
                }
                _ => {}
            }
        }

        if !handled
            && !copy_target_is_focused
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && !mods.shift
            && key == "c"
            && self.diff_text_has_selection()
        {
            self.copy_selected_diff_text_to_clipboard(cx);
            handled = true;
        }

        if !handled
            && !copy_target_is_focused
            && (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && key == "a"
        {
            self.select_all_diff_text();
            handled = true;
        }

        handled
    }
}
