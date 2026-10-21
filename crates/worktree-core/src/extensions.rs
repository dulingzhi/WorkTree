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
    /// Optional stable id used for runtime enable/disable (iteration 09-A).
    /// Legacy hooks omit it; only hooks with a non-empty `id` can be toggled
    /// off from the extension manager, so an absent id always stays enabled.
    pub id: String,
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

/// Merge a global extension config with a repository-local one (iteration 09-A).
///
/// * Commands sharing an `id` are overridden by the repository-local entry
///   (repository config wins; the global command's position is preserved when
///   it already existed, otherwise the repo command is appended).
/// * Hooks are concatenated so that both the global and repository-local hooks
///   for an event fire — hooks carry no identity, so "override" is meaningless.
///
/// Both inputs are themselves tolerant (see [`load_extensions`]); merging two
/// valid configs can never fail.
pub fn merge_extensions(global: &Extensions, repo: &Extensions) -> Extensions {
    let mut merged = global.clone();
    for repo_cmd in &repo.commands {
        match merged.commands.iter().position(|c| c.id == repo_cmd.id) {
            Some(pos) => merged.commands[pos] = repo_cmd.clone(),
            None => merged.commands.push(repo_cmd.clone()),
        }
    }
    merged.hooks.extend(repo.hooks.iter().cloned());
    merged
}

/// Where an extension entry came from in the merged (builtin ∪ global ∪
/// repo-local) view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionSource {
    /// Shipped with GitComet itself and embedded into the binary. Lowest
    /// precedence: a global or repository-local entry with the same `id`
    /// overrides a builtin one.
    Builtin,
    /// Loaded from the global `extensions.toml` (the app data dir).
    Global,
    /// Loaded from the repository-local `extensions.toml` (the repo workdir).
    Repo,
}

/// The bundled extension document, embedded at compile time. See
/// `resources/builtin-extensions.toml` for the shipped set and its scope.
const BUILTIN_EXTENSIONS_TOML: &str = include_str!("../resources/builtin-extensions.toml");

/// Extensions shipped with GitComet itself (iteration 09-A Slice 3). These form
/// the lowest-precedence layer of the merged set: a global or repository-local
/// command with the same `id` overrides a builtin one, and every builtin entry
/// is source-tagged [`ExtensionSource::Builtin`] so the extension manager can
/// surface and toggle it.
///
/// The document is embedded at compile time. A malformed file is a build
/// authoring error, but we degrade to an empty config (with a warning) rather
/// than panic on first use, matching the tolerance of [`load_extensions`].
pub fn builtin_extensions() -> Extensions {
    match parse_extensions(BUILTIN_EXTENSIONS_TOML) {
        Ok(ext) => ext,
        Err(err) => {
            crate::applog_warn!("bundled builtin-extensions.toml parse failed: {err}");
            Extensions::default()
        }
    }
}

/// A command paired with the source config it was resolved from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotatedExtension {
    pub command: ExtensionCommand,
    pub source: ExtensionSource,
}

/// A hook paired with the source config it was resolved from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotatedHook {
    pub hook: ExtensionHook,
    pub source: ExtensionSource,
}

