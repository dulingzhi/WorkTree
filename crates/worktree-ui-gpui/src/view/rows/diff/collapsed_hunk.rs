//! Collapsed-hunk rendering: reveal-affordance geometry, colors, the pinned
//! shell element, and the inline/split collapsed header rows.

use super::*;

const COLLAPSED_DIFF_INLINE_HUNK_SHELL_DEBUG_SELECTOR: &str = "collapsed_diff_inline_hunk_shell";
const COLLAPSED_DIFF_INLINE_HUNK_GUTTER_DEBUG_SELECTOR: &str = "collapsed_diff_inline_hunk_gutter";
const COLLAPSED_DIFF_INLINE_HUNK_UP_DEBUG_SELECTOR: &str = "collapsed_diff_inline_hunk_up";
const COLLAPSED_DIFF_INLINE_HUNK_DOWN_DEBUG_SELECTOR: &str = "collapsed_diff_inline_hunk_down";
const COLLAPSED_DIFF_INLINE_HUNK_SHORT_DEBUG_SELECTOR: &str = "collapsed_diff_inline_hunk_short";
const COLLAPSED_DIFF_SPLIT_LEFT_HUNK_SHELL_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_left_hunk_shell";
const COLLAPSED_DIFF_SPLIT_LEFT_HUNK_GUTTER_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_left_hunk_gutter";
const COLLAPSED_DIFF_SPLIT_LEFT_HUNK_UP_DEBUG_SELECTOR: &str = "collapsed_diff_split_left_hunk_up";
const COLLAPSED_DIFF_SPLIT_LEFT_HUNK_DOWN_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_left_hunk_down";
const COLLAPSED_DIFF_SPLIT_LEFT_HUNK_SHORT_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_left_hunk_short";
const COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_SHELL_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_right_hunk_shell";
const COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_GUTTER_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_right_hunk_gutter";
const COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_UP_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_right_hunk_up";
const COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_DOWN_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_right_hunk_down";
const COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_SHORT_DEBUG_SELECTOR: &str =
    "collapsed_diff_split_right_hunk_short";
