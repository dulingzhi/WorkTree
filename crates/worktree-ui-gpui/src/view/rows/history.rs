use super::diff_canvas;
use super::diff_text::*;
use super::history_canvas;
use super::markdown_preview::{
    MarkdownPreviewRenderContext, MarkdownPreviewWrapMeasure, markdown_preview_font_family_hash,
    markdown_preview_row_required_width, render_markdown_preview_document_rows,
    worktree_preview_bar_color,
};
use super::*;
use crate::view::caches::HistoryListRow;
use palette::IntoColor;

use crate::view::markdown_preview::{
    MarkdownAlertKind, MarkdownChangeHint, MarkdownInlineStyle, MarkdownPreviewDocument,
    MarkdownPreviewRow, MarkdownPreviewRowKind,
};
use crate::view::panes::main::diff_search::DiffSearchMatcher;
use crate::view::perf::{self, ViewPerfRenderLane, ViewPerfSpan};
use rustc_hash::FxHasher;
use worktree_core::services::BisectVerdict;
use worktree_state::msg::CommitSelectMode;

#[derive(Clone)]
struct WorktreePreviewPreparedSyntaxSource {
    document_text: Arc<str>,
    line_starts: Arc<[usize]>,
    document: rows::PreparedDiffSyntaxDocument,
}

fn worktree_preview_apply_query_overlay(
    theme: AppTheme,
    styled: CachedDiffStyledText,
    query_matcher: Option<&DiffSearchMatcher>,
    emphasis: DiffSearchMatchEmphasis,
) -> CachedDiffStyledText {
    query_matcher
        .map(|matcher| {
            build_cached_diff_query_overlay_styled_text(theme, &styled, matcher, emphasis)
        })
        .unwrap_or(styled)
}

fn worktree_preview_streamed_spec(
    raw_text: worktree_core::file_diff::FileDiffLineText,
    line_ix: usize,
    query: &SharedString,
    query_options: super::super::panes::main::diff_search::DiffSearchOptions,
    query_matcher: Option<Arc<DiffSearchMatcher>>,
    query_emphasis: DiffSearchMatchEmphasis,
    language: Option<rows::DiffSyntaxLanguage>,
    syntax_mode: rows::DiffSyntaxMode,
    prepared_syntax_source: Option<&WorktreePreviewPreparedSyntaxSource>,
) -> Option<diff_canvas::StreamedDiffTextPaintSpec> {
    diff_canvas::is_streamable_diff_text(&raw_text).then(|| {
        let syntax = match (language, prepared_syntax_source) {
            (Some(language), Some(prepared_syntax_source)) => {
                diff_canvas::StreamedDiffTextSyntaxSource::Prepared {
                    document_text: Arc::clone(&prepared_syntax_source.document_text),
                    line_starts: Arc::clone(&prepared_syntax_source.line_starts),
                    document: prepared_syntax_source.document,
                    language,
                    line_ix,
                }
            }
            (Some(language), None) => diff_canvas::StreamedDiffTextSyntaxSource::Heuristic {
                language,
                mode: syntax_mode,
            },
            (None, _) => diff_canvas::StreamedDiffTextSyntaxSource::None,
        };
        diff_canvas::StreamedDiffTextPaintSpec {
            raw_text,
            query: query.clone(),
            query_options,
            query_matcher,
            query_emphasis,
            word_ranges: Arc::from([]),
            word_kind: None,
            syntax,
        }
    })
}

impl MainPaneView {
    pub(in super::super) fn render_worktree_preview_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let min_width = this.diff_horizontal_content_width();
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query_matcher = (!query.as_ref().is_empty())
            .then(|| Arc::new(DiffSearchMatcher::new(query.as_ref(), query_options)));
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();

        let theme = this.theme;
        let Some(path) = this.worktree_preview_path.as_ref() else {
            return Vec::new();
        };
        let Some(line_count) = this.worktree_preview_line_count() else {
            return Vec::new();
        };

        let should_clear_cache = match this.worktree_preview_segments_cache_path.as_ref() {
            Some(p) => p != path,
            None => true,
        };
        if should_clear_cache {
            this.worktree_preview_segments_cache_path = Some(path.clone());
            this.worktree_preview_syntax_language = diff_syntax_language_for_path(path);
            this.worktree_preview_segments_cache.clear();
        }

        let language = this.worktree_preview_syntax_language;
        let syntax_document = this.worktree_preview_prepared_syntax_document();
        let syntax_mode = syntax_mode_for_prepared_document(syntax_document);
        let prepared_syntax_source = match syntax_document {
            Some(document) if !this.worktree_preview_text.is_empty() => {
                Some(WorktreePreviewPreparedSyntaxSource {
                    document_text: Arc::from(this.worktree_preview_text.as_ref()),
                    line_starts: Arc::clone(&this.worktree_preview_line_starts),
                    document,
                })
            }
            _ => None,
        };
        let highlight_palette = syntax_highlight_palette(theme);

        let current_match_line = this.diff_search_current_match_row();
        let bar_color = worktree_preview_bar_color(this, theme);
        let defer_cache_write = this.worktree_preview_cache_write_blocked_until_rev
            == Some(this.worktree_preview_content_rev);
        // Blame annotations for the file content view: a fixed left column when
        // annotate is on and blame for this target is loaded.
        let annotation_width = if this.annotate_enabled {
            this.annotate_column_width_px(ui_scale_percent)
        } else {
            px(0.0)
        };
        let blame_ctx = this.blame_render_ctx();

