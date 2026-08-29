//! GitHub REST API client for the sidebar's Pull Requests section.
//!
//! The permalink module already turns a repository's remotes into a forge web
//! base; this module points the same remote knowledge at GitHub's REST API to
//! list open pull requests and read their combined CI status. Requests carry
//! the token the AI commit sources already know how to find (the GitHub CLI's
//! hosts file, then `GH_TOKEN`/`GITHUB_TOKEN`) when one is present, and go
//! out unauthenticated otherwise — public repositories list fine that way,
//! within the 60-requests-per-hour unauthenticated rate limit. Authentication
//! exists purely to widen that budget and unlock private repositories; no
//! write-side API is used.

use std::time::Duration;

use worktree_core::domain::{CommitId, PullRequest, PullRequestChecksState, Remote};
use worktree_core::error::{Error, ErrorKind};
use serde_json::Value;

use super::permalink::{origin_remote, parse_remote_url, ForgeKind};
use crate::ai_commit_sources::{read_gh_token, EnvAccess};
use crate::http;

/// How long one GitHub API request may take. Same ceiling as ordinary
/// fetches; PR listings are small documents.
// Only the `cfg(not(test))` network paths call this; test builds stub them out.
#[cfg_attr(test, allow(dead_code))]
const GITHUB_API_TIMEOUT: Duration = Duration::from_secs(15);

/// Open pull requests per load. Fifty comfortably covers what an interactive
/// client cares about while keeping the response small.
const PULL_REQUESTS_PER_PAGE: usize = 50;

/// Ceiling on the per-PR combined-status pass that follows a listing: one
/// request per PR head, so a busy repository must not turn one reload into
/// fifty API calls.
#[cfg_attr(test, allow(dead_code))]
pub(super) const PULL_REQUEST_CHECKS_CAP: usize = 20;

/// Owner/repo of a GitHub repository, e.g. `dulingzhi/WorkTree`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RepoSlug {
    pub(super) owner: String,
    pub(super) repo: String,
}

