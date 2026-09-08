use super::support::{
    cmd_copy_remote_to_merged_and_exit_success, cmd_delete_merged_and_exit_failure,
    cmd_dump_base_size_and_copy_remote, cmd_dump_stage_paths_and_copy_remote,
    cmd_dump_stage_paths_and_exit_failure, cmd_exit_success,
    cmd_same_size_content_change_and_exit_failure, cmd_write_cli_to_merged,
    cmd_write_gui_to_merged, cmd_write_unresolved_markers_and_exit_success, normalize_stage_var,
    read_stage_env_vars, require_git_shell_for_status_integration_tests, run_git,
    set_repo_local_mergetool_cmd_with_consent, setup_both_added_text_conflict,
    setup_both_modified_text_conflict, stage_var_to_fs_path,
};
#[cfg(unix)]
use super::support::{cmd_write_cmd_to_merged, git_path_arg, make_executable};
use std::fs;
use std::path::{Path, PathBuf};
use worktree_core::domain::{FileConflictKind, FileStatusKind};
use worktree_core::error::ErrorKind;
use worktree_core::external_merge_tool::ExternalMergeToolSelection;
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;
use worktree_test_support::set_fixed_mtime;

#[test]
fn launch_mergetool_trust_exit_false_detects_same_size_content_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    // Normalize pre-tool mtime to a fixed timestamp so metadata-only checks
    // cannot detect the edit when the command restores mtime.
    set_fixed_mtime(&repo.join("a.txt"));

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_same_size_content_change_and_exit_failure(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "false"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(1));

    let on_disk = fs::read(repo.join("a.txt")).unwrap();
    assert!(!on_disk.is_empty());
    assert_eq!(on_disk[0], b'R');
    assert_eq!(result.merged_contents.as_deref(), Some(on_disk.as_slice()));

    let status = opened.status().unwrap();
    assert!(status.unstaged.iter().all(|e| e.path != Path::new("a.txt")));
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Modified),
        "expected staged resolution after content-changing mergetool run, got {status:?}"
    );
}

#[test]
fn launch_mergetool_reflects_config_written_after_backend_open() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_copy_remote_to_merged_and_exit_success(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "theirs\n");

    let status = opened.status().unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .all(|entry| entry.path != Path::new("a.txt"))
    );
    assert!(
        status
            .staged
            .iter()
            .any(|entry| entry.path == Path::new("a.txt") && entry.kind == FileStatusKind::Modified),
        "expected mergetool resolution after config refresh, got {status:?}"
    );
}

#[test]
fn launch_mergetool_trust_exit_false_requires_content_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_exit_success());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "false"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(!result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));
    assert!(result.merged_contents.is_none());

    let status = opened.status().unwrap();
    assert!(
        status
            .staged
            .iter()
            .all(|entry| entry.path != Path::new("a.txt")),
        "unexpected staged resolution when mergetool did not change output: {status:?}"
    );
    let conflict_entry = status
        .unstaged
        .iter()
        .find(|entry| entry.path == Path::new("a.txt"))
        .expect("conflict should remain unresolved");
    assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
    assert_eq!(
        conflict_entry.conflict,
        Some(FileConflictKind::BothModified)
    );
}

#[test]
fn launch_mergetool_trust_exit_false_detects_deleted_output_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_delete_merged_and_exit_failure());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "false"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(1));
    assert!(
        result.merged_contents.is_none(),
        "deleted-output resolution should not return merged file bytes"
    );
    assert!(
        !repo.join("a.txt").exists(),
        "mergetool delete output should remove the worktree file"
    );

    let status = opened.status().unwrap();
    assert!(
        status.unstaged.iter().all(|e| e.path != Path::new("a.txt")),
        "expected conflict to clear from unstaged after delete-output mergetool run, got {status:?}"
    );
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Deleted),
        "expected delete-output mergetool run to stage file deletion, got {status:?}"
    );
}

#[test]
fn launch_mergetool_from_git_config_preference_keeps_existing_behavior() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_copy_remote_to_merged_and_exit_success(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );
}

#[test]
fn launch_mergetool_builtin_preference_overrides_merge_tool_config() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    // `merge.tool` points at a tool with no command; the Builtin preference
    // must win over it.
    run_git(repo, &["config", "merge.tool", "unconfigured-tool"]);
    run_git(repo, &["config", "merge.tool2", "unused"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_copy_remote_to_merged_and_exit_success(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::Builtin {
                id: "fake".to_string(),
                path: None,
            },
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "theirs\n");
}

#[test]
fn launch_mergetool_custom_preference_bypasses_git_config() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    // No merge.tool, no mergetool.* keys at all — the preference alone drives
    // the launch, without touching git config.
    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::Custom {
                command: cmd_copy_remote_to_merged_and_exit_success().to_string(),
                trust_exit_code: true,
            },
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "worktree-custom");
    assert_eq!(result.output.exit_code, Some(0));
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "theirs\n");

    let status = opened.status().unwrap();
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Modified),
        "expected custom-preference mergetool run to stage the resolution, got {status:?}"
    );
}

