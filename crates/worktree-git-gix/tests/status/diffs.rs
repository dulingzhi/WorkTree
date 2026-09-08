use super::support::{
    assert_file_diff_text_sources, assert_git_failure, git_command, png_1x1_rgba,
    require_git_shell_for_status_integration_tests, run_git, run_git_expect_failure,
    run_git_output, setup_both_modified_text_conflict, write,
};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use worktree_core::conflict_session::ConflictResolverStrategy;
use worktree_core::domain::{
    CommitId, DiffArea, DiffLineKind, DiffPreviewTextSide, DiffTarget, FileConflictKind,
    FileStatusKind,
};
use worktree_core::error::{ErrorKind, GitFailureId};
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;

#[test]
fn diff_unified_works_for_staged_and_unstaged() {
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

    let unstaged = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap();
    assert!(unstaged.contains("@@"));

    run_git(repo, &["add", "a.txt"]);

    let staged = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap();
    assert!(staged.contains("@@"));
}

#[test]
fn staged_diff_unified_covers_every_staged_file_and_ignores_unstaged() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["commit", "--allow-empty", "-m", "init"]);

    // Two files staged, one left unstaged on top of a staged edit.
    write(repo, "a.txt", "one\n");
    write(repo, "b.txt", "bee\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);
    write(repo, "a.txt", "one\nunstaged tail\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let diff = opened.staged_diff_unified().unwrap();
    assert!(
        diff.contains("diff --git a/a.txt b/a.txt"),
        "expected a.txt in the whole-staged diff: {diff}"
    );
    assert!(
        diff.contains("diff --git a/b.txt b/b.txt"),
        "expected b.txt in the whole-staged diff: {diff}"
    );
    assert!(
        !diff.contains("unstaged tail"),
        "unstaged worktree content must not leak into the staged diff: {diff}"
    );

    // A clean index yields an empty diff rather than an error (git diff's
    // exit code is 0 there, but the exit-1-with-differences path is the same
    // shared runner `diff_unified` exercises).
    run_git(repo, &["commit", "-m", "stage everything"]);
    assert_eq!(opened.staged_diff_unified().unwrap(), "");
}

#[test]
fn diff_working_tree_unstaged_ignores_crlf_only_line_ending_changes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "core.autocrlf", "false"]);
    run_git(repo, &["config", "core.eol", "lf"]);

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\r\ntwo\r\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let target = DiffTarget::WorkingTree {
        path: PathBuf::from("a.txt"),
        area: DiffArea::Unstaged,
    };

    let unified = opened.diff_unified(&target).unwrap();
    assert!(
        unified.trim().is_empty(),
        "expected CRLF-only unstaged diff to be suppressed:\n{unified}"
    );

    let parsed = opened.diff_parsed(&target).unwrap();
    assert!(
        parsed.lines.is_empty(),
        "expected parsed diff to be empty for CRLF-only unstaged change: {parsed:?}"
    );
}

#[test]
fn diff_file_text_reports_old_and_new_for_working_tree_and_commits() {
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

    let unstaged = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("file diff for unstaged changes");
    assert_eq!(unstaged.path, PathBuf::from("a.txt"));
    assert_file_diff_text_sources(&unstaged, Some("one\n"), Some("one\ntwo\n"));

    run_git(repo, &["add", "a.txt"]);

    let staged = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("file diff for staged changes");
    assert_file_diff_text_sources(&staged, Some("one\n"), Some("one\ntwo\n"));

    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    let head = git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse to run");
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap().trim().to_string();

    let commit = opened
        .diff_file_text(&DiffTarget::Commit {
            commit_id: worktree_core::domain::CommitId(head.into()),
            path: Some(PathBuf::from("a.txt")),
        })
        .unwrap()
        .expect("file diff for commit");
    assert_file_diff_text_sources(&commit, Some("one\n"), Some("one\ntwo\n"));
}

