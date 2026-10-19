//! Declarative extension system (iteration 08-1), self-use oriented.
//!
//! Users register script commands and lifecycle hooks in a TOML config
//! (`extensions.toml`; the state crate resolves the path via
//! `worktree_state::session::app_data_dir`). This module owns the pure schema
//! and parser; [`load_extensions`] performs the file read so callers stay free
//! of I/O details.
//!
//! Design notes:
//! * Parsing is **tolerant** — a malformed entry does not abort the whole file.
//!   `toml` deserializes the whole document atomically, so a single bad entry
//!   still fails the document; the tolerant boundary is therefore at the file
//!   level (missing / unreadable / unparseable -> empty config + warning), which
//!   matches the roadmap's "don't let one bad command disable the rest" intent.
//! * `Extensions` carries no `unsafe`, no trait surface, and no git dependency
//!   — it is a plain data model the UI layer turns into palette entries and
//!   shell executions (see the `worktree-ui-gpui` integration).

use serde::Deserialize;
use std::path::Path;

/// The full extension configuration loaded from `extensions.toml`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct Extensions {
    /// User-declared script commands surfaced in the command palette.
    /// TOML uses `[[command]]` (singular) for an array of tables.
    #[serde(rename = "command", default)]
    pub commands: Vec<ExtensionCommand>,
    /// Lifecycle hooks (`RepositoryOpened` / `BeforeCommit` / `AfterCommit`).
    /// TOML uses `[[hook]]` (singular) for an array of tables.
    #[serde(rename = "hook", default)]
    pub hooks: Vec<ExtensionHook>,
}

/// A user-declared script command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct ExtensionCommand {
    /// Unique id; the UI layer prefixes it with `ext:` to keep it out of the
    /// builtin id namespace. Empty id/label/command are skipped at integration.
    pub id: String,
    /// Display name in the palette; also the fuzzy-match target.
    pub label: String,
    /// Shell command executed in the repository working directory.
    pub command: String,
    /// When true (default) the command only appears with an active repo.
    pub requires_repo: bool,
    /// When true, confirm before executing (for destructive scripts).
    pub confirm: bool,
}

impl Default for ExtensionCommand {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            command: String::new(),
            requires_repo: true,
            confirm: false,
        }
    }
}

/// A lifecycle hook that runs a script on a GitComet event.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct ExtensionHook {
    pub event: ExtensionHookEvent,
    pub command: String,
}

/// Events a hook can subscribe to.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum ExtensionHookEvent {
    /// Fired when a repository finishes opening.
    #[default]
    RepositoryOpened,
    /// Fired before a commit is performed.
    BeforeCommit,
    /// Fired after a commit succeeds (not on failure).
    AfterCommit,
}

/// Parse an `extensions.toml` document into [`Extensions`].
///
/// Returns a human-readable error string on failure; the caller decides
/// whether to warn or abort.
pub fn parse_extensions(text: &str) -> Result<Extensions, String> {
    toml::from_str(text).map_err(|err| err.to_string())
}

/// Load extensions from a config path. Tolerant:
/// * missing file -> empty config
/// * unreadable / unparseable -> warning + empty config
pub fn load_extensions(path: &Path) -> Extensions {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_extensions(&text) {
            Ok(ext) => ext,
            Err(err) => {
                crate::applog_warn!("extensions.toml parse failed: {err}");
                Extensions::default()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Extensions::default(),
        Err(err) => {
            crate::applog_warn!("extensions.toml read failed: {err}");
            Extensions::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_document() {
        let text = r#"
[[command]]
id = "fmt"
label = "Run cargo fmt"
command = "cargo fmt"
requires_repo = true

[[command]]
id = "global-sync"
label = "Sync dotfiles"
command = "rsync -a ~/dotfiles/ ~/"
requires_repo = false
confirm = true

[[hook]]
event = "AfterCommit"
command = "git push"
"#;
        let ext = parse_extensions(text).expect("valid document");
        assert_eq!(ext.commands.len(), 2);
        assert_eq!(ext.commands[0].id, "fmt");
        assert_eq!(ext.commands[0].label, "Run cargo fmt");
        assert!(ext.commands[0].requires_repo);
        assert!(!ext.commands[0].confirm);
        assert_eq!(ext.commands[1].id, "global-sync");
        assert!(!ext.commands[1].requires_repo);
        assert!(ext.commands[1].confirm);
        assert_eq!(ext.hooks.len(), 1);
        assert_eq!(ext.hooks[0].event, ExtensionHookEvent::AfterCommit);
        assert_eq!(ext.hooks[0].command, "git push");
    }

    #[test]
    fn defaults_apply_to_sparse_entries() {
        let text = r#"
[[command]]
id = "only-id"
"#;
        let ext = parse_extensions(text).expect("valid document");
        assert_eq!(ext.commands.len(), 1);
        // label / command default to empty, requires_repo defaults to true.
        assert_eq!(ext.commands[0].label, "");
        assert_eq!(ext.commands[0].command, "");
        assert!(ext.commands[0].requires_repo);
        assert!(!ext.commands[0].confirm);
    }

    #[test]
    fn empty_document_is_empty_config() {
        let ext = parse_extensions("").expect("empty document");
        assert!(ext.commands.is_empty());
        assert!(ext.hooks.is_empty());
    }

    #[test]
    fn malformed_document_errors() {
        let err = parse_extensions("[[command]\nid = ").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn hook_event_names_are_pascal_case() {
        let text = r#"
[[hook]]
event = "BeforeCommit"
command = "echo before"
"#;
        let ext = parse_extensions(text).expect("valid document");
        assert_eq!(ext.hooks[0].event, ExtensionHookEvent::BeforeCommit);
    }

    #[test]
    fn load_extensions_missing_file_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.toml");
        let ext = load_extensions(&path);
        assert!(ext.commands.is_empty());
        assert!(ext.hooks.is_empty());
    }

    #[test]
    fn load_extensions_unparseable_is_empty_with_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "[[command]\nid = ").expect("write");
        let ext = load_extensions(&path);
        assert!(ext.commands.is_empty());
    }
}