const COLLAPSED_HUNK_BACKGROUND_OVERDRAW_PX: f32 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CollapsedHunkRevealAction {
    Up,
    Down,
    DownBefore,
    Short,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CollapsedHunkRevealClick {
    action: CollapsedHunkRevealAction,
    src_ix: usize,
}

pub(super) fn collapsed_hunk_header_row_height(ui_scale_percent: u32) -> Pixels {
    diff_row_height(ui_scale_percent)
}

pub(super) fn collapsed_hunk_shell_width(
    handle: &gpui::UniformListScrollHandle,
    fallback_width: Pixels,
) -> Pixels {
    let width = handle
        .0
        .borrow()
        .base_handle
        .bounds()
        .size
        .width
        .max(px(0.0));
    if width > px(0.0) {
        width
    } else {
        fallback_width.max(px(0.0))
    }
}

fn scroll_pinned_hunk_shell(
    scroll_handle: gpui::UniformListScrollHandle,
    background: Option<gpui::Rgba>,
    child: AnyElement,
) -> ScrollPinnedHunkShell {
    ScrollPinnedHunkShell {
        child,
        scroll_handle,
        background,
    }
}

fn collapsed_hunk_bg_fill_bounds(bounds: gpui::Bounds<Pixels>) -> gpui::Bounds<Pixels> {
    gpui::Bounds::new(
        bounds.origin,
        gpui::size(
            bounds.size.width,
            bounds.size.height + px(COLLAPSED_HUNK_BACKGROUND_OVERDRAW_PX),
        ),
    )
}

pub(super) fn collapsed_hunk_header_bg(theme: AppTheme) -> gpui::Rgba {
    with_alpha(
        theme.colors.foreground.secondary,
        if theme.is_dark { 0.14 } else { 0.10 },
    )
}

pub(super) fn focused_collapsed_hunk_bg(
    theme: AppTheme,
    _hunk: Option<CollapsedDiffHunk>,
) -> gpui::Rgba {
    with_alpha(
        theme.colors.accent.foreground,
        if theme.is_dark { 0.22 } else { 0.16 },
    )
}

pub(super) fn collapsed_inline_hunk_bg(
    theme: AppTheme,
    _hunk: Option<CollapsedDiffHunk>,
    _expansion_kind: CollapsedDiffExpansionKind,
) -> gpui::Rgba {
    collapsed_hunk_header_bg(theme)
}

pub(super) fn collapsed_inline_hunk_fg(
    theme: AppTheme,
    _hunk: Option<CollapsedDiffHunk>,
) -> gpui::Rgba {
    theme.colors.foreground.secondary
}

pub(super) fn collapsed_split_hunk_bg(
    theme: AppTheme,
    _hunk: Option<CollapsedDiffHunk>,
    _column: PatchSplitColumn,
) -> gpui::Rgba {
    collapsed_hunk_header_bg(theme)
}

pub(super) fn collapsed_split_hunk_fg(theme: AppTheme, _column: PatchSplitColumn) -> gpui::Rgba {
    theme.colors.foreground.secondary
}

fn collapsed_hunk_reveal_button(
    id: impl Into<gpui::ElementId>,
    debug_selector: &'static str,
    theme: AppTheme,
    enabled: bool,
    icon: &'static str,
    tooltip: &'static str,
    icon_color: gpui::Rgba,
    click: CollapsedHunkRevealClick,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let mut button = div()
        .id(id)
        .debug_selector(move || debug_selector.to_string())
        .w(px(18.0))
        .h(px(18.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme.radii.row));

    if enabled {
        button = button
            .cursor(CursorStyle::PointingHand)
            .hover(move |s| s.bg(with_alpha(theme.colors.interaction.hover_background, 0.55)))
            .active(move |s| s.bg(theme.colors.interaction.pressed_background))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _e: &MouseDownEvent, _w, cx| {
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                cx.stop_propagation();
                match click.action {
                    CollapsedHunkRevealAction::Up => {
                        this.collapsed_diff_reveal_hunk_up(click.src_ix, cx);
                    }
                    CollapsedHunkRevealAction::Down => {
                        this.collapsed_diff_reveal_hunk_down(click.src_ix, cx);
                    }
                    CollapsedHunkRevealAction::DownBefore => {
                        this.collapsed_diff_reveal_hunk_down_before(click.src_ix, cx);
                    }
                    CollapsedHunkRevealAction::Short => {
                        this.collapsed_diff_reveal_hunk_short(click.src_ix, cx);
                    }
                }
            }));
    }

    button
        .child(svg_icon(icon, icon_color, px(10.0)))
        .worktree_tooltip(theme, tooltip.into())
        .into_any_element()
}

struct ScrollPinnedHunkShell {
    child: AnyElement,
    scroll_handle: gpui::UniformListScrollHandle,
    background: Option<gpui::Rgba>,
}

