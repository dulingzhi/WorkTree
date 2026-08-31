//! App-level preference for the external merge tool launched by the
//! conflicted-file context menu.
//!
//! The launch path lives in `worktree-git-gix` and resolves the tool from git
//! config (`merge.tool` / `mergetool.<tool>.*`) when no preference is
//! installed, so the default behavior is unchanged. Session state only exists
//! in the UI layer while the backend runs on a worker thread, so — like
//! [`crate::process::GitExecutablePreference`] — the installed value lives in
//! a process-global slot that the settings window writes and the backend
//! reads.

use std::sync::{OnceLock, RwLock};

#[cfg(any(test, feature = "test-support"))]
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

/// Which external merge tool the "Open external mergetool" action launches.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalMergeToolSelection {
    /// Resolve the tool from git config, exactly as before this setting existed.
    #[default]
    FromGitConfig,
    /// Force one of git's built-in tool ids (the `mergetools/<id>` table).
    /// Per-tool git config keys (`mergetool.<id>.cmd/.trustExitCode`) still
    /// apply; a manual `path` set here wins over `mergetool.<id>.path` and
    /// the PATH lookup, for tools the settings UI could not find on PATH.
    Builtin {
        id: String,
        /// Manually selected executable; `None` resolves from PATH (and git
        /// config) as before this field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Run a user-authored shell command with `$BASE`/`$LOCAL`/`$REMOTE`/
    /// `$MERGED` pointing at the conflict files.
    Custom {
        command: String,
        trust_exit_code: bool,
    },
}

/// A known merge tool offered in Settings, shared by the settings UI (labels,
/// PATH detection) and the backend (the id must be a built-in table key).
#[derive(Debug, PartialEq)]
pub struct MergeToolPreset {
    /// Canonical git tool id; also the `mergetool.<id>.*` config namespace.
    pub id: &'static str,
    /// Locale key of the display name (`settings.merge_tool.tools.<id>`).
    pub label_key: &'static str,
    /// Executable names to look for on PATH, in order; mirrors git's
    /// `translate_merge_tool_path` where the command differs from the tool
    /// name. A single entry equal to the id means the tool name is the program.
    pub program_candidates: &'static [&'static str],
}

/// Built-in merge tools offered as presets, ordered for the settings dropdown.
pub const MERGE_TOOL_PRESETS: &[MergeToolPreset] = &[
    MergeToolPreset {
        id: "kdiff3",
        label_key: "settings.merge_tool.tools.kdiff3",
        program_candidates: &["kdiff3"],
    },
    MergeToolPreset {
        id: "meld",
        label_key: "settings.merge_tool.tools.meld",
        program_candidates: &["meld"],
    },
    MergeToolPreset {
        id: "bc",
        label_key: "settings.merge_tool.tools.bc",
        program_candidates: &["bcomp", "bcompare"],
    },
    MergeToolPreset {
        id: "p4merge",
        label_key: "settings.merge_tool.tools.p4merge",
        program_candidates: &["p4merge"],
    },
    MergeToolPreset {
        id: "vscode",
        label_key: "settings.merge_tool.tools.vscode",
        program_candidates: &["code"],
    },
    MergeToolPreset {
        id: "smerge",
        label_key: "settings.merge_tool.tools.smerge",
        program_candidates: &["smerge"],
    },
    MergeToolPreset {
        id: "araxis",
        label_key: "settings.merge_tool.tools.araxis",
        program_candidates: &["compare"],
    },
    MergeToolPreset {
        id: "winmerge",
        label_key: "settings.merge_tool.tools.winmerge",
        program_candidates: &["WinMergeU"],
    },
    MergeToolPreset {
        id: "tortoisemerge",
        label_key: "settings.merge_tool.tools.tortoisemerge",
        program_candidates: &["tortoisegitmerge", "tortoisemerge"],
    },
    MergeToolPreset {
        id: "opendiff",
        label_key: "settings.merge_tool.tools.opendiff",
        program_candidates: &["opendiff"],
    },
    MergeToolPreset {
        id: "vimdiff",
        label_key: "settings.merge_tool.tools.vimdiff",
        program_candidates: &["vim"],
    },
];

/// Look up a preset by canonical tool id.
pub fn merge_tool_preset(id: &str) -> Option<&'static MergeToolPreset> {
    MERGE_TOOL_PRESETS.iter().find(|preset| preset.id == id)
}

fn external_merge_tool_slot() -> &'static RwLock<ExternalMergeToolSelection> {
    static SLOT: OnceLock<RwLock<ExternalMergeToolSelection>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(ExternalMergeToolSelection::FromGitConfig))
}

