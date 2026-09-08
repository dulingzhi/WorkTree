use super::support::{
    assert_git_failure, require_git_shell_for_status_integration_tests, run_git, run_git_output,
    write,
};
use std::fs;
use std::path::Path;
use worktree_core::domain::{FileConflictKind, FileStatusKind};
use worktree_core::error::GitFailureId;
use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;

#[test]
fn stash_create_list_apply_and_drop_work() {
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

    opened.stash_create("wip", false, false, &[]).unwrap();
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\n");

    let stashes = opened.stash_list().unwrap();
    assert!(!stashes.is_empty());
    assert_eq!(stashes[0].index, 0);
    assert!(stashes[0].message.contains("wip"));

    opened.stash_apply(0).unwrap();
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );

    opened.stash_drop(0).unwrap();
    let stashes = opened.stash_list().unwrap();
    assert!(stashes.is_empty());
}

#[test]
fn stash_create_with_paths_stashes_only_those_paths() {
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
    write(repo, "b.txt", "bee\n");
    run_git(repo, &["add", "a.txt", "b.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    write(repo, "a.txt", "one\ntwo\n");
    write(repo, "b.txt", "bee\ntwo\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    let paths = [std::path::PathBuf::from("a.txt")];
    opened.stash_create("only-a", false, false, &paths).unwrap();

    // a.txt was stashed back to its committed contents; b.txt kept its edit.
    assert_eq!(fs::read_to_string(repo.join("a.txt")).unwrap(), "one\n");
    assert_eq!(
        fs::read_to_string(repo.join("b.txt")).unwrap(),
        "bee\ntwo\n"
    );

    let stashes = opened.stash_list().unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("only-a"));
}

#[test]
fn stash_create_keep_index_leaves_staged_changes_in_the_worktree() {
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

    write(repo, "a.txt", "one\nstaged\n");
    run_git(repo, &["add", "a.txt"]);
    write(repo, "a.txt", "one\nstaged\nworktree\n");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    opened.stash_create("keep", false, true, &[]).unwrap();

    // --keep-index: the staged half of the change stays in the worktree and
    // the index; only the unstaged remainder was stashed away.
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "one\nstaged\n"
    );
    let staged_diff = run_git_output(repo, &["diff", "--cached", "--name-only"]);
    assert_eq!(staged_diff.trim(), "a.txt");
}

#[test]
fn stash_branch_checks_out_new_branch_applies_and_drops() {
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
    opened.stash_create("wip", false, false, &[]).unwrap();

    opened.stash_branch("recover-wip", 0).unwrap();

    let branch = run_git_output(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
    assert_eq!(branch.trim(), "recover-wip");
    // The stash was applied onto the new branch and dropped on success.
    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert!(opened.stash_list().unwrap().is_empty());
}

#[test]
fn stash_branch_rejects_option_like_branch_names() {
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

    let err = opened
        .stash_branch("--force", 0)
        .expect_err("option-like branch name must be rejected");
    assert!(
        err.to_string().contains("invalid branch name"),
        "unexpected error: {err}"
    );
}

#[test]
fn archive_zip_writes_revision_snapshot() {
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
    write(repo, "b.txt", "two\n");
    run_git(repo, &["add", "b.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add b"],
    );
    run_git(repo, &["tag", "v1.0"]);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // A full sha addresses the first commit's tree.
    let first_sha = run_git_output(repo, &["rev-parse", "HEAD~1"]);
    let zip_head = dir.path().join("head.zip");
    opened
        .archive_zip_with_output(first_sha.trim(), &zip_head)
        .unwrap();
    let bytes = fs::read(&zip_head).unwrap();
    assert!(
        bytes.starts_with(b"PK\x03\x04"),
        "expected a zip local-file header"
    );
    assert!(bytes.len() > 100, "suspiciously small archive");

    // A tag name addresses the same content through a ref.
    let zip_tag = dir.path().join("tag.zip");
    opened.archive_zip_with_output("v1.0", &zip_tag).unwrap();
    assert!(fs::read(&zip_tag).unwrap().starts_with(b"PK\x03\x04"));
}

#[test]
fn archive_zip_rejects_option_like_revisions() {
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

    let err = opened
        .archive_zip_with_output("--output=/tmp/evil", dir.path().join("out.zip").as_path())
        .expect_err("option-like revision must be rejected");
    assert!(
        err.to_string().contains("invalid revision"),
        "unexpected error: {err}"
    );
}

#[test]
fn stash_apply_conflict_is_mergeable() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "a.txt", "base\nstash-change\n");
    opened.stash_create("wip", false, false, &[]).unwrap();

    write(repo, "a.txt", "base\nbranch-change\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "branch-change",
        ],
    );

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply conflict should report failure");
    assert_git_failure(&err, "git stash apply", GitFailureId::StashApplyConflict);
    assert!(
        err.to_string().contains("git stash apply failed"),
        "unexpected error: {err}"
    );

    let status = opened.status().unwrap();
    let conflict_entry = status
        .unstaged
        .iter()
        .find(|entry| entry.path == Path::new("a.txt"))
        .expect("expected conflicted path after stash apply merge");
    assert_eq!(conflict_entry.kind, FileStatusKind::Conflicted);
    assert_eq!(
        conflict_entry.conflict,
        Some(FileConflictKind::BothModified)
    );

    let contents = fs::read_to_string(repo.join("a.txt")).unwrap();
    assert!(contents.contains("<<<<<<<"));
    assert!(contents.contains("======="));
    assert!(contents.contains(">>>>>>>"));
}