impl gpui::IntoElement for ScrollPinnedHunkShell {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for ScrollPinnedHunkShell {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        let scroll_x = -self.scroll_handle.0.borrow().base_handle.offset().x;
        self.child.prepaint_at(
            gpui::point(bounds.origin.x + scroll_x, bounds.origin.y),
            window,
            cx,
        );
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint_state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        if let Some(background) = self.background {
            window.paint_quad(gpui::fill(
                collapsed_hunk_bg_fill_bounds(bounds),
                background,
            ));
        }
        self.child.paint(window, cx);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn collapsed_inline_header_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    visible_ix: usize,
    click_kind: DiffClickKind,
    selected: bool,
    min_width: Pixels,
    pinned_hunk_shell_width: Pixels,
    pinned_hunk_shell_scroll: gpui::UniformListScrollHandle,
    collapsed_hunk: Option<CollapsedDiffHunk>,
    file_stat: Option<(usize, usize)>,
    display: SharedString,
    styled: Option<&CachedDiffStyledText>,
    context_menu_active: bool,
    src_ix: usize,
    expansion_kind: CollapsedDiffExpansionKind,
    hidden_rows: usize,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    match click_kind {
        DiffClickKind::FileHeader => {
            let header_bg = if selected {
                focused_diff_neutral_row_bg(theme)
            } else {
                crate::theme::content_header_bg(theme)
            };
            // Pin the header content to the viewport while the background
            // band spans the full scrollable width, so horizontal scrolling
            // moves neither the band nor the file name.
            let inner = div()
                .id(("collapsed_diff_file_hdr", visible_ix))
                .h(diff_file_header_height(ui_scale_percent))
                .w(pinned_hunk_shell_width)
                .min_w(px(0.0))
                .relative()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(selectable_cached_diff_text(
                    visible_ix,
                    DiffTextRegion::Inline,
                    DiffClickKind::FileHeader,
                    theme.colors.foreground.primary,
                    styled,
                    display,
                    cx,
                ))
                .when(file_stat.is_some_and(|(a, r)| a > 0 || r > 0), |this| {
                    let (a, r) = file_stat.unwrap_or_default();
                    this.child(components::diff_stat(theme, ui_scale_percent, a, r))
                });

            div()
                .h(diff_file_header_height(ui_scale_percent))
                .w_full()
                .min_w(min_width)
                .bg(header_bg)
                .border_b_1()
                .border_color(theme.colors.stroke.subtle)
                .child(scroll_pinned_hunk_shell(
                    pinned_hunk_shell_scroll,
                    None,
                    inner.into_any_element(),
                ))
                .into_any_element()
        }
        DiffClickKind::HunkHeader => {
            let gutter_w = diff_canvas::diff_inline_text_start(ui_scale_percent);
            let trailing_pad = diff_canvas::diff_row_horizontal_padding(ui_scale_percent);
            let text_color = collapsed_inline_hunk_fg(theme, collapsed_hunk);
            let on_right_click = cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.is_inline_submodule_diff_active() {
                    return;
                }
                let Some(repo_id) = this.active_repo_id() else {
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
            let button_color = if hidden_rows > 0 {
                text_color
            } else {
                with_alpha(text_color, 0.45)
            };
            let controls = match expansion_kind {
                CollapsedDiffExpansionKind::Up => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        ("collapsed_diff_hunk_up", visible_ix),
                        COLLAPSED_DIFF_INLINE_HUNK_UP_DEBUG_SELECTOR,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_up.svg",
                        "Show hidden lines above",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Up,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Down => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        ("collapsed_diff_hunk_down", visible_ix),
                        COLLAPSED_DIFF_INLINE_HUNK_DOWN_DEBUG_SELECTOR,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_down.svg",
                        "Show hidden lines below",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Down,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Both => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        ("collapsed_diff_hunk_down", visible_ix),
                        COLLAPSED_DIFF_INLINE_HUNK_DOWN_DEBUG_SELECTOR,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_down.svg",
                        "Show hidden lines below",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::DownBefore,
                            src_ix,
                        },
                        cx,
                    ))
                    .child(collapsed_hunk_reveal_button(
                        ("collapsed_diff_hunk_up", visible_ix),
                        COLLAPSED_DIFF_INLINE_HUNK_UP_DEBUG_SELECTOR,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_up.svg",
                        "Show hidden lines above",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Up,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Short => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        ("collapsed_diff_hunk_short", visible_ix),
                        COLLAPSED_DIFF_INLINE_HUNK_SHORT_DEBUG_SELECTOR,
                        theme,
                        hidden_rows > 0,
                        "icons/plus.svg",
                        "Show hidden lines",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Short,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::None => div().into_any_element(),
            };

            let row_bg = collapsed_inline_hunk_bg(theme, collapsed_hunk, expansion_kind);
            let painted_row_bg = if selected {
                focused_collapsed_hunk_bg(theme, collapsed_hunk)
            } else {
                row_bg
            };
            let mut row = div()
                .id(("collapsed_diff_hunk_hdr", visible_ix))
                .debug_selector(|| COLLAPSED_DIFF_INLINE_HUNK_SHELL_DEBUG_SELECTOR.to_string())
                .h(collapsed_hunk_header_row_height(ui_scale_percent))
                .w(pinned_hunk_shell_width)
                .min_w(px(0.0))
                .relative()
                .overflow_hidden()
                .flex()
                .items_center()
                .bg(painted_row_bg)
                .text_xs()
                .text_color(text_color);
            row = row
                .child(
                    div()
                        .debug_selector(|| {
                            COLLAPSED_DIFF_INLINE_HUNK_GUTTER_DEBUG_SELECTOR.to_string()
                        })
                        .w(gutter_w)
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(controls),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .pr(trailing_pad)
                        .overflow_hidden()
                        .child(selectable_cached_diff_text(
                            visible_ix,
                            DiffTextRegion::Inline,
                            DiffClickKind::HunkHeader,
                            text_color,
                            styled,
                            display,
                            cx,
                        )),
                )
                .on_mouse_down(MouseButton::Right, on_right_click);

            if selected {
                row = row.bg(painted_row_bg);
            }
            if context_menu_active {
                row = row.bg(theme.colors.interaction.pressed_background);
            }

            div()
                .h(collapsed_hunk_header_row_height(ui_scale_percent))
                .min_w(min_width)
                .bg(painted_row_bg)
                .child(scroll_pinned_hunk_shell(
                    pinned_hunk_shell_scroll,
                    Some(painted_row_bg),
                    row.into_any_element(),
                ))
                .into_any_element()
        }
        DiffClickKind::Line => diff_placeholder_row(
            ("collapsed_diff_invalid", visible_ix),
            theme,
            ui_scale_percent,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn collapsed_split_header_row(
    theme: AppTheme,
    ui_scale_percent: u32,
    column: PatchSplitColumn,
    visible_ix: usize,
    click_kind: DiffClickKind,
    selected: bool,
    min_width: Pixels,
    pinned_hunk_shell_width: Pixels,
    pinned_hunk_shell_scroll: gpui::UniformListScrollHandle,
    collapsed_hunk: Option<CollapsedDiffHunk>,
    file_stat: Option<(usize, usize)>,
    display: SharedString,
    styled: Option<&CachedDiffStyledText>,
    context_menu_active: bool,
    src_ix: usize,
    expansion_kind: CollapsedDiffExpansionKind,
    hidden_rows: usize,
    cx: &mut gpui::Context<MainPaneView>,
) -> AnyElement {
    let region = match column {
        PatchSplitColumn::Left => DiffTextRegion::SplitLeft,
        PatchSplitColumn::Right => DiffTextRegion::SplitRight,
    };

    match click_kind {
        DiffClickKind::FileHeader => {
            let header_bg = if selected {
                focused_diff_neutral_row_bg(theme)
            } else {
                crate::theme::content_header_bg(theme)
            };
            // Pin the header content to the viewport while the background
            // band spans the full scrollable width, so horizontal scrolling
            // moves neither the band nor the file name.
            let inner = div()
                .id((
                    match column {
                        PatchSplitColumn::Left => "collapsed_diff_split_left_file_hdr",
                        PatchSplitColumn::Right => "collapsed_diff_split_right_file_hdr",
                    },
                    visible_ix,
                ))
                .h(diff_file_header_height(ui_scale_percent))
                .w(pinned_hunk_shell_width)
                .min_w(px(0.0))
                .relative()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_between()
                .px_2()
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
                });

            div()
                .h(diff_file_header_height(ui_scale_percent))
                .w_full()
                .min_w(min_width)
                .bg(header_bg)
                .border_b_1()
                .border_color(theme.colors.stroke.subtle)
                .child(scroll_pinned_hunk_shell(
                    pinned_hunk_shell_scroll,
                    None,
                    inner.into_any_element(),
                ))
                .into_any_element()
        }
        DiffClickKind::HunkHeader => {
            let gutter_w = diff_canvas::diff_single_column_text_start(ui_scale_percent);
            let trailing_pad = diff_canvas::diff_row_horizontal_padding(ui_scale_percent);
            let text_color = collapsed_split_hunk_fg(theme, column);
            let (
                row_id,
                shell_debug_selector,
                gutter_debug_selector,
                up_id,
                up_debug_selector,
                down_id,
                down_debug_selector,
                short_id,
                short_debug_selector,
            ) = match column {
                PatchSplitColumn::Left => (
                    "collapsed_diff_split_left_hunk_hdr",
                    COLLAPSED_DIFF_SPLIT_LEFT_HUNK_SHELL_DEBUG_SELECTOR,
                    COLLAPSED_DIFF_SPLIT_LEFT_HUNK_GUTTER_DEBUG_SELECTOR,
                    "collapsed_diff_split_left_hunk_up",
                    COLLAPSED_DIFF_SPLIT_LEFT_HUNK_UP_DEBUG_SELECTOR,
                    "collapsed_diff_split_left_hunk_down",
                    COLLAPSED_DIFF_SPLIT_LEFT_HUNK_DOWN_DEBUG_SELECTOR,
                    "collapsed_diff_split_left_hunk_short",
                    COLLAPSED_DIFF_SPLIT_LEFT_HUNK_SHORT_DEBUG_SELECTOR,
                ),
                PatchSplitColumn::Right => (
                    "collapsed_diff_split_right_hunk_hdr",
                    COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_SHELL_DEBUG_SELECTOR,
                    COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_GUTTER_DEBUG_SELECTOR,
                    "collapsed_diff_split_right_hunk_up",
                    COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_UP_DEBUG_SELECTOR,
                    "collapsed_diff_split_right_hunk_down",
                    COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_DOWN_DEBUG_SELECTOR,
                    "collapsed_diff_split_right_hunk_short",
                    COLLAPSED_DIFF_SPLIT_RIGHT_HUNK_SHORT_DEBUG_SELECTOR,
                ),
            };
            let on_right_click = cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.is_inline_submodule_diff_active() {
                    return;
                }
                let Some(repo_id) = this.active_repo_id() else {
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
            let button_color = if hidden_rows > 0 {
                text_color
            } else {
                with_alpha(text_color, 0.45)
            };
            let controls = match expansion_kind {
                CollapsedDiffExpansionKind::Up => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        (up_id, visible_ix),
                        up_debug_selector,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_up.svg",
                        "Show hidden lines above",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Up,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Down => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        (down_id, visible_ix),
                        down_debug_selector,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_down.svg",
                        "Show hidden lines below",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Down,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Both => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        (down_id, visible_ix),
                        down_debug_selector,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_down.svg",
                        "Show hidden lines below",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::DownBefore,
                            src_ix,
                        },
                        cx,
                    ))
                    .child(collapsed_hunk_reveal_button(
                        (up_id, visible_ix),
                        up_debug_selector,
                        theme,
                        hidden_rows > 0,
                        "icons/arrow_up.svg",
                        "Show hidden lines above",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Up,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::Short => div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(collapsed_hunk_reveal_button(
                        (short_id, visible_ix),
                        short_debug_selector,
                        theme,
                        hidden_rows > 0,
                        "icons/plus.svg",
                        "Show hidden lines",
                        button_color,
                        CollapsedHunkRevealClick {
                            action: CollapsedHunkRevealAction::Short,
                            src_ix,
                        },
                        cx,
                    ))
                    .into_any_element(),
                CollapsedDiffExpansionKind::None => div().into_any_element(),
            };

            let row_bg = collapsed_split_hunk_bg(theme, collapsed_hunk, column);
            let painted_row_bg = if selected {
                focused_collapsed_hunk_bg(theme, collapsed_hunk)
            } else {
                row_bg
            };
            let mut row = div()
                .id((row_id, visible_ix))
                .debug_selector(move || shell_debug_selector.to_string())
                .h(collapsed_hunk_header_row_height(ui_scale_percent))
                .w(pinned_hunk_shell_width)
                .min_w(px(0.0))
                .relative()
                .overflow_hidden()
                .flex()
                .items_center()
                .bg(painted_row_bg)
                .text_xs()
                .text_color(text_color)
                .child(
                    div()
                        .debug_selector(move || gutter_debug_selector.to_string())
                        .w(gutter_w)
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(controls),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .pr(trailing_pad)
                        .overflow_hidden()
                        .child(selectable_cached_diff_text(
                            visible_ix,
                            region,
                            DiffClickKind::HunkHeader,
                            text_color,
                            styled,
                            display,
                            cx,
                        )),
                )
                .on_mouse_down(MouseButton::Right, on_right_click);

            if selected {
                row = row.bg(painted_row_bg);
            }
            if context_menu_active {
                row = row.bg(theme.colors.interaction.pressed_background);
            }

            div()
                .h(collapsed_hunk_header_row_height(ui_scale_percent))
                .min_w(min_width)
                .bg(painted_row_bg)
                .child(scroll_pinned_hunk_shell(
                    pinned_hunk_shell_scroll,
                    Some(painted_row_bg),
                    row.into_any_element(),
                ))
                .into_any_element()
        }
        DiffClickKind::Line => diff_placeholder_row(
            (
                match column {
                    PatchSplitColumn::Left => "collapsed_diff_split_left_invalid",
                    PatchSplitColumn::Right => "collapsed_diff_split_right_invalid",
                },
                visible_ix,
            ),
            theme,
            ui_scale_percent,
        ),
    }
}