#[test]
fn diff_file_text_unstaged_uses_git_normalized_worktree_content() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "core.autocrlf", "true"]);

    write(repo, "a.txt", "one\ntwo\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\r\ntwo\r\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let diff = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("file diff for unstaged crlf-only change");

    assert_file_diff_text_sources(&diff, Some("one\ntwo\n"), Some("one\ntwo\n"));
}

#[test]
fn diff_file_text_root_commit_has_no_parent_side() {
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
        &["-c", "commit.gpgsign=false", "commit", "-m", "root"],
    );

    let head = git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse to run");
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap().trim().to_string();

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit = opened
        .diff_file_text(&DiffTarget::Commit {
            commit_id: worktree_core::domain::CommitId(head.into()),
            path: Some(PathBuf::from("a.txt")),
        })
        .unwrap()
        .expect("file diff for root commit");
    assert_file_diff_text_sources(&commit, None, Some("one\n"));
}

#[test]
fn diff_file_text_staged_add_and_delete_report_missing_sides() {
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

    // Stage a new file (missing on HEAD) and delete the initial file (missing on disk + index).
    write(repo, "b.txt", "new\n");
    run_git(repo, &["add", "b.txt"]);
    run_git(repo, &["rm", "a.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let added = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("b.txt"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("file diff for staged added file");
    assert_eq!(added.path, PathBuf::from("b.txt"));
    assert_file_diff_text_sources(&added, None, Some("new\n"));

    let deleted = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("file diff for staged deleted file");
    assert_eq!(deleted.path, PathBuf::from("a.txt"));
    assert_file_diff_text_sources(&deleted, Some("one\n"), None);
}

#[test]
fn diff_preview_text_file_commit_added_file_returns_new_side_blob_path() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "docs/added.txt", "one\ntwo");
    run_git(repo, &["add", "docs/added.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add file"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit_id = CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).into());
    let preview_path = opened
        .diff_preview_text_file(
            &DiffTarget::Commit {
                commit_id,
                path: Some(PathBuf::from("docs/added.txt")),
            },
            DiffPreviewTextSide::New,
        )
        .unwrap()
        .expect("preview text file for committed added file");

    assert!(preview_path.is_file());
    assert_eq!(
        fs::read_to_string(&preview_path).expect("read committed added preview text file"),
        "one\ntwo"
    );
}