        // With word wrap on, a list position is a visual row and one file line
        // owns several of them. Everything that describes the *line* — its
        // text, syntax, blame, number — is looked up by `line_ix`; everything
        // that addresses the *row* on screen keeps `visible_ix`.
        let visible_len = this.worktree_preview_visible_len().unwrap_or(line_count);
        range
            .take_while(|ix| *ix < visible_len)
            .map(|visible_ix| {
                let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);
                let is_continuation = wrap.is_some_and(|wrap| wrap.wrap_ix > 0);
                let ix = this
                    .diff_source_visible_ix_for_visible_ix(visible_ix)
                    .unwrap_or(visible_ix)
                    .min(line_count.saturating_sub(1));
                // A wrapped line is one line however many rows it takes, so its
                // number and its blame belong to the first of them.
                let line_no = if is_continuation {
                    SharedString::default()
                } else {
                    line_number_string(u32::try_from(ix + 1).ok())
                };
                let blame = blame_ctx.as_ref().filter(|_| !is_continuation).and_then(|ctx| {
                    // The file-content view renders every line contiguously, so the
                    // previous rendered line is `ix` (1-based), absent for line 1.
                    let prev_new_line = u32::try_from(ix).ok().filter(|&p| p >= 1);
                    // The full file-content view has no diff sidedness, so it
                    // cannot tell staged from unstaged per line; pass
                    // `is_context = false` so uncommitted lines fall back to the
                    // blamed area's default (staged area → "Staged", unstaged area
                    // → "Unstaged") rather than being mislabeled.
                    super::diff::build_row_blame_paint(
                        ctx,
                        false,
                        None,
                        u32::try_from(ix + 1).ok(),
                        prev_new_line,
                        theme,
                    )
                });
                let Some(raw_text) = this.worktree_preview_line_raw_text(ix) else {
                    return diff_canvas::worktree_preview_row_canvas(
                        theme,
                        cx.entity(),
                        ui_scale_percent,
                        visible_ix,
                        min_width,
                        annotation_width,
                        blame,
                        bar_color,
                        line_no,
                        None,
                        None,
                        None,
                        this.reveal_whitespace_chars,
                        wrap,
                    );
                };
                // This view has no selection of its own, so the row the cursor is
                // on wears the selection wash to stand out from the rest.
                let emphasis = if current_match_line == Some(ix) {
                    DiffSearchMatchEmphasis::Current
                } else {
                    DiffSearchMatchEmphasis::Other
                };
                let is_current_match = emphasis == DiffSearchMatchEmphasis::Current;
                let streamed_spec = worktree_preview_streamed_spec(
                    raw_text.clone(),
                    ix,
                    &query,
                    query_options,
                    query_matcher.clone(),
                    emphasis,
                    language,
                    syntax_mode,
                    prepared_syntax_source.as_ref(),
                );
                let mut pending_styled = None;
                // The current row is rebuilt rather than read from the cache,
                // which holds the plain wash: re-washing an already-washed row
                // would keep the foreground the first pass pinned on light themes.
                // One row per frame, the cost of a cache miss.
                if streamed_spec.is_none()
                    && (is_current_match || this.worktree_preview_segments_cache_get(ix).is_none())
                {
                    let line = raw_text.as_ref();
                    let (styled, is_pending) =
                        build_cached_diff_styled_text_for_prepared_document_line_nonblocking_with_palette(
                            theme,
                            &highlight_palette,
                            PreparedDiffTextBuildRequest {
                                build: DiffTextBuildRequest {
                                    text: line,
                                    word_ranges: &[],
                                    query: "",
                                    syntax: DiffSyntaxConfig {
                                        language,
                                        mode: syntax_mode,
                                    },
                                    word_kind: None,
                                },
                                prepared_line: PreparedDiffSyntaxLine {
                                    document: syntax_document,
                                    line_ix: ix,
                                },
                            },
                        )
                        .into_parts();
                    let styled = worktree_preview_apply_query_overlay(
                        theme,
                        styled,
                        query_matcher.as_deref(),
                        emphasis,
                    );
                    if is_pending {
                        this.ensure_prepared_syntax_chunk_poll(cx);
                        pending_styled = Some(styled);
                    } else {
                        // Never cached while current: the cursor moves off it, and
                        // a cached entry would leave that row painted as current.
                        if defer_cache_write || is_current_match {
                            pending_styled = Some(styled);
                        } else {
                            this.worktree_preview_segments_cache_set(ix, styled);
                        }
                    }
                }

                let cached_styled = this.worktree_preview_segments_cache_get(ix);
                let styled = pending_styled.as_ref().or(cached_styled);

                diff_canvas::worktree_preview_row_canvas(
                    theme,
                    cx.entity(),
                    ui_scale_percent,
                    visible_ix,
                    min_width,
                    annotation_width,
                    blame,
                    bar_color,
                    line_no,
                    styled,
                    streamed_spec,
                    Some(raw_text.as_ref()),
                    this.reveal_whitespace_chars,
                    wrap,
                )
            })
            .collect()
    }

    pub(in super::super) fn render_markdown_diff_left_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let editor_font_family: SharedString =
            crate::font_preferences::current_editor_font_family(cx).into();
        let Loadable::Ready(preview) = &this.file_markdown_preview else {
            return Vec::new();
        };
        let preview = Arc::clone(preview);
        let viewport_width = this
            .diff_scroll
            .0
            .borrow()
            .base_handle
            .bounds()
            .size
            .width
            .max(px(0.0));
        this.update_markdown_preview_horizontal_min_width(
            &preview.old,
            range.clone(),
            editor_font_family.as_ref(),
            window,
            cx,
        );
        let region = match this.diff_view {
            DiffViewMode::Inline => DiffTextRegion::Inline,
            DiffViewMode::Split => DiffTextRegion::SplitLeft,
        };
        let view = cx.entity().clone();
        let image_base_dir: Option<Arc<std::path::Path>> = this
            .markdown_preview_image_base_dir()
            .map(|dir| Arc::from(dir.as_path()));
        let min_width = this.diff_horizontal_content_width().max(viewport_width);
        render_markdown_preview_document_rows(
            &preview.old,
            range,
            &MarkdownPreviewRenderContext {
                theme,
                min_width,
                editor_font_family,
                ui_scale_percent,
                view: Some(view),
                text_region: region,
                wrap_plan: this.markdown_preview_wrap_plan(MarkdownPreviewList::Old),
                image_base_dir: image_base_dir.clone(),
                query: this.markdown_preview_search_query(),
            },
        )
    }

    pub(in super::super) fn render_markdown_diff_inline_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let editor_font_family: SharedString =
            crate::font_preferences::current_editor_font_family(cx).into();
        let Loadable::Ready(preview) = &this.file_markdown_preview else {
            return Vec::new();
        };
        let preview = Arc::clone(preview);
        let viewport_width = this
            .diff_scroll
            .0
            .borrow()
            .base_handle
            .bounds()
            .size
            .width
            .max(px(0.0));
        this.update_markdown_preview_horizontal_min_width(
            &preview.inline,
            range.clone(),
            editor_font_family.as_ref(),
            window,
            cx,
        );
        let view = cx.entity().clone();
        let image_base_dir: Option<Arc<std::path::Path>> = this
            .markdown_preview_image_base_dir()
            .map(|dir| Arc::from(dir.as_path()));
        let min_width = this.diff_horizontal_content_width().max(viewport_width);
        render_markdown_preview_document_rows(
            &preview.inline,
            range,
            &MarkdownPreviewRenderContext {
                theme,
                min_width,
                editor_font_family,
                ui_scale_percent,
                view: Some(view),
                text_region: DiffTextRegion::Inline,
                wrap_plan: this.markdown_preview_wrap_plan(MarkdownPreviewList::Inline),
                image_base_dir: image_base_dir.clone(),
                query: this.markdown_preview_search_query(),
            },
        )
    }

    pub(in super::super) fn render_markdown_diff_right_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = this.theme;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let editor_font_family: SharedString =
            crate::font_preferences::current_editor_font_family(cx).into();
        let Loadable::Ready(preview) = &this.file_markdown_preview else {
            return Vec::new();
        };
        let preview = Arc::clone(preview);
        let viewport_width = this
            .diff_split_right_scroll
            .0
            .borrow()
            .base_handle
            .bounds()
            .size
            .width
            .max(px(0.0));
        this.update_markdown_preview_horizontal_min_width(
            &preview.new,
            range.clone(),
            editor_font_family.as_ref(),
            window,
            cx,
        );
        let view = cx.entity().clone();
        let image_base_dir: Option<Arc<std::path::Path>> = this
            .markdown_preview_image_base_dir()
            .map(|dir| Arc::from(dir.as_path()));
        let min_width = this.diff_horizontal_content_width().max(viewport_width);
        render_markdown_preview_document_rows(
            &preview.new,
            range,
            &MarkdownPreviewRenderContext {
                theme,
                min_width,
                editor_font_family,
                ui_scale_percent,
                view: Some(view),
                text_region: DiffTextRegion::SplitRight,
                wrap_plan: this.markdown_preview_wrap_plan(MarkdownPreviewList::New),
                image_base_dir: image_base_dir.clone(),
                query: this.markdown_preview_search_query(),
            },
        )
    }

    /// Rebuild the wrapped visual-row mapping for one preview list if the
    /// width, font, scale, change bar, or document it was measured against
    /// changed.
    ///
    /// Returns the number of rows the list should render: the wrapped visual
    /// row count while word wrap is on, and the plain source row count
    /// otherwise.
    pub(in crate::view) fn ensure_markdown_preview_wrap_plan(
        &mut self,
        list: MarkdownPreviewList,
        document: &MarkdownPreviewDocument,
        document_rev: u64,
        available_width: Pixels,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> usize {
        let Some(measure) = self.markdown_preview_wrap_measure(document_rev, available_width, cx)
        else {
            self.markdown_preview_wrap.clear_list(list);
            return document.rows.len();
        };

        if !self.markdown_preview_wrap.is_current(list, measure.key) {
            let plan = crate::view::markdown_preview::build_markdown_preview_wrap_plan(
                document,
                measure.wrap_row_fn(window, self.theme),
            );
            self.markdown_preview_wrap.store(list, measure.key, plan);
            // A search over this preview holds *visual* row indices, which the
            // new plan has just renumbered — a resize or a wrap toggle would
            // otherwise leave Enter jumping to unrelated rows.
            self.diff_search_recompute_matches();
        }

        self.markdown_preview_wrap
            .plan_len(list)
            .unwrap_or(document.rows.len())
    }

    /// Rebuild both split-preview wrap plans together so the two columns stay
    /// row-aligned, and return each list's row count.
    pub(in crate::view) fn ensure_markdown_preview_split_wrap_plans(
        &mut self,
        old_doc: &MarkdownPreviewDocument,
        new_doc: &MarkdownPreviewDocument,
        document_rev: u64,
        available_width: Pixels,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> (usize, usize) {
        let measure = self.markdown_preview_wrap_measure(document_rev, available_width, cx);

        match measure {
            None => {
                self.markdown_preview_wrap
                    .clear_list(MarkdownPreviewList::Old);
                self.markdown_preview_wrap
                    .clear_list(MarkdownPreviewList::New);
            }
            Some(measure)
                if !self
                    .markdown_preview_wrap
                    .is_current(MarkdownPreviewList::Old, measure.key)
                    || !self
                        .markdown_preview_wrap
                        .is_current(MarkdownPreviewList::New, measure.key) =>
            {
                let (old_plan, new_plan) =
                    crate::view::markdown_preview::build_markdown_preview_split_wrap_plans(
                        old_doc,
                        new_doc,
                        measure.wrap_row_fn(window, self.theme),
                    )
                    .unzip();
                self.markdown_preview_wrap
                    .store(MarkdownPreviewList::Old, measure.key, old_plan);
                self.markdown_preview_wrap
                    .store(MarkdownPreviewList::New, measure.key, new_plan);
                // See the single-document path: the visual row space a search
                // indexed has just been rebuilt.
                self.diff_search_recompute_matches();
            }
            Some(_) => {}
        }

        (
            self.markdown_preview_wrap
                .plan_len(MarkdownPreviewList::Old)
                .unwrap_or(old_doc.rows.len()),
            self.markdown_preview_wrap
                .plan_len(MarkdownPreviewList::New)
                .unwrap_or(new_doc.rows.len()),
        )
    }

    /// Wrap plans for whichever preview lists the current view mode paints,
    /// returning `(old, new, inline)` row counts.
    ///
    /// Owning the mode switch here keeps the "only painted lists hold a plan"
    /// invariant in one place instead of spread through the render tree, and
    /// stops an unpainted column from being wrapped at a width it never uses.
    pub(in crate::view) fn ensure_markdown_preview_wrap_plans(
        &mut self,
        preview: &crate::view::markdown_preview::MarkdownPreviewDiff,
        document_rev: u64,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> (usize, usize, usize) {
        let (inline_width, split_width) = self.markdown_preview_wrap_widths(cx);
        match self.diff_view {
            DiffViewMode::Inline => {
                self.markdown_preview_wrap
                    .clear_list(MarkdownPreviewList::Old);
                self.markdown_preview_wrap
                    .clear_list(MarkdownPreviewList::New);
                let inline_len = self.ensure_markdown_preview_wrap_plan(
                    MarkdownPreviewList::Inline,
                    &preview.inline,
                    document_rev,
                    inline_width,
                    window,
                    cx,
                );
                (preview.old.rows.len(), preview.new.rows.len(), inline_len)
            }
            DiffViewMode::Split => {
                self.markdown_preview_wrap
                    .clear_list(MarkdownPreviewList::Inline);
                let (old_len, new_len) = self.ensure_markdown_preview_split_wrap_plans(
                    &preview.old,
                    &preview.new,
                    document_rev,
                    split_width,
                    window,
                    cx,
                );
                (old_len, new_len, preview.inline.rows.len())
            }
        }
    }

    /// Everything needed to wrap a preview list at the current width, or
    /// `None` when word wrap is off and the list should render unwrapped.
    ///
    /// The width is quantised so dragging a window edge does not invalidate
    /// the plan on every pixel — re-wrapping a whole document is far more
    /// expensive than the sub-bucket accuracy it would buy, and wrapping to
    /// the rounded-down width keeps rows inside the viewport.
    fn markdown_preview_wrap_measure(
        &self,
        document_rev: u64,
        available_width: Pixels,
        cx: &mut gpui::Context<Self>,
    ) -> Option<MarkdownPreviewWrapMeasure> {
        const WRAP_WIDTH_BUCKET_PX: u32 = 8;

        if !self.diff_word_wrap || available_width <= px(0.0) {
            return None;
        }

        let width_px = (u32::from(available_width.floor()) / WRAP_WIDTH_BUCKET_PX)
            .saturating_mul(WRAP_WIDTH_BUCKET_PX);
        if width_px == 0 {
            return None;
        }

        let editor_font_family: SharedString =
            crate::font_preferences::current_editor_font_family(cx).into();
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        Some(MarkdownPreviewWrapMeasure {
            key: MarkdownPreviewWrapKey {
                width_px,
                ui_scale_percent,
                theme_is_dark: self.theme.is_dark,
                editor_font_family_hash: markdown_preview_font_family_hash(&editor_font_family),
                document_rev,
            },
            wrap_width: px(width_px as f32),
            editor_font_family,
            ui_scale_percent,
        })
    }

    pub(in crate::view) fn update_markdown_preview_horizontal_min_width(
        &mut self,
        document: &MarkdownPreviewDocument,
        range: Range<usize>,
        editor_font_family: &str,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.diff_word_wrap {
            // Wrapped rows never exceed the viewport, so there is no content
            // width to grow; `set_diff_word_wrap` already reset the
            // horizontal scroll state.
            return;
        }
        let mut min_width = self.diff_horizontal_content_width();
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let editor_font_family: SharedString = editor_font_family.to_owned().into();
        for row in range.filter_map(|ix| document.rows.get(ix)) {
            let required = markdown_preview_row_required_width(
                window,
                self.theme,
                row,
                &editor_font_family,
                ui_scale_percent,
            );
            if required > min_width {
                min_width = required;
            }
        }

        self.record_diff_horizontal_content_width(min_width, cx);
    }
}

impl HistoryView {
    pub(in super::super) fn render_history_table_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let (_, worktree_counts) = this.ensure_history_worktree_summary_cache();
        let plan = this.ensure_history_list_plan();
        let stash_ids = this.ensure_history_stash_ids_cache();
        // Bisect marks resolved once per list build; per-row lookups below are
        // scans of tiny vectors. `None` while no session runs, so the whole
        // decoration layer is inert otherwise.
        let bisect_session = this.active_repo().and_then(|repo| match &repo.bisect {
            Loadable::Ready(Some(state)) => Some(state.clone()),
            _ => None,
        });
        // One lane keeps full colour; the rest wash out. Resolved once here rather
        // than per row -- it is a scan of the page behind a memo.
        let selected_lane = this.history_selected_lane(plan.show_working_tree_summary_row());
        // Which rows belong to the selection: reachable from it through the
        // page's parent links, merges included. The other half of the same
        // memo as the lane.
        let related_rows = this.history_related_rows(plan.show_working_tree_summary_row());

        let Some(repo) = this.active_repo() else {
            return Vec::new();
        };
        let show_graph_color_marker =
            history_scope_shows_graph_color_marker(repo.history_state.history_scope);

        let theme = this.theme;
        let col_branch = this.history_col_branch;
        let col_graph = this.history_col_graph;
        let col_author = this.history_col_author;
        let col_date = this.history_col_date;
        let col_sha = this.history_col_sha;
        let ui_scale = this.ui_scale();
        let (show_graph, show_author, show_date, show_sha) = this.history_visible_columns();
        let display_key = HistoryDisplayKey::new(
            this.date_time_format,
            this.timezone,
            this.show_timezone,
            this.history_relative_dates,
        );

        let page = Self::display_log_page_for_repo(repo);
        let cache = this
            .history_cache
            .as_ref()
            .filter(|cache| cache.base.request.repo_id == repo.id);
        let head_visible_ix = repo.head_commit_id().and_then(|head| {
            cache.and_then(|cache| cache.base.visible_ix_by_commit.get(&head).copied())
        });
        let (worktree_node_col, worktree_node_color_ix) = history_worktree_node_placement(
            cache.map(|cache| cache.base.graph_rows.as_ref()),
            head_visible_ix,
        );
        // The pinned row's own column, for whichever row sits directly below
        // it and must stub up into the node without a seam.
        let working_tree_summary_node_col = plan
            .show_working_tree_summary_row()
            .then_some(worktree_node_col);

        let worktree_dirty = match &repo.worktree_dirty {
            Loadable::Ready(dirty) => Some(Arc::clone(dirty)),
            _ => None,
        };
        range
            .filter_map(|list_ix| {
                let row = plan.row_at(list_ix)?;
                if let HistoryListRow::WorktreeUncommitted {
                    visible_ix,
                    worktree_ix,
                } = row
                {
                    let cache = cache?;
                    let summary = worktree_dirty.as_ref()?.get(worktree_ix)?;
                    // The row shows the lanes of the commit it sits on top of,
                    // so it needs that row's paint data.
                    let graph_row = cache.base.graph_rows.get(visible_ix)?;
                    // Whatever sits directly above draws a connector down into
                    // this row; carry it through so the lane is not broken.
                    let connect_from_top_col =
                        super::history_graph_paint::worktree_band_connect_from_top_col(
                            &plan,
                            cache.base.graph_rows.as_ref(),
                            worktree_dirty
                                .as_ref()
                                .map_or(&[][..], |dirty| dirty.as_slice()),
                            working_tree_summary_node_col,
                            list_ix,
                        );
                    return Some(worktree_uncommitted_history_row(
                        theme,
                        ui_scale,
                        col_branch,
                        col_graph,
                        col_author,
                        col_date,
                        col_sha,
                        show_graph,
                        show_author,
                        show_date,
                        show_sha,
                        graph_row,
                        visible_ix,
                        connect_from_top_col,
                        selected_lane,
                        // The band sits on the worktree head's row, so the
                        // row's membership is the band's.
                        related_rows
                            .as_ref()
                            .map(|rows| rows.get(visible_ix).copied().unwrap_or(false)),
                        show_graph_color_marker,
                        repo.id,
                        list_ix,
                        repo.history_state.worktree_selection.as_deref()
                            == Some(summary.path.as_path()),
                        (summary.added, summary.modified, summary.deleted),
                        summary,
                        cx,
                    ));
                }

                if matches!(row, HistoryListRow::WorkingTreeSummary) {
                    // The working-tree row's selection is the uncommitted
                    // sentinel; a selected worktree row instead leaves
                    // `selected_commit` empty. Only one row may read as
                    // selected.
                    let selected = repo
                        .history_state
                        .selected_commit
                        .as_ref()
                        .is_some_and(CommitId::is_uncommitted)
                        || repo.history_state.selected_commit.is_none()
                            && repo.history_state.worktree_selection.is_none();
                    // Uncommitted changes belong to the branch exactly when the
                    // HEAD they sit on does.
                    let related = related_rows.as_ref().map(|rows| {
                        repo.head_commit_id()
                            .and_then(|head| {
                                cache.and_then(|cache| {
                                    cache.base.visible_ix_by_commit.get(&head).copied()
                                })
                            })
                            .is_some_and(|row_ix| rows.get(row_ix).copied().unwrap_or(false))
                    });
                    return Some(working_tree_summary_history_row(
                        theme,
                        ui_scale,
                        col_branch,
                        col_graph,
                        col_author,
                        col_date,
                        col_sha,
                        show_graph,
                        show_author,
                        show_date,
                        show_sha,
                        worktree_node_col,
                        worktree_node_color_ix,
                        selected_lane,
                        related,
                        show_graph_color_marker,
                        repo.id,
                        selected,
                        worktree_counts,
                        cx,
                    ));
                }

                let HistoryListRow::Commit { visible_ix } = row else {
                    return None;
                };

                let page = page.as_deref()?;
                let cache = cache?;

                let commit_ix = cache.base.visible_indices.get(visible_ix)?;
                let commit = page.commits.get(commit_ix)?;
                cache.base.graph_rows.get(visible_ix)?;
                let base_row_vm = cache.base.row_vms.get(visible_ix)?;
                let decoration_row_vm = cache.decorations.row_vms.get(visible_ix)?;
                // A synthetic row above connects down into this commit, so this
                // row draws the matching stub upwards even when its lane is born
                // here. Same resolution the bands use, so the two never disagree
                // about where the stub lands.
                let connect_from_top_col =
                    super::history_graph_paint::worktree_band_connect_from_top_col(
                        &plan,
                        cache.base.graph_rows.as_ref(),
                        worktree_dirty
                            .as_ref()
                            .map_or(&[][..], |dirty| dirty.as_slice()),
                        working_tree_summary_node_col,
                        list_ix,
                    );
                let selected = repo.history_state.selected_commit.as_ref() == Some(&commit.id)
                    || repo.history_state.multi_selection.is_multi()
                        && repo.history_state.multi_selection.contains(&commit.id);
                let selected_branch = this.selected_branch_for_history_row(repo.id, selected);
                let related_to_selection = related_rows
                    .as_ref()
                    .map(|rows| rows.get(visible_ix).copied().unwrap_or(false));
                let is_stash_node = base_row_vm.is_stash
                    || stash_ids
                        .as_ref()
                        .is_some_and(|ids| ids.contains(&commit.id));
                let when = base_row_vm.when.resolve(display_key);
                let short_sha = base_row_vm.short_sha.resolve();

                let lane_branch_name = decoration_row_vm
                    .lane_branch
                    .and_then(|ix| cache.decorations.branch_names.get(usize::from(ix)))
                    .cloned();
                let (bisect_mark, bisect_current) = bisect_session
                    .as_ref()
                    .map(|session| {
                        let mark = if session.bad.as_ref() == Some(&commit.id) {
                            Some(BisectVerdict::Bad)
                        } else if session.good.iter().any(|id| id == &commit.id) {
                            Some(BisectVerdict::Good)
                        } else if session.skipped.iter().any(|id| id == &commit.id) {
                            Some(BisectVerdict::Skip)
                        } else {
                            None
                        };
                        (mark, session.current.as_ref() == Some(&commit.id))
                    })
                    .unwrap_or((None, false));

                Some(history_table_row(
                    theme,
                    ui_scale,
                    col_branch,
                    col_graph,
                    col_author,
                    col_date,
                    col_sha,
                    show_graph,
                    show_author,
                    show_date,
                    show_sha,
                    show_graph_color_marker,
                    list_ix,
                    repo.id,
                    commit,
                    Arc::clone(&cache.base.graph_rows),
                    visible_ix,
                    connect_from_top_col,
                    Arc::clone(&decoration_row_vm.tag_names),
                    Arc::clone(&decoration_row_vm.ref_items),
                    selected_branch,
                    selected_lane,
                    related_to_selection,
                    lane_branch_name,
                    base_row_vm.author.clone(),
                    repo.author_emails
                        .get(commit.author.as_ref())
                        .map(|email| email.as_str()),
                    base_row_vm.summary.clone(),
                    when,
                    short_sha,
                    selected,
                    base_row_vm.is_head,
                    is_stash_node,
                    bisect_mark,
                    bisect_current,
                    this.active_context_menu_invoker.as_ref(),
                    cx,
                ))
            })
            .collect()
    }
}

/// Widest a worktree row's badge may grow before its branch label truncates.
/// Matches the sidebar's branch-row worktree pill.
const HISTORY_WORKTREE_BADGE_MAX_W_PX: f32 = 200.0;
/// Matches the history table's ref chips so the badge sits on the same rhythm.
const HISTORY_WORKTREE_BADGE_HEIGHT_PX: f32 = 18.0;

/// Where the pinned uncommitted-changes row draws its node: its column and
/// the lane colour for that dot and its connector.
///
/// When HEAD is the first visible row — the unfiltered log's usual shape —
/// the node hangs on HEAD's own lane, the lane a commit of these changes
/// would land on. Any other page (a scoped or filtered log whose first row is
/// not HEAD) keeps the historical column-0 anchor, which the row below's stub
/// has always connected through.
fn history_worktree_node_placement(
    graph_rows: Option<&[history_graph::GraphRow]>,
    head_visible_ix: Option<usize>,
) -> (usize, history_graph::LaneColorIx) {
    if let (Some(rows), Some(0)) = (graph_rows, head_visible_ix) {
        let row = &rows[0];
        return (usize::from(row.node_col), row.node_color_ix);
    }
    // Column 0 can be a hole, whose `color_ix` is a real palette index
    // rather than a lane's colour.
    let color_ix = graph_rows
        .and_then(|rows| rows.first())
        .and_then(|row| {
            row.lanes_now
                .first()
                .filter(|lane| lane.is_active())
                .map(|lane| lane.color_ix)
        })
        .unwrap_or(0);
    (0, color_ix)
}

/// The lane-coloured border down the left edge of a message cell, matching the
/// one the commit rows paint on their canvas.
///
/// Absolutely positioned so the label keeps the same left offset it has on a
/// commit row — a flow child would push the text over by the border's width.
fn history_message_border(ui_scale: ui_scale::UiScale, color: gpui::Rgba) -> impl IntoElement {
    let border_w = ui_scale.px(HISTORY_MESSAGE_BORDER_W_PX);
    let inset_y = ui_scale.px(HISTORY_MESSAGE_BORDER_INSET_Y_PX);
    div()
        .absolute()
        .left_0()
        .top(inset_y)
        .bottom(inset_y)
        .w(border_w)
        .rounded(border_w * 0.5)
        .bg(color)
}

fn history_row_height(ui_scale: ui_scale::UiScale, cx: &mut impl gpui::BorrowAppContext) -> Pixels {
    crate::view::components::history_row_height(crate::density::current(cx).density, ui_scale)
}

fn history_scope_shows_graph_color_marker(scope: worktree_core::domain::LogScope) -> bool {
    !matches!(scope, worktree_core::domain::LogScope::FirstParent)
}

#[allow(clippy::too_many_arguments)]
fn history_table_row(
    theme: AppTheme,
    ui_scale: ui_scale::UiScale,
    col_branch: Pixels,
    col_graph: Pixels,
    col_author: Pixels,
    col_date: Pixels,
    col_sha: Pixels,
    show_graph: bool,
    show_author: bool,
    show_date: bool,
    show_sha: bool,
    show_graph_color_marker: bool,
    ix: usize,
    repo_id: RepoId,
    commit: &Commit,
    graph_rows: Arc<[history_graph::GraphRow]>,
    graph_row_ix: usize,
    connect_from_top_col: Option<usize>,
    tag_names: Arc<[HistoryTextVm]>,
    ref_items: Arc<[HistoryRefListItem]>,
    selected_branch: Option<SelectedHistoryBranch>,
    // Colour index of the lane the selection sits on; every other lane washes
    // out. A property of the lane, not of this row.
    selected_lane: Option<super::history_graph_paint::SelectedLane>,
    // Whether this row's commit is reachable from the selection — merges
    // included, so rows off the selected lane count too.
    related_to_selection: Option<bool>,
    // Branch this commit belongs to, shown as a faded badge while the row is
    // hovered. Inherited down the lane, so unlabelled commits have one too.
    lane_branch_name: Option<SharedString>,
    author: HistoryTextVm,
    // Email behind `commit.author`, resolved from the repo's author→email map
    // by the caller; drives the remote-avatar overlay when a source other than
    // initials is active.
    author_email: Option<&str>,
    summary: HistoryTextVm,
    when: HistoryTextVm,
    short_sha: HistoryTextVm,
    selected: bool,
    is_head: bool,
    is_stash_node: bool,
    // Bisect verdict this commit carries (✗ bad / ✓ good / ⊘ skip), and
    // whether it is the candidate currently checked out for testing (◆).
    // Both `None`/false while no session runs.
    bisect_mark: Option<BisectVerdict>,
    bisect_current: bool,
    active_context_menu_invoker: Option<&SharedString>,
    cx: &mut gpui::Context<HistoryView>,
) -> AnyElement {
    let context_menu_invoker: SharedString =
        format!("history_commit_menu_{}_{}", repo_id.0, commit.id.as_ref()).into();
    let context_menu_active = active_context_menu_invoker == Some(&context_menu_invoker);
    // The row's background as one value rather than three `.bg()` calls that
    // overwrite each other, because the graph canvas needs to know it: its icon
    // nodes knock their glyphs out in the colour the row is actually painted,
    // and a knockout in the untinted surface leaves a visible patch inside a
    // tinted row. The hover tint is the canvas's business -- it owns the hitbox
    // -- so it is not folded in here.
    let row_bg_overlay = if context_menu_active {
        Some(theme.colors.interaction.pressed_background)
    } else if selected {
        Some(theme.colors.accent.subtle_background)
    } else if is_head {
        // A quiet tint keeps HEAD findable without competing with selection.
        Some(with_alpha(theme.colors.accent.foreground, 0.06))
    } else {
        None
    };
    // One decision per row build, shared with the canvas: when the remote
    // avatar has resolved to pixels the overlay img owns the avatar slot and
    // the canvas skips its initials; while it is pending (or failed — `d=404`)
    // the canvas paints the initials and the resolver stays armed.
    let remote_avatar = crate::avatar_source::remote_avatar(author_email);
    if remote_avatar.is_none()
        && let Some(url) = crate::avatar_source::avatar_url(author_email)
    {
        crate::avatar_source::ensure_avatar_loaded(&url, cx);
    }
    let commit_row = history_canvas::history_commit_row_canvas(
        theme,
        cx.entity(),
        ix,
        repo_id,
        commit.id.clone(),
        col_branch,
        col_graph,
        col_author,
        col_date,
        col_sha,
        show_graph,
        show_author,
        show_date,
        show_sha,
        show_graph_color_marker,
        is_stash_node,
        connect_from_top_col,
        graph_rows,
        graph_row_ix,
        tag_names,
        ref_items,
        selected_branch,
        selected_lane,
        related_to_selection,
        lane_branch_name,
        author,
        summary,
        when,
        short_sha,
        commit.signed,
        bisect_mark,
        bisect_current,
        remote_avatar.is_some(),
        row_bg_overlay,
        if context_menu_active {
            theme.colors.interaction.pressed_background
        } else {
            theme.colors.interaction.hover_background
        },
    );

    let commit_id = commit.id.clone();
    let row_height = history_row_height(ui_scale, cx);
    let mut row = div()
        .id(ix)
        .debug_selector(move || format!("history_row_{ix}"))
        .relative()
        .h(row_height)
        .w_full()
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| {
            if context_menu_active {
                s.bg(theme.colors.interaction.pressed_background)
            } else {
                s.bg(theme.colors.interaction.hover_background)
            }
        })
        .active(move |s| s.bg(theme.colors.interaction.pressed_background))
        .child(commit_row)
        // Selecting on press, like the sidebar rows: the row the gesture
        // *starts* on owns it, so a release that merely drifted here — the end
        // of a text-selection drag in the details pane, say — selects nothing.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, _w, cx| {
                let modifiers = e.modifiers;
                let mode = if modifiers.shift {
                    CommitSelectMode::Range
                } else if modifiers.secondary() || modifiers.control || modifiers.platform {
                    CommitSelectMode::Toggle
                } else {
                    CommitSelectMode::Single
                };
                let visible_order = (mode == CommitSelectMode::Range)
                    .then(|| this.visible_commit_ids_for_repo(repo_id))
                    .flatten();
                this.store.dispatch(Msg::SelectCommitMulti {
                    repo_id,
                    commit_id: commit_id.clone(),
                    mode,
                    clicked_index: Some(graph_row_ix),
                    visible_order,
                });
                cx.notify();
            }),
        );

    // Remote avatar overlay: when the image has resolved, the img owns the
    // avatar slot — the canvas paints nothing beneath it (a transparent image
    // would show both layers at once); pending and failed loads leave the
    // slot to the canvas's initials circle. The metrics helper is the one the
    // canvas paints from, pinning the overlay to the column layout.
    if let Some((_url, image)) = remote_avatar
        && show_author
        && let Some(metrics) = history_canvas::history_avatar_metrics(
            show_sha,
            show_date,
            col_sha,
            col_date,
            col_author,
            // The canvas layout pads the row by half a rem; `rem = 16 design
            // px * scale`, so the pad is 8 scaled design px.
            ui_scale.px(8.0),
            ui_scale.px(HISTORY_COL_HANDLE_PX / 2.0),
            ui_scale.px(components::AVATAR_DIAMETER_PX),
            row_height,
        )
    {
        row = row.child(
            div()
                .absolute()
                .top(metrics.inset_from_top)
                // `inset_from_right` measures to the slot's LEFT edge (the
                // canvas identity pins that); `.right()` positions this
                // element's right edge, so the diameter comes off first.
                .right(metrics.inset_from_right - metrics.diameter)
                .child(
                    // The img rounds its own painted quad from its style's
                    // corner radii (see `Img::paint`); a rounded wrapper div
                    // would not clip it to a circle.
                    gpui::img(image)
                        .w(metrics.diameter)
                        .h(metrics.diameter)
                        .rounded(metrics.diameter * 0.5),
                ),
        );
    }

    if let Some(overlay) = row_bg_overlay {
        row = row.bg(overlay);
    }

    // On light themes the selection tint lands within a few percent of the list
    // surface, so a selected row is a smudge rather than a marked row. Ring it
    // the way selected sidebar rows already are.
    if selected && let Some(outline) = components::light_theme_selection_outline(theme) {
        row = row.shadow(vec![outline]);
    }

    if is_head {
        row = row.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(ui_scale.px(3.0))
                .bg(with_alpha(theme.colors.accent.foreground, 0.90)),
        );
    }

    row.into_any_element()
}

