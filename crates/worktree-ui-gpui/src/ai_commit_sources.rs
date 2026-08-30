//! Configuration sources for AI commit messages.
//!
//! The HTTP generators (`ai_commit.rs`) need provider credentials, and the
//! CLI generators need a command shape. Both come from a *source*: the
//! manually-filled settings page, credentials another tool already stored
//! (Claude Code, Codex, GitHub CLI, environment variables), or a local CLI
//! (`claude` / `codex` / `gemini` / `ollama` / a custom command).
//!
//! This mirrors WorkTree's config-source design
//! (`docs/superpowers/specs/2026-06-16-ai-commit-config-sources-design.md`):
//! external credentials are resolved at generation time and **never
//! persisted** — the session file keeps only which source was picked.
//!
//! Everything here is synchronous, side-effect-free apart from reads, and
//! parameterized over [`EnvAccess`] so tests inject a temp home directory and
//! synthetic variables instead of touching the real system.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::SharedString;

use crate::ai_commit::{AiCommitSettings, AiProvider};

/// GitHub Models' OpenAI-compatible inference endpoint. A token with
/// `models:read` (or Copilot access) answers here; a 403 usually means
/// `gh auth refresh -s models:read` is due.
const GITHUB_MODELS_ENDPOINT: &str = "https://models.github.ai/inference";
const GITHUB_MODELS_DEFAULT_MODEL: &str = "gpt-4o";

/// Where the AI commit generator gets its model access from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AiSource {
    /// The settings page's provider / API key / endpoint / model fields.
    #[default]
    Manual,
    /// `~/.claude/settings.json` env block, falling back to the environment.
    ClaudeCode,
    /// `~/.codex/auth.json` + `~/.codex/config.toml`, falling back to env.
    Codex,
    /// GitHub CLI token (hosts.yml / `GH_TOKEN` / `gh auth token`) against
    /// GitHub Models.
    Copilot,
    /// `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` (+ `*_BASE_URL`, `*_MODEL`).
    Env,
    /// Shell out to a local CLI instead of HTTP.
    CliClaude,
    CliCodex,
    CliGemini,
    CliOllama,
    /// A user-supplied command template.
    Custom,
}

impl AiSource {
    pub(crate) const ALL: &'static [AiSource] = &[
        AiSource::Manual,
        AiSource::ClaudeCode,
        AiSource::Codex,
        AiSource::Copilot,
        AiSource::Env,
        AiSource::CliClaude,
        AiSource::CliCodex,
        AiSource::CliGemini,
        AiSource::CliOllama,
        AiSource::Custom,
    ];

    /// Stable key persisted in `session.json` (matches the C# source keys).
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Env => "env",
            Self::CliClaude => "cli-claude",
            Self::CliCodex => "cli-codex",
            Self::CliGemini => "cli-gemini",
            Self::CliOllama => "cli-ollama",
            Self::Custom => "custom",
        }
    }

    pub(crate) fn from_key(raw: &str) -> Option<Self> {
        Some(match raw {
            "manual" => Self::Manual,
            "claude-code" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "copilot" => Self::Copilot,
            "env" => Self::Env,
            "cli-claude" => Self::CliClaude,
            "cli-codex" => Self::CliCodex,
            "cli-gemini" => Self::CliGemini,
            "cli-ollama" => Self::CliOllama,
            "custom" => Self::Custom,
            _ => return None,
        })
    }

    /// Label shown in the settings dropdown.
    pub(crate) fn label(self) -> SharedString {
        match self {
            Self::Manual => crate::i18n::tr("settings.ai_commit.source.manual"),
            Self::ClaudeCode => crate::i18n::tr("settings.ai_commit.source.claude_code"),
            Self::Codex => crate::i18n::tr("settings.ai_commit.source.codex"),
            Self::Copilot => crate::i18n::tr("settings.ai_commit.source.copilot"),
            Self::Env => crate::i18n::tr("settings.ai_commit.source.env"),
            Self::CliClaude => crate::i18n::tr("settings.ai_commit.source.cli_claude"),
            Self::CliCodex => crate::i18n::tr("settings.ai_commit.source.cli_codex"),
            Self::CliGemini => crate::i18n::tr("settings.ai_commit.source.cli_gemini"),
            Self::CliOllama => crate::i18n::tr("settings.ai_commit.source.cli_ollama"),
            Self::Custom => crate::i18n::tr("settings.ai_commit.source.custom"),
        }
    }

    /// CLI sources run a subprocess instead of an HTTP request.
    pub(crate) fn is_cli(self) -> bool {
        matches!(
            self,
            Self::CliClaude | Self::CliCodex | Self::CliGemini | Self::CliOllama | Self::Custom
        )
    }
}

