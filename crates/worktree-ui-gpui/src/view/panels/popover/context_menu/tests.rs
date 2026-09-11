use super::*;

use crate::view::shortcut_labels::secondary_shortcut;

#[test]
fn context_menu_shortcut_entry_ix_matches_first_enabled_single_character_entry() {
    let model = ContextMenuModel::new(vec![
        ContextMenuItem::Header("Test".into()),
        ContextMenuItem::Entry {
            label: "Disabled A".into(),
            icon: None,
            shortcut: Some("A".into()),
            disabled: true,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(1) }),
        },
        ContextMenuItem::Entry {
            label: "Enter".into(),
            icon: None,
            shortcut: Some("Enter".into()),
            disabled: false,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(2) }),
        },
        ContextMenuItem::Entry {
            label: "Ctrl Copy".into(),
            icon: None,
            shortcut: Some(secondary_shortcut("C").into()),
            disabled: false,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(3) }),
        },
        ContextMenuItem::Entry {
            label: "Enabled A".into(),
            icon: None,
            shortcut: Some("A".into()),
            disabled: false,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(4) }),
        },
    ]);
    let model_rows = ContextMenuRows::from_model(&model, &FxHashSet::default());

    assert_eq!(context_menu_shortcut_entry_ix(&model_rows, "a"), Some(4));
    assert_eq!(context_menu_shortcut_entry_ix(&model_rows, "A"), Some(4));
    assert_eq!(context_menu_shortcut_entry_ix(&model_rows, "c"), Some(3));
    assert_eq!(context_menu_shortcut_entry_ix(&model_rows, "e"), None);
    assert_eq!(context_menu_shortcut_entry_ix(&model_rows, "enter"), None);
}

#[test]
fn context_menu_activate_entry_ix_prefers_selected_entry_and_falls_back_to_first_selectable() {
    let model = ContextMenuModel::new(vec![
        ContextMenuItem::Header("Test".into()),
        ContextMenuItem::Entry {
            label: "Disabled".into(),
            icon: None,
            shortcut: Some("D".into()),
            disabled: true,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(1) }),
        },
        ContextMenuItem::Entry {
            label: "First".into(),
            icon: None,
            shortcut: Some("Enter".into()),
            disabled: false,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(2) }),
        },
        ContextMenuItem::Entry {
            label: "Second".into(),
            icon: None,
            shortcut: Some("S".into()),
            disabled: false,
            action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(3) }),
        },
    ]);
    let model_rows = ContextMenuRows::from_model(&model, &FxHashSet::default());

    assert_eq!(context_menu_activate_entry_ix(&model_rows, None), Some(2));
    assert_eq!(
        context_menu_activate_entry_ix(&model_rows, Some(3)),
        Some(3)
    );
    assert_eq!(
        context_menu_activate_entry_ix(&model_rows, Some(1)),
        Some(2)
    );
    assert_eq!(
        context_menu_activate_entry_ix(&model_rows, Some(99)),
        Some(2)
    );
}

#[test]
fn context_menu_rows_splice_open_submenu_children_at_depth_one() {
    let entry = |label: &str| ContextMenuItem::Entry {
        label: label.into(),
        icon: None,
        shortcut: None,
        disabled: false,
        action: Box::new(ContextMenuAction::FetchAll { repo_id: RepoId(1) }),
    };
    let model = ContextMenuModel::new(vec![
        entry("Top"),
        ContextMenuItem::Submenu {
            id: "tools".into(),
            label: "Tools".into(),
            icon: None,
            children: vec![entry("Xcode"), entry("Zed")],
        },
        entry("Bottom"),
    ]);

    let collapsed = ContextMenuRows::from_model(&model, &FxHashSet::default());
    let row_kinds = |rows: &ContextMenuRows| {
        rows.iter()
            .map(|(item, depth)| {
                (
                    match item {
                        ContextMenuItem::Entry { label, .. } => label.to_string(),
                        ContextMenuItem::Submenu { label, .. } => label.to_string(),
                        _ => panic!("unexpected row kind"),
                    },
                    *depth,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        row_kinds(&collapsed),
        vec![("Top".into(), 0), ("Tools".into(), 0), ("Bottom".into(), 0)],
        "a closed submenu contributes only its own row"
    );
    assert!(collapsed.is_selectable(1), "the submenu row itself selects");

    let mut open = FxHashSet::default();
    open.insert("tools".into());
    let expanded = ContextMenuRows::from_model(&model, &open);
    assert_eq!(
        row_kinds(&expanded),
        vec![
            ("Top".into(), 0),
            ("Tools".into(), 0),
            ("Xcode".into(), 1),
            ("Zed".into(), 1),
            ("Bottom".into(), 0),
        ]
    );
}

#[test]
fn use_commit_message_action_exposes_full_message_tooltip() {
    let tooltip = context_menu_entry_tooltip(&ContextMenuAction::UseCommitMessage {
        message: "\n\nsubject\n\nbody".to_string(),
    });

    assert_eq!(
        tooltip.as_ref().map(|text| text.as_ref()),
        Some("subject\n\nbody")
    );
}

#[test]
fn non_commit_message_actions_do_not_expose_entry_tooltips() {
    assert!(
        context_menu_entry_tooltip(&ContextMenuAction::FetchAll { repo_id: RepoId(1) }).is_none()
    );
}