/// One linked worktree's uncommitted changes, rendered directly above the commit
/// that worktree has checked out.
///
/// Unlike the working-tree summary row this one is not pinned to the top of the
/// list, so it paints the full lane band of the commit below it rather than a
/// single connector stub: the lanes have to run through it uninterrupted.
#[allow(clippy::too_many_arguments)]
fn worktree_uncommitted_history_row(
    theme: AppTheme,
    ui_scale: ui_scale::UiScale,
    col_branch: Pixels,
    col_graph: Pixels,
    col_author: Pixels,
    col_date: Pixels,
    col_sha: Pixels,
    show_graph: bool,
    show_author: bool,
    show_date: bool,
    show_sha: bool,
    graph_row: &history_graph::GraphRow,
    // Index of the commit row the band sits on top of, whose lanes it draws.
    visible_ix: usize,
    connect_from_top_col: Option<usize>,
    selected_lane: Option<super::history_graph_paint::SelectedLane>,
    // Whether the commit row the band sits on belongs to the selection.
    related_to_selection: Option<bool>,
    show_graph_color_marker: bool,
    repo_id: RepoId,
    list_ix: usize,
    selected: bool,
    counts: (usize, usize, usize),
    summary: &worktree_core::domain::WorktreeDirtySummary,
    cx: &mut gpui::Context<HistoryView>,
) -> AnyElement {
    let scaled_px = |value| ui_scale.px(value);
    let cell_pad_x = scaled_px(HISTORY_COL_HANDLE_PX / 2.0);
    let band_node = super::history_graph_paint::band_node_for(
        graph_row,
        summary.branch.is_some() && !summary.detached,
    );
    // A row the selection reaches keeps its full lane colour — the band is on
    // that branch when its commit is — otherwise it washes with the lane.
    let node_color = if related_to_selection == Some(true) {
        history_graph::lane_color(theme, band_node.color_ix)
    } else {
        super::history_graph_paint::lane_wash_color(
            theme,
            band_node.color_ix,
            visible_ix,
            selected_lane,
        )
    };
    // Everything on the row follows the selection's reach, text included.
    let label_color = history_canvas::selection_related_summary_color(theme, related_to_selection);

    // A pass-through band: whatever entered the commit below from above runs
    // straight through this row, so inserting it leaves the graph unbroken.
    // `None` when the node sits on a lane of its own already (a branch head's
    // fork), which needs only a straight connector down.
    let node_exit_col = (band_node.exit_col != band_node.col).then_some(band_node.exit_col);
    // Only the commit's lanes cross into the band; everything else about its row
    // (`lanes_next`, `joins_in`, `edges_out`) belongs to the commit and is never
    // painted here, so the band carries the lanes alone rather than a row-shaped
    // copy of them.
    let band_lanes = graph_row.lanes_now.clone();
    // The node's middle is opaque, so it has to be filled in what the row is
    // painted over rather than in the list's bare surface.
    let row_background = if selected {
        crate::theme::composite_over(
            theme.colors.surface.canvas,
            theme.colors.accent.subtle_background,
        )
    } else {
        theme.colors.surface.canvas
    };
    let graph = gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            super::history_graph_paint::paint_history_graph_band(
                theme,
                &band_lanes,
                visible_ix,
                connect_from_top_col,
                selected_lane,
                super::history_graph_paint::BandNodePaint {
                    col: band_node.col,
                    color: node_color,
                    exit_col: node_exit_col,
                },
                show_graph_color_marker,
                row_background,
                bounds,
                window,
                cx,
            );
        },
    )
    .w_full()
    .h_full();

    let icon_count = |icon_path: &'static str, color: gpui::Rgba, count: usize| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(svg_icon(icon_path, color, scaled_px(12.0)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(count.to_string()),
            )
            .into_any_element()
    };
    let (added, modified, deleted) = counts;
    let mut parts: Vec<AnyElement> = Vec::with_capacity(3);
    if modified > 0 {
        parts.push(icon_count(
            "icons/pencil.svg",
            theme.colors.status.warning.foreground,
            modified,
        ));
    }
    if added > 0 {
        parts.push(icon_count(
            "icons/plus.svg",
            theme.colors.status.success.foreground,
            added,
        ));
    }
    if deleted > 0 {
        parts.push(icon_count(
            "icons/minus.svg",
            theme.colors.status.danger.foreground,
            deleted,
        ));
    }

    let palette = super::sidebar::worktree_badge_palette(theme);
    let badge_label = super::sidebar::worktree_origin_label(
        summary.branch.as_deref(),
        summary.detached,
        &summary.path,
    );
    let open_path = summary.path.clone();
    let badge_tooltip: SharedString =
        format!("Open this worktree\n{}", summary.path.display()).into();

    let badge = super::sidebar::worktree_origin_chip(
        theme,
        badge_label,
        scaled_px(9.0),
        scaled_px(HISTORY_WORKTREE_BADGE_HEIGHT_PX),
        scaled_px(HISTORY_WORKTREE_BADGE_MAX_W_PX),
        scaled_px(6.0),
    )
    .id(("history_worktree_badge", list_ix))
    .cursor(CursorStyle::PointingHand)
    .hover(move |s| {
        s.border_color(palette.hover_border)
            .text_color(palette.hover_text)
    })
    .worktree_tooltip(theme, badge_tooltip)
    // The badge is a control of its own: a right or middle click must not open
    // the repo, and a left click on it must not also select the row underneath
    // -- the row belongs to the repo we are navigating away from.
    .on_click(cx.listener(move |this, e: &ClickEvent, _w, cx| {
        if !e.standard_click() {
            return;
        }
        cx.stop_propagation();
        this.store.dispatch(Msg::OpenRepo(open_path.clone()));
        cx.notify();
    }));

    let select_path = summary.path.clone();
    let mut row = div()
        .id(("history_worktree_uncommitted", list_ix))
        .h(history_row_height(ui_scale, cx))
        .flex()
        .w_full()
        .items_center()
        .px_2()
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| s.bg(theme.colors.interaction.hover_background))
        .active(move |s| s.bg(theme.colors.interaction.pressed_background))
        .on_click(cx.listener(move |this, e: &ClickEvent, _w, cx| {
            if !e.standard_click() {
                return;
            }
            this.store.dispatch(Msg::SelectWorktreeUncommitted {
                repo_id,
                path: select_path.clone(),
            });
            cx.notify();
        }))
        .child(
            div()
                .w(col_branch)
                .text_xs()
                .line_clamp(1)
                .whitespace_nowrap()
                .child(div()),
        )
        .when(show_graph, |row| {
            row.child(div().w(col_graph).h_full().overflow_hidden().child(graph))
        })
        .child({
            let mut summary = div()
                .relative()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .gap_2()
                // Same offset the commit rows put their text at, so the message
                // column reads as one column down the whole list.
                .pl(ui_scale.px(history_message_text_left_px(show_graph_color_marker)))
                .pr(cell_pad_x)
                .when(show_graph_color_marker, |cell| {
                    cell.child(history_message_border(ui_scale, node_color))
                });
            summary = summary.child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(label_color)
                    .line_clamp(1)
                    .whitespace_nowrap()
                    .child(crate::i18n::tr("layout.worktree.title")),
            );
            if !parts.is_empty() {
                summary = summary.child(div().flex().items_center().gap_2().children(parts));
            }
            summary.child(div().flex_1().min_w(px(0.0))).child(badge)
        })
        .when(show_author, |row| row.child(div().w(col_author)))
        .when(show_date, |row| row.child(div().w(col_date)))
        .when(show_sha, |row| row.child(div().w(col_sha)));

    if selected {
        row = row.bg(theme.colors.accent.subtle_background);
        // Same light-theme selection ring the commit rows wear.
        if let Some(outline) = components::light_theme_selection_outline(theme) {
            row = row.shadow(vec![outline]);
        }
    }

    row.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn working_tree_summary_history_row(
    theme: AppTheme,
    ui_scale: ui_scale::UiScale,
    col_branch: Pixels,
    col_graph: Pixels,
    col_author: Pixels,
    col_date: Pixels,
    col_sha: Pixels,
    show_graph: bool,
    show_author: bool,
    show_date: bool,
    show_sha: bool,
    node_col: usize,
    node_color_ix: history_graph::LaneColorIx,
    selected_lane: Option<super::history_graph_paint::SelectedLane>,
    // Whether the HEAD these changes sit on belongs to the selection.
    related_to_selection: Option<bool>,
    show_graph_color_marker: bool,
    repo_id: RepoId,
    selected: bool,
    counts: (usize, usize, usize),
    cx: &mut gpui::Context<HistoryView>,
) -> AnyElement {
    let scaled_px = |value| ui_scale.px(value);
    let cell_pad_x = scaled_px(HISTORY_COL_HANDLE_PX / 2.0);
    // The connector keeps its full lane colour when the changes belong to the
    // selection, and washes with its lane otherwise. The pinned row sits above
    // the newest commit, so it shares row 0's lanes.
    let node_color = if related_to_selection == Some(true) {
        history_graph::lane_color(theme, node_color_ix)
    } else {
        super::history_graph_paint::lane_wash_color(theme, node_color_ix, 0, selected_lane)
    };
    let label_color = history_canvas::selection_related_summary_color(theme, related_to_selection);
    let icon_count = |icon_path: &'static str, color: gpui::Rgba, count: usize| {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(svg_icon(icon_path, color, scaled_px(12.0)))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(count.to_string()),
            )
            .into_any_element()
    };

    let (added, modified, deleted) = counts;
    let mut parts: Vec<AnyElement> = Vec::with_capacity(3);
    if modified > 0 {
        parts.push(icon_count(
            "icons/pencil.svg",
            theme.colors.status.warning.foreground,
            modified,
        ));
    }
    if added > 0 {
        parts.push(icon_count(
            "icons/plus.svg",
            theme.colors.status.success.foreground,
            added,
        ));
    }
    if deleted > 0 {
        parts.push(icon_count(
            "icons/minus.svg",
            theme.colors.status.danger.foreground,
            deleted,
        ));
    }

    // What the row is *actually* painted over, so the node's opaque middle hides
    // the lane running through its column without leaving an untinted disc
    // punched into a selected row. Same compositing the linked-worktree band row
    // does; the hover tint stays out of it, being the div's business here.
    let node_background = if selected {
        crate::theme::composite_over(
            theme.colors.surface.canvas,
            theme.colors.accent.subtle_background,
        )
    } else {
        theme.colors.surface.canvas
    };
    let circle = gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, cx| {
            use gpui::{PathBuilder, point};
            let design_scale_factor = ui_scale::design_scale_factor_from_window(window);
            let scaled_px = |value| px(value * design_scale_factor);
            let margin_x = scaled_px(HISTORY_GRAPH_MARGIN_X_PX);
            let col_gap = scaled_px(HISTORY_GRAPH_COL_GAP_PX);
            let node_x = margin_x + col_gap * node_col as f32;
            let center = point(
                bounds.left() + node_x,
                bounds.top() + bounds.size.height / 2.0,
            );

            // Connect the working tree node into the history graph below.
            let stroke_width = scaled_px(1.6);
            let mut path = PathBuilder::stroke(stroke_width);
            path.move_to(point(center.x, center.y));
            path.line_to(point(center.x, bounds.bottom()));
            if let Ok(p) = path.build() {
                window.paint_path(p, node_color);
            }

            if show_graph_color_marker {
                super::history_graph_paint::paint_graph_fade(
                    node_color,
                    bounds,
                    scaled_px(HISTORY_GRAPH_FADE_WIDTH_PX),
                    window,
                );
            }

            super::history_graph_paint::paint_ring_icon_node(
                center.x,
                center.y,
                icons::UNCOMMITTED_NODE_ICON_PATH,
                node_color,
                node_background,
                window,
                cx,
            );
        },
    )
    .w_full()
    .h_full()
    .cursor(CursorStyle::PointingHand);

    let mut row = div()
        .id(("history_worktree_summary", repo_id.0))
        .h(history_row_height(ui_scale, cx))
        .flex()
        .w_full()
        .items_center()
        .px_2()
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| s.bg(theme.colors.interaction.hover_background))
        .active(move |s| s.bg(theme.colors.interaction.pressed_background))
        .child(
            div()
                .w(col_branch)
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .line_clamp(1)
                .whitespace_nowrap()
                .child(div()),
        )
        .when(show_graph, |row| {
            row.child(
                div()
                    .w(col_graph)
                    .h_full()
                    .flex()
                    .justify_center()
                    .overflow_hidden()
                    .child(circle),
            )
        })
        .child({
            let mut summary = div()
                .relative()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .gap_2()
                // Same offset the commit rows put their text at, so the message
                // column reads as one column down the whole list.
                .pl(ui_scale.px(history_message_text_left_px(show_graph_color_marker)))
                .pr(cell_pad_x)
                .when(show_graph_color_marker, |cell| {
                    cell.child(history_message_border(ui_scale, node_color))
                });
            summary = summary.child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_sm()
                    .text_color(label_color)
                    .line_clamp(1)
                    .whitespace_nowrap()
                    .child(crate::i18n::tr("tail.history.uncommitted_changes")),
            );
            if !parts.is_empty() {
                summary = summary.child(div().flex().items_center().gap_2().children(parts));
            }
            summary
        })
        .when(show_author, |row| row.child(div().w(col_author)))
        .when(show_date, |row| {
            row.child(
                div()
                    .w(col_date)
                    .flex()
                    .justify_end()
                    .px(cell_pad_x)
                    .text_xs()
                    .font_family(UI_MONOSPACE_FONT_FAMILY)
                    .text_color(theme.colors.foreground.secondary)
                    .whitespace_nowrap()
                    .child(crate::i18n::tr("tail.history.click_to_review")),
            )
        })
        .when(show_sha, |row| row.child(div().w(col_sha)))
        .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
            this.store
                .dispatch(Msg::SelectWorkingTreeSummary { repo_id });
            cx.notify();
        }));

    if selected {
        row = row.bg(theme.colors.accent.subtle_background);
        // Same light-theme selection ring the commit rows wear.
        if let Some(outline) = components::light_theme_selection_outline(theme) {
            row = row.shadow(vec![outline]);
        }
    }

    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::super::markdown_preview::{
        MarkdownPreviewImageSource, MarkdownPreviewPictureSizes,
        markdown_preview_alert_title_label, markdown_preview_expanded_slice_range,
        markdown_preview_image_source, markdown_preview_inline_highlight,
        markdown_preview_no_picture_sizes, markdown_preview_picture_skeleton,
        markdown_preview_row_background, markdown_preview_row_height,
        markdown_preview_row_horizontal_padding, markdown_preview_row_layout,
        markdown_preview_row_marker, markdown_preview_row_styled_text,
        markdown_preview_row_typography,
    };
    use super::{
        DiffSearchMatchEmphasis, MarkdownChangeHint, MarkdownInlineStyle,
        build_cached_diff_styled_text, history_message_text_left_px,
        history_scope_shows_graph_color_marker, history_worktree_node_placement,
        worktree_preview_apply_query_overlay,
    };
    use crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY;
    use crate::view::markdown_preview::{
        MarkdownInlineSpan, MarkdownPreviewRow, MarkdownPreviewRowKind,
    };
    use crate::view::panes::main::diff_search::{DiffSearchMatcher, DiffSearchOptions};
    use crate::view::rows::diff_text::DIFF_WRAP_TAB_EXPANDED_COLUMNS;
    use crate::view::{AppTheme, DateTimeFormat, Timezone, format_datetime, format_datetime_utc};
    use crate::view::{
        HISTORY_COL_HANDLE_PX, HISTORY_MESSAGE_BORDER_GAP_PX, HISTORY_MESSAGE_BORDER_W_PX,
    };
    use gpui::{FontWeight, SharedString, px};
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};
    use worktree_core::domain::LogScope;

    fn markdown_row(kind: MarkdownPreviewRowKind) -> MarkdownPreviewRow {
        MarkdownPreviewRow {
            kind,
            text: SharedString::from("text"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 0,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        }
    }

    #[test]
    fn worktree_preview_query_overlay_honors_search_options_for_cached_rows() {
        let theme = AppTheme::worktree_dark();
        let base = build_cached_diff_styled_text(
            theme,
            "Render render cat concat cat",
            &[],
            "",
            None,
            super::DiffSyntaxMode::Auto,
            None,
        );

        let case_sensitive_options = DiffSearchOptions {
            match_case: true,
            ..Default::default()
        };
        let case_sensitive_matcher = DiffSearchMatcher::new("render", case_sensitive_options);
        let case_sensitive = worktree_preview_apply_query_overlay(
            theme,
            base.clone(),
            Some(&case_sensitive_matcher),
            DiffSearchMatchEmphasis::Other,
        );
        let case_sensitive_ranges: Vec<_> = case_sensitive
            .highlights
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        assert_eq!(case_sensitive_ranges, vec![7..13]);

        let whole_word_options = DiffSearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let whole_word_matcher = DiffSearchMatcher::new("cat", whole_word_options);
        let whole_word = worktree_preview_apply_query_overlay(
            theme,
            base.clone(),
            Some(&whole_word_matcher),
            DiffSearchMatchEmphasis::Other,
        );
        let whole_word_ranges: Vec<_> = whole_word
            .highlights
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        assert_eq!(whole_word_ranges, vec![14..17, 25..28]);

        let regex_options = DiffSearchOptions {
            regex: true,
            ..Default::default()
        };
        let regex_matcher = DiffSearchMatcher::new(r"r.n.e.", regex_options);
        let regex = worktree_preview_apply_query_overlay(
            theme,
            base,
            Some(&regex_matcher),
            DiffSearchMatchEmphasis::Other,
        );
        let regex_ranges: Vec<_> = regex
            .highlights
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        assert_eq!(regex_ranges, vec![0..6, 7..13]);
    }

    /// The working-tree row borrows the lane colour *index* of the first commit
    /// so its connector can be washed like any other lane, rather than taking a
    /// resolved colour it could no longer compare against the selection.
    /// The commit rows paint their text on a canvas and the two
    /// uncommitted-changes rows lay theirs out as elements, so the offset they
    /// agree on has to come from one place — otherwise the message column steps
    /// sideways at every synthetic row.
    #[test]
    fn the_message_text_clears_the_lane_border_by_a_fixed_gap() {
        assert_eq!(
            history_message_text_left_px(true),
            HISTORY_MESSAGE_BORDER_W_PX + HISTORY_MESSAGE_BORDER_GAP_PX
        );
        assert!(
            history_message_text_left_px(true) > HISTORY_MESSAGE_BORDER_W_PX,
            "text that starts inside the border reads as touching it"
        );
    }

    /// Without the border there is nothing to clear, so the cell's own padding
    /// applies and the text does not jump left when the marker is off.
    #[test]
    fn the_message_text_falls_back_to_the_cell_padding_without_a_border() {
        assert_eq!(
            history_message_text_left_px(false),
            HISTORY_COL_HANDLE_PX / 2.0
        );
    }

    #[test]
    fn history_worktree_node_color_falls_back_to_the_primary_lane() {
        assert_eq!(history_worktree_node_placement(None, None), (0, 0));
    }

    /// A first row whose column 0 is a hole is exactly the case the colour
    /// fallback guards: the hole's `color_ix` is a palette index, not a lane.
    #[test]
    fn history_worktree_node_falls_back_to_column_zero_with_a_real_lane_colour() {
        let row = test_graph_row(&[super::history_graph::LanePaint::HOLE], 4, 2);
        let rows = [row];
        // The hole at column 0 is skipped for the colour; the column stays 0.
        assert_eq!(history_worktree_node_placement(Some(&rows), None), (0, 0));
    }

    /// The common page: HEAD is the first visible row, so the uncommitted
    /// node hangs on HEAD's own lane -- both its column and its dot colour.
    #[test]
    fn history_worktree_node_sits_on_the_head_lane_when_head_leads_the_page() {
        let row = test_graph_row(
            &[super::history_graph::LanePaint::lane(5, true, false)],
            0,
            5,
        );
        let rows = [row];
        assert_eq!(
            history_worktree_node_placement(Some(&rows), Some(0)),
            (0, 5),
            "HEAD's node column and colour come straight off its row"
        );
        let head_on_col_2 = test_graph_row(
            &[
                super::history_graph::LanePaint::HOLE,
                super::history_graph::LanePaint::HOLE,
                super::history_graph::LanePaint::lane(1, true, false),
            ],
            2,
            1,
        );
        let rows = [head_on_col_2];
        assert_eq!(
            history_worktree_node_placement(Some(&rows), Some(0)),
            (2, 1),
            "a HEAD pushed off column 0 takes its lane's column with it"
        );
    }

    /// A scoped or filtered log can lead with a row that is not HEAD; the
    /// node keeps its historical column-0 anchor there, because the stub the
    /// row below draws upward has always connected through that column.
    #[test]
    fn history_worktree_node_stays_on_column_zero_when_head_is_not_the_first_row() {
        let row = test_graph_row(
            &[super::history_graph::LanePaint::lane(3, true, false)],
            1,
            3,
        );
        let rows = [row.clone(), row];
        assert_eq!(
            history_worktree_node_placement(Some(&rows), Some(1)),
            (0, 3),
            "HEAD visible but not first: column 0, colour from row 0's first live lane"
        );
    }

    fn test_graph_row(
        lanes_now: &[super::history_graph::LanePaint],
        node_col: u16,
        node_color_ix: super::history_graph::LaneColorIx,
    ) -> super::history_graph::GraphRow {
        super::history_graph::GraphRow {
            lanes_now: lanes_now.iter().copied().collect(),
            lanes_next: Default::default(),
            joins_in: Default::default(),
            edges_out: Default::default(),
            node_col,
            node_color_ix,
            is_merge: false,
        }
    }

    #[test]
    fn history_graph_color_marker_is_shown_for_all_non_first_parent_modes() {
        assert!(history_scope_shows_graph_color_marker(
            LogScope::FullReachable
        ));
        assert!(!history_scope_shows_graph_color_marker(
            LogScope::FirstParent
        ));
        assert!(history_scope_shows_graph_color_marker(LogScope::NoMerges));
        assert!(history_scope_shows_graph_color_marker(LogScope::MergesOnly));
        assert!(history_scope_shows_graph_color_marker(
            LogScope::AllBranches
        ));
    }

    #[test]
    fn commit_date_formats_as_yyyy_mm_dd_utc() {
        assert_eq!(
            format_datetime_utc(UNIX_EPOCH, DateTimeFormat::YmdHm),
            "1970-01-01 00:00 UTC"
        );
        assert_eq!(
            format_datetime_utc(
                UNIX_EPOCH + Duration::from_secs(86_400),
                DateTimeFormat::YmdHm
            ),
            "1970-01-02 00:00 UTC"
        );
        assert_eq!(
            format_datetime_utc(
                UNIX_EPOCH - Duration::from_secs(86_400),
                DateTimeFormat::YmdHm
            ),
            "1969-12-31 00:00 UTC"
        );

        // 2000-02-29 12:34:56 UTC
        assert_eq!(
            format_datetime_utc(
                UNIX_EPOCH + Duration::from_secs(951_782_400 + 12 * 3600 + 34 * 60 + 56),
                DateTimeFormat::YmdHms
            ),
            "2000-02-29 12:34:56 UTC"
        );
    }

    #[test]
    fn format_datetime_with_timezone_offset() {
        // UTC+5:30 (19800 seconds)
        let tz = Timezone::Fixed(19800);
        assert_eq!(
            format_datetime(UNIX_EPOCH, DateTimeFormat::YmdHm, tz, true),
            "1970-01-01 05:30 UTC+5:30"
        );

        // UTC-5
        let tz_neg = Timezone::Fixed(-18000);
        assert_eq!(
            format_datetime(
                UNIX_EPOCH + Duration::from_secs(86_400),
                DateTimeFormat::YmdHm,
                tz_neg,
                true,
            ),
            "1970-01-01 19:00 UTC\u{2212}5"
        );
    }

    #[test]
    fn format_datetime_can_hide_timezone_label() {
        let tz = Timezone::Fixed(7200);
        assert_eq!(
            format_datetime(UNIX_EPOCH, DateTimeFormat::YmdHm, tz, false),
            "1970-01-01 02:00"
        );
    }

    #[test]
    fn timezone_key_round_trips() {
        for tz in Timezone::all() {
            let key = tz.key();
            let parsed = Timezone::from_key(&key);
            assert_eq!(parsed, Some(*tz), "round-trip failed for {key}");
        }
    }

    #[test]
    fn worktree_preview_renderer_avoids_full_document_prepare_calls() {
        let source = include_str!("history.rs");
        let render_start = source
            .find("fn render_worktree_preview_rows")
            .expect("render_worktree_preview_rows should exist");
        let render_end = source[render_start..]
            .find("impl HistoryView")
            .map(|offset| render_start + offset)
            .expect("HistoryView impl should follow worktree preview renderer");
        let render_source = &source[render_start..render_end];

        assert!(
            !render_source.contains("prepare_diff_syntax_document("),
            "row renderer should not build prepared syntax documents"
        );
        assert!(
            !render_source.contains("prepare_diff_syntax_document_with_budget_reuse("),
            "row renderer should not run full-document parse prep"
        );
    }

    #[test]
    fn markdown_preview_heading_typography_scales_above_body_text() {
        let theme = AppTheme::worktree_light();
        let paragraph = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Paragraph,
            text: SharedString::from("body"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 0,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };
        let h1 = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Heading { level: 1 },
            ..paragraph.clone()
        };
        let h2 = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Heading { level: 2 },
            ..paragraph.clone()
        };
        let h6 = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Heading { level: 6 },
            ..paragraph.clone()
        };

        let editor_font_family: SharedString = EDITOR_MONOSPACE_FONT_FAMILY.into();
        let body_typography = markdown_preview_row_typography(
            theme,
            &paragraph,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let h1_typography = markdown_preview_row_typography(
            theme,
            &h1,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let h2_typography = markdown_preview_row_typography(
            theme,
            &h2,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let h6_typography = markdown_preview_row_typography(
            theme,
            &h6,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );

        assert!(h1_typography.font_size > h2_typography.font_size);
        assert!(h2_typography.font_size > body_typography.font_size);
        assert!(h6_typography.font_size > body_typography.font_size);
        assert_eq!(h1_typography.font_weight, Some(FontWeight::BOLD));
        assert_eq!(h2_typography.font_weight, Some(FontWeight::BOLD));
        assert_eq!(h6_typography.font_weight, Some(FontWeight::BOLD));
    }

    #[test]
    fn markdown_preview_list_rows_match_body_line_height_and_keep_tighter_layout() {
        let theme = AppTheme::worktree_light();
        let paragraph = markdown_row(MarkdownPreviewRowKind::Paragraph);
        let list_item = markdown_row(MarkdownPreviewRowKind::ListItem { number: None });

        let editor_font_family: SharedString = EDITOR_MONOSPACE_FONT_FAMILY.into();
        let paragraph_typography = markdown_preview_row_typography(
            theme,
            &paragraph,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let list_typography = markdown_preview_row_typography(
            theme,
            &list_item,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let paragraph_layout =
            markdown_preview_row_layout(&paragraph, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);
        let list_layout =
            markdown_preview_row_layout(&list_item, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);

        assert_eq!(
            list_typography.line_height,
            paragraph_typography.line_height
        );
        assert!(paragraph_layout.bottom_inset_px > list_layout.bottom_inset_px);
    }

    #[test]
    fn markdown_preview_details_summary_rows_are_bold_and_marked() {
        let theme = AppTheme::worktree_light();
        let row = markdown_row(MarkdownPreviewRowKind::DetailsSummary);

        let editor_font_family: SharedString = EDITOR_MONOSPACE_FONT_FAMILY.into();
        let typography = markdown_preview_row_typography(
            theme,
            &row,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );

        assert_eq!(typography.font_weight, Some(FontWeight::BOLD));
        assert_eq!(
            markdown_preview_row_marker(&row)
                .as_ref()
                .map(SharedString::as_ref),
            Some("v")
        );
    }

    #[test]
    fn markdown_preview_code_rows_do_not_reserve_bottom_space_for_local_scrollbar() {
        let first_row = markdown_row(MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: false,
        });
        let last_row = markdown_row(MarkdownPreviewRowKind::CodeLine {
            is_first: false,
            is_last: true,
        });

        let first_layout =
            markdown_preview_row_layout(&first_row, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);
        let last_layout =
            markdown_preview_row_layout(&last_row, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);

        assert_eq!(first_layout.top_inset_px, 5.0);
        assert_eq!(last_layout.bottom_inset_px, 5.0);
    }

    #[test]
    fn markdown_preview_nested_code_rows_keep_small_outer_edge_gap() {
        let mut row = markdown_row(MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: false,
        });
        row.indent_level = 3;

        let padding = markdown_preview_row_horizontal_padding(
            &row,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );

        assert_eq!(
            padding.left_px,
            super::super::markdown_preview::MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX
        );
        assert_eq!(
            padding.right_px,
            super::super::markdown_preview::MARKDOWN_PREVIEW_BOXED_EDGE_GAP_PX
        );
    }

    #[test]
    fn markdown_preview_row_marker_preserves_ordered_item_number() {
        let row = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::ListItem { number: Some(7) },
            text: SharedString::from("item"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 0,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };

        assert_eq!(
            markdown_preview_row_marker(&row)
                .as_ref()
                .map(SharedString::as_ref),
            Some("7.")
        );
    }

    #[test]
    fn markdown_preview_row_marker_is_none_for_blockquotes_without_list_items() {
        let row = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::BlockquoteLine,
            text: SharedString::from("quote"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 2,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };

        assert_eq!(markdown_preview_row_marker(&row), None);
    }

    #[test]
    fn markdown_preview_row_marker_uses_footnote_label_when_present() {
        let row = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Paragraph,
            text: SharedString::from("reference"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 0,
            footnote_label: Some("1".into()),
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };

        assert_eq!(
            markdown_preview_row_marker(&row)
                .as_ref()
                .map(SharedString::as_ref),
            Some("[^1]:")
        );
    }

    #[test]
    fn markdown_preview_row_marker_returns_unordered_bullet_inside_blockquote() {
        let row = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::ListItem { number: None },
            text: SharedString::from("item"),
            inline_spans: Arc::new(Vec::new()),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 1,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };

        assert_eq!(
            markdown_preview_row_marker(&row)
                .as_ref()
                .map(SharedString::as_ref),
            Some("•")
        );
    }

    #[test]
    fn markdown_preview_alert_title_label_requires_alert_start_row() {
        for (kind, label) in [
            (super::MarkdownAlertKind::Note, "NOTE"),
            (super::MarkdownAlertKind::Tip, "TIP"),
            (super::MarkdownAlertKind::Important, "IMPORTANT"),
            (super::MarkdownAlertKind::Warning, "WARNING"),
            (super::MarkdownAlertKind::Caution, "CAUTION"),
        ] {
            let mut row = markdown_row(MarkdownPreviewRowKind::BlockquoteLine);
            row.alert_kind = Some(kind);
            row.starts_alert = true;
            assert_eq!(markdown_preview_alert_title_label(&row), Some(label));

            row.starts_alert = false;
            assert_eq!(markdown_preview_alert_title_label(&row), None);
        }

        let mut row = markdown_row(MarkdownPreviewRowKind::BlockquoteLine);
        row.starts_alert = true;
        assert_eq!(markdown_preview_alert_title_label(&row), None);
    }

    #[test]
    fn markdown_preview_row_background_change_hints_override_alert_and_fallback_states() {
        let theme = AppTheme::worktree_light();

        let mut added_row = markdown_row(MarkdownPreviewRowKind::Paragraph);
        added_row.change_hint = MarkdownChangeHint::Added;

        let mut added_alert_row = added_row.clone();
        added_alert_row.alert_kind = Some(super::MarkdownAlertKind::Warning);
        assert_eq!(
            markdown_preview_row_background(theme, &added_alert_row),
            markdown_preview_row_background(theme, &added_row)
        );

        let mut removed_row = markdown_row(MarkdownPreviewRowKind::Paragraph);
        removed_row.change_hint = MarkdownChangeHint::Removed;

        let mut removed_fallback_row = removed_row.clone();
        removed_fallback_row.kind = MarkdownPreviewRowKind::PlainFallback;
        assert_eq!(
            markdown_preview_row_background(theme, &removed_fallback_row),
            markdown_preview_row_background(theme, &removed_row)
        );
    }

    #[test]
    fn markdown_preview_row_background_uses_alert_and_fallback_only_when_unchanged() {
        let theme = AppTheme::worktree_dark();

        let plain_row = markdown_row(MarkdownPreviewRowKind::Paragraph);
        assert_eq!(markdown_preview_row_background(theme, &plain_row), None);

        let mut alert_row = plain_row.clone();
        alert_row.alert_kind = Some(super::MarkdownAlertKind::Tip);

        let fallback_row = markdown_row(MarkdownPreviewRowKind::PlainFallback);
        let alert_bg = markdown_preview_row_background(theme, &alert_row);
        let fallback_bg = markdown_preview_row_background(theme, &fallback_row);

        assert!(alert_bg.is_some());
        assert!(fallback_bg.is_some());
        assert_ne!(alert_bg, fallback_bg);
    }

    #[test]
    fn markdown_preview_row_styled_text_maps_inline_styles_and_skips_normal_spans() {
        let theme = AppTheme::worktree_light();

        let mut row = markdown_row(MarkdownPreviewRowKind::Paragraph);
        row.text = SharedString::from("link under strike plain");
        row.inline_spans = Arc::new(vec![
            MarkdownInlineSpan {
                byte_range: 0..4,
                style: MarkdownInlineStyle::Link,
                link_url: None,
            },
            MarkdownInlineSpan {
                byte_range: 5..10,
                style: MarkdownInlineStyle::Underline,
                link_url: None,
            },
            MarkdownInlineSpan {
                byte_range: 11..17,
                style: MarkdownInlineStyle::Strikethrough,
                link_url: None,
            },
            MarkdownInlineSpan {
                byte_range: 18..23,
                style: MarkdownInlineStyle::Normal,
                link_url: None,
            },
        ]);

        let styled = markdown_preview_row_styled_text(theme, &row);
        let highlights = styled.highlights.as_ref();

        assert_eq!(styled.text.as_ref(), "link under strike plain");
        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].0, 0..4);
        assert_eq!(
            highlights[0].1,
            markdown_preview_inline_highlight(theme, MarkdownInlineStyle::Link)
        );
        assert_eq!(highlights[1].0, 5..10);
        assert_eq!(
            highlights[1].1,
            markdown_preview_inline_highlight(theme, MarkdownInlineStyle::Underline)
        );
        assert_eq!(highlights[2].0, 11..17);
        assert_eq!(
            highlights[2].1,
            markdown_preview_inline_highlight(theme, MarkdownInlineStyle::Strikethrough)
        );
    }

    #[test]
    fn wrapped_slices_map_onto_the_tab_expanded_painted_text() {
        // Wrap ranges are measured on `row.text`, where a tab is one byte, but
        // the painted text expands each tab to four spaces. Slicing the
        // painted text with raw offsets shifted every wrapped row and dropped
        // the tail of the line.
        let raw = "\tab\tcd";
        let expanded_len =
            raw.len() + raw.matches('\t').count() * (DIFF_WRAP_TAB_EXPANDED_COLUMNS - 1);

        // "\tab" -> "    ab", "\tcd" -> "    cd"
        assert_eq!(
            markdown_preview_expanded_slice_range(raw, expanded_len, &(0..3)),
            0..6
        );
        assert_eq!(
            markdown_preview_expanded_slice_range(raw, expanded_len, &(3..raw.len())),
            6..expanded_len
        );
        // A row without tabs keeps its ranges untouched.
        assert_eq!(
            markdown_preview_expanded_slice_range("abcd", 4, &(1..3)),
            1..3
        );
    }

    #[test]
    fn image_paths_resolve_only_inside_the_documents_own_directory() {
        let dir = std::env::temp_dir().join(format!(
            "worktree_md_image_path_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let nested = dir.join("assets");
        std::fs::create_dir_all(&nested).expect("create fixture dirs");
        let image = nested.join("shot.png");
        std::fs::write(&image, b"not really a png").expect("write fixture image");
        let outside = dir.parent().expect("temp dir parent").join("outside.png");
        std::fs::write(&outside, b"not really a png").expect("write outside fixture");

        let resolve = |source: &str| markdown_preview_image_source(Some(dir.as_path()), source);
        let file = |path: &std::path::Path| Some(MarkdownPreviewImageSource::File(path.to_owned()));
        let remote = |url: &str| {
            Some(MarkdownPreviewImageSource::Remote(SharedString::from(
                url.to_owned(),
            )))
        };

        assert_eq!(resolve("assets/shot.png"), file(&image));
        assert_eq!(resolve("./assets/shot.png"), file(&image));
        // Query and fragment suffixes are common in markdown image sources and
        // are not part of the file name.
        assert_eq!(resolve("assets/shot.png?v=2"), file(&image));
        assert_eq!(resolve("assets/shot.png#frag"), file(&image));

        // Badges and hosted screenshots resolve to the URL, query string and
        // all — that is what identifies the image.
        assert_eq!(
            resolve("https://img.shields.io/badge/a-b.svg?logo=x"),
            remote("https://img.shields.io/badge/a-b.svg?logo=x")
        );
        assert_eq!(
            resolve("http://example.com/a.png"),
            remote("http://example.com/a.png")
        );
        // Remote sources resolve without a base directory, since nothing is
        // resolved against the document's location.
        assert_eq!(
            markdown_preview_image_source(None, "https://example.com/a.png"),
            remote("https://example.com/a.png")
        );

        // A file that exists but sits outside the document's tree is refused,
        // so document content cannot aim the preview at arbitrary files.
        assert_eq!(resolve("../outside.png"), None);
        // Schemes a preview has no business dereferencing.
        assert_eq!(resolve("data:image/png;base64,AAAA"), None);
        assert_eq!(resolve("file:///etc/passwd"), None);
        assert_eq!(resolve("javascript:alert(1)"), None);
        // Missing files, empty sources, and a missing base directory resolve
        // to nothing.
        assert_eq!(resolve("assets/absent.png"), None);
        assert_eq!(resolve("   "), None);
        assert_eq!(markdown_preview_image_source(None, "assets/shot.png"), None);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
    }

    /// A picture row carrying `source`, and whatever size the document declared.
    fn picture_row(
        source: &str,
        width_px: Option<u32>,
        height_px: Option<u32>,
    ) -> MarkdownPreviewRow {
        let mut row = markdown_row(MarkdownPreviewRowKind::Image {
            slice_ix: 0,
            slice_count: 8,
        });
        row.image = Some(Arc::new(crate::view::markdown_preview::MarkdownImage {
            source: SharedString::from(source.to_owned()),
            width_px,
            height_px,
        }));
        row
    }

    fn measured(source: &str, width: u32, height: u32) -> MarkdownPreviewPictureSizes {
        Arc::new(
            [(SharedString::from(source.to_owned()), (width, height))]
                .into_iter()
                .collect(),
        )
    }

    #[test]
    fn a_skeleton_holds_the_box_the_picture_will_fill() {
        // The whole point of measuring a picture's header is that the space it
        // is going to take is reserved before it has been decoded, so the
        // document does not jump when it arrives.
        let empty = markdown_preview_no_picture_sizes();

        // Read from the file: the picture's own pixels, which is what an
        // undeclared picture lays out at.
        let skeleton = markdown_preview_picture_skeleton(
            &picture_row("demo.gif", None, None),
            100,
            &measured("demo.gif", 1280, 720),
        );
        assert_eq!(skeleton.width, Some(px(1280.0)));
        assert_eq!(skeleton.aspect_ratio, Some(1280.0 / 720.0));

        // A declared size wins, and scales with the UI the way the picture will.
        let skeleton = markdown_preview_picture_skeleton(
            &picture_row("demo.gif", Some(200), Some(100)),
            200,
            &measured("demo.gif", 1280, 720),
        );
        assert_eq!(skeleton.width, Some(px(400.0)));
        assert_eq!(skeleton.aspect_ratio, Some(2.0));

        // Nothing to go on: fall back to the rows the parser set aside, which
        // is all the row grid ever had.
        let skeleton =
            markdown_preview_picture_skeleton(&picture_row("demo.gif", None, None), 100, empty);
        assert_eq!(skeleton.width, None);
        assert_eq!(skeleton.aspect_ratio, None);
        assert_eq!(
            skeleton.reserved_height,
            markdown_preview_row_height(100) * 8.0
        );
    }

    #[test]
    fn a_picture_is_named_the_same_way_wherever_it_is_asked_about() {
        // The element that draws a picture and the pane waiting to hear that it
        // decoded look it up in the same cache, so both have to arrive at the
        // key `gpui` filed it under. Building the element one way and the key
        // another would leave the pane waiting on an entry nobody writes.
        let path = std::path::PathBuf::from("assets").join("shot.png");
        assert_eq!(
            MarkdownPreviewImageSource::File(path.clone()).to_resource(),
            gpui::Resource::Path(path.as_path().into())
        );
        assert_eq!(
            MarkdownPreviewImageSource::Remote(SharedString::from("https://example.com/a.png"))
                .to_resource(),
            gpui::Resource::Uri(gpui::SharedUri::from(
                "https://example.com/a.png".to_owned()
            ))
        );
    }

    #[test]
    fn heading_rows_are_inset_evenly_above_and_below() {
        // Headings used to carry more space below than above, so the text rode
        // high in its row instead of sitting centred in the break.
        for level in 1..=6u8 {
            let row = markdown_row(MarkdownPreviewRowKind::Heading { level });
            let layout =
                markdown_preview_row_layout(&row, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);
            assert_eq!(
                layout.top_inset_px, layout.bottom_inset_px,
                "h{level} should be inset evenly: {layout:?}"
            );
        }
    }

    #[test]
    fn markdown_preview_row_styled_text_repairs_spans_that_split_a_multibyte_char() {
        // A span pointing inside a multi-byte character used to reach `gpui`
        // as a text run whose length splits that character, aborting the
        // process inside `str::split_at` while shaping the line.
        let theme = AppTheme::worktree_light();

        let mut row = markdown_row(MarkdownPreviewRowKind::Paragraph);
        row.text = SharedString::from("— dash —");
        row.inline_spans = Arc::new(vec![
            MarkdownInlineSpan {
                byte_range: 0..1,
                style: MarkdownInlineStyle::Bold,
                link_url: None,
            },
            MarkdownInlineSpan {
                byte_range: 6..9,
                style: MarkdownInlineStyle::Italic,
                link_url: None,
            },
        ]);

        let styled = markdown_preview_row_styled_text(theme, &row);
        let text = styled.text.as_ref();

        for (range, _) in styled.highlights.iter() {
            assert!(
                text.is_char_boundary(range.start) && text.is_char_boundary(range.end),
                "highlight {range:?} splits a char in {text:?}"
            );
        }
        assert_eq!(styled.highlights[0].0, 0..3);
    }

    #[test]
    fn markdown_preview_table_rows_use_monospace_typography_and_only_headers_are_bold() {
        let theme = AppTheme::worktree_light();
        let header = markdown_row(MarkdownPreviewRowKind::TableRow { is_header: true });
        let body = markdown_row(MarkdownPreviewRowKind::TableRow { is_header: false });

        let editor_font_family: SharedString = EDITOR_MONOSPACE_FONT_FAMILY.into();
        let header_typography = markdown_preview_row_typography(
            theme,
            &header,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );
        let body_typography = markdown_preview_row_typography(
            theme,
            &body,
            &editor_font_family,
            crate::ui_scale::DEFAULT_UI_SCALE_PERCENT,
        );

        assert_eq!(
            header_typography
                .font_family
                .as_ref()
                .map(SharedString::as_ref),
            Some(EDITOR_MONOSPACE_FONT_FAMILY)
        );
        assert_eq!(
            body_typography
                .font_family
                .as_ref()
                .map(SharedString::as_ref),
            Some(EDITOR_MONOSPACE_FONT_FAMILY)
        );
        assert_eq!(header_typography.font_weight, Some(FontWeight::BOLD));
        assert_eq!(body_typography.font_weight, None);
        assert_eq!(header_typography.font_size, body_typography.font_size);
        assert_eq!(header_typography.line_height, body_typography.line_height);
    }

    #[test]
    fn markdown_preview_code_rows_reuse_diff_syntax_highlighting() {
        let theme = AppTheme::worktree_dark();
        let row = MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::CodeLine {
                is_first: true,
                is_last: true,
            },
            text: SharedString::from("fn\tmain() { let x = 1; }"),
            inline_spans: Arc::new(Vec::new()),
            code_language: Some(crate::view::rows::DiffSyntaxLanguage::Rust),
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 1,
            blockquote_level: 0,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        };

        let dark_highlights = Arc::clone(&markdown_preview_row_styled_text(theme, &row).highlights);
        let dark = markdown_preview_row_styled_text(theme, &row);
        let light = markdown_preview_row_styled_text(AppTheme::worktree_light(), &row);

        assert_eq!(dark.text.as_ref(), "fn    main() { let x = 1; }");
        assert!(
            !dark.highlights.is_empty(),
            "code rows should reuse syntax highlights from the diff text renderer"
        );
        assert!(
            Arc::ptr_eq(&dark_highlights, &dark.highlights),
            "same-theme markdown code rows should reuse cached styled text"
        );
        assert!(
            !Arc::ptr_eq(&dark.highlights, &light.highlights),
            "light and dark markdown preview caches should stay separate"
        );
    }

    #[test]
    fn markdown_preview_spacer_rows_have_no_extra_layout_or_background() {
        let theme = AppTheme::worktree_light();
        let row = markdown_row(MarkdownPreviewRowKind::Spacer);

        let layout = markdown_preview_row_layout(&row, crate::ui_scale::DEFAULT_UI_SCALE_PERCENT);

        assert_eq!(layout.top_inset_px, 0.0);
        assert_eq!(layout.bottom_inset_px, 0.0);
        assert_eq!(markdown_preview_row_background(theme, &row), None);
        assert_eq!(markdown_preview_row_marker(&row), None);
    }
}

