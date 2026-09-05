//! Diff row rendering: styled-text specs, the `MainPaneView` row builders,
//! and the unified/split row bodies.

use super::*;

use super::blame::{BlamePrev, build_row_blame_paint_tracked};
use super::collapsed_hunk::{
    collapsed_hunk_shell_width, collapsed_inline_header_row, collapsed_split_header_row,
};
// This module is itself named `rows`, which would shadow the `rows::…` paths
// below (originally written against `crate::view::rows`); pin the binding back.
use crate::view::rows;

pub(in crate::view) fn should_hide_unified_diff_header_line(line: &AnnotatedDiffLine) -> bool {
    matches!(line.kind, DiffLineKind::Header)
        && (line.text.starts_with("index ")
            || line.text.starts_with("--- ")
            || line.text.starts_with("+++ "))
}

/// The focused row sits inside the diff body, so it takes the diff palette's own
/// focused token rather than a tint derived from the status palette: those are
/// two different greens and reds in every theme, and deriving it here made
/// focusing a row shift its hue and left `diff.*.focused_background` with no
/// effect at all.
///
/// A context/header/hunk row belongs to no diff kind and keeps the neutral wash.
pub(super) fn focused_diff_line_bg(theme: AppTheme, kind: DiffLineKind) -> gpui::Rgba {
    match kind {
        DiffLineKind::Add => theme.colors.diff.added.focused_background,
        DiffLineKind::Remove => theme.colors.diff.removed.focused_background,
        DiffLineKind::Context | DiffLineKind::Header | DiffLineKind::Hunk => {
            focused_diff_neutral_row_bg(theme)
        }
    }
}

/// Which diff palette a line's word highlights come from.
fn diff_line_word_kind(kind: DiffLineKind) -> Option<crate::theme::DiffColorKind> {
    match kind {
        DiffLineKind::Add => Some(crate::theme::DiffColorKind::Added),
        DiffLineKind::Remove => Some(crate::theme::DiffColorKind::Removed),
        _ => None,
    }
}

/// Same, for a file diff split column.
/// Left highlights Remove/Modify; Right highlights Add/Modify.
fn file_diff_split_word_kind(
    column: PatchSplitColumn,
    kind: FileDiffRowKind,
) -> Option<crate::theme::DiffColorKind> {
    match column {
        PatchSplitColumn::Left => matches!(kind, FileDiffRowKind::Remove | FileDiffRowKind::Modify)
            .then_some(crate::theme::DiffColorKind::Removed),
        PatchSplitColumn::Right => matches!(kind, FileDiffRowKind::Add | FileDiffRowKind::Modify)
            .then_some(crate::theme::DiffColorKind::Added),
    }
}

fn streamed_diff_text_spec_with_syntax(
    raw_text: worktree_core::file_diff::FileDiffLineText,
    query: &SharedString,
    query_options: DiffSearchOptions,
    query_matcher: Option<Arc<DiffSearchMatcher>>,
    word_ranges: Vec<Range<usize>>,
    word_kind: Option<crate::theme::DiffColorKind>,
    syntax: diff_canvas::StreamedDiffTextSyntaxSource,
) -> Option<diff_canvas::StreamedDiffTextPaintSpec> {
    diff_canvas::is_streamable_diff_text(&raw_text).then(|| {
        diff_canvas::StreamedDiffTextPaintSpec {
            raw_text,
            query: query.clone(),
            query_options,
            query_matcher,
            query_emphasis: DiffSearchMatchEmphasis::Other,
            word_ranges: Arc::from(word_ranges),
            word_kind,
            syntax,
        }
    })
}

fn heuristic_streamed_diff_text_spec(
    raw_text: worktree_core::file_diff::FileDiffLineText,
    query: &SharedString,
    query_options: DiffSearchOptions,
    query_matcher: Option<Arc<DiffSearchMatcher>>,
    word_ranges: Vec<Range<usize>>,
    word_kind: Option<crate::theme::DiffColorKind>,
    language: Option<rows::DiffSyntaxLanguage>,
    mode: rows::DiffSyntaxMode,
) -> Option<diff_canvas::StreamedDiffTextPaintSpec> {
    let syntax = match language {
        Some(language) => diff_canvas::StreamedDiffTextSyntaxSource::Heuristic { language, mode },
        None => diff_canvas::StreamedDiffTextSyntaxSource::None,
    };
    streamed_diff_text_spec_with_syntax(
        raw_text,
        query,
        query_options,
        query_matcher,
        word_ranges,
        word_kind,
        syntax,
    )
}

#[allow(clippy::too_many_arguments)]
fn prepared_streamed_diff_text_spec(
    raw_text: worktree_core::file_diff::FileDiffLineText,
    query: &SharedString,
    query_options: DiffSearchOptions,
    query_matcher: Option<Arc<DiffSearchMatcher>>,
    word_ranges: Vec<Range<usize>>,
    word_kind: Option<crate::theme::DiffColorKind>,
    language: Option<rows::DiffSyntaxLanguage>,
    fallback_mode: rows::DiffSyntaxMode,
    document_text: Arc<str>,
    line_starts: Arc<[usize]>,
    prepared_line: rows::PreparedDiffSyntaxLine,
) -> Option<diff_canvas::StreamedDiffTextPaintSpec> {
    let syntax = match (language, prepared_line.document) {
        (Some(language), Some(document)) => diff_canvas::StreamedDiffTextSyntaxSource::Prepared {
            document_text,
            line_starts,
            document,
            language,
            line_ix: prepared_line.line_ix,
        },
        (Some(language), None) => diff_canvas::StreamedDiffTextSyntaxSource::Heuristic {
            language,
            mode: fallback_mode,
        },
        (None, _) => diff_canvas::StreamedDiffTextSyntaxSource::None,
    };
    streamed_diff_text_spec_with_syntax(
        raw_text,
        query,
        query_options,
        query_matcher,
        word_ranges,
        word_kind,
        syntax,
    )
}

