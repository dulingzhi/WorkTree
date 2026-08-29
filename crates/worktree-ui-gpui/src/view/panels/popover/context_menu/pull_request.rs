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

    let mut items = vec![ContextMenuItem::Header(
        crate::i18n::t!("cm.pr.menu_title", number = number)
            .to_string()
            .into(),
    )];
    if !title.is_empty() {
        items.push(ContextMenuItem::Label(title.into()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use worktree_core::domain::{
        CommitId, PullRequest, PullRequestChecksState, PullRequestState, RepoSpec,
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
            ContextMenuAction::OpenWebUrl { url } if url == "https://github.com/acme/widgets/pull/7"
        ));
        assert!(matches!(
            actions[2],
            ContextMenuAction::CopyLinkAddress { url }
                if url == "https://github.com/acme/widgets/pull/7"
        ));
    }

    #[test]
    fn model_drops_every_action_when_the_repo_has_no_github_remote() {
        let mut repo = github_repo_state();
        repo.remotes = Loadable::Ready(Arc::new(vec![Remote {
            name: "origin".to_string(),
            url: Some("https://gitlab.com/acme/widgets.git".to_string()),
        }]));
        let model = model_for_pull_request(Some(&repo), RepoId(1), 7);

        // No slug → no remote, no URL: only the header remains.
        assert!(entry_actions(&model).is_empty());
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
}
