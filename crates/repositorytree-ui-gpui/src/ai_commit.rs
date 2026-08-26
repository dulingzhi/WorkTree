//! AI-generated commit messages.
//!
//! The ✨ button next to the commit box asks a language model to draft the
//! commit message from the staged diff. Two HTTP providers are supported —
//! Anthropic's Messages API and any OpenAI-compatible chat-completions
//! endpoint (OpenAI itself, or a relay/proxy via the custom endpoint field).
//!
//! The prompt pairs the diff with the repository's recent commit subjects so
//! the answer matches the project's existing type, scope, and language, and
//! the reply is sanitized (code fences and wrapping quotes stripped) before
//! it lands in the message box. This mirrors RepositoryTree's
//! `CommitPromptBuilder`/`CommitMessageSanitizer` design.
//!
//! Provider settings live in a process-global seeded from the session file,
//! following `avatar_source`'s shape: the commit-box render path has no
//! interest in settings-window entities and only needs to know whether the
//! feature is configured.

use repositorytree_state::session;
use gpui::SharedString;
use std::sync::{LazyLock, RwLock};

/// How many recent commit subjects feed the prompt's format examples. The
/// state-side loader fetches the same number — one source of truth.
pub(crate) const RECENT_COMMITS_COUNT: usize =
    repositorytree_state::model::AI_COMMIT_RECENT_SUBJECTS_LIMIT;

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

/// The effective AI configuration. Fields are the raw persisted strings;
/// empty means "fall back to the provider default".
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AiCommitSettings {
    pub(crate) provider: AiProvider,
    pub(crate) api_key: String,
    pub(crate) model: String,
    pub(crate) endpoint: String,
}

impl AiCommitSettings {
    /// A provider only needs an API key — model and endpoint have defaults.
    pub(crate) fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
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
    let provider = ui_session
        .ai_commit_provider
        .as_deref()
        .and_then(AiProvider::from_key)
        .unwrap_or_default();
    set_current(AiCommitSettings {
        provider,
        api_key: ui_session.ai_commit_api_key.clone().unwrap_or_default(),
        model: ui_session.ai_commit_model.clone().unwrap_or_default(),
        endpoint: ui_session.ai_commit_endpoint.clone().unwrap_or_default(),
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

/// Truncate an over-long diff, marking the cut. Splits on a char boundary so
/// the result stays valid UTF-8.
pub(crate) fn truncate_diff(diff: &str) -> &str {
    if diff.len() <= MAX_DIFF_LENGTH {
        return diff;
    }
    let mut cut = MAX_DIFF_LENGTH;
    while !diff.is_char_boundary(cut) {
        cut -= 1;
    }
    &diff[..cut]
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

/// Run one generation end to end. Network-disabled in tests; the pure halves
/// (`build_request`, `parse_response`) carry the coverage instead.
#[cfg(not(test))]
pub(crate) async fn generate(
    settings: &AiCommitSettings,
    diff: &str,
    recent_subjects: &[String],
) -> Result<String, String> {
    let request = build_request(settings, diff, recent_subjects);
    let response = crate::http::post_json(
        request.url,
        request.headers,
        request.body,
        crate::http::AI_REQUEST_TIMEOUT,
    )
    .await
    .map_err(|err| err.to_string())?;
    parse_response(settings.provider, response.status.into(), &response.body)
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
        assert!(truncated.len() <= MAX_DIFF_LENGTH);
        // The cut must not split a multi-byte char.
        assert!(truncated.chars().all(|c| c == 'ä'));
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
}