fn build_file_diff_cached_styled_text(
    theme: AppTheme,
    raw_text: &worktree_core::file_diff::FileDiffLineText,
    word_ranges: &[Range<usize>],
    context_prefix: &str,
    language: Option<DiffSyntaxLanguage>,
    syntax_mode: DiffSyntaxMode,
    word_kind: Option<crate::theme::DiffColorKind>,
) -> CachedDiffStyledText {
    if should_truncate_file_diff_display(raw_text) {
        let display = file_diff_display_text(raw_text);
        return build_cached_diff_styled_text(
            theme,
            display.as_ref(),
            &[],
            context_prefix,
            None,
            DiffSyntaxMode::HeuristicOnly,
            None,
        );
    }

    build_cached_diff_styled_text(
        theme,
        raw_text.as_ref(),
        word_ranges,
        context_prefix,
        language,
        syntax_mode,
        word_kind,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_file_diff_cached_styled_text_for_prepared_line_nonblocking(
    theme: AppTheme,
    raw_text: &worktree_core::file_diff::FileDiffLineText,
    word_ranges: &[Range<usize>],
    context_prefix: &str,
    syntax: DiffSyntaxConfig,
    word_kind: Option<crate::theme::DiffColorKind>,
    projected: rows::PreparedDiffSyntaxLine,
) -> (CachedDiffStyledText, bool) {
    if should_truncate_file_diff_display(raw_text) {
        let display = file_diff_display_text(raw_text);
        return (
            build_cached_diff_styled_text(
                theme,
                display.as_ref(),
                &[],
                context_prefix,
                None,
                DiffSyntaxMode::HeuristicOnly,
                None,
            ),
            false,
        );
    }

    build_cached_diff_styled_text_for_prepared_document_line_nonblocking(
        theme,
        raw_text.as_ref(),
        word_ranges,
        context_prefix,
        syntax,
        word_kind,
        projected,
    )
    .into_parts()
}

fn file_diff_split_side_text(
    row: &FileDiffRow,
    is_left: bool,
) -> Option<&worktree_core::file_diff::FileDiffLineText> {
    if is_left {
        row.old.as_ref()
    } else {
        row.new.as_ref()
    }
}

fn file_diff_split_side_text_owned(
    row: &FileDiffRow,
    is_left: bool,
) -> Option<worktree_core::file_diff::FileDiffLineText> {
    file_diff_split_side_text(row, is_left).cloned()
}

fn file_diff_split_side_line(row: &FileDiffRow, is_left: bool) -> Option<u32> {
    if is_left { row.old_line } else { row.new_line }
}

impl MainPaneView {
    /// Test-only reader for the blame lines behind that context.
    ///
    /// `blame_render_ctx` needs `&mut self` to memoize its time range, which a
    /// `read`-borrowed pane cannot give it.
    #[cfg(test)]
    pub(in crate::view) fn blame_render_ctx_for_test(
        &self,
    ) -> Option<&std::sync::Arc<Vec<worktree_core::services::BlameLine>>> {
        if !self.annotation_active() || !self.blame_matches_rendered_target() {
            return None;
        }
        match &self.active_repo()?.history_state.blame {
            worktree_state::model::Loadable::Ready(lines) => Some(lines),
            _ => None,
        }
    }

    /// Build a blame render context when annotate is enabled and blame for the
    /// current target is loaded; otherwise `None`.
    ///
    /// While the same target reloads, falls back to the annotations retained by
    /// the store (`retained_blame_while_loading`) so the column keeps its
    /// contents instead of blanking on every refresh. The retained value is
    /// dropped when blame re-targets, so it always describes `blame_path`.
    pub(in crate::view) fn blame_render_ctx(&mut self) -> Option<BlameRenderCtx> {
        if !self.annotation_active() || !self.blame_matches_rendered_target() {
            return None;
        }
        let repo = self.active_repo()?;
        let lines = match &repo.history_state.blame {
            worktree_state::model::Loadable::Ready(lines) => lines,
            worktree_state::model::Loadable::NotLoaded
            | worktree_state::model::Loadable::Loading => {
                repo.history_state.retained_blame_while_loading.as_ref()?
            }
            worktree_state::model::Loadable::Error(_) => return None,
        };
        let path: std::sync::Arc<std::path::Path> =
            std::sync::Arc::from(repo.history_state.blame_path.as_deref()?);
        // When blaming a specific commit, that commit is the one currently being
        // viewed; "view file at this commit" on its own lines would be a no-op.
        let viewed_commit = match &repo.history_state.blame_source {
            Some(worktree_core::domain::BlameSource::Revision(Some(rev))) => {
                Some(std::sync::Arc::<str>::from(rev.as_str()))
            }
            _ => None,
        };
        // The blamed working-tree area, used to classify uncommitted lines as
        // staged vs unstaged. `None` for revision blame (no such distinction).
        let area = match &repo.history_state.blame_source {
            Some(worktree_core::domain::BlameSource::WorkingTree(area)) => Some(*area),
            _ => None,
        };
        let lines = std::sync::Arc::clone(lines);
        // The time range never changes for a given loaded blame, so memoize it by
        // the blame Arc's identity instead of rescanning every frame. Compare by
        // `ptr_eq` against a held Arc clone: keeping the cached allocation alive
        // means a reloaded blame can't reuse the same address and alias a stale
        // range (an ABA hazard a bare pointer key would have).
        let range = match &self.blame_time_range_cache {
            Some((cached, range)) if std::sync::Arc::ptr_eq(cached, &lines) => *range,
            _ => {
                let range = super::super::blame::blame_time_range(&lines);
                self.blame_time_range_cache = Some((std::sync::Arc::clone(&lines), range));
                range
            }
        };
        Some(BlameRenderCtx {
            lines,
            range,
            now: std::time::SystemTime::now(),
            path,
            viewed_commit,
            area,
        })
    }

    fn diff_text_segments_cache_get_for_query(
        &mut self,
        key: usize,
        query: &str,
        options: DiffSearchOptions,
        syntax_epoch: u64,
    ) -> Option<&CachedDiffStyledText> {
        if query.is_empty() {
            return self.diff_text_segments_cache_get(key, syntax_epoch);
        }

        self.sync_diff_text_query_overlay_cache(query, options);
        let query_generation = self.diff_text_query_cache_generation;
        if self.diff_text_query_segments_cache.len() <= key {
            self.diff_text_query_segments_cache
                .resize_with(key + 1, || None);
        }

        if versioned_query_cached_diff_styled_text_is_current(
            self.diff_text_query_segments_cache
                .get(key)
                .and_then(Option::as_ref),
            syntax_epoch,
            query_generation,
        )
        .is_none()
        {
            let base = self
                .diff_text_segments_cache_get(key, syntax_epoch)?
                .clone();
            // The diff view marks its current match by selecting the row, so
            // every match here wears the same wash.
            let overlaid = build_cached_diff_query_overlay_styled_text(
                self.theme,
                &base,
                self.diff_text_query_cache_matcher.as_ref()?,
                DiffSearchMatchEmphasis::Other,
            );
            self.diff_text_query_segments_cache[key] = Some(VersionedCachedDiffStyledText {
                syntax_epoch,
                query_generation,
                styled: overlaid,
            });
        }

        versioned_query_cached_diff_styled_text_is_current(
            self.diff_text_query_segments_cache
                .get(key)
                .and_then(Option::as_ref),
            syntax_epoch,
            query_generation,
        )
    }

    /// The coverage overlay inputs for the file on screen: the imported
    /// report plus the diff target's concrete path. `None` when no report
    /// is imported or the target spans a whole commit with no single file
    /// selected — there is nothing honest to annotate then.
    fn diff_coverage_context(
        &self,
    ) -> Option<(
        std::sync::Arc<worktree_core::coverage::CoverageReport>,
        String,
    )> {
        let repo = self.active_repo()?;
        let report = repo.coverage.clone()?;
        let path = match repo.diff_state.diff_target.as_ref()? {
            DiffTarget::WorkingTree { path, .. } => path,
            DiffTarget::Commit {
                path: Some(path), ..
            } => path,
            DiffTarget::CommitRange {
                path: Some(path), ..
            } => path,
            _ => return None,
        };
        let normalized = worktree_core::coverage::normalize_coverage_path(&path.to_string_lossy());
        (!normalized.is_empty()).then_some((report, normalized))
    }

    pub(in crate::view) fn render_diff_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let annot_hover = this.blame_annot_hover;
        let stage_area = this.diff_stage_gutter_area();
        let stage_hover = this.diff_stage_gutter_hover;
        let min_width = this.diff_horizontal_layout_min_width(DiffHorizontalScrollColumn::Primary);
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query_matcher = (!query.as_ref().is_empty())
            .then(|| Arc::new(DiffSearchMatcher::new(query.as_ref(), query_options)));
        let reveal_whitespace_chars = this.reveal_whitespace_chars;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();
        let annotation_width = if this.annotation_active() {
            this.annotate_column_width_px(ui_scale_percent)
        } else {
            px(0.0)
        };
        let blame_ctx = this.blame_render_ctx();
        let coverage = this.diff_coverage_context();
        let coverage = coverage
            .as_ref()
            .map(|(report, path)| (report.as_ref(), path.as_str()));

        if this.is_collapsed_diff_projection_active() {
            let theme = this.theme;
            let language = this.file_diff_cache_language;
            let old_document_text: Arc<str> = this.file_diff_old_text.clone().into();
            let old_line_starts = Arc::clone(&this.file_diff_old_line_starts);
            let new_document_text: Arc<str> = this.file_diff_new_text.clone().into();
            let new_line_starts = Arc::clone(&this.file_diff_new_line_starts);
            let pinned_hunk_shell_width = collapsed_hunk_shell_width(&this.diff_scroll, min_width);
            let pinned_hunk_shell_scroll = this.diff_scroll.clone();

            let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
            return range
                .map(|visible_ix| {
                    let selected = this
                        .diff_selection_range
                        .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                    let show_line_numbers = this.diff_show_line_numbers;
                    let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);
                    let Some(source_visible_ix) =
                        this.diff_source_visible_ix_for_visible_ix(visible_ix)
                    else {
                        return diff_placeholder_row(
                            ("collapsed_diff_missing", visible_ix),
                            theme,
                            ui_scale_percent,
                        );
                    };
                    let Some(row) = this.collapsed_visible_row(source_visible_ix) else {
                        return diff_placeholder_row(
                            ("collapsed_diff_missing", visible_ix),
                            theme,
                            ui_scale_percent,
                        );
                    };

                    match row {
                        CollapsedDiffVisibleRow::HunkHeader {
                            src_ix,
                            expansion_kind,
                            hidden_rows,
                            ..
                        } => {
                            let display_src_ix = row.header_display_src_ix();
                            let display = display_src_ix
                                .and_then(|display_src_ix| {
                                    this.collapsed_diff_hunk_header_display(display_src_ix)
                                })
                                .unwrap_or_default();
                            let context_menu_active = display_src_ix.is_some()
                                && this.active_repo_id().is_some_and(|repo_id| {
                                    let invoker: SharedString =
                                        format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
                                    this.active_context_menu_invoker.as_ref() == Some(&invoker)
                                });
                            let collapsed_hunk = this.collapsed_diff_hunk_for_src_ix(src_ix);

                            collapsed_inline_header_row(
                                theme,
                                ui_scale_percent,
                                visible_ix,
                                DiffClickKind::HunkHeader,
                                selected,
                                min_width,
                                pinned_hunk_shell_width,
                                pinned_hunk_shell_scroll.clone(),
                                collapsed_hunk,
                                None,
                                display,
                                None,
                                context_menu_active,
                                src_ix,
                                expansion_kind,
                                hidden_rows,
                                cx,
                            )
                        }
                        CollapsedDiffVisibleRow::FileRow { row_ix } => {
                            let row_word_ranges = this.file_diff_inline_word_ranges(row_ix);
                            let Some(row) = this.file_diff_inline_render_data(row_ix) else {
                                return diff_placeholder_row(
                                    ("collapsed_diff_oob", visible_ix),
                                    theme,
                                    ui_scale_percent,
                                );
                            };
                            let visual_kind = this.file_diff_inline_visual_kind(row_ix);
                            let line = AnnotatedDiffLine {
                                kind: row.kind,
                                text: "".into(),
                                old_line: row.old_line,
                                new_line: row.new_line,
                            };
                            let streamed_spec = {
                                let line_language = matches!(
                                    row.kind,
                                    DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                                )
                                .then_some(language)
                                .flatten();
                                let word_kind = diff_line_word_kind(visual_kind);
                                let prepared_line = match row.kind {
                                    DiffLineKind::Remove => {
                                        rows::prepared_diff_syntax_line_for_one_based_line(
                                            this.file_diff_split_prepared_syntax_document(
                                                DiffTextRegion::SplitLeft,
                                            ),
                                            row.old_line,
                                        )
                                    }
                                    DiffLineKind::Add | DiffLineKind::Context => {
                                        rows::prepared_diff_syntax_line_for_one_based_line(
                                            this.file_diff_split_prepared_syntax_document(
                                                DiffTextRegion::SplitRight,
                                            ),
                                            row.new_line,
                                        )
                                    }
                                    DiffLineKind::Header | DiffLineKind::Hunk => {
                                        rows::prepared_diff_syntax_line_for_one_based_line(
                                            None, None,
                                        )
                                    }
                                };
                                let (document_text, line_starts) = match row.kind {
                                    DiffLineKind::Remove => (
                                        Arc::clone(&old_document_text),
                                        Arc::clone(&old_line_starts),
                                    ),
                                    DiffLineKind::Add | DiffLineKind::Context => (
                                        Arc::clone(&new_document_text),
                                        Arc::clone(&new_line_starts),
                                    ),
                                    DiffLineKind::Header | DiffLineKind::Hunk => (
                                        Arc::clone(&new_document_text),
                                        Arc::clone(&new_line_starts),
                                    ),
                                };
                                let syntax_mode = DiffSyntaxMode::Auto;
                                prepared_streamed_diff_text_spec(
                                    row.text.clone(),
                                    &query,
                                    query_options,
                                    query_matcher.clone(),
                                    row_word_ranges.clone(),
                                    word_kind,
                                    line_language,
                                    syntax_mode,
                                    document_text,
                                    line_starts,
                                    prepared_line,
                                )
                            };

                            let styled = if streamed_spec.is_some() {
                                None
                            } else {
                                let cache_epoch =
                                    this.file_diff_style_cache_epochs.inline_epoch(row.kind);
                                if this
                                    .diff_text_segments_cache_get(row_ix, cache_epoch)
                                    .is_none()
                                {
                                    let word_kind = diff_line_word_kind(visual_kind);
                                    let is_content_line = matches!(
                                        line.kind,
                                        DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                                    );
                                    let line_language =
                                        is_content_line.then_some(language).flatten();
                                    let projected = this.file_diff_inline_projected_syntax(&line);
                                    let syntax_mode = DiffSyntaxMode::Auto;
                                    let (styled, is_pending) =
                                        build_file_diff_cached_styled_text_for_prepared_line_nonblocking(
                                            theme,
                                            &row.text,
                                            row_word_ranges.as_slice(),
                                            "",
                                            DiffSyntaxConfig {
                                                language: line_language,
                                                mode: syntax_mode,
                                            },
                                            word_kind,
                                            projected,
                                        );
                                    if is_pending {
                                        this.ensure_prepared_syntax_chunk_poll(cx);
                                    }
                                    this.diff_text_segments_cache_set(row_ix, cache_epoch, styled);
                                }
                                this.diff_text_segments_cache_get_for_query(
                                    row_ix,
                                    query.as_ref(),
                                    query_options,
                                    cache_epoch,
                                )
                            };

                            diff_row(
                                theme,
                                ui_scale_percent,
                                visible_ix,
                                DiffClickKind::Line,
                                selected,
                                DiffViewMode::Inline,
                                min_width,
                                &line,
                                visual_kind,
                                None,
                                None,
                                styled,
                                streamed_spec,
                                Some(row.text.as_ref()),
                                reveal_whitespace_chars,
                                false,
                                show_line_numbers,
                                wrap,
                                annotation_width,
                                blame_ctx
                                    .as_ref()
                                    .and_then(|ctx| build_row_blame_paint_tracked(ctx, matches!(visual_kind, DiffLineKind::Context), line.old_line, line.new_line, &blame_prev_nl, wrap, theme)),
                                annot_hover,
                                stage_area,
                                stage_hover,
                                coverage,
                                cx,
                            )
                        }
                    }
                })
                .collect();
        }

        if this.is_file_diff_view_active() {
            let theme = this.theme;
            let language = this.file_diff_cache_language;
            let old_document_text: Arc<str> = this.file_diff_old_text.clone().into();
            let old_line_starts = Arc::clone(&this.file_diff_old_line_starts);
            let new_document_text: Arc<str> = this.file_diff_new_text.clone().into();
            let new_line_starts = Arc::clone(&this.file_diff_new_line_starts);
            // Inline syntax is now projected from the real old/new (split)
            // documents instead of parsing a synthetic mixed inline stream.
            // syntax_mode is determined per-row based on projection availability.
            if let Some(language) = language {
                struct SyntaxOnlyBatchRow {
                    inline_ix: usize,
                    cache_epoch: u64,
                    line: AnnotatedDiffLine,
                    text: worktree_core::file_diff::FileDiffLineText,
                }

                let mut syntax_only_rows = Vec::new();
                for visible_ix in range.clone() {
                    let Some(inline_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        continue;
                    };
                    let Some(row) = this.file_diff_inline_render_data(inline_ix) else {
                        continue;
                    };
                    if diff_canvas::is_streamable_diff_text(&row.text) {
                        continue;
                    }
                    if should_truncate_file_diff_display(&row.text) {
                        continue;
                    }
                    let line = AnnotatedDiffLine {
                        kind: row.kind,
                        text: "".into(),
                        old_line: row.old_line,
                        new_line: row.new_line,
                    };
                    let cache_epoch = this.file_diff_style_cache_epochs.inline_epoch(row.kind);
                    if this
                        .diff_text_segments_cache_get(inline_ix, cache_epoch)
                        .is_some()
                    {
                        continue;
                    }
                    if !matches!(
                        line.kind,
                        DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                    ) {
                        continue;
                    }
                    if this.file_diff_inline_modify_pair_texts(inline_ix).is_some() {
                        continue;
                    }
                    syntax_only_rows.push(SyntaxOnlyBatchRow {
                        inline_ix,
                        cache_epoch,
                        line,
                        text: row.text,
                    });
                }

                if !syntax_only_rows.is_empty() {
                    let batch_rows = syntax_only_rows
                        .iter()
                        .map(|row| InlineDiffSyntaxOnlyRow {
                            text: row.text.as_ref(),
                            line: &row.line,
                        })
                        .collect::<Vec<_>>();
                    let batched_styles =
                        build_cached_diff_styled_text_for_inline_syntax_only_rows_nonblocking(
                            theme,
                            Some(language),
                            PreparedDiffSyntaxTextSource {
                                document: this.file_diff_split_prepared_syntax_document(
                                    DiffTextRegion::SplitLeft,
                                ),
                            },
                            PreparedDiffSyntaxTextSource {
                                document: this.file_diff_split_prepared_syntax_document(
                                    DiffTextRegion::SplitRight,
                                ),
                            },
                            batch_rows.as_slice(),
                            DiffSyntaxMode::Auto,
                        );
                    let mut pending_batch = false;
                    for (row, prepared) in syntax_only_rows.iter().zip(batched_styles) {
                        let (styled, is_pending) = prepared.into_parts();
                        pending_batch |= is_pending;
                        this.diff_text_segments_cache_set(row.inline_ix, row.cache_epoch, styled);
                    }
                    if pending_batch {
                        this.ensure_prepared_syntax_chunk_poll(cx);
                    }
                }
            }

            let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
            return range
                .map(|visible_ix| {
                    let selected = this
                        .diff_selection_range
                        .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                    let show_line_numbers = this.diff_show_line_numbers;
                    let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);

                    let Some(inline_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return diff_placeholder_row(
                            ("diff_missing", visible_ix),
                            theme,
                            ui_scale_percent,
                        );
                    };
                    let row_word_ranges = this.file_diff_inline_word_ranges(inline_ix);
                    let visual_kind = this.file_diff_inline_visual_kind(inline_ix);
                    let render_data = this.file_diff_inline_render_data(inline_ix);
                    let streamed_spec = render_data.as_ref().and_then(|row| {
                        let line_language = matches!(
                            row.kind,
                            DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                        )
                        .then_some(language)
                        .flatten();
                        let word_kind = diff_line_word_kind(visual_kind);
                        let prepared_line = match row.kind {
                            DiffLineKind::Remove => rows::prepared_diff_syntax_line_for_one_based_line(
                                this.file_diff_split_prepared_syntax_document(
                                    DiffTextRegion::SplitLeft,
                                ),
                                row.old_line,
                            ),
                            DiffLineKind::Add | DiffLineKind::Context => {
                                rows::prepared_diff_syntax_line_for_one_based_line(
                                    this.file_diff_split_prepared_syntax_document(
                                        DiffTextRegion::SplitRight,
                                    ),
                                    row.new_line,
                                )
                            }
                            DiffLineKind::Header | DiffLineKind::Hunk => {
                                rows::prepared_diff_syntax_line_for_one_based_line(None, None)
                            }
                        };
                        let (document_text, line_starts) = match row.kind {
                            DiffLineKind::Remove => (
                                Arc::clone(&old_document_text),
                                Arc::clone(&old_line_starts),
                            ),
                            DiffLineKind::Add | DiffLineKind::Context => (
                                Arc::clone(&new_document_text),
                                Arc::clone(&new_line_starts),
                            ),
                            DiffLineKind::Header | DiffLineKind::Hunk => (
                                Arc::clone(&new_document_text),
                                Arc::clone(&new_line_starts),
                            ),
                        };
                        let syntax_mode = DiffSyntaxMode::Auto;
                        prepared_streamed_diff_text_spec(
                            row.text.clone(),
                            &query,
                            query_options,
                            query_matcher.clone(),
                            row_word_ranges.clone(),
                            word_kind,
                            line_language,
                            syntax_mode,
                            document_text,
                            line_starts,
                            prepared_line,
                        )
                    });

                    let (line, cache_epoch, styled) = if let Some(row) = render_data.as_ref() {
                        let line = AnnotatedDiffLine {
                            kind: row.kind,
                            text: "".into(),
                            old_line: row.old_line,
                            new_line: row.new_line,
                        };
                        let cache_epoch = this.file_diff_style_cache_epochs.inline_epoch(row.kind);
                        if streamed_spec.is_none()
                            && this
                                .diff_text_segments_cache_get(inline_ix, cache_epoch)
                                .is_none()
                            {
                                let word_kind = diff_line_word_kind(visual_kind);
                                let is_content_line = matches!(
                                    line.kind,
                                    DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                                );
                                let line_language = is_content_line.then_some(language).flatten();
                                let projected = this.file_diff_inline_projected_syntax(&line);
                                let syntax_mode = DiffSyntaxMode::Auto;
                                let (styled, is_pending) =
                                    build_file_diff_cached_styled_text_for_prepared_line_nonblocking(
                                        theme,
                                        &row.text,
                                        row_word_ranges.as_slice(),
                                        "",
                                        DiffSyntaxConfig {
                                            language: line_language,
                                            mode: syntax_mode,
                                        },
                                        word_kind,
                                        projected,
                                    );
                                if is_pending {
                                    this.ensure_prepared_syntax_chunk_poll(cx);
                                }
                                this.diff_text_segments_cache_set(inline_ix, cache_epoch, styled);
                            }
                        let styled = if streamed_spec.is_none() {
                            this.diff_text_segments_cache_get_for_query(
                                inline_ix,
                                query.as_ref(),
                                query_options,
                                cache_epoch,
                            )
                        } else {
                            None
                        };
                        debug_assert!(
                            streamed_spec.is_some() || styled.is_some(),
                            "diff text segment cache missing for inline row {inline_ix} after populate"
                        );
                        (line, cache_epoch, styled)
                    } else {
                        let Some(line) = this.file_diff_inline_row(inline_ix) else {
                            return diff_placeholder_row(
                                ("diff_oob", visible_ix),
                                theme,
                                ui_scale_percent,
                            );
                        };
                        let cache_epoch = this.file_diff_inline_style_cache_epoch(&line);
                        if this
                            .diff_text_segments_cache_get(inline_ix, cache_epoch)
                            .is_none()
                        {
                            let word_kind = diff_line_word_kind(visual_kind);
                            let is_content_line = matches!(
                                line.kind,
                                DiffLineKind::Add | DiffLineKind::Remove | DiffLineKind::Context
                            );
                            let line_language = is_content_line.then_some(language).flatten();
                            let projected = this.file_diff_inline_projected_syntax(&line);
                            let syntax_mode = DiffSyntaxMode::Auto;
                            let (styled, is_pending) =
                                build_cached_diff_styled_text_for_prepared_document_line_nonblocking(
                                    theme,
                                    diff_content_text(&line),
                                    row_word_ranges.as_slice(),
                                    "",
                                    DiffSyntaxConfig {
                                        language: line_language,
                                        mode: syntax_mode,
                                    },
                                    word_kind,
                                    projected,
                                )
                                .into_parts();
                            if is_pending {
                                this.ensure_prepared_syntax_chunk_poll(cx);
                            }
                            this.diff_text_segments_cache_set(inline_ix, cache_epoch, styled);
                        }
                        let styled = this.diff_text_segments_cache_get_for_query(
                            inline_ix,
                            query.as_ref(),
                            query_options,
                            cache_epoch,
                        );
                        debug_assert!(
                            styled.is_some(),
                            "diff text segment cache missing for inline row {inline_ix} after populate"
                        );
                        (line, cache_epoch, styled)
                    };
                    let _ = cache_epoch;

                    diff_row(
                        theme,
                        ui_scale_percent,
                        visible_ix,
                        DiffClickKind::Line,
                        selected,
                        DiffViewMode::Inline,
                        min_width,
                        &line,
                        visual_kind,
                        None,
                        None,
                        styled,
                        streamed_spec,
                        render_data
                            .as_ref()
                            .map(|row| row.text.as_ref())
                            .or_else(|| Some(diff_content_text(&line))),
                        reveal_whitespace_chars,
                        false,
                        show_line_numbers,
                        wrap,
                        annotation_width,
                        blame_ctx
                            .as_ref()
                            .and_then(|ctx| build_row_blame_paint_tracked(ctx, matches!(visual_kind, DiffLineKind::Context), line.old_line, line.new_line, &blame_prev_nl, wrap, theme)),
                        annot_hover,
                        stage_area,
                        stage_hover,
                        coverage,
                        cx,
                    )
                })
                .collect();
        }

        let theme = this.theme;
        let cache_epoch = 0u64;
        let repo_id_for_context_menu = this.active_repo_id();
        let active_context_menu_invoker = this.active_context_menu_invoker.clone();
        let syntax_mode = this.patch_diff_syntax_mode();
        let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
        range
            .map(|visible_ix| {
                let selected = this
                    .diff_selection_range
                    .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                let show_line_numbers = this.diff_show_line_numbers;
                let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);

                let Some(src_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                    return diff_placeholder_row(
                        ("diff_missing", visible_ix),
                        theme,
                        ui_scale_percent,
                    );
                };
                let click_kind = this
                    .diff_click_kinds
                    .get(src_ix)
                    .copied()
                    .unwrap_or(DiffClickKind::Line);

                this.ensure_patch_diff_word_highlight_for_src_ix(src_ix);
                let word_ranges: &[Range<usize>] = this
                    .diff_word_highlights
                    .get(src_ix)
                    .and_then(|r| r.as_ref().map(Vec::as_slice))
                    .unwrap_or(&[]);

                let file_stat = this.diff_file_stats.get(src_ix).and_then(|s| *s);

                let language = this.diff_language_for_src_ix.get(src_ix).copied().flatten();
                let Some(line) = this.patch_diff_row(src_ix) else {
                    return diff_placeholder_row(("diff_oob", visible_ix), theme, ui_scale_percent);
                };
                let visual_kind = this.patch_visual_line_kind(src_ix);
                let streamed_spec = matches!(click_kind, DiffClickKind::Line)
                    .then(|| {
                        heuristic_streamed_diff_text_spec(
                            crate::view::diff_utils::diff_content_line_text(&line),
                            &query,
                            query_options,
                            query_matcher.clone(),
                            word_ranges.to_vec(),
                            diff_line_word_kind(visual_kind),
                            language,
                            syntax_mode,
                        )
                    })
                    .flatten();

                let should_style = matches!(click_kind, DiffClickKind::Line) || !query.is_empty();
                if should_style
                    && streamed_spec.is_none()
                    && this
                        .diff_text_segments_cache_get(src_ix, cache_epoch)
                        .is_none()
                {
                    let computed = if matches!(click_kind, DiffClickKind::Line) {
                        let word_kind = diff_line_word_kind(visual_kind);
                        let content_text = diff_content_text(&line);

                        build_cached_diff_styled_text_with_source_identity(
                            theme,
                            content_text,
                            Some(DiffTextSourceIdentity::from_str(content_text)),
                            word_ranges,
                            "",
                            language,
                            syntax_mode,
                            word_kind,
                        )
                    } else {
                        let display =
                            this.diff_text_line_for_region(visible_ix, DiffTextRegion::Inline);
                        build_cached_diff_styled_text(
                            theme,
                            display.as_ref(),
                            &[] as &[Range<usize>],
                            "",
                            None,
                            syntax_mode,
                            None,
                        )
                    };
                    this.diff_text_segments_cache_set(src_ix, cache_epoch, computed);
                }

                let header_display = matches!(
                    click_kind,
                    DiffClickKind::FileHeader | DiffClickKind::HunkHeader
                )
                .then(|| this.diff_header_display_cache.get(&src_ix).cloned())
                .flatten();
                let context_menu_active = click_kind == DiffClickKind::HunkHeader
                    && repo_id_for_context_menu.is_some_and(|repo_id| {
                        let invoker: SharedString =
                            format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
                        active_context_menu_invoker.as_ref() == Some(&invoker)
                    });
                let styled = if should_style && streamed_spec.is_none() {
                    this.diff_text_segments_cache_get_for_query(
                        src_ix,
                        query.as_ref(),
                        query_options,
                        cache_epoch,
                    )
                } else {
                    None
                };
                diff_row(
                    theme,
                    ui_scale_percent,
                    visible_ix,
                    click_kind,
                    selected,
                    DiffViewMode::Inline,
                    min_width,
                    &line,
                    visual_kind,
                    file_stat,
                    header_display,
                    styled,
                    streamed_spec,
                    Some(if matches!(click_kind, DiffClickKind::Line) {
                        diff_content_text(&line)
                    } else {
                        line.text.as_ref()
                    }),
                    reveal_whitespace_chars,
                    context_menu_active,
                    show_line_numbers,
                    wrap,
                    annotation_width,
                    blame_ctx.as_ref().and_then(|ctx| {
                        build_row_blame_paint_tracked(
                            ctx,
                            matches!(visual_kind, DiffLineKind::Context),
                            line.old_line,
                            line.new_line,
                            &blame_prev_nl,
                            wrap,
                            theme,
                        )
                    }),
                    annot_hover,
                    stage_area,
                    stage_hover,
                    coverage,
                    cx,
                )
            })
            .collect()
    }

    pub(in crate::view) fn render_diff_split_left_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let annot_hover = this.blame_annot_hover;
        Self::render_diff_split_rows(this, PatchSplitColumn::Left, range, annot_hover, cx)
    }

    pub(in crate::view) fn render_diff_split_right_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let annot_hover = this.blame_annot_hover;
        Self::render_diff_split_rows(this, PatchSplitColumn::Right, range, annot_hover, cx)
    }

    fn render_diff_split_rows(
        this: &mut Self,
        column: PatchSplitColumn,
        range: Range<usize>,
        annot_hover: Option<(usize, AnnotArea)>,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let stage_area = this.diff_stage_gutter_area();
        let stage_hover = this.diff_stage_gutter_hover;
        let coverage = this.diff_coverage_context();
        let coverage = coverage
            .as_ref()
            .map(|(report, path)| (report.as_ref(), path.as_str()));
        let min_width =
            this.diff_horizontal_layout_min_width(if matches!(column, PatchSplitColumn::Right) {
                DiffHorizontalScrollColumn::SplitRight
            } else {
                DiffHorizontalScrollColumn::Primary
            });
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query_matcher = (!query.as_ref().is_empty())
            .then(|| Arc::new(DiffSearchMatcher::new(query.as_ref(), query_options)));
        let reveal_whitespace_chars = this.reveal_whitespace_chars;
        let ui_scale_percent = crate::ui_scale::UiScale::current(cx).percent();

        let is_left = matches!(column, PatchSplitColumn::Left);
        // The annotation column is only drawn in the left split column. Reserve
        // its width whenever annotate mode is on — even before blame data is
        // ready — so the column space is stable and content does not shift when
        // blame finishes loading. Only the left column needs the blame context;
        // building it for the right column would clone the blame data and rescan
        // the time range for nothing.
        let blame_ctx = if is_left {
            this.blame_render_ctx()
        } else {
            None
        };
        let annotation_width = if this.annotation_active() && is_left {
            this.annotate_column_width_px(ui_scale_percent)
        } else {
            px(0.0)
        };
        let region = if is_left {
            DiffTextRegion::SplitLeft
        } else {
            DiffTextRegion::SplitRight
        };
        // Static ID tags to avoid format!/String allocation in element IDs.
        let (id_missing, id_oob, id_src_oob, id_hidden) = if is_left {
            (
                "diff_split_left_missing",
                "diff_split_left_oob",
                "diff_split_left_src_oob",
                "diff_split_left_hidden_header",
            )
        } else {
            (
                "diff_split_right_missing",
                "diff_split_right_oob",
                "diff_split_right_src_oob",
                "diff_split_right_hidden_header",
            )
        };

        if this.is_collapsed_diff_projection_active() {
            let theme = this.theme;
            let language = this.file_diff_cache_language;
            let cache_epoch = this.file_diff_split_style_cache_epoch(region);
            let syntax_document = this.file_diff_split_prepared_syntax_document(region);
            let syntax_mode = DiffSyntaxMode::Auto;
            let document_text: Arc<str> = if is_left {
                this.file_diff_old_text.clone().into()
            } else {
                this.file_diff_new_text.clone().into()
            };
            let line_starts = if is_left {
                Arc::clone(&this.file_diff_old_line_starts)
            } else {
                Arc::clone(&this.file_diff_new_line_starts)
            };
            let pinned_hunk_shell_width = if is_left {
                collapsed_hunk_shell_width(&this.diff_scroll, min_width)
            } else {
                collapsed_hunk_shell_width(&this.diff_split_right_scroll, min_width)
            };
            let pinned_hunk_shell_scroll = if is_left {
                this.diff_scroll.clone()
            } else {
                this.diff_split_right_scroll.clone()
            };

            let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
            return range
                .map(|visible_ix| {
                    let selected = this
                        .diff_selection_range
                        .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                    let show_line_numbers = this.diff_show_line_numbers;
                    let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);
                    let Some(source_visible_ix) =
                        this.diff_source_visible_ix_for_visible_ix(visible_ix)
                    else {
                        return diff_placeholder_row((id_missing, visible_ix), theme, ui_scale_percent);
                    };
                    let Some(visible_row) = this.collapsed_visible_row(source_visible_ix) else {
                        return diff_placeholder_row((id_missing, visible_ix), theme, ui_scale_percent);
                    };

                    match visible_row {
                        CollapsedDiffVisibleRow::HunkHeader {
                            src_ix,
                            expansion_kind,
                            hidden_rows,
                            ..
                        } => {
                            let display_src_ix = visible_row.header_display_src_ix();
                            let display = display_src_ix
                                .and_then(|display_src_ix| {
                                    this.collapsed_diff_hunk_header_display(display_src_ix)
                                })
                                .unwrap_or_default();
                            let context_menu_active = display_src_ix.is_some()
                                && this.active_repo_id().is_some_and(|repo_id| {
                                    let invoker: SharedString =
                                        format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
                                    this.active_context_menu_invoker.as_ref() == Some(&invoker)
                                });
                            let collapsed_hunk = this.collapsed_diff_hunk_for_src_ix(src_ix);

                            collapsed_split_header_row(
                                theme,
                                ui_scale_percent,
                                column,
                                visible_ix,
                                DiffClickKind::HunkHeader,
                                selected,
                                min_width,
                                pinned_hunk_shell_width,
                                pinned_hunk_shell_scroll.clone(),
                                collapsed_hunk,
                                None,
                                display,
                                None,
                                context_menu_active,
                                src_ix,
                                expansion_kind,
                                hidden_rows,
                                cx,
                            )
                        }
                        CollapsedDiffVisibleRow::FileRow { row_ix } => {
                            let Some(row) = this.file_diff_split_render_data(row_ix) else {
                                return diff_placeholder_row((id_oob, visible_ix), theme, ui_scale_percent);
                            };
                            let visual_kind = this.file_diff_split_visual_kind(row_ix);
                            let row_word_ranges =
                                this.file_diff_split_word_ranges(row_ix, region);
                            let row_word_kind =
                                file_diff_split_word_kind(column, visual_kind);
                            let streamed_spec =
                                file_diff_split_side_text_owned(&row, is_left).and_then(
                                    |raw_text| {
                                        prepared_streamed_diff_text_spec(
                                            raw_text,
                                            &query,
                                            query_options,
                                            query_matcher.clone(),
                                            row_word_ranges.clone(),
                                            row_word_kind,
                                            language,
                                            syntax_mode,
                                            Arc::clone(&document_text),
                                            Arc::clone(&line_starts),
                                            rows::prepared_diff_syntax_line_for_one_based_line(
                                                syntax_document,
                                                file_diff_split_side_line(&row, is_left),
                                            ),
                                        )
                                    },
                                );
                            let key = this.file_diff_split_cache_key(row_ix, region);
                            if let Some(key) = key
                                && streamed_spec.is_none()
                                && this.diff_text_segments_cache_get(key, cache_epoch).is_none()
                            {
                                let raw_text = file_diff_split_side_text(&row, is_left);
                                if let Some(raw_text) = raw_text {
                                    let (styled, is_pending) =
                                        build_file_diff_cached_styled_text_for_prepared_line_nonblocking(
                                            theme,
                                            raw_text,
                                            row_word_ranges.as_slice(),
                                            "",
                                            DiffSyntaxConfig {
                                                language,
                                                mode: syntax_mode,
                                            },
                                            row_word_kind,
                                            rows::prepared_diff_syntax_line_for_one_based_line(
                                                syntax_document,
                                                file_diff_split_side_line(&row, is_left),
                                            ),
                                        );
                                    if is_pending {
                                        this.ensure_prepared_syntax_chunk_poll(cx);
                                    }
                                    this.diff_text_segments_cache_set(key, cache_epoch, styled);
                                }
                            }

                            let row_has_content = file_diff_split_side_text(&row, is_left).is_some();
                            let styled = if row_has_content && streamed_spec.is_none() {
                                if let Some(key) = key {
                                    this.diff_text_segments_cache_get_for_query(
                                        key,
                                        query.as_ref(),
                                        query_options,
                                        cache_epoch,
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            patch_split_column_row(
                                theme,
                                ui_scale_percent,
                                column,
                                visible_ix,
                                selected,
                                min_width,
                                &row,
                                visual_kind,
                                styled,
                                streamed_spec,
                                reveal_whitespace_chars,
                                show_line_numbers,
                                wrap,
                                annotation_width,
                                if is_left {
                                    blame_ctx.as_ref().and_then(|ctx| {
                                        build_row_blame_paint_tracked(ctx, matches!(visual_kind, FileDiffRowKind::Context), row.old_line, row.new_line, &blame_prev_nl, wrap, theme)
                                    })
                                } else {
                                    None
                                },
                                annot_hover,
                                stage_area,
                                stage_hover,
                                coverage,
                                cx,
                            )
                        }
                    }
                })
                .collect();
        }

        if this.is_file_diff_view_active() {
            let theme = this.theme;
            let language = this.file_diff_cache_language;
            let cache_epoch = this.file_diff_split_style_cache_epoch(region);
            let syntax_document = this.file_diff_split_prepared_syntax_document(region);
            let syntax_mode = DiffSyntaxMode::Auto;
            let document_text: Arc<str> = if is_left {
                this.file_diff_old_text.clone().into()
            } else {
                this.file_diff_new_text.clone().into()
            };
            let line_starts = if is_left {
                Arc::clone(&this.file_diff_old_line_starts)
            } else {
                Arc::clone(&this.file_diff_new_line_starts)
            };

            let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
            return range
                .map(|visible_ix| {
                    let selected = this
                        .diff_selection_range
                        .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                    let show_line_numbers = this.diff_show_line_numbers;
                    let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);

                    let Some(row_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                        return diff_placeholder_row((id_missing, visible_ix), theme, ui_scale_percent);
                    };
                    let Some(row) = this.file_diff_split_render_data(row_ix) else {
                        return diff_placeholder_row((id_oob, visible_ix), theme, ui_scale_percent);
                    };
                    let visual_kind = this.file_diff_split_visual_kind(row_ix);
                    let row_word_ranges = this.file_diff_split_word_ranges(row_ix, region);
                    let row_word_kind = file_diff_split_word_kind(column, visual_kind);
                    let streamed_spec = file_diff_split_side_text_owned(&row, is_left).and_then(
                        |raw_text| {
                            prepared_streamed_diff_text_spec(
                                raw_text,
                                &query,
                                query_options,
                                query_matcher.clone(),
                                row_word_ranges.clone(),
                                row_word_kind,
                                language,
                                syntax_mode,
                                Arc::clone(&document_text),
                                Arc::clone(&line_starts),
                                rows::prepared_diff_syntax_line_for_one_based_line(
                                    syntax_document,
                                    file_diff_split_side_line(&row, is_left),
                                ),
                            )
                        },
                    );
                    let key = this.file_diff_split_cache_key(row_ix, region);
                    if let Some(key) = key
                        && streamed_spec.is_none()
                        && this.diff_text_segments_cache_get(key, cache_epoch).is_none()
                    {
                        let raw_text = file_diff_split_side_text(&row, is_left);
                        if let Some(raw_text) = raw_text {
                            let (styled, is_pending) = build_file_diff_cached_styled_text_for_prepared_line_nonblocking(
                                theme,
                                raw_text,
                                row_word_ranges.as_slice(),
                                "",
                                DiffSyntaxConfig {
                                    language,
                                    mode: syntax_mode,
                                },
                                row_word_kind,
                                rows::prepared_diff_syntax_line_for_one_based_line(
                                    syntax_document,
                                    file_diff_split_side_line(&row, is_left),
                                ),
                            );
                            if is_pending {
                                this.ensure_prepared_syntax_chunk_poll(cx);
                            }
                            this.diff_text_segments_cache_set(key, cache_epoch, styled);
                        }
                    }

                    let row_has_content = file_diff_split_side_text(&row, is_left).is_some();
                    let styled = if row_has_content && streamed_spec.is_none() {
                        if let Some(key) = key {
                            this.diff_text_segments_cache_get_for_query(
                                key,
                                query.as_ref(),
                                query_options,
                                cache_epoch,
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    debug_assert!(
                        !row_has_content
                            || key.is_none()
                            || streamed_spec.is_some()
                            || styled.is_some(),
                        "diff text segment cache missing for split-{column:?} row {row_ix} after populate"
                    );

                    patch_split_column_row(
                        theme,
                        ui_scale_percent,
                        column,
                        visible_ix,
                        selected,
                        min_width,
                        &row,
                        visual_kind,
                        styled,
                        streamed_spec,
                        reveal_whitespace_chars,
                        show_line_numbers,
                        wrap,
                        annotation_width,
                        if is_left {
                            blame_ctx
                                .as_ref()
                                .and_then(|ctx| build_row_blame_paint_tracked(ctx, matches!(visual_kind, FileDiffRowKind::Context), row.old_line, row.new_line, &blame_prev_nl, wrap, theme))
                        } else {
                        None
                    },
                    annot_hover,
                    stage_area,
                    stage_hover,
                    coverage,
                    cx,
                )
                })
                .collect();
        }

        let theme = this.theme;
        let cache_epoch = 0u64;
        let syntax_mode = this.patch_diff_syntax_mode();
        let blame_prev_nl = std::cell::Cell::new(BlamePrev::default());
        range
            .map(|visible_ix| {
                let selected = this
                    .diff_selection_range
                    .is_some_and(|(a, b)| visible_ix >= a.min(b) && visible_ix <= a.max(b));
                let show_line_numbers = this.diff_show_line_numbers;
                let wrap = this.diff_text_wrap_for_visible_ix(visible_ix);

                let Some(row_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                    return diff_placeholder_row((id_missing, visible_ix), theme, ui_scale_percent);
                };
                let Some(row) = this.patch_diff_split_row(row_ix) else {
                    return diff_placeholder_row((id_oob, visible_ix), theme, ui_scale_percent);
                };

                match row {
                    PatchSplitRow::Aligned {
                        row,
                        old_src_ix,
                        new_src_ix,
                    } => {
                        let src_ix = if is_left { old_src_ix } else { new_src_ix };
                        let old_changed = old_src_ix.is_some_and(|src_ix| {
                            matches!(this.patch_visual_line_kind(src_ix), DiffLineKind::Remove)
                        });
                        let new_changed = new_src_ix.is_some_and(|src_ix| {
                            matches!(this.patch_visual_line_kind(src_ix), DiffLineKind::Add)
                        });
                        let visual_kind = match (old_changed, new_changed) {
                            (true, true) => FileDiffRowKind::Modify,
                            (true, false) => FileDiffRowKind::Remove,
                            (false, true) => FileDiffRowKind::Add,
                            (false, false) => FileDiffRowKind::Context,
                        };
                        let (streamed_spec, styled) = if let Some(src_ix) = src_ix {
                            let language =
                                this.diff_language_for_src_ix.get(src_ix).copied().flatten();
                            this.ensure_patch_diff_word_highlight_for_src_ix(src_ix);
                            let word_ranges = this
                                .diff_word_highlights
                                .get(src_ix)
                                .and_then(|r| r.as_ref().cloned())
                                .unwrap_or_default();
                            let word_kind =
                                diff_line_word_kind(this.patch_visual_line_kind(src_ix));
                            let streamed_spec = file_diff_split_side_text_owned(&row, is_left)
                                .and_then(|raw_text| {
                                    heuristic_streamed_diff_text_spec(
                                        raw_text,
                                        &query,
                                        query_options,
                                        query_matcher.clone(),
                                        word_ranges.clone(),
                                        word_kind,
                                        language,
                                        syntax_mode,
                                    )
                                });
                            if streamed_spec.is_none()
                                && this
                                    .diff_text_segments_cache_get(src_ix, cache_epoch)
                                    .is_none()
                            {
                                let computed = if let Some(raw_text) =
                                    file_diff_split_side_text(&row, is_left)
                                {
                                    build_file_diff_cached_styled_text(
                                        theme,
                                        raw_text,
                                        word_ranges.as_slice(),
                                        "",
                                        language,
                                        syntax_mode,
                                        word_kind,
                                    )
                                } else {
                                    build_cached_diff_styled_text(
                                        theme,
                                        "",
                                        word_ranges.as_slice(),
                                        "",
                                        language,
                                        syntax_mode,
                                        word_kind,
                                    )
                                };
                                this.diff_text_segments_cache_set(src_ix, cache_epoch, computed);
                            }

                            let styled = if streamed_spec.is_none() {
                                this.diff_text_segments_cache_get_for_query(
                                    src_ix,
                                    query.as_ref(),
                                    query_options,
                                    cache_epoch,
                                )
                            } else {
                                None
                            };
                            (streamed_spec, styled)
                        } else {
                            (None, None)
                        };

                        patch_split_column_row(
                            theme,
                            ui_scale_percent,
                            column,
                            visible_ix,
                            selected,
                            min_width,
                            &row,
                            visual_kind,
                            styled,
                            streamed_spec,
                            reveal_whitespace_chars,
                            show_line_numbers,
                            wrap,
                            annotation_width,
                            if is_left {
                                blame_ctx.as_ref().and_then(|ctx| {
                                    build_row_blame_paint_tracked(
                                        ctx,
                                        matches!(visual_kind, FileDiffRowKind::Context),
                                        row.old_line,
                                        row.new_line,
                                        &blame_prev_nl,
                                        wrap,
                                        theme,
                                    )
                                })
                            } else {
                                None
                            },
                            annot_hover,
                            stage_area,
                            stage_hover,
                            coverage,
                            cx,
                        )
                    }
                    PatchSplitRow::Raw { src_ix, click_kind } => {
                        if this.patch_diff_row(src_ix).is_none() {
                            return diff_placeholder_row(
                                (id_src_oob, visible_ix),
                                theme,
                                ui_scale_percent,
                            );
                        };
                        let file_stat = this.diff_file_stats.get(src_ix).and_then(|s| *s);
                        let should_style = !query.is_empty();
                        if should_style
                            && this
                                .diff_text_segments_cache_get(src_ix, cache_epoch)
                                .is_none()
                        {
                            let display = this.diff_text_line_for_region(visible_ix, region);
                            let computed = build_cached_diff_styled_text(
                                theme,
                                display.as_ref(),
                                &[],
                                "",
                                None,
                                syntax_mode,
                                None,
                            );
                            this.diff_text_segments_cache_set(src_ix, cache_epoch, computed);
                        }
                        let Some(line) = this.patch_diff_row(src_ix) else {
                            return diff_placeholder_row(
                                (id_src_oob, visible_ix),
                                theme,
                                ui_scale_percent,
                            );
                        };
                        if should_hide_unified_diff_header_line(&line) {
                            return div()
                                .id((id_hidden, visible_ix))
                                .h(px(0.0))
                                .into_any_element();
                        }
                        let context_menu_active = click_kind == DiffClickKind::HunkHeader
                            && this.active_repo_id().is_some_and(|repo_id| {
                                let invoker: SharedString =
                                    format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
                                this.active_context_menu_invoker.as_ref() == Some(&invoker)
                            });
                        let header_display = this.diff_header_display_cache.get(&src_ix).cloned();
                        let styled = if should_style {
                            this.diff_text_segments_cache_get_for_query(
                                src_ix,
                                query.as_ref(),
                                query_options,
                                cache_epoch,
                            )
                        } else {
                            None
                        };
                        patch_split_header_row(
                            theme,
                            ui_scale_percent,
                            column,
                            visible_ix,
                            click_kind,
                            selected,
                            min_width,
                            &line,
                            file_stat,
                            header_display,
                            styled,
                            context_menu_active,
                            cx,
                        )
                    }
                }
            })
            .collect()
    }
}

/// The coverage overlay's one surface: the new-side line number's verdict
/// recolors the gutter. `None` leaves the diff's own gutter color.
pub(super) fn coverage_gutter_color(
    theme: AppTheme,
    report: &worktree_core::coverage::CoverageReport,
    path: &str,
    new_line: Option<u32>,
) -> Option<gpui::Rgba> {
    let status = report.line_status(path, new_line?)?;
    Some(match status {
        worktree_core::coverage::CoverageLineStatus::Covered => {
            theme.colors.status.success.foreground
        }
        worktree_core::coverage::CoverageLineStatus::Missed => {
            theme.colors.status.danger.foreground
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn diff_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    visible_ix: usize,
    click_kind: DiffClickKind,
    selected: bool,
    mode: DiffViewMode,
    min_width: Pixels,
    line: &AnnotatedDiffLine,
    visual_kind: DiffLineKind,
    file_stat: Option<(usize, usize)>,
    header_display: Option<SharedString>,
    styled: Option<&CachedDiffStyledText>,
    streamed_spec: Option<diff_canvas::StreamedDiffTextPaintSpec>,
    raw_text: Option<&str>,
    reveal_whitespace_chars: bool,
    context_menu_active: bool,
    show_line_numbers: bool,
    wrap: Option<diff_canvas::DiffTextWrapSlice>,
    annotation_width: Pixels,
    row_blame: Option<diff_canvas::RowBlamePaint>,
    annot_hover: Option<(usize, AnnotArea)>,
    stage_area: Option<DiffArea>,
    stage_hover: Option<diff_canvas::DiffStageHover>,
    coverage: Option<(&worktree_core::coverage::CoverageReport, &str)>,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let on_click = cx.listener(move |this, e: &ClickEvent, _w, cx| {
        if this.consume_suppress_click_after_drag() {
            cx.notify();
            return;
        }
        this.handle_patch_row_click(visible_ix, click_kind, e.modifiers().shift);
        cx.notify();
    });

    if matches!(click_kind, DiffClickKind::FileHeader) {
        let file =
            header_display.unwrap_or_else(|| SharedString::from(line.text.as_ref().to_owned()));
        let mut row = div()
            .id(("diff_file_hdr", visible_ix))
            .h(diff_file_header_height(ui_scale_percent))
            .w_full()
            .min_w(min_width)
            .flex()
            .items_center()
            .justify_between()
            .px_2()
            .bg(crate::theme::content_header_bg(theme))
            .border_b_1()
            .border_color(theme.colors.stroke.default)
            .text_sm()
            .font_weight(FontWeight::BOLD)
            .child(selectable_cached_diff_text(
                visible_ix,
                DiffTextRegion::Inline,
                DiffClickKind::FileHeader,
                theme.colors.foreground.primary,
                None,
                file,
                cx,
            ))
            .when(file_stat.is_some_and(|(a, r)| a > 0 || r > 0), |this| {
                let (a, r) = file_stat.unwrap_or_default();
                this.child(components::diff_stat(theme, ui_scale_percent, a, r))
            })
            .on_click(on_click);

        if selected {
            row = row.bg(focused_diff_neutral_row_bg(theme));
        }

        return row.into_any_element();
    }

    if matches!(click_kind, DiffClickKind::HunkHeader) {
        let display =
            header_display.unwrap_or_else(|| SharedString::from(line.text.as_ref().to_owned()));

        let mut row = div()
            .id(("diff_hunk_hdr", visible_ix))
            .h(diff_hunk_header_height(ui_scale_percent))
            .w_full()
            .min_w(min_width)
            .flex()
            .items_center()
            .px_2()
            .bg(with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.10 } else { 0.07 },
            ))
            .border_b_1()
            .border_color(with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.28 } else { 0.22 },
            ))
            .text_xs()
            .text_color(theme.colors.foreground.secondary)
            .child(selectable_cached_diff_text(
                visible_ix,
                DiffTextRegion::Inline,
                DiffClickKind::HunkHeader,
                theme.colors.foreground.secondary,
                None,
                display,
                cx,
            ))
            .on_click(on_click);
        let on_right_click = cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            if this.is_inline_submodule_diff_active() {
                return;
            }
            let Some(repo_id) = this.active_repo_id() else {
                return;
            };
            let Some(src_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                return;
            };
            let context_menu_invoker: SharedString =
                format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
            this.activate_context_menu_invoker(context_menu_invoker, cx);
            this.open_popover_at(
                PopoverKind::DiffHunkMenu { repo_id, src_ix },
                e.position,
                window,
                cx,
            );
        });
        row = row.on_mouse_down(MouseButton::Right, on_right_click);

        if selected {
            row = row.bg(focused_diff_neutral_row_bg(theme));
        }
        if context_menu_active {
            row = row.bg(theme.colors.interaction.pressed_background);
        }

        return row.into_any_element();
    }

    let (mut bg, fg, gutter_fg) = diff_line_colors(theme, visual_kind);
    if selected {
        bg = focused_diff_line_bg(theme, visual_kind);
    }

    let show_row_numbers = wrap.is_none_or(|wrap| wrap.wrap_ix == 0);
    // Coverage overlay: covered and missed lines recolor the gutter (the
    // new-side number's verdict; the old side has no coverage meaning).
    let gutter_fg = if show_row_numbers {
        coverage
            .and_then(|(report, path)| coverage_gutter_color(theme, report, path, line.new_line))
            .unwrap_or(gutter_fg)
    } else {
        gutter_fg
    };
    // Continuation rows of a wrapped line share the line's gutter, so only the
    // first visual row carries the stage button.
    let stage_area = stage_area.filter(|_| show_row_numbers);
    let old = if show_row_numbers {
        line_number_string(line.old_line)
    } else {
        SharedString::default()
    };
    let new = if show_row_numbers {
        line_number_string(line.new_line)
    } else {
        SharedString::default()
    };

    match mode {
        DiffViewMode::Inline => {
            // `visual_kind`, not `line.kind`: in ignore-whitespace mode a
            // whitespace-only change renders as context, and a row painted as
            // context must not offer to stage itself. The split columns derive
            // their button from the same value.
            let stage = stage_area
                .and_then(|area| stage_gutter_spec(area, DiffStageSlot::Inline, visual_kind));
            diff_canvas::inline_diff_line_row_canvas(
                theme,
                cx.entity(),
                ui_scale_percent,
                visible_ix,
                min_width,
                selected,
                old,
                new,
                bg,
                fg,
                gutter_fg,
                styled,
                streamed_spec,
                raw_text,
                reveal_whitespace_chars,
                show_line_numbers,
                wrap,
                annotation_width,
                row_blame,
                annot_hover,
                stage,
                stage_hover,
            )
        }
        DiffViewMode::Split => {
            let left_kind = if visual_kind == DiffLineKind::Remove {
                DiffLineKind::Remove
            } else {
                DiffLineKind::Context
            };
            let right_kind = if visual_kind == DiffLineKind::Add {
                DiffLineKind::Add
            } else {
                DiffLineKind::Context
            };

            let (mut left_bg, left_fg, left_gutter) = diff_line_colors(theme, left_kind);
            let (mut right_bg, right_fg, right_gutter) = diff_line_colors(theme, right_kind);
            if selected {
                left_bg = focused_diff_line_bg(theme, left_kind);
                right_bg = focused_diff_line_bg(theme, right_kind);
            }

            let (left_text, right_text) = match line.kind {
                DiffLineKind::Remove => (styled, None),
                DiffLineKind::Add => (None, styled),
                DiffLineKind::Context => (styled, styled),
                _ => (styled, None),
            };
            let left_streamed_spec = match line.kind {
                DiffLineKind::Remove | DiffLineKind::Context => streamed_spec.clone(),
                _ => None,
            };
            let right_streamed_spec = match line.kind {
                DiffLineKind::Add | DiffLineKind::Context => streamed_spec,
                _ => None,
            };
            let left_raw_text = match line.kind {
                DiffLineKind::Remove | DiffLineKind::Context => raw_text,
                _ => None,
            };
            let right_raw_text = match line.kind {
                DiffLineKind::Add | DiffLineKind::Context => raw_text,
                _ => None,
            };

            // A split row shows removals on the left and additions on the right,
            // so each side gets the button for its own kind of change.
            let stage_left = stage_area
                .and_then(|area| stage_gutter_spec(area, DiffStageSlot::SplitLeft, visual_kind))
                .filter(|spec| spec.kind == DiffLineKind::Remove);
            let stage_right = stage_area
                .and_then(|area| stage_gutter_spec(area, DiffStageSlot::SplitRight, visual_kind))
                .filter(|spec| spec.kind == DiffLineKind::Add);

            diff_canvas::split_diff_line_row_canvas(
                theme,
                cx.entity(),
                ui_scale_percent,
                visible_ix,
                min_width,
                selected,
                old,
                new,
                left_bg,
                left_fg,
                left_gutter,
                right_bg,
                right_fg,
                right_gutter,
                left_text,
                right_text,
                left_streamed_spec,
                right_streamed_spec,
                left_raw_text,
                right_raw_text,
                reveal_whitespace_chars,
                show_line_numbers,
                wrap,
                annotation_width,
                row_blame,
                annot_hover,
                stage_left,
                stage_right,
                stage_hover,
            )
        }
    }
}

