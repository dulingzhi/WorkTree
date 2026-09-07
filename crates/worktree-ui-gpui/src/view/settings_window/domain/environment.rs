//! Environment category: informational rows about the runtime
//! environment (paths, versions, platform details).

use super::*;

impl SettingsWindowView {
    pub(in crate::view::settings_window) fn environment_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        _cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.card(
            "settings_window_environment",
            tr_str("settings.nav.environment"),
            theme,
        )
        .child(self.info_row(
            "settings_window_build",
            tr_str("settings.environment.build"),
            self.runtime_info.app_version_display.clone(),
            theme,
        ))
        .child(
            self.info_row(
                "settings_window_os",
                tr_str("settings.environment.operating_system"),
                self.runtime_info.operating_system.clone(),
                theme,
            )
            .border_color(no_separator),
        )
    }
}
