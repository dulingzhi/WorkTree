//! `MainPaneView` resolved-output builders: the preview rows and the compare diff.

use super::super::conflict_resolver;
use super::super::diff_text::*;
use super::super::perf::{self, ViewPerfRenderLane, ViewPerfSpan};
use super::super::*;
use super::styled_text::{
    RESOLVED_OUTPUT_BADGE_GLYPH_H_PX, RESOLVED_OUTPUT_BADGE_GLYPH_W_PX, RESOLVED_OUTPUT_BADGE_PX,
    RESOLVED_OUTPUT_CONFIDENCE_DOT_PX, RESOLVED_OUTPUT_MARKER_BAR_PX,
    RESOLVED_OUTPUT_MARKER_CAP_INSET_PX, RESOLVED_OUTPUT_MARKER_CAP_W_PX,
    RESOLVED_OUTPUT_MARKER_PX, conflict_resolved_output_row_min_width,
    resolved_output_line_no_width, resolved_output_source_badge_colors,
};

// @split-module: impl_resolved
impl MainPaneView {
    pub(in super::super::super) fn render_conflict_resolved_preview_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let _perf_scope = perf::span(ViewPerfSpan::RenderResolvedPreviewRows);
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let requested_rows = range.len();
        let theme = this.theme;
        let editor_font_family = crate::font_preferences::current_editor_font_family(cx);
        let line_count = this.conflict_resolved_preview_line_count;
        // Navigation centres the editable output on a row without a `cx` to hand,
        // so leave the height these rows actually lay out at where it can read it.
        this.conflict_resolved_gutter_row_height = crate::ui_scale::design_px_from_percent(
            crate::view::panes::main::RESOLVED_OUTPUT_ROW_HEIGHT_PX,
            ui_scale_percent,
        );

        if this.conflict_resolver.resolved_outline_gutter_rows.len() != line_count {
            let meta = &this.conflict_resolver.resolved_outline.meta;
            let markers = &this.conflict_resolver.resolved_outline.markers;
            let line_starts = &this.conflict_resolved_preview_line_starts;
            // A placeholder row is unresolved by definition, so read that off
            // the row's own text rather than trusting the marker array, which
            // is rebuilt incrementally and can lag a resolve/unresolve.
            let placeholder_rows: Vec<bool> =
                this.conflict_resolver_input.read_with(cx, |input, _| {
                    let text = input.text();
                    (0..line_count)
                        .map(|ix| {
                            conflict_resolver::line_is_unresolved_conflict_placeholder(
                                resolved_output_line_text(text, line_starts, ix),
                            )
                        })
                        .collect()
                });
            let mut gutter_rows = Vec::with_capacity(line_count);
            for ix in 0..line_count {
                let source = meta
                    .get(ix)
                    .map(|entry| entry.source)
                    .unwrap_or(conflict_resolver::ResolvedLineSource::Manual);
                let marker = markers.get(ix).copied().flatten();
                let row = conflict_resolver::ResolvedOutputGutterRow::new(
                    source,
                    marker.map(|entry| entry.conflict_ix),
                    marker.is_some_and(|entry| entry.is_start),
                    marker.is_some_and(|entry| entry.is_end),
                    marker.is_some_and(|entry| entry.unresolved),
                );
                let is_placeholder = placeholder_rows.get(ix).copied().unwrap_or(false);
                gutter_rows.push(if is_placeholder {
                    row.with_unresolved_placeholder()
                } else {
                    row
                });
            }
            this.conflict_resolver.resolved_outline_gutter_rows = gutter_rows;
        }

