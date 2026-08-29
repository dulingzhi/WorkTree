use super::*;

use crate::view::shortcut_labels::secondary_shortcut;

pub(super) fn model(host: &PopoverHost, repo_id: RepoId) -> ContextMenuModel {
    let workdir = host
        .state
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .map(|repo| repo.spec.workdir.clone());
    model_for_state(host.state.as_ref(), repo_id, workdir)
}

fn model_for_state(
    state: &AppState,
    repo_id: RepoId,
    workdir: Option<std::path::PathBuf>,
) -> ContextMenuModel {
    let Some(repo_ix) = state.repos.iter().position(|repo| repo.id == repo_id) else {
        return ContextMenuModel::new(Vec::new());
    };

    let close_to_right: Vec<RepoId> = state
        .repos
        .iter()
        .skip(repo_ix + 1)
        .map(|repo| repo.id)
        .collect();
    let close_others: Vec<RepoId> = state
        .repos
        .iter()
        .filter_map(|repo| (repo.id != repo_id).then_some(repo.id))
        .collect();
    let activate_after_close_to_right = state
        .active_repo
        .filter(|active_repo| close_to_right.contains(active_repo))
        .map(|_| repo_id);

    let mut items = vec![ContextMenuItem::Entry {
        label: "Activate".into(),
        icon: Some("icons/check.svg".into()),
        shortcut: None,
        disabled: state.active_repo == Some(repo_id),
        action: Box::new(ContextMenuAction::ActivateRepo { repo_id }),
    }];

    items.push(ContextMenuItem::Entry {
        label: "Repository settings…".into(),
        icon: Some("icons/settings.svg".into()),
        shortcut: None,
        disabled: false,
        action: Box::new(ContextMenuAction::OpenPopover {
            kind: PopoverKind::RepoSettingsPrompt { repo_id },
        }),
    });

    if let Some(ref workdir) = workdir {
        items.push(ContextMenuItem::Separator);
        items.push(ContextMenuItem::Entry {
            label: "Open repository location".into(),
            icon: Some("icons/folder.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::OpenRepositoryLocation {
                path: workdir.clone(),
            }),
        });
    }

    if let Some(ref workdir) = workdir {
        let configured = crate::external_editor::configured_setting();
        if configured.is_some() {
            items.push(ContextMenuItem::Entry {
                label: "Open in code editor".into(),
                icon: Some("icons/open_external.svg".into()),
                shortcut: Some(secondary_shortcut("Shift+E").into()),
                disabled: false,
                action: Box::new(ContextMenuAction::OpenInCodeEditor {
                    repo_id: None,
                    path: workdir.clone(),
                }),
            });
        }

        let detected = crate::external_editor::detect_external_editors_cached();
        let tool_entries = detected_editor_entries(workdir, &detected, configured.as_ref());
        if !tool_entries.is_empty() {
            items.push(ContextMenuItem::Submenu {
                id: "repo_tab_external_tools".into(),
                label: "Open in external tool".into(),
                icon: Some("icons/open_external.svg".into()),
                children: tool_entries,
            });
        }
    }

    items.push(ContextMenuItem::Separator);
    items.extend_from_slice(&[
        ContextMenuItem::Entry {
            label: "Close".into(),
            icon: Some("icons/repo_tab_close.svg".into()),
            shortcut: None,
            disabled: false,
            action: Box::new(ContextMenuAction::CloseRepo { repo_id }),
        },
        ContextMenuItem::Entry {
            label: "Close repositories to the right".into(),
            icon: Some("icons/arrow_right.svg".into()),
            shortcut: None,
            disabled: close_to_right.is_empty(),
            action: Box::new(ContextMenuAction::CloseRepos {
                repo_ids: close_to_right,
                activate_after: activate_after_close_to_right,
            }),
        },
        ContextMenuItem::Entry {
            label: "Close other repositories".into(),
            icon: Some("icons/swap.svg".into()),
            shortcut: None,
            disabled: close_others.is_empty(),
            action: Box::new(ContextMenuAction::CloseRepos {
                repo_ids: close_others,
                activate_after: Some(repo_id),
            }),
        },
    ]);

    ContextMenuModel::new(items).with_shortcut_keycaps()
}

