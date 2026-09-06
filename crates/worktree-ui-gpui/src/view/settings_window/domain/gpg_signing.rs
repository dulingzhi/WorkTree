//! GPG signing settings: the git-global config snapshot and its writers.

use super::*;

/// Snapshot of git's global commit-signing config, as edited by the GPG
/// signing card. Reads and writes go straight through `git config --global`
/// — there is no app-side persistence for these keys.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::view::settings_window) struct GpgConfig {
    pub(in crate::view::settings_window) commit_signing_enabled: bool,
    pub(in crate::view::settings_window) user_signing_key: String,
    pub(in crate::view::settings_window) gpg_program: String,
}

impl GpgConfig {
    /// Test builds skip the subprocess probe: every settings-window test
    /// constructs the view, and the reads would slow them down while
    /// observing the developer's real global config.
    #[cfg(test)]
    pub(in crate::view::settings_window) fn read_from_git() -> Self {
        GpgConfig::default()
    }

    #[cfg(not(test))]
    pub(in crate::view::settings_window) fn read_from_git() -> Self {
        // One `git config --global --list` spawn covers all three keys; a
        // per-key `--get` made opening this window cost three process spawns
        // on the UI thread, which antivirus scanning turns into a visible
        // stall on Windows.
        let pairs = worktree_core::process::git_config_global_pairs();
        GpgConfig {
            commit_signing_enabled: worktree_core::process::git_config_pair_last(
                &pairs,
                "commit.gpgsign",
            ) == Some("true"),
            user_signing_key: worktree_core::process::git_config_pair_last(
                &pairs,
                "user.signingkey",
            )
            .unwrap_or_default()
            .to_string(),
            gpg_program: worktree_core::process::git_config_pair_last(&pairs, "gpg.program")
                .unwrap_or_default()
                .to_string(),
        }
    }
}

impl SettingsWindowView {
    /// Persist one GPG-related global git-config key and fold the result
    /// into `gpg_config`/`gpg_save_error`. Test builds record the write
    /// instead of touching the developer's real global config.
    fn write_gpg_config(
        &mut self,
        key: &'static str,
        value: Option<&str>,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        #[cfg(test)]
        {
            self.gpg_config_test_writes
                .push((key.to_string(), value.map(str::to_string)));
            self.gpg_save_error = None;
            cx.notify();
            return true;
        }

        #[allow(unreachable_code)]
        #[cfg(not(test))]
        {
            match worktree_core::process::git_config_global_set(key, value) {
                Ok(()) => {
                    self.gpg_save_error = None;
                    cx.notify();
                    true
                }
                Err(err) => {
                    self.gpg_save_error = Some(err.to_string());
                    cx.notify();
                    false
                }
            }
        }
    }

    pub(in crate::view::settings_window) fn set_gpg_commit_signing(
        &mut self,
        enabled: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.gpg_config.commit_signing_enabled == enabled {
            return;
        }
        // Always an explicit value: git treats a bare `commit.gpgsign` (key
        // with no value) as true, which is not what a disabled toggle shows.
        let value = if enabled { "true" } else { "false" };
        if self.write_gpg_config("commit.gpgsign", Some(value), cx) {
            self.gpg_config.commit_signing_enabled = enabled;
        }
    }

    pub(in crate::view::settings_window) fn apply_gpg_signing_key(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = self.gpg_signing_key_draft.trim().to_string();
        let value = (!next.is_empty()).then(|| next.as_str());
        if self.write_gpg_config("user.signingkey", value, cx) {
            self.gpg_config.user_signing_key = next;
        }
    }

    pub(in crate::view::settings_window) fn apply_gpg_program(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = self.gpg_program_draft.trim().to_string();
        let value = (!next.is_empty()).then(|| next.as_str());
        if self.write_gpg_config("gpg.program", value, cx) {
            self.gpg_config.gpg_program = next;
        }
    }
}
