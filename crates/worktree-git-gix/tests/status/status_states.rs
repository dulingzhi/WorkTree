use super::support::{
    require_git_shell_for_status_integration_tests, run_git, run_git_output, write,
};
use std::fs;
use std::path::{Path, PathBuf};
use worktree_core::domain::{DiffArea, DiffTarget, FileStatusKind};
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;
use worktree_test_support::set_fixed_mtime;

#[test]
fn status_separates_staged_and_unstaged() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);
    write(repo, "b.txt", "untracked\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();

    assert_eq!(status.staged.len(), 1);
    assert_eq!(status.staged[0].path, PathBuf::from("a.txt"));
    assert_eq!(status.staged[0].kind, FileStatusKind::Modified);

    assert_eq!(status.unstaged.len(), 1);
    assert_eq!(status.unstaged[0].path, PathBuf::from("b.txt"));
    assert_eq!(status.unstaged[0].kind, FileStatusKind::Untracked);
}

#[test]
fn repeated_status_on_same_repo_instance_reuses_staged_state_and_invalidates_on_index_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "base\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let first = opened.status().unwrap();
    assert_eq!(first.staged.len(), 1);
    assert_eq!(first.staged[0].path, PathBuf::from("a.txt"));
    assert!(first.unstaged.is_empty());

    write(repo, "b.txt", "base\nworktree\n");
    let second = opened.status().unwrap();
    assert_eq!(second.staged.len(), 1);
    assert_eq!(second.staged[0].path, PathBuf::from("a.txt"));
    assert_eq!(second.unstaged.len(), 1);
    assert_eq!(second.unstaged[0].path, PathBuf::from("b.txt"));
    assert_eq!(second.unstaged[0].kind, FileStatusKind::Modified);

    run_git(repo, &["add", "b.txt"]);
    let third = opened.status().unwrap();
    assert_eq!(third.staged.len(), 2);
    assert!(
        third
            .staged
            .iter()
            .any(|entry| entry.path == Path::new("a.txt"))
    );
    assert!(
        third
            .staged
            .iter()
            .any(|entry| entry.path == Path::new("b.txt"))
    );
    assert!(third.unstaged.is_empty());
}

#[test]
fn status_does_not_rewrite_index_when_only_worktree_stat_is_stale() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    set_fixed_mtime(&repo.join("a.txt"));
    let index_before = fs::read(repo.join(".git").join("index")).unwrap();

    let status = opened.status().unwrap();
    assert!(status.staged.is_empty());
    assert!(status.unstaged.is_empty());

    let index_after = fs::read(repo.join(".git").join("index")).unwrap();
    assert_eq!(
        index_after, index_before,
        "status should not rewrite the index for metadata-only worktree changes"
    );
}

#[test]
fn repeated_status_does_not_rewrite_index_when_cached_staged_state_is_reused() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let first = opened.status().unwrap();
    assert!(first.staged.is_empty());
    assert!(first.unstaged.is_empty());

    set_fixed_mtime(&repo.join("a.txt"));
    let index_before = fs::read(repo.join(".git").join("index")).unwrap();

    let second = opened.status().unwrap();
    assert!(second.staged.is_empty());
    assert!(second.unstaged.is_empty());

    let index_after = fs::read(repo.join(".git").join("index")).unwrap();
    assert_eq!(
        index_after, index_before,
        "cached repeated status should stay read-only for metadata-only worktree changes"
    );
}

#[test]
fn repeated_status_on_same_repo_instance_invalidates_when_head_moves_without_index_change() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let clean = opened.status().unwrap();
    assert!(clean.staged.is_empty());
    assert!(clean.unstaged.is_empty());

    run_git(repo, &["reset", "--soft", "HEAD~1"]);

    let after_reset = opened.status().unwrap();
    assert_eq!(after_reset.staged.len(), 1);
    assert_eq!(after_reset.staged[0].path, Path::new("a.txt"));
    assert_eq!(after_reset.staged[0].kind, FileStatusKind::Modified);
    assert!(after_reset.unstaged.is_empty());
}

#[test]
fn status_lists_untracked_files_in_directories() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);

    write(repo, "dir/a.txt", "one\n");
    write(repo, "dir/b.txt", "two\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();

    assert_eq!(status.unstaged.len(), 2);
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("dir/a.txt") && e.kind == FileStatusKind::Untracked)
    );
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("dir/b.txt") && e.kind == FileStatusKind::Untracked)
    );
}

