use super::*;
use worktree_core::domain::Remote;

/// Context menu for a sidebar pull-request row.
pub(super) fn model(this: &PopoverHost, repo_id: RepoId, number: u64) -> ContextMenuModel {
    let repo = this.state.repos.iter().find(|repo| repo.id == repo_id);
    model_for_pull_request(repo, repo_id, number)
}

/// Pure half of [`model`]: everything it needs (title, checkout remote, web
/// URL) is re-resolved from the live repo state, so the row never carries a
/// stale copy of any of them.
fn model_for_pull_request(
    repo: Option<&RepoState>,
    repo_id: RepoId,
    number: u64,
) -> ContextMenuModel {
    let (title, remote, url) = match repo {
        Some(repo) => {
            let title = match &repo.pull_requests {
                Loadable::Ready(pull_requests) => pull_requests
                    .iter()
                    .find(|pull_request| pull_request.number == number)
                    .map(|pull_request| pull_request.title.clone())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            // Checkout targets the remote the API client resolved the slug
            // from — `origin` when present, else the first remote with a URL.
            let slug_and_remote = match &repo.remotes {
                Loadable::Ready(remotes) => crate::view::github::github_slug_from_remotes(remotes)
                    .map(|slug| (slug, origin_remote_name(remotes))),
                _ => None,
            };
            let (remote, url) = match slug_and_remote {
                Some((slug, remote)) => {
                    let remote = remote.unwrap_or_default();
                    let url = crate::view::github::pull_web_url(&slug, number);
                    (remote, url)
                }
                None => (String::new(), String::new()),
            };
            (title, remote, url)
        }
        None => (String::new(), String::new(), String::new()),
    };

    // Resolve the PR's head commit and base ref so the "Review changes" entry can
    // open the local base..head diff. Both commits must be resolvable locally;
    // when either is missing the entry is omitted.
    let (head_sha, base_ref, base_commit) = match repo {
        Some(repo) => {
            let pr = match &repo.pull_requests {
                Loadable::Ready(pull_requests) => pull_requests
                    .iter()
                    .find(|pull_request| pull_request.number == number),
                _ => None,
            };
            let head_sha = pr.map(|pr| pr.head_sha.clone());
            let base_ref = pr.map(|pr| pr.base_ref.clone()).unwrap_or_default();
            let base_commit = resolve_pr_base_commit(repo, &base_ref);
            (head_sha, base_ref, base_commit)
        }
        None => (None, String::new(), None),
    };

    let mut items = vec![ContextMenuItem::Header(
        crate::i18n::t!("cm.pr.menu_title", number = number)
            .to_string()
            .into(),
    )];
    if !title.is_empty() {
        items.push(ContextMenuItem::Label(title.clone().into()));
    }
    items.push(ContextMenuItem::Separator);

    if !remote.is_empty() {
        items.push(ContextMenuItem::Entry {
            label: "Checkout".into(),
            icon: Some("icons/check.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::CheckoutPullRequest {
                repo_id,
                remote,
                number,
            }),
        });
    }
    if let (Some(base), Some(head)) = (&base_commit, &head_sha) {
        items.push(ContextMenuItem::Entry {
            label: crate::i18n::t!("cm.pr.review_changes").to_string().into(),
            icon: Some("icons/open_external.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::ReviewPullRequestChanges {
                repo_id,
                base: base.clone(),
                head: head.clone(),
                from_label: base_ref.clone(),
                to_label: title.clone(),
            }),
        });
    }
    // Read-only check-run summary: the per-run CI detail behind the rollup chip.
    if let Some(repo) = repo
        && let Some(runs) = repo.pull_request_check_runs.get(&number)
        && !runs.is_empty()
    {
        items.push(ContextMenuItem::Separator);
        for run in runs {
            items.push(ContextMenuItem::Label(
                format!("{} · {}", run.name, check_run_status_text(run)).into(),
            ));
        }
    }
    // Read-only PR conversation: a capped, truncated preview of the comments
    // behind the PR. Purely informational — no write path, no links.
    if let Some(repo) = repo
        && let Some(comments) = repo.pull_request_comments.get(&number)
        && !comments.is_empty()
    {
        items.push(ContextMenuItem::Separator);
        items.push(ContextMenuItem::Label(
            format!("{} comments", comments.len()).into(),
        ));
        for comment in comments.iter().take(6) {
            items.push(ContextMenuItem::Label(comment_preview(comment).into()));
        }
        let remaining = comments.len() - comments.iter().take(6).count();
        if remaining > 0 {
            items.push(ContextMenuItem::Label(
                format!("… and {remaining} more").into(),
            ));
        }
    }
    if !url.is_empty() {
        items.push(ContextMenuItem::Entry {
            label: "Open in web browser".into(),
            icon: Some("icons/link.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::OpenWebUrl { url: url.clone() }),
        });
        items.push(ContextMenuItem::Entry {
            label: "Copy link address".into(),
            icon: Some("icons/copy.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::CopyLinkAddress { url }),
        });
    }
    ContextMenuModel::new(items)
}

/// The name of the remote the GitHub slug was resolved from, mirroring the
/// permalink module's `origin`-first preference.
fn origin_remote_name(remotes: &[Remote]) -> Option<String> {
    remotes
        .iter()
        .find(|remote| remote.name == "origin")
        .or_else(|| remotes.iter().find(|remote| remote.url.is_some()))
        .map(|remote| remote.name.clone())
}

/// Resolve a PR base ref to a commit id for the local base..head diff, mirroring
/// how the branch menu resolves a branch tip: prefer the local branch, then the
/// remote-tracking branch whose short name matches. Returns `None` when the ref
/// is not loaded or absent locally (e.g. a fork base never fetched).
fn resolve_pr_base_commit(
    repo: &RepoState,
    base_ref: &str,
) -> Option<worktree_core::domain::CommitId> {
    if let Loadable::Ready(branches) = &repo.branches
        && let Some(branch) = branches.iter().find(|branch| branch.name == base_ref)
    {
        return Some(branch.target.clone());
    }
    if let Loadable::Ready(remote_branches) = &repo.remote_branches
        && let Some(branch) = remote_branches
            .iter()
            .find(|branch| branch.name == base_ref)
    {
        return Some(branch.target.clone());
    }
    None
}

/// Human-readable one-line status for a single check run, used in the read-only
/// CI detail block of the PR menu. Status drives the live states (queued /
/// in progress); a completed run is described by its conclusion when present.
fn check_run_status_text(run: &worktree_core::domain::CheckRun) -> &'static str {
    use worktree_core::domain::{CheckRunConclusion, CheckRunStatus};
    match run.status {
        CheckRunStatus::Queued => "queued",
        CheckRunStatus::InProgress => "in progress",
        CheckRunStatus::Completed => match run.conclusion {
            Some(CheckRunConclusion::Success) => "passed",
            Some(CheckRunConclusion::Failure) => "failed",
            Some(CheckRunConclusion::Neutral) => "neutral",
            Some(CheckRunConclusion::Cancelled) => "cancelled",
            Some(CheckRunConclusion::Skipped) => "skipped",
            Some(CheckRunConclusion::TimedOut) => "timed out",
            Some(CheckRunConclusion::ActionRequired) => "action required",
            Some(CheckRunConclusion::Stale) => "stale",
            Some(CheckRunConclusion::StartupFailure) => "startup failure",
            None => "completed",
        },
    }
}

