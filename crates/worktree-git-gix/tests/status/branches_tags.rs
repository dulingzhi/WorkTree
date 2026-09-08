use super::support::{
    assert_git_failure, git_command, git_path_arg, git_remote_url,
    require_git_shell_for_status_integration_tests, run_git, run_git_output, write,
};
use std::fs;
use worktree_core::error::{ErrorKind, GitFailureId};
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;
use worktree_test_support::hash_blob;

#[test]
fn create_rename_and_delete_local_branch() {
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

    let head = git_command()
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse HEAD");
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout)
        .expect("HEAD is utf-8")
        .trim()
        .to_owned();

    opened
        .create_branch("feature", &worktree_core::domain::CommitId(head.into()))
        .unwrap();
    run_git(
        repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/feature"],
    );

    opened.rename_branch("feature", "renamed-feature").unwrap();
    run_git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/renamed-feature",
        ],
    );
    let old_name = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref old branch name");
    assert!(
        !old_name.success(),
        "expected old branch name to be removed"
    );

    opened.delete_branch("renamed-feature").unwrap();
    let deleted = git_command()
        .arg("-C")
        .arg(repo)
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/renamed-feature",
        ])
        .status()
        .expect("show-ref");
    assert!(!deleted.success(), "expected branch to be deleted");
}

#[test]
fn create_branch_existing_branch_returns_structured_git_error() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
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
    run_git(repo, &["branch", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .create_branch("feature", &worktree_core::domain::CommitId(head.into()))
        .expect_err("creating an existing branch should fail");
    assert_git_failure(&err, "git branch", GitFailureId::CommandFailed);
    let ErrorKind::Git(failure) = err.kind() else {
        unreachable!();
    };
    assert_eq!(failure.exit_code(), Some(128));
    assert_eq!(
        failure.detail(),
        Some("fatal: a branch named 'feature' already exists")
    );
}

#[test]
fn create_branch_on_unborn_head_returns_structured_git_error() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .create_branch("feature", &worktree_core::domain::CommitId("HEAD".into()))
        .expect_err("creating a branch on unborn HEAD should fail");
    assert_git_failure(&err, "git branch", GitFailureId::CommandFailed);
    let ErrorKind::Git(failure) = err.kind() else {
        unreachable!();
    };
    assert_eq!(failure.exit_code(), Some(128));
    assert_eq!(
        failure.detail(),
        Some("fatal: not a valid object name: 'HEAD'")
    );

    let exists = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref feature");
    assert!(!exists.success(), "feature branch should not be created");
}

#[test]
fn create_branch_from_detached_head_using_head_revision() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "first"],
    );

    write(repo, "a.txt", "two\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );

    let first_commit = run_git_output(repo, &["rev-parse", "HEAD~1"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened
        .checkout_commit(&worktree_core::domain::CommitId(
            first_commit.clone().into(),
        ))
        .unwrap();
    opened
        .create_branch("rescue", &worktree_core::domain::CommitId("HEAD".into()))
        .unwrap();

    let rescue_target = run_git_output(repo, &["rev-parse", "rescue"]);
    assert_eq!(rescue_target, first_commit);
}

#[test]
fn create_branch_from_annotated_tag_peels_to_commit() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "tag.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    let head = run_git_output(repo, &["rev-parse", "HEAD"]);
    run_git(repo, &["tag", "-a", "v1", "-m", "v1"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened
        .create_branch("feature", &worktree_core::domain::CommitId("v1".into()))
        .unwrap();

    let feature_target = run_git_output(repo, &["rev-parse", "feature"]);
    assert_eq!(feature_target, head);
    let feature_kind = run_git_output(repo, &["cat-file", "-t", "feature"]);
    assert_eq!(feature_kind, "commit");
}

#[test]
fn create_branch_from_blob_target_returns_structured_git_error() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    let blob = hash_blob(repo, b"blob target\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .create_branch(
            "feature",
            &worktree_core::domain::CommitId(blob.clone().into()),
        )
        .expect_err("creating a branch from a blob should fail");
    assert_git_failure(&err, "git branch", GitFailureId::CommandFailed);
    let ErrorKind::Git(failure) = err.kind() else {
        unreachable!();
    };
    assert_eq!(failure.exit_code(), Some(128));
    let detail = failure.detail().expect("git detail");
    assert!(
        detail.contains("not a valid branch point"),
        "unexpected create-branch detail: {detail}"
    );
    assert!(
        detail.contains(&blob),
        "expected blob id in create-branch detail: {detail}"
    );
}

#[test]
fn create_branch_head_target_reflects_move_after_backend_open() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "first"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "a.txt", "two\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    let second_commit = run_git_output(repo, &["rev-parse", "HEAD"]);

    opened
        .create_branch("feature", &worktree_core::domain::CommitId("HEAD".into()))
        .unwrap();

    let feature_target = run_git_output(repo, &["rev-parse", "feature"]);
    assert_eq!(feature_target, second_commit);
}

#[test]
fn create_branch_target_branch_created_after_backend_open() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
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

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    run_git(repo, &["branch", "source"]);

    opened
        .create_branch("feature", &worktree_core::domain::CommitId("source".into()))
        .unwrap();

    let feature_target = run_git_output(repo, &["rev-parse", "feature"]);
    assert_eq!(feature_target, head);
}