#[test]
fn status_ignores_nested_target_directories_with_target_slash_pattern() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, ".gitignore", "target/\n");
    run_git(repo, &["add", ".gitignore"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init ignore"],
    );

    write(
        repo,
        "crates/worktree-ui-gpui/target/criterion/report/index.html",
        "ignored\n",
    );
    write(repo, "visible.txt", "untracked\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();

    assert!(
        status.unstaged.iter().all(|entry| !entry
            .path
            .starts_with(Path::new("crates/worktree-ui-gpui/target"))),
        "expected nested target/ contents to be ignored, got {status:?}"
    );
    assert!(
        status
            .unstaged
            .iter()
            .any(|entry| entry.path == Path::new("visible.txt")
                && entry.kind == FileStatusKind::Untracked),
        "expected visible.txt as untracked, got {status:?}"
    );
}

#[test]
fn gitlink_added_and_unstaged_modified_reports_expected_status_and_diff() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let nested = repo.join("chess3");

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    std::fs::create_dir_all(&nested).expect("create nested repo path");
    run_git(&nested, &["init"]);
    run_git(&nested, &["config", "user.email", "you@example.com"]);
    run_git(&nested, &["config", "user.name", "You"]);
    run_git(&nested, &["config", "commit.gpgsign", "false"]);

    write(&nested, "file.txt", "one\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c1"],
    );

    run_git(repo, &["add", "chess3"]);

    write(&nested, "file.txt", "one\ntwo\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c2"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let status = opened.status().unwrap();
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("chess3") && e.kind == FileStatusKind::Added),
        "expected staged Added gitlink entry; status={status:?}"
    );
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("chess3") && e.kind == FileStatusKind::Modified),
        "expected unstaged Modified gitlink entry; status={status:?}"
    );

    let diff = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("chess3"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert!(
        diff.contains("Subproject commit"),
        "expected unstaged gitlink unified diff to include subproject commit line; diff={diff}"
    );

    let file_text = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("chess3"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert!(
        file_text.is_none(),
        "expected no direct file text payload for directory-backed gitlink target"
    );
}

#[test]
fn committed_gitlink_unstaged_modified_reports_modified_status_and_diff() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let nested = repo.join("chess3");

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    std::fs::create_dir_all(&nested).expect("create nested repo path");
    run_git(&nested, &["init"]);
    run_git(&nested, &["config", "user.email", "you@example.com"]);
    run_git(&nested, &["config", "user.name", "You"]);
    run_git(&nested, &["config", "commit.gpgsign", "false"]);

    write(&nested, "file.txt", "one\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c1"],
    );

    run_git(repo, &["add", "chess3"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add gitlink"],
    );

    write(&nested, "file.txt", "one\ntwo\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c2"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let status = opened.status().unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("chess3") && e.kind == FileStatusKind::Modified),
        "expected unstaged Modified gitlink entry after nested repo advances; status={status:?}"
    );

    let diff = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("chess3"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert!(
        diff.contains("Subproject commit"),
        "expected unstaged gitlink unified diff to include subproject commit line; diff={diff}"
    );
}

#[test]
fn status_cache_invalidates_when_gitlink_appears_on_same_repo_instance() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let nested = repo.join("chess3");

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let initial_status = opened.status().unwrap();
    assert!(
        initial_status.staged.is_empty() && initial_status.unstaged.is_empty(),
        "expected clean repo before adding gitlink; status={initial_status:?}"
    );

    std::fs::create_dir_all(&nested).expect("create nested repo path");
    run_git(&nested, &["init"]);
    run_git(&nested, &["config", "user.email", "you@example.com"]);
    run_git(&nested, &["config", "user.name", "You"]);
    run_git(&nested, &["config", "commit.gpgsign", "false"]);

    write(&nested, "file.txt", "one\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c1"],
    );

    run_git(repo, &["add", "chess3"]);

    let staged_gitlink = opened.status().unwrap();
    assert!(
        staged_gitlink
            .staged
            .iter()
            .any(|e| e.path == Path::new("chess3") && e.kind == FileStatusKind::Added),
        "expected staged Added gitlink entry after cached clean status; status={staged_gitlink:?}"
    );

    write(&nested, "file.txt", "one\ntwo\n");
    run_git(&nested, &["add", "file.txt"]);
    run_git(
        &nested,
        &["-c", "commit.gpgsign=false", "commit", "-m", "nested c2"],
    );

    let advanced_gitlink = opened.status().unwrap();
    assert!(
        advanced_gitlink
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("chess3") && e.kind == FileStatusKind::Modified),
        "expected cached gitlink capability to keep reporting nested repo advances; status={advanced_gitlink:?}"
    );
}