/// Build the stage-gutter spec for a change line, or `None` for anything that
/// cannot be staged line-by-line (context lines and headers).
fn stage_gutter_spec(
    area: DiffArea,
    slot: DiffStageSlot,
    kind: DiffLineKind,
) -> Option<diff_canvas::StageGutterSpec> {
    matches!(kind, DiffLineKind::Add | DiffLineKind::Remove)
        .then_some(diff_canvas::StageGutterSpec { area, slot, kind })
}

#[allow(clippy::too_many_arguments)]
fn patch_split_column_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    column: PatchSplitColumn,
    visible_ix: usize,
    selected: bool,
    min_width: Pixels,
    row: &worktree_core::file_diff::FileDiffRow,
    visual_kind: FileDiffRowKind,
    styled: Option<&CachedDiffStyledText>,
    streamed_spec: Option<diff_canvas::StreamedDiffTextPaintSpec>,
    reveal_whitespace_chars: bool,
    show_line_numbers: bool,
    wrap: Option<diff_canvas::DiffTextWrapSlice>,
    annotation_width: Pixels,
    row_blame: Option<diff_canvas::RowBlamePaint>,
    annot_hover: Option<(usize, AnnotArea)>,
    stage_area: Option<DiffArea>,
    stage_hover: Option<diff_canvas::DiffStageHover>,
    coverage: Option<(&worktree_core::coverage::CoverageReport, &str)>,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let line_kind = match (column, visual_kind) {
        (PatchSplitColumn::Left, FileDiffRowKind::Remove | FileDiffRowKind::Modify) => {
            DiffLineKind::Remove
        }
        (PatchSplitColumn::Right, FileDiffRowKind::Add | FileDiffRowKind::Modify) => {
            DiffLineKind::Add
        }
        _ => DiffLineKind::Context,
    };
    let (mut bg, fg, gutter_fg) = diff_line_colors(theme, line_kind);
    // Coverage overlay on the split view's right column only: the old side
    // has no coverage meaning.
    let gutter_fg = if column == PatchSplitColumn::Right {
        coverage
            .and_then(|(report, path)| coverage_gutter_color(theme, report, path, row.new_line))
            .unwrap_or(gutter_fg)
    } else {
        gutter_fg
    };
    if selected {
        bg = focused_diff_line_bg(theme, line_kind);
    }

    let show_row_number = wrap.is_none_or(|wrap| wrap.wrap_ix == 0);
    let line_no = if show_row_number {
        match column {
            PatchSplitColumn::Left => line_number_string(row.old_line),
            PatchSplitColumn::Right => line_number_string(row.new_line),
        }
    } else {
        SharedString::default()
    };

    // `line_kind` is already resolved per column, so each side offers the button
    // only for the change it actually shows.
    let stage = stage_area.filter(|_| show_row_number).and_then(|area| {
        stage_gutter_spec(
            area,
            match column {
                PatchSplitColumn::Left => DiffStageSlot::SplitLeft,
                PatchSplitColumn::Right => DiffStageSlot::SplitRight,
            },
            line_kind,
        )
    });

    diff_canvas::patch_split_column_row_canvas(
        theme,
        cx.entity(),
        ui_scale_percent,
        column,
        visible_ix,
        min_width,
        selected,
        bg,
        fg,
        gutter_fg,
        line_no,
        styled,
        streamed_spec,
        match column {
            PatchSplitColumn::Left => row.old.as_ref(),
            PatchSplitColumn::Right => row.new.as_ref(),
        }
        .map(|text| text.as_ref()),
        reveal_whitespace_chars,
        show_line_numbers,
        wrap,
        annotation_width,
        row_blame,
        annot_hover,
        stage,
        stage_hover,
    )
}