/// The currently installed preference; `FromGitConfig` until the settings
/// window (or startup session load) installs a user choice.
pub fn current_external_merge_tool() -> ExternalMergeToolSelection {
    external_merge_tool_slot()
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

/// Install a preference, taking effect from the next mergetool launch.
pub fn install_external_merge_tool(selection: ExternalMergeToolSelection) {
    *external_merge_tool_slot()
        .write()
        .unwrap_or_else(|err| err.into_inner()) = selection;
}

#[cfg(any(test, feature = "test-support"))]
fn external_merge_tool_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Serialize tests that swap the merge tool preference: the process-global
/// slot is shared by every backend instance in the test binary.
#[cfg(any(test, feature = "test-support"))]
pub fn lock_external_merge_tool_test() -> MutexGuard<'static, ()> {
    external_merge_tool_test_lock()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

/// Installs a selection for the duration of a test and restores the previous
/// value on drop.
#[cfg(any(test, feature = "test-support"))]
pub struct ExternalMergeToolResetGuard {
    original: ExternalMergeToolSelection,
}

#[cfg(any(test, feature = "test-support"))]
impl ExternalMergeToolResetGuard {
    pub fn install(selection: ExternalMergeToolSelection) -> Self {
        let original = current_external_merge_tool();
        install_external_merge_tool(selection);
        Self { original }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for ExternalMergeToolResetGuard {
    fn drop(&mut self) {
        install_external_merge_tool(self.original.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_tool_preference_defaults_to_git_config() {
        // The slot starts untouched in a fresh process; no preference means
        // the backend must keep resolving the tool from git config.
        assert_eq!(
            current_external_merge_tool(),
            ExternalMergeToolSelection::FromGitConfig
        );
        assert_eq!(
            ExternalMergeToolSelection::default(),
            ExternalMergeToolSelection::FromGitConfig
        );
    }

    #[test]
    fn merge_tool_preference_install_round_trips() {
        let _lock = lock_external_merge_tool_test();
        for selection in [
            ExternalMergeToolSelection::FromGitConfig,
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: None,
            },
            ExternalMergeToolSelection::Custom {
                command: "code --wait --merge $REMOTE $LOCAL $BASE $MERGED".to_string(),
                trust_exit_code: true,
            },
        ] {
            let _guard = ExternalMergeToolResetGuard::install(selection.clone());
            assert_eq!(current_external_merge_tool(), selection);
        }
        assert_eq!(
            current_external_merge_tool(),
            ExternalMergeToolSelection::FromGitConfig
        );
    }

    #[test]
    fn merge_tool_presets_have_unique_ids_and_keys() {
        for (index, preset) in MERGE_TOOL_PRESETS.iter().enumerate() {
            assert_eq!(
                preset.label_key,
                format!("settings.merge_tool.tools.{}", preset.id),
                "label key must follow the settings.merge_tool.tools.<id> convention"
            );
            assert!(
                !preset.program_candidates.is_empty(),
                "preset {} must name at least one program candidate",
                preset.id
            );
            for earlier in &MERGE_TOOL_PRESETS[..index] {
                assert_ne!(earlier.id, preset.id, "duplicate preset id");
                assert_ne!(earlier.label_key, preset.label_key, "duplicate label key");
            }
        }
    }

    #[test]
    fn merge_tool_selection_serde_round_trips() {
        // Pin the session.json shape: tagged by "kind", snake_case variants.
        assert_eq!(
            serde_json::to_string(&ExternalMergeToolSelection::FromGitConfig).unwrap(),
            r#"{"kind":"from_git_config"}"#
        );
        assert_eq!(
            serde_json::to_string(&ExternalMergeToolSelection::Builtin {
                id: "kdiff3".to_string(),
                path: None,
            })
            .unwrap(),
            r#"{"kind":"builtin","id":"kdiff3"}"#
        );
        assert_eq!(
            serde_json::to_string(&ExternalMergeToolSelection::Custom {
                command: "meld $LOCAL $REMOTE $BASE $MERGED".to_string(),
                trust_exit_code: false,
            })
            .unwrap(),
            r#"{"kind":"custom","command":"meld $LOCAL $REMOTE $BASE $MERGED","trust_exit_code":false}"#
        );

        for selection in [
            ExternalMergeToolSelection::FromGitConfig,
            ExternalMergeToolSelection::Builtin {
                id: "smerge".to_string(),
                path: None,
            },
            ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some(r"C:\Tools\Code\code.exe".to_string()),
            },
            ExternalMergeToolSelection::Custom {
                command: "printf resolved > $MERGED".to_string(),
                trust_exit_code: true,
            },
        ] {
            let json = serde_json::to_string(&selection).unwrap();
            assert_eq!(
                serde_json::from_str::<ExternalMergeToolSelection>(&json).unwrap(),
                selection
            );
        }
    }

    #[test]
    fn merge_tool_selection_deserializes_sessions_without_manual_path() {
        // Sessions saved before the manual path field existed must keep
        // loading: `path` defaults to None and the JSON shape is unchanged.
        assert_eq!(
            serde_json::from_str::<ExternalMergeToolSelection>(r#"{"kind":"builtin","id":"meld"}"#)
                .unwrap(),
            ExternalMergeToolSelection::Builtin {
                id: "meld".to_string(),
                path: None,
            }
        );
        assert_eq!(
            serde_json::to_string(&ExternalMergeToolSelection::Builtin {
                id: "vscode".to_string(),
                path: Some("/usr/local/bin/code".to_string()),
            })
            .unwrap(),
            r#"{"kind":"builtin","id":"vscode","path":"/usr/local/bin/code"}"#
        );
    }

    #[test]
    fn merge_tool_preset_lookup_matches_ids() {
        assert_eq!(
            merge_tool_preset("vscode").map(|preset| preset.id),
            Some("vscode")
        );
        assert_eq!(merge_tool_preset("not-a-tool"), None);
    }
}
