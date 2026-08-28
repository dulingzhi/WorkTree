//! AI-generated commit messages — and hunk explanations.
//!
//! The ✨ button next to the commit box asks a language model to draft the
//! commit message from the staged diff; the diff view's "Explain this
//! change" asks one to explain a single hunk. Access comes from a *source*
//! (`ai_commit_sources`): the manual provider fields (Anthropic's Messages
//! API and any OpenAI-compatible chat-completions endpoint), credentials
//! another tool already stored (Claude Code, Codex, GitHub CLI, environment
//! variables), or a local CLI (`claude` / `codex` / `gemini` / `ollama` / a
//! custom command).
//!
//! The commit prompt pairs the diff with the repository's recent commit
//! subjects so the answer matches the project's existing type, scope, and
//! language; the explanation prompt asks for prose in the UI locale instead.
//! Either reply is sanitized (code fences and wrapping quotes stripped)
//! before it lands. The commit half mirrors RepositoryTree's
//! `CommitPromptBuilder`/`CommitMessageSanitizer` design.
//!
//! Provider settings live in a process-global seeded from the session file,
//! following `avatar_source`'s shape: the commit-box render path has no
//! interest in settings-window entities and only needs to know whether the
//! feature is configured.

use repositorytree_state::session;
use gpui::SharedString;
use std::sync::{LazyLock, RwLock};

/// Ceiling on the diff text sent to the model; longer diffs are truncated.
pub(crate) const MAX_DIFF_LENGTH: usize = 4000;

/// Output budget for one reply. Reasoning models spend tokens on thinking
/// before the visible text — a tight ceiling can be exhausted by the reasoning
/// alone and come back with no message at all (observed with glm-5.3 at 100) —
/// so this is generous for a ≤72-character answer.
pub(crate) const MAX_OUTPUT_TOKENS: u64 = 1024;

/// The system prompt, ported from RepositoryTree's `CommitPromptBuilder`.
pub(crate) const SYSTEM_PROMPT: &str = "You are a git commit message generator. Given a diff, write a concise conventional commit message (type: description). \
First line max 72 chars. If the diff is large, focus on the most significant changes. \
If recent commit messages are provided, match their type, scope, punctuation, and language exactly. \
Reply with ONLY the commit message, no explanation.";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum AiProvider {
    #[default]
    Anthropic,
    OpenAiCompatible,
}

impl AiProvider {
    pub(crate) const ALL: &'static [AiProvider] =
        &[AiProvider::Anthropic, AiProvider::OpenAiCompatible];

    /// Stable key persisted in `session.json`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai",
        }
    }

    pub(crate) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }

    /// Label shown in the settings dropdown.
    pub(crate) fn label(self) -> SharedString {
        match self {
            Self::Anthropic => crate::i18n::tr("settings.ai_commit.provider_anthropic"),
            Self::OpenAiCompatible => crate::i18n::tr("settings.ai_commit.provider_openai"),
        }
    }

    pub(crate) fn default_endpoint(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAiCompatible => "https://api.openai.com",
        }
    }

    pub(crate) fn default_model(self) -> &'static str {
        match self {
            Self::Anthropic => "claude-sonnet-4-20250514",
            Self::OpenAiCompatible => "gpt-4o-mini",
        }
    }
}

/// The effective AI configuration. The manual fields are the raw persisted
/// strings; empty means "fall back to the provider default". The other
/// sources resolve their credentials at generation time and persist nothing
/// but the choice of source.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AiCommitSettings {
    pub(crate) source: crate::ai_commit_sources::AiSource,
    pub(crate) provider: AiProvider,
    pub(crate) api_key: String,
    pub(crate) model: String,
    pub(crate) endpoint: String,
    pub(crate) custom_command: String,
}

impl AiCommitSettings {
    /// Whether the ✨ button should be armed. Manual needs its API key; every
    /// other source is checked when generation actually runs — files and PATH
    /// entries can change between render and click, and a failure surfaces as
    /// a warning naming the source.
    pub(crate) fn is_configured(&self) -> bool {
        match self.source {
            crate::ai_commit_sources::AiSource::Manual => !self.api_key.trim().is_empty(),
            _ => true,
        }
    }

    pub(crate) fn effective_model(&self) -> &str {
        let model = self.model.trim();
        if model.is_empty() {
            self.provider.default_model()
        } else {
            model
        }
    }