/// One "Open in {tool}" entry per distinct editor detected on this machine,
/// so a repository can be handed to VS Code, Zed, a JetBrains IDE, … straight
/// from its tab. One entry per editor *id*: the same tool is often detected
/// twice (the PATH launcher and the macOS app bundle), which would read as a
/// duplicate row. The editor the user configured is skipped entirely — the
/// shortcut-bound "Open in code editor" entry already targets that tool.
///
/// Visual Studio is the exception on targets: `devenv` only makes sense with
/// something to open, so its entries carry the repository's solution file
/// when one is at hand, the folder itself for a CMake project
/// (`CMakeLists.txt` — devenv's Open Folder mode), and stay hidden otherwise.
fn detected_editor_entries(
    workdir: &std::path::Path,
    detected: &[crate::external_editor::DetectedExternalEditor],
    configured: Option<&worktree_state::session::ExternalCodeEditorSetting>,
) -> Vec<ContextMenuItem> {
    let configured_id = match configured {
        Some(worktree_state::session::ExternalCodeEditorSetting::Detected { id, .. }) => {
            Some(id.as_str())
        }
        _ => None,
    };
    let mut seen_ids = std::collections::BTreeSet::new();
    detected
        .iter()
        .filter(|editor| {
            if Some(editor.id.as_str()) == configured_id {
                return false;
            }
            seen_ids.insert(editor.id.clone())
        })
        .filter_map(|editor| {
            let path = if editor.id.starts_with("visual-studio-") {
                visual_studio_target_for(workdir)?
            } else {
                workdir.to_path_buf()
            };
            Some(ContextMenuItem::Entry {
                label: crate::i18n::t!("cm.open_in", name = editor.label.clone())
                    .to_string()
                    .into(),
                icon: Some("icons/open_external.svg".into()),
                shortcut: None,
                disabled: false,
                action: Box::new(ContextMenuAction::OpenInDetectedEditor {
                    repo_id: None,
                    path,
                    id: editor.id.clone(),
                    editor_path: editor.path.clone(),
                }),
            })
        })
        .collect()
}

/// The path a Visual Studio install should open for this repository: the
/// first solution file (devenv opens it as the startup solution), or the
/// workdir itself when a `CMakeLists.txt` marks a CMake project and there is
/// no solution. `None` when neither applies — there is nothing for devenv
/// to do.
///
/// Memoized per workdir: the menu model is rebuilt on every repaint while
/// the menu is open, and the search walks the workdir plus each first-level
/// subdirectory.
fn visual_studio_target_for(workdir: &std::path::Path) -> Option<std::path::PathBuf> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<FxHashMap<std::path::PathBuf, Option<std::path::PathBuf>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut guard = cache.lock().expect("visual studio target cache poisoned");
    guard
        .entry(workdir.to_path_buf())
        .or_insert_with(|| {
            find_solution_file(workdir)
                .or_else(|| has_cmake_lists(workdir).then(|| workdir.to_path_buf()))
        })
        .clone()
}

