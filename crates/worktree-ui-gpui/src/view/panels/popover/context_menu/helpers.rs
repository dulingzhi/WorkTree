//! Small helpers the entry models are built from: path text, titles, debug
//! selectors and the index lookups activation uses.

use super::super::*;

pub(super) fn normalize_platform_path(path: std::path::PathBuf) -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        let mut normalized = std::path::PathBuf::new();
        for component in path.components() {
            normalized.push(component.as_os_str());
        }
        normalized
    }

    #[cfg(not(target_os = "windows"))]
    {
        path
    }
}

/// One line of the "Add to .gitignore" field as a submittable pattern, or
/// `None` when the line is blank.
///
/// Trailing spaces go through git's own rule rather than `str::trim`, which
/// would unescape a deliberate `foo\ ` back into a dangling backslash. Leading
/// whitespace is dropped: it is significant to git, but a leading space in a
/// hand-typed line is copy-paste noise far more often than intent, and the
/// resulting pattern would silently match nothing.
pub(super) fn gitignore_pattern_line(line: &str) -> Option<&str> {
    let line = worktree_core::gitignore::trim_trailing_spaces(line.trim_start());
    (!line.is_empty()).then_some(line)
}

pub(in crate::view::panels::popover) fn path_text_for_copy(path: &std::path::Path) -> String {
    normalize_platform_path(path.to_path_buf())
        .display()
        .to_string()
}

/// The `Copy absolute path` / `Copy relative path` pair every file-ish menu
/// ends with, for a repo-relative `path`.
///
/// Built in one place so the labels, icons and mnemonic cannot drift apart
/// between menus: the mnemonic is matched on the key alone, so `c` has to mean
/// the same thing in whichever menu is open.
pub(in crate::view::panels::popover) fn push_copy_path_entries(
    items: &mut Vec<ContextMenuItem>,
    host: &PopoverHost,
    repo_id: RepoId,
    path: &std::path::Path,
    relative_shortcut: Option<SharedString>,
) {
    // Offered only when the workdir join actually resolves. Falling back to the
    // repo-relative text would put identical content behind two entries whose
    // labels promise different things.
    if let Ok(absolute) = host.resolve_workdir_path(repo_id, path) {
        items.push(ContextMenuItem::Entry {
            label: "Copy absolute path".into(),
            icon: Some("icons/copy.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::CopyText {
                text: path_text_for_copy(&absolute),
            }),
        });
    }
    items.push(ContextMenuItem::Entry {
        label: "Copy relative path".into(),
        icon: Some("icons/copy.svg".into()),
        shortcut: relative_shortcut,
        disabled: false,
        action: Box::new(ContextMenuAction::CopyText {
            text: path_text_for_copy(path),
        }),
    });
}

pub(super) fn active_branch_tracking_upstream_name(host: &PopoverHost) -> Option<String> {
    let repo_id = host.active_repo_id()?;
    let repo = host.state.repos.iter().find(|repo| repo.id == repo_id)?;
    let Loadable::Ready(head) = &repo.head_branch else {
        return None;
    };
    let Loadable::Ready(branches) = &repo.branches else {
        return None;
    };

    branches
        .iter()
        .find(|branch| branch.name == *head)
        .and_then(|branch| branch.upstream.as_ref())
        .map(|upstream| format!("{}/{}", upstream.remote, upstream.branch))
}

pub(super) fn action_menu_title(
    base: &'static str,
    tracking_branch_name: Option<&str>,
) -> SharedString {
    // Gettext-style: the base word ("Pull"/"Push") localizes; the tracking
    // branch name stays verbatim, and `.localized()` at render passes the
    // already-translated title through unchanged.
    match tracking_branch_name {
        Some(name) => format!("{} {name}", crate::i18n::tr_en(base)).into(),
        None => crate::i18n::tr_en(base),
    }
}

pub(super) fn context_menu_entry_debug_selector(label: &str) -> String {
    let mut slug = String::with_capacity(label.len());
    let mut previous_was_separator = true;

    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            slug.push('_');
            previous_was_separator = true;
        }
    }

    while slug.ends_with('_') {
        slug.pop();
    }

    if slug.is_empty() {
        "context_menu_entry".to_string()
    } else {
        format!("context_menu_{slug}")
    }
}

