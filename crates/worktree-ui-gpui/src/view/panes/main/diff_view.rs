//! Facade for the diff pane view: the pane chrome, the toolbar buttons, the
//! file-editor entry points and the `diff_view` renderer itself. The keyboard
//! dispatch lives in `shortcuts`, the submodule summary in
//! `submodule_summary` and the search overlay in `search_overlay`.
use super::*;
use crate::view::panes::PaneChromeExt as _;
use crate::view::panes::main::DiffHorizontalScrollColumn;
use crate::view::panes::main::diff_search::DiffSearchOptions;
use gpui::Focusable;

mod search_overlay;
mod shortcuts;
mod submodule_summary;

impl Focusable for MainPaneView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.diff_panel_focus_handle.clone()
    }
}

impl MainPaneView {
    /// Labels for the split-diff column headers, matched to what is actually
    /// being compared — conflict views keep their own local/remote wording.
    pub(in crate::view) fn split_diff_pane_labels(&self) -> (&'static str, &'static str) {
        let repo = self.active_repo();
        let target = repo.and_then(|repo| match &repo.diff_state.diff {
            Loadable::Ready(diff) => Some(&diff.target),
            _ => repo.diff_state.diff_target.as_ref(),
        });
        match target {
            Some(DiffTarget::Commit { .. }) => (
                crate::i18n::tr_str("diff.split.parent"),
                crate::i18n::tr_str("diff.split.this_commit"),
            ),
            Some(DiffTarget::CommitRange { .. }) => (
                crate::i18n::tr_str("diff.split.from_commit"),
                crate::i18n::tr_str("diff.split.to_commit"),
            ),
            Some(DiffTarget::WorkingTree {
                area: DiffArea::Staged,
                ..
            }) => ("HEAD", crate::i18n::tr_str("diff.split.staged")),
            Some(DiffTarget::WorkingTree { .. }) | None => (
                crate::i18n::tr_str("diff.split.index"),
                crate::i18n::tr_str("diff.split.working_tree"),
            ),
        }
    }