#[test]
fn stage_and_unstage_paths_update_status() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    write(repo, "b.txt", "untracked\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened.stage(&[Path::new("a.txt")]).unwrap();
    let status = opened.status().unwrap();
    assert_eq!(status.staged.len(), 1);
    assert_eq!(status.staged[0].path, PathBuf::from("a.txt"));
    assert_eq!(status.staged[0].kind, FileStatusKind::Modified);
    assert_eq!(status.unstaged.len(), 1);
    assert_eq!(status.unstaged[0].path, PathBuf::from("b.txt"));
    assert_eq!(status.unstaged[0].kind, FileStatusKind::Untracked);

    opened.unstage(&[Path::new("a.txt")]).unwrap();
    let status = opened.status().unwrap();
    assert!(status.staged.is_empty());
    assert_eq!(status.unstaged.len(), 2);
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Modified)
    );
    assert!(
        status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("b.txt") && e.kind == FileStatusKind::Untracked)
    );
}

#[test]
fn unstage_empty_paths_with_head_unstages_all_index_changes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "base\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    write(repo, "b.txt", "base\nnext\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened.unstage(&[]).unwrap();

    let staged = run_git_output(repo, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.is_empty(),
        "expected empty staged diff, got {staged:?}"
    );

    let unstaged = run_git_output(repo, &["diff", "--name-only"]);
    assert!(
        unstaged.lines().any(|line| line == "a.txt"),
        "expected a.txt to be unstaged-modified, got {unstaged:?}"
    );
    assert!(
        unstaged.lines().any(|line| line == "b.txt"),
        "expected b.txt to be unstaged-modified, got {unstaged:?}"
    );
}

#[test]
fn unstage_empty_paths_without_head_unstages_all_added_paths() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);

    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "two\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened.unstage(&[]).unwrap();

    let staged = run_git_output(repo, &["diff", "--cached", "--name-only"]);
    assert!(
        staged.is_empty(),
        "expected empty staged diff, got {staged:?}"
    );

    let short = run_git_output(repo, &["status", "--short"]);
    assert!(
        short.lines().any(|line| line == "?? a.txt"),
        "expected a.txt to be untracked after unstage-all, got {short:?}"
    );
    assert!(
        short.lines().any(|line| line == "?? b.txt"),
        "expected b.txt to be untracked after unstage-all, got {short:?}"
    );
}

#[test]
fn unstage_paths_without_head_only_unstages_selected_entries() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);

    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "two\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened.unstage(&[Path::new("a.txt")]).unwrap();

    let short = run_git_output(repo, &["status", "--short"]);
    assert!(
        short.lines().any(|line| line == "?? a.txt"),
        "expected a.txt to be untracked after targeted unstage, got {short:?}"
    );
    assert!(
        short.lines().any(|line| line == "A  b.txt"),
        "expected b.txt to remain staged after targeted unstage, got {short:?}"
    );
}

#[test]
fn discard_worktree_changes_reverts_to_index_version() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);
    write(repo, "a.txt", "one\ntwo\nthree\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .discard_worktree_changes(&[Path::new("a.txt")])
        .unwrap();

    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );

    let status = opened.status().unwrap();
    assert!(
        status
            .staged
            .iter()
            .any(|e| e.path == Path::new("a.txt") && e.kind == FileStatusKind::Modified)
    );
    assert!(!status.unstaged.iter().any(|e| e.path == Path::new("a.txt")));
}

#[test]
fn discard_worktree_changes_reverts_modified_file_to_head() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .discard_worktree_changes(&[Path::new("a.txt")])
        .unwrap();

    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\n");
    let status = opened.status().unwrap();
    assert!(status.staged.is_empty());
    assert!(status.unstaged.is_empty());
}

#[test]
fn discard_worktree_changes_removes_staged_new_file() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "new.txt", "new\n");
    run_git(repo, &["add", "new.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .discard_worktree_changes(&[Path::new("new.txt")])
        .unwrap();

    assert!(!repo.join("new.txt").exists());
    let status = opened.status().unwrap();
    assert!(!status.staged.iter().any(|e| e.path == Path::new("new.txt")));
    assert!(
        !status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("new.txt"))
    );
}