#[cfg(test)]
mod markdown_preview_search_tests {
    use super::{
        MarkdownPreviewQuery, markdown_preview_reveal_offset_y, markdown_preview_row_extent,
        markdown_preview_styled_row_with_query,
    };
    use crate::view::AppTheme;
    use crate::view::markdown_preview::{
        MarkdownChangeHint, MarkdownInlineSpan, MarkdownInlineStyle, MarkdownPreviewRow,
        MarkdownPreviewRowKind,
    };
    use crate::view::panes::main::diff_search::{DiffSearchMatcher, DiffSearchOptions};
    use gpui::{Bounds, point, px, size};
    use std::sync::Arc;

    fn row(text: &str, spans: Vec<MarkdownInlineSpan>) -> MarkdownPreviewRow {
        MarkdownPreviewRow {
            kind: MarkdownPreviewRowKind::Paragraph,
            text: text.to_string().into(),
            inline_spans: Arc::new(spans),
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range: 0..1,
            change_hint: MarkdownChangeHint::None,
            indent_level: 0,
            blockquote_level: 0,
            footnote_label: None,
            alert_kind: None,
            starts_alert: false,
            image: None,
            inline_images: Arc::from(Vec::new()),
            styled_text_cache: Default::default(),
            measured_width_px: Default::default(),
        }
    }