#[test]
fn stash_apply_still_errors_when_merge_does_not_start() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "a.txt", "base\nstash-change\n");
    opened.stash_create("wip", false, false, &[]).unwrap();

    write(repo, "a.txt", "base\nlocal-uncommitted-change\n");

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should fail when local edits would be overwritten");
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::WorktreeWouldBeOverwritten,
    );
    assert!(
        err.to_string().contains("overwritten by merge"),
        "unexpected error: {err}"
    );

    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|candidate| candidate.path == Path::new("a.txt"))
        .expect("expected modified file in unstaged status");
    assert_eq!(entry.kind, FileStatusKind::Modified);
    assert_eq!(entry.conflict, None);
}

#[test]
fn stash_apply_tracked_payload_overwriting_untracked_file_is_worktree_overwrite() {
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
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "new.txt", "from stash\n");
    run_git(repo, &["add", "new.txt"]);
    opened.stash_create("wip", false, false, &[]).unwrap();

    write(repo, "new.txt", "local untracked\n");

    let err = opened.stash_apply(0).expect_err(
        "stash apply should fail when tracked stash payload would overwrite an untracked file",
    );
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::WorktreeWouldBeOverwritten,
    );
    assert!(
        err.to_string().contains("overwritten by merge"),
        "unexpected error: {err}"
    );

    assert_eq!(
        fs::read_to_string(repo.join("new.txt")).unwrap(),
        "local untracked\n"
    );
    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|candidate| candidate.path == Path::new("new.txt"))
        .expect("expected blocked untracked file to remain in the worktree");
    assert_eq!(entry.kind, FileStatusKind::Untracked);
    assert_eq!(entry.conflict, None);
}

#[test]
fn stash_apply_staged_overlap_still_merges_into_conflict() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "a.txt", "base\nstash-change\n");
    opened.stash_create("wip", false, false, &[]).unwrap();

    write(repo, "a.txt", "base\nlocal-staged-change\n");
    run_git(repo, &["add", "a.txt"]);

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should report a conflict when only the index overlaps");
    assert_git_failure(&err, "git stash apply", GitFailureId::StashApplyConflict);

    let status = opened.status().unwrap();
    let entry = status
        .unstaged
        .iter()
        .find(|candidate| candidate.path == Path::new("a.txt"))
        .expect("expected conflicted file after stash apply merge");
    assert_eq!(entry.kind, FileStatusKind::Conflicted);
    assert_eq!(entry.conflict, Some(FileConflictKind::BothModified));
}

#[test]
fn stash_apply_allows_merge_when_only_untracked_restore_fails() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "a.txt", "base\nstash-change\n");
    write(repo, "Cargo.toml.orig", "from stash\n");
    opened.stash_create("wip", true, false, &[]).unwrap();

    // Existing untracked file blocks restoration of untracked payload from stash.
    write(repo, "Cargo.toml.orig", "local copy\n");

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should report untracked restore failure");
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::UntrackedRestoreConflict,
    );
    assert!(
        err.to_string()
            .contains("could not restore untracked files from stash")
            || err.to_string().contains("already exists, no checkout"),
        "unexpected error: {err}"
    );

    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "base\nstash-change\n"
    );
    let untracked_merged = fs::read_to_string(repo.join("Cargo.toml.orig")).unwrap();
    assert!(untracked_merged.contains("<<<<<<< Current file"));
    assert!(untracked_merged.contains("local copy"));
    assert!(untracked_merged.contains("======="));
    assert!(untracked_merged.contains("from stash"));
    assert!(untracked_merged.contains(">>>>>>> Stashed file"));

    let status = opened.status().unwrap();
    let tracked = status
        .unstaged
        .iter()
        .find(|candidate| candidate.path == Path::new("a.txt"))
        .expect("expected tracked stash change to be present");
    assert_eq!(tracked.kind, FileStatusKind::Modified);
    assert_eq!(tracked.conflict, None);
    assert!(status.unstaged.iter().any(|candidate| {
        candidate.path == Path::new("Cargo.toml.orig")
            && candidate.kind == FileStatusKind::Untracked
    }));
}

