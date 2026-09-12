//! `MainPaneView` two-way diff builders: left/right columns, the aligned diff
//! columns and the context fold row.

use super::super::conflict_resolver;
use super::super::diff_text::*;
use super::super::perf::{self, ViewPerfSpan};
use super::super::*;
use super::conflict_canvas::{self, ConflictChunkContext};
use super::styled_text::{
    CONFLICT_FOLD_REVEAL_BUTTON_PX, CONFLICT_FOLD_REVEAL_ICON_PX, ConflictRowStyledText,
    ConflictRowStyledTextValue, build_conflict_cached_diff_styled_text_with_source_identity,
    build_conflict_row_base_styled, conflict_diff_line_number_cell, conflict_diff_query_matcher,
    conflict_diff_text_cell, conflict_display_text, conflict_input_row_min_width, row_emphasis,
    split_cell_bg, texts_equal_ignoring_whitespace, two_way_aligned_input_row_menu_targets,
    two_way_split_input_row_menu_targets,
};
use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};

// @split-module: impl_diff
impl MainPaneView {
    // ── Per-column two-way diff render functions ────────────────────────
    pub(in super::super::super) fn render_conflict_diff_left_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        Self::render_conflict_diff_column_rows(this, range, ConflictPickSide::Ours, window, cx)
    }

    pub(in super::super::super) fn render_conflict_diff_right_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        Self::render_conflict_diff_column_rows(this, range, ConflictPickSide::Theirs, window, cx)
    }

    fn render_conflict_diff_column_rows(
        this: &mut Self,
        range: Range<usize>,
        side: ConflictPickSide,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        // section 30 aligned row space: two-way full mode shares the three-way
        // projection. The block-local path below remains for giant files and
        // partially loaded sides.
        if this.conflict_resolver.two_way_uses_aligned_rows() {
            return Self::render_conflict_diff_aligned_column_rows(this, range, side, window, cx);
        }
        let _perf_scope = perf::span(ViewPerfSpan::RenderResolverDiffRows);
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query = query.as_ref().to_string();
        this.sync_conflict_diff_query_overlay_caches(query.as_str(), query_options);
        let query_matcher = conflict_diff_query_matcher(query.as_str(), query_options);
        let current_match_row = this.diff_search_current_match_row();
        let syntax_lang = this.conflict_row_syntax_language();
        let syntax_mode = DiffSyntaxMode::Auto;
        let theme = this.theme;
        let editor_font_family = crate::font_preferences::current_editor_font_family(cx);
        let show_ws = this.reveal_whitespace_chars;
        let query_text = this.conflict_diff_query_cache_query.clone();
        let query = query_text.as_ref();
        let column = match side {
            ConflictPickSide::Ours => ThreeWayColumn::Ours,
            ConflictPickSide::Theirs => ThreeWayColumn::Theirs,
        };

        let (div_id_prefix, canvas_id_prefix, chunk_menu_prefix, input_menu_prefix) = match side {
            ConflictPickSide::Ours => (
                "conflict_diff_col_ours",
                "conflict_diff_canvas_ours",
                "resolver_two_way_split_ours_chunk_menu",
                "resolver_two_way_split_ours_input_menu",
            ),
            ConflictPickSide::Theirs => (
                "conflict_diff_col_theirs",
                "conflict_diff_canvas_theirs",
                "resolver_two_way_split_theirs_chunk_menu",
                "resolver_two_way_split_theirs_input_menu",
            ),
        };

        range
            .map(|visible_row_ix| {
                let Some(visible_row) = this
                    .conflict_resolver
                    .two_way_split_visible_row(visible_row_ix)
                else {
                    return div()
                        .id((div_id_prefix, visible_row_ix))
                        .h(conflict_row_height(ui_scale_percent))
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child("")
                        .into_any_element();
                };
                let conflict_resolver::TwoWaySplitVisibleRow {
                    source_row_ix: row_ix,
                    row,
                    conflict_ix,
                } = visible_row;
                let visual_kind = this.conflict_resolver.two_way_split_visual_kind_at(
                    row_ix,
                    &row,
                    this.diff_whitespace_mode,
                );

                let (text_opt, line_no, document) = match side {
                    ConflictPickSide::Ours => (
                        row.old.as_ref(),
                        row.old_line,
                        this.conflict_three_way_prepared_syntax_documents.ours,
                    ),
                    ConflictPickSide::Theirs => (
                        row.new.as_ref(),
                        row.new_line,
                        this.conflict_three_way_prepared_syntax_documents.theirs,
                    ),
                };

                let text = SharedString::new(text_opt.map(AsRef::as_ref).unwrap_or_default());
                let styling_enabled = this.conflict_row_styling_enabled();
                let word_hl = if styling_enabled
                    && !matches!(
                        visual_kind,
                        worktree_core::file_diff::FileDiffRowKind::Context
                    ) {
                    this.conflict_resolver
                        .two_way_split_word_highlight_for_row(row_ix, &row)
                } else {
                    None
                };
                let word_ranges = match side {
                    ConflictPickSide::Ours => word_hl
                        .as_ref()
                        .map(|pair| pair.0.as_slice())
                        .unwrap_or(&[]),
                    ConflictPickSide::Theirs => word_hl
                        .as_ref()
                        .map(|pair| pair.1.as_slice())
                        .unwrap_or(&[]),
                };
                let styled_result = Self::conflict_split_row_styled(
                    theme,
                    &mut this.conflict_diff_segments_cache_split,
                    &mut this.conflict_diff_query_segments_cache_split,
                    row_ix,
                    side,
                    text_opt.map(AsRef::as_ref),
                    word_ranges,
                    query,
                    query_options,
                    query_matcher.as_ref(),
                    row_emphasis(current_match_row, visible_row_ix),
                    syntax_lang,
                    syntax_mode,
                    prepared_diff_syntax_line_for_one_based_line(document, line_no),
                );
                if styled_result.pending {
                    this.ensure_prepared_syntax_chunk_poll(cx);
                }
                let styled = styled_result.resolve(
                    &this.conflict_diff_segments_cache_split,
                    &this.conflict_diff_query_segments_cache_split,
                    (row_ix, side),
                );

                let bg = split_cell_bg(theme, visual_kind, side);
                let fg = if text_opt.is_some() {
                    theme.colors.foreground.primary
                } else {
                    theme.colors.foreground.secondary
                };
                let display_text = conflict_display_text(&text, styled, show_ws);
                let show_line_numbers = this.mergetool_show_line_numbers;
                let min_width = conflict_input_row_min_width(
                    window,
                    &display_text,
                    editor_font_family.as_str(),
                    show_line_numbers,
                    ui_scale_percent,
                );

                let is_active_conflict = this.conflict_resolver.conflict_is_active(conflict_ix);
                if this.conflict_canvas_rows_enabled {
                    let chunk_context_data = conflict_ix.map(|conflict_ix| ConflictChunkContext {
                        conflict_ix,
                        has_base: this
                            .conflict_resolver
                            .conflict_has_base
                            .get(conflict_ix)
                            .copied()
                            .unwrap_or(false),
                        selected_choices: this
                            .conflict_resolver_selected_choices_for_conflict_ix(conflict_ix),
                    });
                    return conflict_canvas::single_column_conflict_canvas(
                        theme,
                        cx.entity(),
                        canvas_id_prefix,
                        visible_row_ix,
                        row_ix,
                        min_width,
                        show_line_numbers,
                        line_number_string(line_no),
                        bg,
                        fg,
                        text,
                        styled,
                        show_ws,
                        chunk_context_data,
                        chunk_menu_prefix,
                        false,
                        None,
                        is_active_conflict,
                        // Block-local two-way rows are not in the shared aligned
                        // space, so split selection and manual alignment are
                        // both unavailable here.
                        None,
                        None,
                        Some(column),
                        ui_scale_percent,
                    );
                }

                let mut cell = div()
                    .id((div_id_prefix, row_ix))
                    .relative()
                    .w_full()
                    .min_w(min_width)
                    .h(conflict_row_height(ui_scale_percent))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .bg(bg)
                    .text_color(fg)
                    .whitespace_nowrap()
                    .when(is_active_conflict, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(conflict_scaled_px(
                                    CONFLICT_ROW_ACCENT_BAR_WIDTH_PX,
                                    ui_scale_percent,
                                ))
                                .bg(theme.colors.accent.foreground),
                        )
                    })
                    .when(show_line_numbers, |d| {
                        d.child(conflict_diff_line_number_cell(
                            theme,
                            line_number_string(line_no),
                            ui_scale_percent,
                        ))
                    })
                    .child(conflict_diff_text_cell(text.clone(), styled, show_ws));

                if let Some(conflict_ix) = conflict_ix {
                    let has_base = this
                        .conflict_resolver
                        .conflict_has_base
                        .get(conflict_ix)
                        .copied()
                        .unwrap_or(false);
                    let selected_choices =
                        this.conflict_resolver_selected_choices_for_conflict_ix(conflict_ix);
                    let (line_label, line_target, chunk_label, chunk_target) =
                        two_way_split_input_row_menu_targets(row_ix, conflict_ix, side);
                    cell = cell.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                            // section 30: clicking a conflict block body selects it.
                            this.conflict_resolver_select_conflict(conflict_ix, cx);
                        }),
                    );
                    cell = cell.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            if e.modifiers.shift {
                                let invoker: SharedString =
                                    format!("{}_{}_{}", input_menu_prefix, conflict_ix, row_ix)
                                        .into();
                                this.open_conflict_resolver_input_row_context_menu(
                                    invoker,
                                    line_label.clone(),
                                    line_target.clone(),
                                    chunk_label.clone(),
                                    chunk_target.clone(),
                                    e.position,
                                    window,
                                    cx,
                                );
                            } else {
                                let invoker: SharedString =
                                    format!("{}_{}_{}", chunk_menu_prefix, conflict_ix, row_ix)
                                        .into();
                                this.open_conflict_resolver_chunk_context_menu(
                                    invoker,
                                    conflict_ix,
                                    has_base,
                                    false,
                                    selected_choices.clone(),
                                    None,
                                    e.position,
                                    window,
                                    cx,
                                );
                            }
                        }),
                    );
                }

                cell.into_any_element()
            })
            .collect()
    }

    /// section 30 aligned row space: two-way full mode. Renders one column of the
    /// ours↔theirs diff over the shared three-way visible projection —
    /// whole-file rows, context folds, and collapsed resolved blocks — while
    /// keeping the two-way diff styling (add/remove/modify backgrounds and
    /// ours↔theirs word highlights). Row keys for the styled-text caches are
    /// aligned rows, which are stable for the session.
    fn render_conflict_diff_aligned_column_rows(
        this: &mut Self,
        range: Range<usize>,
        side: ConflictPickSide,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        use worktree_core::file_diff::FileDiffRowKind as RK;

        let _perf_scope = perf::span(ViewPerfSpan::RenderResolverDiffRows);
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query = query.as_ref().to_string();
        this.sync_conflict_diff_query_overlay_caches(query.as_str(), query_options);
        let query_matcher = conflict_diff_query_matcher(query.as_str(), query_options);
        let current_match_row = this.diff_search_current_match_row();
        let syntax_lang = this.conflict_row_syntax_language();
        let syntax_mode = DiffSyntaxMode::Auto;
        let theme = this.theme;
        let editor_font_family = crate::font_preferences::current_editor_font_family(cx);
        let show_ws = this.reveal_whitespace_chars;
        let query_text = this.conflict_diff_query_cache_query.clone();
        let query = query_text.as_ref();
        let whitespace_mode = this.diff_whitespace_mode;
        let styling_enabled = this.conflict_row_styling_enabled();

        let column = match side {
            ConflictPickSide::Ours => ThreeWayColumn::Ours,
            ConflictPickSide::Theirs => ThreeWayColumn::Theirs,
        };
        let document = match side {
            ConflictPickSide::Ours => this.conflict_three_way_prepared_syntax_documents.ours,
            ConflictPickSide::Theirs => this.conflict_three_way_prepared_syntax_documents.theirs,
        };
        let (div_id_prefix, canvas_id_prefix, chunk_menu_prefix, input_menu_prefix) = match side {
            ConflictPickSide::Ours => (
                "conflict_diff_col_ours",
                "conflict_diff_canvas_ours",
                "resolver_two_way_split_ours_chunk_menu",
                "resolver_two_way_split_ours_input_menu",
            ),
            ConflictPickSide::Theirs => (
                "conflict_diff_col_theirs",
                "conflict_diff_canvas_theirs",
                "resolver_two_way_split_theirs_chunk_menu",
                "resolver_two_way_split_theirs_input_menu",
            ),
        };
        let conflict_choices = this.conflict_resolver.conflict_choices.clone();

        let mut needs_chunk_poll = false;
        let mut elements = Vec::with_capacity(range.len());
        for vi in range {
            let Some(visible_item) = this.conflict_resolver.three_way_visible_item(vi) else {
                // Past-the-end rows exist only as bottom overscroll space.
                elements.push(
                    div()
                        .id((div_id_prefix, vi))
                        .w_full()
                        .h(conflict_row_height(ui_scale_percent))
                        .into_any_element(),
                );
                continue;
            };

            match visible_item {
                conflict_resolver::ThreeWayVisibleItem::CollapsedBlock(range_ix) => {
                    let label: SharedString = if matches!(side, ConflictPickSide::Ours) {
                        let choice_label = conflict_choices
                            .get(range_ix)
                            .map(|c| match *c {
                                conflict_resolver::ConflictChoice::Base => "Base (A)",
                                conflict_resolver::ConflictChoice::Ours => "Local (B)",
                                conflict_resolver::ConflictChoice::Theirs => "Remote (C)",
                                conflict_resolver::ConflictChoice::Both => "Local+Remote (B+C)",
                                _ => "Ordered source selection",
                            })
                            .unwrap_or("?");
                        format!("  Resolved: picked {choice_label}").into()
                    } else {
                        "".into()
                    };
                    let has_base = this
                        .conflict_resolver
                        .conflict_has_base
                        .get(range_ix)
                        .copied()
                        .unwrap_or(false);
                    let selected_choices =
                        this.conflict_resolver_selected_choices_for_conflict_ix(range_ix);
                    let collapsed = div()
                        .id((div_id_prefix, vi))
                        .relative()
                        .w_full()
                        .h(conflict_row_height(ui_scale_percent))
                        .flex()
                        .items_center()
                        .bg(with_alpha(
                            theme.colors.status.success.foreground,
                            if theme.is_dark { 0.08 } else { 0.06 },
                        ))
                        .when(
                            Some(range_ix) == this.conflict_resolver.active_conflict,
                            |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(conflict_scaled_px(
                                            CONFLICT_ROW_ACCENT_BAR_WIDTH_PX,
                                            ui_scale_percent,
                                        ))
                                        .bg(theme.colors.accent.foreground),
                                )
                            },
                        )
                        .px_2()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(label)
                        .cursor(CursorStyle::PointingHand)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                                // section 30: clicking a conflict block body selects it.
                                this.conflict_resolver_select_conflict(range_ix, cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                let invoker: SharedString = format!(
                                    "resolver_two_way_collapsed_chunk_menu_{}_{}",
                                    range_ix, vi
                                )
                                .into();
                                this.open_conflict_resolver_chunk_context_menu(
                                    invoker,
                                    range_ix,
                                    has_base,
                                    false,
                                    selected_choices.clone(),
                                    None,
                                    e.position,
                                    window,
                                    cx,
                                );
                            }),
                        );
                    elements.push(collapsed.into_any_element());
                }
                conflict_resolver::ThreeWayVisibleItem::CollapsedContext {
                    source_line_start,
                    len,
                    fold_id,
                } => {
                    elements.push(Self::conflict_context_fold_row(
                        theme,
                        div_id_prefix,
                        vi,
                        source_line_start,
                        len,
                        fold_id,
                        false,
                        cx,
                    ));
                }
                conflict_resolver::ThreeWayVisibleItem::Line(row) => {
                    let ours_line = this
                        .conflict_resolver
                        .three_way_side_line_for_row(ThreeWayColumn::Ours, row);
                    let theirs_line = this
                        .conflict_resolver
                        .three_way_side_line_for_row(ThreeWayColumn::Theirs, row);
                    let ours_text = ours_line.and_then(|l| {
                        this.conflict_resolver
                            .three_way_line_text(ThreeWayColumn::Ours, l)
                    });
                    let theirs_text = theirs_line.and_then(|l| {
                        this.conflict_resolver
                            .three_way_line_text(ThreeWayColumn::Theirs, l)
                    });

                    // Per-row diff kind from the aligned pair. Unlike the
                    // block-local path there is no run-level whitespace
                    // arbitration; a row is whitespace-equal on its own.
                    let visual_kind = match (ours_text, theirs_text) {
                        (Some(o), Some(t)) if o == t => RK::Context,
                        (Some(o), Some(t)) => {
                            if whitespace_mode != DiffWhitespaceMode::Show
                                && texts_equal_ignoring_whitespace(o, t)
                            {
                                RK::Context
                            } else {
                                RK::Modify
                            }
                        }
                        (Some(_), None) => RK::Remove,
                        (None, Some(_)) => RK::Add,
                        // Padding-only row (e.g. a base-only run): blank.
                        (None, None) => RK::Context,
                    };

                    // Word highlights are precomputed once per rebuild (shared by
                    // both columns); look up this aligned row and take the current
                    // side's ranges. Cloned into an owned Vec so it doesn't hold a
                    // borrow of `this` across the `&mut this` cache use below.
                    let word_ranges_owned: Vec<Range<usize>> =
                        if styling_enabled && matches!(visual_kind, RK::Modify) {
                            this.conflict_resolver
                                .two_way_aligned_word_highlights
                                .get(&row)
                                .map(|(o, n)| match side {
                                    ConflictPickSide::Ours => o.as_slice().to_vec(),
                                    ConflictPickSide::Theirs => n.as_slice().to_vec(),
                                })
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                    let word_ranges: &[Range<usize>] = &word_ranges_owned;

                    let (side_line, side_text) = match side {
                        ConflictPickSide::Ours => (ours_line, ours_text),
                        ConflictPickSide::Theirs => (theirs_line, theirs_text),
                    };
                    let has_text = side_text.is_some();
                    // kdiff3 behavior: per-column line numbers from the
                    // side's own file; padding rows have none.
                    let line_no_opt = side_line
                        .filter(|_| has_text)
                        .and_then(|l| u32::try_from(l + 1).ok());

                    let styled_result = Self::conflict_split_row_styled(
                        theme,
                        &mut this.conflict_diff_segments_cache_split,
                        &mut this.conflict_diff_query_segments_cache_split,
                        row,
                        side,
                        side_text,
                        word_ranges,
                        query,
                        query_options,
                        query_matcher.as_ref(),
                        row_emphasis(current_match_row, vi),
                        syntax_lang,
                        syntax_mode,
                        prepared_diff_syntax_line_for_one_based_line(document, line_no_opt),
                    );
                    if styled_result.pending {
                        needs_chunk_poll = true;
                    }
                    let styled = styled_result.resolve(
                        &this.conflict_diff_segments_cache_split,
                        &this.conflict_diff_query_segments_cache_split,
                        (row, side),
                    );

                    let text = SharedString::new(side_text.unwrap_or_default());
                    let bg = split_cell_bg(theme, visual_kind, side);
                    let fg = if has_text {
                        theme.colors.foreground.primary
                    } else {
                        theme.colors.foreground.secondary
                    };
                    let display_text = conflict_display_text(&text, styled, show_ws);
                    let show_line_numbers = this.mergetool_show_line_numbers;
                    let min_width = conflict_input_row_min_width(
                        window,
                        &display_text,
                        editor_font_family.as_str(),
                        show_line_numbers,
                        ui_scale_percent,
                    );

                    let conflict_ix = this
                        .conflict_resolver
                        .conflict_index_for_side_line(column, row);
                    let semantic_nav_target =
                        this.conflict_resolver.nav_target_index_for_aligned_row(row);
                    let is_active_conflict = this.conflict_resolver.conflict_is_active(conflict_ix)
                        || this
                            .conflict_resolver
                            .selected_nav_target_contains_aligned_row(row);
                    let row_selected = this.conflict_resolver.conflict_row_is_selected(row);
                    let row_selection_enabled =
                        this.conflict_resolver.conflict_row_selection_enabled();

                    if this.conflict_canvas_rows_enabled {
                        let chunk_context = conflict_ix.map(|conflict_ix| ConflictChunkContext {
                            conflict_ix,
                            has_base: this
                                .conflict_resolver
                                .conflict_has_base
                                .get(conflict_ix)
                                .copied()
                                .unwrap_or(false),
                            selected_choices: this
                                .conflict_resolver_selected_choices_for_conflict_ix(conflict_ix),
                        });
                        elements.push(conflict_canvas::single_column_conflict_canvas(
                            theme,
                            cx.entity(),
                            canvas_id_prefix,
                            vi,
                            row,
                            min_width,
                            show_line_numbers,
                            line_number_string(line_no_opt),
                            bg,
                            fg,
                            text,
                            styled,
                            show_ws,
                            chunk_context,
                            chunk_menu_prefix,
                            false,
                            semantic_nav_target,
                            is_active_conflict,
                            row_selection_enabled.then_some(row_selected),
                            // The two-way split shows ours/theirs only; a pin
                            // needs all three source columns to place it.
                            None,
                            Some(column),
                            ui_scale_percent,
                        ));
                        continue;
                    }

                    let mut cell = div()
                        .id((div_id_prefix, vi))
                        .relative()
                        .w_full()
                        .min_w(min_width)
                        .h(conflict_row_height(ui_scale_percent))
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .bg(bg)
                        .text_color(fg)
                        .whitespace_nowrap()
                        .when(is_active_conflict, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(conflict_scaled_px(
                                        CONFLICT_ROW_ACCENT_BAR_WIDTH_PX,
                                        ui_scale_percent,
                                    ))
                                    .bg(theme.colors.accent.foreground),
                            )
                        })
                        .when(row_selected, |d| {
                            d.child(div().absolute().inset_0().bg(with_alpha(
                                theme.colors.accent.foreground,
                                if theme.is_dark { 0.20 } else { 0.14 },
                            )))
                        })
                        .when(show_line_numbers, |d| {
                            d.child(conflict_diff_line_number_cell(
                                theme,
                                line_number_string(line_no_opt),
                                ui_scale_percent,
                            ))
                        })
                        .child(conflict_diff_text_cell(text.clone(), styled, show_ws));

                    if let Some(conflict_ix) = conflict_ix {
                        let has_base = this
                            .conflict_resolver
                            .conflict_has_base
                            .get(conflict_ix)
                            .copied()
                            .unwrap_or(false);
                        let selected_choices =
                            this.conflict_resolver_selected_choices_for_conflict_ix(conflict_ix);
                        let (line_label, line_target, chunk_label, chunk_target) =
                            two_way_aligned_input_row_menu_targets(row, conflict_ix, side);
                        if row_selection_enabled {
                            cell = cell
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _window, cx| {
                                        if e.modifiers.shift || e.modifiers.control {
                                            this.conflict_resolver_click_row_selection(
                                                conflict_ix,
                                                row,
                                                e.modifiers,
                                                cx,
                                            );
                                        } else {
                                            this.conflict_resolver_begin_row_selection(
                                                conflict_ix,
                                                row,
                                                cx,
                                            );
                                        }
                                    }),
                                )
                                .on_mouse_move(cx.listener(
                                    move |this, _e: &MouseMoveEvent, _window, cx| {
                                        this.conflict_resolver_extend_row_selection(
                                            conflict_ix,
                                            row,
                                            cx,
                                        );
                                    },
                                ));
                        } else {
                            cell = cell.on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                                    // section 30: clicking a conflict block body selects it.
                                    this.conflict_resolver_select_conflict(conflict_ix, cx);
                                }),
                            );
                        }
                        cell = cell.on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                if e.modifiers.shift {
                                    let invoker: SharedString =
                                        format!("{}_{}_{}", input_menu_prefix, conflict_ix, row)
                                            .into();
                                    this.open_conflict_resolver_input_row_context_menu(
                                        invoker,
                                        line_label.clone(),
                                        line_target.clone(),
                                        chunk_label.clone(),
                                        chunk_target.clone(),
                                        e.position,
                                        window,
                                        cx,
                                    );
                                } else {
                                    let invoker: SharedString =
                                        format!("{}_{}_{}", chunk_menu_prefix, conflict_ix, row)
                                            .into();
                                    this.open_conflict_resolver_chunk_context_menu(
                                        invoker,
                                        conflict_ix,
                                        has_base,
                                        false,
                                        selected_choices.clone(),
                                        None,
                                        e.position,
                                        window,
                                        cx,
                                    );
                                }
                            }),
                        );
                    } else if let Some(target_index) = semantic_nav_target {
                        cell = cell.cursor(CursorStyle::PointingHand).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                                this.conflict_jump_to_nav_target(target_index, cx);
                            }),
                        );
                    }

                    elements.push(cell.into_any_element());
                }
            }
        }
        if needs_chunk_poll {
            this.ensure_prepared_syntax_chunk_poll(cx);
        }
        elements
    }

    /// Diff-view-style collapsed context fold row (section 30, R6): muted band,
    /// reveal arrows clustered where the line-number gutter sits, and a
    /// left-aligned hidden-range label; clicking elsewhere expands the whole
    /// fold. `output_pane` selects which fold-reveal state the controls
    /// mutate (source columns vs resolved output).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn conflict_context_fold_row(
        theme: AppTheme,
        id_prefix: &'static str,
        vi: usize,
        source_line_start: usize,
        len: usize,
        fold_id: usize,
        output_pane: bool,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let first_line = source_line_start + 1;
        let last_line = source_line_start + len;
        let label: SharedString =
            format!("⋯ {len} unchanged lines ({first_line}–{last_line})").into();
        let fold_bg = with_alpha(
            theme.colors.foreground.secondary,
            if theme.is_dark { 0.14 } else { 0.10 },
        );
        let reveal_btn = |id_suffix: &'static str,
                          icon: &'static str,
                          tooltip: &'static str,
                          from_top: bool,
                          cx: &mut gpui::Context<Self>| {
            let btn_size = conflict_scaled_px(CONFLICT_FOLD_REVEAL_BUTTON_PX, ui_scale_percent);
            div()
                .id((id_suffix, vi))
                .w(btn_size)
                .h(btn_size)
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme.radii.row))
                .cursor(CursorStyle::PointingHand)
                .hover(move |style| {
                    style.bg(with_alpha(theme.colors.interaction.hover_background, 0.55))
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        if output_pane {
                            this.conflict_resolver_reveal_output_context_fold(
                                fold_id, from_top, cx,
                            );
                        } else {
                            this.conflict_resolver_reveal_context_fold(fold_id, from_top, cx);
                        }
                    }),
                )
                .child(svg_icon(
                    icon,
                    theme.colors.foreground.secondary,
                    conflict_scaled_px(CONFLICT_FOLD_REVEAL_ICON_PX, ui_scale_percent),
                ))
                .worktree_tooltip(theme, tooltip.into())
        };
        div()
            .id((id_prefix, vi))
            .w_full()
            .h(conflict_row_height(ui_scale_percent))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(fold_bg)
            .text_xs()
            .text_color(theme.colors.foreground.secondary)
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(reveal_btn(
                        "conflict_fold_reveal_top",
                        "icons/arrow_down.svg",
                        crate::i18n::tr_str("misc.conflict_fold.reveal_top"),
                        true,
                        cx,
                    ))
                    .child(reveal_btn(
                        "conflict_fold_reveal_bottom",
                        "icons/arrow_up.svg",
                        crate::i18n::tr_str("misc.conflict_fold.reveal_bottom"),
                        false,
                        cx,
                    )),
            )
            .child(label)
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    if output_pane {
                        this.conflict_resolver_expand_output_context_fold(fold_id, cx);
                    } else {
                        this.conflict_resolver_expand_context_fold(fold_id, cx);
                    }
                }),
            )
            .worktree_tooltip(theme, crate::i18n::tr("misc.conflict_fold.expand_all"))
            .into_any_element()
    }

    pub(in super::super::super) fn render_conflict_compare_diff_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let query = this.diff_search_query_or_empty();
        let query_options = this.diff_search_options_or_default();
        let query = query.as_ref().to_string();
        this.sync_conflict_diff_query_overlay_caches(query.as_str(), query_options);
        let query_matcher = conflict_diff_query_matcher(query.as_str(), query_options);
        let syntax_lang = this.conflict_row_syntax_language();
        // Streamed conflicts may or may not have prepared side documents; Auto
        // remains the safe fallback when a row is not backed by one.
        let syntax_mode = DiffSyntaxMode::Auto;
        range
            .map(|visible_row_ix| {
                let Some(visible_row) = this
                    .conflict_resolver
                    .two_way_split_visible_row(visible_row_ix)
                else {
                    return div()
                        .id(("conflict_compare_split_visible_oob", visible_row_ix))
                        .h(conflict_row_height(ui_scale_percent))
                        .px_2()
                        .text_xs()
                        .text_color(this.theme.colors.foreground.secondary)
                        .child("")
                        .into_any_element();
                };
                let row_ix = visible_row.source_row_ix;
                let row = visible_row.row;
                this.render_conflict_compare_split_row(
                    visible_row_ix,
                    row_ix,
                    row,
                    syntax_lang,
                    syntax_mode,
                    query_matcher.as_ref(),
                    cx,
                )
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn conflict_split_row_styled(
        theme: AppTheme,
        stable_cache: &mut conflict_resolver::ConflictSplitStyledTextCache,
        query_cache: &mut conflict_resolver::ConflictSplitStyledTextCache,
        row_ix: usize,
        side: ConflictPickSide,
        text: Option<&str>,
        word_ranges: &[Range<usize>],
        query: &str,
        _query_options: DiffSearchOptions,
        query_matcher: Option<&DiffSearchMatcher>,
        emphasis: DiffSearchMatchEmphasis,
        syntax_lang: Option<DiffSyntaxLanguage>,
        syntax_mode: DiffSyntaxMode,
        prepared_line: PreparedDiffSyntaxLine,
    ) -> ConflictRowStyledText {
        let Some(text) = text else {
            return ConflictRowStyledText::default();
        };
        let source_identity = Some(DiffTextSourceIdentity::from_str(text));
        let key = (row_ix, side);
        let mut result = ConflictRowStyledText::default();
        if text.is_empty() {
            return result;
        }

        let query_active = !query.is_empty();
        let base_has_style = !word_ranges.is_empty() || syntax_lang.is_some();

        if base_has_style {
            if let Some(cached) = stable_cache.get(&key) {
                let _ = cached;
                result.styled = Some(ConflictRowStyledTextValue::StableCached);
            } else {
                let (styled, pending) = build_conflict_row_base_styled(
                    theme,
                    text,
                    source_identity,
                    word_ranges,
                    syntax_lang,
                    syntax_mode,
                    prepared_line,
                )
                .into_parts();
                if !pending {
                    stable_cache.insert(key, styled);
                    result.styled = Some(ConflictRowStyledTextValue::StableCached);
                } else {
                    result.styled = Some(ConflictRowStyledTextValue::Owned(styled));
                }
                result.pending = pending;
            }
        }

        if query_active {
            let Some(query_matcher) = query_matcher else {
                return result;
            };
            // The current match is the row the search cursor is on, so it wears
            // a different wash and moves every time the cursor steps. Caching it
            // would leave that wash behind on the row it just left, so it is
            // rebuilt each frame and never stored.
            let is_current = emphasis == DiffSearchMatchEmphasis::Current;
            if !is_current
                && !result.pending
                && let Some(cached) = query_cache.get(&key)
            {
                let _ = cached;
                result.styled = Some(ConflictRowStyledTextValue::QueryCached);
                return result;
            }

            let styled = if let Some(base) = match result.styled.as_ref() {
                Some(ConflictRowStyledTextValue::Owned(styled)) => Some(styled),
                _ => stable_cache.get(&key),
            } {
                build_cached_diff_query_overlay_styled_text(theme, base, query_matcher, emphasis)
            } else {
                let base = build_conflict_cached_diff_styled_text_with_source_identity(
                    theme,
                    text,
                    source_identity,
                    word_ranges,
                    "",
                    syntax_lang,
                    syntax_mode,
                    None,
                );
                build_cached_diff_query_overlay_styled_text(theme, &base, query_matcher, emphasis)
            };
            if !is_current && !result.pending {
                query_cache.insert(key, styled);
                result.styled = Some(ConflictRowStyledTextValue::QueryCached);
            } else {
                result.styled = Some(ConflictRowStyledTextValue::Owned(styled));
            }
        }

        result
    }

    fn render_conflict_compare_split_row(
        &mut self,
        visible_row_ix: usize,
        row_ix: usize,
        row: worktree_core::file_diff::FileDiffRow,
        syntax_lang: Option<DiffSyntaxLanguage>,
        syntax_mode: DiffSyntaxMode,
        query_matcher: Option<&DiffSearchMatcher>,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let theme = self.theme;
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let show_ws = self.reveal_whitespace_chars;

        let left_text = SharedString::new(row.old.as_deref().unwrap_or_default());
        let right_text = SharedString::new(row.new.as_deref().unwrap_or_default());
        let ours_document = self.conflict_three_way_prepared_syntax_documents.ours;
        let theirs_document = self.conflict_three_way_prepared_syntax_documents.theirs;
        let visual_kind = self.conflict_resolver.two_way_split_visual_kind_at(
            row_ix,
            &row,
            self.diff_whitespace_mode,
        );

        // Large streamed compare views should avoid retaining per-row styled
        // caches as users scroll through the whole-file projection.
        let styling_enabled = self.conflict_row_styling_enabled()
            && self.conflict_resolver.three_way_len
                <= conflict_resolver::LARGE_CONFLICT_BLOCK_DIFF_MAX_LINES;
        let word_hl = if styling_enabled
            && !matches!(
                visual_kind,
                worktree_core::file_diff::FileDiffRowKind::Context
            ) {
            self.conflict_resolver
                .two_way_split_word_highlight_for_row(row_ix, &row)
        } else {
            None
        };
        let old_word_ranges = word_hl
            .as_ref()
            .map(|pair| pair.0.as_slice())
            .unwrap_or(&[]);
        let new_word_ranges = word_hl
            .as_ref()
            .map(|pair| pair.1.as_slice())
            .unwrap_or(&[]);
        let query_text = self.conflict_diff_query_cache_query.clone();
        let query_options = self.conflict_diff_query_cache_options;
        let query = query_text.as_ref();
        let (left_styled, right_styled) = if styling_enabled {
            (
                Self::conflict_split_row_styled(
                    theme,
                    &mut self.conflict_diff_segments_cache_split,
                    &mut self.conflict_diff_query_segments_cache_split,
                    row_ix,
                    ConflictPickSide::Ours,
                    row.old.as_deref(),
                    old_word_ranges,
                    query,
                    query_options,
                    query_matcher,
                    DiffSearchMatchEmphasis::Other,
                    syntax_lang,
                    syntax_mode,
                    prepared_diff_syntax_line_for_one_based_line(ours_document, row.old_line),
                ),
                Self::conflict_split_row_styled(
                    theme,
                    &mut self.conflict_diff_segments_cache_split,
                    &mut self.conflict_diff_query_segments_cache_split,
                    row_ix,
                    ConflictPickSide::Theirs,
                    row.new.as_deref(),
                    new_word_ranges,
                    query,
                    query_options,
                    query_matcher,
                    DiffSearchMatchEmphasis::Other,
                    syntax_lang,
                    syntax_mode,
                    prepared_diff_syntax_line_for_one_based_line(theirs_document, row.new_line),
                ),
            )
        } else {
            (
                ConflictRowStyledText::default(),
                ConflictRowStyledText::default(),
            )
        };
        if left_styled.pending || right_styled.pending {
            self.ensure_prepared_syntax_chunk_poll(cx);
        }
        let left_styled = left_styled.resolve(
            &self.conflict_diff_segments_cache_split,
            &self.conflict_diff_query_segments_cache_split,
            (row_ix, ConflictPickSide::Ours),
        );
        let right_styled = right_styled.resolve(
            &self.conflict_diff_segments_cache_split,
            &self.conflict_diff_query_segments_cache_split,
            (row_ix, ConflictPickSide::Theirs),
        );

        let left_bg = split_cell_bg(theme, visual_kind, ConflictPickSide::Ours);
        let right_bg = split_cell_bg(theme, visual_kind, ConflictPickSide::Theirs);

        let [left_col_w, right_col_w] = self.conflict_diff_split_col_widths;
        let left_fg = if row.old.is_some() {
            theme.colors.foreground.primary
        } else {
            theme.colors.foreground.secondary
        };
        let right_fg = if row.new.is_some() {
            theme.colors.foreground.primary
        } else {
            theme.colors.foreground.secondary
        };

        if self.conflict_canvas_rows_enabled {
            let min_width = left_col_w
                + right_col_w
                + conflict_scaled_px(PANE_RESIZE_HANDLE_PX, ui_scale_percent);
            return conflict_canvas::split_conflict_row_canvas(
                theme,
                cx.entity(),
                visible_row_ix,
                row_ix,
                min_width,
                left_col_w,
                right_col_w,
                self.mergetool_show_line_numbers,
                line_number_string(row.old_line),
                line_number_string(row.new_line),
                left_bg,
                right_bg,
                left_fg,
                right_fg,
                left_text,
                right_text,
                left_styled,
                right_styled,
                show_ws,
                None,
                ui_scale_percent,
            );
        }

        let left = div()
            .id(("conflict_compare_split_ours", row_ix))
            .w(left_col_w)
            .min_w(px(0.0))
            .h(conflict_row_height(ui_scale_percent))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .bg(left_bg)
            .text_color(left_fg)
            .whitespace_nowrap()
            .overflow_hidden()
            .when(self.mergetool_show_line_numbers, |d| {
                d.child(conflict_diff_line_number_cell(
                    theme,
                    line_number_string(row.old_line),
                    ui_scale_percent,
                ))
            })
            .child(conflict_diff_text_cell(
                left_text.clone(),
                left_styled,
                show_ws,
            ));

        let right = div()
            .id(("conflict_compare_split_theirs", row_ix))
            .w(right_col_w)
            .flex_grow(1.)
            .min_w(px(0.0))
            .h(conflict_row_height(ui_scale_percent))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .bg(right_bg)
            .text_color(right_fg)
            .whitespace_nowrap()
            .overflow_hidden()
            .when(self.mergetool_show_line_numbers, |d| {
                d.child(conflict_diff_line_number_cell(
                    theme,
                    line_number_string(row.new_line),
                    ui_scale_percent,
                ))
            })
            .child(conflict_diff_text_cell(
                right_text.clone(),
                right_styled,
                show_ws,
            ));

        let handle_w = conflict_scaled_px(PANE_RESIZE_HANDLE_PX, ui_scale_percent);
        div()
            .id(("conflict_compare_split_row", row_ix))
            .w_full()
            .flex()
            .child(left)
            .child(
                div()
                    .w(handle_w)
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().w(px(1.0)).h_full().bg(theme.colors.stroke.default)),
            )
            .child(right)
            .into_any_element()
    }
}