    fn query(needle: &str, current_row: Option<usize>) -> MarkdownPreviewQuery {
        MarkdownPreviewQuery {
            matcher: Arc::new(DiffSearchMatcher::new(needle, DiffSearchOptions::default())),
            current_row,
        }
    }

    #[test]
    fn reveal_centres_the_row_and_clamps_to_the_scrollable_range() {
        // Far down a long document: centre it in the viewport.
        assert_eq!(
            markdown_preview_reveal_offset_y(px(1000.0), px(20.0), px(400.0), px(2000.0), px(0.0)),
            Some(px(-810.0))
        );
        // A row near the top cannot be centred; the document stops at its top.
        assert_eq!(
            markdown_preview_reveal_offset_y(px(10.0), px(20.0), px(400.0), px(2000.0), px(-50.0)),
            Some(px(0.0))
        );
        // Past the end of the scrollable range, clamp to the bottom.
        assert_eq!(
            markdown_preview_reveal_offset_y(px(5000.0), px(20.0), px(400.0), px(600.0), px(0.0)),
            Some(px(-600.0))
        );
        // Already there: no scroll, so nothing repaints.
        assert_eq!(
            markdown_preview_reveal_offset_y(
                px(1000.0),
                px(20.0),
                px(400.0),
                px(2000.0),
                px(-810.0)
            ),
            None
        );
        // An unmeasured container has no centre to compute.
        assert_eq!(
            markdown_preview_reveal_offset_y(px(1000.0), px(20.0), px(0.0), px(2000.0), px(0.0)),
            None
        );
    }

