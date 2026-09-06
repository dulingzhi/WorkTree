//! Window chrome: sizing, window/titlebar options, the client frame and
//! the custom-chrome helpers the settings window shares with the main window.

use super::*;
use gpui::{
    TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowOptions,
};

pub(super) const SETTINGS_WINDOW_MIN_WIDTH_PX: f32 = 620.0;

pub(super) const SETTINGS_WINDOW_MIN_HEIGHT_PX: f32 = 460.0;

pub(super) const SETTINGS_WINDOW_DEFAULT_WIDTH_PX: f32 = 720.0;

pub(super) const SETTINGS_WINDOW_DEFAULT_HEIGHT_PX: f32 = 620.0;

// The user-visible title is translated (`settings.window.title`); the tests
// below still assert against this literal, which is the English catalog value.
#[cfg(test)]
pub(super) const SETTINGS_WINDOW_TITLE: &str = "Settings: WorkTree";

const SETTINGS_TRAFFIC_LIGHTS_SAFE_INSET_PX: f32 = 78.0;

fn settings_window_min_size_for_percent(percent: u32) -> gpui::Size<Pixels> {
    ui_scale::design_size_from_percent(
        SETTINGS_WINDOW_MIN_WIDTH_PX,
        SETTINGS_WINDOW_MIN_HEIGHT_PX,
        percent,
    )
}

pub(super) fn settings_window_default_size_for_percent(percent: u32) -> gpui::Size<Pixels> {
    ui_scale::design_size_from_percent(
        SETTINGS_WINDOW_DEFAULT_WIDTH_PX,
        SETTINGS_WINDOW_DEFAULT_HEIGHT_PX,
        percent,
    )
}

fn settings_window_traffic_light_position(_percent: u32) -> Point<Pixels> {
    point(px(9.0), px(9.0))
}

pub(super) fn settings_window_traffic_lights_safe_inset(_percent: u32) -> Pixels {
    px(SETTINGS_TRAFFIC_LIGHTS_SAFE_INSET_PX)
}

#[cfg(test)]
pub(super) fn settings_window_options(bounds: Bounds<Pixels>) -> WindowOptions {
    settings_window_options_for_scale(bounds, ui_scale::DEFAULT_UI_SCALE_PERCENT)
}

pub(super) fn settings_window_options_for_scale(
    bounds: Bounds<Pixels>,
    ui_scale_percent: u32,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(settings_window_min_size_for_percent(ui_scale_percent)),
        titlebar: Some(settings_window_titlebar_options_for_scale(ui_scale_percent)),
        app_id: Some("worktree-settings".into()),
        window_decorations: Some(WindowDecorations::Client),
        // Match the main window: the area outside the rounded client frame
        // must be see-through.
        window_background: if cfg!(target_os = "macos") {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Transparent
        },
        is_movable: true,
        is_resizable: true,
        ..Default::default()
    }
}

#[cfg(test)]
pub(super) fn settings_window_titlebar_options() -> TitlebarOptions {
    settings_window_titlebar_options_for_scale(ui_scale::DEFAULT_UI_SCALE_PERCENT)
}

fn settings_window_titlebar_options_for_scale(ui_scale_percent: u32) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(tr_str("settings.window.title").into()),
        // Windows needs a transparent native titlebar to avoid rendering its own
        // caption on top of the custom settings header.
        appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
        traffic_light_position: cfg!(target_os = "macos")
            .then_some(settings_window_traffic_light_position(ui_scale_percent)),
    }
}

#[cfg(test)]
pub(super) fn settings_window_client_inset() -> Pixels {
    settings_window_client_inset_for_scale(ui_scale::DEFAULT_UI_SCALE_PERCENT)
}

pub(super) fn settings_window_client_inset_for_scale(ui_scale_percent: u32) -> Pixels {
    if cfg!(target_os = "windows") {
        px(0.0)
    } else {
        chrome::client_side_decoration_inset(ui_scale_percent)
    }
}

pub(super) fn settings_window_frame(
    theme: AppTheme,
    decorations: Decorations,
    content: AnyElement,
    ui_scale_percent: u32,
) -> AnyElement {
    if cfg!(target_os = "windows") {
        content
    } else {
        chrome::window_frame(theme, decorations, content, None, ui_scale_percent)
    }
}

impl SettingsWindowView {
    pub(crate) fn apply_ui_scale_percent(
        &mut self,
        percent: u32,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let percent = ui_scale::sanitize_percent(Some(percent));
        if self.ui_scale_percent == percent {
            return;
        }

        self.ui_scale_percent = percent;
        ui_scale::apply_to_window(window, percent);
        crate::app::ensure_window_respects_min_size(
            window,
            settings_window_min_size_for_percent(percent),
        );
        cx.notify();
    }
}