#[test]
fn launch_mergetool_custom_preference_empty_command_errors() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::Custom {
                command: "   ".to_string(),
                trust_exit_code: true,
            },
        )
        .expect_err("empty custom command must be rejected");

    match err.kind() {
        ErrorKind::Backend(msg) => {
            assert!(
                msg.contains("Custom merge command is empty"),
                "unexpected backend error: {msg}"
            );
        }
        other => panic!("expected Backend error, got {other:?}"),
    }
}

#[test]
fn launch_mergetool_custom_preference_trust_exit_code_false_requires_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::Custom {
                command: cmd_exit_success().to_string(),
                trust_exit_code: false,
            },
        )
        .unwrap();
    assert!(
        !result.success,
        "exit 0 without an output change must not count as resolved"
    );
    assert_eq!(result.tool_name, "worktree-custom");
    assert_eq!(result.output.exit_code, Some(0));
    assert!(result.merged_contents.is_none());

    let status = opened.status().unwrap();
    assert!(
        status.staged.iter().all(|e| e.path != Path::new("a.txt")),
        "unexpected staged resolution when the custom command changed nothing: {status:?}"
    );
}

#[test]
fn launch_mergetool_builtin_preference_still_gates_repo_local_cmd() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    // Repo-local cmd WITHOUT the consent key: the Builtin preference forces
    // the tool name but must not weaken the repository-local command gate.
    run_git(repo, &["config", "merge.tool", "other"]);
    run_git(repo, &["config", "mergetool.fake.cmd", "echo forbidden"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::Builtin {
                id: "fake".to_string(),
                path: None,
            },
        )
        .expect_err("repo-local cmd must stay gated under a Builtin preference");

    match err.kind() {
        ErrorKind::Backend(msg) => {
            assert!(
                msg.contains("Refusing to execute repository-local mergetool command"),
                "unexpected backend error: {msg}"
            );
        }
        other => panic!("expected Backend error, got {other:?}"),
    }
}

#[test]
fn launch_mergetool_rejects_unresolved_marker_output() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_write_unresolved_markers_and_exit_success(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .expect_err("mergetool should fail when merged output still has markers");

    match err.kind() {
        ErrorKind::Backend(msg) => {
            assert!(
                msg.contains("left unresolved conflict markers"),
                "unexpected backend error: {msg}"
            );
            assert!(
                msg.contains("a.txt"),
                "backend error should include conflicted path: {msg}"
            );
        }
        other => panic!("expected backend error, got {other:?}"),
    }

    let status = opened.status().unwrap();
    assert!(
        status
            .staged
            .iter()
            .all(|entry| entry.path != Path::new("a.txt")),
        "unexpected staged resolution when mergetool left markers: {status:?}"
    );
    let conflict_entry = status
        .unstaged
        .iter()
        .find(|entry| entry.path == Path::new("a.txt"))
        .expect("conflict should remain unresolved");
    assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
    assert_eq!(
        conflict_entry.conflict,
        Some(FileConflictKind::BothModified)
    );
}

#[cfg(not(windows))]
#[test]
fn launch_mergetool_custom_cmd_supports_braced_env_variables() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let conflicted_path = "docs/a space.txt";
    setup_both_modified_text_conflict(repo, conflicted_path, "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        "cat \"${REMOTE}\" > \"${MERGED}\"; exit 0",
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let path = Path::new(conflicted_path);
    let result = opened
        .launch_mergetool(path, &ExternalMergeToolSelection::FromGitConfig)
        .unwrap();
    assert!(
        result.success,
        "expected braced variable expansion to succeed, got {result:?}"
    );
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));

    let on_disk = fs::read_to_string(repo.join(conflicted_path)).unwrap();
    assert_eq!(on_disk, "theirs\n");
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );

    let status = opened.status().unwrap();
    assert!(
        status.unstaged.iter().all(|e| e.path != path),
        "expected conflict to clear after mergetool resolution: {status:?}"
    );
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == path && e.kind == FileStatusKind::Modified),
        "expected resolved file to be staged after mergetool run: {status:?}"
    );
}

