use super::support::{
    assert_git_failure, git_command, git_path_arg, git_remote_url,
    require_git_shell_for_status_integration_tests, run_git, run_git_output, write,
};
use std::fs;
use worktree_core::error::{ErrorKind, GitFailureId};
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;

#[test]
fn list_remote_tags_collects_sorted_results_and_skips_unavailable_remote() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let origin = dir.path().join("origin.git");
    let backup = dir.path().join("backup.git");
    let missing = dir.path().join("missing.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&backup).unwrap();

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
    run_git(&backup, &["init", "--bare", "-b", "main"]);
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(
        &repo,
        &["remote", "add", "backup", git_remote_url(&backup).as_str()],
    );
    run_git(
        &repo,
        &["remote", "add", "broken", git_remote_url(&missing).as_str()],
    );

    run_git(&repo, &["tag", "origin-tag"]);
    run_git(&repo, &["tag", "backup-tag"]);
    run_git(&repo, &["push", "origin", "refs/tags/origin-tag"]);
    run_git(&repo, &["push", "backup", "refs/tags/backup-tag"]);

    let head = run_git_output(&repo, &["rev-parse", "HEAD"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let remote_tags = opened.list_remote_tags().unwrap();
    let tuples = remote_tags
        .iter()
        .map(|tag| {
            (
                tag.remote.as_str(),
                tag.name.as_str(),
                tag.target.as_ref().to_string(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        tuples,
        vec![
            ("backup", "backup-tag", head.clone()),
            ("origin", "origin-tag", head)
        ]
    );
}

#[test]
fn list_remote_branches_includes_fetched_remote_tracking_refs() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    let origin = dir.path().join("origin.git");
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

    fs::create_dir_all(&origin).unwrap();
    run_git(&origin, &["init", "--bare", "-b", "main"]);
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);

    run_git(&repo, &["checkout", "-b", "feature"]);
    write(&repo, "b.txt", "feature\n");
    run_git(&repo, &["add", "b.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&repo, &["push", "-u", "origin", "feature"]);
    run_git(&repo, &["fetch", "origin"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let branches = opened.list_remote_branches().unwrap();

    assert!(
        branches
            .iter()
            .any(|b| b.remote == "origin" && b.name == "main")
    );
    assert!(
        branches
            .iter()
            .any(|b| b.remote == "origin" && b.name == "feature")
    );
    assert!(!branches.iter().any(|b| b.name == "HEAD"));
}

#[test]
fn checkout_remote_branch_creates_tracking_branch_when_missing_locally() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    run_git(&seed, &["checkout", "-b", "feature"]);
    write(&seed, "feature.txt", "feature\n");
    run_git(&seed, &["add", "feature.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&seed, &["push", "-u", "origin", "feature"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    opened
        .checkout_remote_branch("origin", "feature", "feature")
        .unwrap();

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "feature");

    let upstream = run_git_output(
        &clone,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    assert_eq!(upstream, "origin/feature");
}

#[test]
fn checkout_pull_request_fetches_pr_ref_into_local_branch() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    // Simulate a GitHub pull request: refs/pull/7/head pointing at a commit
    // that no branch carries (the PR author's pushed tip).
    run_git(&seed, &["checkout", "-b", "pr-work"]);
    write(&seed, "pr.txt", "pull request change\n");
    run_git(&seed, &["add", "pr.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "pr change"],
    );
    run_git(&seed, &["push", "origin", "pr-work"]);
    let pr_tip = run_git_output(&seed, &["rev-parse", "HEAD"]);
    run_git(&origin, &["update-ref", "refs/pull/7/head", &pr_tip]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    opened.checkout_pull_request("origin", 7).unwrap();

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "pr/7");
    let local_tip = run_git_output(&clone, &["rev-parse", "pr/7"]);
    assert_eq!(local_tip, pr_tip);
    assert!(clone.join("pr.txt").exists());
}

#[test]
fn checkout_pull_request_reuses_existing_local_branch_without_moving_it() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);
    let main_tip = run_git_output(&seed, &["rev-parse", "main"]);

    // The tip itself is never asserted — the review commit's parent is
    // the PR tip by construction.
    let _ = {
        run_git(&seed, &["checkout", "-b", "pr-work"]);
        write(&seed, "pr.txt", "pull request change\n");
        run_git(&seed, &["add", "pr.txt"]);
        run_git(
            &seed,
            &["-c", "commit.gpgsign=false", "commit", "-m", "pr change"],
        );
        let tip = run_git_output(&seed, &["rev-parse", "HEAD"]);
        run_git(&seed, &["push", "origin", "pr-work"]);
        run_git(&origin, &["update-ref", "refs/pull/7/head", &tip]);
        tip
    };

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );

    // A previous checkout already created pr/7 — at the PR tip, with the
    // user's own review commit on top of it.
    run_git(&clone, &["config", "user.email", "you@example.com"]);
    run_git(&clone, &["config", "user.name", "You"]);
    run_git(&clone, &["config", "commit.gpgsign", "false"]);
    run_git(&clone, &["fetch", "origin", "refs/pull/7/head"]);
    run_git(&clone, &["checkout", "-b", "pr/7", "FETCH_HEAD"]);
    write(&clone, "review.txt", "review note\n");
    run_git(&clone, &["add", "review.txt"]);
    run_git(
        &clone,
        &["-c", "commit.gpgsign=false", "commit", "-m", "review"],
    );
    let review_tip = run_git_output(&clone, &["rev-parse", "HEAD"]);
    run_git(&clone, &["checkout", "main"]);

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    opened.checkout_pull_request("origin", 7).unwrap();

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "pr/7");
    let branch_tip = run_git_output(&clone, &["rev-parse", "pr/7"]);
    assert_eq!(
        branch_tip, review_tip,
        "an existing pr/<N> branch keeps the user's commits; the PR tip is not force-moved"
    );
    assert_ne!(branch_tip, main_tip);
}

#[test]
fn checkout_remote_branch_existing_local_branch_updates_upstream_and_checks_out() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    run_git(&seed, &["checkout", "-b", "feature"]);
    write(&seed, "feature.txt", "feature\n");
    run_git(&seed, &["add", "feature.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&seed, &["push", "-u", "origin", "feature"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );
    run_git(&clone, &["checkout", "-b", "topic"]);
    run_git(&clone, &["checkout", "main"]);

    let upstream_before = git_command()
        .arg("-C")
        .arg(&clone)
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "topic@{upstream}",
        ])
        .status()
        .expect("topic upstream probe");
    assert!(
        !upstream_before.success(),
        "topic should start without upstream tracking"
    );

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    opened
        .checkout_remote_branch("origin", "feature", "topic")
        .unwrap();

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "topic");
    let upstream = run_git_output(
        &clone,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    assert_eq!(upstream, "origin/feature");
}

#[test]
fn checkout_remote_branch_sees_local_branch_created_after_backend_open() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    run_git(&seed, &["checkout", "-b", "feature"]);
    write(&seed, "feature.txt", "feature\n");
    run_git(&seed, &["add", "feature.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&seed, &["push", "-u", "origin", "feature"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();

    run_git(&clone, &["checkout", "-b", "topic"]);
    run_git(&clone, &["checkout", "main"]);

    let upstream_before = git_command()
        .arg("-C")
        .arg(&clone)
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "topic@{upstream}",
        ])
        .status()
        .expect("topic upstream probe");
    assert!(
        !upstream_before.success(),
        "topic should start without upstream tracking"
    );

    opened
        .checkout_remote_branch("origin", "feature", "topic")
        .unwrap();

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "topic");
    let upstream = run_git_output(
        &clone,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    );
    assert_eq!(upstream, "origin/feature");
}

#[test]
fn checkout_remote_branch_returns_structured_git_error_for_missing_remote_branch() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&repo).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

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
    run_git(
        &repo,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);
    run_git(&repo, &["fetch", "origin"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    let err = opened
        .checkout_remote_branch("origin", "missing-branch", "topic")
        .expect_err("missing remote branch should return structured git error");
    match err.kind() {
        ErrorKind::Git(failure) => {
            assert_eq!(failure.id(), GitFailureId::CommandFailed);
            assert_eq!(failure.command(), "git checkout --track");
            assert!(
                failure.exit_code().is_some(),
                "git checkout failure should preserve exit code"
            );
            assert!(
                failure
                    .detail()
                    .is_some_and(|detail| !detail.trim().is_empty()),
                "git checkout failure should preserve stderr detail"
            );
        }
        other => panic!("expected structured git error, got {other:?}"),
    }
}

#[test]
fn checkout_remote_branch_with_existing_local_branch_and_missing_remote_keeps_head_unchanged() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );
    run_git(&clone, &["checkout", "-b", "topic"]);
    run_git(&clone, &["checkout", "main"]);

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    let err = opened
        .checkout_remote_branch("origin", "missing-branch", "topic")
        .expect_err("missing remote branch should not switch to the existing local branch");
    assert_git_failure(&err, "git checkout --track", GitFailureId::CommandFailed);

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "main");

    let upstream = git_command()
        .arg("-C")
        .arg(&clone)
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "topic@{upstream}",
        ])
        .status()
        .expect("topic upstream probe");
    assert!(
        !upstream.success(),
        "topic should remain without upstream tracking after the failed checkout"
    );
}

