use super::*;

pub(super) fn model(this: &PopoverHost) -> ContextMenuModel {
    let repo_id = this.active_repo_id();
    let disabled = repo_id.is_none();
    let repo_id = repo_id.unwrap_or(RepoId(0));
    let tracking_branch_name = super::active_branch_tracking_upstream_name(this);
    let repo = this.state.repos.iter().find(|repo| repo.id == repo_id);
    let force_push_label = if repo
        .and_then(|repo| repo.pending_force_push_lease.as_ref())
        .is_some()
    {
        "Force push published amend with lease…"
    } else {
        "Force push (with lease)…"
    };

    let mut items = vec![
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
    ];
    // jj compat: force (and leased) pushes have no jj routing — the entry is
    // hidden rather than opening a confirm the reducer would drop.
    if repo.is_none_or(|repo| !repo.capabilities.read_only) {
        items.push(ContextMenuItem::Entry {
            label: force_push_label.into(),
            icon: Some("icons/warning.svg".into()),
            shortcut: Some("F".into()),
            disabled,
            action: Box::new(ContextMenuAction::OpenPopover {
                kind: PopoverKind::ForcePushConfirm { repo_id },
            }),
        });
    }
    ContextMenuModel::new(items)
}
