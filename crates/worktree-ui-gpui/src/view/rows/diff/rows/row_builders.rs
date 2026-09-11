//! Row builders for the unified, split and patch views.

use super::super::*;
use super::text_spec::focused_diff_line_bg;

pub(in crate::view) fn should_hide_unified_diff_header_line(line: &AnnotatedDiffLine) -> bool {
    matches!(line.kind, DiffLineKind::Header)
        && (line.text.starts_with("index ")
            || line.text.starts_with("--- ")
            || line.text.starts_with("+++ "))
}

/// The coverage overlay's one surface: the new-side line number's verdict
/// recolors the gutter. `None` leaves the diff's own gutter color.
pub(in crate::view::rows::diff) fn coverage_gutter_color(
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
pub(super) fn diff_row(
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
pub(super) fn patch_split_column_row(
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
pub(super) fn patch_split_header_row(
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