/// The GitHub repository behind a repo's link remote, when there is one.
/// GitHub Enterprise hosts are out of scope for v1: only `github.com`
/// remotes resolve, so the fixed `api.github.com` root is always right.
pub(super) fn github_slug_from_remotes(remotes: &[Remote]) -> Option<RepoSlug> {
    let base = parse_remote_url(origin_remote(remotes)?.url.as_deref()?)?;
    if base.kind != ForgeKind::GitHub {
        return None;
    }
    // `web_root` is `{scheme}://github.com/{owner}/{repo}`; the scheme varies
    // with the remote (`git://`, `ssh://`, …), so split on the host, not `://`.
    let path = base.web_root.split_once("github.com/")?.1;
    let mut parts = path.trim_matches('/').split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(RepoSlug {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// The GitHub token the AI commit sources resolve (gh's hosts file, then
/// `GH_TOKEN`/`GITHUB_TOKEN`), if any. Reading it per request keeps a login
/// performed after launch effective without a restart.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn github_token() -> Option<String> {
    read_gh_token(&EnvAccess::real())
}

pub(super) fn pulls_api_url(slug: &RepoSlug) -> String {
    format!(
        "https://api.github.com/repos/{}/{}/pulls?state=open&per_page={PULL_REQUESTS_PER_PAGE}",
        slug.owner, slug.repo
    )
}

pub(super) fn combined_status_api_url(slug: &RepoSlug, sha: &str) -> String {
    format!(
        "https://api.github.com/repos/{}/{}/commits/{sha}/status",
        slug.owner, slug.repo
    )
}

pub(super) fn pull_web_url(slug: &RepoSlug, number: u64) -> String {
    format!(
        "https://github.com/{}/{}/pull/{number}",
        slug.owner, slug.repo
    )
}

/// Headers every GitHub REST call carries, plus `Authorization` when a token
/// resolved. `http::get_json` adds the `Content-Type` itself.
fn github_headers(token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("Accept", "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version", "2022-11-28".to_string()),
    ];
    if let Some(token) = token {
        headers.push(("Authorization", format!("Bearer {token}")));
    }
    headers
}

/// List the repository's open pull requests. Runs from UI-driven async
/// context (the sidebar pane spawns it); results return to the store via
/// `InternalMsg::PullRequestsLoaded`.
#[cfg_attr(test, allow(dead_code))]
pub(super) async fn fetch_pull_requests(slug: &RepoSlug) -> Result<Vec<PullRequest>, Error> {
    let token = github_token();
    let response = http::get_json(
        pulls_api_url(slug),
        github_headers(token.as_deref()),
        GITHUB_API_TIMEOUT,
    )
    .await
    .map_err(|err| Error::new(ErrorKind::Backend(err.to_string())))?;
    parse_pull_requests(response.status.into(), &response.body)
        .map_err(|err| Error::new(ErrorKind::Backend(err)))
}

/// Read the combined status of one pull request head. `Ok(None)` means the
/// commit reports no statuses at all — common for repositories whose CI is
/// all check-runs (GitHub Actions) — and renders as "no chip" rather than a
/// misleading "pending". The check-runs rollup is a deferred follow-up.
#[cfg_attr(test, allow(dead_code))]
pub(super) async fn fetch_pull_request_checks(
    slug: &RepoSlug,
    sha: &str,
) -> Result<Option<PullRequestChecksState>, Error> {
    let token = github_token();
    let response = http::get_json(
        combined_status_api_url(slug, sha),
        github_headers(token.as_deref()),
        GITHUB_API_TIMEOUT,
    )
    .await
    .map_err(|err| Error::new(ErrorKind::Backend(err.to_string())))?;
    parse_combined_status(response.status.into(), &response.body)
        .map_err(|err| Error::new(ErrorKind::Backend(err)))
}

/// Parse an open-PR listing. Entries the API shape no longer matches (no
/// number) are dropped rather than failing the whole list; `head` can be
/// `null` when the fork was deleted, which leaves empty head fields — the
/// row still lists, and checkout re-resolves the ref server-side anyway.
pub(super) fn parse_pull_requests(
    status: u16,
    body: &[u8],
) -> Result<Vec<PullRequest>, String> {
    if !(200..300).contains(&status) {
        return Err(api_error_detail(status, body));
    }
    let json: Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid response JSON: {err}"))?;
    let entries = json
        .as_array()
        .ok_or_else(|| "expected a pull request array".to_string())?;
    Ok(entries.iter().filter_map(pull_request_from_json).collect())
}

fn pull_request_from_json(entry: &Value) -> Option<PullRequest> {
    let number = entry.get("number")?.as_u64()?;
    let string_at = |pointer: &str| {
        entry
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    Some(PullRequest {
        number,
        title: string_at("/title"),
        author: string_at("/user/login"),
        head_ref: string_at("/head/ref"),
        head_sha: CommitId(string_at("/head/sha").into()),
        base_ref: string_at("/base/ref"),
        draft: entry.get("draft").and_then(Value::as_bool).unwrap_or(false),
        checks: None,
    })
}

/// Parse a combined-status reply into a chip state, or `None` when the
/// commit has zero statuses.
pub(super) fn parse_combined_status(
    status: u16,
    body: &[u8],
) -> Result<Option<PullRequestChecksState>, String> {
    if !(200..300).contains(&status) {
        return Err(api_error_detail(status, body));
    }
    let json: Value =
        serde_json::from_slice(body).map_err(|err| format!("invalid response JSON: {err}"))?;
    let total = json
        .pointer("/total_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if total == 0 {
        return Ok(None);
    }
    let state = json
        .pointer("/state")
        .and_then(Value::as_str)
        .ok_or_else(|| "combined status reply has no state".to_string())?;
    match state {
        "success" => Ok(Some(PullRequestChecksState::Success)),
        "failure" => Ok(Some(PullRequestChecksState::Failure)),
        "pending" => Ok(Some(PullRequestChecksState::Pending)),
        "error" => Ok(Some(PullRequestChecksState::Error)),
        other => Err(format!("unknown combined status state: {other}")),
    }
}

/// Surface the API's own message on failure, like the AI providers' error
/// detail does. 401/403/404 carry a sign-in hint because the common causes
/// are the unauthenticated rate limit and private repositories.
fn api_error_detail(status: u16, body: &[u8]) -> String {
    let mut detail = format!("HTTP {status}");
    if let Ok(json) = serde_json::from_slice::<Value>(body)
        && let Some(message) = json
            .pointer("/message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    {
        detail = format!("HTTP {status}: {message}");
    }
    if matches!(status, 401 | 403 | 404) {
        detail.push_str(
            " — private or rate-limited? sign in with the GitHub CLI \
             (`gh auth login`) or set GH_TOKEN, then reload",
        );
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, url: Option<&str>) -> Remote {
        Remote {
            name: name.to_string(),
            url: url.map(str::to_string),
        }
    }

    fn sample_pulls_body() -> String {
        r#"[
            {
                "number": 7,
                "title": "Fix merge dialog focus",
                "user": {"login": "jai"},
                "head": {"ref": "fix/merge-focus", "sha": "aa1111111111111111111111111111111111111111"},
                "base": {"ref": "main"},
                "draft": false
            },
            {
                "number": 9,
                "title": "WIP: sidebar sections",
                "user": {"login": "mira"},
                "head": null,
                "base": {"ref": "main"}
            }
        ]"#
        .to_string()
    }

    #[test]
    fn slug_resolves_from_https_and_ssh_github_remotes() {
        let https = github_slug_from_remotes(&[remote("origin", Some("https://github.com/dulingzhi/WorkTree.git"))]);
        assert_eq!(
            https,
            Some(RepoSlug {
                owner: "dulingzhi".into(),
                repo: "WorkTree".into()
            })
        );
        let ssh = github_slug_from_remotes(&[remote(
            "origin",
            Some("git@github.com:acme/widgets.git"),
        )]);
        assert_eq!(
            ssh,
            Some(RepoSlug {
                owner: "acme".into(),
                repo: "widgets".into()
            })
        );
    }

    #[test]
    fn slug_rejects_other_forges_and_missing_urls() {
        assert!(github_slug_from_remotes(&[remote("origin", Some("https://gitlab.com/acme/widgets.git"))]).is_none());
        assert!(github_slug_from_remotes(&[remote("origin", None)]).is_none());
        assert!(github_slug_from_remotes(&[]).is_none());
    }

    #[test]
    fn urls_and_headers_follow_the_documented_shapes() {
        let slug = RepoSlug {
            owner: "acme".into(),
            repo: "widgets".into(),
        };
        assert_eq!(
            pulls_api_url(&slug),
            "https://api.github.com/repos/acme/widgets/pulls?state=open&per_page=50"
        );
        assert_eq!(
            combined_status_api_url(&slug, "deadbeef"),
            "https://api.github.com/repos/acme/widgets/commits/deadbeef/status"
        );
        assert_eq!(
            pull_web_url(&slug, 7),
            "https://github.com/acme/widgets/pull/7"
        );

        let anonymous = github_headers(None);
        assert_eq!(anonymous.len(), 2);
        assert_eq!(anonymous[0], ("Accept", "application/vnd.github+json".to_string()));

        let authorized = github_headers(Some("tok"));
        assert_eq!(authorized.len(), 3);
        assert_eq!(
            authorized[2],
            ("Authorization", "Bearer tok".to_string())
        );
    }

    #[test]
    fn pull_list_parses_entries_and_tolerates_a_deleted_head() {
        let pulls = parse_pull_requests(200, sample_pulls_body().as_bytes()).unwrap();
        assert_eq!(pulls.len(), 2);
        assert_eq!(pulls[0].number, 7);
        assert_eq!(pulls[0].title, "Fix merge dialog focus");
        assert_eq!(pulls[0].author, "jai");
        assert_eq!(pulls[0].head_ref, "fix/merge-focus");
        assert_eq!(
            pulls[0].head_sha.0.as_ref(),
            "aa1111111111111111111111111111111111111111"
        );
        assert_eq!(pulls[0].base_ref, "main");
        assert_eq!(pulls[0].checks, None);
        // Draft defaults to false when absent; head fields empty out.
        assert!(!pulls[1].draft);
        assert_eq!(pulls[1].head_ref, "");
        assert_eq!(pulls[1].head_sha.0.as_ref(), "");
    }

    #[test]
    fn pull_list_errors_carry_the_api_message_and_sign_in_hint() {
        let body = br#"{"message": "API rate limit exceeded"}"#;
        let err = parse_pull_requests(403, body).unwrap_err();
        assert!(err.contains("HTTP 403: API rate limit exceeded"), "{err}");
        assert!(err.contains("gh auth login"), "{err}");
        assert!(parse_pull_requests(200, br#"{"message": "not a list"}"#).is_err());
    }

    #[test]
    fn combined_status_maps_states_and_treats_zero_statuses_as_absent() {
        let state = |total: u64, state: &str| {
            format!(r#"{{"total_count": {total}, "state": "{state}"}}"#)
        };
        assert_eq!(
            parse_combined_status(200, state(2, "success").as_bytes()).unwrap(),
            Some(PullRequestChecksState::Success)
        );
        assert_eq!(
            parse_combined_status(200, state(2, "failure").as_bytes()).unwrap(),
            Some(PullRequestChecksState::Failure)
        );
        assert_eq!(
            parse_combined_status(200, state(1, "pending").as_bytes()).unwrap(),
            Some(PullRequestChecksState::Pending)
        );
        assert_eq!(
            parse_combined_status(200, state(1, "error").as_bytes()).unwrap(),
            Some(PullRequestChecksState::Error)
        );
        assert_eq!(
            parse_combined_status(200, state(0, "pending").as_bytes()).unwrap(),
            None
        );
        assert!(parse_combined_status(200, br#"{"total_count": 3}"#).is_err());
        assert!(parse_combined_status(500, b"{}").is_err());
    }
}