#[allow(clippy::too_many_arguments)]
fn patch_split_header_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    column: PatchSplitColumn,
    visible_ix: usize,
    click_kind: DiffClickKind,
    selected: bool,
    min_width: Pixels,
    line: &AnnotatedDiffLine,
    file_stat: Option<(usize, usize)>,
    header_display: Option<SharedString>,
    styled: Option<&CachedDiffStyledText>,
    context_menu_active: bool,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let on_click = cx.listener(move |this, e: &ClickEvent, _w, cx| {
        if this.consume_suppress_click_after_drag() {
            cx.notify();
            return;
        }
        this.handle_patch_row_click(visible_ix, click_kind, e.modifiers().shift);
        cx.notify();
    });
    let region = match column {
        PatchSplitColumn::Left => DiffTextRegion::SplitLeft,
        PatchSplitColumn::Right => DiffTextRegion::SplitRight,
    };

    match click_kind {
        DiffClickKind::FileHeader => {
            let display =
                header_display.unwrap_or_else(|| SharedString::from(line.text.as_ref().to_owned()));
            let mut row = div()
                .id((
                    match column {
                        PatchSplitColumn::Left => "diff_split_left_file_hdr",
                        PatchSplitColumn::Right => "diff_split_right_file_hdr",
                    },
                    visible_ix,
                ))
                .h(diff_file_header_height(ui_scale_percent))
                .w_full()
                .min_w(min_width)
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .bg(crate::theme::content_header_bg(theme))
                .border_b_1()
                .border_color(theme.colors.stroke.default)
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(selectable_cached_diff_text(
                    visible_ix,
                    region,
                    DiffClickKind::FileHeader,
                    theme.colors.foreground.primary,
                    styled,
                    display,
                    cx,
                ))
                .when(file_stat.is_some_and(|(a, r)| a > 0 || r > 0), |this| {
                    let (a, r) = file_stat.unwrap_or_default();
                    this.child(components::diff_stat(theme, ui_scale_percent, a, r))
                })
                .on_click(on_click);

            if selected {
                row = row.bg(focused_diff_neutral_row_bg(theme));
            }

            row.into_any_element()
        }
        DiffClickKind::HunkHeader => {
            let display =
                header_display.unwrap_or_else(|| SharedString::from(line.text.as_ref().to_owned()));

            let mut row = div()
                .id((
                    match column {
                        PatchSplitColumn::Left => "diff_split_left_hunk_hdr",
                        PatchSplitColumn::Right => "diff_split_right_hunk_hdr",
                    },
                    visible_ix,
                ))
                .h(diff_hunk_header_height(ui_scale_percent))
                .w_full()
                .min_w(min_width)
                .flex()
                .items_center()
                .px_2()
                .bg(with_alpha(
                    theme.colors.accent.foreground,
                    if theme.is_dark { 0.10 } else { 0.07 },
                ))
                .border_b_1()
                .border_color(with_alpha(
                    theme.colors.accent.foreground,
                    if theme.is_dark { 0.28 } else { 0.22 },
                ))
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(selectable_cached_diff_text(
                    visible_ix,
                    region,
                    DiffClickKind::HunkHeader,
                    theme.colors.foreground.secondary,
                    styled,
                    display,
                    cx,
                ))
                .on_click(on_click);
            let on_right_click = cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.is_inline_submodule_diff_active() {
                    return;
                }
                let Some(repo_id) = this.active_repo_id() else {
                    return;
                };
                let Some(row_ix) = this.diff_mapped_ix_for_visible_ix(visible_ix) else {
                    return;
                };
                let Some(PatchSplitRow::Raw {
                    src_ix,
                    click_kind: DiffClickKind::HunkHeader,
                }) = this.patch_diff_split_row(row_ix)
                else {
                    return;
                };
                let context_menu_invoker: SharedString =
                    format!("diff_hunk_menu_{}_{}", repo_id.0, src_ix).into();
                this.activate_context_menu_invoker(context_menu_invoker, cx);
                this.open_popover_at(
                    PopoverKind::DiffHunkMenu { repo_id, src_ix },
                    e.position,
                    window,
                    cx,
                );
            });
            row = row.on_mouse_down(MouseButton::Right, on_right_click);

            if selected {
                row = row.bg(focused_diff_neutral_row_bg(theme));
            }
            if context_menu_active {
                row = row.bg(theme.colors.interaction.pressed_background);
            }

            row.into_any_element()
        }
        DiffClickKind::Line => patch_split_meta_row(
            theme,
            ui_scale_percent,
            column,
            visible_ix,
            selected,
            line,
            cx,
        ),
    }
}

