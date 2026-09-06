//! Search overlay for the diff pane: the layer element that paints the box
//! above the diff rows, and the panel renderer that builds it.
use super::*;
use crate::view::panels::COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX;

struct DiffSearchOverlayLayer {
    child: AnyElement,
}

impl IntoElement for DiffSearchOverlayLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DiffSearchOverlayLayer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let _ = self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Diff text rows paint in their own layers, so layer the search UI as a unit.
        window.paint_layer(bounds, |window| self.child.paint(window, cx));
    }
}

impl MainPaneView {
    pub(super) fn render_diff_search_overlay(
        &mut self,
        theme: AppTheme,
        ui_scale_percent: u32,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        if !self.diff_search_active {
            return None;
        }

        let query = self.diff_search_query.as_ref();
        let regex_invalid = self.diff_search_regex_error.is_some();
        let match_label: SharedString = if query.is_empty() {
            crate::i18n::tr("diff.search.type_to_search")
        } else if regex_invalid {
            crate::i18n::tr("diff.search.invalid_regex")
        } else if self.diff_search_matches.is_empty() {
            crate::i18n::tr("diff.search.no_matches")
        } else {
            let ix = self
                .diff_search_match_ix
                .unwrap_or(0)
                .min(self.diff_search_matches.len().saturating_sub(1));
            format!("{}/{}", ix + 1, self.diff_search_matches.len()).into()
        };
        let match_label_color = if regex_invalid && !query.is_empty() {
            theme.colors.status.danger.foreground
        } else {
            theme.colors.foreground.secondary
        };
        let option_selected_bg = with_alpha(
            theme.colors.accent.foreground,
            if theme.is_dark { 0.34 } else { 0.24 },
        );
        let options = self.diff_search_options;
        let compact_control_height = px(26.0);
        let compact_icon_button_width = px(22.0);
        let compact_option_button_width = px(24.0);
        let max_search_input_height = px(COMMIT_MESSAGE_INPUT_MAX_HEIGHT_PX);

        let panel = div()
            .flex()
            .items_start()
            .gap(px(2.0))
            .px(px(4.0))
            .py(px(2.0))
            .rounded(px(theme.radii.control))
            .border_1()
            .border_color(theme.colors.stroke.default)
            .bg(theme.colors.surface.raised)
            .shadow(crate::theme::shadow_surface(theme))
            .child(
                div()
                    .relative()
                    .w(px(220.0))
                    .min_w(px(140.0))
                    .debug_selector(|| "diff_search_input_slot".to_string())
                    .child(
                        div()
                            .id("diff_search_input_scroll")
                            .relative()
                            .w_full()
                            .min_w(px(0.0))
                            .max_h(max_search_input_height)
                            .pr(components::Scrollbar::visible_gutter(
                                self.diff_search_scroll.clone(),
                                components::ScrollbarAxis::Vertical,
                            ))
                            .overflow_y_scroll()
                            .track_scroll(&self.diff_search_scroll)
                            .child(self.diff_search_input.clone()),
                    )
                    .child(
                        components::Scrollbar::new(
                            "diff_search_scrollbar",
                            self.diff_search_scroll.clone(),
                        )
                        .render(theme),
                    ),
            )
            .child(
                components::Button::new("diff_search_newline", "")
                    .start_slot(svg_icon(
                        "icons/line_break.svg",
                        theme.colors.foreground.primary,
                        px(14.0),
                    ))
                    .borderless()
                    .style(components::ButtonStyle::Subtle)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.insert_diff_search_line_break(window, cx);
                    })
                    .w(compact_icon_button_width)
                    .h(compact_control_height)
                    .worktree_tooltip(theme, crate::i18n::tr("diff.search.insert_newline"))
                    .debug_selector(|| "diff_search_newline".to_string()),
            )
            .child(
                components::Button::new("diff_search_match_case", "Aa")
                    .borderless()
                    .style(components::ButtonStyle::Subtle)
                    .selected(options.match_case)
                    .selected_bg(option_selected_bg)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        let mut next = this.diff_search_options;
                        next.match_case = !next.match_case;
                        this.set_diff_search_options(next, window, cx);
                    })
                    .w(compact_option_button_width)
                    .h(compact_control_height)
                    .worktree_tooltip(theme, crate::i18n::tr("diff.search.match_case"))
                    .debug_selector(|| "diff_search_match_case".to_string()),
            )
            .child(
                components::Button::new("diff_search_whole_word", "W")
                    .borderless()
                    .style(components::ButtonStyle::Subtle)
                    .selected(options.whole_word)
                    .selected_bg(option_selected_bg)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        let mut next = this.diff_search_options;
                        next.whole_word = !next.whole_word;
                        this.set_diff_search_options(next, window, cx);
                    })
                    .w(compact_option_button_width)
                    .h(compact_control_height)
                    .worktree_tooltip(theme, crate::i18n::tr("diff.search.match_whole_word"))
                    .debug_selector(|| "diff_search_whole_word".to_string()),
            )
            .child(
                components::Button::new("diff_search_regex", ".*")
                    .borderless()
                    .style(components::ButtonStyle::Subtle)
                    .selected(options.regex)
                    .selected_bg(option_selected_bg)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        let mut next = this.diff_search_options;
                        next.regex = !next.regex;
                        this.set_diff_search_options(next, window, cx);
                    })
                    .w(compact_option_button_width)
                    .h(compact_control_height)
                    .worktree_tooltip(theme, crate::i18n::tr("diff.search.use_regex"))
                    .debug_selector(|| "diff_search_regex".to_string()),
            )
            .child(
                div()
                    .w(px(104.0))
                    .min_w(px(104.0))
                    .max_w(px(104.0))
                    .h(compact_control_height)
                    .flex()
                    .items_center()
                    .justify_end()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(match_label_color)
                    .debug_selector(|| "diff_search_match_label".to_string())
                    .child(match_label),
            )
            .child(
                components::Button::new("diff_search_close", "")
                    .start_slot(svg_icon(
                        "icons/generic_close.svg",
                        theme.colors.foreground.secondary,
                        px(12.0),
                    ))
                    .style(components::ButtonStyle::Transparent)
                    .on_click(theme, cx, |this, _e, window, cx| {
                        this.deactivate_diff_search(window, cx);
                        cx.notify();
                    })
                    .w(compact_icon_button_width)
                    .h(compact_control_height)
                    .debug_selector(|| "diff_search_close".to_string()),
            )
            .occlude()
            .with_animation(
                "diff_search_overlay_mount",
                Animation::new(Duration::from_millis(120)).with_easing(gpui::quadratic),
                |panel, delta| {
                    let slide_y = (1.0 - delta) * -8.0;
                    panel.opacity(delta).relative().top(px(slide_y))
                },
            );

        let overlay_panel = div()
            .id("diff_search_overlay_panel")
            .debug_selector(|| "diff_search_overlay".to_string())
            .absolute()
            .top(components::control_height_md(ui_scale_percent))
            .right(px(8.0))
            .child(panel)
            .into_any_element();

        let overlay = div()
            .id("diff_search_overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(overlay_panel)
            .into_any_element();

        Some(DiffSearchOverlayLayer { child: overlay }.into_any_element())
    }
}
