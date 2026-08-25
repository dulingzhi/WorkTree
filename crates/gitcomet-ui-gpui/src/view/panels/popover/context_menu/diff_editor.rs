use super::*;

use crate::view::shortcut_labels::secondary_shortcut;

#[allow(clippy::too_many_arguments)]
pub(super) fn model(
    this: &PopoverHost,
    repo_id: RepoId,
    area: DiffArea,
    path: &Option<std::path::PathBuf>,
    hunk_patch: &Option<String>,
    hunks_count: usize,
    lines_patch: &Option<String>,
    discard_lines_patch: &Option<String>,
    lines_count: usize,
    copy_text: &Option<String>,
    copy_target: Option<(usize, DiffTextRegion)>,
) -> ContextMenuModel {
    // jj compat: the four write entries (stage/unstage line and hunks,
    // discard line/hunks) are unrouted on read-only repos; the read entries
    // (open/copy) below stay.
    let (staging_supported, worktree_writes_supported) = this
        .state
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .map(|repo| (repo.capabilities.staging, !repo.capabilities.read_only))
        .unwrap_or((true, true));

    let title: SharedString = path
        .as_ref()
        .and_then(|p| {
            p.file_name()
                .and_then(|name| name.to_str().map(ToOwned::to_owned))
                .map(Into::into)
        })
        .unwrap_or_else(|| "Diff".into());

    let mut items = vec![ContextMenuItem::Header(title.into())];
    if let Some(path) = path {
        items.push(ContextMenuItem::Label(
            components::ContextMenuText::path_single_line(path.display().to_string()),
        ));
    }
    items.push(ContextMenuItem::Separator);

    let (line_label, line_icon, line_shortcut, line_reverse) = match area {
        DiffArea::Unstaged => ("Stage line", "icons/plus.svg", Some("S"), false),
        DiffArea::Staged => ("Unstage line", "icons/minus.svg", Some("U"), true),
    };
    if staging_supported {
        items.push(ContextMenuItem::Entry {
            label: if lines_count > 1 {
                format!("{line_label}s ({lines_count})").into()
            } else {
                line_label.into()
            },
            icon: Some(line_icon.into()),
            shortcut: line_shortcut.map(Into::into),
            disabled: lines_patch.is_none(),
            action: Box::new(ContextMenuAction::ApplyIndexPatch {
                repo_id,
                patch: lines_patch.clone().unwrap_or_default(),
                reverse: line_reverse,
            }),
        });
    }

    if area == DiffArea::Unstaged && worktree_writes_supported {
        items.push(ContextMenuItem::Entry {
            label: if lines_count > 1 {
                crate::i18n::t!("cm.diff.discard_lines_count", count = lines_count)
                    .into_owned()
                    .into()
            } else {
                "Discard line".into()
            },
            icon: Some("icons/refresh.svg".into()),
            shortcut: Some("D".into()),
            disabled: discard_lines_patch.is_none(),
            action: Box::new(ContextMenuAction::ApplyWorktreePatch {
                repo_id,
                patch: discard_lines_patch.clone().unwrap_or_default(),
                reverse: true,
            }),
        });
    }

    items.push(ContextMenuItem::Separator);

    let (hunk_label, hunk_icon, hunk_reverse) = match area {
        DiffArea::Unstaged => ("Stage hunk", "icons/plus.svg", false),
        DiffArea::Staged => ("Unstage hunk", "icons/minus.svg", true),
    };
    if staging_supported {
        items.push(ContextMenuItem::Entry {
            label: if hunks_count > 1 {
                match area {
                    DiffArea::Unstaged => {
                        crate::i18n::t!("cm.diff.stage_hunks_count", count = hunks_count)
                            .into_owned()
                    }
                    DiffArea::Staged => {
                        crate::i18n::t!("cm.diff.unstage_hunks_count", count = hunks_count)
                            .into_owned()
                    }
                }
                .into()
            } else {
                hunk_label.into()
            },
            icon: Some(hunk_icon.into()),
            shortcut: None,
            disabled: hunk_patch.is_none(),
            action: Box::new(ContextMenuAction::ApplyIndexPatch {
                repo_id,
                patch: hunk_patch.clone().unwrap_or_default(),
                reverse: hunk_reverse,
            }),
        });
    }

    if area == DiffArea::Unstaged && worktree_writes_supported {
        items.push(ContextMenuItem::Entry {
            label: if hunks_count > 1 {
                crate::i18n::t!("cm.diff.discard_hunks_count", count = hunks_count)
                    .into_owned()
                    .into()
            } else {
                "Discard hunk".into()
            },
            icon: Some("icons/refresh.svg".into()),
            shortcut: None,
            disabled: hunk_patch.is_none(),
            action: Box::new(ContextMenuAction::ApplyWorktreePatch {
                repo_id,
                patch: hunk_patch.clone().unwrap_or_default(),
                reverse: true,
            }),
        });
    }

    items.push(ContextMenuItem::Separator);
    if let Some(path) = path {
        items.push(ContextMenuItem::Entry {
            label: "Open file".into(),
            icon: Some("icons/file.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::OpenFile {
                repo_id,
                path: path.clone(),
            }),
        });
        items.push(ContextMenuItem::Entry {
            label: "Open file location".into(),
            icon: Some("icons/folder.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::OpenFileLocation {
                repo_id,
                path: path.clone(),
            }),
        });
        if crate::external_editor::configured_setting().is_some() {
            items.push(ContextMenuItem::Entry {
                label: "Open in code editor".into(),
                icon: Some("icons/open_external.svg".into()),
                shortcut: Some(secondary_shortcut("E").into()),
                disabled: false,
                action: Box::new(ContextMenuAction::OpenInCodeEditor {
                    repo_id: Some(repo_id),
                    path: path.clone(),
                }),
            });
        }
        items.push(ContextMenuItem::Separator);
    }
    items.push(ContextMenuItem::Entry {
        label: "Copy".into(),
        icon: Some("icons/copy.svg".into()),
        shortcut: Some("C".into()),
        disabled: copy_text
            .as_ref()
            .map(|text| text.trim().is_empty())
            .unwrap_or(copy_target.is_none()),
        action: Box::new(match copy_text {
            Some(text) => ContextMenuAction::CopyDiffSelection { text: text.clone() },
            None => {
                let (visible_ix, region) = copy_target.unwrap_or((0, DiffTextRegion::Inline));
                ContextMenuAction::CopyDiffText { visible_ix, region }
            }
        }),
    });

    ContextMenuModel::new(items)
}