fn patch_split_meta_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    column: PatchSplitColumn,
    visible_ix: usize,
    selected: bool,
    line: &AnnotatedDiffLine,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let on_click = cx.listener(move |this, e: &ClickEvent, _w, cx| {
        if this.consume_suppress_click_after_drag() {
            cx.notify();
            return;
        }
        this.handle_patch_row_click(visible_ix, DiffClickKind::Line, e.modifiers().shift);
        cx.notify();
    });
    let region = match column {
        PatchSplitColumn::Left => DiffTextRegion::SplitLeft,
        PatchSplitColumn::Right => DiffTextRegion::SplitRight,
    };

    let (bg, fg, _) = diff_line_colors(theme, line.kind);
    let mut row = div()
        .id((
            match column {
                PatchSplitColumn::Left => "diff_split_left_meta",
                PatchSplitColumn::Right => "diff_split_right_meta",
            },
            visible_ix,
        ))
        .h(diff_row_height(ui_scale_percent))
        .flex()
        .items_center()
        .px_2()
        .text_xs()
        .bg(bg)
        .text_color(fg)
        .whitespace_nowrap()
        .child(selectable_cached_diff_text(
            visible_ix,
            region,
            DiffClickKind::Line,
            fg,
            None,
            SharedString::from(line.text.as_ref().to_owned()),
            cx,
        ))
        .on_click(on_click);

    if selected {
        row = row.bg(focused_diff_line_bg(theme, line.kind));
    }

    row.into_any_element()
}