    /// The trimmed base URL every provider path is built on: the configured
    /// endpoint, or the provider's default when it is blank.
    fn base_url(&self) -> &str {
        let endpoint = self.endpoint.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            self.provider.default_endpoint()
        } else {
            endpoint
        }
    }

    /// The full request URL for the provider's chat endpoint.
    ///
    /// Relays hand out base URLs in three shapes — bare host, host ending in
    /// `/v1`, or the complete chat path — and appending `/v1/…` blindly turns
    /// the second into `/v1/v1/…`, which the server answers with 404. Each
    /// shape is met where it is.
    pub(crate) fn endpoint_url(&self) -> String {
        let base = self.base_url();
        let chat_path = match self.provider {
            AiProvider::Anthropic => "/v1/messages",
            AiProvider::OpenAiCompatible => "/v1/chat/completions",
        };
        if base.ends_with(chat_path) {
            return base.to_string();
        }
        if base.ends_with("/v1") {
            return format!("{base}{}", chat_path.trim_start_matches("/v1"));
        }
        format!("{base}{chat_path}")
    }

    /// Provider-appropriate authentication headers, shared by the chat
    /// request and the model listing.
    pub(crate) fn auth_headers(&self) -> Vec<(&'static str, String)> {
        match self.provider {
            AiProvider::Anthropic => vec![
                ("x-api-key", self.api_key.trim().to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            AiProvider::OpenAiCompatible => {
                vec![("Authorization", format!("Bearer {}", self.api_key.trim()))]
            }
        }
    }

    /// Candidate URLs for the model listing, tried in order. The versioned
    /// path is what the real providers serve (`api.openai.com/v1/models`,
    /// `api.anthropic.com/v1/models`); the unversioned one catches relays
    /// that mount `/models` at the root.
    pub(crate) fn models_urls(&self) -> Vec<String> {
        let base = self.base_url();
        let mut urls = Vec::new();
        if !base.ends_with("/v1") {
            urls.push(format!("{base}/v1/models"));
        }
        urls.push(format!("{base}/models"));
        urls
    }
}

static CURRENT: LazyLock<RwLock<AiCommitSettings>> =
    LazyLock::new(|| RwLock::new(AiCommitSettings::default()));

/// The active settings. Clones on read — this is only consulted on button
/// render and click, not per-frame.
pub(crate) fn current() -> AiCommitSettings {
    CURRENT.read().expect("ai commit settings lock").clone()
}

pub(crate) fn set_current(settings: AiCommitSettings) {
    *CURRENT.write().expect("ai commit settings lock") = settings;
}

/// Seed the global from the persisted session. Safe to call from every
/// window — settings changes overwrite it later.
pub(crate) fn init_from_session(ui_session: &session::UiSession) {
    let source = ui_session
        .ai_commit_source
        .as_deref()
        .and_then(crate::ai_commit_sources::AiSource::from_key)
        .unwrap_or_default();
    let provider = ui_session
        .ai_commit_provider
        .as_deref()
        .and_then(AiProvider::from_key)
        .unwrap_or_default();
    set_current(AiCommitSettings {
        source,
        provider,
        api_key: ui_session.ai_commit_api_key.clone().unwrap_or_default(),
        model: ui_session.ai_commit_model.clone().unwrap_or_default(),
        endpoint: ui_session.ai_commit_endpoint.clone().unwrap_or_default(),
        custom_command: ui_session
            .ai_commit_custom_command
            .clone()
            .unwrap_or_default(),
    });
}

/// Serializes tests that touch the process-global [`CURRENT`]. Tests run in
/// parallel within the crate, so without this one test's configured key leaks
/// into another's unconfigured assertion. Recover from poison: tests that use
/// it restore the default on drop, so a panic must not cascade.
#[cfg(test)]
pub(crate) static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_settings() -> std::sync::MutexGuard<'static, ()> {
    TEST_SETTINGS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Truncate an over-long diff, marking the cut so the model knows there was
/// more. Splits on a char boundary so the result stays valid UTF-8.
pub(crate) fn truncate_diff(diff: &str) -> std::borrow::Cow<'_, str> {
    if diff.len() <= MAX_DIFF_LENGTH {
        return std::borrow::Cow::Borrowed(diff);
    }
    let mut cut = MAX_DIFF_LENGTH;
    while !diff.is_char_boundary(cut) {
        cut -= 1;
    }
    std::borrow::Cow::Owned(format!("{}\n[truncated]", &diff[..cut]))
}

/// Assemble the user content: recent subjects first (so the model matches
/// their format and language), then the diff. The examples block sits outside
/// the diff truncation budget, as in the reference implementation.
pub(crate) fn build_user_content(diff: &str, recent_subjects: &[String]) -> String {
    let truncated = truncate_diff(diff);
    let examples: Vec<&str> = recent_subjects
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if examples.is_empty() {
        return truncated.to_string();
    }
    format!(
        "Recent commit messages from this repository (match their format, scope, punctuation, and language):\n{}\n\n--- Diff ---\n{}",
        examples.join("\n"),
        truncated,
    )
}

/// The single prompt text handed to a CLI generator: system prompt and user
/// content joined, so CLI and HTTP generations share one prompt shape.
// Only the `cfg(not(test))` generators call this; test builds stub them out.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn build_cli_prompt(diff: &str, recent_subjects: &[String]) -> String {
    format!(
        "{}\n\n{}",
        SYSTEM_PROMPT,
        build_user_content(diff, recent_subjects)
    )
}

/// The system prompt for the diff view's "Explain this change" action — the
/// same providers as the commit ✨, a different job. The patch already says
/// *what* changed line by line; the answer is for the reader who wants the
/// change summarized and its intent drawn out.
pub(crate) const EXPLAIN_SYSTEM_PROMPT: &str = "You are a senior engineer explaining a git change to a colleague. \
Given a unified diff patch, explain what the change does and, where it can be inferred, why — the intent behind it. \
Be concise: a few short paragraphs or bullets at most. Do not restate the patch line by line. \
Reply with ONLY the explanation.";

