use std::time::SystemTime;

use super::*;

use crate::view::statistics::{PeriodModel, StatisticsBar, build_statistics_model};

/// Chart body height; bucket labels get their own row underneath.
const STATISTICS_CHART_HEIGHT_PX: f32 = 96.0;
/// Contributor list cap, matching the assume-unchanged list: a handful of
/// authors reads at a glance, and a busy year stays scrollable without
/// pushing the dialog off screen.
const STATISTICS_LIST_MAX_HEIGHT_PX: f32 = 240.0;
/// Width of the numeric count column at the end of a ranking row; counts
/// stay right-aligned when the list widens for a four-digit year total.
const STATISTICS_COUNT_COLUMN_PX: f32 = 44.0;
/// Locale keys for the weekday labels, Sunday-first to match the bars.
const WEEKDAY_KEYS: [&str; 7] = [
    "ui.statistics.weekday.0",
    "ui.statistics.weekday.1",
    "ui.statistics.weekday.2",
    "ui.statistics.weekday.3",
    "ui.statistics.weekday.4",
    "ui.statistics.weekday.5",
    "ui.statistics.weekday.6",
];
/// Locale keys for the calendar-month labels.
const MONTH_KEYS: [&str; 12] = [
    "ui.statistics.month.1",
    "ui.statistics.month.2",
    "ui.statistics.month.3",
    "ui.statistics.month.4",
    "ui.statistics.month.5",
    "ui.statistics.month.6",
    "ui.statistics.month.7",
    "ui.statistics.month.8",
    "ui.statistics.month.9",
    "ui.statistics.month.10",
    "ui.statistics.month.11",
    "ui.statistics.month.12",
];

/// Period tab shown in the statistics popover. Kept on the host (not the
/// `PopoverKind`) so switching tabs doesn't reopen the popover; reset to
/// Week every time the dialog opens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum StatisticsPeriod {
    #[default]
    Week,
    Month,
    Year,
}

impl StatisticsPeriod {
    fn all() -> [StatisticsPeriod; 3] {
        [
            StatisticsPeriod::Week,
            StatisticsPeriod::Month,
            StatisticsPeriod::Year,
        ]
    }

    fn id_key(self) -> &'static str {
        match self {
            StatisticsPeriod::Week => "week",
            StatisticsPeriod::Month => "month",
            StatisticsPeriod::Year => "year",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            StatisticsPeriod::Week => "ui.statistics.period.week",
            StatisticsPeriod::Month => "ui.statistics.period.month",
            StatisticsPeriod::Year => "ui.statistics.period.year",
        }
    }
}

/// Commit counts over the current week / month / year, with a bar chart per
/// bucket and a contributor ranking. The commit list is requested on open
/// (see `request_lazy_popover_repo_data`) and bucketed in the user's display
/// timezone here at render time, so the same data regroups instantly when
/// the preference changes.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale_percent = super::popover_ui_scale_percent(cx);
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);
    let repo = this.state.repos.iter().find(|r| r.id == repo_id);
    let timezone = this.timezone;
    let period = this.statistics_period;

    let header = div()
        .px(scaled_px(8.0))
        .py(scaled_px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(crate::i18n::tr("ui.statistics.title")),
        )
        .child(
            components::Button::new("statistics_close", crate::i18n::tr("ui.statistics.close"))
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, |this, _e, _w, cx| this.close_popover(cx)),
        );

    let body: AnyElement = match repo.map(|r| &r.statistics) {
        None => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.no_repository"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        // The open path dispatches the load, so NotLoaded is the brief moment
        // before the Loading transition lands — same face for both.
        Some(Loadable::NotLoaded) | Some(Loadable::Loading) => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.loading_ellipsis"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Error(e)) => components::context_menu_label(
            theme,
            ui_scale_percent,
            e.clone(),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Ready(commits)) => {
            let model = build_statistics_model(commits, timezone, SystemTime::now());
            let period_model = match period {
                StatisticsPeriod::Week => &model.week,
                StatisticsPeriod::Month => &model.month,
                StatisticsPeriod::Year => &model.year,
            };
            div()
                .id("statistics_body")
                .debug_selector(|| "statistics_body".to_string())
                .flex()
                .flex_col()
                .child(period_tabs(theme, period, cx))
                .child(summary_line(theme, period_model))
                .child(chart(theme, period, &period_model.bars, &scaled_px))
                .child(contributor_list(theme, period_model, &scaled_px))
                .into_any_element()
        }
    };

    components::context_menu(
        theme,
        div()
            .id("statistics_popover")
            .debug_selector(|| "statistics_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(540.0))
            .child(header)
            .child(super::dialog_divider(theme))
            .child(body),
    )
}

/// One bordered chip per period; the selected chip carries the accent border
/// and wash, mirroring the cherry-pick mainline choices.
fn period_tabs(
    theme: AppTheme,
    selected: StatisticsPeriod,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let hover_overlay = theme.colors.interaction.hover_overlay;
    let outlined_border = theme.colors.stroke.control;
    let mut row = div().flex().gap_1().px_2().pt_2();

    for period in StatisticsPeriod::all() {
        let is_selected = period == selected;
        let id_key = period.id_key();
        row = row.child(
            div()
                .id(SharedString::from(format!("statistics_period_{id_key}")))
                .debug_selector(move || format!("statistics_period_tab_{id_key}"))
                .px_3()
                .py_1()
                .rounded_md()
                .text_sm()
                .text_color(theme.colors.foreground.primary)
                .border_1()
                .border_color(if is_selected {
                    theme.colors.accent.foreground
                } else {
                    outlined_border
                })
                .when(is_selected, |chip| {
                    chip.bg(crate::theme::with_alpha(
                        theme.colors.accent.foreground,
                        if theme.is_dark { 0.12 } else { 0.08 },
                    ))
                })
                .when(!is_selected, |chip| {
                    chip.hover(move |style| style.bg(hover_overlay))
                        .cursor_pointer()
                })
                .on_click(cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                    this.statistics_period = period;
                    cx.notify();
                }))
                .child(crate::i18n::tr(period.label_key())),
        );
    }
    row
}