#[test]
#[cfg(windows)]
fn launch_mergetool_custom_cmd_supports_cmd_percent_env_variables() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let conflicted_path = "docs/a space.txt";
    setup_both_modified_text_conflict(repo, conflicted_path, "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        "copy /Y \"%REMOTE%\" \"%MERGED%\" > NUL && exit /b 0",
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let path = Path::new(conflicted_path);
    let result = opened
        .launch_mergetool(path, &ExternalMergeToolSelection::FromGitConfig)
        .unwrap();
    assert!(result.success, "{result:?}");
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(repo.join(conflicted_path)).unwrap(),
        "theirs\n"
    );
}

#[test]
fn launch_mergetool_custom_cmd_supports_unicode_conflicted_path() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let conflicted_path = "docs/spaced 日本語 file.txt";
    setup_both_modified_text_conflict(repo, conflicted_path, "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_copy_remote_to_merged_and_exit_success(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let path = Path::new(conflicted_path);
    let result = opened
        .launch_mergetool(path, &ExternalMergeToolSelection::FromGitConfig)
        .unwrap();
    assert!(
        result.success,
        "expected unicode conflicted path to resolve, got {result:?}"
    );
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));

    let on_disk = fs::read_to_string(repo.join(conflicted_path)).unwrap();
    assert_eq!(on_disk, "theirs\n");
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );

    let status = opened.status().unwrap();
    assert!(
        status.unstaged.iter().all(|entry| entry.path != path),
        "expected unicode conflict to clear after mergetool resolution: {status:?}"
    );
    assert!(
        status
            .staged
            .iter()
            .any(|entry| entry.path == path && entry.kind == FileStatusKind::Modified),
        "expected resolved unicode path to be staged after mergetool run: {status:?}"
    );
}

#[test]
fn launch_mergetool_prefers_merge_guitool_when_gui_default_true() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "cli"]);
    run_git(repo, &["config", "merge.guitool", "gui"]);
    run_git(repo, &["config", "mergetool.guiDefault", "true"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "cli", cmd_write_cli_to_merged());
    set_repo_local_mergetool_cmd_with_consent(repo, "gui", cmd_write_gui_to_merged());
    run_git(repo, &["config", "mergetool.cli.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.gui.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "gui");
    assert_eq!(result.merged_contents.as_deref(), Some("gui\n".as_bytes()));
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "gui\n");
}

#[cfg(unix)]
#[test]
fn launch_mergetool_uses_tool_path_override_without_custom_cmd() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let script_path = repo.join("fake-merge-tool.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\n# args: local base remote merged\ncat \"$3\" > \"$4\"\n",
    )
    .unwrap();
    make_executable(&script_path);

    run_git(repo, &["config", "merge.tool", "fake"]);
    run_git(
        repo,
        &[
            "config",
            "mergetool.fake.path",
            git_path_arg(&script_path).as_str(),
        ],
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("theirs\n".as_bytes())
    );
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "theirs\n");
}

