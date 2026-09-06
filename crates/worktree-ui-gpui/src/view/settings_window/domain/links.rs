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
}
