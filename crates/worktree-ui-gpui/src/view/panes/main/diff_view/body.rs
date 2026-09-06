//! Diff pane body dispatch: picks the renderer for the active content
//! kind from the computed `DiffViewIntents`. The heavy per-kind bodies
//! (markdown preview, file preview list, conflict compare, patch diff)
//! are the private helpers below it.
use super::*;

impl MainPaneView {
    pub(super) fn diff_body(
        &mut self,
        intents: &DiffViewIntents,
        repo_id: Option<RepoId>,
        theme: AppTheme,
        ui_scale_percent: u32,
        editor_font_family: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        if intents.has_submodule_summary && !intents.inline_submodule_diff_active {
            self.render_submodule_summary(theme, cx)
        } else if let Some(message) = intents.untracked_directory_notice.clone() {
            components::empty_state(theme, crate::i18n::tr("diff.pane.directory"), message)
                .into_any_element()
        } else if intents.is_file_editor {
            self.render_file_editor(theme, cx)
        } else if intents.is_file_preview {
            if intents.is_markdown_preview_view {
                self.diff_body_worktree_markdown_preview(
                    theme,
                    intents.content_bg,
                    ui_scale_percent,
                    editor_font_family,
                    cx,
                )
            } else {
                self.diff_body_worktree_file_preview(
                    theme,
                    intents.content_bg,
                    ui_scale_percent,
                    editor_font_family,
                    window,
                    cx,
                )
            }
        } else if intents.is_conflict_resolver {
            self.render_conflict_resolver_pane(
                intents.conflict_target_path.clone(),
                repo_id,
                theme,
                ui_scale_percent,
                editor_font_family.clone(),
                cx,
            )
        } else if intents.is_conflict_compare {
            self.diff_body_conflict_compare(
                theme,
                ui_scale_percent,
                editor_font_family,
                intents.conflict_target_path.clone(),
                cx,
            )
        } else if intents.wants_file_diff || intents.wants_collapsed_diff {
            self.render_selected_file_diff(theme, window, cx)
        } else {
            self.diff_body_patch_diff(
                intents,
                theme,
                ui_scale_percent,
                editor_font_family,
                window,
                cx,
            )
        }
    }

    /// The worktree markdown preview body (a single document in a flowing
    /// layout): load states first, then the rendered document in its
    /// scroll container.
    fn diff_body_worktree_markdown_preview(
        &mut self,
        theme: AppTheme,
        content_bg: gpui::Rgba,
        ui_scale_percent: u32,
        editor_font_family: String,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        match &self.worktree_preview {
            Loadable::NotLoaded | Loadable::Loading => components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.preview"),
                crate::i18n::tr("diff.common.loading"),
            )
            .into_any_element(),
            Loadable::Error(e) => {
                components::empty_state(theme, crate::i18n::tr("diff.pane.preview"), e.clone())
                    .into_any_element()
            }
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
                                    change_bar_color: rows::worktree_markdown_preview_bar_color(
                                        self, theme,
                                    ),
                                    query: self.markdown_preview_search_query(),
                                    reveal: self.markdown_preview_reveal.clone(),
                                    scroll: Some(
                                        self.worktree_preview_scroll.0.borrow().base_handle.clone(),
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
    }

    /// The worktree file preview body: the read-only line list with its
    /// scrollbars and annotation gutter.
    fn diff_body_worktree_file_preview(
        &mut self,
        theme: AppTheme,
        content_bg: gpui::Rgba,
        ui_scale_percent: u32,
        editor_font_family: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
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

                    let scroll_handle = self.worktree_preview_scroll.0.borrow().base_handle.clone();
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
                        .when_some(annotate_handle, |container, handle| container.child(handle))
                        .into_any_element()
                }
            }
        }
    }

    /// The two-way conflict compare body (ours/theirs columns over the
    /// shared diff scroll) for conflicts without a resolver strategy.
    fn diff_body_conflict_compare(
        &mut self,
        theme: AppTheme,
        ui_scale_percent: u32,
        editor_font_family: String,
        conflict_target_path: Option<std::path::PathBuf>,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        match (self.active_repo(), conflict_target_path) {
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
    }

    /// The patch diff body: the inline or split row lists with their
    /// scrollbars, markers and the split-column resize handles.
    fn diff_body_patch_diff(
        &mut self,
        intents: &DiffViewIntents,
        theme: AppTheme,
        ui_scale_percent: u32,
        editor_font_family: String,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        match self.active_repo() {
            None => components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.diff"),
                crate::i18n::tr("diff.common.no_repository"),
            )
            .into_any_element(),
            Some(_repo) => {
                match self.rendered_patch_diff_loadable() {
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
                        if intents.wants_file_diff || intents.wants_collapsed_diff {
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
                }
            }
        }
    }
}