/// Assemble the explanation user content: the locale the answer should be
/// written in first (models default to English otherwise), then the patch
/// under the same truncation budget as commit diffs.
pub(crate) fn build_explanation_user_content(patch: &str, locale: &str) -> String {
    format!(
        "Explain this change. Write the explanation in the language of this locale: {locale}.\n\n--- Patch ---\n{}",
        truncate_diff(patch),
    )
}

/// The single prompt text handed to a CLI generator, mirroring
/// [`build_cli_prompt`]'s shape.
pub(crate) fn build_explanation_cli_prompt(patch: &str, locale: &str) -> String {
    format!(
        "{}\n\n{}",
        EXPLAIN_SYSTEM_PROMPT,
        build_explanation_user_content(patch, locale)
    )
}

/// Build the provider-specific request for one explanation.
pub(crate) fn build_explanation_request(
    settings: &AiCommitSettings,
    patch: &str,
    locale: &str,
) -> AiCommitRequest {
    let user_content = build_explanation_user_content(patch, locale);
    let model = settings.effective_model();
    match settings.provider {
        AiProvider::Anthropic => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "max_tokens": MAX_OUTPUT_TOKENS,
                "system": EXPLAIN_SYSTEM_PROMPT,
                "messages": [{ "role": "user", "content": user_content }],
            })
            .to_string(),
        },
        AiProvider::OpenAiCompatible => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": EXPLAIN_SYSTEM_PROMPT },
                    { "role": "user", "content": user_content },
                ],
                "max_tokens": MAX_OUTPUT_TOKENS,
                "temperature": 0.3,
            })
            .to_string(),
        },
    }
}

/// How long a CLI generator may run before it is killed.
// Only the `cfg(not(test))` CLI runner reads this; test builds stub it out.
#[cfg_attr(test, allow(dead_code))]
const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// The system prompt for the MR push prompt's "generate description"
/// action — the same providers as the commit ✨, a third job. The commits
/// and diffstat say what changed; the description is for the reviewer
/// opening the merge request.
pub(crate) const MR_DESCRIPTION_SYSTEM_PROMPT: &str = "You are writing the description for a merge request (GitLab) or pull request (GitHub). \
Given the target branch, the commits between it and HEAD, and a diffstat, write the description in markdown: \
a short opening paragraph summarizing the change, then a '## Changes' section of grouped bullets, \
and a brief '## Testing' section only when the changes imply one. \
Ground every claim in the provided commits and diffstat; do not invent details. \
Reply with ONLY the markdown description.";

/// Assemble the MR description user content: the target, the locale the
/// answer should be written in first, the commits, and the diffstat under
/// the same truncation budget as commit diffs.
pub(crate) fn build_mr_description_user_content(
    target: &str,
    commits: &[(String, String)],
    diff_stat: &str,
    locale: &str,
) -> String {
    let commit_lines = commits
        .iter()
        .map(|(sha, subject)| format!("- {sha} {subject}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Write the merge request description for HEAD merging into \"{target}\". \
Write it in the language of this locale: {locale}.\n\n--- Commits ({}) ---\n{commit_lines}\n\n--- Diffstat vs {target} ---\n{}",
        commits.len(),
        truncate_diff(diff_stat),
    )
}

/// The single prompt text handed to a CLI generator, mirroring
/// [`build_cli_prompt`]'s shape.
pub(crate) fn build_mr_description_cli_prompt(
    target: &str,
    commits: &[(String, String)],
    diff_stat: &str,
    locale: &str,
) -> String {
    format!(
        "{}\n\n{}",
        MR_DESCRIPTION_SYSTEM_PROMPT,
        build_mr_description_user_content(target, commits, diff_stat, locale)
    )
}

/// Build the provider-specific request for one MR description.
pub(crate) fn build_mr_description_request(
    settings: &AiCommitSettings,
    target: &str,
    commits: &[(String, String)],
    diff_stat: &str,
    locale: &str,
) -> AiCommitRequest {
    let user_content = build_mr_description_user_content(target, commits, diff_stat, locale);
    let model = settings.effective_model();
    match settings.provider {
        AiProvider::Anthropic => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "max_tokens": MAX_OUTPUT_TOKENS,
                "system": MR_DESCRIPTION_SYSTEM_PROMPT,
                "messages": [{ "role": "user", "content": user_content }],
            })
            .to_string(),
        },
        AiProvider::OpenAiCompatible => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": MR_DESCRIPTION_SYSTEM_PROMPT },
                    { "role": "user", "content": user_content },
                ],
                "max_tokens": MAX_OUTPUT_TOKENS,
                "temperature": 0.3,
            })
            .to_string(),
        },
    }
}

