//! Facade for the main pane's core `impl` surface: the constructor, the state
//! snapshot plumbing and the members the six domain modules hang off. The
//! resolved-output editor lives in `resolved_output_syntax`, wrap projection in
//! `diff_wrap`, scroll sync in `scroll_sync`, target queries in `target_query`,
//! setters in `settings` and the popover/context-menu entry points in
//! `context_menus`.
use super::helpers::{
    ClearDiffSelectionAction, DiffHorizontalScrollState,
    FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES, FileDiffStyleCacheEpochs, FocusedMergetoolOutput,
    ICommitEditorMode, IRebaseViewState, ResolvedOutputSourceRevision,
    apply_focused_mergetool_output, build_focused_mergetool_save_payload,
    clear_diff_selection_action, coalesce_resolved_output_edit_deltas,
    conflict_canvas_rows_enabled_from_env, focused_mergetool_save_exit_code,
    resolved_outline_delta_for_snapshot_transition, resolved_output_snapshot_is_modified,
    versioned_cached_diff_styled_text_is_current,
};
use super::*;
use crate::kit::text_model::TextModelSnapshot;
use crate::view::branch_sidebar::BranchSection;
use crate::view::panes::PaneChromeExt as _;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::sync::Arc;
use worktree_core::domain::LogScope;

// `set_theme` measures the resolved output against its widest row, and the
// conflict_actions module reaches the uniform-list base handle through this
// facade, so both domain items stay nameable from the core_impl root.
use self::resolved_output_syntax::resolved_output_measure_row;
pub(in crate::view::panes::main) use self::scroll_sync::uniform_list_base_handle;

// The in-file tests exercise the domain modules' free functions through these
// named imports (they are private to their domains otherwise).
#[cfg(test)]
use self::diff_wrap::diff_wrap_byte_ranges_for_text;
#[cfg(test)]
use self::scroll_sync::{
    clamp_raw_scroll_y, compute_synced_scroll_offsets, compute_synced_scroll_offsets_with_master,
};
#[cfg(test)]
use self::target_query::should_request_blame;

mod context_menus;
mod diff_wrap;
mod resolved_output_syntax;
mod scroll_sync;
mod settings;
mod target_query;