/// Like [`merge_extensions`] but records, for every resulting command and hook,
/// which config layer it resolved from: [`ExtensionSource::Builtin`],
/// [`ExtensionSource::Global`], or [`ExtensionSource::Repo`]. The extension
/// manager UI (iteration 09-A Slice 2/3) uses this to render a source badge and
/// to drive runtime enable/disable.
///
/// * Commands are layered **builtin → global → repo**; an entry with a shared
///   `id` is overridden by the higher-precedence layer and reported with that
///   layer's source (repo > global > builtin). This mirrors the runtime
///   precedence used by [`merge_extensions`].
/// * Hooks are concatenated in the same precedence order (builtin first, then
///   global, then repository-local), each tagged by source. Hooks carry no
///   identity, so order is the only meaningful signal — all fire on their
///   event.
pub fn merge_extensions_with_source(
    builtin: &Extensions,
    global: &Extensions,
    repo: &Extensions,
) -> (Vec<AnnotatedExtension>, Vec<AnnotatedHook>) {
    let mut commands =
        Vec::with_capacity(builtin.commands.len() + global.commands.len() + repo.commands.len());
    for builtin_cmd in &builtin.commands {
        commands.push(AnnotatedExtension {
            command: builtin_cmd.clone(),
            source: ExtensionSource::Builtin,
        });
    }
    for global_cmd in &global.commands {
        match commands
            .iter_mut()
            .find(|annotated| annotated.command.id == global_cmd.id)
        {
            Some(existing) => {
                existing.command = global_cmd.clone();
                existing.source = ExtensionSource::Global;
            }
            None => commands.push(AnnotatedExtension {
                command: global_cmd.clone(),
                source: ExtensionSource::Global,
            }),
        }
    }
    for repo_cmd in &repo.commands {
        match commands
            .iter_mut()
            .find(|annotated| annotated.command.id == repo_cmd.id)
        {
            Some(existing) => {
                existing.command = repo_cmd.clone();
                existing.source = ExtensionSource::Repo;
            }
            None => commands.push(AnnotatedExtension {
                command: repo_cmd.clone(),
                source: ExtensionSource::Repo,
            }),
        }
    }

    let mut hooks = Vec::with_capacity(builtin.hooks.len() + global.hooks.len() + repo.hooks.len());
    for builtin_hook in &builtin.hooks {
        hooks.push(AnnotatedHook {
            hook: builtin_hook.clone(),
            source: ExtensionSource::Builtin,
        });
    }
    for global_hook in &global.hooks {
        hooks.push(AnnotatedHook {
            hook: global_hook.clone(),
            source: ExtensionSource::Global,
        });
    }
    for repo_hook in &repo.hooks {
        hooks.push(AnnotatedHook {
            hook: repo_hook.clone(),
            source: ExtensionSource::Repo,
        });
    }

    (commands, hooks)
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

    #[test]
    fn merge_extensions_appends_repo_commands() {
        let global = parse_extensions("[[command]]\nid = \"g\"\ncommand = \"g-cmd\"\n").unwrap();
        let repo = parse_extensions("[[command]]\nid = \"r\"\ncommand = \"r-cmd\"\n").unwrap();
        let merged = merge_extensions(&global, &repo);
        assert_eq!(merged.commands.len(), 2);
        assert_eq!(merged.commands[0].id, "g");
        assert_eq!(merged.commands[1].id, "r");
    }

    #[test]
    fn merge_extensions_repo_overrides_global_by_id() {
        let global =
            parse_extensions("[[command]]\nid = \"lint\"\ncommand = \"cargo clippy\"\n").unwrap();
        let repo = parse_extensions(
            "[[command]]\nid = \"lint\"\ncommand = \"cargo clippy --features repo\"\n",
        )
        .unwrap();
        let merged = merge_extensions(&global, &repo);
        assert_eq!(merged.commands.len(), 1);
        assert_eq!(merged.commands[0].command, "cargo clippy --features repo");
    }

    #[test]
    fn merge_extensions_concats_hooks() {
        let global =
            parse_extensions("[[hook]]\nevent = \"AfterCommit\"\ncommand = \"git push\"\n")
                .unwrap();
        let repo = parse_extensions(
            "[[hook]]\nevent = \"BeforeCommit\"\ncommand = \"cargo fmt --check\"\n",
        )
        .unwrap();
        let merged = merge_extensions(&global, &repo);
        assert_eq!(merged.hooks.len(), 2);
        // Global hook preserved, repo hook appended (hooks have no id to override).
        assert_eq!(merged.hooks[0].event, ExtensionHookEvent::AfterCommit);
        assert_eq!(merged.hooks[1].event, ExtensionHookEvent::BeforeCommit);
    }

    #[test]
    fn merge_with_source_tags_global_and_repo_commands() {
        let global = parse_extensions("[[command]]\nid = \"g\"\ncommand = \"g-cmd\"\n").unwrap();
        let repo = parse_extensions(
            "[[command]]\nid = \"r\"\ncommand = \"r-cmd\"\n\
             [[command]]\nid = \"g\"\ncommand = \"r-over\"\n",
        )
        .unwrap();
        let (commands, _hooks) =
            merge_extensions_with_source(&Extensions::default(), &global, &repo);
        assert_eq!(commands.len(), 2);
        let global_cmd = commands.iter().find(|c| c.command.id == "g").unwrap();
        // The shared-id command is overridden by the repo config and tagged Repo.
        assert_eq!(global_cmd.source, ExtensionSource::Repo);
        assert_eq!(global_cmd.command.command, "r-over");
        let repo_cmd = commands.iter().find(|c| c.command.id == "r").unwrap();
        assert_eq!(repo_cmd.source, ExtensionSource::Repo);
    }

    #[test]
    fn merge_with_source_concats_hooks_with_source() {
        let global =
            parse_extensions("[[hook]]\nevent = \"AfterCommit\"\ncommand = \"git push\"\n")
                .unwrap();
        let repo = parse_extensions(
            "[[hook]]\nevent = \"BeforeCommit\"\ncommand = \"cargo fmt --check\"\n",
        )
        .unwrap();
        let (_commands, hooks) =
            merge_extensions_with_source(&Extensions::default(), &global, &repo);
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0].source, ExtensionSource::Global);
        assert_eq!(hooks[1].source, ExtensionSource::Repo);
    }

    #[test]
    fn merge_with_source_empty_configs_are_empty() {
        let (commands, hooks) = merge_extensions_with_source(
            &Extensions::default(),
            &Extensions::default(),
            &Extensions::default(),
        );
        assert!(commands.is_empty());
        assert!(hooks.is_empty());
    }

    #[test]
    fn merge_with_source_tags_builtin_commands() {
        // A builtin-only command keeps the Builtin source.
        let builtin =
            parse_extensions("[[command]]\nid = \"fetch-all\"\ncommand = \"git fetch\"\n").unwrap();
        let (commands, _hooks) =
            merge_extensions_with_source(&builtin, &Extensions::default(), &Extensions::default());
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].source, ExtensionSource::Builtin);
        assert_eq!(commands[0].command.id, "fetch-all");
    }

    #[test]
    fn merge_with_source_precedence_repo_over_global_over_builtin() {
        // Same id across all three layers: repo wins and is tagged Repo.
        let builtin = parse_extensions("[[command]]\nid = \"lint\"\ncommand = \"b\"\n").unwrap();
        let global = parse_extensions("[[command]]\nid = \"lint\"\ncommand = \"g\"\n").unwrap();
        let repo = parse_extensions("[[command]]\nid = \"lint\"\ncommand = \"r\"\n").unwrap();
        let (commands, _hooks) = merge_extensions_with_source(&builtin, &global, &repo);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].source, ExtensionSource::Repo);
        assert_eq!(commands[0].command.command, "r");

        // Global overrides builtin but not repo.
        let (commands, _hooks) =
            merge_extensions_with_source(&builtin, &global, &Extensions::default());
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].source, ExtensionSource::Global);
        assert_eq!(commands[0].command.command, "g");
    }

    #[test]
    fn merge_with_source_concats_hooks_in_precedence_order() {
        let builtin =
            parse_extensions("[[hook]]\nevent = \"AfterCommit\"\ncommand = \"b-push\"\n").unwrap();
        let global =
            parse_extensions("[[hook]]\nevent = \"AfterCommit\"\ncommand = \"g-push\"\n").unwrap();
        let repo =
            parse_extensions("[[hook]]\nevent = \"AfterCommit\"\ncommand = \"r-push\"\n").unwrap();
        let (_commands, hooks) = merge_extensions_with_source(&builtin, &global, &repo);
        assert_eq!(hooks.len(), 3);
        assert_eq!(hooks[0].source, ExtensionSource::Builtin);
        assert_eq!(hooks[1].source, ExtensionSource::Global);
        assert_eq!(hooks[2].source, ExtensionSource::Repo);
    }

    #[test]
    fn builtin_extensions_parse_and_tag_as_builtin() {
        // Guards the embedded resource: it must parse and ship the expected
        // commands (ids + sane fields). If someone edits the bundled TOML this
        // test catches a malformed or regressed default set.
        let builtin = builtin_extensions();
        assert!(
            builtin
                .commands
                .iter()
                .any(|c| c.id == "fetch-all" && !c.command.is_empty() && c.requires_repo),
            "builtin set must ship fetch-all with a script and requires_repo"
        );
        assert!(
            builtin
                .commands
                .iter()
                .any(|c| c.id == "pull-rebase" && !c.command.is_empty() && c.requires_repo),
            "builtin set must ship pull-rebase with a script and requires_repo"
        );
        // Whatever the bundled set contains, every entry must be resolvable and
        // tagged Builtin when merged with empty global/repo configs.
        let (commands, _hooks) =
            merge_extensions_with_source(&builtin, &Extensions::default(), &Extensions::default());
        assert_eq!(commands.len(), builtin.commands.len());
        assert!(
            commands
                .iter()
                .all(|c| c.source == ExtensionSource::Builtin)
        );
    }
}
