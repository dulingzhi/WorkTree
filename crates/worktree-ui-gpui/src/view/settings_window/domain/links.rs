//! Links and licenses: the about-links constants and the open-source
//! license rows.

use super::*;
use gpui::Stateful;

pub(in crate::view::settings_window) const GITHUB_URL: &str =
    "https://github.com/dulingzhi/WorkTree";

pub(in crate::view::settings_window) const THEMES_GUIDE_URL: &str =
    "https://github.com/dulingzhi/WorkTree/blob/main/docs/themes.md";

pub(in crate::view::settings_window) const LICENSE_URL: &str =
    "https://github.com/dulingzhi/WorkTree/blob/main/LICENSE-AGPL-3.0";

pub(in crate::view::settings_window) const LICENSE_NAME: &str = "AGPL-3.0";

impl SettingsWindowView {
    fn open_source_license_row(
        &self,
        ix: usize,
        row: crate::view::open_source_licenses_data::OpenSourceLicenseRow,
        theme: AppTheme,
    ) -> Stateful<gpui::Div> {
        div()
            .id(("settings_window_open_source_license_row", ix))
            .w_full()
            .px_2()
            .py_1()
            .h(px(24.0))
            .flex()
            .items_center()
            .rounded(px(theme.radii.row))
            .hover(move |s| s.bg(theme.colors.interaction.hover_background))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(200.0))
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(row.crate_name),
                    )
                    .child(
                        div()
                            .w(px(90.0))
                            .text_xs()
                            .font_family(UI_MONOSPACE_FONT_FAMILY)
                            .text_color(theme.colors.foreground.secondary)
                            .whitespace_nowrap()
                            .child(row.version),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_xs()
                            .font_family(UI_MONOSPACE_FONT_FAMILY)
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(row.license),
                    ),
            )
    }

    pub(in crate::view::settings_window) fn render_open_source_license_rows(
        this: &mut Self,
        range: Range<usize>,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = crate::view::open_source_licenses_data::open_source_license_rows();
        let theme = this.theme;

        range
            .filter_map(|ix| rows.get(ix).copied().map(|row| (ix, row)))
            .map(|(ix, row)| {
                this.open_source_license_row(ix, row, theme)
                    .into_any_element()
            })
            .collect()
    }

    pub(in crate::view::settings_window) fn links_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.card("settings_window_links", tr_str("settings.nav.links"), theme)
            .child(
                self.link_row(
                    "settings_window_links_theme_guide",
                    tr_str("settings.row.theme_guide"),
                    "docs/themes.md".into(),
                    theme,
                )
                .on_click(|_, _, cx| {
                    cx.open_url(THEMES_GUIDE_URL);
                }),
            )
            .child(
                self.link_row(
                    "settings_window_github",
                    "GitHub",
                    "dulingzhi/WorkTree".into(),
                    theme,
                )
                .on_click(|_, _, cx| {
                    cx.open_url(GITHUB_URL);
                }),
            )
            .child(
                self.link_row(
                    "settings_window_license",
                    tr_str("settings.links.license"),
                    LICENSE_NAME.into(),
                    theme,
                )
                .on_click(|_, _, cx| {
                    cx.open_url(LICENSE_URL);
                }),
            )
            .child(
                self.link_row(
                    "settings_window_professional_edition_waitlist",
                    tr_str("settings.links.professional_waitlist"),
                    "worktree.dev".into(),
                    theme,
                )
                .on_click(|_, _, cx| {
                    cx.open_url(EDITIONS_URL);
                }),
            )
            .child(
                self.link_row(
                    "settings_window_open_source_licenses",
                    tr_str("settings.links.open_source_licenses"),
                    tr("settings.action.show"),
                    theme,
                )
                .border_color(no_separator)
                .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                    this.show_open_source_licenses(cx);
                })),
            )
    }

    pub(in crate::view::settings_window) fn licenses_card(
        &self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let rows = crate::view::open_source_licenses_data::open_source_license_rows();

        let list = if rows.is_empty() {
            div()
                .px_2()
                .py_1()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(tr_str("settings.licenses.empty"))
                .into_any_element()
        } else {
            restrict_scroll_to_vertical_axis(
                uniform_list(
                    "settings_window_open_source_licenses_list",
                    rows.len(),
                    cx.processor(Self::render_open_source_license_rows),
                )
                .w_full()
                .min_w(px(0.0))
                .h_full()
                .min_h(px(0.0))
                .track_scroll(&self.open_source_licenses_scroll),
            )
            .into_any_element()
        };

        let list_container = div()
            .id("settings_window_open_source_licenses_list_container")
            .w_full()
            .min_w(px(0.0))
            .relative()
            .flex_1()
            .min_h(px(0.0))
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .h_full()
                    .min_w(px(0.0))
                    .min_h(px(0.0))
                    .pr(components::Scrollbar::visible_gutter(
                        self.open_source_licenses_scroll.clone(),
                        components::ScrollbarAxis::Vertical,
                    ))
                    .child(list),
            )
            .child(
                {
                    let scrollbar = components::Scrollbar::new(
                        "settings_window_open_source_licenses_scrollbar",
                        self.open_source_licenses_scroll.clone(),
                    )
                    .always_visible();
                    #[cfg(test)]
                    let scrollbar =
                        scrollbar.debug_selector("settings_window_open_source_licenses_scrollbar");
                    scrollbar
                }
                .render(theme),
            );

        self.card(
            "settings_window_open_source_licenses_card",
            tr_str("settings.links.open_source_licenses"),
            theme,
        )
        .flex_1()
        .min_h(px(0.0))
        .child(
            div()
                .px_2()
                .pb_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .child(t!("settings.licenses.count", count = rows.len()).into_owned()),
        )
        .child(
            div()
                .id("settings_window_open_source_licenses_columns")
                .debug_selector(|| "settings_window_open_source_licenses_columns".to_string())
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.colors.foreground.secondary)
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(200.0))
                        .child(tr_str("settings.licenses.column_crate")),
                )
                .child(
                    div()
                        .w(px(90.0))
                        .child(tr_str("settings.licenses.column_version")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(tr_str("settings.licenses.column_license")),
                ),
        )
        .child(list_container)
    }
}
