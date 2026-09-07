use super::*;
use crate::i18n::{t, tr, tr_str};
use crate::view::commit_message_text::commit_message_summary_highlights;
use crate::view::panels::layout::{
    COMPARISON_CARDS_MAX_BODY_FRACTION, MULTI_COMMIT_ROW_HEIGHT_PX,
    RANGE_FILES_SECTION_MIN_HEIGHT_PX, STATUS_SECTION_MIN_HEIGHT_PX, clamp_vertical_split_height,
    commit_allowed, commit_details_author_row, commit_details_monospace_element,
    commit_details_monospace_value, commit_details_selectable_row, commit_message_link_highlights,
    commit_sha_field_highlights, commit_sha_field_links, label_width_chars, merge_active,
    min_change_tracking_stack_height, resolved_vertical_split_height, status_action_all_label,
    status_action_count_label, status_action_file_count, status_action_labels_for_width,
    visible_bounds_probe,
};
use crate::view::panels::{
    COMMIT_DETAILS_MESSAGE_MAX_HEIGHT_PX, COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX,
};
use gpui::{Div, Stateful};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct StatusSectionActionSelection {
    pub(in crate::view) paths: Vec<std::path::PathBuf>,
    pub(in crate::view) from_explicit_selection: bool,
}

impl StatusSectionActionSelection {
    fn count(&self) -> usize {
        self.paths.len()
    }

    pub(in crate::view) fn popover_path(&self) -> Option<std::path::PathBuf> {
        (!self.from_explicit_selection && self.paths.len() == 1).then(|| self.paths[0].clone())
    }
}

fn explicit_status_section_action_paths(
    selection: &StatusMultiSelection,
    section: StatusSection,
) -> Vec<std::path::PathBuf> {
    match section {
        StatusSection::CombinedUnstaged => selection
            .selected_paths_for_area(DiffArea::Unstaged)
            .to_vec(),
        StatusSection::Untracked => selection.untracked.clone(),
        StatusSection::Unstaged => selection.unstaged.clone(),
        StatusSection::Staged => selection.staged.clone(),
    }
}

fn active_status_section_action_path(
    repo: &RepoState,
    diff_target: Option<&DiffTarget>,
    section: StatusSection,
) -> Option<std::path::PathBuf> {
    let DiffTarget::WorkingTree { path, area } = diff_target? else {
        return None;
    };
    if *area != section.diff_area() {
        return None;
    }

    StatusSectionEntries::from_repo(repo, section)
        .is_some_and(|entries| entries.contains_path(path.as_path()))
        .then(|| path.clone())
}

pub(in crate::view) fn status_section_action_selection(
    repo: &RepoState,
    diff_target: Option<&DiffTarget>,
    selection: Option<&StatusMultiSelection>,
    section: StatusSection,
) -> StatusSectionActionSelection {
    if let Some(selection) = selection {
        let paths = explicit_status_section_action_paths(selection, section);
        if !paths.is_empty() {
            return StatusSectionActionSelection {
                paths,
                from_explicit_selection: true,
            };
        }
    }

    active_status_section_action_path(repo, diff_target, section)
        .map(|path| StatusSectionActionSelection {
            paths: vec![path],
            from_explicit_selection: false,
        })
        .unwrap_or_default()
}

impl DetailsPaneView {
    fn status_section_action_selection(
        &self,
        repo_id: RepoId,
        section: StatusSection,
    ) -> StatusSectionActionSelection {
        let Some(repo) = self.active_repo().filter(|repo| repo.id == repo_id) else {
            return StatusSectionActionSelection::default();
        };

        status_section_action_selection(
            repo,
            repo.diff_state.diff_target.as_ref(),
            self.status_multi_selection.get(&repo_id),
            section,
        )
    }

    fn take_status_section_action_selection(
        &mut self,
        repo_id: RepoId,
        section: StatusSection,
    ) -> StatusSectionActionSelection {
        let selection = self.status_section_action_selection(repo_id, section);
        if selection.from_explicit_selection {
            self.status_multi_selection.remove(&repo_id);
        }
        selection
    }

    fn measured_status_sections_total_height(&self, resize_handle_h: Pixels) -> Option<Pixels> {
        self.current_status_sections_bounds()
            .map(|bounds| (bounds.size.height - resize_handle_h).max(px(0.0)))
    }

    fn resolved_measured_change_tracking_section_height(
        &self,
        resize_handle_h: Pixels,
    ) -> Option<Pixels> {
        let section_min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let min_height = min_change_tracking_stack_height(
            self.change_tracking_view == ChangeTrackingView::SplitUntracked,
            resize_handle_h,
        );

        self.measured_status_sections_total_height(resize_handle_h)
            .map(|total_height| {
                resolved_vertical_split_height(
                    self.change_tracking_height,
                    total_height,
                    min_height,
                    section_min_h,
                )
            })
    }

    fn resolved_measured_change_tracking_stack_total_height(
        &self,
        resize_handle_h: Pixels,
    ) -> Option<Pixels> {
        self.resolved_measured_change_tracking_section_height(resize_handle_h)
            .map(|section_height| (section_height - resize_handle_h).max(px(0.0)))
            .or_else(|| {
                self.current_change_tracking_stack_bounds()
                    .map(|bounds| (bounds.size.height - resize_handle_h).max(px(0.0)))
            })
    }

    pub(in crate::view) fn sanitized_restored_change_tracking_height_design(
        view: ChangeTrackingView,
        height: Option<u32>,
    ) -> Option<f32> {
        let min_height: f32 = min_change_tracking_stack_height(
            view == ChangeTrackingView::SplitUntracked,
            px(PANE_RESIZE_HANDLE_PX),
        )
        .into();
        height.map(|value| (value as f32).max(min_height))
    }

    #[cfg(test)]
    pub(in crate::view) fn sanitized_restored_change_tracking_height(
        view: ChangeTrackingView,
        height: Option<u32>,
    ) -> Option<Pixels> {
        Self::sanitized_restored_change_tracking_height_design(view, height).map(px)
    }

    pub(in crate::view) fn sanitized_restored_untracked_height_design(
        height: Option<u32>,
    ) -> Option<f32> {
        height.map(|value| (value as f32).max(STATUS_SECTION_MIN_HEIGHT_PX))
    }

    #[cfg(test)]
    pub(in crate::view) fn sanitized_restored_untracked_height(
        height: Option<u32>,
    ) -> Option<Pixels> {
        Self::sanitized_restored_untracked_height_design(height).map(px)
    }

    fn status_resize_total_height(
        &self,
        handle: StatusSectionResizeHandle,
        resize_handle_h: Pixels,
    ) -> Option<Pixels> {
        match handle {
            StatusSectionResizeHandle::ChangeTrackingAndStaged => {
                self.measured_status_sections_total_height(resize_handle_h)
            }
            StatusSectionResizeHandle::UntrackedAndUnstaged => {
                self.resolved_measured_change_tracking_stack_total_height(resize_handle_h)
            }
        }
    }

    fn start_status_section_resize(
        &mut self,
        handle: StatusSectionResizeHandle,
        start_y: Pixels,
        cx: &mut gpui::Context<Self>,
    ) {
        let section_min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let resize_handle_h = px(PANE_RESIZE_HANDLE_PX);
        let total_height = self.status_resize_total_height(handle, resize_handle_h);
        let start_height = match handle {
            StatusSectionResizeHandle::ChangeTrackingAndStaged => total_height
                .map(|total_height| {
                    resolved_vertical_split_height(
                        self.change_tracking_height,
                        total_height,
                        min_change_tracking_stack_height(
                            self.change_tracking_view == ChangeTrackingView::SplitUntracked,
                            resize_handle_h,
                        ),
                        section_min_h,
                    )
                })
                .or(self.change_tracking_height)
                .unwrap_or(section_min_h),
            StatusSectionResizeHandle::UntrackedAndUnstaged => total_height
                .map(|total_height| {
                    resolved_vertical_split_height(
                        self.untracked_height,
                        total_height,
                        section_min_h,
                        section_min_h,
                    )
                })
                .or(self.untracked_height)
                .unwrap_or(section_min_h),
        };

        self.status_section_resize = Some(StatusSectionResizeState {
            handle,
            start_y,
            start_height,
        });
        cx.notify();
    }

    pub(in crate::view) fn update_status_section_resize(
        &mut self,
        current_y: Pixels,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(state) = self.status_section_resize else {
            return false;
        };

        let section_min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let resize_handle_h = px(PANE_RESIZE_HANDLE_PX);
        let total_height = self.status_resize_total_height(state.handle, resize_handle_h);

        let delta_y = current_y - state.start_y;
        let mut changed = false;
        match state.handle {
            StatusSectionResizeHandle::ChangeTrackingAndStaged => {
                let min_top = min_change_tracking_stack_height(
                    self.change_tracking_view == ChangeTrackingView::SplitUntracked,
                    resize_handle_h,
                );
                let next_height = if let Some(total_height) = total_height {
                    clamp_vertical_split_height(
                        state.start_height + delta_y,
                        total_height,
                        min_top,
                        section_min_h,
                    )
                } else {
                    (state.start_height + delta_y).max(min_top)
                };
                if self.change_tracking_height != Some(next_height) {
                    self.set_change_tracking_height_from_pixels(Some(next_height));
                    changed = true;
                }
            }
            StatusSectionResizeHandle::UntrackedAndUnstaged => {
                let next_height = if let Some(total_height) = total_height {
                    clamp_vertical_split_height(
                        state.start_height + delta_y,
                        total_height,
                        section_min_h,
                        section_min_h,
                    )
                } else {
                    (state.start_height + delta_y).max(section_min_h)
                };
                if self.untracked_height != Some(next_height) {
                    self.set_untracked_height_from_pixels(Some(next_height));
                    changed = true;
                }
            }
        }

        if changed {
            cx.notify();
        }
        changed
    }

    pub(in crate::view) fn finish_status_section_resize(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if self.status_section_resize.take().is_some() {
            let pane = cx.entity();
            self.schedule_ui_settings_persist(cx);
            cx.notify();
            cx.defer(move |cx| {
                pane.update(cx, |_this, cx| {
                    cx.notify();
                });
            });
            true
        } else {
            false
        }
    }

    fn repo_has_head_commit(repo: &RepoState) -> bool {
        if repo.detached_head_commit.is_some() {
            return true;
        }

        match &repo.head_branch {
            Loadable::Ready(head) if head == "HEAD" => true,
            Loadable::Ready(head) => match &repo.branches {
                Loadable::Ready(branches) => branches.iter().any(|branch| branch.name == *head),
                _ => true,
            },
            _ => true,
        }
    }