/// Left offset of one submenu nesting level in the flattened menu rows.
pub(super) const CONTEXT_MENU_SUBMENU_INDENT_PX: f32 = 20.0;

pub(super) fn context_menu_entry_action_at(
    rows: &ContextMenuRows,
    ix: usize,
) -> Option<ContextMenuAction> {
    match rows.get(ix) {
        Some((ContextMenuItem::Entry { action, .. }, _)) => Some((**action).clone()),
        _ => None,
    }
}

pub(super) fn context_menu_entry_tooltip(action: &ContextMenuAction) -> Option<SharedString> {
    match action {
        ContextMenuAction::UseCommitMessage { message } => {
            let text = message.trim();
            (!text.is_empty()).then(|| text.to_owned().into())
        }
        _ => None,
    }
}

pub(in super::super::super) fn context_menu_activate_entry_ix(
    rows: &ContextMenuRows,
    selected_ix: Option<usize>,
) -> Option<usize> {
    selected_ix
        .filter(|&ix| rows.is_selectable(ix))
        .or_else(|| rows.first_selectable())
}

pub(in super::super::super) fn context_menu_shortcut_entry_ix(
    rows: &ContextMenuRows,
    key: &str,
) -> Option<usize> {
    if key.chars().count() != 1 {
        return None;
    }

    rows.iter().enumerate().find_map(|(ix, (item, _))| {
        let ContextMenuItem::Entry {
            shortcut, disabled, ..
        } = item
        else {
            return None;
        };
        if *disabled {
            return None;
        }
        let shortcut = shortcut.as_ref()?;
        let shortcut_key = shortcut
            .as_ref()
            .rsplit('+')
            .next()
            .unwrap_or(shortcut.as_ref());
        shortcut_key.eq_ignore_ascii_case(key).then_some(ix)
    })
}

pub(super) fn interactive_rebase_action_menu_model(
    ix: usize,
    can_squash: bool,
    can_drop: bool,
    pick_locked: bool,
) -> ContextMenuModel {
    let mut items = vec![
        ContextMenuItem::Entry {
            label: "pick".into(),
            icon: None,
            shortcut: None,
            // A squash run's target is auto-managed: it stays Reword while
            // commits squash into it, and a dropped entry in target position
            // would re-promote the instant it were picked back — so `pick`
            // locks for the position rather than turning into a surprise
            // reword. Demote a target by dropping it or removing the squash.
            disabled: pick_locked,
            action: Box::new(ContextMenuAction::SetInteractiveRebaseAction {
                ix,
                action: InteractiveRebaseAction::Pick,
            }),
        },
        ContextMenuItem::Entry {
            label: "reword".into(),
            icon: None,
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::SetInteractiveRebaseAction {
                ix,
                action: InteractiveRebaseAction::Reword,
            }),
        },
        ContextMenuItem::Entry {
            label: "drop".into(),
            icon: None,
            shortcut: None,
            disabled: !can_drop,
            action: Box::new(ContextMenuAction::SetInteractiveRebaseAction {
                ix,
                action: InteractiveRebaseAction::Drop,
            }),
        },
    ];
    if can_squash {
        items.push(ContextMenuItem::Entry {
            label: "squash".into(),
            icon: None,
            shortcut: None,
            disabled: !can_squash,
            action: Box::new(ContextMenuAction::SetInteractiveRebaseAction {
                ix,
                action: InteractiveRebaseAction::Squash,
            }),
        });
    }
    ContextMenuModel::new(items)
}

pub(super) fn interactive_rebase_autosquash_menu_model() -> ContextMenuModel {
    // Auto Squash is a one-shot action: pick a strategy and it folds the
    // duplicate-message commits, no persisted on/off state to display.
    let entry = |mode: AutosquashMode| ContextMenuItem::Entry {
        label: mode.label().into(),
        icon: None,
        shortcut: None,
        disabled: false,
        action: Box::new(ContextMenuAction::SetInteractiveRebaseAutosquashMode { mode }),
    };
    ContextMenuModel::new(vec![
        ContextMenuItem::Header("Auto Squash".into()),
        entry(AutosquashMode::ToTop),
        entry(AutosquashMode::Neighbor),
        entry(AutosquashMode::ToBottom),
    ])
}
