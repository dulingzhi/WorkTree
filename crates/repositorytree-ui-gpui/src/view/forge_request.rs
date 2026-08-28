//! Web-form "create pull / merge request" URLs — the zero-API entry to the
//! PR/MR flow for both forges. Nothing leaves the machine except the browser
//! open: GitHub gets a prefilled compare page, GitLab a prefilled
//! merge-request form. Self-hosted GitLab hosts are not guessed — only the
//! hosts the permalink forge map already knows produce these URLs.

use super::permalink::{parse_remote_url, ForgeKind, ForgeWebBase};
use repositorytree_core::domain::Remote;

/// The forges with a known create-request web shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum ForgeRequestKind {
    GitHub,
    GitLab,
}

/// The web base of the remote's forge, when that forge has a known
/// create-request page. Origin is preferred, mirroring the permalink and
/// GitHub-slug resolutions.
pub(in crate::view) fn forge_request_base_from_remotes(
    remotes: &[Remote],
) -> Option<(ForgeRequestKind, ForgeWebBase)> {
    let preferred = remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .or_else(|| remotes.iter().find(|remote| remote.url.is_some()))?;
    let url = preferred.url.as_deref()?;
    let base = parse_remote_url(url)?;
    let kind = match base.kind {
        ForgeKind::GitHub => ForgeRequestKind::GitHub,
        ForgeKind::GitLab => ForgeRequestKind::GitLab,
        _ => return None,
    };
    Some((kind, base))
}

/// Percent-encode a branch name for a query component: unreserved bytes and
/// `/` (legal and readable in a query) stay, everything else escapes.
fn encode_query_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A branch is safe to splice into a URL when it cannot be read as an option
/// or a range — the same intent as the gix layer's ref validators, inline
/// because those live in the backend crate.
pub(in crate::view) fn branch_is_url_safe(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && !branch
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ':' | '?' | '#' | '[' | '\\' | '^' | '~'))
}

/// The prefilled create-request URL for one branch pair.
///
/// GitHub: `{root}/compare/{base}...{head}` walks straight to the "open a
/// pull request" page; without a known base, `/pull/new/{head}` starts the
/// form with the repository's default as the target.
/// GitLab: `{root}/-/merge_requests/new` with query-prefilled source and
/// target branches.
pub(in crate::view) fn create_request_url(
    kind: ForgeRequestKind,
    web_root: &str,
    head: &str,
    base_branch: Option<&str>,
) -> String {
    let head = encode_query_component(head);
    match kind {
        ForgeRequestKind::GitHub => match base_branch {
            Some(base) if branch_is_url_safe(base) => {
                format!("{web_root}/compare/{}...{head}", encode_query_component(base))
            }
            _ => format!("{web_root}/pull/new/{head}"),
        },
        ForgeRequestKind::GitLab => {
            let mut url = format!(
                "{web_root}/-/merge_requests/new?merge_request[source_branch]={head}"
            );
            if let Some(base) = base_branch.filter(|base| branch_is_url_safe(base)) {
                url.push_str("&merge_request[target_branch]=");
                url.push_str(&encode_query_component(base));
            }
            url
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, url: &str) -> Remote {
        Remote {
            name: name.to_string(),
            url: Some(url.to_string()),
        }
    }

    #[test]
    fn forge_base_resolves_github_and_gitlab_but_not_generic_hosts() {
        let (kind, base) =
            forge_request_base_from_remotes(&[remote("origin", "git@github.com:acme/widgets.git")])
                .unwrap();
        assert_eq!(kind, ForgeRequestKind::GitHub);
        assert_eq!(base.web_root, "https://github.com/acme/widgets");

        let (kind, base) = forge_request_base_from_remotes(&[remote(
            "origin",
            "https://gitlab.com/acme/widgets.git",
        )])
        .unwrap();
        assert_eq!(kind, ForgeRequestKind::GitLab);
        assert_eq!(base.web_root, "https://gitlab.com/acme/widgets");

        // A self-hosted GitLab is indistinguishable from any generic host by
        // URL alone — no create-request URL is guessed for it.
        assert!(forge_request_base_from_remotes(&[remote(
            "origin",
            "https://git.acme.dev/widgets.git"
        )])
        .is_none());
        // No URL at all: nothing to resolve from.
        assert!(forge_request_base_from_remotes(&[Remote {
            name: "origin".to_string(),
            url: None,
        }])
        .is_none());
    }

    #[test]
    fn github_urls_use_compare_with_a_base_and_pull_new_without() {
        let root = "https://github.com/acme/widgets";
        assert_eq!(
            create_request_url(
                ForgeRequestKind::GitHub,
                root,
                "feat/widget",
                Some("main")
            ),
            "https://github.com/acme/widgets/compare/main...feat/widget"
        );
        assert_eq!(
            create_request_url(ForgeRequestKind::GitHub, root, "feat/widget", None),
            "https://github.com/acme/widgets/pull/new/feat/widget"
        );
    }

    #[test]
    fn gitlab_urls_prefill_the_merge_request_form() {
        let root = "https://gitlab.com/acme/widgets";
        assert_eq!(
            create_request_url(
                ForgeRequestKind::GitLab,
                root,
                "feat/widget",
                Some("main")
            ),
            "https://gitlab.com/acme/widgets/-/merge_requests/new\
             ?merge_request[source_branch]=feat/widget\
             &merge_request[target_branch]=main"
        );
        // Without a base the form opens with only the source prefilled.
        assert_eq!(
            create_request_url(ForgeRequestKind::GitLab, root, "feat/widget", None),
            "https://gitlab.com/acme/widgets/-/merge_requests/new\
             ?merge_request[source_branch]=feat/widget"
        );
    }

    #[test]
    fn branches_that_cannot_travel_in_a_url_are_refused() {
        for bad in ["", "-x", "a..b", "a b", "a?b", "a#b"] {
            assert!(!branch_is_url_safe(bad), "{bad:?} must be refused");
        }
        assert!(branch_is_url_safe("feat/widget-2.0_x"));
        assert!(branch_is_url_safe("main"));
    }

    #[test]
    fn query_components_escape_the_characters_that_break_a_url() {
        assert_eq!(encode_query_component("feat/a+b"), "feat/a%2Bb");
        assert_eq!(encode_query_component("100%"), "100%25");
        assert_eq!(encode_query_component("sp ace"), "sp%20ace");
        assert_eq!(encode_query_component("feat/x"), "feat/x");
    }
}
