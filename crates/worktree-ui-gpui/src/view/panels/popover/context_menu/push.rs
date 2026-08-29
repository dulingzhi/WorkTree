use super::*;

pub(super) fn model(this: &PopoverHost) -> ContextMenuModel {
    // The zero-API create-request page is offered whenever the remote is on
    // a forge whose web shape we know and a branch is checked out.
    let create_request_ready = this
        .state
        .repos
        .iter()
        .find(|repo| Some(repo.id) == this.state.active_repo)
        .is_some_and(|repo| {
            repo.remotes.ready().is_some_and(|remotes| {
                super::super::super::super::forge_request::forge_request_base_from_remotes(remotes)
                    .is_some()
            }) && repo.head_branch.ready().is_some_and(|head| {
                super::super::super::super::forge_request::branch_is_url_safe(head.as_str())
            })
        });

    let repo_id = this.active_repo_id();
    let disabled = repo_id.is_none();
    let repo_id = repo_id.unwrap_or(RepoId(0));
    let tracking_branch_name = super::active_branch_tracking_upstream_name(this);
    let force_push_label = if this
        .state
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .and_then(|repo| repo.pending_force_push_lease.as_ref())
        .is_some()
    {
        "Force push published amend with lease…"
    } else {
        "Force push (with lease)…"
    };

    ContextMenuModel::new(vec![
        ContextMenuItem::Header(
            super::action_menu_title("Push", tracking_branch_name.as_deref()).into(),
        ),
        ContextMenuItem::Separator,
        ContextMenuItem::Entry {
            label: "Push".into(),
            icon: Some("icons/arrow_up.svg".into()),
            shortcut: None,
            disabled,
            action: Box::new(ContextMenuAction::Push { repo_id }),
        },
        ContextMenuItem::Entry {
            label: "Pull and re-push on failure".into(),
            icon: this
                .push_pull_retry_enabled
                .then_some("icons/check.svg".into()),
            shortcut: None,
            disabled,
            action: Box::new(ContextMenuAction::SetPushPullRetryEnabled {
                enabled: !this.push_pull_retry_enabled,
            }),
        },
        ContextMenuItem::Entry {
            label: "Push with merge request…".into(),
            icon: Some("icons/git_merge.svg".into()),
            shortcut: None,
            disabled,
            action: Box::new(ContextMenuAction::OpenPopover {
                kind: PopoverKind::MergeRequestPushPrompt { repo_id },
            }),
        },
        ContextMenuItem::Entry {
            label: "Create pull request on the web…".into(),
            icon: Some("icons/link.svg".into()),
            shortcut: None,
            disabled: !create_request_ready,
            action: Box::new(ContextMenuAction::CreateWebRequestPage),
        },
        ContextMenuItem::Entry {
            label: force_push_label.into(),
            icon: Some("icons/warning.svg".into()),
            shortcut: Some("F".into()),
            disabled,
            action: Box::new(ContextMenuAction::OpenPopover {
                kind: PopoverKind::ForcePushConfirm { repo_id },
            }),
        },
    ])
}