#[test]
fn checkout_remote_branch_dirty_worktree_failure_does_not_create_local_branch() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let seed = dir.path().join("seed");
    let clone = dir.path().join("clone");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&seed).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&seed, &["init", "-b", "main"]);
    run_git(&seed, &["config", "user.email", "you@example.com"]);
    run_git(&seed, &["config", "user.name", "You"]);
    run_git(&seed, &["config", "commit.gpgsign", "false"]);
    write(&seed, "a.txt", "one\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &seed,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&seed, &["push", "-u", "origin", "main"]);

    run_git(&seed, &["checkout", "-b", "feature"]);
    write(&seed, "a.txt", "feature\n");
    run_git(&seed, &["add", "a.txt"]);
    run_git(
        &seed,
        &["-c", "commit.gpgsign=false", "commit", "-m", "feature"],
    );
    run_git(&seed, &["push", "-u", "origin", "feature"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&clone).as_str(),
        ],
    );
    write(&clone, "a.txt", "dirty\n");

    let backend = GixBackend;
    let opened = backend.open(&clone).unwrap();
    let err = opened
        .checkout_remote_branch("origin", "feature", "topic")
        .expect_err("dirty checkout should fail");
    assert_git_failure(&err, "git checkout --track", GitFailureId::CommandFailed);

    let topic_exists = git_command()
        .arg("-C")
        .arg(&clone)
        .args(["show-ref", "--verify", "--quiet", "refs/heads/topic"])
        .status()
        .expect("show-ref topic");
    assert!(
        !topic_exists.success(),
        "topic branch should not be created when checkout fails"
    );

    let head = run_git_output(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(head, "main");
    assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "dirty\n");
}