#[test]
fn create_branch_succeeds_without_persisted_user_identity() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &[
            "-c",
            "user.email=you@example.com",
            "-c",
            "user.name=You",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "init",
        ],
    );

    let head = run_git_output(repo, &["rev-parse", "HEAD"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened
        .create_branch("feature", &worktree_core::domain::CommitId(head.into()))
        .unwrap();

    run_git(
        repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/feature"],
    );
}

#[test]
fn checkout_branch_switches_head_to_target_branch() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(repo, &["branch", "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.checkout_branch("feature").unwrap();

    let head = run_git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "feature");
}

#[test]
fn delete_branch_force_removes_unmerged_branch() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    run_git(repo, &["checkout", "-b", "feature"]);
    write(repo, "feature.txt", "feature\n");
    run_git(repo, &["add", "feature.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(repo, &["checkout", "main"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let err = opened
        .delete_branch("feature")
        .expect_err("safe delete should fail for unmerged branch");
    match err.kind() {
        ErrorKind::Git(failure) => {
            assert_eq!(failure.command(), "git branch -d");
            let msg = failure.to_string();
            assert!(
                msg.contains("not fully merged") || msg.contains("cannot delete branch"),
                "unexpected delete-branch error: {msg}"
            );
        }
        other => panic!("expected structured git error, got {other:?}"),
    }

    opened.delete_branch_force("feature").unwrap();

    let deleted = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref");
    assert!(!deleted.success(), "expected force-delete to remove branch");
}

#[test]
fn delete_branch_force_removes_branch_config_section() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(repo, &["branch", "feature"]);
    run_git(repo, &["config", "branch.feature.remote", "origin"]);
    run_git(
        repo,
        &["config", "branch.feature.merge", "refs/heads/feature"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.delete_branch_force("feature").unwrap();

    let branch_config = git_command()
        .arg("-C")
        .arg(repo)
        .args(["config", "--local", "--get-regexp", "^branch\\.feature\\."])
        .output()
        .expect("git config --get-regexp");
    assert_eq!(
        branch_config.status.code(),
        Some(1),
        "expected branch config section to be removed; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&branch_config.stdout),
        String::from_utf8_lossy(&branch_config.stderr),
    );
}

#[test]
fn delete_branch_force_keeps_branch_config_when_local_config_is_locked() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(repo, &["branch", "feature"]);
    run_git(repo, &["config", "branch.feature.remote", "origin"]);
    run_git(
        repo,
        &["config", "branch.feature.merge", "refs/heads/feature"],
    );
    fs::write(repo.join(".git").join("config.lock"), b"held elsewhere").unwrap();

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.delete_branch_force("feature").unwrap();

    let deleted = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref");
    assert!(!deleted.success(), "expected force-delete to remove branch");

    let branch_config = git_command()
        .arg("-C")
        .arg(repo)
        .args(["config", "--local", "--get-regexp", "^branch\\.feature\\."])
        .output()
        .expect("git config --get-regexp");
    assert!(
        branch_config.status.success(),
        "expected branch config section to remain when .git/config is locked"
    );
    let branch_config_stdout = String::from_utf8_lossy(&branch_config.stdout);
    assert!(
        branch_config_stdout.contains("branch.feature.remote origin")
            && branch_config_stdout.contains("branch.feature.merge refs/heads/feature"),
        "unexpected branch config after locked cleanup skip: {branch_config_stdout}"
    );
}

#[test]
fn delete_branch_force_missing_branch_is_structured_git_failure() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .delete_branch_force("missing")
        .expect_err("missing branch must surface as a git command failure");
    assert_git_failure(&err, "git branch -D", GitFailureId::CommandFailed);
    let msg = err.to_string();
    assert!(
        msg.contains("branch 'missing' not found"),
        "unexpected delete-branch-force error: {msg}"
    );
}

#[test]
fn delete_branch_force_rejects_unborn_current_branch_before_missing_ref_check() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init", "-b", "main"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .delete_branch_force("main")
        .expect_err("unborn checked-out branch must still be treated as in-use");
    assert_git_failure(&err, "git branch -D", GitFailureId::CommandFailed);
    let msg = err.to_string();
    assert!(
        msg.contains("used by worktree") && msg.contains(&git_path_arg(repo)),
        "unexpected delete-branch-force error: {msg}"
    );
}