#[test]
fn discard_worktree_changes_removes_untracked_file() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "untracked.txt", "new\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .discard_worktree_changes(&[Path::new("untracked.txt")])
        .unwrap();

    assert!(!repo.join("untracked.txt").exists());
    let status = opened.status().unwrap();
    assert!(
        !status
            .unstaged
            .iter()
            .any(|e| e.path == Path::new("untracked.txt"))
    );
}

#[test]
fn discard_worktree_changes_supports_mixed_selection() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "two\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one!\n");
    fs::remove_file(repo.join("b.txt")).unwrap();
    write(repo, "c.txt", "three\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened
        .discard_worktree_changes(&[Path::new("a.txt"), Path::new("b.txt"), Path::new("c.txt")])
        .unwrap();

    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(repo.join("b.txt")).unwrap(), "two\n");
    assert!(!repo.join("c.txt").exists());
    let status = opened.status().unwrap();
    assert!(status.staged.is_empty());
    assert!(status.unstaged.is_empty());
}

#[test]
fn stage_hunk_applies_only_part_of_a_file_to_index() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let mut base = String::new();
    for i in 1..=30 {
        base.push_str(&format!("L{i:02}\n"));
    }
    write(repo, "a.txt", &base);
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let modified = base
        .replace("L02\n", "L02-mod\n")
        .replace("L25\n", "L25-mod\n");
    write(repo, "a.txt", &modified);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let unstaged_before = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    let hunk_count_before = unstaged_before
        .lines()
        .filter(|l| l.starts_with("@@"))
        .count();
    assert_eq!(
        hunk_count_before, 2,
        "expected two hunks:\n{unstaged_before}"
    );

    let lines = unstaged_before.lines().collect::<Vec<_>>();
    let file_start = lines
        .iter()
        .position(|l| l.starts_with("diff --git "))
        .unwrap_or(0);
    let first_hunk = lines
        .iter()
        .position(|l| l.starts_with("@@"))
        .expect("first hunk header");
    let second_hunk = (first_hunk + 1..lines.len())
        .find(|&ix| lines.get(ix).is_some_and(|l| l.starts_with("@@")))
        .expect("second hunk header");

    let patch = lines[file_start..first_hunk]
        .iter()
        .chain(lines[first_hunk..second_hunk].iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    opened
        .apply_unified_patch_to_index_with_output(&patch, false)
        .unwrap();

    let staged_after = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert_eq!(
        staged_after.lines().filter(|l| l.starts_with("@@")).count(),
        1,
        "expected one staged hunk:\n{staged_after}"
    );
    assert!(staged_after.contains("-L02"));
    assert!(staged_after.contains("+L02-mod"));
    assert!(!staged_after.contains("L25-mod"));

    let unstaged_after = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert_eq!(
        unstaged_after
            .lines()
            .filter(|l| l.starts_with("@@"))
            .count(),
        1,
        "expected one remaining unstaged hunk:\n{unstaged_after}"
    );
    assert!(!unstaged_after.contains("L02-mod"));
    assert!(unstaged_after.contains("-L25"));
    assert!(unstaged_after.contains("+L25-mod"));
}

#[test]
fn unstage_hunk_reverts_only_that_part_in_index() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let mut base = String::new();
    for i in 1..=30 {
        base.push_str(&format!("L{i:02}\n"));
    }
    write(repo, "a.txt", &base);
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let modified = base
        .replace("L02\n", "L02-mod\n")
        .replace("L25\n", "L25-mod\n");
    write(repo, "a.txt", &modified);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let unstaged_before = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert_eq!(
        unstaged_before
            .lines()
            .filter(|l| l.starts_with("@@"))
            .count(),
        2,
        "expected two hunks:\n{unstaged_before}"
    );

    let lines = unstaged_before.lines().collect::<Vec<_>>();
    let file_start = lines
        .iter()
        .position(|l| l.starts_with("diff --git "))
        .unwrap_or(0);
    let first_hunk = lines
        .iter()
        .position(|l| l.starts_with("@@"))
        .expect("first hunk header");
    let second_hunk = (first_hunk + 1..lines.len())
        .find(|&ix| lines.get(ix).is_some_and(|l| l.starts_with("@@")))
        .expect("second hunk header");

    let patch = lines[file_start..first_hunk]
        .iter()
        .chain(lines[first_hunk..second_hunk].iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    opened
        .apply_unified_patch_to_index_with_output(&patch, false)
        .unwrap();

    let staged_after_stage = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert_eq!(
        staged_after_stage
            .lines()
            .filter(|l| l.starts_with("@@"))
            .count(),
        1,
        "expected one staged hunk:\n{staged_after_stage}"
    );

    opened
        .apply_unified_patch_to_index_with_output(&patch, true)
        .unwrap();

    let staged_after_unstage = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert!(
        staged_after_unstage.trim().is_empty(),
        "expected staged diff to be empty:\n{staged_after_unstage}"
    );

    let unstaged_after_unstage = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert_eq!(
        unstaged_after_unstage
            .lines()
            .filter(|l| l.starts_with("@@"))
            .count(),
        2,
        "expected two unstaged hunks:\n{unstaged_after_unstage}"
    );
    assert!(unstaged_after_unstage.contains("+L02-mod"));
    assert!(unstaged_after_unstage.contains("+L25-mod"));
}