#[test]
fn push_and_delete_remote_tag() {
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
    run_git(&repo, &["config", "tag.gpgsign", "false"]);

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

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();

    opened
        .create_tag_with_output("v1.0.0", "HEAD", None, false)
        .unwrap();
    opened.push_tag_with_output("origin", "v1.0.0").unwrap();
    run_git(
        &origin,
        &["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"],
    );

    opened
        .delete_remote_tag_with_output("origin", "v1.0.0")
        .unwrap();
    let deleted = git_command()
        .arg("-C")
        .arg(&origin)
        .args(["show-ref", "--verify", "--quiet", "refs/tags/v1.0.0"])
        .status()
        .expect("show-ref");
    assert!(!deleted.success(), "expected remote tag to be deleted");
}

#[test]
fn push_with_output_updates_remote_head() {
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

    write(&repo, "a.txt", "one\ntwo\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    let head_local = git_command()
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse HEAD");
    assert!(head_local.status.success());
    let head_local = String::from_utf8(head_local.stdout)
        .unwrap()
        .trim()
        .to_string();

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    opened.push_with_output().unwrap();

    let head_remote = git_command()
        .arg("-C")
        .arg(&origin)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("rev-parse origin/main");
    assert!(head_remote.status.success());
    let head_remote = String::from_utf8(head_remote.stdout)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(head_remote, head_local);
}

#[test]
fn force_push_with_output_updates_remote_head_after_rewrite() {
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

    write(&repo, "a.txt", "one\ntwo\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    run_git(&repo, &["push"]);
    run_git(&repo, &["fetch", "origin"]);

    // Rewrite local history so it diverges from the remote.
    run_git(&repo, &["reset", "--hard", "HEAD~1"]);
    write(&repo, "a.txt", "one\ntwo (rewritten)\n");
    run_git(&repo, &["add", "a.txt"]);
    run_git(
        &repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "second rewritten",
        ],
    );
    let head_local = git_command()
        .arg("-C")
        .arg(&repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse HEAD");
    assert!(head_local.status.success());
    let head_local = String::from_utf8(head_local.stdout)
        .unwrap()
        .trim()
        .to_string();

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();
    opened.push_force_with_output().unwrap();

    let head_remote = git_command()
        .arg("-C")
        .arg(&origin)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("rev-parse refs/heads/main");
    assert!(head_remote.status.success());
    let head_remote = String::from_utf8(head_remote.stdout)
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(head_remote, head_local);
}

#[test]
fn pull_with_output_fast_forwards_from_remote() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let repo_a = dir.path().join("repo-a");
    let repo_b = dir.path().join("repo-b");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&repo_a).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&repo_a, &["init", "-b", "main"]);
    run_git(&repo_a, &["config", "user.email", "you@example.com"]);
    run_git(&repo_a, &["config", "user.name", "You"]);
    run_git(&repo_a, &["config", "commit.gpgsign", "false"]);
    write(&repo_a, "a.txt", "one\n");
    run_git(&repo_a, &["add", "a.txt"]);
    run_git(
        &repo_a,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &repo_a,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo_a, &["push", "-u", "origin", "main"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&repo_b).as_str(),
        ],
    );

    write(&repo_a, "a.txt", "one\ntwo\n");
    run_git(&repo_a, &["add", "a.txt"]);
    run_git(
        &repo_a,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    run_git(&repo_a, &["push"]);

    let head_origin = git_command()
        .arg("-C")
        .arg(&origin)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("rev-parse origin");
    assert!(head_origin.status.success());
    let head_origin = String::from_utf8(head_origin.stdout)
        .unwrap()
        .trim()
        .to_string();

    let backend = GixBackend;
    let opened_b = backend.open(&repo_b).unwrap();
    opened_b
        .pull_with_output(worktree_core::services::PullMode::FastForwardOnly)
        .unwrap();

    let head_b = git_command()
        .arg("-C")
        .arg(&repo_b)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse b");
    assert!(head_b.status.success());
    let head_b = String::from_utf8(head_b.stdout).unwrap().trim().to_string();
    assert_eq!(head_b, head_origin);
}