    pub(in crate::view) fn can_submit_commit(
        repo: Option<&RepoState>,
        message: &str,
        amend: bool,
    ) -> bool {
        let Some(repo) = repo else {
            return false;
        };
        if repo.commit_in_flight > 0 {
            return false;
        }
        if message.trim().is_empty() {
            return false;
        }
        if amend {
            return !merge_active(Some(repo))
                && !matches!(repo.rebase_in_progress, Loadable::Ready(true))
                && Self::repo_has_head_commit(repo);
        }
        let staged_count = repo
            .staged_status_entries()
            .map_or(0, |entries| entries.len());
        let is_merge_active = merge_active(Some(repo));
        commit_allowed(is_merge_active, staged_count)
    }

    fn submit_commit(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        let Some(repo_id) = self.active_repo_id() else {
            return false;
        };
        let message = self
            .commit_message_input
            .read_with(cx, |input, _| input.text().to_string());
        let amend = self.commit_amend_enabled;
        if !Self::can_submit_commit(self.active_repo(), &message, amend) {
            return false;
        }

        if amend {
            self.mark_pending_commit_amend(repo_id);
            self.store.dispatch(Msg::CommitAmend {
                repo_id,
                message: message.trim().to_string(),
                push_after_commit: self.commit_push_after_enabled,
            });
        } else {
            self.store.dispatch(Msg::Commit {
                repo_id,
                message: message.trim().to_string(),
                push_after_commit: self.commit_push_after_enabled,
            });
        }
        self.commit_message_programmatic_change = true;
        self.commit_message_input
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.commit_message_scroll
            .set_offset(point(px(0.0), px(0.0)));
        cx.notify();
        true
    }

    pub(in crate::view) fn handle_commit_submit_shortcut(
        &mut self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self
            .commit_message_input
            .read(cx)
            .focus_handle()
            .is_focused(window)
        {
            return false;
        }

        let _ = self.submit_commit(cx);
        true
    }

    fn sync_commit_details_input_value(
        input: &Entity<components::TextInput>,
        value: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        if input.read(cx).text() != value {
            input.update(cx, |input, cx| {
                input.set_text(value.to_string(), cx);
            });
        }
    }

    fn sync_commit_details_message_input(
        &mut self,
        message: &str,
        theme: AppTheme,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        let (mut highlights, links) = commit_message_link_highlights(message, theme);
        let mut merged = commit_message_summary_highlights(message, theme, &highlights);
        merged.append(&mut highlights);
        merged.sort_by_key(|(range, _)| range.start);
        self.commit_details_message_input.update(cx, |input, cx| {
            if input.text() != message {
                input.set_text(message.to_string(), cx);
            }
            input.set_highlights(merged, cx);
        });
        self.commit_details_message_link_menu
            .update(cx, |menu, cx| {
                menu.sync(
                    self.commit_details_message_input.clone(),
                    repo_id,
                    links,
                    "commit_details_message_link_menu",
                    cx,
                );
            });
    }

    fn sync_commit_details_parent_input(
        &mut self,
        parent: &str,
        repo_id: RepoId,
        interactive: bool,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) {
        Self::sync_commit_details_input_value(&self.commit_details_parent_input, parent, cx);
        self.commit_details_parent_input.update(cx, |input, cx| {
            input.set_highlights(commit_sha_field_highlights(parent, theme), cx);
        });
        let parent_links = commit_sha_field_links(parent, interactive, true);
        self.commit_details_parent_link_menu.update(cx, |menu, cx| {
            menu.sync(
                self.commit_details_parent_input.clone(),
                repo_id,
                parent_links,
                "commit_details_parent_link_menu",
                cx,
            );
        });
    }

    fn sync_commit_details_sha_menu(
        &mut self,
        sha: &str,
        repo_id: RepoId,
        interactive: bool,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) {
        Self::sync_commit_details_input_value(&self.commit_details_sha_input, sha, cx);
        self.commit_details_sha_input.update(cx, |input, cx| {
            input.set_highlights(commit_sha_field_highlights(sha, theme), cx);
        });
        // A commit's own SHA has nowhere to navigate to.
        let sha_links = commit_sha_field_links(sha, interactive, false);
        self.commit_details_sha_link_menu.update(cx, |menu, cx| {
            menu.sync(
                self.commit_details_sha_input.clone(),
                repo_id,
                sha_links,
                "commit_details_sha_link_menu",
                cx,
            );
        });
    }

    fn sync_retained_commit_details_message_input(
        &mut self,
        message: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        let theme = self.theme;
        self.commit_details_message_input.update(cx, |input, cx| {
            if input.text() != message {
                input.set_text(message.to_string(), cx);
            }
            input.set_highlights(commit_message_summary_highlights(message, theme, &[]), cx);
        });
        self.commit_details_message_link_menu
            .update(cx, |menu, cx| {
                menu.sync(
                    self.commit_details_message_input.clone(),
                    RepoId(0),
                    Arc::<[components::MessageLink]>::from([]),
                    "commit_details_message_link_menu",
                    cx,
                );
            });
    }

    /// Selected commits resolved against the loaded log page, in log order
    /// (youngest first). Ids missing from the page are skipped.
    fn multi_selected_commits_in_log_order(repo: &RepoState) -> Vec<Commit> {
        let selection = &repo.history_state.multi_selection;
        let Loadable::Ready(page) = &repo.log else {
            return Vec::new();
        };
        // Hash the selection first: this runs per frame (twice, and once more
        // per visible row batch) over the whole loaded page, and
        // `CommitMultiSelection::contains` is a linear scan — so a large
        // selection against a large page would be quadratic on every repaint.
        let selected: FxHashSet<&CommitId> = selection.commits.iter().collect();
        page.commits
            .iter()
            .filter(|commit| selected.contains(&commit.id))
            .cloned()
            .collect()
    }

    /// Commits to preview as cards while a two-point comparison is active.
    /// Prefers a multi-selection *only* when it is genuinely multi, because that
    /// is the one case where the selection is what is being compared (its merged
    /// diff). A single leftover selection — every plain history click leaves one
    /// — describes an unrelated commit, so the mark + compare, branch/tag and
    /// working-tree flows derive their endpoints from the range itself, looking
    /// each SHA up in the loaded log so its summary/author/time can be shown.
    /// Ordered newest first (tip before base) to match the log. The working tree
    /// has no commit of its own, so a compare-against-working-tree range yields
    /// a single card.
    fn range_comparison_commits(repo: &RepoState) -> Vec<Commit> {
        if repo.history_state.multi_selection.is_multi() {
            let multi = Self::multi_selected_commits_in_log_order(repo);
            if !multi.is_empty() {
                return multi;
            }
        }
        let Some(range) = repo.history_state.range_selection.as_ref() else {
            return Vec::new();
        };
        let Loadable::Ready(page) = &repo.log else {
            return Vec::new();
        };
        let find = |id: &CommitId| page.commits.iter().find(|c| &c.id == id).cloned();
        let mut commits = Vec::new();
        if let Some(to) = range.to.as_ref().and_then(&find) {
            commits.push(to);
        }
        if let Some(from) = find(&range.from) {
            commits.push(from);
        }
        commits
    }

    /// One selected/compared-commit preview card: avatar, summary, an author +
    /// relative-time line, and the short SHA. Shared by the multi-selection and
    /// range-comparison lists so both read identically.
    fn commit_card_element(
        &self,
        ix: usize,
        commit: &Commit,
        // Email behind `commit.author`, resolved from the repo's author→email
        // map by the caller; drives the card's remote avatar when a source
        // other than initials is active.
        author_email: Option<&str>,
        now: std::time::SystemTime,
        show_border: bool,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale = self.ui_scale();
        let scaled_px =
            |value: f32| crate::ui_scale::design_px_from_percent(value, self.ui_scale_percent);

        let short_sha: SharedString = commit
            .id
            .as_ref()
            .get(0..8)
            .unwrap_or(commit.id.as_ref())
            .to_string()
            .into();
        let summary: SharedString = commit.summary.to_string().into();
        let author = commit.author.to_string();
        let unix_secs = commit
            .time
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let when: SharedString = format!(
            "{} · {}",
            author,
            crate::view::date_time::format_relative_time(unix_secs, now)
        )
        .into();

        div()
            .id(("commit_multi_row", ix))
            .debug_selector(move || format!("commit_multi_row_{ix}"))
            .h(scaled_px(MULTI_COMMIT_ROW_HEIGHT_PX))
            .w_full()
            .flex()
            .items_center()
            .gap(scaled_px(8.0))
            .px(scaled_px(8.0))
            // The last card sits directly above the files section's own top
            // separator, so it omits its bottom border to avoid a double line.
            .when(show_border, |row| {
                row.border_b_1().border_color(theme.colors.stroke.default)
            })
            .child(components::author_avatar_image(
                theme,
                ui_scale,
                &author,
                author_email,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(scaled_px(2.0))
                    .child(div().text_sm().line_clamp(1).child(summary))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .child(when),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .font_family(crate::view::UI_MONOSPACE_FONT_FAMILY)
                    .text_color(theme.colors.foreground.secondary)
                    .child(short_sha),
            )
            .into_any_element()
    }

    /// Rows for both the comparison view's endpoint cards and the plain
    /// multi-selection list. `range_comparison_commits` already resolves to the
    /// multi-selection when that is what is being compared, so one renderer
    /// serves both and the two views cannot drift apart.
    pub(in crate::view) fn render_multi_commit_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(repo) = this.active_repo() else {
            return Vec::new();
        };
        let commits = Self::range_comparison_commits(repo);
        let last_ix = commits.len().saturating_sub(1);
        let now = std::time::SystemTime::now();
        range
            .filter_map(|ix| commits.get(ix).map(|commit| (ix, commit.clone())))
            .map(|(ix, commit)| {
                let author_email = repo
                    .author_emails
                    .get(commit.author.as_ref())
                    .map(|email| email.as_str());
                // Attach the load-watcher so these cards repaint when the
                // remote avatar lands (gpui notifies only the first view
                // that requested a given image URL).
                if let Some(url) = crate::avatar_source::avatar_url(author_email) {
                    crate::avatar_source::ensure_avatar_loaded(&url, cx);
                }
                this.commit_card_element(ix, &commit, author_email, now, ix != last_ix)
            })
            .collect()
    }