#[cfg(unix)]
#[test]
fn launch_mergetool_builtin_tool_gets_merge_mode_arguments() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    // Stand-in for kdiff3: like the real tool it only merges when an output
    // file is named with `-o`, and otherwise just shows a read-only 3-way diff.
    let script_path = repo.join("fake-kdiff3.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\n\
         : > \"$PWD/kdiff3-args\"\n\
         output=\n\
         prev=\n\
         for arg in \"$@\"; do\n\
         \tprintf '%s\\n' \"$arg\" >> \"$PWD/kdiff3-args\"\n\
         \tif [ \"$prev\" = \"-o\" ]; then output=$arg; fi\n\
         \tprev=$arg\n\
         done\n\
         [ -n \"$output\" ] || exit 1\n\
         printf 'merged\\n' > \"$output\"\n",
    )
    .unwrap();
    make_executable(&script_path);

    run_git(repo, &["config", "merge.tool", "kdiff3"]);
    run_git(
        repo,
        &[
            "config",
            "mergetool.kdiff3.path",
            git_path_arg(&script_path).as_str(),
        ],
    );
    run_git(repo, &["config", "mergetool.kdiff3.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();

    assert!(
        result.success,
        "kdiff3 should be launched in merge mode: {:?}",
        result.output
    );
    assert_eq!(
        result.merged_contents.as_deref(),
        Some("merged\n".as_bytes())
    );
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "merged\n");

    let args: Vec<String> = fs::read_to_string(repo.join("kdiff3-args"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert!(args.iter().any(|arg| arg == "--auto"), "{args:?}");

    let output_index = args
        .iter()
        .position(|arg| arg == "-o")
        .expect("merge output flag should be passed");
    let output_path = Path::new(&args[output_index + 1]);
    assert!(output_path.is_absolute(), "{args:?}");
    assert_eq!(output_path.file_name().unwrap(), "a.txt");

    // git's kdiff3 recipe ends with BASE, LOCAL, REMOTE in that order.
    let tail = &args[args.len() - 3..];
    assert!(tail[0].contains("_BASE_"), "{args:?}");
    assert!(tail[1].contains("_LOCAL_"), "{args:?}");
    assert!(tail[2].contains("_REMOTE_"), "{args:?}");

    let label_index = args
        .iter()
        .position(|arg| arg == "--L1")
        .expect("window labels should be passed");
    assert_eq!(args[label_index + 1], "a.txt (Base)", "{args:?}");
}

#[test]
fn launch_mergetool_rejects_builtin_tool_that_cannot_merge() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "kompare"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap_err();

    assert!(
        format!("{err}").contains("cannot merge"),
        "expected a clear diff-only tool error, got {err}"
    );
    assert!(
        fs::read_to_string(repo.join("a.txt"))
            .unwrap()
            .contains("<<<<<<<"),
        "the conflicted file should be left untouched"
    );
}

#[cfg(unix)]
#[test]
fn launch_mergetool_prefers_custom_cmd_over_tool_path_override() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let script_path = repo.join("fake-merge-tool.sh");
    fs::write(
        &script_path,
        "#!/bin/sh\nprintf 'path\\n' > \"$4\"\ntouch \"$PWD/path_invoked\"\n",
    )
    .unwrap();
    make_executable(&script_path);

    run_git(repo, &["config", "merge.tool", "fake"]);
    run_git(
        repo,
        &[
            "config",
            "mergetool.fake.path",
            git_path_arg(&script_path).as_str(),
        ],
    );
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_write_cmd_to_merged());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);
    assert_eq!(result.tool_name, "fake");
    assert_eq!(result.output.exit_code, Some(0));
    assert_eq!(result.merged_contents.as_deref(), Some("cmd\n".as_bytes()));
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "cmd\n");
    assert!(
        !repo.join("path_invoked").exists(),
        "tool path executable should not run when mergetool.<tool>.cmd is configured"
    );
}

#[test]
fn launch_mergetool_write_to_temp_true_uses_temp_stage_paths() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_dump_stage_paths_and_copy_remote());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success);

    let vars = read_stage_env_vars(&repo.join("a.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");
    for var in vars {
        let var_path = Path::new(&var);
        let normalized_var = normalize_stage_var(&var);
        assert!(
            var_path.is_absolute(),
            "writeToTemp=true should pass absolute temp paths, got {var}"
        );
        assert!(
            normalized_var.contains("worktree-mergetool-"),
            "expected temporary mergetool prefix in path, got {var}"
        );
        assert!(
            !normalized_var.starts_with("./"),
            "writeToTemp=true should not use workdir-prefixed paths: {var}"
        );
        assert!(
            !var_path.exists(),
            "writeToTemp=true with default keepTemporaries=false should cleanup stage files: {var}"
        );
    }
}

#[test]
fn launch_mergetool_write_to_temp_false_uses_workdir_prefixed_stage_paths() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "docs/note.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_dump_stage_paths_and_copy_remote());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "false"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("docs/note.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success, "{result:?}");

    let vars = read_stage_env_vars(&repo.join("docs/note.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");
    for var in vars {
        let normalized_var = normalize_stage_var(&var);
        assert!(
            normalized_var.starts_with("./docs/note_"),
            "writeToTemp=false should use './' prefixed workdir paths, got {var}"
        );
        assert!(
            normalized_var.contains("_BASE_")
                || normalized_var.contains("_LOCAL_")
                || normalized_var.contains("_REMOTE_"),
            "unexpected stage-file naming: {var}"
        );
        let fs_path = stage_var_to_fs_path(repo, &var);
        assert!(
            !fs_path.exists(),
            "writeToTemp=false with default keepTemporaries=false should cleanup stage files: {var}"
        );
    }
}