#[test]
fn pull_with_output_fast_forwards_when_possible_even_if_pull_ff_is_disabled() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let repo_a = dir.path().join("repo-a");
    let repo_b = dir.path().join("repo-b");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&repo_a).unwrap();

    run_git(&origin, &["init", "--bare", "-b", "main"]);

    run_git(&repo_a, &["init", "-b", "main"]);
    run_git(&repo_a, &["config", "user.email", "you@example.com"]);
    run_git(&repo_a, &["config", "user.name", "You"]);
    run_git(&repo_a, &["config", "commit.gpgsign", "false"]);
    write(&repo_a, "a.txt", "one\n");
    run_git(&repo_a, &["add", "a.txt"]);
    run_git(
        &repo_a,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );
    run_git(
        &repo_a,
        &["remote", "add", "origin", git_remote_url(&origin).as_str()],
    );
    run_git(&repo_a, &["push", "-u", "origin", "main"]);

    run_git(
        dir.path(),
        &[
            "clone",
            git_remote_url(&origin).as_str(),
            git_path_arg(&repo_b).as_str(),
        ],
    );

    run_git(&repo_b, &["config", "user.email", "you@example.com"]);
    run_git(&repo_b, &["config", "user.name", "You"]);
    run_git(&repo_b, &["config", "commit.gpgsign", "false"]);
    run_git(&repo_b, &["config", "pull.ff", "false"]);

    write(&repo_a, "a.txt", "one\ntwo\n");
    run_git(&repo_a, &["add", "a.txt"]);
    run_git(
        &repo_a,
        &["-c", "commit.gpgsign=false", "commit", "-m", "second"],
    );
    run_git(&repo_a, &["push"]);

    let head_origin = git_command()
        .arg("-C")
        .arg(&origin)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("rev-parse origin");
    assert!(head_origin.status.success());
    let head_origin = String::from_utf8(head_origin.stdout)
        .unwrap()
        .trim()
        .to_string();

    let backend = GixBackend;
    let opened_b = backend.open(&repo_b).unwrap();
    opened_b
        .pull_with_output(worktree_core::services::PullMode::Merge)
        .unwrap();

    let head_b = git_command()
        .arg("-C")
        .arg(&repo_b)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse b");
    assert!(head_b.status.success());
    let head_b = String::from_utf8(head_b.stdout).unwrap().trim().to_string();
    assert_eq!(head_b, head_origin);

    let parents = git_command()
        .arg("-C")
        .arg(&repo_b)
        .args(["rev-list", "--parents", "-n", "1", "HEAD"])
        .output()
        .expect("rev-list --parents");
    assert!(parents.status.success());
    let parent_count = String::from_utf8(parents.stdout)
        .unwrap()
        .split_whitespace()
        .count()
        .saturating_sub(1);
    assert_eq!(parent_count, 1, "expected fast-forward");
}