    /// The details pane's standard vertical-scroll frame: the list fills the
    /// container, a gutter reserves room for the scrollbar so rows never sit
    /// underneath it, and the scrollbar overlays the right edge. Every scrolling
    /// list in this pane is built from this, so they all scroll alike.
    fn vertical_scroll_frame(
        theme: AppTheme,
        container_id: impl Into<ElementId>,
        scrollbar_id: impl Into<ElementId>,
        scroll: &UniformListScrollHandle,
        list: gpui::UniformList,
    ) -> Stateful<Div> {
        let list = restrict_scroll_to_vertical_axis(
            list.w_full().h_full().min_h(px(0.0)).track_scroll(scroll),
        );
        let scrollbar_gutter = components::Scrollbar::visible_gutter(
            scroll.clone(),
            components::ScrollbarAxis::Vertical,
        );
        div()
            .id(container_id)
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .w_full()
            .overflow_hidden()
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .pr(scrollbar_gutter)
                    .child(list),
            )
            .child(components::Scrollbar::new(scrollbar_id, scroll.clone()).render(theme))
    }

    /// The scrolling column of commit preview cards, shared by the plain
    /// multi-selection view (where it fills the pane) and the comparison view
    /// (where it is capped and sits above the changed-file list).
    fn commit_cards_list(
        &mut self,
        repo_id: RepoId,
        count: usize,
        cx: &mut gpui::Context<Self>,
    ) -> Stateful<Div> {
        Self::vertical_scroll_frame(
            self.theme,
            ("commit_multi_container", repo_id.0),
            ("commit_multi_scrollbar", repo_id.0),
            &self.commit_multi_scroll,
            uniform_list(
                ("commit_multi_list", repo_id.0),
                count,
                cx.processor(Self::render_multi_commit_rows),
            ),
        )
    }

    fn multi_commit_details_view(
        &mut self,
        repo_id: RepoId,
        count: usize,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale = self.ui_scale();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(components::control_height_md(ui_scale))
            .px_2()
            .bg(theme.colors.surface.raised)
            .border_b_1()
            .border_color(theme.colors.stroke.default)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .line_clamp(1)
                    .child(SharedString::from(t!(
                        "layout.multi.commits_selected",
                        count = count
                    ))),
            )
            .child(
                components::Button::new("commit_details_close", "")
                    .start_slot(svg_icon(
                        "icons/generic_close.svg",
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        if let Some(repo_id) = this.active_repo_id() {
                            this.store.dispatch(Msg::ClearCommitSelection { repo_id });
                        }
                        cx.notify();
                    })
                    .worktree_tooltip(theme, tr("layout.commit_details.close")),
            );

        let body = self.commit_cards_list(repo_id, count, cx);

        div()
            .id("commit_details_container")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .child(header)
            .child(
                div()
                    .id("commit_details_body_container")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .p_2()
                    .child(body),
            )
            .into_any_element()
    }

    /// The changed files of a linked worktree that is *not* this tab, shown when
    /// its history row is selected.
    ///
    /// The worktree chip is the header rather than decoration: everything below
    /// belongs to another checkout, and nothing else on screen says so.
    fn worktree_uncommitted_view(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale = self.ui_scale();

        // Only the counts and the chip's three fields are needed here. Cloning the
        // summary would copy both `FileStatus` vectors -- every changed *and*
        // untracked file of the worktree -- on every repaint of this pane.
        let Some((file_count, loaded_file_count, chip_label, worktree_path)) =
            self.selected_worktree_summary().map(|summary| {
                (
                    // Counts, not `staged.len() + unstaged.len()`: the file lists
                    // arrive with the scan the selection asked for, and the header
                    // has to be right before then. Each changed file lands in
                    // exactly one bucket, so the two agree once loaded.
                    summary.added + summary.modified + summary.deleted,
                    summary.staged.len() + summary.unstaged.len(),
                    crate::view::rows::sidebar::worktree_origin_label(
                        summary.branch.as_deref(),
                        summary.detached,
                        &summary.path,
                    ),
                    summary.path.clone(),
                )
            })
        else {
            return div().into_any_element();
        };

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .h(components::control_height_md(ui_scale))
            .px_2()
            .bg(theme.colors.surface.raised)
            .border_b_1()
            .border_color(theme.colors.stroke.default)
            .child(
                div()
                    .flex_none()
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    // Not "Uncommitted changes": that is the current repo's
                    // own row, and these are somebody else's.
                    .child(tr("layout.worktree.title")),
            )
            .child(div().flex_1().min_w(px(0.0)))
            .child({
                let open_path = worktree_path.clone();
                let palette = crate::view::rows::sidebar::worktree_badge_palette(theme);
                crate::view::rows::sidebar::worktree_origin_chip(
                    theme,
                    chip_label,
                    ui_scale.px(10.0),
                    ui_scale.px(18.0),
                    ui_scale.px(220.0),
                    ui_scale.px(6.0),
                )
                .id("worktree_uncommitted_origin")
                .debug_selector(|| "worktree_uncommitted_open".to_string())
                .cursor(CursorStyle::PointingHand)
                .hover(move |s| {
                    s.border_color(palette.hover_border)
                        .text_color(palette.hover_text)
                })
                .worktree_tooltip(
                    theme,
                    t!(
                        "layout.worktree.open_in_tab",
                        path = worktree_path.display().to_string()
                    )
                    .into(),
                )
                // A chip is a control of its own: a right or middle click must not
                // open a repo tab, and a left click must not reach the row behind it.
                .on_click(cx.listener(move |this, e: &ClickEvent, _w, cx| {
                    if !e.standard_click() {
                        return;
                    }
                    cx.stop_propagation();
                    this.store.dispatch(Msg::OpenRepo(open_path.clone()));
                    cx.notify();
                }))
            })
            .child(
                components::Button::new("worktree_uncommitted_close", "")
                    .start_slot(svg_icon(
                        "icons/generic_close.svg",
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        this.store.dispatch(Msg::ClearCommitSelection { repo_id });
                        cx.notify();
                    })
                    .worktree_tooltip(theme, tr("layout.worktree.close")),
            );

        // No "no files" state: the scan only reports a worktree once
        // `WorktreeDirtySummary::is_dirty` holds, so `file_count` -- the sum of
        // those same three counts -- is always positive here. A change that
        // started reporting clean worktrees would need a branch of its own; it
        // would otherwise sit on "Loading files…" forever.
        let files_body: AnyElement = if loaded_file_count == 0 {
            // Counts without files means the scan carrying them is still running.
            // Saying so beats an empty list that reads as "nothing changed" while
            // the header above it counts the changes.
            div()
                .debug_selector(|| "worktree_files_loading".to_string())
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(tr_str("layout.worktree.loading_files"))
                .into_any_element()
        } else {
            Self::vertical_scroll_frame(
                theme,
                ("worktree_files_container", repo_id.0),
                ("worktree_files_scrollbar", repo_id.0),
                &self.worktree_files_scroll,
                uniform_list(
                    ("worktree_files_list", repo_id.0),
                    loaded_file_count,
                    cx.processor(Self::render_worktree_file_rows),
                ),
            )
            .into_any_element()
        };

        div()
            .id("worktree_uncommitted_container")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .child(header)
            .child(
                div()
                    .id("worktree_uncommitted_body")
                    .debug_selector(|| "worktree_uncommitted_body".to_string())
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .p_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .child(SharedString::from(t!(
                                "layout.changed_count",
                                count = file_count
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .h_full()
                            .min_h(ui_scale.px(RANGE_FILES_SECTION_MIN_HEIGHT_PX))
                            .border_t_1()
                            .border_color(theme.colors.stroke.subtle)
                            .pt_2()
                            .child(files_body),
                    ),
            )
            .into_any_element()
    }

    /// The details-pane view shown while two points are being compared: the
    /// selected commit cards, a "viewing diff between" subheader, and the list
    /// of files that differ between them. The diff pane starts empty; clicking a
    /// file loads that file's range diff in the main pane.
    fn range_comparison_view(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale = self.ui_scale();

        /// What the files section has to say for itself, kept separate from the
        /// count so a failed load can't read as an empty comparison.
        enum RangeFilesState {
            Loading,
            Failed(String),
            Loaded(usize),
        }

        let (card_count, is_merged_selection, range, files_state) = {
            let Some(repo) = self.active_repo() else {
                return div().into_any_element();
            };
            let Some(range) = repo.history_state.range_selection.clone() else {
                return div().into_any_element();
            };
            let card_count = Self::range_comparison_commits(repo).len();
            // Only a genuine multi-selection is a "merged diff of N commits";
            // every other flow compares two named points, however many of them
            // happen to resolve to a card.
            let is_merged_selection = repo.history_state.multi_selection.is_multi();
            let files_state = match &repo.history_state.range_files {
                Loadable::Ready(files) => RangeFilesState::Loaded(files.len()),
                Loadable::Error(e) => RangeFilesState::Failed(e.clone()),
                Loadable::Loading | Loadable::NotLoaded => RangeFilesState::Loading,
            };
            (card_count, is_merged_selection, range, files_state)
        };

        let header_title: SharedString = if is_merged_selection {
            t!("layout.multi.commits_selected", count = card_count).into()
        } else {
            tr("layout.comparison.title")
        };
        let subheader: SharedString = if is_merged_selection {
            t!("layout.comparison.viewing_merged_diff", count = card_count).into()
        } else {
            t!(
                "layout.comparison.viewing_diff",
                from = range.from_label,
                to = range.to_label
            )
            .into()
        };

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(components::control_height_md(ui_scale))
            .px_2()
            .bg(theme.colors.surface.raised)
            .border_b_1()
            .border_color(theme.colors.stroke.default)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .font_weight(FontWeight::BOLD)
                    .line_clamp(1)
                    .child(header_title),
            )
            .child(
                components::Button::new("range_comparison_close", "")
                    .start_slot(svg_icon(
                        "icons/generic_close.svg",
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        if let Some(repo_id) = this.active_repo_id() {
                            this.store.dispatch(Msg::ClearComparison { repo_id });
                        }
                        cx.notify();
                    })
                    .worktree_tooltip(theme, tr("layout.comparison.close")),
            );

        // Compared-commit preview cards. A two-point comparison has one or two,
        // but a multi-selection has one per selected commit, so the section
        // grows with the selection only up to half the comparison body and
        // scrolls past that — an even split with the changed-file list below,
        // rather than crowding it out.
        //
        // The cap is relative to the body, so it tracks the pane at whatever
        // height the splitter leaves it. The requested height stays definite
        // though: the card list is a `uniform_list`, which paints nothing when
        // its viewport height is indefinite, so `max_h` does the capping rather
        // than the height itself being content-derived.
        let card_row_height = ui_scale.px(MULTI_COMMIT_ROW_HEIGHT_PX);
        let cards = (card_count > 0).then(|| {
            div()
                .debug_selector(|| "range_comparison_cards".to_string())
                .flex()
                .flex_col()
                .w_full()
                .h(card_row_height * card_count as f32)
                .max_h(relative(COMPARISON_CARDS_MAX_BODY_FRACTION))
                .min_h(card_row_height)
                .child(self.commit_cards_list(repo_id, card_count, cx))
        });

        // No count until there is one: claiming "0 changed" while the diff is
        // still running states a number that is usually about to be wrong. The
        // selector names which of the two the label is, so a test can tell them
        // apart without reading painted text.
        let (files_label, files_label_selector): (SharedString, &'static str) = match &files_state {
            RangeFilesState::Loading | RangeFilesState::Failed(_) => (
                tr("layout.comparison.changed_files"),
                "range_files_label_pending",
            ),
            RangeFilesState::Loaded(count) => (
                t!("layout.changed_count", count = *count).into(),
                "range_files_label_count",
            ),
        };
        let files_body: AnyElement = match &files_state {
            RangeFilesState::Loading => div()
                .debug_selector(|| "range_files_loading".to_string())
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(tr_str("layout.loading"))
                .into_any_element(),
            // An error must not render as "No files." — that is exactly what a
            // pair of identical commits looks like, so the user would read a
            // failed comparison as a successful, empty one.
            RangeFilesState::Failed(message) => div()
                .debug_selector(|| "range_files_error".to_string())
                .text_sm()
                .text_color(theme.colors.status.danger.foreground)
                .child(SharedString::from(message.clone()))
                .into_any_element(),
            RangeFilesState::Loaded(0) => div()
                .debug_selector(|| "range_files_empty".to_string())
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(tr_str("layout.no_files"))
                .into_any_element(),
            RangeFilesState::Loaded(count) => Self::vertical_scroll_frame(
                theme,
                ("range_files_container", repo_id.0),
                ("range_files_scrollbar", repo_id.0),
                &self.range_files_scroll,
                uniform_list(
                    ("range_files_list", repo_id.0),
                    *count,
                    cx.processor(Self::render_range_file_rows),
                ),
            )
            .into_any_element(),
        };

        div()
            .id("range_comparison_container")
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .child(header)
            .child(
                div()
                    .id("range_comparison_body")
                    .debug_selector(|| "range_comparison_body".to_string())
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .p_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .child(subheader),
                    )
                    .children(cards)
                    .child(
                        div()
                            .debug_selector(|| "range_comparison_files".to_string())
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .h_full()
                            // A real floor, not `px(0.0)`: the cards above are
                            // sized from the window, so on a pane shorter than
                            // that this is what stops them from taking the whole
                            // pane and collapsing the list to nothing.
                            .min_h(ui_scale.px(RANGE_FILES_SECTION_MIN_HEIGHT_PX))
                            .border_t_1()
                            .border_color(theme.colors.stroke.subtle)
                            .pt_2()
                            .child(
                                div()
                                    .debug_selector(move || files_label_selector.to_string())
                                    .text_sm()
                                    .text_color(theme.colors.foreground.secondary)
                                    .child(files_label),
                            )
                            .child(files_body),
                    ),
            )
            .into_any_element()
    }

    /// Commit message shown in the details pane: a scrollable block whose
    /// summary line is emphasized and whose SHA references are linkified.
    fn commit_details_message_view(&self, theme: AppTheme, repo_id: RepoId) -> AnyElement {
        components::ScrollContainer::vertical(
            ("commit_details_message_scroll_surface", repo_id.0),
            ("commit_details_message_scrollbar", repo_id.0),
            self.commit_scroll.clone(),
            px(COMMIT_DETAILS_MESSAGE_MAX_HEIGHT_PX),
        )
        .container_id(("commit_details_message_container", repo_id.0))
        .debug_selector("commit_details_message_scroll_surface")
        .render(theme, self.commit_details_message_link_menu.clone())
    }

    pub(in crate::view) fn commit_details_view(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale = self.ui_scale();
        let commit_files_min_viewport_height = ui_scale.px(24.0);
        let commit_files_section_min_height = ui_scale.px(44.0);
        let active_repo_id = self.active_repo_id();
        // The uncommitted-changes row parks the not-committed-yet sentinel in
        // `selected_commit`, but what it selects is the working tree, not a
        // commit -- and the pane's default view, the staging sections below,
        // already reviews exactly those files. Reading the sentinel as "no
        // commit selected" keeps the pane on that view instead of swapping in
        // a detail view of its own.
        let selected_id = self
            .active_repo()
            .and_then(|repo| repo.history_state.selected_commit.clone())
            .filter(|commit_id| !commit_id.is_uncommitted());

        // A selected worktree row owns the pane outright: its files belong to a
        // different checkout, so none of the commit-detail views below apply.
        //
        // Only while its scan entry is actually there, though. The reducer drops
        // the selection when the worktree goes clean, but a scan that is still in
        // flight (or that failed) leaves the selection pointing at nothing for a
        // frame or two, and this view has nothing to render without it.
        let has_worktree_selection = self.selected_worktree_summary().is_some();
        if let (Some(repo_id), true) = (active_repo_id, has_worktree_selection) {
            return self.worktree_uncommitted_view(repo_id, cx);
        }

        // An active two-point comparison takes precedence over both the single
        // and multi commit-detail views: show the range's changed files.
        let has_range_comparison = self
            .active_repo()
            .is_some_and(|repo| repo.history_state.range_selection.is_some());
        if let (Some(repo_id), true) = (active_repo_id, has_range_comparison) {
            return self.range_comparison_view(repo_id, cx);
        }

        let multi_count = self
            .active_repo()
            .filter(|repo| repo.history_state.multi_selection.is_multi())
            .map(Self::multi_selected_commits_in_log_order)
            .filter(|commits| commits.len() > 1)
            .map(|commits| commits.len());
        if let (Some(repo_id), Some(count)) = (active_repo_id, multi_count) {
            return self.multi_commit_details_view(repo_id, count, cx);
        }

        if let (Some(repo_id), Some(selected_id)) = (active_repo_id, selected_id) {
            let show_delayed_loading = self.commit_details_delay.as_ref().is_some_and(|s| {
                s.repo_id == repo_id && s.commit_id == selected_id && s.show_loading
            });

            let header_title: SharedString = tr("layout.commit_details.title");

            let header = div()
                .flex()
                .items_center()
                .justify_between()
                .h(components::control_height_md(ui_scale))
                .px_2()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .line_clamp(1)
                        .child(header_title),
                )
                .child(
                    components::Button::new("commit_details_close", "")
                        .start_slot(svg_icon(
                            "icons/generic_close.svg",
                            theme.colors.foreground.secondary,
                            px(12.0),
                        ))
                        .style(components::ButtonStyle::Transparent)
                        .on_click(theme, cx, |this, _e, _w, cx| {
                            // The commit details and diff views are independent
                            // panels; closing details must not close the diff.
                            if let Some(repo_id) = this.active_repo_id() {
                                this.store.dispatch(Msg::ClearCommitSelection { repo_id });
                            }
                            cx.notify();
                        })
                        .worktree_tooltip(theme, tr("layout.commit_details.close")),
                );

            let active_commit_details = self.active_repo().map(|repo| {
                (
                    repo.history_state.commit_details.clone(),
                    repo.history_state.commit_details_rev,
                )
            });
            let body: AnyElement = match active_commit_details.as_ref().map(|(details, _)| details)
            {
                None => components::empty_state(
                    theme,
                    tr_str("layout.commit_details.empty_title"),
                    tr_str("layout.commit_details.no_repository"),
                )
                .into_any_element(),
                Some(Loadable::Loading) => {
                    if show_delayed_loading {
                        components::empty_state(
                            theme,
                            tr_str("layout.commit_details.empty_title"),
                            tr_str("layout.loading"),
                        )
                        .into_any_element()
                    } else {
                        div().into_any_element()
                    }
                }
                Some(Loadable::Error(e)) => components::empty_state(
                    theme,
                    tr_str("layout.commit_details.empty_title"),
                    e.clone(),
                )
                .into_any_element(),
                Some(Loadable::NotLoaded) => {
                    if show_delayed_loading {
                        components::empty_state(
                            theme,
                            tr_str("layout.commit_details.empty_title"),
                            tr_str("layout.loading"),
                        )
                        .into_any_element()
                    } else {
                        div().into_any_element()
                    }
                }
                Some(Loadable::Ready(details)) => {
                    if details.id != selected_id {
                        if show_delayed_loading {
                            components::empty_state(
                                theme,
                                tr_str("layout.commit_details.empty_title"),
                                tr_str("layout.loading"),
                            )
                            .into_any_element()
                        } else {
                            let parent = details
                                .parent_ids
                                .first()
                                .map(|p: &CommitId| p.as_ref().to_string())
                                .unwrap_or_else(|| "—".to_string());

                            let files = if details.files.is_empty() {
                                div()
                                    .text_sm()
                                    .text_color(theme.colors.foreground.secondary)
                                    .child(tr_str("layout.no_files"))
                                    .into_any_element()
                            } else {
                                Self::vertical_scroll_frame(
                                    theme,
                                    ("commit_details_files_container", repo_id.0),
                                    ("commit_details_files_scrollbar", repo_id.0),
                                    &self.commit_files_scroll,
                                    uniform_list(
                                        ("commit_details_files_list", repo_id.0),
                                        details.files.len(),
                                        cx.processor(Self::render_commit_file_rows),
                                    ),
                                )
                                .min_h(commit_files_min_viewport_height)
                                .into_any_element()
                            };

                            self.sync_retained_commit_details_message_input(
                                details.message.as_str(),
                                cx,
                            );
                            Self::sync_commit_details_input_value(
                                &self.commit_details_sha_input,
                                details.id.as_ref(),
                                cx,
                            );
                            Self::sync_commit_details_input_value(
                                &self.commit_details_date_input,
                                self.commit_details_date_display(details).as_str(),
                                cx,
                            );
                            self.sync_commit_details_parent_input(
                                parent.as_str(),
                                RepoId(0),
                                false,
                                theme,
                                cx,
                            );

                            let message = self.commit_details_message_view(theme, repo_id);

                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .flex_1()
                                .h_full()
                                .min_h(px(0.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .w_full()
                                        .min_w(px(0.0))
                                        .child(message)
                                        .children(commit_details_author_row(
                                            theme, ui_scale, details, cx,
                                        ))
                                        .child(commit_details_selectable_row(
                                            theme,
                                            tr_str("layout.commit_details.commit_sha"),
                                            // The signed badge rides beside the
                                            // sha, mirroring the history row's
                                            // existence marker (green ✓).
                                            if details.signed {
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .min_w(px(0.0))
                                                    .child(commit_details_monospace_value(
                                                        self.commit_details_sha_input.clone(),
                                                    ))
                                                    .child(
                                                        div()
                                                            .id("commit_details_signed_badge")
                                                            .debug_selector(|| {
                                                                "commit_details_signed_badge"
                                                                    .to_string()
                                                            })
                                                            .flex_none()
                                                            .text_xs()
                                                            .text_color(
                                                                theme
                                                                    .colors
                                                                    .status
                                                                    .success
                                                                    .foreground,
                                                            )
                                                            .worktree_tooltip(
                                                                theme,
                                                                tr("layout.commit_details.signed"),
                                                            )
                                                            .child("✓"),
                                                    )
                                                    .into_any_element()
                                            } else {
                                                commit_details_monospace_value(
                                                    self.commit_details_sha_input.clone(),
                                                )
                                            },
                                        ))
                                        .child(commit_details_selectable_row(
                                            theme,
                                            tr_str("layout.commit_details.commit_date"),
                                            commit_details_monospace_value(
                                                self.commit_details_date_input.clone(),
                                            ),
                                        ))
                                        .child(commit_details_selectable_row(
                                            theme,
                                            tr_str("layout.commit_details.parent_commit_sha"),
                                            commit_details_monospace_value(
                                                self.commit_details_parent_input.clone(),
                                            ),
                                        )),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .flex_1()
                                        .h_full()
                                        .min_h(commit_files_section_min_height)
                                        .border_t_1()
                                        .border_color(theme.colors.stroke.subtle)
                                        .pt_2()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(theme.colors.foreground.secondary)
                                                .child(format!(
                                                    "Committed files ({})",
                                                    details.files.len()
                                                )),
                                        )
                                        .child(files),
                                )
                                .into_any_element()
                        }
                    } else {
                        let parent = details
                            .parent_ids
                            .first()
                            .map(|p: &CommitId| p.as_ref().to_string())
                            .unwrap_or_else(|| "—".to_string());

                        let files = if details.files.is_empty() {
                            div()
                                .text_sm()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("layout.no_files"))
                                .into_any_element()
                        } else {
                            Self::vertical_scroll_frame(
                                theme,
                                ("commit_details_files_container", repo_id.0),
                                ("commit_details_files_scrollbar", repo_id.0),
                                &self.commit_files_scroll,
                                uniform_list(
                                    ("commit_details_files_list", repo_id.0),
                                    details.files.len(),
                                    cx.processor(Self::render_commit_file_rows),
                                ),
                            )
                            .min_h(commit_files_min_viewport_height)
                            .into_any_element()
                        };

                        self.sync_commit_details_message_input(
                            details.message.as_str(),
                            theme,
                            repo_id,
                            cx,
                        );
                        Self::sync_commit_details_input_value(
                            &self.commit_details_sha_input,
                            details.id.as_ref(),
                            cx,
                        );
                        Self::sync_commit_details_input_value(
                            &self.commit_details_date_input,
                            self.commit_details_date_display(details).as_str(),
                            cx,
                        );
                        self.sync_commit_details_sha_menu(
                            details.id.as_ref(),
                            repo_id,
                            true,
                            theme,
                            cx,
                        );
                        self.sync_commit_details_parent_input(
                            parent.as_str(),
                            repo_id,
                            parent != "—",
                            theme,
                            cx,
                        );

                        let message = self.commit_details_message_view(theme, repo_id);

                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .flex_1()
                            .h_full()
                            .min_h(px(0.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .w_full()
                                    .min_w(px(0.0))
                                    .child(message)
                                    .children(commit_details_author_row(
                                        theme, ui_scale, details, cx,
                                    ))
                                    .child(commit_details_selectable_row(
                                        theme,
                                        tr_str("layout.commit_details.commit_sha"),
                                        commit_details_monospace_element(
                                            self.commit_details_sha_link_menu
                                                .clone()
                                                .into_any_element(),
                                        ),
                                    ))
                                    .child(commit_details_selectable_row(
                                        theme,
                                        tr_str("layout.commit_details.commit_date"),
                                        commit_details_monospace_value(
                                            self.commit_details_date_input.clone(),
                                        ),
                                    ))
                                    .child(commit_details_selectable_row(
                                        theme,
                                        tr_str("layout.commit_details.parent_commit_sha"),
                                        commit_details_monospace_element(
                                            self.commit_details_parent_link_menu
                                                .clone()
                                                .into_any_element(),
                                        ),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .flex_1()
                                    .h_full()
                                    .min_h(commit_files_section_min_height)
                                    .border_t_1()
                                    .border_color(theme.colors.stroke.subtle)
                                    .pt_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.colors.foreground.secondary)
                                            .child(SharedString::from(t!(
                                                "layout.commit_details.committed_files",
                                                count = details.files.len()
                                            ))),
                                    )
                                    .child(files),
                            )
                            .into_any_element()
                    }
                }
            };

            return div()
                .id("commit_details_container")
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .min_h(px(0.0))
                .child(header)
                .child(
                    div()
                        .id("commit_details_body_container")
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .h_full()
                        .min_h(px(0.0))
                        .p_2()
                        .child(body),
                )
                .into_any_element();
        }

        let local_actions_in_flight = self
            .active_repo()
            .map(|r| r.local_actions_in_flight > 0)
            .unwrap_or(false);
        let (staged_count, unstaged_count, untracked_count, split_unstaged_count) = self
            .active_repo()
            .map(|repo| {
                (
                    StatusSectionEntries::from_repo(repo, StatusSection::Staged)
                        .map_or(0, |entries| entries.len()),
                    StatusSectionEntries::from_repo(repo, StatusSection::CombinedUnstaged)
                        .map_or(0, |entries| entries.len()),
                    StatusSectionEntries::from_repo(repo, StatusSection::Untracked)
                        .map_or(0, |entries| entries.len()),
                    StatusSectionEntries::from_repo(repo, StatusSection::Unstaged)
                        .map_or(0, |entries| entries.len()),
                )
            })
            .unwrap_or((0, 0, 0, 0));
        let (untracked_paths, split_unstaged_paths) = self
            .active_repo()
            .map(|repo| {
                (
                    StatusSectionEntries::from_repo(repo, StatusSection::Untracked)
                        .map_or_else(Vec::new, |entries| entries.path_vec()),
                    StatusSectionEntries::from_repo(repo, StatusSection::Unstaged)
                        .map_or_else(Vec::new, |entries| entries.path_vec()),
                )
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new()));
        let (unstaged_loading, untracked_loading, split_unstaged_loading, staged_loading) = self
            .active_repo()
            .map(|repo| {
                (
                    status_section_is_loading(repo, StatusSection::CombinedUnstaged),
                    status_section_is_loading(repo, StatusSection::Untracked),
                    status_section_is_loading(repo, StatusSection::Unstaged),
                    status_section_is_loading(repo, StatusSection::Staged),
                )
            })
            .unwrap_or((false, false, false, false));

        let repo_id = self.active_repo_id();
        let selected_combined_unstaged = repo_id
            .map(|rid| {
                self.status_section_action_selection(rid, StatusSection::CombinedUnstaged)
                    .count()
            })
            .unwrap_or(0);
        let selected_untracked = repo_id
            .map(|rid| {
                self.status_section_action_selection(rid, StatusSection::Untracked)
                    .count()
            })
            .unwrap_or(0);
        let selected_split_unstaged = repo_id
            .map(|rid| {
                self.status_section_action_selection(rid, StatusSection::Unstaged)
                    .count()
            })
            .unwrap_or(0);
        let selected_staged = repo_id
            .map(|rid| {
                self.status_section_action_selection(rid, StatusSection::Staged)
                    .count()
            })
            .unwrap_or(0);

        let spinner = |id: (&'static str, u64), color: gpui::Rgba| svg_spinner(id, color, px(14.0));
        let repo_key = repo_id.map(|id| id.0).unwrap_or(0);
        let split_change_tracking = self.change_tracking_view == ChangeTrackingView::SplitUntracked;
        let icon_muted = with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.72 } else { 0.82 },
        );
        let ui_scale_percent = crate::ui_scale::current(cx).percent;

        // Measured last frame by the probe on the sections container below; the
        // prepaint callback refreshes the window when it changes. Unmeasured on
        // the very first frame, which reads as "plenty of room" and settles on
        // the next one.
        let header_width = self
            .current_status_sections_bounds()
            .map(|bounds| bounds.size.width)
            .unwrap_or(Pixels::MAX);
        let labels_for =
            |title_chars: usize, title_is_dropdown: bool, action_label_chars: &[usize]| {
                status_action_labels_for_width(
                    header_width,
                    title_chars,
                    title_is_dropdown,
                    action_label_chars,
                    local_actions_in_flight,
                    ui_scale_percent,
                )
            };
        // Locale-dependent words, resolved once so the width budget above and
        // the painted labels below can never disagree.
        let unstaged_title = tr_str("layout.status.unstaged");
        let untracked_title = tr_str("layout.status.untracked");
        let staged_title = tr_str("layout.status.staged");
        let stage_word = tr_str("layout.status.stage");
        let discard_word = tr_str("layout.status.discard");
        let unstage_word = tr_str("layout.status.unstage");
        let stage_all_text = tr_str("layout.status.stage_all");
        let stage_all_changes_text = tr_str("layout.status.stage_all_changes");
        let unstage_all_changes_text = tr_str("layout.status.unstage_all_changes");
        let count_chars =
            |word: &str, count: usize| label_width_chars(word) + 3 + count.to_string().len();
        let unstaged_labels = if selected_combined_unstaged > 0 {
            labels_for(
                label_width_chars(unstaged_title),
                true,
                &[
                    count_chars(stage_word, selected_combined_unstaged),
                    count_chars(discard_word, selected_combined_unstaged),
                    label_width_chars(stage_all_changes_text),
                ],
            )
        } else {
            labels_for(
                label_width_chars(unstaged_title),
                true,
                &[label_width_chars(stage_all_changes_text)],
            )
        };
        let untracked_labels = if selected_untracked > 0 {
            labels_for(
                label_width_chars(untracked_title),
                true,
                &[
                    count_chars(stage_word, selected_untracked),
                    count_chars(discard_word, selected_untracked),
                    label_width_chars(stage_all_text),
                ],
            )
        } else {
            labels_for(
                label_width_chars(untracked_title),
                true,
                &[label_width_chars(stage_all_text)],
            )
        };
        let split_unstaged_labels = if selected_split_unstaged > 0 {
            labels_for(
                label_width_chars(unstaged_title),
                true,
                &[
                    count_chars(stage_word, selected_split_unstaged),
                    count_chars(discard_word, selected_split_unstaged),
                    label_width_chars(stage_all_text),
                ],
            )
        } else {
            labels_for(
                label_width_chars(unstaged_title),
                true,
                &[label_width_chars(stage_all_text)],
            )
        };
        let staged_labels = if selected_staged > 0 {
            labels_for(
                label_width_chars(staged_title),
                false,
                &[
                    count_chars(unstage_word, selected_staged),
                    label_width_chars(unstage_all_changes_text),
                ],
            )
        } else {
            labels_for(
                label_width_chars(staged_title),
                false,
                &[label_width_chars(unstage_all_changes_text)],
            )
        };

        let stage_all = components::Button::new(
            "stage_all",
            status_action_all_label(unstaged_labels, stage_all_changes_text),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            // Empty paths: this button stages every change there is.
            this.stage_all_with_conflict_confirmation(repo_id, Vec::new(), _w, cx);
        })
        .worktree_tooltip(theme, tr("layout.status.stage_all_changes"));

        let stage_selected = components::Button::new(
            "stage_selected",
            status_action_count_label(unstaged_labels, stage_word, selected_combined_unstaged),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            // Read without consuming: the confirmation below can still be
            // cancelled, and that must leave the selection as the user built it.
            let selection =
                this.status_section_action_selection(repo_id, StatusSection::CombinedUnstaged);
            let paths = selection.paths;
            if paths.is_empty() {
                return;
            }
            if let Some(confirm) = crate::view::conflict_markers::stage_confirm_popover(
                &this.state,
                repo_id,
                paths.clone(),
                selection.from_explicit_selection,
            ) {
                let anchor = crate::view::conflict_markers::centered_dialog_anchor(_w);
                this.open_popover_at(confirm, anchor, _w, cx);
                cx.notify();
                return;
            }
            if selection.from_explicit_selection {
                this.clear_status_multi_selection(repo_id);
            }
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::StagePaths {
                repo_id,
                paths: paths.into(),
            });
            cx.notify();
        })
        .debug_selector(|| "stage_selected_button".to_string())
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.stage_selected_tooltip",
                count = selected_combined_unstaged,
                file_word = status_action_file_count(selected_combined_unstaged)
            )
            .into(),
        );

        let discard_selected = components::Button::new(
            "discard_selected",
            status_action_count_label(unstaged_labels, discard_word, selected_combined_unstaged),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, e, window, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            let selection =
                this.status_section_action_selection(repo_id, StatusSection::CombinedUnstaged);
            if selection.paths.is_empty() {
                return;
            }
            this.open_popover_at(
                PopoverKind::DiscardChangesConfirm {
                    repo_id,
                    area: DiffArea::Unstaged,
                    path: selection.popover_path(),
                },
                e.position(),
                window,
                cx,
            );
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.discard_selected_tooltip",
                count = selected_combined_unstaged,
                file_word = status_action_file_count(selected_combined_unstaged)
            )
            .into(),
        );

        let untracked_paths_for_stage_all =
            worktree_state::msg::RepoPathList::from(untracked_paths.clone());
        let stage_all_untracked = components::Button::new(
            "stage_all_untracked",
            status_action_all_label(untracked_labels, stage_all_text),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight || untracked_paths_for_stage_all.is_empty())
        .on_click(theme, cx, move |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            if untracked_paths_for_stage_all.is_empty() {
                return;
            }
            this.status_multi_selection.remove(&repo_id);
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::StagePaths {
                repo_id,
                paths: untracked_paths_for_stage_all.clone(),
            });
            cx.notify();
        })
        .worktree_tooltip(theme, tr("layout.status.stage_all_untracked"));

        let stage_selected_untracked = components::Button::new(
            "stage_selected_untracked",
            status_action_count_label(untracked_labels, stage_word, selected_untracked),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            // Read without consuming: the confirmation below can still be
            // cancelled, and that must leave the selection as the user built it.
            let selection = this.status_section_action_selection(repo_id, StatusSection::Untracked);
            let paths = selection.paths;
            if paths.is_empty() {
                return;
            }
            if let Some(confirm) = crate::view::conflict_markers::stage_confirm_popover(
                &this.state,
                repo_id,
                paths.clone(),
                selection.from_explicit_selection,
            ) {
                let anchor = crate::view::conflict_markers::centered_dialog_anchor(_w);
                this.open_popover_at(confirm, anchor, _w, cx);
                cx.notify();
                return;
            }
            if selection.from_explicit_selection {
                this.clear_status_multi_selection(repo_id);
            }
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::StagePaths {
                repo_id,
                paths: paths.into(),
            });
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.stage_selected_tooltip",
                count = selected_untracked,
                file_word = status_action_file_count(selected_untracked)
            )
            .into(),
        );

        let discard_selected_untracked = components::Button::new(
            "discard_selected_untracked",
            status_action_count_label(untracked_labels, discard_word, selected_untracked),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, e, window, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            let selection = this.status_section_action_selection(repo_id, StatusSection::Untracked);
            if selection.paths.is_empty() {
                return;
            }
            this.open_popover_at(
                PopoverKind::DiscardChangesConfirm {
                    repo_id,
                    area: DiffArea::Unstaged,
                    path: selection.popover_path(),
                },
                e.position(),
                window,
                cx,
            );
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.discard_selected_tooltip",
                count = selected_untracked,
                file_word = status_action_file_count(selected_untracked)
            )
            .into(),
        );

        let split_unstaged_paths_for_stage_all = split_unstaged_paths.clone();
        let stage_all_split_unstaged = components::Button::new(
            "stage_all_split_unstaged",
            status_action_all_label(split_unstaged_labels, stage_all_text),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight || split_unstaged_paths_for_stage_all.is_empty())
        .on_click(theme, cx, move |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            if split_unstaged_paths_for_stage_all.is_empty() {
                return;
            }
            // Named paths: this button stages the tracked-changes section only —
            // conflicted files among them, so it needs the same confirmation the
            // combined view's button gets.
            this.stage_all_with_conflict_confirmation(
                repo_id,
                split_unstaged_paths_for_stage_all.clone(),
                _w,
                cx,
            );
        })
        .worktree_tooltip(theme, tr("layout.status.stage_all_unstaged"));

        let stage_selected_split_unstaged = components::Button::new(
            "stage_selected_split_unstaged",
            status_action_count_label(split_unstaged_labels, stage_word, selected_split_unstaged),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            // Read without consuming: the confirmation below can still be
            // cancelled, and that must leave the selection as the user built it.
            let selection = this.status_section_action_selection(repo_id, StatusSection::Unstaged);
            let paths = selection.paths;
            if paths.is_empty() {
                return;
            }
            if let Some(confirm) = crate::view::conflict_markers::stage_confirm_popover(
                &this.state,
                repo_id,
                paths.clone(),
                selection.from_explicit_selection,
            ) {
                let anchor = crate::view::conflict_markers::centered_dialog_anchor(_w);
                this.open_popover_at(confirm, anchor, _w, cx);
                cx.notify();
                return;
            }
            if selection.from_explicit_selection {
                this.clear_status_multi_selection(repo_id);
            }
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::StagePaths {
                repo_id,
                paths: paths.into(),
            });
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.stage_selected_tooltip",
                count = selected_split_unstaged,
                file_word = status_action_file_count(selected_split_unstaged)
            )
            .into(),
        );

        let discard_selected_split_unstaged = components::Button::new(
            "discard_selected_split_unstaged",
            status_action_count_label(split_unstaged_labels, discard_word, selected_split_unstaged),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, e, window, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            let selection = this.status_section_action_selection(repo_id, StatusSection::Unstaged);
            if selection.paths.is_empty() {
                return;
            }
            this.open_popover_at(
                PopoverKind::DiscardChangesConfirm {
                    repo_id,
                    area: DiffArea::Unstaged,
                    path: selection.popover_path(),
                },
                e.position(),
                window,
                cx,
            );
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.discard_selected_tooltip",
                count = selected_split_unstaged,
                file_word = status_action_file_count(selected_split_unstaged)
            )
            .into(),
        );

        let unstage_all = components::Button::new(
            "unstage_all",
            status_action_all_label(staged_labels, unstage_all_changes_text),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            this.status_multi_selection.remove(&repo_id);
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::UnstagePaths {
                repo_id,
                paths: Default::default(),
            });
            cx.notify();
        })
        .worktree_tooltip(theme, tr("layout.status.unstage_all_changes"));

        let unstage_selected = components::Button::new(
            "unstage_selected",
            status_action_count_label(staged_labels, unstage_word, selected_staged),
        )
        .style(components::ButtonStyle::Subtle)
        .disabled(local_actions_in_flight)
        .on_click(theme, cx, |this, _e, _w, cx| {
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            let paths = this
                .take_status_section_action_selection(repo_id, StatusSection::Staged)
                .paths;
            if paths.is_empty() {
                return;
            }
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::UnstagePaths {
                repo_id,
                paths: paths.into(),
            });
            cx.notify();
        })
        .worktree_tooltip(
            theme,
            t!(
                "layout.status.unstage_selected_tooltip",
                count = selected_staged,
                file_word = status_action_file_count(selected_staged)
            )
            .into(),
        );

        let section_header = |id: &'static str,
                              title: gpui::AnyElement,
                              show_action: bool,
                              action: gpui::AnyElement|
         -> gpui::AnyElement {
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .h(components::control_height_md(ui_scale_percent))
                .px_2()
                .overflow_hidden()
                // The labels shrink before this matters, but a UI zoom or a font
                // wider than the budget assumes can still overrun the header —
                // and then the title, not the actions, is what gives way.
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .child(title),
                )
                .when(show_action, |d| d.child(div().flex_none().child(action)))
                .into_any_element()
        };

        let normal_header_title = |label: &'static str| {
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .line_clamp(1)
                .whitespace_nowrap()
                .child(label)
                .into_any_element()
        };

        let section_min_h = px(STATUS_SECTION_MIN_HEIGHT_PX);
        let resize_handle_h = px(PANE_RESIZE_HANDLE_PX);

        let unstaged_actions = {
            let mut actions = div().flex().items_center().gap_2();
            if local_actions_in_flight {
                actions = actions.child(
                    spinner(
                        ("unstaged_actions_spinner", repo_key),
                        with_alpha(
                            theme.colors.accent.foreground,
                            if theme.is_dark { 0.72 } else { 0.82 },
                        ),
                    )
                    .into_any_element(),
                );
            }
            if selected_combined_unstaged > 0 {
                actions = actions.child(stage_selected).child(discard_selected);
            }
            actions.child(stage_all).into_any_element()
        };

        let untracked_actions = {
            let mut actions = div().flex().items_center().gap_2();
            if local_actions_in_flight {
                actions = actions.child(
                    spinner(
                        ("untracked_actions_spinner", repo_key),
                        with_alpha(
                            theme.colors.accent.foreground,
                            if theme.is_dark { 0.72 } else { 0.82 },
                        ),
                    )
                    .into_any_element(),
                );
            }
            if selected_untracked > 0 {
                actions = actions
                    .child(stage_selected_untracked)
                    .child(discard_selected_untracked);
            }
            actions.child(stage_all_untracked).into_any_element()
        };

        let split_unstaged_actions = {
            let mut actions = div().flex().items_center().gap_2();
            if local_actions_in_flight {
                actions = actions.child(
                    spinner(
                        ("split_unstaged_actions_spinner", repo_key),
                        with_alpha(
                            theme.colors.accent.foreground,
                            if theme.is_dark { 0.72 } else { 0.82 },
                        ),
                    )
                    .into_any_element(),
                );
            }
            if selected_split_unstaged > 0 {
                actions = actions
                    .child(stage_selected_split_unstaged)
                    .child(discard_selected_split_unstaged);
            }
            actions.child(stage_all_split_unstaged).into_any_element()
        };

        let staged_actions = {
            let mut actions = div().flex().items_center().gap_2();
            if local_actions_in_flight {
                actions = actions.child(
                    spinner(
                        ("staged_actions_spinner", repo_key),
                        with_alpha(
                            theme.colors.accent.foreground,
                            if theme.is_dark { 0.72 } else { 0.82 },
                        ),
                    )
                    .into_any_element(),
                );
            }
            if selected_staged > 0 {
                actions = actions.child(unstage_selected);
            }
            actions.child(unstage_all).into_any_element()
        };

        let unstaged_body = if unstaged_loading {
            components::empty_state_message(theme, tr_str("layout.status.loading"))
                .into_any_element()
        } else if unstaged_count == 0 {
            components::empty_state_message(theme, tr_str("layout.status.no_unstaged_changes"))
                .into_any_element()
        } else {
            self.status_list(cx, StatusSection::CombinedUnstaged, unstaged_count)
        };

        let untracked_body = if untracked_loading {
            components::empty_state_message(theme, tr_str("layout.status.loading"))
                .into_any_element()
        } else if untracked_count == 0 {
            components::empty_state_message(theme, tr_str("layout.status.no_untracked_files"))
                .into_any_element()
        } else {
            self.status_list(cx, StatusSection::Untracked, untracked_count)
        };

        let split_unstaged_body = if split_unstaged_loading {
            components::empty_state_message(theme, tr_str("layout.status.loading"))
                .into_any_element()
        } else if split_unstaged_count == 0 {
            components::empty_state_message(theme, tr_str("layout.status.no_unstaged_changes"))
                .into_any_element()
        } else {
            self.status_list(cx, StatusSection::Unstaged, split_unstaged_count)
        };

        let staged_list = if staged_loading {
            components::empty_state_message(theme, tr_str("layout.status.loading"))
                .into_any_element()
        } else if staged_count == 0 {
            components::empty_state_message(theme, tr_str("layout.status.nothing_staged"))
                .into_any_element()
        } else {
            self.status_list(cx, StatusSection::Staged, staged_count)
        };

        let build_change_tracking_header_title =
            |id: &'static str, invoker_key: &'static str, label: &'static str| {
                let change_tracking_invoker: SharedString = invoker_key.into();
                let change_tracking_active =
                    self.active_context_menu_invoker.as_ref() == Some(&change_tracking_invoker);
                let change_tracking_invoker = change_tracking_invoker.clone();
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .h(px(18.0))
                    .rounded(px(theme.radii.row))
                    .when(change_tracking_active, |d| {
                        d.bg(theme.colors.interaction.pressed_background)
                    })
                    .hover(move |s| {
                        if change_tracking_active {
                            s.bg(theme.colors.interaction.pressed_background)
                        } else {
                            s.bg(with_alpha(theme.colors.interaction.hover_background, 0.55))
                        }
                    })
                    .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                    .cursor(CursorStyle::PointingHand)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .child(label),
                    )
                    .child(svg_icon("icons/chevron_down.svg", icon_muted, px(12.0)))
                    .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                        this.activate_context_menu_invoker(change_tracking_invoker.clone(), cx);
                        this.open_popover_at(
                            PopoverKind::ChangeTrackingSettings,
                            e.position(),
                            window,
                            cx,
                        );
                        cx.notify();
                    }))
                    .into_any_element()
            };

        let build_unstaged_header_title = || {
            build_change_tracking_header_title(
                "change_tracking_unstaged_header",
                "change_tracking_unstaged_header",
                unstaged_title,
            )
        };

        let build_untracked_header_title = || {
            build_change_tracking_header_title(
                "change_tracking_untracked_header",
                "change_tracking_untracked_header",
                untracked_title,
            )
        };

        let active_status_resize = self.status_section_resize;
        let build_status_resize_handle = |id: &'static str, handle: StatusSectionResizeHandle| {
            let dragging = active_status_resize.is_some_and(|state| state.handle == handle);
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .group(id)
                .w_full()
                .h(resize_handle_h)
                .flex_none()
                .cursor(CursorStyle::ResizeUpDown)
                .child(components::resize_grip(
                    theme,
                    ui_scale,
                    id,
                    components::ResizeGripAxis::Horizontal,
                    dragging,
                    Some(theme.colors.stroke.default),
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        crate::press_gesture::claim_press(cx);
                        this.start_status_section_resize(handle, e.position.y, cx);
                        window.refresh();
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _e, window, cx| {
                        if this
                            .status_section_resize
                            .is_some_and(|state| state.handle == handle)
                        {
                            this.finish_status_section_resize(cx);
                            window.refresh();
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(move |this, _e, window, cx| {
                        if this
                            .status_section_resize
                            .is_some_and(|state| state.handle == handle)
                        {
                            this.finish_status_section_resize(cx);
                            window.refresh();
                        }
                    }),
                )
        };

        let with_split_sizing = |mut section: gpui::Div,
                                 exact_height: Option<Pixels>,
                                 fallback_grow: f32,
                                 min_h: Pixels| {
            section = section.min_h(min_h);
            if let Some(exact_height) = exact_height {
                let exact_height = exact_height.max(min_h);
                section = section.h(exact_height).max_h(exact_height);
                section.style().flex_grow = Some(0.0);
                section.style().flex_shrink = Some(0.0);
                section.style().flex_basis = Some(exact_height.into());
            } else {
                section.style().flex_grow = Some(fallback_grow.max(1.0));
                section.style().flex_shrink = Some(1.0);
                section.style().flex_basis = Some(relative(0.0).into());
            }
            section
        };
        let px_to_grow = |value: Pixels| -> f32 {
            let px_value: f32 = value.into();
            px_value.max(1.0)
        };

        let change_tracking_total_height =
            self.measured_status_sections_total_height(resize_handle_h);
        let change_tracking_heights = change_tracking_total_height.map(|total_height| {
            let top_height = resolved_vertical_split_height(
                self.change_tracking_height,
                total_height,
                min_change_tracking_stack_height(split_change_tracking, resize_handle_h),
                section_min_h,
            );
            (top_height, (total_height - top_height).max(section_min_h))
        });

        let untracked_total_height =
            self.resolved_measured_change_tracking_stack_total_height(resize_handle_h);
        let untracked_heights = untracked_total_height.map(|total_height| {
            let top_height = resolved_vertical_split_height(
                self.untracked_height,
                total_height,
                section_min_h,
                section_min_h,
            );
            (top_height, (total_height - top_height).max(section_min_h))
        });
        let unstaged_section = div()
            .flex()
            .flex_col()
            .min_h(section_min_h)
            .overflow_hidden()
            .child(section_header(
                "unstaged_header",
                build_unstaged_header_title(),
                unstaged_count > 0,
                unstaged_actions,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(unstaged_body),
            );

        let untracked_section = div()
            .flex()
            .flex_col()
            .min_h(section_min_h)
            .overflow_hidden()
            .child(section_header(
                "untracked_header",
                build_untracked_header_title(),
                untracked_count > 0,
                untracked_actions,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(untracked_body),
            );

        let split_unstaged_section = div()
            .flex()
            .flex_col()
            .min_h(section_min_h)
            .overflow_hidden()
            .child(section_header(
                "split_unstaged_header",
                build_unstaged_header_title(),
                split_unstaged_count > 0,
                split_unstaged_actions,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(split_unstaged_body),
            );

        let staged_section = div()
            .flex()
            .flex_col()
            .min_h(section_min_h)
            .overflow_hidden()
            .child(section_header(
                "staged_header",
                normal_header_title(staged_title),
                staged_count > 0,
                staged_actions,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(staged_list),
            );

        let change_tracking_section = if split_change_tracking {
            let change_tracking_stack_bounds_for_prepaint =
                std::rc::Rc::clone(&self.change_tracking_stack_bounds_ref);
            let stack_container = div()
                .relative()
                .flex()
                .flex_col()
                .w_full()
                .min_w_full()
                .max_w_full()
                .h_full()
                .min_h(min_change_tracking_stack_height(
                    split_change_tracking,
                    resize_handle_h,
                ))
                .overflow_hidden()
                .on_children_prepainted(move |children_bounds, window, _app| {
                    let next_bounds = children_bounds.first().copied();
                    let mut measured = change_tracking_stack_bounds_for_prepaint.borrow_mut();
                    if *measured != next_bounds {
                        *measured = next_bounds;
                        window.refresh();
                    }
                });
            let untracked_top_height = untracked_heights.map(|(top_height, _)| top_height);
            let split_unstaged_height = untracked_heights.map(|(_, bottom_height)| bottom_height);
            let (untracked_grow, split_unstaged_grow) = untracked_heights
                .map(|(top_height, bottom_height)| {
                    (px_to_grow(top_height), px_to_grow(bottom_height))
                })
                .unwrap_or((1.0, 1.0));
            stack_container
                .child(visible_bounds_probe())
                .child(
                    with_split_sizing(
                        untracked_section,
                        untracked_top_height,
                        untracked_grow,
                        section_min_h,
                    )
                    .debug_selector(|| "status_untracked_wrapper".to_string()),
                )
                .child(build_status_resize_handle(
                    "status_resize_untracked_unstaged",
                    StatusSectionResizeHandle::UntrackedAndUnstaged,
                ))
                .child(
                    with_split_sizing(
                        split_unstaged_section,
                        split_unstaged_height,
                        split_unstaged_grow,
                        section_min_h,
                    )
                    .debug_selector(|| "status_split_unstaged_wrapper".to_string()),
                )
        } else {
            unstaged_section
        };
        let (change_tracking_grow, staged_grow) = change_tracking_heights
            .map(|(top_height, bottom_height)| (px_to_grow(top_height), px_to_grow(bottom_height)))
            .unwrap_or((1.0, 1.0));
        let change_tracking_section = with_split_sizing(
            change_tracking_section,
            change_tracking_heights.map(|(top_height, _)| top_height),
            change_tracking_grow,
            min_change_tracking_stack_height(split_change_tracking, resize_handle_h),
        );
        let staged_section = with_split_sizing(
            staged_section,
            change_tracking_heights.map(|(_, bottom_height)| bottom_height),
            staged_grow,
            section_min_h,
        );
        let change_tracking_section =
            change_tracking_section.debug_selector(|| "status_change_tracking_wrapper".to_string());
        let staged_section = staged_section.debug_selector(|| "status_staged_wrapper".to_string());
        let status_sections_bounds_for_prepaint =
            std::rc::Rc::clone(&self.status_sections_bounds_ref);
        let status_sections_container = div()
            .relative()
            .w_full()
            .min_w_full()
            .max_w_full()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .overflow_hidden()
            .on_children_prepainted(move |children_bounds, window, _app| {
                let next_bounds = children_bounds.first().copied();
                let mut measured = status_sections_bounds_for_prepaint.borrow_mut();
                if *measured != next_bounds {
                    *measured = next_bounds;
                    window.refresh();
                }
            });
        let status_sections = status_sections_container
            .child(visible_bounds_probe())
            .flex()
            .flex_col()
            .child(change_tracking_section)
            .child(build_status_resize_handle(
                "status_resize_change_tracking_staged",
                StatusSectionResizeHandle::ChangeTrackingAndStaged,
            ))
            .child(staged_section);

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .h_full()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e, _w, cx| {
                    this.finish_status_section_resize(cx);
                }),
            )
            .child(if repo_id.is_some() {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(status_sections)
                    .child(div().px_2().py_2().child(self.commit_box(cx)))
                    .into_any_element()
            } else {
                components::empty_state(
                    theme,
                    tr_str("layout.status.changes_title"),
                    tr_str("layout.status.no_repository_selected"),
                )
                .into_any_element()
            })
            .into_any_element()
    }

    pub(in crate::view) fn status_list(
        &mut self,
        cx: &mut gpui::Context<Self>,
        section: StatusSection,
        count: usize,
    ) -> AnyElement {
        let theme = self.theme;
        if count == 0 {
            return components::empty_state_message(
                theme,
                tr_str("layout.status.working_tree_clean"),
            )
            .into_any_element();
        }
        match section {
            StatusSection::CombinedUnstaged => {
                let list =
                    uniform_list("unstaged", count, cx.processor(Self::render_unstaged_rows))
                        .h_full()
                        .min_h(px(0.0))
                        .track_scroll(&self.unstaged_scroll);
                let list = restrict_scroll_to_vertical_axis(list);
                let list = div()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        self.unstaged_scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list);
                div()
                    .id("unstaged_scroll_container")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(list)
                    .child(
                        components::Scrollbar::new(
                            "unstaged_scrollbar",
                            self.unstaged_scroll.clone(),
                        )
                        .render(theme),
                    )
                    .into_any_element()
            }
            StatusSection::Untracked => {
                let list = uniform_list(
                    "untracked",
                    count,
                    cx.processor(Self::render_untracked_rows),
                )
                .h_full()
                .min_h(px(0.0))
                .track_scroll(&self.untracked_scroll);
                let list = restrict_scroll_to_vertical_axis(list);
                let list = div()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        self.untracked_scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list);
                div()
                    .id("untracked_scroll_container")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(list)
                    .child(
                        components::Scrollbar::new(
                            "untracked_scrollbar",
                            self.untracked_scroll.clone(),
                        )
                        .render(theme),
                    )
                    .into_any_element()
            }
            StatusSection::Unstaged => {
                let list = uniform_list(
                    "split_unstaged",
                    count,
                    cx.processor(Self::render_split_unstaged_rows),
                )
                .h_full()
                .min_h(px(0.0))
                .track_scroll(&self.unstaged_scroll);
                let list = restrict_scroll_to_vertical_axis(list);
                let list = div()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        self.unstaged_scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list);
                div()
                    .id("split_unstaged_scroll_container")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(list)
                    .child(
                        components::Scrollbar::new(
                            "split_unstaged_scrollbar",
                            self.unstaged_scroll.clone(),
                        )
                        .render(theme),
                    )
                    .into_any_element()
            }
            StatusSection::Staged => {
                let list = uniform_list("staged", count, cx.processor(Self::render_staged_rows))
                    .h_full()
                    .min_h(px(0.0))
                    .track_scroll(&self.staged_scroll);
                let list = restrict_scroll_to_vertical_axis(list);
                let list = div()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        self.staged_scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list);
                div()
                    .id("staged_scroll_container")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .child(list)
                    .child(
                        components::Scrollbar::new("staged_scrollbar", self.staged_scroll.clone())
                            .render(theme),
                    )
                    .into_any_element()
            }
        }
    }

    pub(in crate::view) fn commit_box(&mut self, cx: &mut gpui::Context<Self>) -> gpui::Div {
        let theme = self.theme;
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let commit_in_flight = self
            .active_repo()
            .is_some_and(|repo| repo.commit_in_flight > 0);
        let commit_message_text = self.commit_message_input.read(cx).text().to_string();
        let can_submit_commit = Self::can_submit_commit(
            self.active_repo(),
            &commit_message_text,
            self.commit_amend_enabled,
        );
        let repo_key = self.active_repo_id().map(|id| id.0).unwrap_or(0);
        let icon_color = theme.colors.accent.foreground;
        let icon = |path: &'static str| svg_icon(path, icon_color, px(14.0));
        let spinner = |id: (&'static str, u64)| svg_spinner(id, icon_color, px(14.0));
        let commit_label = match (self.commit_amend_enabled, self.commit_push_after_enabled) {
            (false, false) => tr_str("layout.commit_box.commit"),
            (false, true) => tr_str("layout.commit_box.commit_and_push"),
            (true, false) => tr_str("layout.commit_box.amend_previous"),
            (true, true) => tr_str("layout.commit_box.amend_and_push"),
        };
        let commit_tooltip = match (self.commit_amend_enabled, self.commit_push_after_enabled) {
            (false, false) => tr_str("layout.commit_box.tooltip_commit"),
            (false, true) => tr_str("layout.commit_box.tooltip_commit_and_push"),
            (true, false) => tr_str("layout.commit_box.tooltip_amend"),
            (true, true) => tr_str("layout.commit_box.tooltip_amend_and_push"),
        };
        let commit_options_invoker: SharedString = "commit_options".into();
        let commit_options_active = self
            .active_context_menu_invoker
            .as_ref()
            .is_some_and(|id| id.as_ref() == commit_options_invoker.as_ref());
        let previous_messages_invoker: SharedString = "previous_commit_messages".into();
        let ai_generating = self.ai_commit_generation.is_some();
        let ai_generating_this_repo = self
            .ai_commit_generation
            .as_ref()
            .is_some_and(|generation| self.active_repo_id() == Some(generation.repo_id()));
        let ai_icon_color = if ai_generating_this_repo {
            theme.colors.accent.foreground
        } else {
            theme.colors.foreground.secondary
        };
        let previous_messages_active = self
            .active_context_menu_invoker
            .as_ref()
            .is_some_and(|id| id.as_ref() == previous_messages_invoker.as_ref());
        let menu_selected_bg = with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.26 } else { 0.20 },
        );
        let menu_icon_color = if commit_options_active {
            theme.colors.accent.foreground
        } else {
            theme.colors.foreground.secondary
        };
        let previous_messages_icon_color = if previous_messages_active {
            theme.colors.accent.foreground
        } else {
            theme.colors.foreground.secondary
        };
        let commit_message = components::ScrollContainer::vertical(
            ("commit_message_scroll_surface", repo_key),
            ("commit_message_scrollbar", repo_key),
            self.commit_message_scroll.clone(),
            px(COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX),
        )
        .container_id(("commit_message_container", repo_key))
        .render(theme, self.commit_message_input.clone());
        let commit_main = components::Button::new("commit", commit_label)
            .rounded_left()
            .start_slot(if commit_in_flight {
                spinner(("commit_spinner", repo_key)).into_any_element()
            } else {
                icon("icons/check.svg").into_any_element()
            })
            .style(components::ButtonStyle::Subtle)
            .disabled(!can_submit_commit)
            .on_click(theme, cx, |this, _e, _w, cx| {
                let _ = this.submit_commit(cx);
            })
            .debug_selector(|| "commit_button".to_string())
            .worktree_tooltip(theme, commit_tooltip.into());
        let commit_menu = components::Button::new("commit_options", "")
            .rounded_right()
            .start_slot(svg_icon(
                "icons/chevron_down.svg",
                menu_icon_color,
                px(14.0),
            ))
            .style(components::ButtonStyle::Subtle)
            .selected(commit_options_active)
            .selected_bg(menu_selected_bg)
            .disabled(self.active_repo_id().is_none())
            .on_click(theme, cx, move |this, e, window, cx| {
                let Some(repo_id) = this.active_repo_id() else {
                    return;
                };
                this.activate_context_menu_invoker(commit_options_invoker.clone(), cx);
                this.open_popover_at(
                    PopoverKind::CommitOptionsMenu { repo_id },
                    e.position(),
                    window,
                    cx,
                );
            })
            .worktree_tooltip(theme, tr("layout.commit_box.options"));
        let previous_messages_menu = components::Button::new("previous_commit_messages", "")
            .start_slot(svg_icon(
                "icons/history.svg",
                previous_messages_icon_color,
                px(14.0),
            ))
            .style(components::ButtonStyle::Subtle)
            .selected(previous_messages_active)
            .selected_bg(menu_selected_bg)
            .disabled(self.active_repo_id().is_none())
            .on_click(theme, cx, move |this, e, window, cx| {
                let Some(repo_id) = this.active_repo_id() else {
                    return;
                };
                this.activate_context_menu_invoker(previous_messages_invoker.clone(), cx);
                this.open_popover_at(
                    PopoverKind::PreviousCommitMessagesMenu { repo_id },
                    e.position(),
                    window,
                    cx,
                );
            })
            .debug_selector(|| "previous_commit_messages_button".to_string())
            .worktree_tooltip(theme, tr("layout.commit_box.previous_messages"));
        let ai_generate = components::Button::new("ai_commit_generate", "")
            .start_slot(if ai_generating_this_repo {
                spinner(("ai_commit_spinner", repo_key)).into_any_element()
            } else {
                svg_icon("icons/sparkle.svg", ai_icon_color, px(14.0)).into_any_element()
            })
            .style(components::ButtonStyle::Subtle)
            .disabled(self.active_repo_id().is_none() || ai_generating)
            .on_click(theme, cx, |this, _e, _w, cx| {
                this.start_ai_commit_message_generation(cx);
            })
            .debug_selector(|| "ai_commit_generate_button".to_string())
            .worktree_tooltip(
                theme,
                tr(if ai_generating_this_repo {
                    "layout.commit_box.ai_generating"
                } else {
                    "layout.commit_box.ai_generate"
                }),
            );
        div().flex().flex_col().gap_2().child(commit_message).child(
            div().flex().items_center().justify_end().child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ai_generate)
                    .child(previous_messages_menu)
                    .child(
                        components::SplitButton::new(commit_main, commit_menu)
                            .style(components::SplitButtonStyle::Filled)
                            .render(theme, ui_scale_percent)
                            .debug_selector(|| "commit_split_button".to_string()),
                    ),
            ),
        )
    }
}
