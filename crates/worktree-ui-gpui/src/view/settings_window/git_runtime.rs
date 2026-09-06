//! Git runtime: executable preference, version probing, compatibility
//! classification and the runtime rows of the Git Executable card.

use super::*;
use gpui::Stateful;
use std::path::PathBuf;
use worktree_core::process::{GitRuntimeState, current_git_runtime, install_git_executable_path};

pub(super) const MIN_GIT_MAJOR: u32 = 2;

pub(super) const MIN_GIT_MINOR: u32 = 50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitExecutableMode {
    SystemPath,
    Custom,
}

impl GitExecutableMode {
    pub(super) fn from_preference(preference: &GitExecutablePreference) -> Self {
        match preference {
            GitExecutablePreference::SystemPath => Self::SystemPath,
            GitExecutablePreference::Custom(_) => Self::Custom,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct SettingsRuntimeInfo {
    pub(super) git: GitRuntimeInfo,
    pub(super) app_version_display: SharedString,
    pub(super) operating_system: SharedString,
}

#[derive(Clone, Debug)]
pub(super) struct GitRuntimeInfo {
    pub(super) runtime: GitRuntimeState,
    pub(super) version_display: SharedString,
    pub(super) compatibility: GitCompatibility,
    pub(super) detail: Option<SharedString>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GitCompatibility {
    Supported,
    TooOld,
    Unknown,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GitVersion {
    pub(super) major: u32,
    pub(super) minor: u32,
}

pub(super) fn applied_git_executable_path(runtime: &GitRuntimeState) -> Option<PathBuf> {
    match &runtime.preference {
        GitExecutablePreference::SystemPath => None,
        GitExecutablePreference::Custom(path) => Some(path.clone()),
    }
}

pub(super) fn git_executable_scope_note() -> &'static str {
    tr_str("settings.git_executable.scope_note")
}

impl SettingsRuntimeInfo {
    pub(super) fn detect() -> Self {
        // The cached runtime, not a fresh probe: probing runs `git --version`
        // as a subprocess on the UI thread every time this window opens, the
        // slot is already populated at launch, and every path that can change
        // the preference (applying a custom executable in this window)
        // re-probes and reports back through `sync_git_runtime_state`.
        Self::from_runtime(current_git_runtime())
    }

    fn from_runtime(runtime: GitRuntimeState) -> Self {
        Self {
            git: git_runtime_info_from_state(runtime),
            app_version_display: format!("WorkTree v{}", env!("CARGO_PKG_VERSION")).into(),
            operating_system: format!(
                "{} ({})",
                os_display_name(std::env::consts::OS),
                std::env::consts::ARCH
            )
            .into(),
        }
    }
}

/// Human-readable OS name for the Environment card ("windows" reads like a
/// debug dump; "Windows" reads like a product).
fn os_display_name(os: &str) -> &str {
    match os {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        other => other,
    }
}

pub(super) fn git_runtime_info_from_state(runtime: GitRuntimeState) -> GitRuntimeInfo {
    let compatibility_message = t!(
        "settings.git.compatibility_note",
        version = format!("{MIN_GIT_MAJOR}.{MIN_GIT_MINOR}")
    )
    .into_owned();
    let compatibility = if !runtime.is_available() {
        GitCompatibility::Unavailable
    } else {
        match runtime.version_output().and_then(parse_git_version) {
            Some(version) if is_supported_git_version(version) => GitCompatibility::Supported,
            Some(_) => GitCompatibility::TooOld,
            None => GitCompatibility::Unknown,
        }
    };

    let version_display = runtime
        .version_output()
        .unwrap_or(tr_str("settings.common.unavailable"))
        .to_string()
        .into();

    let detail = match compatibility {
        GitCompatibility::Supported => None,
        GitCompatibility::TooOld | GitCompatibility::Unknown => Some(compatibility_message.into()),
        GitCompatibility::Unavailable => runtime
            .unavailable_detail()
            .map(|detail| SharedString::from(detail.to_string())),
    };

    GitRuntimeInfo {
        runtime,
        version_display,
        compatibility,
        detail,
    }
}

pub(super) fn parse_git_version(raw: &str) -> Option<GitVersion> {
    raw.split_whitespace().find_map(parse_git_version_token)
}

pub(super) fn parse_git_version_token(token: &str) -> Option<GitVersion> {
    let mut parts = token.split('.');
    let major = parse_u32_prefix(parts.next()?)?;
    let minor = parse_u32_prefix(parts.next()?)?;
    Some(GitVersion { major, minor })
}

pub(super) fn parse_u32_prefix(part: &str) -> Option<u32> {
    let end = part
        .char_indices()
        .find_map(|(ix, ch)| (!ch.is_ascii_digit()).then_some(ix))
        .unwrap_or(part.len());
    if end == 0 {
        return None;
    }
    part[..end].parse::<u32>().ok()
}

pub(super) fn is_supported_git_version(version: GitVersion) -> bool {
    version.major > MIN_GIT_MAJOR
        || (version.major == MIN_GIT_MAJOR && version.minor >= MIN_GIT_MINOR)
}

impl SettingsWindowView {
    fn selected_git_executable_path(&self) -> Option<std::path::PathBuf> {
        match self.git_executable_mode {
            GitExecutableMode::SystemPath => None,
            GitExecutableMode::Custom => {
                let trimmed = self.git_custom_path_draft.trim();
                Some(if trimmed.is_empty() {
                    std::path::PathBuf::new()
                } else {
                    std::path::PathBuf::from(trimmed)
                })
            }
        }
    }

    fn sync_git_runtime_state(&mut self, runtime: GitRuntimeState, cx: &mut gpui::Context<Self>) {
        self.git_executable_mode = GitExecutableMode::from_preference(&runtime.preference);
        if let GitExecutablePreference::Custom(path) = &runtime.preference {
            let next_draft = if path.as_os_str().is_empty() {
                String::new()
            } else {
                path.display().to_string()
            };
            if self.git_custom_path_draft != next_draft {
                self.git_custom_path_draft = next_draft.clone();
                self.git_executable_input
                    .update(cx, |input, cx| input.set_text(next_draft, cx));
            }
        }

        self.runtime_info = SettingsRuntimeInfo::from_runtime(runtime.clone());
        self.persist_preferences(cx);
        self.update_main_windows(cx, move |view, _window, _cx| {
            view.store
                .dispatch(Msg::SetGitRuntimeState(runtime.clone()));
        });
        cx.notify();
    }

    pub(super) fn apply_git_executable_settings(&mut self, cx: &mut gpui::Context<Self>) {
        let runtime = install_git_executable_path(self.selected_git_executable_path());
        self.sync_git_runtime_state(runtime, cx);
    }

    pub(super) fn set_git_executable_mode(
        &mut self,
        mode: GitExecutableMode,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.git_executable_mode == mode {
            return;
        }

        self.git_executable_mode = mode;
        self.apply_git_executable_settings(cx);
    }

    pub(super) fn git_runtime_row(&self, theme: AppTheme) -> Stateful<gpui::Div> {
        let min_git_version = format!("{MIN_GIT_MAJOR}.{MIN_GIT_MINOR}");
        let (git_icon_path, git_icon_color, git_status_text): (
            &'static str,
            gpui::Rgba,
            SharedString,
        ) = match self.runtime_info.git.compatibility {
            GitCompatibility::Supported => (
                "icons/check.svg",
                theme.colors.status.success.foreground,
                t!("settings.git.status_supported", version = min_git_version)
                    .into_owned()
                    .into(),
            ),
            GitCompatibility::TooOld => (
                "icons/warning.svg",
                theme.colors.status.warning.foreground,
                t!("settings.git.status_too_old", version = min_git_version)
                    .into_owned()
                    .into(),
            ),
            GitCompatibility::Unknown => (
                "icons/warning.svg",
                theme.colors.status.warning.foreground,
                tr("settings.git.status_unknown"),
            ),
            GitCompatibility::Unavailable => (
                "icons/warning.svg",
                theme.colors.status.danger.foreground,
                tr("settings.common.unavailable"),
            ),
        };

        div()
            .id("settings_window_git_runtime")
            .debug_selector(|| "settings_window_git_runtime".to_string())
            .w_full()
            .px_2()
            .pt_1()
            .pb_3()
            .flex()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .child(
                div()
                    .debug_selector(|| "settings_window_git_runtime_label".to_string())
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_sm()
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(tr_str("settings.git.detected_runtime")),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "settings_window_git_runtime_value".to_string())
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .overflow_hidden()
                    .child(svg_icon(git_icon_path, git_icon_color, px(14.0)))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .text_sm()
                            .font_family(UI_MONOSPACE_FONT_FAMILY)
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(self.runtime_info.git.version_display.clone()),
                    )
                    .child(
                        div()
                            .min_w(px(0.0))
                            .text_xs()
                            .text_color(git_icon_color)
                            .line_clamp(1)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .flex_shrink_0()
                            .child(git_status_text),
                    ),
            )
    }
}