#[test]
fn delete_branch_force_rejects_branch_checked_out_in_linked_worktree() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let linked_worktree = dir.path().join("feature-worktree");

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(repo, &["branch", "feature"]);

    let linked_worktree_arg = git_path_arg(&linked_worktree);
    run_git(repo, &["worktree", "add", &linked_worktree_arg, "feature"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let err = opened
        .delete_branch_force("feature")
        .expect_err("branch checked out in linked worktree must not be deleted");
    assert_git_failure(&err, "git branch -D", GitFailureId::CommandFailed);
    let msg = err.to_string();
    assert!(
        msg.contains("used by worktree") && msg.contains(&linked_worktree_arg),
        "unexpected delete-branch-force error: {msg}"
    );

    let still_exists = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref");
    assert!(
        still_exists.success(),
        "branch should remain after linked-worktree rejection"
    );
}

#[test]
fn delete_branch_force_rejects_branch_checked_out_in_main_worktree_when_opened_from_linked() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let linked_worktree = dir.path().join("feature-worktree");

    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(repo, &["branch", "feature"]);

    let linked_worktree_arg = git_path_arg(&linked_worktree);
    run_git(repo, &["worktree", "add", &linked_worktree_arg, "feature"]);

    let backend = GixBackend;
    let opened = backend.open(&linked_worktree).unwrap();
    let err = opened
        .delete_branch_force("main")
        .expect_err("main-worktree branch use must block deletion from linked worktree");
    assert_git_failure(&err, "git branch -D", GitFailureId::CommandFailed);
    let msg = err.to_string();
    assert!(
        msg.contains("used by worktree") && msg.contains(&git_path_arg(repo)),
        "unexpected delete-branch-force error: {msg}"
    );

    let still_exists = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/main"])
        .status()
        .expect("show-ref");
    assert!(
        still_exists.success(),
        "branch should remain after main-worktree rejection"
    );
}

#[test]
fn create_and_delete_local_tag() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "tag.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // No message => lightweight tag (a ref pointing straight at the commit),
    // matching `git tag <name>` semantics.
    opened
        .create_tag_with_output("v1.0.0", "HEAD", None, false)
        .unwrap();
    run_git(
        repo,
        &["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"],
    );
    let tag_type = git_command()
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "-t", "refs/tags/v1.0.0"])
        .output()
        .expect("cat-file");
    assert!(
        tag_type.status.success(),
        "expected refs/tags/v1.0.0 to exist"
    );
    assert_eq!(
        String::from_utf8_lossy(&tag_type.stdout).trim(),
        "commit",
        "a tag created without a message should be lightweight"
    );

    opened.delete_tag_with_output("v1.0.0").unwrap();
    let deleted = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"])
        .status()
        .expect("show-ref");
    assert!(!deleted.success(), "expected tag to be deleted");
}

#[test]
fn create_annotated_tag_includes_message() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "tag.gpgsign", "false"]);

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // A message => annotated tag object that stores the message.
    opened
        .create_tag_with_output("v1.0.0", "HEAD", Some("Release 1.0"), true)
        .unwrap();

    let tag_type = git_command()
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "-t", "refs/tags/v1.0.0"])
        .output()
        .expect("cat-file");
    assert!(
        tag_type.status.success(),
        "expected refs/tags/v1.0.0 to exist"
    );
    assert_eq!(
        String::from_utf8_lossy(&tag_type.stdout).trim(),
        "tag",
        "a tag created with a message should be annotated"
    );

    let contents = git_command()
        .arg("-C")
        .arg(repo)
        .args([
            "for-each-ref",
            "--format=%(contents:subject)",
            "refs/tags/v1.0.0",
        ])
        .output()
        .expect("for-each-ref");
    assert_eq!(
        String::from_utf8_lossy(&contents.stdout).trim(),
        "Release 1.0",
        "annotated tag should carry the provided message"
    );
}

