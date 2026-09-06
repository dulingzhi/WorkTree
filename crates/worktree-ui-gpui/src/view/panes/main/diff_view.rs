//! Facade for the diff pane view: the pane chrome, the toolbar buttons,
//! the file-editor entry points and the `diff_view` spine — intent
//! computation plus dispatch. The keyboard dispatch lives in `shortcuts`,
//! the submodule summary in `submodule_summary`, the search overlay in
//! `search_overlay`, the toolbar cluster in `toolbar` and the body
//! renderers in `body`.
use super::*;
use crate::view::panes::PaneChromeExt as _;
use crate::view::panes::main::DiffHorizontalScrollColumn;
use crate::view::panes::main::diff_search::DiffSearchOptions;
use gpui::Focusable;

mod body;
mod search_overlay;
mod shortcuts;
mod submodule_summary;
mod toolbar;

/// The predicates a single `diff_view` pass dispatches on, computed once
/// up front (including the load/reset side effects of the preview state
/// machine) so the toolbar and body renderers only read them.
struct DiffViewIntents {
    has_submodule_summary: bool,
    inline_submodule_diff_active: bool,
    untracked_directory_notice: Option<SharedString>,
    is_file_preview: bool,
    supports_diff_content_toggle: bool,
    historical_browse: bool,
    content_bg: gpui::Rgba,
    is_file_editor: bool,
    wants_file_diff: bool,
    wants_collapsed_diff: bool,
    conflict_target_path: Option<std::path::PathBuf>,
    is_conflict_resolver: bool,
    is_conflict_compare: bool,
    conflict_rendered_preview_active: bool,
    rendered_view_toggle_kind: Option<RenderedPreviewKind>,
    is_markdown_preview_view: bool,
    is_image_diff_view: bool,
    is_simple_conflict_strategy: bool,
}

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

    /// Compute the dispatch intents for one `diff_view` pass. The preview
    /// state-machine side effects (editor/preview loading and the stale
    /// preview reset) run here, in their original order.
    fn diff_view_intents(
        &mut self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> DiffViewIntents {
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

        let is_simple_conflict_strategy = matches!(
            self.conflict_resolver.strategy,
            Some(
                worktree_core::conflict_session::ConflictResolverStrategy::BinarySidePick
                    | worktree_core::conflict_session::ConflictResolverStrategy::TwoWayKeepDelete
                    | worktree_core::conflict_session::ConflictResolverStrategy::DecisionOnly
            )
        );

        DiffViewIntents {
            has_submodule_summary,
            inline_submodule_diff_active,
            untracked_directory_notice,
            is_file_preview,
            supports_diff_content_toggle,
            historical_browse,
            content_bg,
            is_file_editor,
            wants_file_diff,
            wants_collapsed_diff,
            conflict_target_path,
            is_conflict_resolver,
            is_conflict_compare,
            conflict_rendered_preview_active,
            rendered_view_toggle_kind,
            is_markdown_preview_view,
            is_image_diff_view,
            is_simple_conflict_strategy,
        }
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

        let intents = self.diff_view_intents(theme, cx);

        let controls = self.diff_controls(&intents, repo_id, theme, ui_scale_percent, cx);

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

        let body: AnyElement = self.diff_body(
            &intents,
            repo_id,
            theme,
            ui_scale_percent,
            editor_font_family,
            window,
            cx,
        );

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
                    .bg(if intents.historical_browse {
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