#[test]
fn launch_mergetool_write_to_temp_false_keep_temporaries_preserves_stage_files() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "docs/note.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_dump_stage_paths_and_copy_remote());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "false"]);
    run_git(repo, &["config", "mergetool.keepTemporaries", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("docs/note.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success, "{result:?}");

    let vars = read_stage_env_vars(&repo.join("docs/note.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");
    for var in vars {
        let normalized_var = normalize_stage_var(&var);
        assert!(
            normalized_var.starts_with("./docs/note_"),
            "writeToTemp=false should use './' prefixed workdir paths, got {var}"
        );
        let fs_path = stage_var_to_fs_path(repo, &var);
        assert!(
            fs_path.exists(),
            "keepTemporaries=true should keep stage file in workdir mode: {var}"
        );
    }
}

#[test]
fn launch_mergetool_write_to_temp_false_keep_temporaries_preserves_stage_files_on_abort() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "docs/note.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_dump_stage_paths_and_exit_failure(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "false"]);
    run_git(repo, &["config", "mergetool.keepTemporaries", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("docs/note.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(
        !result.success,
        "tool exit failure should be reported as unresolved"
    );

    let vars = read_stage_env_vars(&repo.join("docs/note.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");
    for var in vars {
        let normalized_var = normalize_stage_var(&var);
        assert!(
            normalized_var.starts_with("./docs/note_"),
            "writeToTemp=false should use './' prefixed workdir paths, got {var}"
        );
        let fs_path = stage_var_to_fs_path(repo, &var);
        assert!(
            fs_path.exists(),
            "keepTemporaries=true should keep stage file on abort in workdir mode: {var}"
        );
    }
}

#[test]
fn launch_mergetool_write_to_temp_true_keep_temporaries_preserves_stage_files() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_dump_stage_paths_and_copy_remote());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "true"]);
    run_git(repo, &["config", "mergetool.keepTemporaries", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success, "{result:?}");

    let vars = read_stage_env_vars(&repo.join("a.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");

    let mut temp_dirs: Vec<PathBuf> = Vec::new();
    for var in vars {
        let var_path = Path::new(&var);
        let normalized_var = normalize_stage_var(&var);
        assert!(
            var_path.is_absolute(),
            "writeToTemp=true should pass absolute temp paths, got {var}"
        );
        assert!(
            normalized_var.contains("worktree-mergetool-"),
            "expected temporary mergetool prefix in path, got {var}"
        );
        assert!(
            var_path.exists(),
            "keepTemporaries=true should keep stage file in temp mode: {var}"
        );
        if let Some(parent) = var_path.parent()
            && !temp_dirs.iter().any(|dir| dir == parent)
        {
            temp_dirs.push(parent.to_path_buf());
        }
    }

    // Keep test environment clean even though behavior keeps temp files.
    for dir in temp_dirs {
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn launch_mergetool_write_to_temp_true_keep_temporaries_preserves_stage_files_on_abort() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(
        repo,
        "fake",
        cmd_dump_stage_paths_and_exit_failure(),
    );
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);
    run_git(repo, &["config", "mergetool.writeToTemp", "true"]);
    run_git(repo, &["config", "mergetool.keepTemporaries", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("a.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(
        !result.success,
        "tool exit failure should be reported as unresolved"
    );

    let vars = read_stage_env_vars(&repo.join("a.txt.env"));
    assert_eq!(vars.len(), 3, "expected BASE/LOCAL/REMOTE dump");

    let mut temp_dirs: Vec<PathBuf> = Vec::new();
    for var in vars {
        let var_path = Path::new(&var);
        let normalized_var = normalize_stage_var(&var);
        assert!(
            var_path.is_absolute(),
            "writeToTemp=true should pass absolute temp paths, got {var}"
        );
        assert!(
            normalized_var.contains("worktree-mergetool-"),
            "expected temporary mergetool prefix in path, got {var}"
        );
        assert!(
            var_path.exists(),
            "keepTemporaries=true should keep stage file on abort in temp mode: {var}"
        );
        if let Some(parent) = var_path.parent()
            && !temp_dirs.iter().any(|dir| dir == parent)
        {
            temp_dirs.push(parent.to_path_buf());
        }
    }

    // Keep test environment clean even though behavior keeps temp files.
    for dir in temp_dirs {
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
fn launch_mergetool_no_base_conflict_passes_empty_base_file() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_added_text_conflict(repo, "new.txt", "ours added\n", "theirs added\n");

    run_git(repo, &["config", "merge.tool", "fake"]);
    set_repo_local_mergetool_cmd_with_consent(repo, "fake", cmd_dump_base_size_and_copy_remote());
    run_git(repo, &["config", "mergetool.fake.trustExitCode", "true"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let result = opened
        .launch_mergetool(
            Path::new("new.txt"),
            &ExternalMergeToolSelection::FromGitConfig,
        )
        .unwrap();
    assert!(result.success, "{result:?}");
    assert_eq!(
        fs::read_to_string(repo.join("new.txt.base-size")).unwrap(),
        "0",
        "BASE should be an empty file for both-added/no-base conflicts"
    );
    assert_eq!(
        fs::read_to_string(repo.join("new.txt")).unwrap(),
        "theirs added\n"
    );
}
