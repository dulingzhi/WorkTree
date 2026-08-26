use gpui::Decorations;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinuxGuiEnvironment {
    pub(crate) is_wsl: bool,
    pub(crate) has_x11: bool,
    pub(crate) has_wayland: bool,
    pub(crate) has_xdg_runtime_dir: bool,
}

impl LinuxGuiEnvironment {
    #[cfg(target_os = "linux")]
    pub(crate) fn detect() -> Self {
        let osrelease = read_linux_osrelease();
        Self::from_sources(
            detect_is_wsl(
                env_has_non_empty_os(std::env::var_os("WSL_DISTRO_NAME").as_deref()),
                env_has_non_empty_os(std::env::var_os("WSL_INTEROP").as_deref()),
                osrelease.as_deref(),
            ),
            env_has_non_empty_os(std::env::var_os("DISPLAY").as_deref()),
            env_has_non_empty_os(std::env::var_os("WAYLAND_DISPLAY").as_deref()),
            env_has_non_empty_os(std::env::var_os("XDG_RUNTIME_DIR").as_deref()),
        )
    }

    #[cfg(any(target_os = "linux", test))]
    fn from_sources(
        is_wsl: bool,
        has_x11: bool,
        has_wayland: bool,
        has_xdg_runtime_dir: bool,
    ) -> Self {
        Self {
            is_wsl,
            has_x11,
            has_wayland,
            has_xdg_runtime_dir,
        }
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn session_is_gui_capable(&self) -> bool {
        self.has_x11 || (self.has_wayland && self.has_xdg_runtime_dir)
    }

    pub(crate) fn should_render_custom_window_chrome(decorations: Decorations) -> bool {
        let _ = decorations;
        true
    }

    pub(crate) fn should_suppress_custom_window_frame(decorations: Decorations) -> bool {
        !Self::should_render_custom_window_chrome(decorations)
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn launch_failure_message(&self) -> String {
        if self.is_wsl {
            if self.has_wayland && !self.has_xdg_runtime_dir && !self.has_x11 {
                return crate::i18n::t!("misc.linux_gui.wsl_wayland_missing_xdg").into_owned();
            }
            return crate::i18n::t!("misc.linux_gui.wsl_no_gui").into_owned();
        }

        if self.has_wayland && !self.has_xdg_runtime_dir && !self.has_x11 {
            return crate::i18n::t!("misc.linux_gui.wayland_missing_xdg").into_owned();
        }

        crate::i18n::t!("misc.linux_gui.no_gui").into_owned()
    }
}

#[cfg(target_os = "linux")]
fn env_has_non_empty_os(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

#[cfg(any(target_os = "linux", test))]
fn detect_is_wsl(
    has_wsl_distro_name: bool,
    has_wsl_interop: bool,
    osrelease: Option<&str>,
) -> bool {
    has_wsl_distro_name || has_wsl_interop || osrelease_mentions_microsoft(osrelease)
}

#[cfg(any(target_os = "linux", test))]
fn osrelease_mentions_microsoft(osrelease: Option<&str>) -> bool {
    osrelease
        .map(|value| value.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn read_linux_osrelease() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Tiling;

    #[test]
    fn detect_is_wsl_prefers_explicit_wsl_environment_variables() {
        assert!(detect_is_wsl(true, false, None));
        assert!(detect_is_wsl(false, true, None));
    }

    #[test]
    fn detect_is_wsl_falls_back_to_linux_osrelease() {
        assert!(detect_is_wsl(
            false,
            false,
            Some("6.6.87.2-microsoft-standard-WSL2")
        ));
        assert!(!detect_is_wsl(false, false, Some("6.8.0-generic")));
    }

    #[test]
    fn session_is_gui_capable_accepts_x11_without_xdg_runtime_dir() {
        let env = LinuxGuiEnvironment::from_sources(false, true, false, false);
        assert!(env.session_is_gui_capable());
    }

    #[test]
    fn session_is_gui_capable_requires_xdg_runtime_dir_for_wayland_only_sessions() {
        let env = LinuxGuiEnvironment::from_sources(false, false, true, false);
        assert!(!env.session_is_gui_capable());
    }

    #[test]
    fn suppresses_custom_window_frame_for_server_decorations() {
        assert!(!LinuxGuiEnvironment::should_suppress_custom_window_frame(
            Decorations::Server
        ));
        assert!(!LinuxGuiEnvironment::should_suppress_custom_window_frame(
            Decorations::Client {
                tiling: Tiling::default(),
            }
        ));
    }

    #[test]
    fn custom_window_chrome_is_kept_for_server_decorations() {
        assert!(LinuxGuiEnvironment::should_render_custom_window_chrome(
            Decorations::Server
        ));
        assert!(LinuxGuiEnvironment::should_render_custom_window_chrome(
            Decorations::Client {
                tiling: Tiling::default(),
            }
        ));
    }

    #[test]
    fn launch_failure_message_mentions_wslg_for_wsl_sessions_without_display() {
        let env = LinuxGuiEnvironment::from_sources(true, false, false, false);
        let message = env.launch_failure_message();
        assert!(message.contains("WSLg"));
        assert!(message.contains("WAYLAND_DISPLAY"));
    }

    #[test]
    fn launch_failure_message_mentions_xdg_runtime_for_wayland_only_sessions() {
        let env = LinuxGuiEnvironment::from_sources(false, false, true, false);
        let message = env.launch_failure_message();
        assert!(message.contains("XDG_RUNTIME_DIR"));
    }
}