/// A line-level unstage applies its patch in reverse, so the side it has to
/// match is the index. The patch therefore keeps the additions it is *not*
/// unstaging as context and drops the removals, which the index does not have.
/// Built the staging way instead, git rejects it with "patch does not apply".
#[test]
fn unstage_line_patch_must_describe_the_index_side() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(
        repo,
        "a.txt",
        "context one\nold one\nold two\ncontext two\n",
    );
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    // Stage a two-line modification, then unstage only the first of them.
    write(
        repo,
        "a.txt",
        "context one\nnew one\nnew two\ncontext two\n",
    );
    run_git(repo, &["add", "a.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let staging_shaped = concat!(
        "diff --git a/a.txt b/a.txt\n",
        "--- a/a.txt\n",
        "+++ b/a.txt\n",
        "@@ -1,4 +1,4 @@\n",
        " context one\n",
        " old one\n",
        " old two\n",
        "+new one\n",
        " context two\n",
    );
    assert!(
        opened
            .apply_unified_patch_to_index_with_output(staging_shaped, true)
            .is_err(),
        "a patch describing the HEAD side cannot be reverse-applied to the index"
    );

    let unstage_shaped = concat!(
        "diff --git a/a.txt b/a.txt\n",
        "--- a/a.txt\n",
        "+++ b/a.txt\n",
        "@@ -1,4 +1,4 @@\n",
        " context one\n",
        "+new one\n",
        " new two\n",
        " context two\n",
    );
    opened
        .apply_unified_patch_to_index_with_output(unstage_shaped, true)
        .expect("a patch describing the index side reverse-applies");

    let staged_after = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert!(
        staged_after.contains("+new two") && !staged_after.contains("+new one"),
        "only the unstaged line should have left the index:\n{staged_after}"
    );
}

/// A space in a path makes the `diff --git` line ambiguous, so git disambiguates
/// by repeating the name on the `---`/`+++` lines and terminating it with a TAB.
/// Both the diff we hand to the UI and the patch that comes back have to carry
/// that shape for a line-level stage to work at all.
#[test]
fn line_level_staging_round_trips_a_path_containing_spaces() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let rel = "src/rules - Copy.rs";
    write(repo, rel, "context one\nold one\nold two\ncontext two\n");
    run_git(repo, &["add", rel]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    write(repo, rel, "context one\nnew one\nnew two\ncontext two\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let unstaged = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from(rel),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert!(
        unstaged.contains(&format!("+++ b/{rel}\t")),
        "git must repeat the spaced name with a terminating TAB:\n{unstaged}"
    );

    // Stage only the first of the two changed lines, keeping the second's
    // addition out and demoting both removals to context.
    let one_line = format!(
        "diff --git a/{rel} b/{rel}\n\
         --- a/{rel}\t\n\
         +++ b/{rel}\t\n\
         @@ -1,4 +1,4 @@\n\
         \x20context one\n\
         -old one\n\
         \x20old two\n\
         +new one\n\
         \x20context two\n"
    );
    opened
        .apply_unified_patch_to_index_with_output(&one_line, false)
        .expect("a per-line patch for a spaced path must apply to the index");

    let staged_after = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from(rel),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert!(
        staged_after.contains("+new one") && !staged_after.contains("+new two"),
        "only the staged line should have reached the index:\n{staged_after}"
    );
}