    #[test]
    fn row_extent_spans_every_part_of_the_row() {
        let marker = Bounds {
            origin: point(px(0.0), px(120.0)),
            size: size(px(10.0), px(16.0)),
        };
        let text = Bounds {
            origin: point(px(12.0), px(118.0)),
            size: size(px(200.0), px(40.0)),
        };
        assert_eq!(
            markdown_preview_row_extent(&[marker, text]),
            Some((px(118.0), px(40.0)))
        );
        assert_eq!(markdown_preview_row_extent(&[]), None);
    }

    /// The wash is layered on the rendered text, so a query matches what the
    /// reader sees — not the markdown that produced it.
    #[test]
    fn the_search_wash_covers_rendered_text_and_leaves_unmatched_rows_untouched() {
        let theme = AppTheme::worktree_dark();
        let bolded = row(
            "a bold word",
            vec![MarkdownInlineSpan {
                byte_range: 2..6,
                style: MarkdownInlineStyle::Bold,
                link_url: None,
            }],
        );

        let base = markdown_preview_styled_row_with_query(theme, &bolded, 0, None);
        // `word` sits outside the bold span, so the wash has to add a range of
        // its own rather than restyle one that was already there.
        let washed =
            markdown_preview_styled_row_with_query(theme, &bolded, 0, Some(&query("word", None)));
        assert!(
            washed.highlights.len() > base.highlights.len(),
            "expected the query wash to add a highlight range alongside the bold span"
        );

        // The `**` that made it bold is not in the rendered text.
        let unmatched =
            markdown_preview_styled_row_with_query(theme, &bolded, 0, Some(&query("**", None)));
        assert_eq!(
            unmatched.highlights.len(),
            base.highlights.len(),
            "markdown syntax the renderer consumed must not be searchable"
        );
    }

    /// The current match is washed differently from the rest, so stepping
    /// through hits is visible.
    #[test]
    fn the_current_match_row_is_washed_differently_from_the_others() {
        let theme = AppTheme::worktree_dark();
        let plain = row("find me here", Vec::new());

        let current =
            markdown_preview_styled_row_with_query(theme, &plain, 3, Some(&query("me", Some(3))));
        let other =
            markdown_preview_styled_row_with_query(theme, &plain, 3, Some(&query("me", Some(9))));
        assert_ne!(
            current.highlights, other.highlights,
            "the row the search cursor sits on should not look like every other hit"
        );
    }
}