#[test]
fn create_tag_respects_tag_gpgsign_config() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "tag.gpgsign", "true"]);
    run_git(
        repo,
        &["config", "gpg.program", "worktree-missing-gpg-program"],
    );

    write(repo, "a.txt", "one\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    // Signing only applies to annotated tags, so request one with a message.
    let err = opened
        .create_tag_with_output("v1.0.0", "HEAD", Some("Release 1.0"), true)
        .expect_err("tag creation should fail when signing is required and gpg is missing");

    match err.kind() {
        ErrorKind::Git(failure) => {
            assert_eq!(failure.command(), "git tag -m <message> -- v1.0.0 HEAD");
            let msg = failure.to_string();
            assert!(
                msg.contains("git tag -m <message> -- v1.0.0 HEAD failed"),
                "unexpected git error: {msg}"
            );
            let lower = msg.to_ascii_lowercase();
            assert!(
                msg.contains("worktree-missing-gpg-program") || lower.contains("sign"),
                "expected signing failure details in git error: {msg}"
            );
        }
        other => panic!("expected structured git error, got {other:?}"),
    }

    let tag_present = git_command()
        .arg("-C")
        .arg(repo)
        .args(["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"])
        .status()
        .expect("show-ref");
    assert!(
        !tag_present.success(),
        "tag should not exist when signing failed"
    );
}

#[test]
fn list_tags_returns_sorted_names_with_commit_targets() {
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

    run_git(repo, &["tag", "-a", "a-first", "-m", "a-first"]);
    run_git(repo, &["tag", "z-last"]);
    let head = run_git_output(repo, &["rev-parse", "HEAD"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let tags = opened.list_tags().unwrap();

    let names = tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec!["a-first", "z-last"]);
    assert!(tags.iter().all(|tag| tag.target.as_ref() == head));
}

#[test]
fn prune_merged_branches_deletes_local_branches_missing_on_remote() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let origin = dir.path().join("origin.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&origin).unwrap();

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);

    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    run_git(&origin, &["init", "--bare", "-b", "main"]);
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);

    run_git(&repo, &["checkout", "-b", "feature"]);
    write(&repo, "feature.txt", "feature\n");
    run_git(&repo, &["add", "feature.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&repo, &["push", "-u", "origin", "feature"]);

    run_git(&repo, &["checkout", "main"]);
    run_git(
        &repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "merge",
            "--no-ff",
            "feature",
            "-m",
            "merge feature",
        ],
    );
    run_git(&repo, &["push", "origin", "main"]);
    run_git(&repo, &["push", "origin", "--delete", "feature"]);

    run_git(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/heads/feature"],
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    opened.prune_merged_branches_with_output().unwrap();

    let deleted = git_command()
        .arg("-C")
        .arg(&repo)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/feature"])
        .status()
        .expect("show-ref");
    assert!(
        !deleted.success(),
        "expected merged local branch to be deleted"
    );
}

#[test]
fn prune_local_tags_deletes_tags_missing_from_remotes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let origin = dir.path().join("origin.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&origin).unwrap();

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);

    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    run_git(&origin, &["init", "--bare", "-b", "main"]);
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);

    run_git(&repo, &["tag", "v1.0.0"]);
    run_git(&repo, &["tag", "stale-local"]);
    run_git(&repo, &["push", "origin", "refs/tags/v1.0.0"]);
    run_git(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/tags/stale-local"],
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    opened.prune_local_tags_with_output().unwrap();

    run_git(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"],
    );
    let stale_deleted = git_command()
        .arg("-C")
        .arg(&repo)
        .args(["show-ref", "--verify", "--quiet", "refs/tags/stale-local"])
        .status()
        .expect("show-ref");
    assert!(
        !stale_deleted.success(),
        "expected stale local tag to be deleted"
    );
}

#[test]
fn prune_local_tags_with_output_no_remotes_is_noop() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);
    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(&repo, &["tag", "local-only"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let output = opened.prune_local_tags_with_output().unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(
        output
            .stdout
            .contains("No remotes configured; skipping tag prune."),
        "unexpected stdout: {}",
        output.stdout
    );
    run_git(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/tags/local-only"],
    );
}

#[test]
fn prune_local_tags_with_output_reports_noop_when_all_tags_exist_remotely() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let origin = dir.path().join("origin.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&origin).unwrap();

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "you@example.com"]);
    run_git(&repo, &["config", "user.name", "You"]);
    run_git(&repo, &["config", "commit.gpgsign", "false"]);
    write(&repo, "a.txt", "one\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    run_git(&origin, &["init", "--bare", "-b", "main"]);
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);
    run_git(&repo, &["tag", "v1.0.0"]);
    run_git(&repo, &["push", "origin", "refs/tags/v1.0.0"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let output = opened.prune_local_tags_with_output().unwrap();

    assert_eq!(output.exit_code, Some(0));
    assert!(
        output.stdout.contains("No local tags to prune."),
        "unexpected stdout: {}",
        output.stdout
    );
    run_git(
        &repo,
        &["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"],
    );
}