#[test]
fn diff_preview_text_file_commit_deleted_file_returns_old_side_blob_path() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "docs/delete-me.txt", "one\ntwo");
    run_git(repo, &["add", "docs/delete-me.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );
    run_git(repo, &["rm", "docs/delete-me.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "delete file"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit_id = CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).into());
    let preview_path = opened
        .diff_preview_text_file(
            &DiffTarget::Commit {
                commit_id,
                path: Some(PathBuf::from("docs/delete-me.txt")),
            },
            DiffPreviewTextSide::Old,
        )
        .unwrap()
        .expect("preview text file for committed deleted file");

    assert!(preview_path.is_file());
    assert_eq!(
        fs::read_to_string(&preview_path).expect("read committed deleted preview text file"),
        "one\ntwo"
    );
}

#[test]
fn diff_preview_text_file_staged_deleted_file_returns_head_blob_path() {
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
    run_git(repo, &["rm", "a.txt"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let preview_path = opened
        .diff_preview_text_file(
            &DiffTarget::WorkingTree {
                path: PathBuf::from("a.txt"),
                area: DiffArea::Staged,
            },
            DiffPreviewTextSide::Old,
        )
        .unwrap()
        .expect("preview text file for staged deleted file");

    assert!(preview_path.is_file());
    assert_eq!(
        fs::read_to_string(&preview_path).expect("read staged deleted preview text file"),
        "one\n"
    );
}

#[test]
fn diff_file_text_returns_none_for_directories() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    write(repo, "dir/a.txt", "one\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let result = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("dir"),
            area: DiffArea::Unstaged,
        })
        .unwrap();

    assert!(result.is_none());

    run_git(repo, &["add", "dir/a.txt"]);
    let staged_result = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("dir"),
            area: DiffArea::Staged,
        })
        .unwrap();

    assert!(staged_result.is_none());
}

#[test]
fn diff_file_image_reports_old_and_new_for_working_tree_and_commits() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    let old_png = png_1x1_rgba(0, 0, 0, 255);
    let new_png = png_1x1_rgba(255, 0, 0, 255);

    write(repo, "img.png", &old_png);
    run_git(repo, &["add", "img.png"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "img.png", &new_png);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let unstaged = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: PathBuf::from("img.png"),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("image diff for unstaged changes");
    assert_eq!(unstaged.path, PathBuf::from("img.png"));
    assert_eq!(unstaged.old.as_deref(), Some(old_png.as_slice()));
    assert_eq!(unstaged.new.as_deref(), Some(new_png.as_slice()));

    run_git(repo, &["add", "img.png"]);

    let staged = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: PathBuf::from("img.png"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("image diff for staged changes");
    assert_eq!(staged.old.as_deref(), Some(old_png.as_slice()));
    assert_eq!(staged.new.as_deref(), Some(new_png.as_slice()));

    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    let head = git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse to run");
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap().trim().to_string();

    let commit = opened
        .diff_file_image(&DiffTarget::Commit {
            commit_id: worktree_core::domain::CommitId(head.into()),
            path: Some(PathBuf::from("img.png")),
        })
        .unwrap()
        .expect("image diff for commit");
    assert_eq!(commit.old.as_deref(), Some(old_png.as_slice()));
    assert_eq!(commit.new.as_deref(), Some(new_png.as_slice()));
}

#[test]
fn diff_file_image_returns_none_for_directories() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    write(repo, "dir/a.png", "not really a png\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let result = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: PathBuf::from("dir"),
            area: DiffArea::Unstaged,
        })
        .unwrap();

    assert!(result.is_none());

    run_git(repo, &["add", "dir/a.png"]);
    let staged_result = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: PathBuf::from("dir"),
            area: DiffArea::Staged,
        })
        .unwrap();

    assert!(staged_result.is_none());
}

#[test]
fn diff_file_commit_target_without_path_returns_none() {
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

    let head = run_git_output(repo, &["rev-parse", "HEAD"]);
    let target = DiffTarget::Commit {
        commit_id: worktree_core::domain::CommitId(head.into()),
        path: None,
    };

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    assert!(opened.diff_file_text(&target).unwrap().is_none());
    assert!(opened.diff_file_image(&target).unwrap().is_none());
}

#[test]
fn diff_unified_outside_repository_path_returns_structured_git_error() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let outside = dir.path().join("outside.txt");
    fs::create_dir_all(&repo).unwrap();
    fs::write(&outside, "outside\n").unwrap();

    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);
    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let err = opened
        .diff_unified(&DiffTarget::WorkingTree {
            path: outside,
            area: DiffArea::Unstaged,
        })
        .expect_err("expected diff_unified to fail for outside path");
    assert_git_failure(&err, "git diff", GitFailureId::CommandFailed);
    let ErrorKind::Git(failure) = err.kind() else {
        unreachable!("assert_git_failure() already checked the error kind");
    };
    assert_eq!(failure.exit_code(), Some(128));
}

#[test]
fn diff_parsed_outside_repository_path_returns_structured_git_error() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let outside = dir.path().join("outside.txt");
    fs::create_dir_all(&repo).unwrap();
    fs::write(&outside, "outside\n").unwrap();

    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);
    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let err = opened
        .diff_parsed(&DiffTarget::WorkingTree {
            path: outside,
            area: DiffArea::Unstaged,
        })
        .expect_err("expected diff_parsed to fail for outside path");
    assert_git_failure(&err, "git diff", GitFailureId::CommandFailed);
    let ErrorKind::Git(failure) = err.kind() else {
        unreachable!("assert_git_failure() already checked the error kind");
    };
    assert_eq!(failure.exit_code(), Some(128));
}