        let fold_bg = with_alpha(
            theme.colors.foreground.secondary,
            if theme.is_dark { 0.14 } else { 0.10 },
        );
        // Line-number cell sized to this file's digit count so short numbers sit
        // snug against the marker lane; the gutter container width tracks it.
        let line_no_w = resolved_output_line_no_width(line_count, ui_scale_percent);
        let elements: Vec<AnyElement> = range
            .map(|vi| {
                // Collapsed context mode projects the output row space; map
                // each visible row to its line (folds render a matching band).
                let ix = match this.resolved_output_item_for_visible(vi) {
                    Some(conflict_resolver::ThreeWayVisibleItem::Line(line)) => line,
                    Some(conflict_resolver::ThreeWayVisibleItem::CollapsedContext { .. }) => {
                        return div()
                            .id(("conflict_resolved_preview_fold", vi))
                            .h(conflict_row_height(ui_scale_percent))
                            .w_full()
                            .bg(fold_bg)
                            .into_any_element();
                    }
                    Some(conflict_resolver::ThreeWayVisibleItem::CollapsedBlock(_)) | None => {
                        return div()
                            .id(("conflict_resolved_preview_oob", vi))
                            .h(conflict_row_height(ui_scale_percent))
                            .px_2()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child("")
                            .into_any_element();
                    }
                };

                let gutter_row = this
                    .conflict_resolver
                    .resolved_outline_gutter_rows
                    .get(ix)
                    .copied()
                    .unwrap_or_default();
                let source = gutter_row.source();
                let (_, badge_fg) = resolved_output_source_badge_colors(theme, source);
                // section 30 R11: outside marker regions the badge is provenance of
                // a line git itself pre-merged (or plain context), not a
                // resolver pick — mute it so only real picks read as
                // decisions.
                let badge_fg = if gutter_row.has_marker() && gutter_row.unresolved() {
                    theme.colors.status.danger.foreground
                } else if gutter_row.has_marker() {
                    badge_fg
                } else {
                    with_alpha(badge_fg, if theme.is_dark { 0.45 } else { 0.55 })
                };
                let conflict_ix = gutter_row.marker_conflict_ix();
                let conflict_active = this.conflict_resolver.conflict_is_active(conflict_ix);
                let conflict_unresolved = gutter_row.unresolved();
                let marker_color = if conflict_unresolved {
                    with_alpha(
                        theme.colors.status.danger.foreground,
                        if theme.is_dark { 0.96 } else { 0.90 },
                    )
                } else if conflict_active {
                    with_alpha(
                        theme.colors.accent.foreground,
                        if theme.is_dark { 0.92 } else { 0.84 },
                    )
                } else {
                    with_alpha(
                        theme.colors.status.success.foreground,
                        if theme.is_dark { 0.82 } else { 0.72 },
                    )
                };
                let marker_bar_w =
                    conflict_scaled_px(RESOLVED_OUTPUT_MARKER_BAR_PX, ui_scale_percent);
                let marker_cap_w =
                    conflict_scaled_px(RESOLVED_OUTPUT_MARKER_CAP_W_PX, ui_scale_percent);
                let marker_cap_inset =
                    conflict_scaled_px(-RESOLVED_OUTPUT_MARKER_CAP_INSET_PX, ui_scale_percent);
                let marker_lane = div()
                    .w(conflict_scaled_px(
                        RESOLVED_OUTPUT_MARKER_PX,
                        ui_scale_percent,
                    ))
                    .mr_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(gutter_row.has_marker(), |d| {
                        d.child(
                            div()
                                .relative()
                                .w(marker_bar_w)
                                .h_full()
                                .bg(marker_color)
                                .when(gutter_row.is_start(), |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .top(px(0.0))
                                            .left(marker_cap_inset)
                                            .w(marker_cap_w)
                                            .h(marker_bar_w)
                                            .bg(marker_color),
                                    )
                                })
                                .when(gutter_row.is_end(), |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .bottom(px(0.0))
                                            .left(marker_cap_inset)
                                            .w(marker_cap_w)
                                            .h(marker_bar_w)
                                            .bg(marker_color),
                                    )
                                }),
                        )
                    });

                let mut row = div()
                    .id(("conflict_resolved_preview_row", ix))
                    .relative()
                    .h(crate::ui_scale::design_px_from_percent(
                        crate::view::panes::main::RESOLVED_OUTPUT_ROW_HEIGHT_PX,
                        ui_scale_percent,
                    ))
                    .px_2()
                    .flex()
                    .items_center()
                    .text_xs()
                    .font_family(editor_font_family.clone())
                    .text_color(theme.colors.foreground.primary)
                    // The active conflict's open row wears the same yellow wash
                    // the editor paints behind its `<Merge Conflict>` text, so
                    // the gutter and the code read as one highlighted row.
                    .when(conflict_active && conflict_unresolved, |d| {
                        d.bg(
                            crate::view::panes::main::resolved_output_active_conflict_background(
                                theme,
                            ),
                        )
                    })
                    .when(gutter_row.manual_without_marker(), |d| {
                        d.bg(with_alpha(
                            theme.colors.surface.raised,
                            if theme.is_dark { 0.18 } else { 0.12 },
                        ))
                    })
                    .child(marker_lane)
                    .when(this.mergetool_show_line_numbers, |d| {
                        // half the marker gap between the number and the badge;
                        // right-align so short numbers hug the badge instead of
                        // leaving a wide empty stretch inside the cell.
                        d.child(
                            div()
                                .w(line_no_w)
                                .mr_1()
                                .flex()
                                .justify_end()
                                .text_color(theme.colors.foreground.secondary)
                                .child(line_number_string(u32::try_from(ix + 1).ok())),
                        )
                    })
                    .child({
                        // section 30: confidence dot on the first row of an
                        // auto-resolved conflict (accent/warning/danger for
                        // high/medium/low). Rule detail for the active
                        // conflict shows in the resolver header trace label.
                        let confidence = conflict_ix
                            .filter(|_| gutter_row.is_start() && !conflict_unresolved)
                            .and_then(|cix| this.conflict_autosolve_confidence_for_ix(cix));
                        div()
                            .w(conflict_scaled_px(
                                RESOLVED_OUTPUT_BADGE_PX,
                                ui_scale_percent,
                            ))
                            .relative()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .w(conflict_scaled_px(
                                        RESOLVED_OUTPUT_BADGE_GLYPH_W_PX,
                                        ui_scale_percent,
                                    ))
                                    .h(conflict_scaled_px(
                                        RESOLVED_OUTPUT_BADGE_GLYPH_H_PX,
                                        ui_scale_percent,
                                    ))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(badge_fg)
                                    .child(gutter_row.badge_char().to_string()),
                            )
                            .when_some(confidence, |d, confidence| {
                                use worktree_core::conflict_session::AutosolveConfidence;
                                let dot_color = match confidence {
                                    AutosolveConfidence::High => theme.colors.accent.foreground,
                                    AutosolveConfidence::Medium => {
                                        theme.colors.status.warning.foreground
                                    }
                                    AutosolveConfidence::Low => {
                                        theme.colors.status.danger.foreground
                                    }
                                };
                                let dot = conflict_scaled_px(
                                    RESOLVED_OUTPUT_CONFIDENCE_DOT_PX,
                                    ui_scale_percent,
                                );
                                d.child(
                                    div()
                                        .absolute()
                                        .top(conflict_scaled_px(1.0, ui_scale_percent))
                                        .right(px(0.0))
                                        .w(dot)
                                        .h(dot)
                                        .rounded(dot * 0.5)
                                        .bg(dot_color),
                                )
                            })
                    });
                if let Some(conflict_ix) = conflict_ix {
                    let has_base = this
                        .conflict_resolver
                        .conflict_has_base
                        .get(conflict_ix)
                        .copied()
                        .unwrap_or(false);
                    let is_three_way =
                        this.conflict_resolver.view_mode == ConflictResolverViewMode::ThreeWay;
                    let selected_choices =
                        this.conflict_resolver_selected_choices_for_conflict_ix(conflict_ix);
                    let context_menu_invoker: SharedString =
                        format!("resolver_output_chunk_menu_{}_{}", conflict_ix, ix).into();
                    row = row.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_conflict_resolver_chunk_context_menu(
                                context_menu_invoker.clone(),
                                conflict_ix,
                                has_base,
                                is_three_way,
                                selected_choices.clone(),
                                Some(ix),
                                e.position,
                                window,
                                cx,
                            );
                        }),
                    );
                }
                row.into_any_element()
            })
            .collect();
        perf::record_row_batch(
            ViewPerfRenderLane::ResolvedPreview,
            requested_rows,
            elements.len(),
        );
        elements
    }

    pub(in super::super::super) fn render_conflict_resolved_output_rows(
        this: &mut Self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let _perf_scope = perf::span(ViewPerfSpan::RenderResolvedPreviewRows);
        let ui_scale_percent = crate::ui_scale::current(cx).percent;
        let requested_rows = range.len();
        let theme = this.theme;
        let editor_font_family = crate::font_preferences::current_editor_font_family(cx);
        let show_ws = this.reveal_whitespace_chars;
        if let Some(projection) = this.conflict_resolved_output_projection.as_ref() {
            let unresolved_row_bg = with_alpha(
                theme.colors.status.danger.foreground,
                if theme.is_dark { 0.18 } else { 0.10 },
            );
            let active_unresolved_row_bg =
                crate::view::panes::main::resolved_output_active_conflict_background(theme);
            let resolved_row_bg = with_alpha(
                theme.colors.status.success.foreground,
                if theme.is_dark { 0.12 } else { 0.08 },
            );
            let line_count = this.conflict_resolved_preview_line_count;
            let mut elements = Vec::with_capacity(requested_rows);

            let push_row = |ix: usize, line: &str| {
                let line_text = if show_ws {
                    whitespace_visible_line_text(line)
                } else {
                    SharedString::new(line)
                };
                let min_width = conflict_resolved_output_row_min_width(
                    window,
                    &line_text,
                    editor_font_family.as_str(),
                    ui_scale_percent,
                );

                let conflict_marker = this
                    .conflict_resolver
                    .resolved_outline
                    .markers
                    .get(ix)
                    .copied()
                    .flatten();
                let row_bg = conflict_marker.map(|marker| {
                    if !marker.unresolved {
                        resolved_row_bg
                    } else if this
                        .conflict_resolver
                        .conflict_is_active(Some(marker.conflict_ix))
                    {
                        // Same yellow the editable output washes its active row
                        // with: which open conflict the picks apply to.
                        active_unresolved_row_bg
                    } else {
                        unresolved_row_bg
                    }
                });
                let text_color = if conflict_marker.is_some_and(|marker| marker.unresolved) {
                    theme.colors.status.danger.foreground
                } else {
                    theme.colors.foreground.primary
                };

                elements.push(
                    div()
                        .id(("conflict_resolved_output_row", ix))
                        .w_full()
                        .min_w(min_width)
                        .h(conflict_row_height(ui_scale_percent))
                        .px_2()
                        .flex()
                        .items_center()
                        .text_xs()
                        .font_family(editor_font_family.clone())
                        .text_color(text_color)
                        .whitespace_nowrap()
                        .when_some(row_bg, |d, bg| d.bg(bg))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.open_conflict_resolver_output_context_menu_for_line(
                                    ix, e.position, window, cx,
                                );
                            }),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .child(line_text),
                        )
                        .into_any_element(),
                );
            };

            let visible_end = range.end.min(line_count);
            if range.start < visible_end {
                projection.for_each_line_text_in_range(
                    &this.conflict_resolver.marker_segments,
                    range.start..visible_end,
                    push_row,
                );
            }

            for ix in range.start.max(visible_end)..range.end {
                elements.push(
                    div()
                        .id(("conflict_resolved_output_oob", ix))
                        .h(conflict_row_height(ui_scale_percent))
                        .px_2()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child("")
                        .into_any_element(),
                );
            }
            perf::record_row_batch(
                ViewPerfRenderLane::ResolvedPreview,
                requested_rows,
                elements.len(),
            );
            return elements;
        }

        // Unreachable: this list is only mounted when the output is streamed
        // (`conflict_resolver_pane.rs`, inside `if streamed`), and `streamed` is
        // exactly `conflict_resolved_output_projection.is_some()` — the branch
        // above. The editable output is drawn by the `TextInput` instead, with
        // `render_conflict_resolved_preview_rows` supplying only its gutter.
        perf::record_row_batch(ViewPerfRenderLane::ResolvedPreview, requested_rows, 0);
        Vec::new()
    }
}