impl MainPaneView {
    pub(in crate::view) fn sync_interactive_commit_editor_states(&mut self) {
        let repos_with_setup: Vec<RepoId> = self
            .state
            .repos
            .iter()
            .filter(|r| {
                r.interactive_rebase_setup.is_some() || r.interactive_cherry_pick_setup.is_some()
            })
            .map(|r| r.id)
            .collect();
        self.interactive_rebase_states
            .retain(|repo_id, _| repos_with_setup.contains(repo_id));
        for repo in self.state.repos.iter() {
            if let Some(setup) = repo.interactive_rebase_setup.as_ref() {
                let Loadable::Ready(entries) = &setup.entries else {
                    continue;
                };
                let replace = self
                    .interactive_rebase_states
                    .get(&repo.id)
                    .is_none_or(|st| {
                        st.mode != ICommitEditorMode::Rebase || st.original_entries != *entries
                    });
                if replace {
                    self.interactive_rebase_states.insert(
                        repo.id,
                        IRebaseViewState {
                            mode: ICommitEditorMode::Rebase,
                            entries: entries.clone(),
                            original_entries: entries.clone(),
                            ..Default::default()
                        },
                    );
                }
            } else if let Some(setup) = repo.interactive_cherry_pick_setup.as_ref() {
                if !matches!(setup.full_messages, Loadable::Ready(())) {
                    // Do not retain subject-only view-local entries from this
                    // or a replaced setup while full messages are pending.
                    self.interactive_rebase_states.remove(&repo.id);
                    continue;
                }
                let source_colors = setup
                    .source_colors
                    .iter()
                    .cloned()
                    .collect::<FxHashMap<_, _>>();
                // A repeated state application for the same setup must not
                // replace view-local reordering or action edits. A different
                // id set is a genuinely new setup.
                let same_plan =
                    self.interactive_rebase_states.get(&repo.id).is_some_and(
                        |st: &IRebaseViewState| {
                            st.mode == ICommitEditorMode::CherryPick
                                && st.original_entries.len() == setup.entries.len()
                                && st.original_entries.iter().zip(setup.entries.iter()).all(
                                    |(current, incoming)| current.commit_id == incoming.commit_id,
                                )
                        },
                    );
                if same_plan {
                    let st = self
                        .interactive_rebase_states
                        .get_mut(&repo.id)
                        .expect("same_plan implies the state exists");
                    for (current, incoming) in
                        st.original_entries.iter_mut().zip(setup.entries.iter())
                    {
                        current.message = incoming.message.clone();
                    }
                    for entry in st.entries.iter_mut() {
                        if let Some(incoming) = setup
                            .entries
                            .iter()
                            .find(|incoming| incoming.commit_id == entry.commit_id)
                        {
                            entry.message = incoming.message.clone();
                        }
                    }
                } else {
                    self.interactive_rebase_states.insert(
                        repo.id,
                        IRebaseViewState {
                            mode: ICommitEditorMode::CherryPick,
                            entries: setup.entries.clone(),
                            original_entries: setup.entries.clone(),
                            source_colors,
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }

    pub(super) fn notify_fingerprint_for(state: &AppState) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = FxHasher::default();
        state.active_repo.hash(&mut hasher);

        if let Some(repo_id) = state.active_repo
            && let Some(repo) = state.repos.iter().find(|r| r.id == repo_id)
        {
            match repo.diff_state.diff_target.as_ref() {
                Some(DiffTarget::WorkingTree { path, area }) => {
                    0u8.hash(&mut hasher);
                    path.hash(&mut hasher);
                    match area {
                        DiffArea::Staged => 0u8.hash(&mut hasher),
                        DiffArea::Unstaged => 1u8.hash(&mut hasher),
                    }
                }
                Some(DiffTarget::Commit { commit_id, path }) => {
                    1u8.hash(&mut hasher);
                    commit_id.hash(&mut hasher);
                    path.hash(&mut hasher);
                }
                Some(DiffTarget::CommitRange {
                    from_commit_id,
                    to_commit_id,
                    path,
                }) => {
                    2u8.hash(&mut hasher);
                    from_commit_id.hash(&mut hasher);
                    to_commit_id.hash(&mut hasher);
                    path.hash(&mut hasher);
                }
                None => {
                    3u8.hash(&mut hasher);
                }
            }
            repo.diff_state.diff_state_rev.hash(&mut hasher);
            // The historical-browse tint keys off content-preview mode, which can
            // share a diff_target with a plain diff of the same commit+path.
            repo.diff_state.content_preview.hash(&mut hasher);
            // Entering or leaving the editor swaps the whole content body and
            // the toolbar; without this the pane would not re-render for it.
            repo.diff_state.edit_mode.hash(&mut hasher);
            repo.conflict_state.conflict_rev.hash(&mut hasher);

            // Only include status changes when viewing a working tree diff.
            let status_rev = if matches!(
                repo.diff_state.diff_target,
                Some(DiffTarget::WorkingTree { .. })
            ) {
                repo.status_cache_rev()
            } else {
                0
            };
            status_rev.hash(&mut hasher);
            let commit_details_rev = if matches!(
                repo.diff_state.diff_target,
                Some(DiffTarget::Commit { path: Some(_), .. })
            ) {
                repo.history_state.commit_details_rev
            } else {
                0
            };
            commit_details_rev.hash(&mut hasher);
            // The historical-browse tint keys off the file browser source.
            repo.file_browser.file_browser_rev.hash(&mut hasher);

            match &repo.interactive_rebase_setup {
                Some(setup) => {
                    1u8.hash(&mut hasher);
                    setup.base.hash(&mut hasher);
                    match &setup.entries {
                        Loadable::NotLoaded => 0u8.hash(&mut hasher),
                        Loadable::Loading => 1u8.hash(&mut hasher),
                        Loadable::Ready(_) => 2u8.hash(&mut hasher),
                        Loadable::Error(err) => {
                            3u8.hash(&mut hasher);
                            err.hash(&mut hasher);
                        }
                    }
                }
                None => {
                    0u8.hash(&mut hasher);
                }
            }
            match &repo.interactive_cherry_pick_setup {
                Some(setup) => {
                    1u8.hash(&mut hasher);
                    setup.entries.len().hash(&mut hasher);
                    for entry in &setup.entries {
                        entry.commit_id.hash(&mut hasher);
                        entry.summary.hash(&mut hasher);
                    }
                    setup.source_colors.hash(&mut hasher);
                    match &setup.full_messages {
                        Loadable::NotLoaded => 0u8.hash(&mut hasher),
                        Loadable::Loading => 1u8.hash(&mut hasher),
                        Loadable::Ready(()) => 2u8.hash(&mut hasher),
                        Loadable::Error(error) => {
                            3u8.hash(&mut hasher);
                            error.hash(&mut hasher);
                        }
                    }
                }
                None => 0u8.hash(&mut hasher),
            }
            // Blame/annotate data — when blame loads for the first time or changes
            // target, the annotation sidebar needs to repaint.
            repo.history_state.blame_path.hash(&mut hasher);
            repo.history_state.blame_source.hash(&mut hasher);
            matches!(
                &repo.history_state.blame,
                worktree_state::model::Loadable::Ready(_)
            )
            .hash(&mut hasher);
        }

        hasher.finish()
    }

    pub(in crate::view) fn clear_diff_selection_or_exit(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        match clear_diff_selection_action(self.view_mode) {
            ClearDiffSelectionAction::ClearSelection => {
                self.store.dispatch(Msg::ClearDiffSelection { repo_id });
            }
            ClearDiffSelectionAction::ExitFocusedMergetool => {
                self.set_focused_mergetool_exit_code(FOCUSED_MERGETOOL_EXIT_CANCELED);
                cx.quit();
            }
        }
    }

    pub(in crate::view) fn reveal_history_commit(
        &mut self,
        repo_id: RepoId,
        commit_id: CommitId,
        fallback_scope: Option<LogScope>,
        cx: &mut gpui::Context<Self>,
    ) {
        if matches!(
            clear_diff_selection_action(self.view_mode),
            ClearDiffSelectionAction::ExitFocusedMergetool
        ) {
            self.clear_diff_selection_or_exit(repo_id, cx);
            return;
        }

        self.clear_diff_selection_or_exit(repo_id, cx);
        // Resolve and show the commit immediately; the history walk below only
        // has to find its row. Without this the details pane would sit on the
        // working tree — or flip in and out of it — for the whole walk.
        self.store.dispatch(Msg::RevealCommit {
            repo_id,
            reference: commit_id.clone(),
        });
        self.history_view.update(cx, |view, cx| {
            view.request_reveal_commit(repo_id, commit_id, fallback_scope, cx);
        });
        cx.notify();
    }

    pub(in crate::view) fn reveal_history_worktree(
        &mut self,
        repo_id: RepoId,
        worktree_path: std::path::PathBuf,
        is_current: bool,
        head: Option<CommitId>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.history_view.update(cx, |view, cx| {
            view.reveal_worktree(repo_id, worktree_path, is_current, head, cx);
        });
    }

    pub(in crate::view) fn reveal_history_branch_commit(
        &mut self,
        repo_id: RepoId,
        section: BranchSection,
        branch_name: &str,
        commit_id: CommitId,
        fallback_scope: Option<LogScope>,
        cx: &mut gpui::Context<Self>,
    ) {
        let branch_name = branch_name.to_string();
        self.history_view.update(cx, |view, cx| {
            view.set_selected_branch(repo_id, section, &branch_name, cx);
        });
        self.reveal_history_commit(repo_id, commit_id, fallback_scope, cx);
    }

    pub(super) fn set_focused_mergetool_exit_code(&self, code: i32) {
        if let Some(exit_code) = &self.focused_mergetool_exit_code {
            exit_code.store(code, Ordering::SeqCst);
        }
    }

    pub(super) fn focused_mergetool_labels_or_default(&self) -> FocusedMergetoolLabels {
        self.focused_mergetool_labels
            .clone()
            .unwrap_or(FocusedMergetoolLabels {
                local: "LOCAL".to_string(),
                remote: "REMOTE".to_string(),
                base: "BASE".to_string(),
            })
    }

    pub(in crate::view) fn focused_mergetool_save_and_exit(
        &mut self,
        repo_id: RepoId,
        path: std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        use worktree_core::conflict_output::ConflictMarkerLabels;

        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            self.set_focused_mergetool_exit_code(FOCUSED_MERGETOOL_EXIT_ERROR);
            cx.quit();
            return;
        };

        let labels = self.focused_mergetool_labels_or_default();
        let materialized_output = (!self.conflict_resolved_output_is_streamed()).then(|| {
            self.conflict_resolver_input
                .read_with(cx, |input, _| input.text().to_string())
        });
        let save_payload = build_focused_mergetool_save_payload(
            &self.conflict_resolver.marker_segments,
            &self.conflict_resolver.conflict_region_indices,
            &self.conflict_resolved_output_block_map,
            materialized_output.as_deref(),
            ConflictMarkerLabels {
                local: labels.local.as_str(),
                remote: labels.remote.as_str(),
                base: labels.base.as_str(),
            },
        );
        if save_payload.total_conflicts != save_payload.resolved_conflicts
            || conflict_resolver::text_contains_conflict_markers(&save_payload.output)
        {
            cx.notify();
            return;
        }
        let output = save_payload.output;
        let exit_code = focused_mergetool_save_exit_code(
            save_payload.total_conflicts,
            save_payload.resolved_conflicts,
        );
        let full_path = repo.spec.workdir.join(&path);
        self.finish_focused_mergetool_output(
            &full_path,
            FocusedMergetoolOutput::Write(output.as_bytes()),
            exit_code,
            cx,
        );
    }

    pub(in crate::view) fn focused_mergetool_write_side_and_exit(
        &self,
        repo_id: RepoId,
        path: &std::path::Path,
        bytes: &[u8],
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            self.set_focused_mergetool_exit_code(FOCUSED_MERGETOOL_EXIT_ERROR);
            cx.quit();
            return;
        };
        let full_path = repo.spec.workdir.join(path);
        self.finish_focused_mergetool_output(
            &full_path,
            FocusedMergetoolOutput::Write(bytes),
            FOCUSED_MERGETOOL_EXIT_SUCCESS,
            cx,
        );
    }

    pub(in crate::view) fn focused_mergetool_delete_and_exit(
        &self,
        repo_id: RepoId,
        path: &std::path::Path,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            self.set_focused_mergetool_exit_code(FOCUSED_MERGETOOL_EXIT_ERROR);
            cx.quit();
            return;
        };
        let full_path = repo.spec.workdir.join(path);
        self.finish_focused_mergetool_output(
            &full_path,
            FocusedMergetoolOutput::Delete,
            FOCUSED_MERGETOOL_EXIT_SUCCESS,
            cx,
        );
    }

    fn finish_focused_mergetool_output(
        &self,
        path: &std::path::Path,
        output: FocusedMergetoolOutput<'_>,
        success_exit_code: i32,
        cx: &mut gpui::Context<Self>,
    ) {
        match apply_focused_mergetool_output(path, output) {
            Ok(()) => self.set_focused_mergetool_exit_code(success_exit_code),
            Err(err) => {
                let operation = match output {
                    FocusedMergetoolOutput::Write(_) => "write merged output to",
                    FocusedMergetoolOutput::Delete => "delete merged output",
                };
                eprintln!("Failed to {operation} {}: {err}", path.display());
                self.set_focused_mergetool_exit_code(FOCUSED_MERGETOOL_EXIT_ERROR);
            }
        }
        cx.quit();
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::view) fn new(
        store: Arc<AppStore>,
        ui_model: Entity<AppUiModel>,
        theme: AppTheme,
        date_time_format: DateTimeFormat,
        timezone: Timezone,
        show_timezone: bool,
        history_relative_dates: bool,
        history_highlight_commit_chain: bool,
        diff_scroll_sync: DiffScrollSync,
        diff_content_mode: DiffContentMode,
        diff_whitespace_mode: DiffWhitespaceMode,
        diff_view_mode: DiffViewMode,
        annotate_enabled: bool,
        diff_reveal_whitespace_chars: bool,
        diff_word_wrap: bool,
        diff_show_line_numbers: bool,
        auto_save_file_edits: bool,
        history_show_graph: bool,
        history_show_author: bool,
        history_show_date: bool,
        history_show_sha: bool,
        history_show_tags: bool,
        history_auto_fetch_tags_on_repo_activation: bool,
        view_mode: WorkTreeViewMode,
        focused_mergetool_labels: Option<FocusedMergetoolLabels>,
        focused_mergetool_exit_code: Option<Arc<AtomicI32>>,
        root_view: WeakEntity<WorkTreeView>,
        tooltip_host: WeakEntity<TooltipHost>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let state = Arc::clone(&ui_model.read(cx).state);
        let initial_fingerprint = Self::notify_fingerprint_for(&state);
        let subscription = cx.observe(&ui_model, |this, model, cx| {
            let next = Arc::clone(&model.read(cx).state);
            let next_fingerprint = Self::notify_fingerprint_for(&next);
            if next_fingerprint == this.notify_fingerprint {
                this.state = next;
                return;
            }

            this.notify_fingerprint = next_fingerprint;
            this.apply_state_snapshot(next, cx);
            cx.notify();
        });

        let diff_raw_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    multiline: true,
                    read_only: true,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        let submodule_hash_inputs = (0..4)
            .map(|_| {
                cx.new(|cx| {
                    let mut input = components::TextInput::new(
                        components::TextInputOptions {
                            read_only: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    input.set_read_only(true, cx);
                    input
                })
            })
            .collect::<Vec<_>>();

        let conflict_resolver_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "Resolve file contents…".into(),
                    multiline: true,
                    chromeless: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_suppress_right_click(true);
            input.set_line_height(
                Some(ui_scale::design_px_from_percent(
                    20.0,
                    ui_scale::current(cx).percent,
                )),
                cx,
            );
            input
        });

        let conflict_resolver_subscription =
            cx.observe(&conflict_resolver_input, |this, input, cx| {
                let _perf_scope = crate::view::perf::span(
                    crate::view::perf::ViewPerfSpan::ResolvedOutputEditObserve,
                );
                let (output_snapshot, edit_deltas) = input.update(cx, |input, _| {
                    (input.text_snapshot(), input.drain_recent_utf8_edit_deltas())
                });
                let outline_edit_delta = (edit_deltas.len() == 1)
                    .then(|| edit_deltas.first().cloned())
                    .flatten();
                // Fold the tree forward before anything else looks at the
                // buffer, so the very next frame paints from a tree that
                // already describes what was just typed.
                let syntax_edit = coalesce_resolved_output_edit_deltas(&edit_deltas);
                this.apply_conflict_resolved_output_edit_deltas(
                    edit_deltas,
                    &output_snapshot.rope(),
                );
                if !this.conflict_resolved_output_is_streamed() {
                    this.refresh_conflict_resolved_output_syntax(&output_snapshot, syntax_edit, cx);
                }
                let source_revision = ResolvedOutputSourceRevision::from_snapshot(&output_snapshot);
                let output_modified = resolved_output_snapshot_is_modified(
                    this.conflict_resolved_output_saved_snapshot.as_ref(),
                    &output_snapshot,
                );
                if this.conflict_resolved_output_modified != output_modified {
                    this.conflict_resolved_output_modified = output_modified;
                    cx.notify();
                }
                let outline_delta = resolved_outline_delta_for_snapshot_transition(
                    &this.conflict_resolved_preview_text,
                    &output_snapshot,
                    outline_edit_delta,
                );

                let path = this.conflict_resolver.path.clone();
                let needs_update = this.conflict_resolved_preview_path.as_ref() != path.as_ref()
                    || this.conflict_resolved_preview_source_revision != Some(source_revision);
                if !needs_update {
                    return;
                }

                this.conflict_resolved_preview_path = path.clone();
                this.conflict_resolved_preview_source_revision = Some(source_revision);
                this.schedule_conflict_resolved_outline_recompute(
                    path,
                    source_revision,
                    outline_delta,
                    cx,
                );
                // The Save gates derive effective resolutions from the live
                // editor text, so the containing toolbar must re-render for
                // every edit even while session state remains deferred.
                cx.notify();
            });

        let file_editor_scroll = ScrollHandle::new();
        let file_editor_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    multiline: true,
                    chromeless: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            // The input lays out at its content width inside an
            // `overflow_scroll` container, which is what gives that container a
            // horizontal extent to scroll — the same arrangement the resolved
            // output uses.
            input.set_content_width_layout(true);
            input.set_vertical_scroll_handle(Some(file_editor_scroll.clone()));
            input.set_line_height(
                Some(ui_scale::design_px_from_percent(
                    20.0,
                    ui_scale::current(cx).percent,
                )),
                cx,
            );
            input
        });
        let file_editor_subscription = cx.observe(&file_editor_input, |this, _input, cx| {
            this.on_file_editor_edited(cx);
        });