/// The repository's solution file: one at the workdir root, else the first
/// (sorted, so stable) one directly inside a subdirectory — solutions
/// commonly sit in `Build/` or `src/` rather than at the root.
fn find_solution_file(workdir: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Some(solution) = first_file_with_extension(workdir, "sln") {
        return Some(solution);
    }
    let mut subdirs: Vec<std::path::PathBuf> = std::fs::read_dir(workdir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect();
    subdirs.sort();
    subdirs
        .into_iter()
        .find_map(|dir| first_file_with_extension(&dir, "sln"))
}

fn first_file_with_extension(dir: &std::path::Path, extension: &str) -> Option<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        })
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Whether a `CMakeLists.txt` marks this directory (or a first-level
/// subdirectory of it) as a CMake project.
fn has_cmake_lists(workdir: &std::path::Path) -> bool {
    if workdir.join("CMakeLists.txt").is_file() {
        return true;
    }
    std::fs::read_dir(workdir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .any(|entry| entry.path().join("CMakeLists.txt").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use worktree_core::domain::RepoSpec;
    use worktree_state::model::RepoState;

    fn state_with_repo_tabs(active_repo: RepoId, repo_count: u64) -> AppState {
        let mut state = AppState {
            active_repo: Some(active_repo),
            ..AppState::default()
        };
        for ix in 1..=repo_count {
            state.repos.push(RepoState::new_opening(
                RepoId(ix),
                RepoSpec {
                    workdir: PathBuf::from(format!("/tmp/repo-tab-menu-{ix}")),
                },
            ));
        }
        state
    }

    fn entry_with_label<'a>(model: &'a ContextMenuModel, expected: &str) -> &'a ContextMenuItem {
        model
            .items
            .iter()
            .find(|item| {
                matches!(
                    item,
                    ContextMenuItem::Entry { label, .. } if label.as_ref() == expected
                )
            })
            .unwrap_or_else(|| panic!("expected {expected} menu item"))
    }

    fn entry_action<'a>(
        model: &'a ContextMenuModel,
        expected: &str,
    ) -> (bool, &'a ContextMenuAction) {
        let ContextMenuItem::Entry {
            disabled, action, ..
        } = entry_with_label(model, expected)
        else {
            panic!("expected {expected} menu item to be an entry");
        };
        (*disabled, action.as_ref())
    }

    #[test]
    fn activate_entry_activates_inactive_repo_tab() {
        let state = state_with_repo_tabs(RepoId(1), 3);
        let model = model_for_state(&state, RepoId(2), None);

        let (disabled, action) = entry_action(&model, "Activate");

        assert!(!disabled);
        assert!(matches!(
            action,
            ContextMenuAction::ActivateRepo { repo_id } if *repo_id == RepoId(2)
        ));
    }

    #[test]
    fn activate_entry_is_disabled_for_active_repo_tab() {
        let state = state_with_repo_tabs(RepoId(2), 3);
        let model = model_for_state(&state, RepoId(2), None);

        let (disabled, action) = entry_action(&model, "Activate");

        assert!(disabled);
        assert!(matches!(
            action,
            ContextMenuAction::ActivateRepo { repo_id } if *repo_id == RepoId(2)
        ));
    }

    #[test]
    fn close_repo_entry_uses_repo_tab_close_icon() {
        let state = state_with_repo_tabs(RepoId(1), 3);
        let model = model_for_state(&state, RepoId(2), None);

        let ContextMenuItem::Entry {
            icon,
            disabled,
            action,
            ..
        } = entry_with_label(&model, "Close")
        else {
            panic!("expected Close menu item to be an entry");
        };

        assert_eq!(
            icon.as_ref().map(|icon| icon.as_ref()),
            Some("icons/repo_tab_close.svg")
        );
        assert!(!disabled);
        assert!(matches!(
            action.as_ref(),
            ContextMenuAction::CloseRepo { repo_id } if *repo_id == RepoId(2)
        ));
    }

    #[test]
    fn open_repository_location_entry_targets_the_repository_workdir() {
        let state = state_with_repo_tabs(RepoId(1), 3);
        let workdir = PathBuf::from("/tmp/repo-tab-menu-2");
        let model = model_for_state(&state, RepoId(2), Some(workdir.clone()));

        let (disabled, action) = entry_action(&model, "Open repository location");

        assert!(!disabled);
        assert!(matches!(
            action,
            ContextMenuAction::OpenRepositoryLocation { path } if path == &workdir
        ));
    }

    #[test]
    fn repo_tab_menu_uses_shared_shortcut_keycaps() {
        let state = state_with_repo_tabs(RepoId(1), 3);
        let model = model_for_state(
            &state,
            RepoId(2),
            Some(PathBuf::from("/tmp/repo-tab-menu-2")),
        );

        assert!(model.shortcut_keycaps);
    }

    #[test]
    fn close_right_entry_targets_only_repos_to_the_right() {
        let state = state_with_repo_tabs(RepoId(3), 3);
        let model = model_for_state(&state, RepoId(2), None);

        let (disabled, action) = entry_action(&model, "Close repositories to the right");

        assert!(!disabled);
        let ContextMenuAction::CloseRepos {
            repo_ids,
            activate_after,
        } = action
        else {
            panic!("expected Close repositories to the right to close multiple repos");
        };
        assert_eq!(repo_ids, &vec![RepoId(3)]);
        assert_eq!(*activate_after, Some(RepoId(2)));
    }

    #[test]
    fn close_right_entry_is_disabled_for_last_repo_tab() {
        let state = state_with_repo_tabs(RepoId(2), 3);
        let model = model_for_state(&state, RepoId(3), None);

        let (disabled, action) = entry_action(&model, "Close repositories to the right");

        assert!(disabled);
        let ContextMenuAction::CloseRepos {
            repo_ids,
            activate_after,
        } = action
        else {
            panic!("expected Close repositories to the right to close multiple repos");
        };
        assert!(repo_ids.is_empty());
        assert_eq!(*activate_after, None);
    }

    #[test]
    fn close_other_repositories_entry_targets_every_repo_except_selected() {
        let state = state_with_repo_tabs(RepoId(1), 3);
        let model = model_for_state(&state, RepoId(2), None);

        let (disabled, action) = entry_action(&model, "Close other repositories");

        assert!(!disabled);
        let ContextMenuAction::CloseRepos {
            repo_ids,
            activate_after,
        } = action
        else {
            panic!("expected Close other repositories to close multiple repos");
        };
        assert_eq!(repo_ids, &vec![RepoId(1), RepoId(3)]);
        assert_eq!(*activate_after, Some(RepoId(2)));
    }

    #[test]
    fn close_other_repositories_entry_is_disabled_for_single_repo_tab() {
        let state = state_with_repo_tabs(RepoId(1), 1);
        let model = model_for_state(&state, RepoId(1), None);

        let (disabled, action) = entry_action(&model, "Close other repositories");

        assert!(disabled);
        let ContextMenuAction::CloseRepos {
            repo_ids,
            activate_after,
        } = action
        else {
            panic!("expected Close other repositories to close multiple repos");
        };
        assert!(repo_ids.is_empty());
        assert_eq!(*activate_after, Some(RepoId(1)));
    }

    #[test]
    fn missing_repo_tab_returns_empty_menu_model() {
        let state = state_with_repo_tabs(RepoId(1), 3);

        assert!(model_for_state(&state, RepoId(99), None).items.is_empty());
    }

    fn detected_editor(
        id: &str,
        label: &str,
        path: &str,
    ) -> crate::external_editor::DetectedExternalEditor {
        crate::external_editor::detected_editor_for_tests(id, label, PathBuf::from(path))
    }

    #[test]
    fn detected_editor_entries_open_each_tool_at_the_workdir() {
        let workdir = PathBuf::from("/tmp/repo-tab-menu-2");
        let detected = vec![
            detected_editor(
                "vscode",
                "Visual Studio Code",
                "/Applications/Visual Studio Code.app",
            ),
            detected_editor("zed", "Zed", "/Applications/Zed.app"),
        ];

        let entries = detected_editor_entries(&workdir, &detected, None);

        let labels: Vec<&str> = entries
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { label, .. } => label.as_ref(),
                _ => panic!("expected only entries"),
            })
            .collect();
        assert_eq!(
            labels,
            vec!["Open in Visual Studio Code", "Open in Zed"],
            "tests pin the locale to en, so cm.open_in renders its English template"
        );

        let ContextMenuItem::Entry { action, .. } = &entries[1] else {
            panic!("expected an entry");
        };
        assert!(matches!(
            action.as_ref(),
            ContextMenuAction::OpenInDetectedEditor {
                repo_id: None,
                path,
                id,
                editor_path,
            } if path == &workdir && id == "zed" && editor_path.as_os_str() == "/Applications/Zed.app"
        ));
    }

    #[test]
    fn detected_editor_entries_skip_the_configured_editor() {
        let workdir = PathBuf::from("/tmp/repo-tab-menu-2");
        // A different path than the detected row: the skip keys on the editor
        // id, since detection and configuration can disagree on the install.
        let configured = worktree_state::session::ExternalCodeEditorSetting::Detected {
            id: "vscode".to_string(),
            path: PathBuf::from("/usr/local/bin/code"),
        };
        let detected = vec![
            detected_editor(
                "vscode",
                "Visual Studio Code",
                "/Applications/Visual Studio Code.app",
            ),
            detected_editor("zed", "Zed", "/Applications/Zed.app"),
        ];

        let entries = detected_editor_entries(&workdir, &detected, Some(&configured));

        let labels: Vec<&str> = entries
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { label, .. } => label.as_ref(),
                _ => panic!("expected only entries"),
            })
            .collect();
        assert_eq!(
            labels,
            vec!["Open in Zed"],
            "the configured install is reachable through the Open in code editor entry"
        );
    }

    #[test]
    fn detected_editor_entries_list_all_tools_for_a_custom_configuration() {
        let workdir = PathBuf::from("/tmp/repo-tab-menu-2");
        let configured = worktree_state::session::ExternalCodeEditorSetting::Custom {
            executable: PathBuf::from("/usr/bin/editor"),
            arguments: None,
        };
        let detected = vec![
            detected_editor(
                "vscode",
                "Visual Studio Code",
                "/Applications/Visual Studio Code.app",
            ),
            detected_editor("zed", "Zed", "/Applications/Zed.app"),
        ];

        let entries = detected_editor_entries(&workdir, &detected, Some(&configured));

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn detected_editor_entries_collapse_double_detected_tools_into_one_row() {
        let workdir = PathBuf::from("/tmp/repo-tab-menu-2");
        // Xcode is found twice on a stock macOS install: the `xed` launcher
        // on PATH and the app bundle. Both rows share an id, so only the
        // first survives.
        let detected = vec![
            detected_editor("xcode", "Xcode", "/usr/bin/xed"),
            detected_editor("xcode", "Xcode", "/Applications/Xcode.app"),
            detected_editor("zed", "Zed", "/Applications/Zed.app"),
        ];

        let entries = detected_editor_entries(&workdir, &detected, None);

        let labels: Vec<&str> = entries
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { label, .. } => label.as_ref(),
                _ => panic!("expected only entries"),
            })
            .collect();
        assert_eq!(labels, vec!["Open in Xcode", "Open in Zed"]);
    }

    fn workdir_fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "worktree-repo-tab-vs-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn visual_studio_editor() -> crate::external_editor::DetectedExternalEditor {
        detected_editor(
            "visual-studio-2022-community",
            "Visual Studio 2022 Community",
            "C:/Program Files/Microsoft Visual Studio/2022/Community/Common7/IDE/devenv.exe",
        )
    }

    fn entry_paths(entries: &[ContextMenuItem]) -> Vec<std::path::PathBuf> {
        entries
            .iter()
            .filter_map(|item| match item {
                ContextMenuItem::Entry { action, .. } => match action.as_ref() {
                    ContextMenuAction::OpenInDetectedEditor { path, .. } => Some(path.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    #[test]
    fn visual_studio_entries_open_the_repository_solution() {
        let workdir = workdir_fixture("solution");
        std::fs::write(workdir.join("MyRepo.sln"), "").expect("write solution");
        let detected = vec![visual_studio_editor()];

        let entries = detected_editor_entries(&workdir, &detected, None);

        assert_eq!(entry_paths(&entries), vec![workdir.join("MyRepo.sln")]);
    }

    #[test]
    fn visual_studio_entries_find_solutions_in_a_subdirectory() {
        let workdir = workdir_fixture("solution-subdir");
        std::fs::create_dir_all(workdir.join("Build")).expect("create Build dir");
        std::fs::write(workdir.join("Build/MyRepo.sln"), "").expect("write solution");
        let detected = vec![visual_studio_editor()];

        let entries = detected_editor_entries(&workdir, &detected, None);

        assert_eq!(
            entry_paths(&entries),
            vec![workdir.join("Build/MyRepo.sln")]
        );
    }

    #[test]
    fn visual_studio_entries_open_the_folder_for_cmake_projects() {
        let workdir = workdir_fixture("cmake");
        std::fs::write(workdir.join("CMakeLists.txt"), "").expect("write cmake lists");
        let detected = vec![visual_studio_editor()];

        let entries = detected_editor_entries(&workdir, &detected, None);

        assert_eq!(entry_paths(&entries), vec![workdir.clone()]);
    }

    #[test]
    fn visual_studio_entries_prefer_the_solution_over_the_cmake_folder() {
        let workdir = workdir_fixture("solution-and-cmake");
        std::fs::write(workdir.join("MyRepo.sln"), "").expect("write solution");
        std::fs::write(workdir.join("CMakeLists.txt"), "").expect("write cmake lists");
        let detected = vec![visual_studio_editor()];

        let entries = detected_editor_entries(&workdir, &detected, None);

        assert_eq!(entry_paths(&entries), vec![workdir.join("MyRepo.sln")]);
    }

    #[test]
    fn visual_studio_entries_hide_without_a_solution_or_cmake_project() {
        let workdir = workdir_fixture("nothing-to-open");
        let detected = vec![
            visual_studio_editor(),
            detected_editor("zed", "Zed", "/Applications/Zed.app"),
        ];

        let entries = detected_editor_entries(&workdir, &detected, None);

        // Visual Studio drops out; the tools that take a plain folder stay.
        let labels: Vec<&str> = entries
            .iter()
            .map(|item| match item {
                ContextMenuItem::Entry { label, .. } => label.as_ref(),
                _ => panic!("expected only entries"),
            })
            .collect();
        assert_eq!(labels, vec!["Open in Zed"]);
    }
}