/// Run one CLI generation: spawn, collect stdout/stderr, kill on timeout,
/// and sanitize stdout as the reply. The prompt arrives as a single argv
/// element — no shell is involved, so its contents need no escaping.
///
/// `Command::output` owns the child for the whole run; `kill_on_drop` is what
/// stops it on timeout, because the timeout arm drops that future (borrowing
/// the child for an explicit `kill` does not convince the borrow checker).
#[cfg(not(test))]
pub(crate) async fn generate_via_cli(
    executable: &str,
    args: &[String],
) -> Result<String, String> {
    let mut command = smol::process::Command::new(executable);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let run = command.output();

    let outcome =
        futures::future::select(Box::pin(run), Box::pin(smol::Timer::after(CLI_TIMEOUT)))
            .await;
    let output = match outcome {
        futures::future::Either::Left((output, _timer)) => output
            .map_err(|err| format!("could not run `{executable}`: {err}"))?,
        futures::future::Either::Right((_expired, run)) => {
            // Dropping the run future kills the child (kill_on_drop).
            drop(run);
            return Err(format!(
                "`{executable}` timed out after {}s and was stopped",
                CLI_TIMEOUT.as_secs()
            ));
        }
    };

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        let detail = if detail.is_empty() {
            format!("exit code {}", output.status.code().unwrap_or(-1))
        } else {
            detail.to_string()
        };
        return Err(format!("`{executable}` failed: {detail}"));
    }
    let cleaned = sanitize(&String::from_utf8_lossy(&output.stdout));
    if cleaned.is_empty() {
        return Err(format!("`{executable}` returned an empty message"));
    }
    Ok(cleaned)
}

/// Strip the wrapping an LLM tends to add: markdown code fences and paired
/// quotes/backticks. Ported from RepositoryTree's `CommitMessageSanitizer`.
pub(crate) fn sanitize(raw: &str) -> String {
    let mut s = raw.trim();

    // ```lang\n ... \n``` — keep the inside; ```...``` on one line too.
    if let Some(rest) = s.strip_prefix("```") {
        if let Some(nl) = rest.find('\n') {
            let after = &rest[nl + 1..];
            if let Some(close) = after.rfind("```") {
                s = after[..close].trim();
            }
        } else if rest.len() > 3 && rest.ends_with("```") {
            s = rest[..rest.len() - 3].trim();
        }
    }

    let first = s.chars().next();
    let last = s.chars().next_back();
    if s.chars().count() >= 2
        && first == last
        && matches!(first, Some('"') | Some('\'') | Some('`'))
    {
        s = s[1..s.len() - 1].trim();
    }

    s.to_string()
}

/// A fully-built provider request: URL, headers, and the serialized JSON
/// body. Pure data so tests can assert on it without networking.
pub(crate) struct AiCommitRequest {
    pub(crate) url: String,
    pub(crate) headers: Vec<(&'static str, String)>,
    pub(crate) body: String,
}

/// Build the provider-specific request for one generation.
pub(crate) fn build_request(
    settings: &AiCommitSettings,
    diff: &str,
    recent_subjects: &[String],
) -> AiCommitRequest {
    let user_content = build_user_content(diff, recent_subjects);
    let model = settings.effective_model();
    match settings.provider {
        AiProvider::Anthropic => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "max_tokens": MAX_OUTPUT_TOKENS,
                "system": SYSTEM_PROMPT,
                "messages": [{ "role": "user", "content": user_content }],
            })
            .to_string(),
        },
        AiProvider::OpenAiCompatible => AiCommitRequest {
            url: settings.endpoint_url(),
            headers: settings.auth_headers(),
            body: serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": SYSTEM_PROMPT },
                    { "role": "user", "content": user_content },
                ],
                "max_tokens": MAX_OUTPUT_TOKENS,
                "temperature": 0.3,
            })
            .to_string(),
        },
    }
}

/// Pull the assistant's text out of a provider response body. Non-success
/// statuses surface the server's own error message when there is one.
pub(crate) fn parse_response(
    provider: AiProvider,
    status: u16,
    body: &[u8],
) -> Result<String, String> {
    if !(200..300).contains(&status) {
        return Err(error_detail(status, body));
    }

    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid response JSON: {err}"))?;

    let text = match provider {
        AiProvider::Anthropic => anthropic_text(&json),
        AiProvider::OpenAiCompatible => json
            .pointer("/choices/0/message/content")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }
    .unwrap_or_default();

    let cleaned = sanitize(&text);
    if cleaned.is_empty() {
        return Err("the model returned an empty message".to_string());
    }
    Ok(cleaned)
}