        let diff_search_scroll = ScrollHandle::new();
        let diff_search_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "Search diff".into(),
                    multiline: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_submit_on_enter(true);
            input.set_vertical_scroll_handle(Some(diff_search_scroll.clone()));
            input.set_vertical_padding(Some(px(4.0)), cx);
            input.set_line_height(
                Some(ui_scale::design_px_from_percent(
                    18.0,
                    ui_scale::current(cx).percent,
                )),
                cx,
            );
            input
        });
        let diff_search_subscription = cx.observe(&diff_search_input, |this, input, cx| {
            if input.update(cx, |input, _| input.take_enter_pressed()) {
                if this.diff_search_active {
                    this.diff_search_next_match();
                    cx.notify();
                }
                return;
            }
            let next: SharedString = input.read(cx).text().to_string().into();
            if this.diff_search_query != next {
                let previous_query = this.diff_search_query.clone();
                this.diff_search_query = next.clone();
                if next.is_empty() {
                    this.diff_search_scroll.set_offset(point(px(0.0), px(0.0)));
                }
                this.invalidate_diff_text_query_overlay_cache(
                    next.as_ref(),
                    this.diff_search_options,
                );
                this.clear_worktree_preview_segments_cache();
                this.clear_conflict_diff_query_overlay_caches();
                if next.is_empty() {
                    this.diff_search_cancel_pending_query_recompute();
                    this.diff_search_recompute_matches_for_query_change(previous_query.as_ref());
                } else {
                    this.diff_search_schedule_query_recompute(previous_query, cx);
                }
                cx.notify();
            }
        });

        let diff_panel_focus_handle = cx.focus_handle().tab_index(0).tab_stop(false);

        let last_window_size = window.viewport_size();
        let ui_scale_percent = ui_scale::current(cx).percent;
        let history_view = cx.new(|cx| {
            super::HistoryView::new(
                Arc::clone(&store),
                ui_model.clone(),
                theme,
                ui_scale_percent,
                date_time_format,
                timezone,
                show_timezone,
                history_relative_dates,
                history_highlight_commit_chain,
                history_show_graph,
                history_show_author,
                history_show_date,
                history_show_sha,
                history_show_tags,
                history_auto_fetch_tags_on_repo_activation,
                root_view.clone(),
                last_window_size,
                window,
                cx,
            )
        });

        let mut pane = Self {
            store,
            state,
            view_mode,
            focused_mergetool_labels,
            focused_mergetool_exit_code,
            theme,
            date_time_format,
            _ui_model_subscription: subscription,
            root_view,
            tooltip_host,
            notify_fingerprint: initial_fingerprint,
            active_context_menu_invoker: None,
            last_window_size: size(px(0.0), px(0.0)),
            layout_sidebar_render_width: px(280.0),
            layout_details_render_width: px(420.0),
            layout_sidebar_collapsed: false,
            layout_details_collapsed: false,
            reveal_whitespace_chars: diff_reveal_whitespace_chars,
            mergetool_auto_advance: true,
            mergetool_collapse_unchanged: false,
            mergetool_output_scroll_sync: true,
            mergetool_show_line_numbers: true,
            mergetool_view_three_way: true,
            diff_view: diff_view_mode,
            annotate_enabled,
            annotate_column_width: rows::DIFF_ANNOTATION_COLUMN_WIDTH_PX,
            annotate_resize: None,
            blame_annot_hover: None,
            diff_stage_gutter_hover: None,
            diff_stage_gutter_cells: FxHashMap::default(),
            blame_time_range_cache: None,
            rendered_preview_modes: RenderedPreviewModes::default(),
            diff_word_wrap,
            diff_show_line_numbers,
            diff_scroll_sync,
            diff_content_mode,
            diff_whitespace_mode,
            diff_split_ratio: 0.5,
            diff_split_resize: None,
            diff_split_last_synced_x: [px(0.0); 2],
            diff_split_last_synced_y: [px(0.0); 2],
            diff_horizontal_scroll: DiffHorizontalScrollState::new(),
            diff_cache_repo_id: None,
            diff_cache_rev: 0,
            diff_cache_content_signature: None,
            diff_cache_target: None,
            diff_cache: Vec::new(),
            diff_row_provider: None,
            diff_split_row_provider: None,
            diff_file_for_src_ix: Vec::new(),
            diff_language_for_src_ix: Vec::new(),
            diff_yaml_block_scalar_for_src_ix: Vec::new(),
            diff_click_kinds: Vec::new(),
            diff_line_kind_for_src_ix: Vec::new(),
            diff_visual_line_kind_for_src_ix: Vec::new(),
            diff_hide_unified_header_for_src_ix: Vec::new(),
            diff_header_display_cache: FxHashMap::default(),
            diff_split_cache: Vec::new(),
            diff_split_cache_len: 0,
            diff_panel_focus_handle,
            diff_autoscroll_pending: false,
            diff_raw_input,
            submodule_hash_inputs,
            diff_visible_indices: Vec::new(),
            diff_visible_inline_map: None,
            diff_wrap_visible_rows: Vec::new(),
            diff_wrap_visible_cache_key: None,
            collapsed_diff_hunks: Vec::new(),
            collapsed_diff_hunk_ix_by_src_ix: FxHashMap::default(),
            collapsed_diff_reveals: FxHashMap::default(),
            collapsed_diff_visible_rows: Vec::new(),
            collapsed_diff_hunk_visible_indices: Vec::new(),
            collapsed_diff_header_display_cache: FxHashMap::default(),
            collapsed_diff_projection_identity: None,
            diff_visible_cache_len: 0,
            diff_visible_view: DiffViewMode::Split,
            diff_visible_is_file_view: false,
            diff_visible_projection_rev: 0,
            diff_visible_cache_projection_rev: u64::MAX,
            diff_scrollbar_markers_cache: Vec::new(),
            diff_word_highlights: Vec::new(),
            diff_word_highlights_inflight: None,
            diff_file_stats: Vec::new(),
            diff_text_segments_cache: Vec::new(),
            diff_text_query_segments_cache: Vec::new(),
            diff_text_query_cache_query: SharedString::default(),
            diff_text_query_cache_options: Default::default(),
            diff_text_query_cache_matcher: None,
            diff_text_query_cache_generation: 0,
            diff_selection_anchor: None,
            diff_selection_range: None,
            diff_text_selecting: false,
            diff_text_anchor: None,
            diff_text_head: None,
            diff_text_autoscroll_seq: 0,
            diff_text_autoscroll_target: None,
            diff_text_last_mouse_pos: point(px(0.0), px(0.0)),
            diff_suppress_clicks_remaining: 0,
            diff_text_hitboxes: FxHashMap::default(),
            diff_search_horizontal_reveal: None,
            conflict_text_hitboxes: FxHashMap::default(),
            diff_text_layout_cache_epoch: 0,
            diff_text_layout_cache: FxHashMap::default(),
            diff_search_active: false,
            diff_search_query: "".into(),
            diff_search_options: Default::default(),
            diff_search_regex_error: None,
            diff_search_matches: Vec::new(),
            diff_search_inline_patch_trigram_index: None,
            diff_search_match_ix: None,
            diff_search_debounce_seq: 0,
            diff_search_pending_previous_query: None,
            diff_search_scroll,
            diff_search_input,
            _diff_search_subscription: diff_search_subscription,
            file_diff_cache_repo_id: None,
            file_diff_cache_rev: 0,
            file_diff_cache_content_signature: None,
            file_diff_cache_whitespace_mode: diff_whitespace_mode,
            file_diff_cache_target: None,
            file_diff_cache_error: None,
            file_diff_cache_path: None,
            file_diff_cache_language: None,
            file_diff_cache_rows: Vec::new(),
            file_diff_row_provider: None,
            file_diff_old_text: SharedString::default(),
            file_diff_old_line_starts: Arc::default(),
            file_diff_old_line_to_row: Arc::default(),
            file_diff_old_line_to_inline_row: Arc::default(),
            file_diff_new_text: SharedString::default(),
            file_diff_new_line_starts: Arc::default(),
            file_diff_new_line_to_row: Arc::default(),
            file_diff_new_line_to_inline_row: Arc::default(),
            file_diff_inline_cache: Vec::new(),
            file_diff_inline_row_provider: None,
            file_diff_inline_text: SharedString::default(),
            file_diff_inline_word_highlights: rows::new_lru_cache(
                FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES,
            ),
            file_diff_split_word_highlights: rows::new_lru_cache(
                FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES,
            ),
            file_diff_cache_seq: 0,
            file_diff_cache_inflight: None,
            file_diff_syntax_generation: 0,
            file_diff_style_cache_epochs: FileDiffStyleCacheEpochs::default(),
            syntax_chunk_poll_task: None,
            prepared_syntax_documents: FxHashMap::default(),
            #[cfg(test)]
            diff_syntax_budget_override: None,
            file_markdown_preview_cache_repo_id: None,
            file_markdown_preview_cache_rev: 0,
            file_markdown_preview_cache_content_signature: None,
            file_markdown_preview_cache_target: None,
            file_markdown_preview: Loadable::NotLoaded,
            markdown_preview_wrap: MarkdownPreviewWrapCache::default(),
            markdown_preview_reveal: Default::default(),
            file_markdown_preview_seq: 0,
            file_markdown_preview_inflight: None,
            file_image_diff_cache_repo_id: None,
            file_image_diff_cache_rev: 0,
            file_image_diff_cache_content_signature: None,
            file_image_diff_cache_target: None,
            file_image_diff_cache_seq: 0,
            file_image_diff_cache_inflight: None,
            file_image_diff_cache_path: None,
            file_image_diff_cache_old: None,
            file_image_diff_cache_new: None,
            file_image_diff_cache_old_svg_path: None,
            file_image_diff_cache_new_svg_path: None,
            worktree_preview_path: None,
            worktree_preview_source_path: None,
            worktree_preview: Loadable::NotLoaded,
            worktree_preview_source_len: 0,
            worktree_preview_text: SharedString::default(),
            worktree_preview_line_starts: Arc::default(),
            worktree_preview_line_flags: Arc::default(),
            worktree_preview_search_trigram_index: None,
            worktree_preview_content_rev: 0,
            worktree_markdown_preview_path: None,
            worktree_markdown_preview_source_rev: 0,
            worktree_markdown_preview: Loadable::NotLoaded,
            worktree_markdown_preview_picture_sizes: Default::default(),
            worktree_markdown_preview_block_scrolls: Default::default(),
            worktree_markdown_preview_blocks: Default::default(),
            worktree_markdown_preview_image_waits: FxHashSet::default(),
            worktree_markdown_preview_seq: 0,
            worktree_markdown_preview_inflight: None,
            worktree_preview_segments_cache_path: None,
            worktree_preview_syntax_language: None,
            worktree_preview_style_cache_epoch: 0,
            worktree_preview_cache_write_blocked_until_rev: None,
            worktree_preview_segments_cache: FxHashMap::default(),
            diff_preview_is_new_file: false,
            file_editor_input,
            _file_editor_input_subscription: file_editor_subscription,
            file_editor_key: None,
            file_editor_language: None,
            file_editor_loading: false,
            file_editor_loaded_status_rev: 0,
            file_editor_error: None,
            file_editor_dirty: false,
            file_editor_first_dirty_line: None,
            unsaved_file_edits_rev: 0,
            file_editor_saved_fingerprint: None,
            file_editor_stash: FxHashMap::default(),
            file_editor_autosave: None,
            file_editor_live_syntax: None,
            file_editor_live_syntax_source: None,
            file_editor_live_syntax_building: None,
            file_editor_live_syntax_build: None,
            file_editor_live_syntax_reparse: None,
            file_editor_bracket_match: None,
            file_editor_search_matches: Vec::new(),
            file_editor_search_source: None,
            file_editor_search_rev: 0,
            file_editor_search_applied_rev: 0,
            file_editor_search_reveal_rev: 0,
            file_editor_search_reveal_applied_rev: 0,
            file_editor_search_reveal_x_pending: false,
            file_editor_provider_theme_epoch: 1,
            file_editor_scroll,
            file_editor_gutter_scroll: UniformListScrollHandle::new(),
            file_editor_gutter_row_height: ui_scale::design_px_from_percent(
                RESOLVED_OUTPUT_ROW_HEIGHT_PX,
                ui_scale::current(cx).percent,
            ),
            conflict_resolved_gutter_row_height: ui_scale::design_px_from_percent(
                RESOLVED_OUTPUT_ROW_HEIGHT_PX,
                ui_scale::current(cx).percent,
            ),
            file_editor_blame: None,
            file_editor_blame_width: px(0.0),
            file_editor_wrap_row_starts: Vec::new(),
            auto_save_file_edits,
            conflict_resolver_input,
            _conflict_resolver_input_subscription: conflict_resolver_subscription,
            conflict_resolver: ConflictResolverUiState::default(),
            conflict_open_summary_toasted_files: FxHashSet::default(),
            conflict_resolver_vsplit_ratio: 0.6,
            conflict_resolver_vsplit_resize: None,
            conflict_three_way_col_ratios: [1.0 / 3.0, 2.0 / 3.0],
            conflict_three_way_col_widths: [px(0.0); 3],
            conflict_hsplit_resize: None,
            conflict_diff_split_ratio: 0.5,
            conflict_diff_split_resize: None,
            conflict_diff_split_col_widths: [px(0.0); 2],
            conflict_canvas_rows_enabled: conflict_canvas_rows_enabled_from_env(),
            conflict_diff_segments_cache_split:
                conflict_resolver::ConflictSplitStyledTextCache::default(),
            conflict_diff_query_segments_cache_split:
                conflict_resolver::ConflictSplitStyledTextCache::default(),
            conflict_diff_query_cache_query: SharedString::default(),
            conflict_diff_query_cache_options: Default::default(),
            conflict_three_way_segments_cache: FxHashMap::default(),
            conflict_three_way_query_segments_cache: FxHashMap::default(),
            conflict_three_way_prepared_syntax_documents: ThreeWaySides::default(),
            conflict_three_way_syntax_inflight: ThreeWaySides::default(),
            conflict_resolved_preview_path: None,
            conflict_resolved_preview_source_revision: None,
            conflict_resolved_output_saved_snapshot: None,
            conflict_resolved_output_modified: false,
            conflict_resolved_output_projection: None,
            conflict_resolved_output_block_map: conflict_resolver::ResolvedOutputBlockMap::default(
            ),
            conflict_resolved_preview_text: TextModelSnapshot::default(),
            conflict_resolved_preview_syntax_language: None,
            conflict_resolved_preview_line_count: 0,
            conflict_resolved_preview_line_starts: Arc::default(),
            conflict_resolved_output_live_syntax: None,
            conflict_resolved_output_live_syntax_reparse: None,
            conflict_resolved_output_live_syntax_source: None,
            conflict_resolved_output_provider_theme_epoch: 1,
            conflict_resolved_output_highlighted_conflict: None,
            conflict_resolved_output_unresolved_rows: None,
            #[cfg(test)]
            conflict_resolved_output_full_scans: 0,
            conflict_resolved_output_live_syntax_building: None,
            conflict_resolved_output_live_syntax_build: None,
            conflict_resolved_output_measure_row: 0,
            conflict_resolved_outline_stash: None,
            #[cfg(test)]
            conflict_resolved_outline_background_delay_override: None,
            history_view,
            diff_scroll: UniformListScrollHandle::default(),
            diff_split_right_scroll: UniformListScrollHandle::default(),
            conflict_resolver_diff_scroll: UniformListScrollHandle::default(),
            conflict_preview_ours_scroll: UniformListScrollHandle::default(),
            conflict_preview_theirs_scroll: UniformListScrollHandle::default(),
            conflict_preview_last_synced_x: [px(0.0); 4],
            conflict_preview_last_synced_y: [px(0.0); 4],
            conflict_preview_vertical_wheel_master: None,
            conflict_output_gutter_wheel_sync_pending: false,
            conflict_resolved_preview_scroll: UniformListScrollHandle::default(),
            conflict_resolved_output_editor_scroll: ScrollHandle::new(),
            conflict_resolved_preview_gutter_scroll: UniformListScrollHandle::default(),
            conflict_resolved_preview_gutter_last_synced_y: [px(0.0); 2],
            worktree_preview_scroll: UniformListScrollHandle::default(),
            path_display_cache: std::cell::RefCell::new(path_display::PathDisplayCache::default()),
            interactive_rebase_states: FxHashMap::default(),
        };

        pane.set_theme(theme, cx);
        pane.ensure_rendered_patch_diff_cache(cx);
        pane
    }

    pub(in crate::view) fn sync_root_layout_snapshot(&mut self, cx: &mut gpui::Context<Self>) {
        let fallback_sidebar = self.layout_sidebar_render_width;
        let fallback_details = self.layout_details_render_width;
        let fallback_sidebar_collapsed = self.layout_sidebar_collapsed;
        let fallback_details_collapsed = self.layout_details_collapsed;

        let (sidebar_w, details_w, sidebar_collapsed, details_collapsed) = self
            .root_view
            .read_with(cx, |root, _cx| {
                (
                    root.sidebar_render_width,
                    root.details_render_width,
                    root.sidebar_collapsed,
                    root.details_collapsed,
                )
            })
            .unwrap_or((
                fallback_sidebar,
                fallback_details,
                fallback_sidebar_collapsed,
                fallback_details_collapsed,
            ));

        self.layout_sidebar_render_width = sidebar_w;
        self.layout_details_render_width = details_w;
        self.layout_sidebar_collapsed = sidebar_collapsed;
        self.layout_details_collapsed = details_collapsed;
    }

    pub(in crate::view) fn set_theme(&mut self, theme: AppTheme, cx: &mut gpui::Context<Self>) {
        self.theme = theme;
        self.conflict_resolved_output_provider_theme_epoch = self
            .conflict_resolved_output_provider_theme_epoch
            .wrapping_add(1)
            .max(1);
        self.file_editor_provider_theme_epoch =
            self.file_editor_provider_theme_epoch.wrapping_add(1).max(1);
        self.clear_diff_text_style_caches();
        self.clear_worktree_preview_segments_cache();
        self.clear_conflict_diff_style_caches();
        self.conflict_three_way_segments_cache.clear();
        self.conflict_three_way_query_segments_cache.clear();
        self.diff_raw_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.diff_search_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.conflict_resolver_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.file_editor_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.rebind_file_editor_highlight_provider(cx);
        if self.conflict_resolved_output_is_streamed() {
            self.conflict_resolved_preview_syntax_language = self
                .conflict_resolved_preview_path
                .as_ref()
                .and_then(rows::diff_syntax_language_for_path);
            self.conflict_resolved_output_measure_row = self
                .conflict_resolved_output_projection
                .as_ref()
                .map(conflict_resolver::ResolvedOutputProjection::widest_line_ix)
                .unwrap_or(0);
        } else {
            let output_snapshot = self
                .conflict_resolver_input
                .read_with(cx, |input, _| input.text_snapshot());
            self.conflict_resolved_preview_line_starts = output_snapshot.shared_line_starts();
            self.conflict_resolved_preview_line_count = output_snapshot.line_count().max(1);
            self.conflict_resolved_output_measure_row =
                resolved_output_measure_row(&output_snapshot);
            self.refresh_conflict_resolved_output_syntax(&output_snapshot, None, cx);
        }
        self.history_view
            .update(cx, |view, cx| view.set_theme(theme, cx));
        cx.notify();
    }

    pub(in crate::view) fn apply_ui_scale_percent(
        &mut self,
        previous_percent: u32,
        next_percent: u32,
        cx: &mut gpui::Context<Self>,
    ) {
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_line_height(
                Some(ui_scale::design_px_from_percent(20.0, next_percent)),
                cx,
            );
        });
        // The editor's gutter sizes its rows from the same scale, so leaving
        // the buffer at the old line height would put the numbers out of step
        // with the code they label.
        self.file_editor_input.update(cx, |input, cx| {
            input.set_line_height(
                Some(ui_scale::design_px_from_percent(20.0, next_percent)),
                cx,
            );
        });
        self.history_view.update(cx, |view, cx| {
            view.apply_ui_scale_percent(previous_percent, next_percent, cx);
        });
        cx.notify();
    }

    pub(in crate::view) fn invalidate_font_metrics(&mut self, cx: &mut gpui::Context<Self>) {
        self.diff_text_hitboxes.clear();
        self.diff_stage_gutter_cells.clear();
        self.diff_text_layout_cache_epoch = self.diff_text_layout_cache_epoch.wrapping_add(1);
        self.diff_text_layout_cache.clear();
        cx.notify();
    }

    pub(in crate::view) fn reset_diff_horizontal_scroll_state(&mut self) {
        self.diff_horizontal_scroll.reset();
        // A reveal names a row in the view it was armed over. That view is gone,
        // so the request must go with it rather than fire against whatever row
        // now holds that index.
        self.diff_search_horizontal_reveal = None;
        self.markdown_preview_reveal.clear();
    }

    pub(in crate::view) fn diff_horizontal_content_width(&self) -> Pixels {
        self.diff_horizontal_content_width_for_column(DiffHorizontalScrollColumn::Primary)
    }

    pub(in crate::view) fn diff_horizontal_content_width_for_column(
        &self,
        column: DiffHorizontalScrollColumn,
    ) -> Pixels {
        self.diff_horizontal_scroll.content_widths[column.index()]
    }

    pub(in crate::view) fn diff_horizontal_layout_min_width(
        &self,
        column: DiffHorizontalScrollColumn,
    ) -> Pixels {
        self.diff_horizontal_content_width_for_column(column)
    }

    pub(in crate::view) fn record_diff_horizontal_content_width(
        &mut self,
        width: Pixels,
        cx: &mut gpui::Context<Self>,
    ) {
        self.record_diff_horizontal_content_width_for_column(
            DiffHorizontalScrollColumn::Primary,
            width,
            cx,
        );
    }

    pub(in crate::view) fn record_diff_horizontal_content_width_for_column(
        &mut self,
        column: DiffHorizontalScrollColumn,
        width: Pixels,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap {
            return;
        }

        if self
            .diff_horizontal_scroll
            .record_content_width(column, width)
        {
            cx.notify();
        }
    }

    pub(in crate::view) fn diff_vertical_scrollbar_gutter_for_column(
        &self,
        _column: DiffHorizontalScrollColumn,
        _handle: UniformListScrollHandle,
    ) -> Pixels {
        components::Scrollbar::gutter(components::ScrollbarAxis::Vertical)
    }

    #[cfg(test)]
    pub(in crate::view) fn diff_horizontal_scroll_max_offset_for_viewport(
        &self,
        column: DiffHorizontalScrollColumn,
        viewport_width: Pixels,
    ) -> Pixels {
        let viewport_width = viewport_width.max(px(0.0));
        let content_width = self.diff_horizontal_content_width_for_column(column);
        (content_width - viewport_width).max(px(0.0))
    }

    /// Record the hovered blame annotation sub-area and drive the shared tooltip
    /// host. `next` is the (row, area) now hovered, or `None` when leaving; the
    /// blame canvas repaints on `notify` and renders the accent highlight from
    /// this state. Callers gate this so it only runs when the hover changes.
    pub(in crate::view) fn update_blame_annot_hover(
        &mut self,
        next: Option<(usize, rows::AnnotArea)>,
        tooltip: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.blame_annot_hover == next {
            return;
        }
        self.blame_annot_hover = next;
        // Only a pointer on the button itself owns a stage tooltip; merely
        // hovering the row shows the button without one.
        let stage_hover_owns_tooltip = self
            .diff_stage_gutter_hover
            .is_some_and(|hover| hover.on_button);
        self.apply_diff_hover_tooltip(tooltip, stage_hover_owns_tooltip, cx);
        cx.notify();
    }

    /// Drop a stage-gutter hover whose button was not painted in the frame just
    /// gone. Called while `diff_stage_gutter_cells` still holds that frame's
    /// buttons, so an entry missing from it means the row no longer offers one
    /// and can no longer clear the hover itself. Without this the button and its
    /// tooltip stay pinned under a pointer that is over something else.
    pub(in crate::view) fn clear_diff_stage_gutter_hover_if_unpainted(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let unpainted = self.diff_stage_gutter_hover.is_some_and(|hover| {
            !self
                .diff_stage_gutter_cells
                .contains_key(&(hover.visible_ix, hover.slot))
        });
        if unpainted {
            self.update_diff_stage_gutter_hover(None, None, cx);
        }
    }

    /// Record the hovered stage/unstage gutter button and drive the shared
    /// tooltip host, mirroring [`Self::update_blame_annot_hover`]. The row canvas
    /// paints the button from this state (never from the live cursor), so it
    /// stays in step with the value folded into the canvas revision key.
    pub(in crate::view) fn update_diff_stage_gutter_hover(
        &mut self,
        next: Option<rows::DiffStageHover>,
        tooltip: Option<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_stage_gutter_hover == next {
            return;
        }
        self.diff_stage_gutter_hover = next;
        let blame_hover_owns_tooltip = self.blame_annot_hover.is_some();
        self.apply_diff_hover_tooltip(tooltip, blame_hover_owns_tooltip, cx);
        cx.notify();
    }

    /// Shared tooltip plumbing for the two diff-row hover systems (blame column
    /// and stage gutter). Both write to the same host, so a hover that is leaving
    /// must not clear a tooltip the other one just set: `other_hover_active` says
    /// whether the other system currently owns the tooltip.
    fn apply_diff_hover_tooltip(
        &mut self,
        tooltip: Option<SharedString>,
        other_hover_active: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(host) = self.tooltip_host.upgrade() else {
            return;
        };
        host.update(cx, |host, cx| match tooltip {
            Some(text) => {
                host.set_tooltip_text_if_changed(Some(text), cx);
            }
            None => {
                if !other_hover_active {
                    host.clear_tooltip(cx);
                }
            }
        });
    }

    /// Paths a stage/unstage shortcut should act on when the file it targets is
    /// part of a multi-file status selection: the whole selection, resolved the
    /// same way the status row button and the context menu resolve it. `None`
    /// means there is no such selection and the caller keeps acting on the one
    /// file it already resolved.
    ///
    /// Reads only. The shortcut may still raise a confirmation the user cancels,
    /// so [`Self::clear_status_selection_for_shortcut`] is a separate step the
    /// caller owes once it commits to the action.
    pub(in crate::view) fn status_selection_for_shortcut(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        path: &std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) -> Option<Vec<std::path::PathBuf>> {
        self.root_view
            .update(cx, |root, cx| {
                let (paths, used_selection) = root
                    .details_pane
                    .read(cx)
                    .status_selected_paths_for_action(repo_id, area, path);
                used_selection.then_some(paths)
            })
            .ok()
            .flatten()
    }

    /// Drop the row selection a shortcut has just acted on.
    pub(in crate::view) fn clear_status_selection_for_shortcut(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.details_pane.update(cx, |pane, cx| {
                pane.clear_status_multi_selection(repo_id);
                cx.notify();
            });
        });
    }

    /// Raise the unresolved-conflict confirmation if staging `paths` would mark
    /// files resolved while they still contain conflict markers. Returns whether
    /// the dialog took over, in which case the caller must not stage: the dialog
    /// dispatches it if the user goes ahead. Unstaging never marks anything
    /// resolved, so it is left alone.
    pub(in crate::view) fn confirm_stage_conflict_markers(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        paths: Vec<std::path::PathBuf>,
        clear_selection: bool,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if area != DiffArea::Unstaged {
            return false;
        }
        let Some(confirm) = crate::view::conflict_markers::stage_confirm_popover(
            &self.state,
            repo_id,
            paths,
            clear_selection,
        ) else {
            return false;
        };
        let anchor = crate::view::conflict_markers::centered_dialog_anchor(window);
        self.open_popover_at(confirm, anchor, window, cx);
        cx.notify();
        true
    }

    /// Stage (or unstage) a whole status selection in one batch, clearing the
    /// diff selection first because every one of those files is about to move to
    /// the other section. Same order the context menu uses.
    pub(in crate::view) fn stage_or_unstage_status_paths(
        &mut self,
        repo_id: RepoId,
        area: DiffArea,
        paths: Vec<std::path::PathBuf>,
    ) {
        self.store.dispatch(Msg::ClearDiffSelection { repo_id });
        let paths = paths.into();
        self.store.dispatch(match area {
            DiffArea::Unstaged => Msg::StagePaths { repo_id, paths },
            DiffArea::Staged => Msg::UnstagePaths { repo_id, paths },
        });
    }

    pub(in crate::view) fn clear_status_multi_selection(
        &mut self,
        repo_id: RepoId,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.details_pane.update(cx, |pane, cx| {
                pane.status_multi_selection.remove(&repo_id);
                cx.notify();
            });
        });
    }

    pub(in crate::view) fn open_submodule_inner_diff(
        &mut self,
        submodule_repo_path: std::path::PathBuf,
        target: DiffTarget,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, move |root, cx| {
            root.submodule_diff_bootstrap =
                Some(SubmoduleDiffBootstrap::new(submodule_repo_path, target));
            root.drive_submodule_diff_bootstrap();
            cx.notify();
        });
    }

    pub(in crate::view) fn active_change_tracking_view(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> ChangeTrackingView {
        self.root_view
            .update(cx, |root, _cx| root.change_tracking_view)
            .unwrap_or(ChangeTrackingView::Combined)
    }

    pub(in crate::view) fn scroll_status_section_to_ix(
        &mut self,
        section: StatusSection,
        ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.details_pane
                .update(cx, |pane: &mut DetailsPaneView, cx| {
                    match section {
                        StatusSection::CombinedUnstaged | StatusSection::Unstaged => pane
                            .unstaged_scroll
                            .scroll_to_item_strict(ix, gpui::ScrollStrategy::Center),
                        StatusSection::Untracked => pane
                            .untracked_scroll
                            .scroll_to_item_strict(ix, gpui::ScrollStrategy::Center),
                        StatusSection::Staged => pane
                            .staged_scroll
                            .scroll_to_item_strict(ix, gpui::ScrollStrategy::Center),
                    }
                    cx.notify();
                });
        });
    }

    pub(in crate::view) fn scroll_commit_details_file_to_ix(
        &mut self,
        ix: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let _ = self.root_view.update(cx, |root, cx| {
            root.details_pane
                .update(cx, |pane: &mut DetailsPaneView, cx| {
                    pane.commit_files_scroll
                        .scroll_to_item_strict(ix, gpui::ScrollStrategy::Center);
                    cx.notify();
                });
        });
    }

    pub(super) fn apply_state_snapshot(
        &mut self,
        next: Arc<AppState>,
        cx: &mut gpui::Context<Self>,
    ) {
        let prev_active_repo_id = self.state.active_repo;
        let prev_diff_target = Self::rendered_diff_target_for_state(self.state.as_ref());

        let next_repo_id = next.active_repo;
        let next_diff_target = Self::rendered_diff_target_for_state(next.as_ref());

        if prev_diff_target != next_diff_target {
            self.clear_diff_selection_state();
            self.diff_autoscroll_pending = next_diff_target.is_some();
            self.worktree_preview_path = None;
            self.worktree_preview = Loadable::NotLoaded;
            self.worktree_preview_content_rev = 0;
            self.worktree_markdown_preview_path = None;
            self.worktree_markdown_preview_source_rev = 0;
            self.worktree_markdown_preview = Loadable::NotLoaded;
            self.worktree_markdown_preview_inflight = None;
            self.worktree_preview_syntax_language = None;
            self.reset_worktree_preview_source_state();
            self.reset_diff_horizontal_scroll_state();
            self.reset_collapsed_diff_projection(true);
        }

        self.state = next;
        // A closed repo tab takes its `RepoId` with it; buffers stashed under it
        // can never be saved again and would block every future close.
        self.prune_orphaned_file_editor_stash();

        self.sync_conflict_resolver(cx);
        self.ensure_file_image_diff_cache(cx);
        if self.current_main_diff_supports_diff_content_toggle() {
            self.ensure_file_diff_cache(cx);
        }

        if prev_active_repo_id != next_repo_id {
            self.history_view.update(cx, |view, _| {
                view.history_scroll
                    .scroll_to_item_strict(0, gpui::ScrollStrategy::Top);
            });
        }

        self.ensure_rendered_patch_diff_cache(cx);

        // Sync per-repo interactive commit editing state. Each repo with a setup
        // gets its own `IRebaseViewState`, populated once its entries become Ready
        // and kept (with local edits) across repo-tab switches. State for repos
        // whose setup is gone (cancelled, started, repo closed) is dropped.
        self.sync_interactive_commit_editor_states();

        // History caches are now managed by HistoryView.
    }

    pub(in crate::view) fn cached_path_display(&self, path: &std::path::Path) -> SharedString {
        let mut cache = self.path_display_cache.borrow_mut();
        path_display::cached_path_display(&mut cache, path)
    }

    pub(in crate::view) fn touch_diff_text_layout_cache(
        &mut self,
        key: u64,
        layout: Option<ShapedLine>,
    ) {
        let epoch = self.diff_text_layout_cache_epoch;
        match layout {
            Some(layout) => {
                self.diff_text_layout_cache.insert(
                    key,
                    DiffTextLayoutCacheEntry {
                        layout,
                        last_used_epoch: epoch,
                    },
                );
            }
            None => {
                if let Some(entry) = self.diff_text_layout_cache.get_mut(&key) {
                    entry.last_used_epoch = epoch;
                }
            }
        }
    }

    /// Prune the layout cache if it has grown past the high-water mark.
    /// Call once per render frame (after bumping the epoch), **not** from
    /// the per-row `touch_diff_text_layout_cache` hot path.
    pub(in crate::view) fn prune_diff_text_layout_cache(&mut self) {
        if self.diff_text_layout_cache.len()
            <= DIFF_TEXT_LAYOUT_CACHE_MAX_ENTRIES + DIFF_TEXT_LAYOUT_CACHE_PRUNE_OVERAGE
        {
            return;
        }

        let over_by = self
            .diff_text_layout_cache
            .len()
            .saturating_sub(DIFF_TEXT_LAYOUT_CACHE_MAX_ENTRIES);
        if over_by == 0 {
            return;
        }

        let mut by_age: Vec<(u64, u64)> = self
            .diff_text_layout_cache
            .iter()
            .map(|(k, v)| (*k, v.last_used_epoch))
            .collect();
        by_age.sort_by_key(|(_, last_used)| *last_used);

        for (key, _) in by_age.into_iter().take(over_by) {
            self.diff_text_layout_cache.remove(&key);
        }
    }

    pub(in crate::view) fn diff_text_segments_cache_get(
        &self,
        key: usize,
        syntax_epoch: u64,
    ) -> Option<&CachedDiffStyledText> {
        versioned_cached_diff_styled_text_is_current(
            self.diff_text_segments_cache
                .get(key)
                .and_then(Option::as_ref),
            syntax_epoch,
        )
    }

    pub(in crate::view) fn file_diff_split_cache_key(
        &self,
        row_ix: usize,
        region: DiffTextRegion,
    ) -> Option<usize> {
        let base = row_ix.checked_mul(2)?;
        match region {
            DiffTextRegion::SplitLeft => Some(base),
            DiffTextRegion::SplitRight => base.checked_add(1),
            DiffTextRegion::Inline => None,
        }
    }

    pub(in crate::view) fn diff_text_segments_cache_set(
        &mut self,
        key: usize,
        syntax_epoch: u64,
        value: CachedDiffStyledText,
    ) -> &CachedDiffStyledText {
        if self.diff_text_segments_cache.len() <= key {
            self.diff_text_segments_cache.resize_with(key + 1, || None);
        }
        self.diff_text_segments_cache[key] = Some(VersionedCachedDiffStyledText {
            syntax_epoch,
            query_generation: 0,
            styled: value,
        });
        if self.diff_text_query_segments_cache.len() > key {
            self.diff_text_query_segments_cache[key] = None;
        }
        self.diff_text_segments_cache[key]
            .as_ref()
            .map(|entry| &entry.styled)
            .expect("just set")
    }

    /// Returns the current diff search query, or an empty `SharedString` if search is inactive.
    pub(in crate::view) fn diff_search_query_or_empty(&self) -> SharedString {
        if self.diff_search_active {
            self.diff_search_query.clone()
        } else {
            SharedString::default()
        }
    }

    /// Returns the syntax mode for patch diff views (non-full-document).
    /// Uses `Auto` for small diffs and `HeuristicOnly` for large ones.
    pub(in crate::view) fn patch_diff_syntax_mode(&self) -> rows::DiffSyntaxMode {
        if self.patch_diff_row_len() <= rows::MAX_LINES_FOR_SYNTAX_HIGHLIGHTING {
            rows::DiffSyntaxMode::Auto
        } else {
            rows::DiffSyntaxMode::HeuristicOnly
        }
    }

    pub(in crate::view) fn conflict_row_styling_enabled(&self) -> bool {
        !self.conflict_resolver.is_binary_conflict
    }

    pub(in crate::view) fn conflict_row_syntax_language(&self) -> Option<rows::DiffSyntaxLanguage> {
        self.conflict_resolver.conflict_syntax_language
    }

    pub(in crate::view) fn worktree_preview_segments_cache_get(
        &self,
        key: usize,
    ) -> Option<&CachedDiffStyledText> {
        versioned_cached_diff_styled_text_is_current(
            self.worktree_preview_segments_cache.get(&key),
            self.worktree_preview_style_cache_epoch,
        )
    }

    pub(in crate::view) fn worktree_preview_segments_cache_set(
        &mut self,
        key: usize,
        value: CachedDiffStyledText,
    ) {
        self.worktree_preview_segments_cache.insert(
            key,
            VersionedCachedDiffStyledText {
                syntax_epoch: self.worktree_preview_style_cache_epoch,
                query_generation: 0,
                styled: value,
            },
        );
    }

    pub(in crate::view) fn is_file_diff_view_active(&self) -> bool {
        self.effective_diff_content_mode() == DiffContentMode::Full
            && self.rendered_file_diff_cache_is_current()
    }

    /// Whether the rasterized image diff on screen belongs to the current
    /// target. Deliberately not gated on [`DiffContentMode`]: an image has no
    /// collapsed form, so its rendered view is the same in either diff mode.
    pub(in crate::view) fn is_file_image_diff_view_active(&self) -> bool {
        let Some((repo_id, diff_file_rev, diff_target, _workdir, abs_path)) =
            self.rendered_file_diff_identity()
        else {
            return false;
        };
        self.file_image_diff_cache_repo_id == Some(repo_id)
            && self.file_image_diff_cache_rev == diff_file_rev
            && self.file_image_diff_cache_target == Some(diff_target)
            && self.file_image_diff_cache_path.as_ref() == Some(&abs_path)
            && (self.file_image_diff_cache_old.is_some()
                || self.file_image_diff_cache_new.is_some()
                || self.file_image_diff_cache_old_svg_path.is_some()
                || self.file_image_diff_cache_new_svg_path.is_some())
    }

    pub(in crate::view) fn consume_suppress_click_after_drag(&mut self) -> bool {
        if self.diff_suppress_clicks_remaining > 0 {
            self.diff_suppress_clicks_remaining =
                self.diff_suppress_clicks_remaining.saturating_sub(1);
            return true;
        }
        false
    }

    pub(in crate::view) fn select_all_diff_text(&mut self) {
        // Markdown preview (both file preview and diff preview) uses
        // markdown preview row counts instead of source-text line counts.
        if self.is_markdown_preview_active() {
            let Some(count) = self.markdown_preview_row_count() else {
                return;
            };
            if count == 0 {
                return;
            }
            let region = if self.is_file_preview_active() {
                DiffTextRegion::Inline
            } else {
                match self.diff_view {
                    DiffViewMode::Inline => DiffTextRegion::Inline,
                    DiffViewMode::Split => self
                        .diff_text_head
                        .or(self.diff_text_anchor)
                        .map(|p| p.region)
                        .filter(|r| {
                            matches!(r, DiffTextRegion::SplitLeft | DiffTextRegion::SplitRight)
                        })
                        .unwrap_or(DiffTextRegion::SplitLeft),
                }
            };
            let end_visible_ix = count - 1;
            let end_offset = self.diff_text_line_len_for_region(end_visible_ix, region);

            self.diff_text_selecting = false;
            self.diff_text_anchor = Some(DiffTextPos {
                source_visible_ix: 0,
                region,
                offset: 0,
            });
            self.diff_text_head = Some(DiffTextPos {
                source_visible_ix: end_visible_ix,
                region,
                offset: end_offset,
            });
            self.sync_diff_focus_to_text_selection();
            return;
        }

        if self.is_file_preview_active() {
            let Some(count) = self.worktree_preview_line_count() else {
                return;
            };
            if count == 0 {
                return;
            }
            let end_visible_ix = count - 1;
            let end_offset =
                self.diff_text_line_len_for_region(end_visible_ix, DiffTextRegion::Inline);

            self.diff_text_selecting = false;
            self.diff_text_anchor = Some(DiffTextPos {
                source_visible_ix: 0,
                region: DiffTextRegion::Inline,
                offset: 0,
            });
            self.diff_text_head = Some(DiffTextPos {
                source_visible_ix: end_visible_ix,
                region: DiffTextRegion::Inline,
                offset: end_offset,
            });
            self.sync_diff_focus_to_text_selection();
            return;
        }

        if self.diff_source_visible_len() == 0 {
            return;
        }

        let start_region = match self.diff_view {
            DiffViewMode::Inline => DiffTextRegion::Inline,
            DiffViewMode::Split => self
                .diff_text_head
                .or(self.diff_text_anchor)
                .map(|p| p.region)
                .filter(|r| matches!(r, DiffTextRegion::SplitLeft | DiffTextRegion::SplitRight))
                .unwrap_or(DiffTextRegion::SplitLeft),
        };

        let end_visible_ix = self.diff_source_visible_len() - 1;
        let end_region = start_region;
        let end_offset = self
            .diff_text_full_line_for_region(end_visible_ix, end_region)
            .len();

        self.diff_text_selecting = false;
        self.diff_text_anchor = Some(DiffTextPos {
            source_visible_ix: 0,
            region: start_region,
            offset: 0,
        });
        self.diff_text_head = Some(DiffTextPos {
            source_visible_ix: end_visible_ix,
            region: end_region,
            offset: end_offset,
        });
        self.sync_diff_focus_to_text_selection();
    }

    pub(in crate::view) fn main_pane_content_width(&self, cx: &mut gpui::Context<Self>) -> Pixels {
        let _ = cx;

        super::pane_content_width_for_layout(
            self.last_window_size.width,
            self.layout_sidebar_render_width,
            self.layout_details_render_width,
            self.layout_sidebar_collapsed,
            self.layout_details_collapsed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_fingerprint_tracks_cherry_pick_message_readiness() {
        use std::path::PathBuf;
        use worktree_core::domain::RepoSpec;
        use worktree_core::services::{InteractiveRebaseAction, InteractiveRebaseEntry};
        use worktree_state::model::{InteractiveCherryPickSetup, RepoState};

        let mut state = AppState::default();
        state.active_repo = Some(RepoId(1));
        state.repos.push(RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        ));
        let without_setup = MainPaneView::notify_fingerprint_for(&state);

        state.repos[0].interactive_cherry_pick_setup = Some(InteractiveCherryPickSetup {
            entries: vec![InteractiveRebaseEntry {
                action: InteractiveRebaseAction::Pick,
                commit_id: "1111111111111111111111111111111111111111".to_string(),
                summary: "subject".to_string(),
                message: "subject".to_string(),
                new_message: None,
            }],
            source_colors: vec![],
            full_messages: Loadable::Loading,
        });
        let loading = MainPaneView::notify_fingerprint_for(&state);
        assert_ne!(loading, without_setup);

        state.repos[0]
            .interactive_cherry_pick_setup
            .as_mut()
            .expect("setup")
            .full_messages = Loadable::Ready(());
        let ready = MainPaneView::notify_fingerprint_for(&state);
        assert_ne!(ready, loading);
    }

    #[test]
    fn should_request_blame_retries_failure_only_when_forced() {
        use worktree_state::model::Loadable;
        // A new/changed target always loads, regardless of state or force.
        assert!(should_request_blame(
            false,
            &Loadable::<()>::Ready(()),
            false
        ));
        assert!(should_request_blame(
            false,
            &Loadable::<()>::Error("x".into()),
            false
        ));
        // Same target, healthy or in flight: never reload (even when forced), so a
        // toggle-on doesn't re-blame an already-loaded file.
        assert!(!should_request_blame(
            true,
            &Loadable::<()>::Ready(()),
            true
        ));
        assert!(!should_request_blame(true, &Loadable::<()>::Loading, true));
        // Same target, not yet loaded: load.
        assert!(should_request_blame(
            true,
            &Loadable::<()>::NotLoaded,
            false
        ));
        // Same target, failed: never retry from the per-frame Render path
        // (force=false), but retry on an explicit toggle (force=true).
        assert!(!should_request_blame(
            true,
            &Loadable::<()>::Error("e".into()),
            false
        ));
        assert!(should_request_blame(
            true,
            &Loadable::<()>::Error("e".into()),
            true
        ));
    }

    #[test]
    fn clamp_raw_scroll_y_uses_gpui_negative_offset_range() {
        assert_eq!(clamp_raw_scroll_y(px(-180.0), px(120.0)), px(-120.0));
        assert_eq!(clamp_raw_scroll_y(px(180.0), px(120.0)), px(0.0));
        assert_eq!(clamp_raw_scroll_y(px(-40.0), px(120.0)), px(-40.0));
    }

    #[test]
    fn synced_scroll_offsets_keep_longer_pane_as_master_after_shorter_clamps() {
        let targets = compute_synced_scroll_offsets(
            [px(-100.0), px(-500.0)],
            [px(100.0), px(500.0)],
            [px(-90.0), px(-90.0)],
            1,
        );

        assert_eq!(targets, [px(-100.0), px(-500.0)]);
    }

    #[test]
    fn synced_scroll_offsets_follow_shorter_pane_when_user_scrolled_it() {
        let targets = compute_synced_scroll_offsets(
            [px(-100.0), px(-320.0)],
            [px(100.0), px(500.0)],
            [px(-80.0), px(-320.0)],
            1,
        );

        assert_eq!(targets, [px(-100.0), px(-100.0)]);
    }

    #[test]
    fn synced_scroll_offsets_support_four_panes_when_output_is_scrolled() {
        let targets = compute_synced_scroll_offsets(
            [px(-100.0), px(-100.0), px(-100.0), px(-320.0)],
            [px(100.0), px(100.0), px(100.0), px(500.0)],
            [px(-100.0), px(-100.0), px(-100.0), px(-80.0)],
            3,
        );

        assert_eq!(targets, [px(-100.0), px(-100.0), px(-100.0), px(-320.0)]);
    }

    #[test]
    fn synced_scroll_offsets_hold_steady_when_nothing_changed() {
        // A clamped follower (shorter pane, offset -100) sits alongside a master
        // scrolled further (-320). Nothing moved since the last sync (offsets ==
        // last_synced), so even though the offsets are unequal the follower must
        // stay put — re-driving it onto the widest handle here is the idle-frame
        // snap-back the horizontal output sync used to produce.
        let steady = [px(-100.0), px(-320.0)];
        let targets = compute_synced_scroll_offsets(steady, [px(100.0), px(500.0)], steady, 1);

        assert_eq!(targets, steady);
    }

    #[test]
    fn synced_scroll_offsets_do_not_promote_a_follower_clamped_during_paint() {
        let steady = [px(-100.0), px(-500.0)];
        let targets = compute_synced_scroll_offsets(
            steady,
            [px(100.0), px(500.0)],
            // The previous render requested -120 for the shorter follower;
            // GPUI painted it at its current -100 maximum afterward.
            [px(-120.0), px(-500.0)],
            1,
        );

        assert_eq!(targets, steady);
    }

    #[test]
    fn explicit_wheel_master_wins_when_multiple_handles_changed() {
        let targets = compute_synced_scroll_offsets_with_master(
            [px(0.0), px(-100.0)],
            [px(500.0), px(500.0)],
            [px(-100.0), px(0.0)],
            0,
            Some(1),
        );

        assert_eq!(targets, [px(-100.0), px(-100.0)]);
    }

    #[test]
    fn explicit_wheel_master_at_top_pulls_stale_follower_to_top() {
        let targets = compute_synced_scroll_offsets_with_master(
            [px(0.0), px(-100.0)],
            [px(500.0), px(500.0)],
            [px(0.0), px(-100.0)],
            1,
            Some(0),
        );

        assert_eq!(targets, [px(0.0), px(0.0)]);
    }

    #[test]
    fn revealed_whitespace_wrap_ranges_follow_rendered_tab_markers() {
        let hidden = diff_wrap_byte_ranges_for_text("a    b", Some("a\tb"), 4, false)
            .into_iter()
            .map(rows::DiffWrapByteRange::range)
            .collect::<Vec<_>>();
        assert_eq!(hidden, vec![0..4, 4..6]);

        let revealed = diff_wrap_byte_ranges_for_text("a    b", Some("a\tb"), 4, true)
            .into_iter()
            .map(rows::DiffWrapByteRange::range)
            .collect::<Vec<_>>();
        assert_eq!(revealed, vec![0..6]);
    }
}