/// The filesystem / environment seam resolvers read through. Tests inject a
/// temp home and synthetic variables; production fills both from the process.
pub(crate) struct EnvAccess {
    home: PathBuf,
    vars: HashMap<String, String>,
    /// Last-resort token source: `gh auth token`, the CLI's documented
    /// reader for tokens it keeps in the OS credential store (gh's default
    /// storage on Windows and macOS, where hosts.yml holds no token at
    /// all). Production installs the subprocess; tests stage a value so the
    /// fallback order stays covered without a real CLI.
    gh_cli_token: Option<Box<dyn Fn() -> Option<String> + Send + Sync>>,
}

impl EnvAccess {
    pub(crate) fn real() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            home,
            vars: std::env::vars().collect(),
            gh_cli_token: Some(Box::new(run_gh_auth_token)),
        }
    }

    /// A test seam: an explicit home directory and variable set.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn synthetic(home: PathBuf, vars: HashMap<String, String>) -> Self {
        Self {
            home,
            vars,
            gh_cli_token: None,
        }
    }

    /// Test seam: pretend `gh auth token` printed this (or nothing).
    #[cfg(test)]
    fn stage_gh_cli_token(&mut self, token: Option<&str>) {
        let token = token.map(str::to_string);
        self.gh_cli_token = Some(Box::new(move || token.clone()));
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars
            .get(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    /// Run the staged `gh auth token` source, if one is installed.
    fn gh_cli_token(&self) -> Option<String> {
        self.gh_cli_token.as_ref().and_then(|token| token())
    }
}

/// Ask the GitHub CLI for the signed-in token. gh's default storage is the
/// OS credential store (Windows Credential Manager, macOS keychain), which
/// leaves hosts.yml without an `oauth_token:` line — a signed-in user would
/// otherwise resolve as anonymous and every private-repo request 404. A
/// missing CLI, failing run, or empty output simply means "no token".
fn run_gh_auth_token() -> Option<String> {
    // `background_command` sets CREATE_NO_WINDOW on Windows — a bare
    // Command would flash a console window over the GUI on every resolve.
    let mut command = worktree_core::process::background_command("gh");
    let output = command.args(["auth", "token"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Resolve the HTTP settings a source yields right now, or `None` when the
/// source has no usable credentials. Manual echoes the persisted fields; the
/// external-tool sources read their config files live and nothing they find
/// is ever written back. CLI sources never go through HTTP and return `None`.
pub(crate) fn resolve_http_settings(
    source: AiSource,
    manual: &AiCommitSettings,
    env: &EnvAccess,
) -> Option<AiCommitSettings> {
    match source {
        AiSource::Manual => manual.is_configured().then(|| manual.clone()),
        AiSource::ClaudeCode => resolve_claude_code(env),
        AiSource::Codex => resolve_codex(env),
        AiSource::Copilot => resolve_copilot(env),
        AiSource::Env => resolve_env_vars(env),
        _ => None,
    }
}

fn resolve_claude_code(env: &EnvAccess) -> Option<AiCommitSettings> {
    // settings.json first, environment second — a settings block overrides,
    // an exported variable covers users who never opened the settings file.
    let read = |name: &str| read_claude_settings_env(&env.home, name).or_else(|| env.var(name));
    // Claude Code relays keep their credential in ANTHROPIC_AUTH_TOKEN, a
    // bearer token; a plain console key may sit in ANTHROPIC_API_KEY. The
    // auth token wins when both are set, and only it turns on Bearer auth.
    let (api_key, bearer_auth) = match read("ANTHROPIC_AUTH_TOKEN") {
        Some(token) => (token, true),
        None => (read("ANTHROPIC_API_KEY")?, false),
    };
    Some(AiCommitSettings {
        provider: AiProvider::Anthropic,
        api_key,
        bearer_auth,
        model: read("ANTHROPIC_MODEL").unwrap_or_default(),
        endpoint: read("ANTHROPIC_BASE_URL").unwrap_or_default(),
        ..AiCommitSettings::default()
    })
}

/// One string out of the `env` object of `~/.claude/settings.json`. A missing
/// file or malformed JSON simply yields `None` — the caller falls back to the
/// process environment.
fn read_claude_settings_env(home: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".claude").join("settings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let value = json.get("env")?.get(name)?.as_str()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn resolve_codex(env: &EnvAccess) -> Option<AiCommitSettings> {
    let api_key = read_codex_api_key(&env.home).or_else(|| env.var("OPENAI_API_KEY"))?;
    let (model, base_url) = read_codex_config(&env.home);
    Some(AiCommitSettings {
        provider: AiProvider::OpenAiCompatible,
        api_key,
        model,
        endpoint: base_url,
        ..AiCommitSettings::default()
    })
}

fn read_codex_api_key(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".codex").join("auth.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let value = json.get("OPENAI_API_KEY")?.as_str()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The two facts Codex's `config.toml` can contribute — the default `model`
/// and the selected `[model_providers.<id>]`'s `base_url` — never need more
/// than string scalars, so a line-oriented scan suffices; a real TOML parser
/// is not warranted for one config file.
fn read_codex_config(home: &Path) -> (String, String) {
    let Ok(text) = std::fs::read_to_string(home.join(".codex").join("config.toml")) else {
        return (String::new(), String::new());
    };
    let tables = parse_mini_toml(&text);
    let root = tables.get("").cloned().unwrap_or_default();
    let model = root.get("model").cloned().unwrap_or_default();
    let provider = root.get("model_provider").cloned().unwrap_or_default();
    let base_url = if provider.is_empty() {
        String::new()
    } else {
        tables
            .get(&format!("model_providers.{provider}"))
            .and_then(|table| table.get("base_url").cloned())
            .unwrap_or_default()
    };
    (model, base_url)
}

/// Minimal TOML subset: `[dotted.table]` headers plus `key = value` pairs
/// with string or bare-scalar values. Enough for `config.toml`'s model
/// selection; anything else in the file is ignored rather than rejected.
fn parse_mini_toml(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut tables: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            current = header.trim().trim_matches('"').to_string();
            tables.entry(current.clone()).or_default();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_string();
        tables
            .entry(current.clone())
            .or_default()
            .insert(key, strip_toml_value(value.trim()));
    }
    tables
}

/// Decode a `key = ` right-hand side: a quoted string keeps its inside; a
/// bare scalar ends at the first `#` comment.
fn strip_toml_value(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    raw.split('#').next().unwrap_or_default().trim().to_string()
}

fn resolve_copilot(env: &EnvAccess) -> Option<AiCommitSettings> {
    let token = read_gh_token(env)?;
    Some(AiCommitSettings {
        provider: AiProvider::OpenAiCompatible,
        api_key: token,
        endpoint: GITHUB_MODELS_ENDPOINT.to_string(),
        model: GITHUB_MODELS_DEFAULT_MODEL.to_string(),
        ..AiCommitSettings::default()
    })
}

pub(crate) fn read_gh_token(env: &EnvAccess) -> Option<String> {
    for candidate in gh_hosts_candidates(env) {
        if let Ok(text) = std::fs::read_to_string(candidate) {
            if let Some(token) = scan_oauth_token(&text) {
                return Some(token);
            }
        }
    }
    env.var("GH_TOKEN")
        .or_else(|| env.var("GITHUB_TOKEN"))
        // gh's default storage is the OS credential store, leaving hosts.yml
        // tokenless — the CLI itself is the only reader for that token.
        .or_else(|| env.gh_cli_token())
}

/// gh keeps its config under XDG on every platform (`~/.config/gh`), plus the
/// Windows roaming location when `APPDATA` is set.
fn gh_hosts_candidates(env: &EnvAccess) -> Vec<PathBuf> {
    let mut candidates = vec![env.home.join(".config").join("gh").join("hosts.yml")];
    if let Some(appdata) = env.var("APPDATA") {
        candidates.push(Path::new(&appdata).join("GitHub CLI").join("hosts.yml"));
    }
    candidates
}

/// `hosts.yml` nests per-host entries; the oauth token sits under
/// `oauth_token:` at any depth, so scan line-wise for the first non-empty
/// value (same tolerance as the C# resolver).
fn scan_oauth_token(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("oauth_token:"))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_env_vars(env: &EnvAccess) -> Option<AiCommitSettings> {
    if let Some(api_key) = env.var("ANTHROPIC_API_KEY") {
        return Some(AiCommitSettings {
            provider: AiProvider::Anthropic,
            api_key,
            model: env.var("ANTHROPIC_MODEL").unwrap_or_default(),
            endpoint: env.var("ANTHROPIC_BASE_URL").unwrap_or_default(),
            ..AiCommitSettings::default()
        });
    }
    if let Some(api_key) = env.var("OPENAI_API_KEY") {
        return Some(AiCommitSettings {
            provider: AiProvider::OpenAiCompatible,
            api_key,
            model: env.var("OPENAI_MODEL").unwrap_or_default(),
            endpoint: env.var("OPENAI_BASE_URL").unwrap_or_default(),
            ..AiCommitSettings::default()
        });
    }
    None
}

/// A built-in CLI generator's invocation shape.
pub(crate) struct CliSpec {
    pub(crate) executable: &'static str,
    pub(crate) args_template: &'static str,
    pub(crate) default_model: &'static str,
}

pub(crate) fn cli_spec(source: AiSource) -> Option<CliSpec> {
    Some(match source {
        AiSource::CliClaude => CliSpec {
            executable: "claude",
            args_template: "-p {PROMPT}",
            default_model: "",
        },
        AiSource::CliCodex => CliSpec {
            executable: "codex",
            args_template: "exec {PROMPT}",
            default_model: "",
        },
        AiSource::CliGemini => CliSpec {
            executable: "gemini",
            args_template: "-p {PROMPT}",
            default_model: "",
        },
        AiSource::CliOllama => CliSpec {
            executable: "ollama",
            args_template: "run {MODEL} {PROMPT}",
            default_model: "llama3",
        },
        _ => return None,
    })
}

/// A custom command's executable + args template: the first token is the
/// program, the remainder the template. A blank command resolves to nothing.
pub(crate) fn custom_cli_spec(command: &str) -> Option<(String, String)> {
    let mut tokens = command.split_whitespace();
    let executable = tokens.next()?.to_string();
    let template = tokens.collect::<Vec<_>>().join(" ");
    Some((executable, template))
}

/// Substitute `{PROMPT}` / `{MODEL}` in an args template. Each placeholder
/// becomes exactly one argument — the prompt is passed as a single argv
/// element, never through a shell — and a template without `{PROMPT}` gets
/// the prompt appended, so `my-tool --quiet` still receives it.
pub(crate) fn build_cli_args(template: &str, prompt: &str, model: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut has_prompt = false;
    for token in template.split_whitespace() {
        match token {
            "{PROMPT}" => {
                has_prompt = true;
                args.push(prompt.to_string());
            }
            "{MODEL}" => args.push(model.to_string()),
            other => args.push(other.to_string()),
        }
    }
    if !has_prompt {
        args.push(prompt.to_string());
    }
    args
}

/// Look a bare executable name up on `PATH`. On Windows the AI CLIs install
/// as `.cmd` shims, so the script extensions are tried alongside `.exe`;
/// anything holding a path separator must exist as-is.
pub(crate) fn find_executable(name: &str, env: &EnvAccess) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.contains('/') || name.contains('\\') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    let path = env.var("PATH")?;
    let mut names = vec![name.to_string()];
    if cfg!(windows) {
        names.push(format!("{name}.exe"));
        names.push(format!("{name}.cmd"));
        names.push(format!("{name}.bat"));
    }
    std::env::split_paths(&path).find_map(|dir| {
        names
            .iter()
            .map(|candidate| dir.join(candidate))
            .find(|full| full.is_file())
    })
}

/// What the settings page shows for a source: detected, or the reason it is
/// not. The message is a locale key plus an optional `%{detail}`
/// interpolation (the path that was missing, the executable not found) so
/// localization happens on the render side.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceAvailability {
    pub(crate) detected: bool,
    pub(crate) message: Option<(&'static str, Option<String>)>,
}

impl SourceAvailability {
    fn detected() -> Self {
        Self {
            detected: true,
            message: None,
        }
    }

    fn missing(key: &'static str, detail: Option<String>) -> Self {
        Self {
            detected: false,
            message: Some((key, detail)),
        }
    }
}

/// Check a source end to end: credentials resolve, or the CLI executable is
/// on PATH. Runs wherever it is called — the settings page calls it from a
/// background task because resolution touches the filesystem.
pub(crate) fn check_availability(
    source: AiSource,
    manual: &AiCommitSettings,
    custom_command: &str,
    env: &EnvAccess,
) -> SourceAvailability {
    if let Some(spec) = cli_spec(source) {
        return if find_executable(spec.executable, env).is_some() {
            SourceAvailability::detected()
        } else {
            SourceAvailability::missing(
                "settings.ai_commit.missing.cli",
                Some(spec.executable.to_string()),
            )
        };
    }
    if source == AiSource::Custom {
        return if custom_command.trim().is_empty() {
            SourceAvailability::missing("settings.ai_commit.missing.custom_empty", None)
        } else {
            // The command's shape is all that can be checked here — whether
            // its program actually runs is answered at generation time.
            SourceAvailability::detected()
        };
    }
    if resolve_http_settings(source, manual, env).is_some() {
        SourceAvailability::detected()
    } else {
        SourceAvailability::missing(unavailable_key(source), unavailable_detail(source, env))
    }
}

/// Locale key naming why a non-CLI source resolved to nothing.
pub(crate) fn unavailable_key(source: AiSource) -> &'static str {
    match source {
        AiSource::Manual => "settings.ai_commit.missing.manual",
        AiSource::ClaudeCode => "settings.ai_commit.missing.claude_code",
        AiSource::Codex => "settings.ai_commit.missing.codex",
        AiSource::Copilot => "settings.ai_commit.missing.copilot",
        AiSource::Env => "settings.ai_commit.missing.env",
        // CLI sources report through check_availability instead.
        _ => "settings.ai_commit.missing.manual",
    }
}

/// The `%{detail}` for an unavailable source: the file path that was missing,
/// when naming one helps.
pub(crate) fn unavailable_detail(source: AiSource, env: &EnvAccess) -> Option<String> {
    match source {
        AiSource::ClaudeCode => Some(
            env.home
                .join(".claude")
                .join("settings.json")
                .display()
                .to_string(),
        ),
        AiSource::Codex => Some(
            env.home
                .join(".codex")
                .join("auth.json")
                .display()
                .to_string(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_home(home: &std::path::Path) -> EnvAccess {
        EnvAccess::synthetic(home.to_path_buf(), HashMap::new())
    }

    fn env_with_vars(home: &std::path::Path, vars: &[(&str, &str)]) -> EnvAccess {
        EnvAccess::synthetic(
            home.to_path_buf(),
            vars.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }

    fn manual_settings(api_key: &str) -> AiCommitSettings {
        AiCommitSettings {
            provider: AiProvider::Anthropic,
            api_key: api_key.to_string(),
            ..AiCommitSettings::default()
        }
    }

    #[test]
    fn source_keys_round_trip() {
        for source in AiSource::ALL {
            assert_eq!(AiSource::from_key(source.key()), Some(*source));
        }
        assert_eq!(AiSource::from_key("nonsense"), None);
        // Old sessions with no stored source land on manual.
        assert_eq!(
            AiSource::from_key("").or(Some(AiSource::Manual)),
            Some(AiSource::Manual)
        );
    }

    #[test]
    fn manual_resolves_only_with_a_key() {
        let env = env_with_home(Path::new("/nonexistent"));
        assert!(resolve_http_settings(AiSource::Manual, &manual_settings("sk-x"), &env).is_some());
        assert!(
            resolve_http_settings(AiSource::Manual, &manual_settings("   "), &env).is_none(),
            "a blank key is no configuration"
        );
    }

    #[test]
    fn claude_code_reads_settings_json_then_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".claude").join("settings.json"),
            r#"{"env": {"ANTHROPIC_AUTH_TOKEN": "tok-file", "ANTHROPIC_BASE_URL": "https://relay.example.com"}}"#,
        )
        .unwrap();

        let env = env_with_home(dir.path());
        let settings =
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .expect("settings.json credentials resolve");
        assert_eq!(settings.provider, AiProvider::Anthropic);
        assert_eq!(settings.api_key, "tok-file");
        assert!(settings.bearer_auth, "an auth token is a bearer credential");
        assert_eq!(settings.endpoint, "https://relay.example.com");

        // Without the file, the environment variable takes over.
        std::fs::remove_file(dir.path().join(".claude").join("settings.json")).unwrap();
        let env = env_with_vars(dir.path(), &[("ANTHROPIC_AUTH_TOKEN", "tok-env")]);
        let settings =
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .expect("env credentials resolve");
        assert_eq!(settings.api_key, "tok-env");
        assert_eq!(settings.model, "");

        // A plain API key still resolves, as an x-api-key credential.
        let env = env_with_vars(dir.path(), &[("ANTHROPIC_API_KEY", "sk-env")]);
        let settings =
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .unwrap();
        assert_eq!(settings.api_key, "sk-env");
        assert!(!settings.bearer_auth);

        // Both set → the auth token wins.
        let env = env_with_vars(
            dir.path(),
            &[
                ("ANTHROPIC_AUTH_TOKEN", "tok-both"),
                ("ANTHROPIC_API_KEY", "sk-both"),
            ],
        );
        let settings =
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .unwrap();
        assert_eq!(settings.api_key, "tok-both");
        assert!(settings.bearer_auth);

        // Neither present → unavailable.
        let env = env_with_home(dir.path());
        assert!(
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .is_none()
        );

        // Malformed JSON falls back to the environment rather than failing.
        std::fs::write(
            dir.path().join(".claude").join("settings.json"),
            "{not json",
        )
        .unwrap();
        let env = env_with_vars(dir.path(), &[("ANTHROPIC_AUTH_TOKEN", "tok-env2")]);
        let settings =
            resolve_http_settings(AiSource::ClaudeCode, &AiCommitSettings::default(), &env)
                .expect("malformed settings.json falls back to env");
        assert_eq!(settings.api_key, "tok-env2");
    }

    #[test]
    fn codex_reads_auth_json_and_config_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex").join("auth.json"),
            r#"{"OPENAI_API_KEY": "sk-codex"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".codex").join("config.toml"),
            r#"
# codex config
model = "gpt-5.2-codex"
model_provider = "openrouter"

[model_providers.openrouter]
base_url = "https://openrouter.example.com/v1"

[model_providers.unused]
base_url = "https://unused.example.com"
"#,
        )
        .unwrap();

        let env = env_with_home(dir.path());
        let settings = resolve_http_settings(AiSource::Codex, &AiCommitSettings::default(), &env)
            .expect("codex credentials resolve");
        assert_eq!(settings.provider, AiProvider::OpenAiCompatible);
        assert_eq!(settings.api_key, "sk-codex");
        assert_eq!(settings.model, "gpt-5.2-codex");
        assert_eq!(settings.endpoint, "https://openrouter.example.com/v1");

        // A provider without a matching table leaves the endpoint empty
        // (the OpenAI default then applies downstream).
        std::fs::write(
            dir.path().join(".codex").join("config.toml"),
            "model_provider = \"missing\"\n",
        )
        .unwrap();
        let settings =
            resolve_http_settings(AiSource::Codex, &AiCommitSettings::default(), &env).unwrap();
        assert_eq!(settings.endpoint, "");
        assert_eq!(settings.model, "");

        // No auth.json anywhere → env fallback, then unavailable.
        std::fs::remove_file(dir.path().join(".codex").join("auth.json")).unwrap();
        let env = env_with_vars(dir.path(), &[("OPENAI_API_KEY", "sk-env")]);
        let settings =
            resolve_http_settings(AiSource::Codex, &AiCommitSettings::default(), &env).unwrap();
        assert_eq!(settings.api_key, "sk-env");

        let env = env_with_home(dir.path());
        assert!(
            resolve_http_settings(AiSource::Codex, &AiCommitSettings::default(), &env).is_none()
        );
    }

    #[test]
    fn mini_toml_handles_quotes_and_comments() {
        let tables = parse_mini_toml(
            "top = \"a # not a comment\"\nbare = plain # comment\nc = \"unterminated\n[d]\ne = \"f\"",
        );
        assert_eq!(tables[""]["top"], "a # not a comment");
        assert_eq!(tables[""]["bare"], "plain");
        // An unterminated quote keeps the raw text rather than guessing.
        assert_eq!(tables[""]["c"], "\"unterminated");
        assert_eq!(tables["d"]["e"], "f");
    }

    #[test]
    fn copilot_reads_hosts_yyyml_then_env_tokens() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".config").join("gh")).unwrap();
        std::fs::write(
            dir.path().join(".config").join("gh").join("hosts.yml"),
            "github.com:\n    user: alice\n    oauth_token: gho_abc\n    git_protocol: ssh\n",
        )
        .unwrap();

        let env = env_with_home(dir.path());
        let settings = resolve_http_settings(AiSource::Copilot, &AiCommitSettings::default(), &env)
            .expect("hosts.yml token resolves");
        assert_eq!(settings.provider, AiProvider::OpenAiCompatible);
        assert_eq!(settings.api_key, "gho_abc");
        assert_eq!(settings.endpoint, "https://models.github.ai/inference");
        assert_eq!(settings.model, "gpt-4o");

        std::fs::remove_file(dir.path().join(".config").join("gh").join("hosts.yml")).unwrap();
        let env = env_with_vars(dir.path(), &[("GITHUB_TOKEN", "ghp_env"), ("GH_TOKEN", "")]);
        // GH_TOKEN set but blank is skipped in favor of GITHUB_TOKEN.
        let settings =
            resolve_http_settings(AiSource::Copilot, &AiCommitSettings::default(), &env).unwrap();
        assert_eq!(settings.api_key, "ghp_env");

        let env = env_with_home(dir.path());
        assert!(
            resolve_http_settings(AiSource::Copilot, &AiCommitSettings::default(), &env).is_none()
        );
    }

    #[test]
    fn gh_cli_fallback_covers_keyring_stored_tokens() {
        // gh's default token storage is the OS credential store, so hosts.yml
        // carries the login but no `oauth_token:` line — exactly what a
        // signed-in Windows user has. The CLI seam is the only reader left.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".config").join("gh")).unwrap();
        std::fs::write(
            dir.path().join(".config").join("gh").join("hosts.yml"),
            "github.com:\n    git_protocol: https\n    user: alice\n",
        )
        .unwrap();

        let mut env = env_with_home(dir.path());
        env.stage_gh_cli_token(Some("gho_keyring"));
        assert_eq!(read_gh_token(&env).as_deref(), Some("gho_keyring"));

        // A CLI that answers nothing still resolves as signed out.
        env.stage_gh_cli_token(None);
        assert_eq!(read_gh_token(&env), None);
    }

    #[test]
    fn gh_cli_fallback_runs_after_hosts_yml_and_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".config").join("gh")).unwrap();
        std::fs::write(
            dir.path().join(".config").join("gh").join("hosts.yml"),
            "github.com:\n    oauth_token: gho_file\n",
        )
        .unwrap();

        let mut env = env_with_home(dir.path());
        env.stage_gh_cli_token(Some("gho_keyring"));
        assert_eq!(read_gh_token(&env).as_deref(), Some("gho_file"));

        std::fs::remove_file(dir.path().join(".config").join("gh").join("hosts.yml")).unwrap();
        let mut env = env_with_vars(dir.path(), &[("GH_TOKEN", "ghp_env")]);
        env.stage_gh_cli_token(Some("gho_keyring"));
        assert_eq!(read_gh_token(&env).as_deref(), Some("ghp_env"));
    }

    /// Reads this machine's real gh login — run explicitly with
    /// `cargo test -p worktree-ui-gpui real_env -- --ignored` to check the
    /// keyring fallback against an actually signed-in CLI. On a machine
    /// without gh signed in there is nothing to assert and it passes.
    #[test]
    #[ignore = "reads this machine's real gh login"]
    fn real_env_resolves_the_signed_in_gh_token() {
        let cli = std::process::Command::new("gh")
            .args(["auth", "token"])
            .output()
            .expect("gh should be installed to run this test");
        if !cli.status.success() {
            return;
        }
        let expected = String::from_utf8_lossy(&cli.stdout).trim().to_string();
        assert_eq!(
            read_gh_token(&EnvAccess::real()),
            Some(expected),
            "the resolver must agree with what gh itself prints"
        );
    }

    #[test]
    fn env_source_prefers_anthropic_then_openai() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_with_vars(
            dir.path(),
            &[
                ("ANTHROPIC_API_KEY", "sk-ant"),
                ("OPENAI_API_KEY", "sk-oai"),
                ("ANTHROPIC_BASE_URL", "https://relay.example.com"),
            ],
        );
        let settings =
            resolve_http_settings(AiSource::Env, &AiCommitSettings::default(), &env).unwrap();
        assert_eq!(settings.provider, AiProvider::Anthropic);
        assert_eq!(settings.api_key, "sk-ant");
        assert_eq!(settings.endpoint, "https://relay.example.com");

        let env = env_with_vars(
            dir.path(),
            &[
                ("OPENAI_API_KEY", "sk-oai"),
                ("OPENAI_MODEL", "gpt-4.1-mini"),
            ],
        );
        let settings =
            resolve_http_settings(AiSource::Env, &AiCommitSettings::default(), &env).unwrap();
        assert_eq!(settings.provider, AiProvider::OpenAiCompatible);
        assert_eq!(settings.model, "gpt-4.1-mini");

        let env = env_with_home(dir.path());
        assert!(resolve_http_settings(AiSource::Env, &AiCommitSettings::default(), &env).is_none());
    }

    #[test]
    fn cli_sources_never_resolve_over_http() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_with_home(dir.path());
        for source in [
            AiSource::CliClaude,
            AiSource::CliCodex,
            AiSource::CliGemini,
            AiSource::CliOllama,
            AiSource::Custom,
        ] {
            assert!(resolve_http_settings(source, &AiCommitSettings::default(), &env).is_none());
        }
    }

    #[test]
    fn cli_specs_cover_the_four_builtins() {
        let claude = cli_spec(AiSource::CliClaude).unwrap();
        assert_eq!(claude.executable, "claude");
        assert_eq!(claude.args_template, "-p {PROMPT}");
        assert_eq!(
            cli_spec(AiSource::CliOllama).unwrap().default_model,
            "llama3"
        );
        assert!(cli_spec(AiSource::Manual).is_none());
    }

    #[test]
    fn build_cli_args_substitutes_placeholders_as_single_arguments() {
        let args = build_cli_args("run {MODEL} {PROMPT}", "the prompt", "llama3");
        assert_eq!(args, vec!["run", "llama3", "the prompt"]);

        let args = build_cli_args("-p {PROMPT}", "multi\nline prompt", "");
        assert_eq!(args, vec!["-p", "multi\nline prompt"]);
    }

    #[test]
    fn build_cli_args_appends_the_prompt_when_the_template_omits_it() {
        let args = build_cli_args("--quiet --json", "the prompt", "");
        assert_eq!(args, vec!["--quiet", "--json", "the prompt"]);
    }

    #[test]
    fn custom_cli_spec_splits_program_from_template() {
        assert_eq!(
            custom_cli_spec("  my-tool  --flag {PROMPT}  "),
            Some(("my-tool".to_string(), "--flag {PROMPT}".to_string()))
        );
        assert_eq!(custom_cli_spec(""), None);
        assert_eq!(custom_cli_spec("   "), None);
    }

    #[test]
    fn find_executable_uses_path_and_accepts_verbatim_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("fake-ai"), b"#!/bin/sh\n").unwrap();

        let path_var = dir.path().display().to_string();
        let env = env_with_vars(Path::new("/nonexistent"), &[("PATH", &path_var)]);
        let found = find_executable("fake-ai", &env).expect("found on PATH");
        assert_eq!(found, dir.path().join("fake-ai"));
        assert!(find_executable("absent-ai", &env).is_none());

        let verbatim = dir.path().join("fake-ai");
        let env = env_with_home(Path::new("/nonexistent"));
        assert_eq!(
            find_executable(&verbatim.display().to_string(), &env),
            Some(verbatim)
        );
    }

    #[test]
    fn availability_reports_cli_and_credential_sources() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("claude"), b"#!/bin/sh\n").unwrap();
        let path_var = dir.path().display().to_string();

        let env = env_with_vars(Path::new("/nonexistent"), &[("PATH", &path_var)]);
        let availability =
            check_availability(AiSource::CliClaude, &AiCommitSettings::default(), "", &env);
        assert!(availability.detected);

        let env = env_with_vars(Path::new("/nonexistent"), &[("PATH", "/empty")]);
        let availability =
            check_availability(AiSource::CliClaude, &AiCommitSettings::default(), "", &env);
        assert!(!availability.detected);
        assert_eq!(
            availability.message,
            Some(("settings.ai_commit.missing.cli", Some("claude".to_string())))
        );

        let env = env_with_home(Path::new("/nonexistent"));
        let availability =
            check_availability(AiSource::Custom, &AiCommitSettings::default(), "", &env);
        assert!(!availability.detected);
        assert_eq!(
            availability.message.unwrap().0,
            "settings.ai_commit.missing.custom_empty"
        );

        let availability = check_availability(AiSource::Manual, &manual_settings("sk-x"), "", &env);
        assert!(availability.detected);
    }
}