#[test]
fn stash_apply_preserves_original_error_when_untracked_merge_markers_fail() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "Cargo.toml.orig", "from stash\n");
    opened.stash_create("wip", true, false, &[]).unwrap();

    let local_binary = b"\xff\xfe\x00\x80";
    write(repo, "Cargo.toml.orig", local_binary);

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should still report the original untracked restore failure");
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::UntrackedRestoreConflict,
    );
    assert!(
        !err.to_string().contains("cannot merge binary"),
        "unexpected recovery error replaced the original stash failure: {err}"
    );
    assert_eq!(
        fs::read(repo.join("Cargo.toml.orig")).unwrap(),
        local_binary,
    );
}

#[test]
fn stash_apply_allows_untracked_restore_failure_when_stash_has_tracked_payload() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    // Stash contains tracked and untracked payload.
    write(repo, "a.txt", "base\nstash-change\n");
    write(repo, "Cargo.toml.orig", "from stash\n");
    opened.stash_create("wip", true, false, &[]).unwrap();

    // Apply the same tracked change on the branch first, so stash apply has no
    // tracked-status delta even though stash had tracked payload.
    write(repo, "a.txt", "base\nstash-change\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "same-tracked-change",
        ],
    );

    // Existing untracked file blocks restoration of stash untracked payload.
    write(repo, "Cargo.toml.orig", "local copy\n");

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should report untracked restore failure");
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::UntrackedRestoreConflict,
    );
    assert!(
        err.to_string()
            .contains("could not restore untracked files from stash")
            || err.to_string().contains("already exists, no checkout"),
        "unexpected error: {err}"
    );

    assert_eq!(
        fs::read_to_string(repo.join("a.txt")).unwrap(),
        "base\nstash-change\n"
    );
    let untracked_merged = fs::read_to_string(repo.join("Cargo.toml.orig")).unwrap();
    assert!(untracked_merged.contains("<<<<<<< Current file"));
    assert!(untracked_merged.contains("local copy"));
    assert!(untracked_merged.contains("======="));
    assert!(untracked_merged.contains("from stash"));
    assert!(untracked_merged.contains(">>>>>>> Stashed file"));

    let status = opened.status().unwrap();
    assert!(
        status
            .unstaged
            .iter()
            .all(|entry| entry.path != Path::new("a.txt"))
    );
    assert!(status.unstaged.iter().any(|candidate| {
        candidate.path == Path::new("Cargo.toml.orig")
            && candidate.kind == FileStatusKind::Untracked
    }));
}

#[test]
fn stash_apply_merges_when_only_untracked_restore_fails_without_tracked_changes() {
    if !require_git_shell_for_status_integration_tests() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();

    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    write(repo, "a.txt", "base\nline\n");
    run_git(repo, &["add", "a.txt"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();

    write(repo, "Cargo.toml.orig", "from stash\n");
    opened.stash_create("wip", true, false, &[]).unwrap();

    write(repo, "Cargo.toml.orig", "local copy\n");

    let err = opened
        .stash_apply(0)
        .expect_err("stash apply should report untracked restore failure");
    assert_git_failure(
        &err,
        "git stash apply",
        GitFailureId::UntrackedRestoreConflict,
    );
    assert!(
        err.to_string()
            .contains("could not restore untracked files from stash")
            || err.to_string().contains("already exists, no checkout"),
        "unexpected error: {err}"
    );

    let contents = fs::read_to_string(repo.join("Cargo.toml.orig")).unwrap();
    assert!(contents.contains("<<<<<<< Current file"));
    assert!(contents.contains("local copy"));
    assert!(contents.contains("======="));
    assert!(contents.contains("from stash"));
    assert!(contents.contains(">>>>>>> Stashed file"));

    let status = opened.status().unwrap();
    assert!(status.unstaged.iter().any(|entry| {
        entry.path == Path::new("Cargo.toml.orig") && entry.kind == FileStatusKind::Untracked
    }));
}

#[test]
fn stash_list_reports_reflog_indices_for_drop() {
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
    opened.stash_create("wip-1", false, false, &[]).unwrap();

    write(repo, "a.txt", "one\nthree\n");
    opened.stash_create("wip-2", false, false, &[]).unwrap();

    let stashes = opened.stash_list().unwrap();
    assert_eq!(stashes.len(), 2);
    assert_eq!(stashes[0].index, 0);
    assert_eq!(stashes[1].index, 1);

    // Drop the older stash by the index returned from `stash_list`.
    opened.stash_drop(stashes[1].index).unwrap();
    let stashes = opened.stash_list().unwrap();
    assert_eq!(stashes.len(), 1);
    assert_eq!(stashes[0].index, 0);
    assert!(stashes[0].message.contains("wip-2"));
}