#[test]
fn remote_ssh_key_set_and_clear_round_trip() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let key = dir.path().join("id_ed25519");
    opened
        .set_remote_ssh_key_with_output("origin", Some(key.to_str().unwrap()))
        .unwrap();
    assert_eq!(
        run_git_output(repo, &["config", "--get", "remote.origin.sshkey"]),
        key.to_str().unwrap()
    );

    // Clearing a configured key removes the config entry. `git config --get`
    // exits 1 when unset, which run_git_output reports as failure.
    opened
        .set_remote_ssh_key_with_output("origin", None)
        .unwrap();
    let cleared = git_command()
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", "remote.origin.sshkey"])
        .output()
        .expect("git command to run");
    assert!(
        !cleared.status.success(),
        "sshkey config should be gone, got: {:?}",
        String::from_utf8_lossy(&cleared.stdout)
    );

    // Clearing again when already unset is a no-op success.
    opened
        .set_remote_ssh_key_with_output("origin", None)
        .unwrap();
}

#[test]
fn remote_ssh_key_rejects_option_like_paths() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let err = opened
        .set_remote_ssh_key_with_output("origin", Some("--global"))
        .expect_err("option-like key path must be rejected");
    assert!(
        err.to_string().contains("invalid ssh key path"),
        "unexpected error: {err}"
    );
}

#[test]
fn fetch_and_push_succeed_with_remote_ssh_key_configured() {
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

    let backend = GixBackend;
    let opened = backend.open(&repo).unwrap();

    // A file://-style local transport never consults ssh, so a configured
    // key only proves the injection composes with these commands.
    let key = dir.path().join("id_ed25519");
    opened
        .set_remote_ssh_key_with_output("origin", Some(key.to_str().unwrap()))
        .unwrap();

    opened.fetch_all_with_output_prune(false).unwrap();
    opened
        .push_set_upstream_with_output("origin", "main")
        .unwrap();
    let remote_head = run_git_output(&origin, &["rev-parse", "HEAD"]);
    let local_head = run_git_output(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(remote_head, local_head);
}