/// Anthropic replies are a list of content blocks, and reasoning models put a
/// `thinking` block ahead of the text — collect every text block rather than
/// reading the first element, which would miss the reply entirely.
fn anthropic_text(json: &serde_json::Value) -> Option<String> {
    let blocks = json.pointer("/content")?.as_array()?;
    let texts: Vec<&str> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Render a non-success body for the user: JSON `{"error": {"message": …}}`
/// (both providers) or Anthropic's bare top-level `message` when present,
/// otherwise the raw body text, otherwise just the status. Error bodies are
/// often not JSON at all — a wrong path on a plain relay answers 404 with
/// plain text or nothing.
fn error_detail(status: u16, body: &[u8]) -> String {
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) {
        let detail = json
            .pointer("/error/message")
            .or_else(|| json.pointer("/message"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        return match detail {
            Some(detail) => format!("HTTP {status}: {detail}"),
            None => format!("HTTP {status}"),
        };
    }
    match std::str::from_utf8(body).map(str::trim) {
        Ok(text) if !text.is_empty() => format!("HTTP {status}: {text}"),
        _ => format!("HTTP {status}"),
    }
}

/// Run one generation end to end, dispatching on the configured source:
/// CLI sources shell out locally, every other source resolves HTTP settings
/// (live, never persisted) and posts. Network/process-disabled in tests; the
/// pure halves (`build_request`, `parse_response`, `build_cli_args`) carry
/// the coverage instead.
#[cfg(not(test))]
pub(crate) async fn generate(
    settings: &AiCommitSettings,
    diff: &str,
    recent_subjects: &[String],
) -> Result<String, String> {
    generate_from_source(settings, build_cli_prompt(diff, recent_subjects), |resolved| {
        build_request(resolved, diff, recent_subjects)
    })
    .await
}

/// One hunk explanation, over the same source dispatch. The reply shares
/// the commit path's sanitization and empty-reply rejection.
#[cfg(not(test))]
pub(crate) async fn generate_explanation(
    settings: &AiCommitSettings,
    patch: &str,
    locale: &str,
) -> Result<String, String> {
    generate_from_source(
        settings,
        build_explanation_cli_prompt(patch, locale),
        |resolved| build_explanation_request(resolved, patch, locale),
    )
    .await
}

/// One MR/PR description, over the same source dispatch. The reply shares
/// the commit path's sanitization and empty-reply rejection; it lands in an
/// editable field, not a commit message, so markdown survives.
#[cfg(not(test))]
pub(crate) async fn generate_mr_description(
    settings: &AiCommitSettings,
    target: &str,
    commits: &[(String, String)],
    diff_stat: &str,
    locale: &str,
) -> Result<String, String> {
    generate_from_source(
        settings,
        build_mr_description_cli_prompt(target, commits, diff_stat, locale),
        |resolved| {
            build_mr_description_request(resolved, target, commits, diff_stat, locale)
        },
    )
    .await
}

/// The source dispatch shared by every prompt: `cli_prompt` is the single
/// argv element a CLI generator receives (no shell, so no escaping);
/// `http_request` builds the provider request from the resolved HTTP
/// settings once credential resolution has succeeded.
#[cfg(not(test))]
async fn generate_from_source(
    settings: &AiCommitSettings,
    cli_prompt: String,
    http_request: impl FnOnce(&AiCommitSettings) -> AiCommitRequest,
) -> Result<String, String> {
    use crate::ai_commit_sources::{
        build_cli_args, cli_spec, custom_cli_spec, resolve_http_settings, AiSource,
        EnvAccess,
    };

    let source = settings.source;
    if source.is_cli() {
        let (executable, template) = match source {
            AiSource::Custom => custom_cli_spec(&settings.custom_command).ok_or_else(|| {
                "the custom command is empty — configure it in settings".to_string()
            })?,
            other => {
                let spec = cli_spec(other).expect("cli sources have a built-in spec");
                (spec.executable.to_string(), spec.args_template.to_string())
            }
        };
        let model = cli_spec(source)
            .map(|spec| spec.default_model.to_string())
            .unwrap_or_default();
        let args = build_cli_args(&template, &cli_prompt, &model);
        return generate_via_cli(&executable, &args).await;
    }

    // Resolution reads config files — keep that off the async executor.
    let manual = settings.clone();
    let resolved = smol::unblock(move || {
        resolve_http_settings(source, &manual, &EnvAccess::real())
    })
    .await
    .ok_or_else(|| source_unavailable_message(source))?;

    let request = http_request(&resolved);
    let response = crate::http::post_json(
        request.url,
        request.headers,
        request.body,
        crate::http::AI_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| err.to_string())?;
    parse_response(resolved.provider, response.status.into(), &response.body)
}

/// The toast text when a source resolves to nothing at click time. Localized
/// here (main thread) rather than inside the background resolution.
#[cfg(not(test))]
fn source_unavailable_message(source: crate::ai_commit_sources::AiSource) -> String {
    use crate::ai_commit_sources::{unavailable_detail, unavailable_key, EnvAccess};
    let env = EnvAccess::real();
    let key = unavailable_key(source);
    match unavailable_detail(source, &env) {
        Some(detail) => crate::i18n::t!(key, detail = detail).to_string(),
        None => crate::i18n::t!(key).to_string(),
    }
}

/// Pull the model ids out of a model-listing response body. Both providers
/// answer `{"data": [{"id": …}, …]}` (OpenAI's and Anthropic's `/v1/models`
/// share the shape); a bare top-level array is accepted too, since relays
/// have been seen to unwrap it. Server order is kept — it is often the
/// relay's curated preference order.
pub(crate) fn parse_models(status: u16, body: &[u8]) -> Result<Vec<String>, String> {
    if !(200..300).contains(&status) {
        return Err(error_detail(status, body));
    }
    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid response JSON: {err}"))?;
    let entries = json
        .pointer("/data")
        .and_then(|value| value.as_array())
        .or_else(|| json.as_array())
        .ok_or_else(|| "no model list in response".to_string())?;
    let models: Vec<String> = entries
        .iter()
        .filter_map(|entry| entry.get("id").and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    if models.is_empty() {
        return Err("the model list is empty".to_string());
    }
    Ok(models)
}

/// Fetch the provider's model list, trying [`AiCommitSettings::models_urls`]
/// in order. The first URL that answers with a parseable list wins; the last
/// failure is what the user sees. Network-disabled in tests like [`generate`].
#[cfg(not(test))]
pub(crate) async fn fetch_models(settings: &AiCommitSettings) -> Result<Vec<String>, String> {
    let mut last_error = String::new();
    for url in settings.models_urls() {
        match crate::http::get_json(
            url,
            settings.auth_headers(),
            crate::http::AI_REQUEST_TIMEOUT,
        )
        .await
        {
            Ok(response) => match parse_models(response.status.into(), &response.body) {
                Ok(models) => return Ok(models),
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(provider: AiProvider) -> AiCommitSettings {
        AiCommitSettings {
            provider,
            api_key: "sk-test".to_string(),
            model: String::new(),
            endpoint: String::new(),
            ..AiCommitSettings::default()
        }
    }

    #[test]
    fn provider_keys_round_trip() {
        for provider in AiProvider::ALL {
            assert_eq!(AiProvider::from_key(provider.key()), Some(*provider));
        }
        assert_eq!(AiProvider::from_key("nonsense"), None);
    }

    #[test]
    fn unconfigured_means_missing_api_key() {
        let mut settings = settings(AiProvider::Anthropic);
        assert!(settings.is_configured());
        settings.api_key = "   ".to_string();
        assert!(!settings.is_configured());
        // Model and endpoint stay optional.
        settings.api_key = String::new();
        settings.model = String::new();
        settings.endpoint = String::new();
        assert!(!settings.is_configured());
    }

    #[test]
    fn defaults_fill_model_and_endpoint() {
        let anthropic = settings(AiProvider::Anthropic);
        assert_eq!(anthropic.effective_model(), "claude-sonnet-4-20250514");
        assert_eq!(
            anthropic.endpoint_url(),
            "https://api.anthropic.com/v1/messages"
        );

        let openai = settings(AiProvider::OpenAiCompatible);
        assert_eq!(openai.effective_model(), "gpt-4o-mini");
        assert_eq!(
            openai.endpoint_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn custom_endpoint_overrides_the_base_url() {
        let mut settings = settings(AiProvider::OpenAiCompatible);
        settings.endpoint = "https://relay.example.com/".to_string();
        assert_eq!(
            settings.endpoint_url(),
            "https://relay.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn custom_endpoint_ending_in_v1_is_not_doubled() {
        // Relays hand out base URLs both with and without the version segment;
        // appending `/v1/…` to one that already ends in `/v1` produces a
        // `/v1/v1/…` path the server answers with 404.
        let mut openai = settings(AiProvider::OpenAiCompatible);
        openai.endpoint = "http://127.0.0.1:15721/v1".to_string();
        assert_eq!(
            openai.endpoint_url(),
            "http://127.0.0.1:15721/v1/chat/completions"
        );

        let mut anthropic = settings(AiProvider::Anthropic);
        anthropic.endpoint = "http://127.0.0.1:15721/v1/".to_string();
        assert_eq!(
            anthropic.endpoint_url(),
            "http://127.0.0.1:15721/v1/messages"
        );
    }

    #[test]
    fn endpoint_already_pointing_at_the_chat_path_is_used_verbatim() {
        let mut openai = settings(AiProvider::OpenAiCompatible);
        openai.endpoint = "https://relay.example.com/v1/chat/completions".to_string();
        assert_eq!(
            openai.endpoint_url(),
            "https://relay.example.com/v1/chat/completions"
        );

        let mut anthropic = settings(AiProvider::Anthropic);
        anthropic.endpoint = "https://relay.example.com/v1/messages".to_string();
        assert_eq!(
            anthropic.endpoint_url(),
            "https://relay.example.com/v1/messages"
        );
    }

    #[test]
    fn long_diffs_are_truncated_on_a_char_boundary() {
        let short = "diff --git a/x b/x";
        assert_eq!(truncate_diff(short), short);

        let long = "ä".repeat(MAX_DIFF_LENGTH + 10);
        let truncated = truncate_diff(&long);
        let body = truncated.strip_suffix("\n[truncated]").expect("cut marker");
        assert!(body.len() <= MAX_DIFF_LENGTH);
        // The cut must not split a multi-byte char.
        assert!(body.chars().all(|c| c == 'ä'));
    }

    #[test]
    fn user_content_marks_a_truncated_diff() {
        let long = "x".repeat(MAX_DIFF_LENGTH + 10);
        let content = build_user_content(&long, &[]);
        assert!(content.contains("\n[truncated]"));
    }

    #[test]
    fn user_content_leads_with_recent_subjects() {
        let content = build_user_content(
            "--- Diff --- body",
            &["feat: one".to_string(), "fix: two".to_string()],
        );
        assert!(content.starts_with("Recent commit messages"));
        assert!(content.contains("feat: one\nfix: two"));
        assert!(content.contains("--- Diff ---\n--- Diff --- body"));
    }

    #[test]
    fn user_content_without_subjects_is_just_the_diff() {
        let content = build_user_content("the diff", &["  ".to_string(), String::new()]);
        assert_eq!(content, "the diff");
    }

    #[test]
    fn sanitize_strips_code_fences_and_wrapping_quotes() {
        assert_eq!(
            sanitize("```rust\nfeat: add AI commit messages\n```"),
            "feat: add AI commit messages"
        );
        assert_eq!(sanitize("```\nplain fence\n```"), "plain fence");
        assert_eq!(sanitize("`quoted`"), "quoted");
        assert_eq!(sanitize("\"quoted\""), "quoted");
        assert_eq!(sanitize("  feat: bare  "), "feat: bare");
        // A fence with no closing marker is left alone rather than mangled.
        assert_eq!(sanitize("```unclosed"), "```unclosed");
    }

    #[test]
    fn anthropic_request_carries_key_version_and_body() {
        let request = build_request(
            &settings(AiProvider::Anthropic),
            "the diff",
            &["feat: prior".to_string()],
        );
        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        assert!(
            request
                .headers
                .contains(&("x-api-key", "sk-test".to_string()))
        );
        assert!(
            request
                .headers
                .contains(&("anthropic-version", "2023-06-01".to_string()))
        );

        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["max_tokens"], MAX_OUTPUT_TOKENS);
        assert_eq!(body["system"], SYSTEM_PROMPT);
        assert_eq!(body["messages"][0]["role"], "user");
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("the diff")
        );
    }

    #[test]
    fn openai_request_uses_bearer_auth_and_system_role() {
        let request = build_request(&settings(AiProvider::OpenAiCompatible), "the diff", &[]);
        assert_eq!(request.url, "https://api.openai.com/v1/chat/completions");
        assert!(
            request
                .headers
                .contains(&("Authorization", "Bearer sk-test".to_string()))
        );

        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["temperature"], 0.3);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn parse_response_reads_both_provider_shapes() {
        let anthropic = br#"{"content":[{"type":"text","text":"feat: done"}]}"#;
        assert_eq!(
            parse_response(AiProvider::Anthropic, 200, anthropic).unwrap(),
            "feat: done"
        );

        let openai = br#"{"choices":[{"message":{"content":"fix: it"}}]}"#;
        assert_eq!(
            parse_response(AiProvider::OpenAiCompatible, 200, openai).unwrap(),
            "fix: it"
        );
    }

    #[test]
    fn parse_response_skips_anthropic_thinking_blocks() {
        // Reasoning models answer with a `thinking` block ahead of the text, so
        // reading only `content/0` misses the reply entirely.
        let body = br#"{"content":[
            {"type":"thinking","thinking":"pondering the diff","signature":"sig"},
            {"type":"text","text":"feat: done"}
        ]}"#;
        assert_eq!(
            parse_response(AiProvider::Anthropic, 200, body).unwrap(),
            "feat: done"
        );
    }

    #[test]
    fn parse_response_sanitizes_the_reply() {
        let anthropic = br#"{"content":[{"type":"text","text":"```\nfeat: fenced\n```"}]}"#;
        assert_eq!(
            parse_response(AiProvider::Anthropic, 200, anthropic).unwrap(),
            "feat: fenced"
        );
    }

    #[test]
    fn parse_response_surfaces_server_errors() {
        let body = br#"{"error":{"message":"invalid api key"}}"#;
        assert_eq!(
            parse_response(AiProvider::Anthropic, 401, body).unwrap_err(),
            "HTTP 401: invalid api key"
        );
        assert_eq!(
            parse_response(AiProvider::OpenAiCompatible, 500, br#"{}"#).unwrap_err(),
            "HTTP 500"
        );
    }

    #[test]
    fn parse_response_surfaces_plain_text_error_bodies() {
        // A wrong path on a plain relay answers with a 404 and no JSON at all;
        // the body is still the most useful thing to show.
        assert_eq!(
            parse_response(AiProvider::OpenAiCompatible, 404, b"Not Found").unwrap_err(),
            "HTTP 404: Not Found"
        );
        assert_eq!(
            parse_response(AiProvider::OpenAiCompatible, 404, b"").unwrap_err(),
            "HTTP 404"
        );
    }

    #[test]
    fn parse_response_rejects_empty_replies() {
        let body = br#"{"content":[]}"#;
        assert!(parse_response(AiProvider::Anthropic, 200, body).is_err());
    }

    #[test]
    fn models_urls_cover_versioned_and_unversioned_mounts() {
        let bare = settings(AiProvider::OpenAiCompatible);
        assert_eq!(
            bare.models_urls(),
            vec![
                "https://api.openai.com/v1/models",
                "https://api.openai.com/models",
            ]
        );

        let mut versioned = settings(AiProvider::Anthropic);
        versioned.endpoint = "http://127.0.0.1:15721/v1/".to_string();
        assert_eq!(
            versioned.models_urls(),
            vec!["http://127.0.0.1:15721/v1/models"]
        );
    }

    #[test]
    fn parse_models_reads_openai_and_anthropic_shapes() {
        let openai = br#"{"data":[{"id":"gpt-4o-mini"},{"id":"  "},{"id":"gpt-4o"}]}"#;
        assert_eq!(
            parse_models(200, openai).unwrap(),
            vec!["gpt-4o-mini".to_string(), "gpt-4o".to_string()]
        );

        // Anthropic's /v1/models uses the same envelope.
        let anthropic = br#"{"data":[{"id":"claude-sonnet-5"}]}"#;
        assert_eq!(
            parse_models(200, anthropic).unwrap(),
            vec!["claude-sonnet-5".to_string()]
        );

        // Some relays unwrap the envelope entirely.
        let bare = br#"[{"id":"glm-5.3"}]"#;
        assert_eq!(
            parse_models(200, bare).unwrap(),
            vec!["glm-5.3".to_string()]
        );
    }

    #[test]
    fn parse_models_rejects_error_and_empty_lists() {
        let error = br#"{"error":{"message":"bad key"}}"#;
        assert_eq!(parse_models(401, error).unwrap_err(), "HTTP 401: bad key");
        assert_eq!(parse_models(404, b"").unwrap_err(), "HTTP 404");
        assert_eq!(
            parse_models(200, br#"{"data":[]}"#).unwrap_err(),
            "the model list is empty"
        );
        assert_eq!(
            parse_models(200, br#"{"object":"list"}"#).unwrap_err(),
            "no model list in response"
        );
    }

    #[test]
    fn auth_headers_match_the_provider() {
        let anthropic = settings(AiProvider::Anthropic);
        assert_eq!(
            anthropic.auth_headers(),
            vec![
                ("x-api-key", "sk-test".to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ]
        );
        let openai = settings(AiProvider::OpenAiCompatible);
        assert_eq!(
            openai.auth_headers(),
            vec![("Authorization", "Bearer sk-test".to_string())]
        );
    }

    #[test]
    fn explanation_prompt_carries_locale_and_patch() {
        let prompt = build_explanation_cli_prompt(
            "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new",
            "zh-CN",
        );
        assert!(
            prompt.starts_with(EXPLAIN_SYSTEM_PROMPT),
            "the CLI prompt leads with the explanation system prompt"
        );
        assert!(prompt.contains("locale: zh-CN"));
        assert!(prompt.contains("@@ -1 +1 @@"));
        assert!(prompt.contains("+new"));
    }

    #[test]
    fn explanation_prompt_truncates_long_patches() {
        let long = "+".repeat(MAX_DIFF_LENGTH + 10);
        let content = build_explanation_user_content(&long, "en");
        assert!(
            content.contains("\n[truncated]"),
            "explanations share the commit path's diff budget"
        );
    }

    #[test]
    fn explanation_request_swaps_the_system_prompt() {
        let request = build_explanation_request(&settings(AiProvider::Anthropic), "the patch", "en");
        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["system"], EXPLAIN_SYSTEM_PROMPT);
        assert_ne!(body["system"], SYSTEM_PROMPT);
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("the patch")
        );

        let openai =
            build_explanation_request(&settings(AiProvider::OpenAiCompatible), "the patch", "en");
        let body: serde_json::Value = serde_json::from_str(&openai.body).unwrap();
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], EXPLAIN_SYSTEM_PROMPT);
    }

    fn mr_commits() -> Vec<(String, String)> {
        vec![
            ("abc1234".to_string(), "Fix widget focus ring".to_string()),
            ("def5678".to_string(), "Add focus regression test".to_string()),
        ]
    }

    #[test]
    fn mr_description_prompt_carries_target_locale_and_context() {
        let prompt = build_mr_description_cli_prompt(
            "main",
            &mr_commits(),
            " src/widget.rs | 12 ++++++---\n 2 files changed",
            "zh-CN",
        );
        assert!(
            prompt.starts_with(MR_DESCRIPTION_SYSTEM_PROMPT),
            "the CLI prompt leads with the MR description system prompt"
        );
        assert!(prompt.contains("locale: zh-CN"));
        assert!(prompt.contains("merging into \"main\""));
        assert!(prompt.contains("- abc1234 Fix widget focus ring"));
        assert!(prompt.contains("Commits (2)"));
        assert!(prompt.contains("src/widget.rs | 12"));
    }

    #[test]
    fn mr_description_prompt_truncates_a_huge_diffstat() {
        let long = "file.rs | 1000 ++++++++++\n".repeat(MAX_DIFF_LENGTH / 8);
        let content =
            build_mr_description_user_content("main", &mr_commits(), &long, "en");
        assert!(
            content.contains("\n[truncated]"),
            "the diffstat shares the commit path's diff budget"
        );
    }

    #[test]
    fn mr_description_request_swaps_the_system_prompt() {
        let request = build_mr_description_request(
            &settings(AiProvider::Anthropic),
            "main",
            &mr_commits(),
            " src/widget.rs | 12 ++++++---",
            "en",
        );
        let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["system"], MR_DESCRIPTION_SYSTEM_PROMPT);
        assert_ne!(body["system"], SYSTEM_PROMPT);
        assert_ne!(body["system"], EXPLAIN_SYSTEM_PROMPT);
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("abc1234")
        );

        let openai = build_mr_description_request(
            &settings(AiProvider::OpenAiCompatible),
            "main",
            &mr_commits(),
            "stat",
            "en",
        );
        let body: serde_json::Value = serde_json::from_str(&openai.body).unwrap();
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], MR_DESCRIPTION_SYSTEM_PROMPT);
    }
}