#[test]
fn diff_parsed_commit_rename_preserves_rename_headers_and_hunks() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "diff.renames", "true"]);

    write(repo, "docs/source.txt", "one\ntwo\n");
    run_git(repo, &["add", "docs/source.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );

    fs::create_dir_all(repo.join("docs/renamed")).unwrap();
    fs::rename(
        repo.join("docs/source.txt"),
        repo.join("docs/renamed/target.txt"),
    )
    .unwrap();
    fs::write(repo.join("docs/renamed/target.txt"), "one\ntwo\nthree\n").unwrap();
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "rename"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit_id = CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).into());
    let diff = opened
        .diff_parsed(&DiffTarget::Commit {
            commit_id,
            path: None,
        })
        .expect("parse rename commit diff");

    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Header
                && line.text.as_ref() == "rename from docs/source.txt")
    );
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Header
                && line.text.as_ref() == "rename to docs/renamed/target.txt")
    );
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Hunk && line.text.as_ref().starts_with("@@")),
    );
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Add && line.text.as_ref() == "+three"),
    );
}

#[test]
fn diff_parsed_commit_added_file_matches_git_show_output() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "docs/added.txt", "one\ntwo");
    run_git(repo, &["add", "docs/added.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add file"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit_id = CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).into());
    let diff = opened
        .diff_parsed(&DiffTarget::Commit {
            commit_id: commit_id.clone(),
            path: Some(PathBuf::from("docs/added.txt")),
        })
        .expect("parse added file commit diff");
    let expected = run_git_output(
        repo,
        &[
            "show",
            "--no-ext-diff",
            "--pretty=format:",
            commit_id.as_ref(),
            "--",
            "docs/added.txt",
        ],
    );
    let actual = diff
        .lines
        .iter()
        .map(|line| line.text.as_ref())
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, expected.trim_end_matches('\n'));
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Header
                && line.text.as_ref().starts_with("new file mode ")),
    );
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Context
                && line.text.as_ref() == "\\ No newline at end of file"),
    );
}

#[test]
fn diff_parsed_commit_deleted_file_matches_git_show_output() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "docs/delete-me.txt", "one\ntwo");
    run_git(repo, &["add", "docs/delete-me.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "seed"],
    );
    run_git(repo, &["rm", "docs/delete-me.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "delete file"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let commit_id = CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).into());
    let diff = opened
        .diff_parsed(&DiffTarget::Commit {
            commit_id: commit_id.clone(),
            path: Some(PathBuf::from("docs/delete-me.txt")),
        })
        .expect("parse deleted file commit diff");
    let expected = run_git_output(
        repo,
        &[
            "show",
            "--no-ext-diff",
            "--pretty=format:",
            commit_id.as_ref(),
            "--",
            "docs/delete-me.txt",
        ],
    );
    let actual = diff
        .lines
        .iter()
        .map(|line| line.text.as_ref())
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(actual, expected.trim_end_matches('\n'));
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Header
                && line.text.as_ref().starts_with("deleted file mode ")),
    );
    assert!(
        diff.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Context
                && line.text.as_ref() == "\\ No newline at end of file"),
    );
}

#[test]
fn diff_working_tree_with_absolute_file_path_reads_current_file() {
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
    let absolute = repo.join("a.txt");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let text = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: absolute.clone(),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("text diff for absolute path");
    assert_file_diff_text_sources(&text, Some("one\n"), Some("one\ntwo\n"));

    let image = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: absolute,
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("image diff for absolute path");
    assert_eq!(image.old.as_deref(), Some("one\n".as_bytes()));
    assert_eq!(image.new.as_deref(), Some("one\ntwo\n".as_bytes()));
}