/// "N commits · M contributors" for the selected period. Both totals derive
/// from the same window, so the numbers always agree with the chart.
fn summary_line(theme: AppTheme, period: &PeriodModel) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(
            crate::i18n::t!(
                "ui.statistics.summary",
                commits = period.total_commits,
                contributors = period.contributors.len(),
            )
            .into_owned(),
        )
}

/// Canvas-drawn bar chart: one column per bucket, zero-count buckets showing
/// a faint baseline stub so the axis stays legible on quiet periods.
fn chart(
    theme: AppTheme,
    period: StatisticsPeriod,
    bars: &[StatisticsBar],
    scaled_px: &impl Fn(f32) -> Pixels,
) -> gpui::Stateful<gpui::Div> {
    let counts: Vec<u64> = bars.iter().map(|bar| bar.count).collect();
    let bar_color = theme.colors.accent.solid;
    let stub_color = theme.colors.stroke.subtle;
    let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;

    let canvas = gpui::canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            let n = counts.len().max(1) as f32;
            let slot_w = bounds.size.width / n;
            let bar_w = slot_w * 0.62;
            let full_h = bounds.size.height;
            for (ix, count) in counts.iter().enumerate() {
                let x = bounds.origin.x + slot_w * ix as f32 + (slot_w - bar_w) / 2.0;
                if *count == 0 {
                    // Baseline stub keeps the bucket's slot visible.
                    window.paint_quad(fill(
                        Bounds::new(
                            point(x, bounds.origin.y + full_h - px(2.0)),
                            size(bar_w, px(2.0)),
                        ),
                        stub_color,
                    ));
                } else {
                    let h = (full_h - px(2.0)) * (*count as f32 / max);
                    window.paint_quad(fill(
                        Bounds::new(point(x, bounds.origin.y + full_h - h), size(bar_w, h)),
                        bar_color,
                    ));
                }
            }
        },
    )
    .size_full();

    let chart_selector = format!("statistics_chart_{}", period.id_key());
    div()
        .id("statistics_chart")
        .debug_selector(move || chart_selector)
        .mx_2()
        .mt_1()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .h(scaled_px(STATISTICS_CHART_HEIGHT_PX))
                .child(canvas),
        )
        .child(labels_row(theme, period, bars))
}

/// Bucket labels under the chart: weekday names for weeks, day-of-month
/// numbers for months, month names for years. Day numbers are locale-free.
fn labels_row(theme: AppTheme, period: StatisticsPeriod, bars: &[StatisticsBar]) -> gpui::Div {
    let mut row = div().flex().w_full();
    for bar in bars {
        let label: SharedString = match period {
            StatisticsPeriod::Week => crate::i18n::tr(WEEKDAY_KEYS[bar.label_index as usize]),
            StatisticsPeriod::Month => bar.label_index.to_string().into(),
            StatisticsPeriod::Year => crate::i18n::tr(MONTH_KEYS[(bar.label_index - 1) as usize]),
        };
        row = row.child(
            div()
                .flex_1()
                .text_center()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label),
        );
    }
    row
}

/// Ranking rows: rank, author, a proportion bar against the top contributor,
/// and the count right-aligned in the editor mono face.
fn contributor_list(
    theme: AppTheme,
    period: &PeriodModel,
    scaled_px: &impl Fn(f32) -> Pixels,
) -> gpui::Div {
    let section = div().px_2().pb_2().pt_2().flex().flex_col().gap_1().child(
        div()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .child(crate::i18n::tr("ui.statistics.contributors")),
    );

    if period.contributors.is_empty() {
        return section.child(
            div()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::tr("ui.statistics.empty")),
        );
    }

    let max = period
        .contributors
        .first()
        .map(|c| c.count)
        .unwrap_or(1)
        .max(1) as f32;
    let track = theme.colors.surface.raised;
    let mut rows = div().flex().flex_col();

    for (ix, contributor) in period.contributors.iter().enumerate() {
        let fraction = contributor.count as f32 / max;
        rows = rows.child(
            div()
                .id(("statistics_contributor", ix))
                .debug_selector(move || format!("statistics_contributor_{ix}"))
                .flex()
                .items_center()
                .gap_2()
                .py_1()
                .child(
                    div()
                        .w(px(18.0))
                        .text_right()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child((ix + 1).to_string()),
                )
                .child(
                    div()
                        .max_w(relative(0.42))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_sm()
                        .child(contributor.author.to_string()),
                )
                .child(
                    div().flex_1().h(px(4.0)).rounded_sm().bg(track).child(
                        div()
                            .w(relative(fraction))
                            .h_full()
                            .rounded_sm()
                            .bg(theme.colors.accent.solid),
                    ),
                )
                .child(
                    div()
                        .w(scaled_px(STATISTICS_COUNT_COLUMN_PX))
                        .text_right()
                        .text_xs()
                        .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                        .text_color(theme.colors.foreground.secondary)
                        .child(contributor.count.to_string()),
                ),
        );
    }

    section.child(
        div()
            .id("statistics_contributors")
            .debug_selector(|| "statistics_contributors".to_string())
            .max_h(scaled_px(STATISTICS_LIST_MAX_HEIGHT_PX))
            .overflow_y_scroll()
            .child(rows),
    )
}