/// One-line, capped preview of a PR comment for the read-only menu block: the
/// author, then the first non-empty line of the body truncated to a sane width.
fn comment_preview(comment: &worktree_core::domain::PullRequestComment) -> String {
    let first_line = comment
        .body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let first_line = if first_line.chars().count() > 80 {
        let truncated: String = first_line.chars().take(80).collect();
        format!("{truncated}…")
    } else {
        first_line.to_string()
    };
    format!("{}: {}", comment.author, first_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{
        Branch, CommitId, PullRequest, PullRequestChecksState, PullRequestComment,
        PullRequestState, RepoSpec,
    };

    fn github_repo_state() -> RepoState {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        );
        repo.remotes = Loadable::Ready(Arc::new(vec![Remote {
            name: "origin".to_string(),
            url: Some("https://github.com/acme/widgets.git".to_string()),
        }]));
        repo.pull_requests = Loadable::Ready(Arc::new(vec![PullRequest {
            number: 7,
            title: "Fix merge dialog focus".to_string(),
            author: "jai".to_string(),
            head_ref: "fix/merge-focus".to_string(),
            head_sha: CommitId("aa1111111111111111111111111111111111111111".into()),
            base_ref: "main".to_string(),
            state: PullRequestState::Open,
            draft: false,
            checks: Some(PullRequestChecksState::Success),
        }]));
        repo.branches = Loadable::Ready(Arc::new(vec![Branch {
            name: "main".to_string(),
            target: CommitId("bb2222222222222222222222222222222222222222".into()),
            upstream: None,
            divergence: None,
        }]));
        repo
    }

    fn entry_actions(model: &ContextMenuModel) -> Vec<&ContextMenuAction> {
        model
            .items
            .iter()
            .filter_map(|item| match item {
                ContextMenuItem::Entry { action, .. } => Some(action.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn model_offers_checkout_open_and_copy_for_a_listed_pr() {
        let repo = github_repo_state();
        let model = model_for_pull_request(Some(&repo), RepoId(1), 7);

        let labels: Vec<String> = model
            .items
            .iter()
            .filter_map(|item| match item {
                ContextMenuItem::Entry { label, .. } => Some(label.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                "Checkout".to_string(),
                crate::i18n::t!("cm.pr.review_changes").to_string(),
                "Open in web browser".to_string(),
                "Copy link address".to_string(),
            ]
        );
        assert!(model.items.iter().any(|item| matches!(
            item,
            ContextMenuItem::Label(label) if label.as_ref() == "Fix merge dialog focus"
        )));

        let actions = entry_actions(&model);
        assert!(matches!(
            actions[0],
            ContextMenuAction::CheckoutPullRequest { repo_id, remote, number }
                if *repo_id == RepoId(1) && remote == "origin" && *number == 7
        ));
        assert!(matches!(
            actions[1],
            ContextMenuAction::ReviewPullRequestChanges {
                repo_id,
                base,
                head,
                from_label,
                to_label,
            }
                if *repo_id == RepoId(1)
                    && *base == CommitId("bb2222222222222222222222222222222222222222".into())
                    && *head == CommitId("aa1111111111111111111111111111111111111111".into())
                    && from_label == "main"
                    && to_label == "Fix merge dialog focus"
        ));
        assert!(matches!(
            actions[2],
            ContextMenuAction::OpenWebUrl { url } if url == "https://github.com/acme/widgets/pull/7"
        ));
        assert!(matches!(
            actions[3],
            ContextMenuAction::CopyLinkAddress { url }
                if url == "https://github.com/acme/widgets/pull/7"
        ));
    }

    #[test]
    fn model_drops_github_actions_but_keeps_local_review_when_no_github_remote() {
        let mut repo = github_repo_state();
        repo.remotes = Loadable::Ready(Arc::new(vec![Remote {
            name: "origin".to_string(),
            url: Some("https://gitlab.com/acme/widgets.git".to_string()),
        }]));
        let model = model_for_pull_request(Some(&repo), RepoId(1), 7);

        // No GitHub slug → no remote name, no web URL: Checkout / Open / Copy
        // (all GitHub-only) are suppressed. "Review changes" opens a local
        // base..head diff and needs neither, so it stays.
        let actions = entry_actions(&model);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            ContextMenuAction::ReviewPullRequestChanges { .. }
        ));
    }

    #[test]
    fn model_survives_an_unknown_pr_number() {
        let repo = github_repo_state();
        let model = model_for_pull_request(Some(&repo), RepoId(1), 404);
        // The PR is gone from the listing but the remote/URL actions still
        // resolve — the number is all the link needs.
        assert_eq!(entry_actions(&model).len(), 3);
        assert!(
            model
                .items
                .iter()
                .all(|item| !matches!(item, ContextMenuItem::Label(_)))
        );
    }

    #[test]
    fn model_shows_loaded_comments_read_only() {
        let mut repo = github_repo_state();
        repo.pull_request_comments.insert(
            7u64,
            vec![
                PullRequestComment {
                    author: "jai".to_string(),
                    body: "Looks good to me\nSecond line".to_string(),
                    created_at: Some("2026-01-01T00:00:00Z".to_string()),
                    html_url: Some(
                        "https://github.com/acme/widgets/pull/7#issuecomment-1".to_string(),
                    ),
                },
                PullRequestComment {
                    author: "sam".to_string(),
                    body: "  \nPlease rebase".to_string(),
                    created_at: None,
                    html_url: None,
                },
            ],
        );
        let model = model_for_pull_request(Some(&repo), RepoId(1), 7);

        // The comments section is read-only: a count label plus one preview per
        // comment. They never become actionable entries — the entry count stays
        // the four GitHub-derived actions (Checkout / Review / Open / Copy).
        let labels: Vec<String> = model
            .items
            .iter()
            .filter_map(|item| match item {
                ContextMenuItem::Label(label) => Some(label.as_ref().to_string()),
                _ => None,
            })
            .collect();
        assert!(labels.iter().any(|label| label == "2 comments"));
        assert!(labels.iter().any(|label| label == "jai: Looks good to me"));
        // The second comment's first non-empty line ("Please rebase") survives
        // the leading blank/whitespace line.
        assert!(labels.iter().any(|label| label == "sam: Please rebase"));
        assert_eq!(entry_actions(&model).len(), 4);
    }
}