    /// A thin vertical drag handle at the annotation column's right edge that
    /// resizes the column. Positioned absolutely; the caller's container must
    /// be `relative()`.
    pub(in crate::view) fn annotate_resize_handle(
        &self,
        ui_scale_percent: u32,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let annot_w = self.annotate_column_width_px(ui_scale_percent);
        let handle_w = px(7.0);
        div()
            .id("annotate_resize_handle")
            .debug_selector(|| "annotate_resize_handle".to_string())
            .group("annotate_resize_handle")
            .absolute()
            .left((annot_w - handle_w / 2.0).max(px(0.0)))
            .top(px(0.0))
            .h_full()
            .w(handle_w)
            .cursor(CursorStyle::ResizeLeftRight)
            .child(components::resize_grip(
                theme,
                ui_scale_percent,
                "annotate_resize_handle",
                components::ResizeGripAxis::Vertical,
                self.annotate_resize.is_some(),
                None,
            ))
            .on_drag(AnnotateResizeHandle::Divider, |_h, _o, _w, cx| {
                cx.new(|_cx| AnnotateResizeDragGhost)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                    crate::press_gesture::claim_press(cx);
                    this.annotate_resize = Some(AnnotateResizeState {
                        start_x: e.position.x,
                        start_width: this.annotate_column_width,
                    });
                }),
            )
            .on_drag_move(cx.listener(
                move |this, e: &gpui::DragMoveEvent<AnnotateResizeHandle>, _w, cx| {
                    let Some(state) = this.annotate_resize else {
                        return;
                    };
                    if *e.drag(cx) != AnnotateResizeHandle::Divider {
                        return;
                    }
                    let per_unit = f32::from(crate::ui_scale::design_px_from_percent(
                        1.0,
                        ui_scale_percent,
                    ))
                    .max(0.01);
                    let dx_design = f32::from(e.event.position.x - state.start_x) / per_unit;
                    this.annotate_column_width = (state.start_width + dx_design).clamp(
                        crate::view::rows::DIFF_ANNOTATION_MIN_WIDTH_PX,
                        crate::view::rows::DIFF_ANNOTATION_MAX_WIDTH_PX,
                    );
                    this.invalidate_diff_wrap_visible_cache();
                    cx.notify();
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e, _w, cx| {
                    this.annotate_resize = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _e, _w, cx| {
                    this.annotate_resize = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    fn toggle_reveal_whitespace_chars(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_diff_reveal_whitespace_chars_and_persist(!self.reveal_whitespace_chars, cx);
    }

    fn activate_diff_search(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        // Search used to drop every rendered preview back to Source here so there
        // was plain text to scan. It scans the rendered markdown rows instead,
        // so opening the search box no longer changes what you are looking at —
        // except over a rendered *picture*, which has no text at all and would
        // otherwise leave search with nothing to find.
        if self.is_conflict_rendered_preview_active()
            && !self.is_conflict_rendered_markdown_preview_active()
        {
            self.conflict_resolver.resolver_preview_mode = ConflictResolverPreviewMode::Text;
        }
        let was_search_active = self.diff_search_active;
        self.diff_search_active = true;
        self.clear_diff_text_query_overlay_cache();
        self.worktree_preview_segments_cache_path = None;
        self.worktree_preview_segments_cache.clear();
        self.clear_conflict_diff_query_overlay_caches();
        self.diff_search_cancel_pending_query_recompute();
        if was_search_active {
            self.diff_search_recompute_matches();
        } else {
            self.diff_search_recompute_matches_and_scroll_to_first();
        }
        let focus = self.diff_search_input.read(cx).focus_handle();
        window.focus(&focus, cx);
        self.diff_search_input
            .update(cx, |input, cx| input.select_all_text(cx));
    }

    fn deactivate_diff_search(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.diff_search_cancel_pending_query_recompute();
        self.diff_search_active = false;
        self.diff_search_query = SharedString::default();
        self.diff_search_regex_error = None;
        self.diff_search_matches.clear();
        self.diff_search_match_ix = None;
        self.diff_search_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.diff_search_scroll.set_offset(point(px(0.0), px(0.0)));
        self.clear_diff_text_query_overlay_cache();
        self.clear_worktree_preview_segments_cache();
        self.clear_conflict_diff_query_overlay_caches();
        self.markdown_preview_reveal.clear();
        // Hand the buffer back, caret still on the match. The panel focus handle
        // every other view returns to would drop the user out of the text.
        if self.is_file_editor_active() {
            self.file_editor_search_clear();
            let focus = self.file_editor_input.read(cx).focus_handle();
            window.focus(&focus, cx);
            return;
        }
        window.focus(&self.diff_panel_focus_handle, cx);
    }

    fn focus_diff_search_input(&self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let focus = self.diff_search_input.read(cx).focus_handle();
        window.focus(&focus, cx);
    }

    fn refresh_diff_search_after_option_change(&mut self) {
        let query = self.diff_search_query.clone();
        self.invalidate_diff_text_query_overlay_cache(query.as_ref(), self.diff_search_options);
        self.clear_worktree_preview_segments_cache();
        self.clear_conflict_diff_query_overlay_caches();
        self.diff_search_cancel_pending_query_recompute();
        self.diff_search_recompute_matches_and_scroll_to_first();
    }

    fn set_diff_search_options(
        &mut self,
        next: DiffSearchOptions,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_search_options != next {
            self.diff_search_options = next;
            self.refresh_diff_search_after_option_change();
        }
        self.focus_diff_search_input(window, cx);
        cx.notify();
    }

    fn insert_diff_search_line_break(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.diff_search_input.update(cx, |input, cx| {
            input.replace_selection_utf8("\n", cx);
        });
        self.focus_diff_search_input(window, cx);
        cx.notify();
    }

    fn restore_diff_panel_focus_after_toolbar_action(
        &self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let focus = self.diff_panel_focus_handle.clone();
        window.focus(&focus, cx);
        cx.focus_self(window);
        let focus = self.diff_panel_focus_handle.clone();
        window.on_next_frame(move |window, cx| {
            window.focus(&focus, cx);
        });
    }

    /// The "Blame" toggle button, shared by the diff toolbar and the file
    /// content view so annotations can be toggled in either.
    fn diff_annotate_toggle_button(
        &self,
        theme: AppTheme,
        selected_bg: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use worktree_state::model::Loadable;
        // Reflect blame load status on the toggle so a failed or slow blame is not
        // a silently blank annotation column: the column is only drawn from `blame`
        // Ready, so when it is Loading/Error this control is the user-facing
        // feedback. Toggling off then on retries (see request_blame_for_current_target).
        let blame_status = self
            .annotate_enabled
            .then(|| self.active_repo().map(|repo| &repo.history_state.blame))
            .flatten();
        // A rendered preview has no annotation gutter to draw into, so the
        // toggle greys out there rather than silently doing nothing — matching
        // Alt+B, which is inert for the same reason. Text mode still annotates.
        let preview_blocks_blame = self.is_markdown_preview_active();
        let (tooltip, errored): (SharedString, bool) = if preview_blocks_blame {
            (
                crate::i18n::tr("diff.tooltip.blame_preview_unavailable"),
                false,
            )
        } else {
            match blame_status {
                Some(Loadable::Loading) => (crate::i18n::tr("diff.tooltip.blame_loading"), false),
                Some(Loadable::Error(message)) => (
                    crate::i18n::t!("diff.tooltip.blame_failed", message = message)
                        .to_string()
                        .into(),
                    true,
                ),
                _ => (
                    crate::i18n::t!(
                        "diff.tooltip.blame_toggle",
                        shortcut = crate::view::shortcut_labels::alt_shortcut("B")
                    )
                    .to_string()
                    .into(),
                    false,
                ),
            }
        };
        let selected_bg = if errored {
            with_alpha(
                theme.colors.status.danger.foreground,
                if theme.is_dark { 0.30 } else { 0.20 },
            )
        } else {
            selected_bg
        };
        components::Button::new("diff_annotate", crate::i18n::tr("diff.toolbar.blame"))
            .borderless()
            .style(components::ButtonStyle::Subtle)
            .disabled(preview_blocks_blame)
            .selected(self.annotate_enabled)
            .selected_bg(selected_bg)
            .on_click(theme, cx, |this, _e, window, cx| {
                let next = !this.annotate_enabled;
                this.restore_diff_panel_focus_after_toolbar_action(window, cx);
                let root_view = this.root_view.clone();
                cx.defer(move |cx| {
                    if let Some(root) = root_view.upgrade() {
                        root.update(cx, |root, cx| {
                            root.set_annotate_enabled(next, cx);
                        });
                    }
                });
                cx.notify();
            })
            .debug_selector(|| "diff_annotate".to_string())
            .worktree_tooltip(theme, tooltip)
    }

    /// The "Edit" toggle, shared by the diff toolbar and the file content view.
    ///
    /// Turning it on always goes through `OpenFileEditor`, which re-targets the
    /// working tree — so pressing Edit on a commit's diff or content opens the
    /// workspace copy of that file, which is the only copy that can be written.
    fn file_edit_toggle_button(
        &self,
        theme: AppTheme,
        selected_bg: gpui::Rgba,
        editing: bool,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let dirty = editing && self.file_editor_is_dirty();
        let path = self.editable_path_for_current_target();
        // A rendered preview has no buffer to type into, and a file with no
        // working-tree path (a range comparison, a whole-tree diff) has nothing
        // to edit.
        let blocked_by_preview = self.is_markdown_preview_active();
        let blocked_by_kind = path
            .as_ref()
            .is_some_and(|p| self.content_preview_is_picture(p));
        let disabled = path.is_none() || blocked_by_preview || blocked_by_kind;
        let tooltip: SharedString = if blocked_by_preview {
            crate::i18n::tr("diff.tooltip.edit_preview_unavailable")
        } else if blocked_by_kind {
            crate::i18n::tr("diff.tooltip.edit_not_text")
        } else if path.is_none() {
            crate::i18n::tr("diff.tooltip.edit_no_working_tree_file")
        } else if dirty {
            crate::i18n::t!(
                "diff.tooltip.edit_unsaved",
                shortcut = crate::view::shortcut_labels::secondary_shortcut("S")
            )
            .to_string()
            .into()
        } else {
            crate::i18n::t!(
                "diff.tooltip.edit_toggle",
                shortcut = crate::view::shortcut_labels::alt_shortcut("E")
            )
            .to_string()
            .into()
        };

        components::Button::new(
            "diff_edit",
            if dirty {
                crate::i18n::tr("diff.toolbar.edit_dirty")
            } else {
                crate::i18n::tr("diff.toolbar.edit")
            },
        )
        .borderless()
        .style(components::ButtonStyle::Subtle)
        .disabled(disabled)
        .selected(editing)
        .selected_bg(selected_bg)
        .on_click(theme, cx, move |this, _e, window, cx| {
            this.toggle_file_editor(window, cx);
        })
        .debug_selector(|| "diff_edit".to_string())
        .worktree_tooltip(theme, tooltip)
    }

    /// Whether the file on screen has text the editor can open.
    ///
    /// Pictures have none — but an SVG showing its Code *is* source, and editing
    /// it is exactly what that toggle was for. Shared by the toolbar button, the
    /// Alt+E shortcut and the context-menu entries so the three cannot disagree
    /// about what is editable.
    pub(in crate::view) fn can_edit_current_target(&self) -> bool {
        self.editable_path_for_current_target()
            .is_some_and(|path| !self.content_preview_is_picture(&path))
    }

    /// Enter or leave the editor for the file on screen.
    pub(in crate::view) fn toggle_file_editor(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        if self.is_file_editor_active() {
            // Whatever is unsaved is either written or kept, never dropped.
            self.flush_file_editor_buffer(cx);
            self.store.dispatch(Msg::ExitDiffEditMode { repo_id });
            self.restore_diff_panel_focus_after_toolbar_action(window, cx);
            cx.notify();
            return;
        }
        let Some(path) = self.editable_path_for_current_target() else {
            return;
        };
        self.store.dispatch(Msg::OpenFileEditor { repo_id, path });
        // The buffer is seeded a frame later, so focus has to wait for it.
        let input = self.file_editor_input.clone();
        window.on_next_frame(move |window, cx| {
            let handle = input.read(cx).focus_handle().clone();
            window.focus(&handle, cx);
            input.update(cx, |_, cx| cx.notify());
        });
        cx.notify();
    }

    /// Throw the buffer away and restore the view that opened the editor.
    ///
    /// Discarding is the user saying they are done with this edit, so it exits
    /// the way Escape and the Edit toggle do rather than leaving them parked in
    /// an editor over text they just abandoned. Deliberately *not* routed
    /// through `toggle_file_editor`, whose exit path flushes the buffer — it
    /// would stash the very edits this is dropping.
    pub(in crate::view) fn discard_file_editor_buffer_and_exit(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.discard_file_editor_buffer(cx);
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        self.store.dispatch(Msg::ExitDiffEditMode { repo_id });
        self.restore_diff_panel_focus_after_toolbar_action(window, cx);
        cx.notify();
    }

    /// Save the editable buffer and restore the view that opened it.
    pub(in crate::view) fn save_file_editor_buffer_and_exit(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // The toolbar button is disabled in these states, but the keyboard
        // shortcut can still arrive. A no-op save must not unexpectedly act as
        // an editor-close shortcut.
        if self.file_editor_loading
            || !self.file_editor_is_dirty()
            || self.file_editor_key.is_none()
        {
            return;
        }
        self.save_file_editor_buffer(cx);
        let Some(repo_id) = self.active_repo_id() else {
            return;
        };
        self.store.dispatch(Msg::ExitDiffEditMode { repo_id });
        self.restore_diff_panel_focus_after_toolbar_action(window, cx);
        cx.notify();
    }

    /// The explicit "Save" button, shown while editing with auto-save off.
    fn file_editor_save_button(
        &self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        components::Button::new("file_editor_save", crate::i18n::tr("diff.toolbar.save"))
            .style(components::ButtonStyle::Outlined)
            .disabled(!self.file_editor_is_dirty())
            .on_click(theme, cx, |this, _e, window, cx| {
                this.save_file_editor_buffer_and_exit(window, cx);
            })
            .debug_selector(|| "file_editor_save".to_string())
            .worktree_tooltip(
                theme,
                crate::i18n::t!(
                    "diff.tooltip.save_and_return",
                    shortcut = crate::view::shortcut_labels::secondary_shortcut("S")
                )
                .to_string()
                .into(),
            )
    }

    /// "Discard", shown beside Save while editing.
    ///
    /// Throws the buffer away and re-reads the file, with no confirmation: the
    /// button is only enabled while there is something to discard, and it sits
    /// next to the Save that is the alternative. The close/quit dialog is the
    /// place that asks, because there the choice is being forced on the user
    /// rather than made by them.
    fn file_editor_discard_button(
        &self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let dirty = self.file_editor_is_dirty();
        components::Button::new(
            "file_editor_discard",
            crate::i18n::tr("diff.toolbar.discard"),
        )
        .style(components::ButtonStyle::Subtle)
        .borderless()
        .disabled(!dirty)
        .on_click(theme, cx, |this, _e, window, cx| {
            this.discard_file_editor_buffer_and_exit(window, cx);
        })
        .debug_selector(|| "file_editor_discard".to_string())
        .worktree_tooltip(
            theme,
            if dirty {
                crate::i18n::tr("diff.tooltip.discard_dirty")
            } else {
                crate::i18n::tr("diff.tooltip.discard_clean")
            },
        )
    }

    pub(in crate::view) fn open_search_for_active_view(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let diff_visible = self
            .active_repo()
            .and_then(|repo| repo.diff_state.diff_target.as_ref())
            .is_some();
        if !diff_visible {
            return false;
        }

        self.activate_diff_search(window, cx);
        true
    }

    pub(in crate::view) fn diff_view(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Div {
        let theme = self.theme;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let repo_id = self.active_repo_id();
        let editor_font_family = crate::font_preferences::current_editor_font_family(cx);

        // Intentionally no outer panel header; keep diff controls in the inner header.

        let title = self.diff_panel_title(theme, cx);
        let viewer_nav = self.diff_viewer_nav_cluster(theme, cx);
        let inline_submodule_diff_active = self.is_inline_submodule_diff_active();

        let has_submodule_summary = self
            .active_repo()
            .is_some_and(|repo| !matches!(repo.diff_state.submodule_summary, Loadable::NotLoaded));
        let untracked_directory_notice = if has_submodule_summary || inline_submodule_diff_active {
            None
        } else {
            self.untracked_directory_notice()
        };

        let is_file_preview = self.is_file_preview_active()
            && untracked_directory_notice.is_none()
            && !has_submodule_summary
            && !inline_submodule_diff_active;
        let supports_diff_content_toggle = (inline_submodule_diff_active || !has_submodule_summary)
            && self.supports_diff_content_mode_toggle(is_file_preview);

        // Browsing a historical commit: tint the header and the content surface
        // instead of framing the pane, and only while the content on screen is
        // the browsed commit's.
        let historical_browse = is_file_preview && self.historical_browse_content_active();
        let content_bg = if historical_browse {
            crate::theme::historical_surface_bg(theme, theme.colors.surface.canvas)
        } else {
            theme.colors.surface.canvas
        };

        // Deliberately not gated on `is_file_preview`: that predicate also asks
        // whether the path is *previewable*, and a file the preview declines
        // (an unknown extension, say) is still a file the editor can open — it
        // does its own UTF-8 check and reports the failure in place.
        let is_file_editor = self.is_file_editor_active()
            && untracked_directory_notice.is_none()
            && !has_submodule_summary
            && !inline_submodule_diff_active;
        if is_file_editor {
            self.ensure_file_editor_loaded(cx);
        } else if is_file_preview {
            self.ensure_selected_file_preview_loaded(cx);
        } else if (has_submodule_summary
            || inline_submodule_diff_active
            || untracked_directory_notice.is_some())
            && matches!(self.worktree_preview, Loadable::Loading)
        {
            self.worktree_preview_path = None;
            self.worktree_preview = Loadable::NotLoaded;
            self.reset_worktree_preview_source_state();
            self.reset_diff_horizontal_scroll_state();
        }
        let wants_file_diff =
            supports_diff_content_toggle && self.wants_file_diff_view(is_file_preview);
        let wants_collapsed_diff =
            supports_diff_content_toggle && self.wants_collapsed_diff_view(is_file_preview);

        let repo = self.active_repo();
        let conflict_target = (!inline_submodule_diff_active)
            .then_some(())
            .and(repo)
            .and_then(|repo| {
                let DiffTarget::WorkingTree { path, area } =
                    repo.diff_state.diff_target.as_ref()?
                else {
                    return None;
                };
                if *area != DiffArea::Unstaged {
                    return None;
                }
                let conflict = repo
                    .status_entry_for_path(DiffArea::Unstaged, path.as_path())
                    .filter(|entry| entry.kind == FileStatusKind::Conflicted)?;
                Some((path.clone(), conflict.conflict))
            });
        let (conflict_target_path, conflict_kind) = conflict_target
            .map(|(path, kind)| (Some(path), kind))
            .unwrap_or((None, None));
        let conflict_file_state = match (repo, conflict_target_path.as_deref()) {
            (Some(repo), Some(path)) => Some(renderable_conflict_file(
                repo,
                &self.conflict_resolver,
                path,
            )),
            _ => None,
        };
        // Detect binary from the renderable conflict file, including the
        // same-target cached snapshot we keep during transient reloads.
        let is_binary_conflict = conflict_file_state
            .and_then(|state| match state {
                RenderableConflictFile::File(file) => Some(conflict_file_is_binary(&file)),
                _ => None,
            })
            .unwrap_or(false);
        let conflict_strategy = {
            // The loaded session's strategy is authoritative when it is for
            // this path: it sees payload facts the local recomputation
            // cannot — a submodule conflict's stages are commit pointers
            // rendered as short sha text, not text to merge — so it routes
            // those to the side-pick resolver instead of a bogus text
            // merge. The recomputation stays as the pre-session fallback.
            let session_strategy = repo.and_then(|repo| {
                let session = repo.conflict_state.conflict_session.as_ref()?;
                let path = conflict_target_path.as_deref()?;
                (session.path.as_path() == path).then_some(session.strategy)
            });
            session_strategy
                .or_else(|| Self::conflict_resolver_strategy(conflict_kind, is_binary_conflict))
        };
        let is_conflict_resolver = conflict_strategy.is_some();
        let is_conflict_compare = conflict_target_path.is_some() && conflict_strategy.is_none();
        let conflict_rendered_preview_active = self.is_conflict_rendered_preview_active();

        let rendered_preview_kind = diff_target_rendered_preview_kind(self.rendered_diff_target());
        let rendered_view_toggle_kind = main_diff_rendered_preview_toggle_kind(
            wants_file_diff,
            wants_collapsed_diff,
            is_file_preview,
            rendered_preview_kind,
        );
        let is_markdown_preview_view = rendered_view_toggle_kind
            == Some(RenderedPreviewKind::Markdown)
            && self
                .rendered_preview_modes
                .get(RenderedPreviewKind::Markdown)
                == RenderedPreviewMode::Rendered;
        let is_image_diff_loaded = wants_file_diff
            && self
                .rendered_file_image_diff_loadable()
                .is_some_and(|file| !matches!(file, Loadable::NotLoaded));
        let is_image_diff_view = wants_file_diff
            && is_image_diff_loaded
            && (!matches!(rendered_preview_kind, Some(RenderedPreviewKind::Svg))
                || self.rendered_preview_modes.get(RenderedPreviewKind::Svg)
                    == RenderedPreviewMode::Rendered);

        let (prev_file_btn, next_file_btn) = if show_diff_file_navigation(self.view_mode) {
            self.diff_prev_next_file_buttons(repo_id, is_conflict_resolver, theme, cx)
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
        let is_simple_conflict_strategy = matches!(
            self.conflict_resolver.strategy,
            Some(
                worktree_core::conflict_session::ConflictResolverStrategy::BinarySidePick
                    | worktree_core::conflict_session::ConflictResolverStrategy::TwoWayKeepDelete
                    | worktree_core::conflict_session::ConflictResolverStrategy::DecisionOnly
            )
        );
        if is_conflict_resolver && is_simple_conflict_strategy {
            controls = self.conflict_toolbar_simple_controls(
                controls,
                prev_file_btn,
                next_file_btn,
                theme,
            );
        } else if is_conflict_resolver {
            controls = self.conflict_toolbar_full_controls(
                controls,
                prev_file_btn,
                next_file_btn,
                conflict_rendered_preview_active,
                repo_id,
                &conflict_target_path,
                theme,
                cx,
            );
        } else if !is_file_preview && !is_file_editor {
            let view_toggle_selected_bg = with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.26 } else { 0.20 },
            );
            let view_toggle_border = with_alpha(
                theme.colors.foreground.secondary,
                if theme.is_dark { 0.38 } else { 0.28 },
            );
            let view_toggle_divider = with_alpha(view_toggle_border, 0.90);

            if supports_diff_content_toggle {
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

            if !is_image_diff_view {
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
                    .file_edit_toggle_button(theme, view_toggle_selected_bg, is_file_editor, cx)
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
                    // `is_file_editor`, not `is_file_editor_active`: edit mode
                    // can be on while a submodule summary or an
                    // untracked-directory notice owns the body, and a Save
                    // control over a body with no buffer is a trap.
                    // Discard sits before Save, so the pair reads as the two
                    // ways out of an unsaved buffer in the order they are meant.
                    .when(is_file_editor && !self.auto_save_file_edits, |d| {
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
                    is_file_editor,
                    cx,
                ))
                .child(self.diff_annotate_toggle_button(theme, annotate_selected_bg, cx))
                // Saving is explicit only when auto-save is off; with it on the
                // button would never be enabled long enough to click, and
                // neither would the Discard beside it.
                .when(is_file_editor && !self.auto_save_file_edits, |d| {
                    d.child(self.file_editor_discard_button(theme, cx))
                        .child(self.file_editor_save_button(theme, cx))
                });
        }

        if !is_conflict_resolver && let Some(preview_kind) = rendered_view_toggle_kind {
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
            let resolver_settings_active = is_conflict_resolver && !is_simple_conflict_strategy;
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

        let header = div()
            .debug_selector(|| "diff_file_header".to_string())
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .h(components::control_height_md(ui_scale_percent))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(div().min_w(px(0.0)).overflow_hidden().child(title))
                    .when_some(viewer_nav, |d, cluster| d.child(cluster)),
            )
            .child(
                // Right-anchor the controls and clip from the leading edge so a
                // narrow pane hides lower-priority buttons instead of pushing
                // the action menu / close button past the pane clip, where they
                // still paint but can no longer be clicked.
                div()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .child(controls),
            );

        let body: AnyElement = if has_submodule_summary && !inline_submodule_diff_active {
            self.render_submodule_summary(theme, cx)
        } else if let Some(message) = untracked_directory_notice {
            components::empty_state(theme, crate::i18n::tr("diff.pane.directory"), message)
                .into_any_element()
        } else if is_file_editor {
            self.render_file_editor(theme, cx)
        } else if is_file_preview {
            if is_markdown_preview_view {
                match &self.worktree_preview {
                    Loadable::NotLoaded | Loadable::Loading => components::empty_state(
                        theme,
                        crate::i18n::tr("diff.pane.preview"),
                        crate::i18n::tr("diff.common.loading"),
                    )
                    .into_any_element(),
                    Loadable::Error(e) => components::empty_state(
                        theme,
                        crate::i18n::tr("diff.pane.preview"),
                        e.clone(),
                    )
                    .into_any_element(),
                    Loadable::Ready(_) => {
                        self.ensure_single_markdown_preview_cache(cx);
                        self.watch_pending_markdown_preview_images(cx);
                        match &self.worktree_markdown_preview {
                            Loadable::NotLoaded | Loadable::Loading => components::empty_state(
                                theme,
                                crate::i18n::tr("diff.pane.preview"),
                                crate::i18n::tr("diff.common.loading"),
                            )
                            .into_any_element(),
                            Loadable::Error(e) => components::empty_state(
                                theme,
                                crate::i18n::tr("diff.pane.preview"),
                                e.clone(),
                            )
                            .into_any_element(),
                            Loadable::Ready(document) => {
                                if document.rows.is_empty() {
                                    let message = if self.worktree_preview_line_count() == Some(0) {
                                        crate::i18n::tr("diff.common.empty_file")
                                    } else {
                                        crate::i18n::tr("diff.common.nothing_to_render")
                                    };
                                    components::empty_state(
                                        theme,
                                        crate::i18n::tr("diff.pane.preview"),
                                        message,
                                    )
                                    .into_any_element()
                                } else {
                                    // A single document lays out as one flowing
                                    // element tree rather than a uniform row
                                    // list: text wraps by itself, images sit at
                                    // their own size, and the gaps around
                                    // headings are margins.
                                    self.markdown_preview_wrap
                                        .clear_list(MarkdownPreviewList::Worktree);
                                    let document = std::sync::Arc::clone(document);
                                    let image_base_dir = self
                                        .markdown_preview_image_base_dir()
                                        .map(|dir| std::sync::Arc::from(dir.as_path()));
                                    let body = rows::render_markdown_document(
                                        &document,
                                        &rows::MarkdownDocumentContext {
                                            theme,
                                            ui_scale_percent,
                                            editor_font_family: editor_font_family.clone().into(),
                                            image_base_dir,
                                            picture_sizes: std::sync::Arc::clone(
                                                &self.worktree_markdown_preview_picture_sizes,
                                            ),
                                            block_scrolls: self
                                                .worktree_markdown_preview_block_scrolls
                                                .clone(),
                                            blocks: self.worktree_markdown_preview_blocks.clone(),
                                            view: Some(cx.entity()),
                                            text_region: DiffTextRegion::Inline,
                                            change_bar_color:
                                                rows::worktree_markdown_preview_bar_color(
                                                    self, theme,
                                                ),
                                            query: self.markdown_preview_search_query(),
                                            reveal: self.markdown_preview_reveal.clone(),
                                            scroll: Some(
                                                self.worktree_preview_scroll
                                                    .0
                                                    .borrow()
                                                    .base_handle
                                                    .clone(),
                                            ),
                                        },
                                    );

                                    let scroll_handle =
                                        self.worktree_preview_scroll.0.borrow().base_handle.clone();
                                    let scrollbar_gutter = components::Scrollbar::visible_gutter(
                                        scroll_handle.clone(),
                                        components::ScrollbarAxis::Vertical,
                                    );
                                    let edge_gap = crate::ui_scale::design_px_from_percent(
                                        super::diff::MARKDOWN_PREVIEW_DOCUMENT_EDGE_GAP_PX,
                                        ui_scale_percent,
                                    );
                                    div()
                                        .id("worktree_markdown_preview_scroll_container")
                                        .debug_selector(|| {
                                            "worktree_markdown_preview_scroll_container".to_string()
                                        })
                                        .relative()
                                        .h_full()
                                        .min_h(px(0.0))
                                        .bg(content_bg)
                                        .child(
                                            div()
                                                .id("worktree_markdown_preview_document")
                                                .debug_selector(|| {
                                                    "worktree_markdown_preview_document".to_string()
                                                })
                                                .size_full()
                                                .min_h(px(0.0))
                                                .overflow_y_scroll()
                                                .track_scroll(&scroll_handle)
                                                .pt(edge_gap)
                                                .pb(edge_gap)
                                                .pr(scrollbar_gutter)
                                                .child(body),
                                        )
                                        .child(
                                            components::Scrollbar::new(
                                                "worktree_markdown_preview_scrollbar",
                                                scroll_handle,
                                            )
                                            .render(theme),
                                        )
                                        .into_any_element()
                                }
                            }
                        }
                    }
                }
            } else {
                match &self.worktree_preview {
                    Loadable::NotLoaded | Loadable::Loading => components::empty_state(
                        theme,
                        crate::i18n::tr("diff.pane.file"),
                        crate::i18n::tr("diff.common.loading"),
                    )
                    .into_any_element(),
                    Loadable::Error(e) => {
                        self.diff_raw_input.update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text(e.clone(), cx);
                            input.set_read_only(true, cx);
                        });
                        div()
                            .id("worktree_preview_error_scroll")
                            .bg(content_bg)
                            .font_family(editor_font_family.clone())
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .child(self.diff_raw_input.clone())
                            .into_any_element()
                    }
                    Loadable::Ready(line_count) => {
                        let line_count = *line_count;
                        if line_count == 0 {
                            components::empty_state(
                                theme,
                                crate::i18n::tr("diff.pane.file"),
                                crate::i18n::tr("diff.common.empty_file"),
                            )
                            .into_any_element()
                        } else {
                            // Word wrap turns one line into several rows, so the
                            // projection has to be built before the list is
                            // asked how long it is.
                            self.ensure_diff_wrap_visible_rows(window, cx);
                            let row_count = self
                                .worktree_preview_visible_len()
                                .unwrap_or(line_count)
                                .max(1);
                            let wrapped = self.worktree_preview_wrap_active();
                            let list = uniform_list(
                                "worktree_preview_list",
                                row_count,
                                cx.processor(Self::render_worktree_preview_rows),
                            )
                            .h_full()
                            .min_h(px(0.0))
                            .track_scroll(&self.worktree_preview_scroll)
                            .with_horizontal_sizing_behavior(
                                gpui::ListHorizontalSizingBehavior::Unconstrained,
                            );

                            let scroll_handle =
                                self.worktree_preview_scroll.0.borrow().base_handle.clone();
                            let scrollbar_gutter = components::Scrollbar::visible_gutter(
                                scroll_handle.clone(),
                                components::ScrollbarAxis::Vertical,
                            );
                            let annotate_handle = self
                                .annotate_enabled
                                .then(|| self.annotate_resize_handle(ui_scale_percent, theme, cx));
                            div()
                                .id("worktree_preview_scroll_container")
                                .debug_selector(|| "worktree_preview_scroll_container".to_string())
                                .relative()
                                .h_full()
                                .min_h(px(0.0))
                                .bg(content_bg)
                                .font_family(editor_font_family.clone())
                                .child(
                                    div()
                                        .h_full()
                                        .min_h(px(0.0))
                                        .pr(scrollbar_gutter)
                                        .child(list),
                                )
                                .child(
                                    components::Scrollbar::new(
                                        "worktree_preview_scrollbar",
                                        scroll_handle.clone(),
                                    )
                                    .render(theme),
                                )
                                // Wrapped rows end at the pane, so there is
                                // nothing left of the line to scroll to.
                                .when(!wrapped, |container| {
                                    container.child(
                                        components::Scrollbar::horizontal(
                                            "worktree_preview_hscrollbar",
                                            scroll_handle,
                                        )
                                        .always_visible()
                                        .render(theme),
                                    )
                                })
                                .when_some(annotate_handle, |container, handle| {
                                    container.child(handle)
                                })
                                .into_any_element()
                        }
                    }
                }
            }
        } else if is_conflict_resolver {
            self.render_conflict_resolver_pane(
                conflict_target_path,
                repo_id,
                theme,
                ui_scale_percent,
                editor_font_family.clone(),
                cx,
            )
        } else if is_conflict_compare {
            match (repo, conflict_target_path) {
                (None, _) => components::empty_state(
                    theme,
                    crate::i18n::tr("diff.pane.resolve"),
                    crate::i18n::tr("diff.common.no_repository"),
                )
                .into_any_element(),
                (_, None) => components::empty_state(
                    theme,
                    crate::i18n::tr("diff.pane.resolve"),
                    crate::i18n::tr("diff.conflict.no_file_selected"),
                )
                .into_any_element(),
                (Some(repo), Some(path)) => {
                    let path_display = self.cached_path_display(&path);
                    let title: SharedString =
                        crate::i18n::t!("diff.conflict.title", path = path_display)
                            .to_string()
                            .into();

                    match renderable_conflict_file(repo, &self.conflict_resolver, &path) {
                        RenderableConflictFile::Loading => components::empty_state(
                            theme,
                            title,
                            crate::i18n::tr("diff.conflict.loading_data"),
                        )
                        .into_any_element(),
                        RenderableConflictFile::Error(error) => {
                            components::empty_state(theme, title, error).into_any_element()
                        }
                        RenderableConflictFile::Missing => components::empty_state(
                            theme,
                            title,
                            crate::i18n::tr("diff.conflict.no_data"),
                        )
                        .into_any_element(),
                        RenderableConflictFile::File(file) => {
                            let ours_label: SharedString = if file.ours.is_some() {
                                crate::i18n::tr("diff.conflict.ours")
                            } else {
                                crate::i18n::tr("diff.conflict.ours_deleted")
                            };
                            let theirs_label: SharedString = if file.theirs.is_some() {
                                crate::i18n::tr("diff.conflict.theirs")
                            } else {
                                crate::i18n::tr("diff.conflict.theirs_deleted")
                            };

                            // The body reserves this gutter for its vertical scrollbar; the
                            // header has to reserve it too or the two halves split a wider
                            // box than the rows do and the labels drift off their columns.
                            let compare_scrollbar_gutter = components::Scrollbar::visible_gutter(
                                self.diff_scroll.clone(),
                                components::ScrollbarAxis::Vertical,
                            );
                            let columns_header = components::split_columns_header(
                                theme,
                                ui_scale_percent,
                                ours_label,
                                theirs_label,
                            )
                            .pr(compare_scrollbar_gutter);

                            let diff_len = self.conflict_resolver.two_way_split_visible_len();

                            let diff_body: AnyElement = if diff_len == 0 {
                                components::empty_state(
                                    theme,
                                    crate::i18n::tr("diff.pane.diff"),
                                    crate::i18n::tr("diff.conflict.no_diff_to_show"),
                                )
                                .into_any_element()
                            } else {
                                let scroll_handle = self.diff_scroll.0.borrow().base_handle.clone();
                                let list = uniform_list(
                                    "conflict_compare_diff",
                                    diff_len,
                                    cx.processor(Self::render_conflict_compare_diff_rows),
                                )
                                .h_full()
                                .min_h(px(0.0))
                                .track_scroll(&self.diff_scroll)
                                .with_horizontal_sizing_behavior(
                                    gpui::ListHorizontalSizingBehavior::Unconstrained,
                                );

                                div()
                                    .id("conflict_compare_container")
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .h_full()
                                    .min_h(px(0.0))
                                    .bg(theme.colors.surface.canvas)
                                    .font_family(editor_font_family.clone())
                                    .child(columns_header)
                                    .child(
                                        div()
                                            .id("conflict_compare_scroll_container")
                                            .relative()
                                            .flex_1()
                                            .min_h(px(0.0))
                                            .child(
                                                div()
                                                    .h_full()
                                                    .min_h(px(0.0))
                                                    .pr(compare_scrollbar_gutter)
                                                    .child(list),
                                            )
                                            .child(
                                                components::Scrollbar::new(
                                                    "conflict_compare_scrollbar",
                                                    self.diff_scroll.clone(),
                                                )
                                                .always_visible()
                                                .render(theme),
                                            )
                                            .child(
                                                components::Scrollbar::horizontal(
                                                    "conflict_compare_hscrollbar",
                                                    scroll_handle,
                                                )
                                                .always_visible()
                                                .render(theme),
                                            ),
                                    )
                                    .into_any_element()
                            };

                            diff_body
                        }
                    }
                }
            }
        } else if wants_file_diff || wants_collapsed_diff {
            self.render_selected_file_diff(theme, window, cx)
        } else {
            match repo {
                None => components::empty_state(
                    theme,
                    crate::i18n::tr("diff.pane.diff"),
                    crate::i18n::tr("diff.common.no_repository"),
                )
                .into_any_element(),
                Some(_repo) => match self.rendered_patch_diff_loadable() {
                    Some(Loadable::NotLoaded) | None => components::empty_state(
                        theme,
                        crate::i18n::tr("diff.pane.diff"),
                        crate::i18n::tr("diff.common.select_a_file"),
                    )
                    .into_any_element(),
                    Some(Loadable::Loading) => components::empty_state(
                        theme,
                        crate::i18n::tr("diff.pane.diff"),
                        crate::i18n::tr("diff.common.loading"),
                    )
                    .into_any_element(),
                    Some(Loadable::Error(e)) => {
                        self.diff_raw_input.update(cx, |input, cx| {
                            input.set_theme(theme, cx);
                            input.set_text(e.clone(), cx);
                            input.set_read_only(true, cx);
                        });
                        div()
                            .id("diff_error_scroll")
                            .font_family(editor_font_family.clone())
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .child(self.diff_raw_input.clone())
                            .into_any_element()
                    }
                    Some(Loadable::Ready(_diff)) => {
                        if wants_file_diff || wants_collapsed_diff {
                            self.render_selected_file_diff(theme, window, cx)
                        } else {
                            self.ensure_diff_visible_indices();
                            self.ensure_diff_wrap_visible_rows(window, cx);
                            self.maybe_autoscroll_diff_to_first_change();

                            {
                                if self.patch_diff_row_len() == 0 {
                                    components::empty_state(
                                        theme,
                                        crate::i18n::tr("diff.pane.diff"),
                                        crate::i18n::tr("diff.common.no_differences"),
                                    )
                                    .into_any_element()
                                } else if self.diff_visible_len() == 0 {
                                    components::empty_state(
                                        theme,
                                        crate::i18n::tr("diff.pane.diff"),
                                        crate::i18n::tr("diff.common.nothing_to_render"),
                                    )
                                    .into_any_element()
                                } else {
                                    let markers = self.diff_scrollbar_markers_cache.clone();
                                    match self.diff_view {
                                        DiffViewMode::Inline => {
                                            let horizontal_scrollbar_gutter =
                                                components::Scrollbar::gutter(
                                                    components::ScrollbarAxis::Horizontal,
                                                );
                                            let scrollbar_gutter = self
                                                .diff_vertical_scrollbar_gutter_for_column(
                                                    DiffHorizontalScrollColumn::Primary,
                                                    self.diff_scroll.clone(),
                                                );
                                            let list = uniform_list(
                                                "diff",
                                                self.diff_visible_len(),
                                                cx.processor(Self::render_diff_rows),
                                            )
                                            .h_full()
                                            .min_h(px(0.0))
                                            .pb(if self.diff_word_wrap {
                                                px(0.0)
                                            } else {
                                                horizontal_scrollbar_gutter
                                            })
                                            .track_scroll(&self.diff_scroll)
                                            .when(!self.diff_word_wrap, |list| {
                                                list.with_horizontal_sizing_behavior(
                                                    gpui::ListHorizontalSizingBehavior::Unconstrained,
                                                )
                                            });
                                            div()
                                                .id("diff_scroll_container")
                                                .relative()
                                                .h_full()
                                                .min_h(px(0.0))
                                                .bg(theme.colors.surface.canvas)
                                                .font_family(editor_font_family.clone())
                                                .child(
                                                    div()
                                                        .h_full()
                                                        .min_h(px(0.0))
                                                        .pr(scrollbar_gutter)
                                                        .child(list),
                                                )
                                                .child(
                                                    components::Scrollbar::new(
                                                        "diff_scrollbar",
                                                        self.diff_scroll.clone(),
                                                    )
                                                    .markers(markers)
                                                    .always_visible()
                                                    .render(theme),
                                                )
                                                .when(!self.diff_word_wrap, |d| {
                                                    d.child(Self::render_diff_horizontal_scrollbar(
                                                        theme,
                                                        "diff_hscrollbar",
                                                        self.diff_scroll.clone(),
                                                        scrollbar_gutter,
                                                        "diff_hscrollbar",
                                                    ))
                                                })
                                                .into_any_element()
                                        }
                                        DiffViewMode::Split => {
                                            self.sync_diff_split_scroll();
                                            let vertical_sync_enabled =
                                                self.diff_scroll_sync.includes_vertical();
                                            let count = self.diff_visible_len();
                                            let horizontal_scrollbar_gutter =
                                                components::Scrollbar::gutter(
                                                    components::ScrollbarAxis::Horizontal,
                                                );
                                            let left_scrollbar_gutter = self
                                                .diff_vertical_scrollbar_gutter_for_column(
                                                    DiffHorizontalScrollColumn::Primary,
                                                    self.diff_scroll.clone(),
                                                );
                                            let right_scrollbar_gutter = self
                                                .diff_vertical_scrollbar_gutter_for_column(
                                                    DiffHorizontalScrollColumn::SplitRight,
                                                    self.diff_split_right_scroll.clone(),
                                                );
                                            let shared_scrollbar_gutter = if vertical_sync_enabled {
                                                left_scrollbar_gutter
                                            } else {
                                                px(0.0)
                                            };
                                            let handle_w = px(PANE_RESIZE_HANDLE_PX);
                                            let main_w = (self.main_pane_content_width(cx)
                                                - shared_scrollbar_gutter)
                                                .max(px(0.0));
                                            let (_, min_col_w) = diff_split_drag_params(main_w);
                                            let (left_w, right_w) = diff_split_column_widths(
                                                main_w,
                                                self.diff_split_ratio,
                                            );
                                            let left = uniform_list(
                                                "diff_split_left",
                                                count,
                                                cx.processor(Self::render_diff_split_left_rows),
                                            )
                                            .h_full()
                                            .min_h(px(0.0))
                                            .pb(if self.diff_word_wrap {
                                                px(0.0)
                                            } else {
                                                horizontal_scrollbar_gutter
                                            })
                                            .track_scroll(&self.diff_scroll)
                                            .when(!self.diff_word_wrap, |list| {
                                                list.with_horizontal_sizing_behavior(
                                                    gpui::ListHorizontalSizingBehavior::Unconstrained,
                                                )
                                            });
                                            let right = uniform_list(
                                                "diff_split_right",
                                                count,
                                                cx.processor(Self::render_diff_split_right_rows),
                                            )
                                            .h_full()
                                            .min_h(px(0.0))
                                            .pb(if self.diff_word_wrap {
                                                px(0.0)
                                            } else {
                                                horizontal_scrollbar_gutter
                                            })
                                            .track_scroll(&self.diff_split_right_scroll)
                                            .when(!self.diff_word_wrap, |list| {
                                                list.with_horizontal_sizing_behavior(
                                                    gpui::ListHorizontalSizingBehavior::Unconstrained,
                                                )
                                            });
                                            let collapsed_file_stat = self
                                                .is_collapsed_diff_projection_active()
                                                .then(|| self.collapsed_diff_total_file_stat())
                                                .flatten();
                                            let (left_label, right_label) =
                                                self.split_diff_pane_labels();
                                            let left_header = Self::split_column_header_label(
                                                left_label,
                                                collapsed_file_stat.map(|(_, removed)| removed),
                                                '-',
                                                theme.colors.diff.removed.foreground,
                                            );
                                            let right_header = Self::split_column_header_label(
                                                right_label,
                                                collapsed_file_stat.map(|(added, _)| added),
                                                '+',
                                                theme.colors.diff.added.foreground,
                                            );

                                            let split_dragging = self.diff_split_resize.is_some();
                                            let resize_handle = |id: &'static str| {
                                                div()
                                                    .id(id)
                                                    .group(id)
                                                    .w(handle_w)
                                                    .h_full()
                                                    .cursor(CursorStyle::ResizeLeftRight)
                                                    .child(components::resize_grip(
                                                        theme,
                                                        ui_scale_percent,
                                                        id,
                                                        components::ResizeGripAxis::Vertical,
                                                        split_dragging,
                                                        Some(theme.colors.stroke.default),
                                                    ))
                                                    .on_drag(
                                                        DiffSplitResizeHandle::Divider,
                                                        |_handle, _offset, _window, cx| {
                                                            cx.new(|_cx| DiffSplitResizeDragGhost)
                                                        },
                                                    )
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |this,
                                                                  e: &MouseDownEvent,
                                                                  _w,
                                                                  cx| {
                                                                cx.stop_propagation();
                                                                crate::press_gesture::claim_press(
                                                                    cx,
                                                                );
                                                                this.diff_split_resize = Some(
                                                                    DiffSplitResizeState {
                                                                        handle:
                                                                            DiffSplitResizeHandle::Divider,
                                                                        start_x: e.position.x,
                                                                        start_ratio: this
                                                                            .diff_split_ratio,
                                                                    },
                                                                );
                                                                cx.notify();
                                                            },
                                                        ),
                                                    )
                                                    .on_drag_move(cx.listener(
                                                        move |this,
                                                              e: &gpui::DragMoveEvent<
                                                            DiffSplitResizeHandle,
                                                        >,
                                                              _w,
                                                              cx| {
                                                            let Some(state) = this.diff_split_resize
                                                            else {
                                                                return;
                                                            };
                                                            if state.handle != *e.drag(cx) {
                                                                return;
                                                            }

                                                            let scrollbar_gutter = if this
                                                                .diff_scroll_sync
                                                                .includes_vertical()
                                                            {
                                                                components::Scrollbar::visible_gutter(
                                                                    this.diff_scroll.clone(),
                                                                    components::ScrollbarAxis::Vertical,
                                                                )
                                                            } else {
                                                                px(0.0)
                                                            };
                                                            let main_w = (this
                                                                .main_pane_content_width(cx)
                                                                - scrollbar_gutter)
                                                                .max(px(0.0));
                                                            let available =
                                                                (main_w - handle_w).max(px(0.0));
                                                            let dx =
                                                                e.event.position.x - state.start_x;
                                                            match next_diff_split_drag_ratio(
                                                                available,
                                                                min_col_w,
                                                                state.start_ratio,
                                                                dx,
                                                            ) {
                                                                None => {
                                                                    this.diff_split_ratio = 0.5;
                                                                }
                                                                Some(next_ratio) => {
                                                                    this.diff_split_ratio =
                                                                        next_ratio;
                                                                }
                                                            }
                                                            cx.notify();
                                                        },
                                                    ))
                                                    .on_mouse_up(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _e, _w, cx| {
                                                            this.diff_split_resize = None;
                                                            cx.notify();
                                                        }),
                                                    )
                                                    .on_mouse_up_out(
                                                        MouseButton::Left,
                                                        cx.listener(|this, _e, _w, cx| {
                                                            this.diff_split_resize = None;
                                                            cx.notify();
                                                        }),
                                                    )
                                            };

                                            let columns_header = div()
                                                .id("diff_split_columns_header")
                                                .debug_selector(|| {
                                                    "diff_split_columns_header".to_string()
                                                })
                                                .w_full()
                                                // Same right inset as the body below, so both rows
                                                // divide the identical content box and the column
                                                // divider lines up. Padding keeps the band and its
                                                // bottom border full-bleed.
                                                .pr(shared_scrollbar_gutter)
                                                .h(components::control_height(ui_scale_percent))
                                                .flex()
                                                .items_center()
                                                .text_xs()
                                                .text_color(theme.colors.foreground.secondary)
                                                .bg(crate::theme::content_header_bg(theme))
                                                .border_b_1()
                                                .border_color(theme.colors.stroke.default)
                                                .child(
                                                    div()
                                                        .w(left_w)
                                                        .min_w(px(0.0))
                                                        .px_2()
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .child(left_header),
                                                )
                                                .child(resize_handle(
                                                    "diff_split_resize_handle_header",
                                                ))
                                                .child(
                                                    div()
                                                        .w(right_w)
                                                        .min_w(px(0.0))
                                                        .px_2()
                                                        .overflow_hidden()
                                                        .whitespace_nowrap()
                                                        .child(right_header),
                                                );

                                            div()
                                                .id("diff_split_scroll_container")
                                                .relative()
                                                .h_full()
                                                .min_h(px(0.0))
                                                .flex()
                                                .flex_col()
                                                .bg(theme.colors.surface.canvas)
                                                .font_family(editor_font_family.clone())
                                                .child(columns_header)
                                                .child(
                                                    div()
                                                        .relative()
                                                        .pr(shared_scrollbar_gutter)
                                                        .flex()
                                                        .flex_col()
                                                        .flex_1()
                                                        .min_h(px(0.0))
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_h(px(0.0))
                                                                .flex()
                                                                .child(
                                                                    div()
                                                                        .relative()
                                                                        .w(left_w)
                                                                        .min_w(px(0.0))
                                                                        .h_full()
                                                                        .child(
                                                                            div()
                                                                                .h_full()
                                                                                .min_h(px(0.0))
                                                                                .pr(
                                                                                    if vertical_sync_enabled {
                                                                                        px(0.0)
                                                                                    } else {
                                                                                        left_scrollbar_gutter
                                                                                    },
                                                                                )
                                                                                .child(left),
                                                                        )
                                                                        .when(
                                                                            !vertical_sync_enabled,
                                                                            |d| {
                                                                                d.child(
                                                                                    components::Scrollbar::new(
                                                                                        "diff_split_left_scrollbar",
                                                                                        self.diff_scroll.clone(),
                                                                                    )
                                                                                    .markers(
                                                                                        markers
                                                                                            .clone(),
                                                                                    )
                                                                                    .always_visible()
                                                                                    .render(theme),
                                                                                )
                                                                            },
                                                                        )
                                                                        .when(
                                                                            !self.diff_word_wrap,
                                                                            |d| {
                                                                                d.child(
                                                                                    Self::render_diff_horizontal_scrollbar(
                                                                                        theme,
                                                                                        "diff_split_left_hscrollbar",
                                                                                        self.diff_scroll.clone(),
                                                                                        if vertical_sync_enabled {
                                                                                            px(0.0)
                                                                                        } else {
                                                                                            left_scrollbar_gutter
                                                                                        },
                                                                                        "diff_split_left_hscrollbar",
                                                                                    ),
                                                                                )
                                                                            },
                                                                        ),
                                                                )
                                                                .child(resize_handle(
                                                                    "diff_split_resize_handle_body",
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .relative()
                                                                        .w(right_w)
                                                                        .min_w(px(0.0))
                                                                        .h_full()
                                                                        .child(
                                                                            div()
                                                                                .h_full()
                                                                                .min_h(px(0.0))
                                                                                .pr(
                                                                                    if vertical_sync_enabled {
                                                                                        px(0.0)
                                                                                    } else {
                                                                                        right_scrollbar_gutter
                                                                                    },
                                                                                )
                                                                                .child(right),
                                                                        )
                                                                        .when(
                                                                            !vertical_sync_enabled,
                                                                            |d| {
                                                                                d.child(
                                                                                    components::Scrollbar::new(
                                                                                        "diff_split_right_scrollbar",
                                                                                        self.diff_split_right_scroll.clone(),
                                                                                    )
                                                                                    .markers(
                                                                                        markers
                                                                                            .clone(),
                                                                                    )
                                                                                    .always_visible()
                                                                                    .render(theme),
                                                                                )
                                                                            },
                                                                        )
                                                                        .when(
                                                                            !self.diff_word_wrap,
                                                                            |d| {
                                                                                d.child(
                                                                                    Self::render_diff_horizontal_scrollbar(
                                                                                        theme,
                                                                                        "diff_split_right_hscrollbar",
                                                                                        self.diff_split_right_scroll.clone(),
                                                                                        if vertical_sync_enabled {
                                                                                            px(0.0)
                                                                                        } else {
                                                                                            right_scrollbar_gutter
                                                                                        },
                                                                                        "diff_split_right_hscrollbar",
                                                                                    ),
                                                                                )
                                                                            },
                                                                        ),
                                                                ),
                                                        ),
                                                )
                                                .when(vertical_sync_enabled, |d| {
                                                    d.child(
                                                        components::Scrollbar::new(
                                                            "diff_scrollbar",
                                                            self.diff_scroll.clone(),
                                                        )
                                                        .markers(markers)
                                                        .always_visible()
                                                        .render(theme),
                                                    )
                                                })
                                                .into_any_element()
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        };
        self.diff_text_layout_cache_epoch = self.diff_text_layout_cache_epoch.wrapping_add(1);
        self.prune_diff_text_layout_cache();
        // Last chance to read the geometry the previous frame painted: a search
        // jump can only be measured sideways once its row has been laid out at
        // the position the vertical scroll put it in.
        self.apply_pending_diff_search_horizontal_reveal(window);
        self.diff_text_hitboxes.clear();
        self.conflict_text_hitboxes.clear();
        // The map still holds last frame's buttons, so it is the one place that
        // knows a hovered button has stopped being painted — the row itself
        // clears its hover on the next mouse move, but a row that scrolled away
        // or stopped being a change line paints no handler to do it, and a wheel
        // scroll delivers no mouse move at all.
        self.clear_diff_stage_gutter_hover_if_unpainted(cx);
        self.diff_stage_gutter_cells.clear();
        let diff_editor_menu_active = self
            .active_context_menu_invoker
            .as_ref()
            .is_some_and(|id| id.as_ref() == "diff_editor_menu");
        let diff_search_overlay = self.render_diff_search_overlay(theme, ui_scale_percent, cx);

        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .min_h(px(0.0))
            .bg(crate::theme::content_header_bg(theme))
            .when(diff_editor_menu_active, |d| {
                d.bg(theme.colors.interaction.pressed_background)
            })
            .track_focus(&self.diff_panel_focus_handle)
            .on_action(
                cx.listener(|this, _: &crate::view::TextInputDiffPrevFile, window, cx| {
                    if let Some(repo_id) = this.active_repo_id()
                        && this
                            .try_select_adjacent_diff_file_preserving_focus(repo_id, -1, window, cx)
                    {
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::view::TextInputDiffNextFile, window, cx| {
                    if let Some(repo_id) = this.active_repo_id()
                        && this
                            .try_select_adjacent_diff_file_preserving_focus(repo_id, 1, window, cx)
                    {
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_action(cx.listener(
                |this, _: &crate::view::TextInputDiffPrevSearchMatchOrChange, _window, cx| {
                    if this.navigate_prev_search_match_or_diff_change(cx) {
                        cx.notify();
                    }
                    cx.stop_propagation();
                },
            ))
            .on_action(cx.listener(
                |this, _: &crate::view::TextInputDiffNextSearchMatchOrChange, _window, cx| {
                    if this.navigate_next_search_match_or_diff_change(cx) {
                        cx.notify();
                    }
                    cx.stop_propagation();
                },
            ))
            .on_action(cx.listener(
                |this, _: &crate::view::TextInputDiffPrevChange, _window, cx| {
                    if this.navigate_prev_diff_change(cx) {
                        cx.notify();
                    }
                    cx.stop_propagation();
                },
            ))
            .on_action(cx.listener(
                |this, _: &crate::view::TextInputDiffNextChange, _window, cx| {
                    if this.navigate_next_diff_change(cx) {
                        cx.notify();
                    }
                    cx.stop_propagation();
                },
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, window, cx| {
                    window.focus(&this.diff_panel_focus_handle, cx);
                }),
            )
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if this.handle_diff_shortcut(&e.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                header
                    .h(components::control_height_md(ui_scale_percent))
                    .px_2()
                    .bg(if historical_browse {
                        crate::theme::historical_header_bg(
                            theme,
                            crate::theme::content_header_bg(theme),
                        )
                    } else {
                        crate::theme::content_header_bg(theme)
                    })
                    .border_b_1()
                    .border_color(theme.colors.stroke.default),
            )
            .child(
                div()
                    .id("diff_body_container")
                    .debug_selector(|| "diff_body_container".to_string())
                    .flex_1()
                    .min_h(px(0.0))
                    .w_full()
                    .h_full()
                    .child(body),
            )
            .when_some(diff_search_overlay, |d, overlay| d.child(overlay))
            .child(DiffTextSelectionTracker { view: cx.entity() })
    }
}

impl MainPaneView {
    /// Path-level reject for an agent compare: restore one file from the
    /// session baseline inside the agent worktree (never HEAD — the
    /// baseline is the point that preserves the pre-session state).
    fn restore_agent_file_from_baseline(
        &mut self,
        repo_id: RepoId,
        worktree: std::path::PathBuf,
        baseline: CommitId,
        path: std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        let baseline_arg = baseline.0.as_ref().to_string();
        let path_arg = path.display().to_string();
        let path_display = path_arg.clone();
        cx.spawn(async move |this, cx| {
            let result = smol::unblock(move || {
                crate::view::panels::git_output(
                    &worktree,
                    &["checkout", baseline_arg.as_str(), "--", path_arg.as_str()],
                )
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.store.dispatch(Msg::ReloadRepo { repo_id });
                    }
                    Err(error) => {
                        let _ = this.root_view.update(cx, |root, cx| {
                            root.push_toast(
                                components::ToastKind::Error,
                                crate::i18n::t!(
                                    "chrome.agent.restore_failed",
                                    path = path_display.as_str(),
                                    err = error
                                )
                                .to_string(),
                                cx,
                            );
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