#[cfg(unix)]
#[test]
fn diff_working_tree_with_absolute_file_path_through_symlinked_repo_reads_current_file() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let repo_alias = dir.path().join("repo-alias");
    fs::create_dir_all(&repo).unwrap();
    symlink(&repo, &repo_alias).unwrap();

    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);

    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(&repo, "a.txt", "one\ntwo\n");
    let absolute = repo_alias.join("a.txt");

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();

    let text = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: absolute.clone(),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("text diff for symlinked absolute path");
    assert_file_diff_text_sources(&text, Some("one\n"), Some("one\ntwo\n"));

    let image = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: absolute,
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("image diff for symlinked absolute path");
    assert_eq!(image.old.as_deref(), Some("one\n".as_bytes()));
    assert_eq!(image.new.as_deref(), Some("one\ntwo\n".as_bytes()));
}

#[test]
fn staged_diff_for_unmerged_conflict_prefers_ours_for_text_and_image() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_both_modified_text_conflict(repo, "a.txt", "ours\n", "theirs\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let text = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("staged text diff for conflict");
    assert_file_diff_text_sources(&text, Some("ours\n"), Some("ours\n"));

    let image = opened
        .diff_file_image(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Staged,
        })
        .unwrap()
        .expect("staged image diff for conflict");
    assert_eq!(image.old.as_deref(), Some("ours\n".as_bytes()));
    assert_eq!(image.new.as_deref(), Some("ours\n".as_bytes()));
}

#[test]
fn diff_commit_with_unknown_revision_and_outside_conflict_path_are_handled() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let outside = dir.path().join("outside.txt");
    fs::create_dir_all(&repo).unwrap();
    fs::write(&outside, "outside\n").unwrap();

    run_git(&repo, &["init"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);
    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let unknown_target = DiffTarget::Commit {
        commit_id: worktree_core::domain::CommitId("not-a-real-revision".into()),
        path: Some(PathBuf::from("a.txt")),
    };

    let text = opened
        .diff_file_text(&unknown_target)
        .unwrap()
        .expect("text diff object for unknown revision");
    assert_file_diff_text_sources(&text, None, None);

    let image = opened
        .diff_file_image(&unknown_target)
        .unwrap()
        .expect("image diff object for unknown revision");
    assert_eq!(image.old, None);
    assert_eq!(image.new, None);

    let err = opened
        .conflict_session(&outside)
        .expect_err("outside absolute path should be rejected");
    assert!(
        matches!(err.kind(), ErrorKind::Backend(_)),
        "expected backend error for outside path, got {err:?}"
    );
}

#[test]
fn diff_file_text_uses_ours_and_theirs_for_conflicted_paths() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    write(repo, "a.txt", "ours\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let status = opened.status().unwrap();
    assert_eq!(status.unstaged.len(), 1);
    assert_eq!(status.unstaged[0].path, PathBuf::from("a.txt"));
    assert_eq!(status.unstaged[0].kind, FileStatusKind::Conflicted);
    assert_eq!(
        status.unstaged[0].conflict,
        Some(FileConflictKind::BothModified)
    );

    let diff = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("file diff for conflicted changes");
    assert_file_diff_text_sources(&diff, Some("ours\n"), Some("theirs\n"));

    let session = opened
        .conflict_session(Path::new("a.txt"))
        .unwrap()
        .expect("conflict session");
    assert_eq!(session.conflict_kind, FileConflictKind::BothModified);
    assert_eq!(session.strategy, ConflictResolverStrategy::FullTextResolver);
    assert_eq!(session.total_regions(), 1);
    assert_eq!(session.unsolved_count(), 1);
    assert_eq!(session.regions[0].ours, "ours\n");
    assert_eq!(session.regions[0].theirs, "theirs\n");
}

#[test]
fn diff_file_text_handles_modify_delete_conflicts() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "a.txt", "theirs\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "theirs"],
    );

    run_git(repo, &["checkout", "-"]);
    run_git(repo, &["rm", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "ours_delete"],
    );

    run_git_expect_failure(repo, &["merge", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let diff = opened
        .diff_file_text(&DiffTarget::WorkingTree {
            path: PathBuf::from("a.txt"),
            area: DiffArea::Unstaged,
        })
        .unwrap()
        .expect("file diff for conflicted changes");
    assert_file_diff_text_sources(&diff, None, Some("theirs\n"));
}
