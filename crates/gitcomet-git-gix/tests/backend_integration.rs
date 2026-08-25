use gitcomet_core::error::ErrorKind;
use gitcomet_core::path_utils::canonicalize_or_original;
use gitcomet_core::services::{GitBackend, RepoCapabilities};
use gitcomet_git_gix::GixBackend;
use std::fs;
use std::path::Path;
use std::process::Command;

fn run_git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .expect("run git command");
    assert!(status.success(), "git {:?} failed", args);
}

/// `run_git`, but capturing stdout (trimmed by the caller).
fn run_git_capture(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// `git init` a non-bare repo at `path` with one commit, using an inline
/// identity so the test does not depend on ambient git config.
fn init_repo_with_commit(path: &Path) {
    fs::create_dir_all(path).expect("create repo directory");
    run_git(path, &["init"]);
    fs::write(path.join("file.txt"), "contents").expect("write file");
    run_git(path, &["add", "."]);
    run_git(
        path,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "init",
        ],
    );
}

#[test]
fn gix_backend_open_succeeds_for_git_repository() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");

    run_git(&repo, &["init"]);

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open repository");
    assert_eq!(
        opened.spec().workdir,
        canonicalize_or_original(repo.clone())
    );
    assert_eq!(opened.capabilities(), RepoCapabilities::default());
}

#[test]
fn gix_backend_open_detects_colocated_jj_repo_as_read_only() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");

    run_git(&repo, &["init"]);
    // A colocated Jujutsu repo is a git repo with a `.jj` directory beside
    // `.git`; the marker alone decides detection, no jj binary is involved.
    fs::create_dir_all(repo.join(".jj")).expect("create .jj marker directory");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open repository");
    // The adapter re-enables `commits` on top of the detected read-only set —
    // the one write routed through the jj CLI.
    assert_eq!(
        opened.capabilities(),
        RepoCapabilities {
            commits: true,
            ..RepoCapabilities::jj_read_only()
        }
    );
}

/// Skips tests that shell out to a real `jj` binary on machines without it
/// (the fake-`.jj`-marker test above still runs everywhere).
fn jj_available_for_integration_tests() -> bool {
    std::process::Command::new("jj")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Run `jj git init --colocate` in `repo`, panicking with the command's
/// output on failure.
fn colocate_with_jj(repo: &Path) {
    let output = std::process::Command::new("jj")
        .arg("git")
        .arg("init")
        .arg("--colocate")
        .current_dir(repo)
        .output()
        .expect("run jj git init --colocate");
    assert!(
        output.status.success(),
        "jj git init --colocate failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Run `jj log --no-graph -r @ -T <template>` and return the trimmed output.
fn jj_log_at_working_copy(repo: &Path, template: &str) -> String {
    let output = std::process::Command::new("jj")
        .arg("log")
        .arg("--no-graph")
        .arg("-r")
        .arg("@")
        .arg("-T")
        .arg(template)
        .current_dir(repo)
        .output()
        .expect("run jj log");
    assert!(
        output.status.success(),
        "jj log -r @ -T {template} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// The current working-copy commit id (`@`), read straight from jj.
fn jj_working_copy_commit_id(repo: &Path) -> String {
    jj_log_at_working_copy(repo, "commit_id")
}

/// The current working-copy commit's description, read straight from jj.
fn jj_working_copy_description(repo: &Path) -> String {
    jj_log_at_working_copy(repo, "description")
}

/// The P1 adapter contract, checked without a real jj install: a colocated
/// repo (fake `.jj` marker) opens through `JjRepository`, every read still
/// answers through gix, and every write refuses with `Unsupported`.
#[test]
fn jj_adapter_delegates_reads_and_refuses_writes() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    fs::create_dir_all(repo.join(".jj")).expect("create .jj marker directory");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open repository");
    // The adapter re-enables `commits` (routed describe+new) on top of the
    // detected read-only set.
    assert_eq!(
        opened.capabilities(),
        RepoCapabilities {
            commits: true,
            ..RepoCapabilities::jj_read_only()
        }
    );

    // Reads still answer through the composed gix repo.
    let page = opened
        .log_head_page(10, None)
        .expect("adapter delegates log reads");
    assert!(
        page.commits
            .iter()
            .any(|commit| commit.summary.contains("init"))
    );
    let branches = opened.list_branches().expect("list branches");
    assert_eq!(
        branches.len(),
        1,
        "exactly one branch after init: {branches:?}"
    );
    assert_eq!(
        opened.current_branch().expect("current branch"),
        branches[0].name,
        "current branch must match the initial branch regardless of init.defaultBranch"
    );

    // Writes refuse with `Unsupported` — the second lock behind the reducer's
    // capability gate. Commit is the exception: it is routed through the jj
    // CLI, so it is covered by the colocated tests below instead of here (a
    // fake `.jj` marker is not a repo jj can operate on).
    for unsupported in [
        opened.stage(&[Path::new("file.txt")]).err(),
        opened.create_branch("topic", &page.commits[0].id).err(),
        opened.fetch_all().err(),
    ] {
        let err = unsupported.expect("write must fail");
        assert!(
            matches!(err.kind(), gitcomet_core::error::ErrorKind::Unsupported(_)),
            "expected Unsupported, got {err:?}"
        );
    }
}

/// Opens a genuinely colocated repo (created by `jj git init --colocate`) and
/// pins the L1 contract end to end: detection reports read-only, reads still
/// work through gix, and the working copy stays clean because a colocated jj
/// repo syncs the git index.
#[test]
fn gix_backend_reads_real_colocated_jj_repo() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    colocate_with_jj(&repo);

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");
    // The adapter re-enables `commits` (routed describe+new) on top of the
    // detected read-only set.
    assert_eq!(
        opened.capabilities(),
        RepoCapabilities {
            commits: true,
            ..RepoCapabilities::jj_read_only()
        }
    );

    // History reads must keep working: the initial git commit remains
    // reachable through the jj-managed refs.
    let page = opened.log_head_page(10, None).expect("read log head page");
    assert!(
        page.commits
            .iter()
            .any(|commit| commit.summary.contains("init")),
        "expected the git init commit in the log, got {:?}",
        page.commits
            .iter()
            .map(|commit| &*commit.summary)
            .collect::<Vec<_>>()
    );

    // A colocated repo exports the working-copy state to the git index, so
    // plain-git status reads as clean — no phantom modifications.
    let status = opened.status().expect("read status");
    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "colocated working copy should read clean, got {status:?}"
    );
}

/// The snapshot trigger: a status read through the adapter runs a throttled
/// `jj st`, absorbing working-copy edits into `@` on the jj side while the
/// git-side view stays untouched (`@` is invisible to git; HEAD/index keep
/// `@`'s parent, so the unstaged lane keeps showing the edit — that IS the jj
/// working-copy view). A second read inside the throttle interval must not
/// spawn jj again.
#[test]
fn jj_status_read_snapshots_working_copy_and_throttles_repeats() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    colocate_with_jj(&repo);

    let before = jj_working_copy_commit_id(&repo);
    fs::write(repo.join("file.txt"), "contents\nedited").expect("edit tracked file");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");

    // The first status read claims the snapshot slot and runs `jj st`.
    let status = opened.status().expect("read status");
    let after_first = jj_working_copy_commit_id(&repo);
    assert_ne!(
        before, after_first,
        "the status read must snapshot the working-copy edit into @"
    );
    assert!(
        status.staged.is_empty(),
        "colocated staged lane reads empty, got {:?}",
        status.staged
    );
    assert!(
        status
            .unstaged
            .iter()
            .any(|entry| entry.path == Path::new("file.txt")),
        "the edit stays in the unstaged lane, got {:?}",
        status.unstaged
    );

    // An immediate second read is throttled: no new snapshot, no new `@`.
    opened.status().expect("read status again");
    let after_second = jj_working_copy_commit_id(&repo);
    assert_eq!(
        after_first, after_second,
        "a second read inside the throttle interval must not re-run jj"
    );
}

/// The P1 status semantics on a colocated repo, which plain delegation already
/// provides: the staged lane is empty (HEAD and the index both sit at `@`'s
/// parent — jj has no staging area), and the unstaged lane IS the jj
/// working-copy change (`diff(@^, worktree)` — it reads the worktree directly,
/// so it is fresh even between snapshots). Covers the full entry spectrum:
/// a modified tracked file, a brand-new untracked file, and a deleted one.
#[test]
fn jj_colocated_status_maps_working_copy_change_to_unstaged_only() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    fs::write(repo.join("doomed.txt"), "to be removed").expect("write second tracked file");
    run_git(&repo, &["add", "doomed.txt"]);
    run_git(
        &repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "second",
        ],
    );
    colocate_with_jj(&repo);

    // The working-copy change under test: modify one tracked file, create an
    // untracked one, delete another tracked one.
    fs::write(repo.join("file.txt"), "contents\nedited").expect("modify tracked file");
    fs::write(repo.join("new.txt"), "brand new").expect("create untracked file");
    fs::remove_file(repo.join("doomed.txt")).expect("delete tracked file");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");

    let status = opened.status().expect("read status");
    assert!(
        status.staged.is_empty(),
        "jj has no staging area — the staged lane must read empty, got {:?}",
        status.staged
    );
    let kind_of = |path: &str| {
        status
            .unstaged
            .iter()
            .find(|entry| entry.path == Path::new(path))
            .map(|entry| entry.kind)
    };
    assert_eq!(
        kind_of("file.txt"),
        Some(gitcomet_core::domain::FileStatusKind::Modified),
        "a modified tracked file lands in the unstaged lane: {:?}",
        status.unstaged
    );
    // A brand-new file flips Untracked → Added across a snapshot: jj's
    // snapshot adds new working-copy files to the git index as intent-to-add
    // entries (`git status --short` shows the same as ` A`), which gix
    // classifies in the index→worktree pass — the unstaged lane, exactly
    // where it belongs. Either way the staged lane above stays empty.
    assert_eq!(
        kind_of("new.txt"),
        Some(gitcomet_core::domain::FileStatusKind::Added),
        "a brand-new file lands in the unstaged lane: {:?}",
        status.unstaged
    );
    assert_eq!(
        kind_of("doomed.txt"),
        Some(gitcomet_core::domain::FileStatusKind::Deleted),
        "a deleted tracked file lands in the unstaged lane: {:?}",
        status.unstaged
    );
}

/// The detached-HEAD shape jj leaves when `@`'s parent actually moves
/// (`jj new <older-sha>`): git HEAD detaches at the new parent, the index is
/// synced, and the worktree is rewritten to match, so status reads clean and
/// `current_branch` falls back to "HEAD" like any detached git repo — no jj
/// special-casing needed. (When the parent does NOT move, jj leaves HEAD on
/// its branch — the detach only happens on a real move.)
#[test]
fn jj_colocated_detached_head_after_jj_new_reads_clean() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    fs::write(repo.join("second.txt"), "second").expect("write second file");
    run_git(&repo, &["add", "second.txt"]);
    run_git(
        &repo,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "second",
        ],
    );
    colocate_with_jj(&repo);

    // Move @'s parent back to the init commit; jj rewrites the working copy
    // to its tree, detaches git HEAD there, and syncs the index.
    let init_sha = run_git_capture(&repo, &["rev-parse", "HEAD~1"])
        .trim()
        .to_string();
    let jj_new = std::process::Command::new("jj")
        .arg("new")
        .arg(&init_sha)
        .current_dir(&repo)
        .output()
        .expect("run jj new");
    assert!(
        jj_new.status.success(),
        "jj new {init_sha} failed: {}",
        String::from_utf8_lossy(&jj_new.stderr)
    );

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");

    let status = opened.status().expect("read status");
    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "jj synced the index to the moved working copy — status must read clean, got {status:?}"
    );
    assert_eq!(
        opened.current_branch().expect("current branch"),
        "HEAD",
        "detached HEAD falls back to the generic label"
    );
    assert_eq!(
        opened.head_commit_id().expect("head commit id"),
        Some(gitcomet_core::domain::CommitId(init_sha.into())),
        "git HEAD sits at @'s parent after jj new"
    );
}

/// The routed commit: describe the working-copy change, then open a fresh
/// one. After it the described change is git HEAD (detached) and shows in
/// the log, the working copy reads clean, and jj has a fresh empty `@` on
/// top — the exact state the next describe+new cycle starts from.
#[test]
fn jj_commit_routes_describe_then_new() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    colocate_with_jj(&repo);
    fs::write(repo.join("file.txt"), "contents\nedited").expect("edit tracked file");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");
    let capabilities = opened.capabilities();
    assert!(
        capabilities.is_jj && capabilities.read_only && capabilities.commits,
        "commit is the one routed write on a read-only jj repo: {capabilities:?}"
    );

    opened.commit("first jj commit").expect("routed commit");

    // `jj new` moved git HEAD to the described commit; the same handle must
    // see the move (the app never re-opens for it).
    let head = run_git_capture(&repo, &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    assert_ne!(head, "", "rev-parse HEAD must answer");
    assert_eq!(
        opened.head_commit_id().expect("head commit id"),
        Some(gitcomet_core::domain::CommitId(head.into())),
        "git HEAD sits at the described commit after the routed commit"
    );

    let page = opened
        .log_head_page(10, None)
        .expect("log includes the described commit");
    assert!(
        page.commits
            .iter()
            .any(|commit| commit.summary.contains("first jj commit")),
        "expected the routed commit in the log, got {:?}",
        page.commits
            .iter()
            .map(|commit| &*commit.summary)
            .collect::<Vec<_>>()
    );

    let status = opened.status().expect("read status");
    assert!(
        status.staged.is_empty() && status.unstaged.is_empty(),
        "the fresh @ re-synced index and worktree — status must read clean, got {status:?}"
    );
    assert_eq!(
        jj_working_copy_description(&repo),
        "",
        "jj new leaves a fresh, undescribed working-copy commit"
    );
}

/// The routed amend: describe only. The working-copy change keeps absorbing
/// edits (the unstaged lane keeps showing them) and git HEAD does not move.
#[test]
fn jj_commit_amend_describes_the_working_copy_change() {
    if !jj_available_for_integration_tests() {
        eprintln!("skipping: jj binary not found in PATH");
        return;
    }

    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    fs::create_dir_all(&repo).expect("create repo directory");
    init_repo_with_commit(&repo);
    colocate_with_jj(&repo);
    fs::write(repo.join("file.txt"), "contents\nedited").expect("edit tracked file");

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open colocated repository");
    let head_before = run_git_capture(&repo, &["rev-parse", "HEAD"])
        .trim()
        .to_string();

    opened
        .commit_amend("amended working change")
        .expect("routed amend");

    assert_eq!(
        run_git_capture(&repo, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "describe only — git HEAD must not move"
    );
    assert_eq!(
        jj_working_copy_description(&repo),
        "amended working change",
        "the message lands on the working-copy commit"
    );
    let status = opened.status().expect("read status");
    assert!(
        status
            .unstaged
            .iter()
            .any(|entry| entry.path == Path::new("file.txt")),
        "the edit stays in the unstaged lane (index keeps @'s parent tree): {:?}",
        status.unstaged
    );
}

#[test]
fn gix_backend_open_maps_not_a_repository_error() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let non_repo = dir.path().join("plain-dir");
    fs::create_dir_all(&non_repo).expect("create plain directory");

    let backend = GixBackend;
    let err = match backend.open(&non_repo) {
        Ok(_) => panic!("opening a non-git directory should fail"),
        Err(err) => err,
    };
    assert!(matches!(err.kind(), ErrorKind::NotARepository));
}

#[test]
fn gix_backend_open_maps_io_error_for_missing_path() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let missing = dir.path().join("does-not-exist");

    let backend = GixBackend;
    let err = match backend.open(&missing) {
        Ok(_) => panic!("opening a missing path should fail"),
        Err(err) => err,
    };
    assert!(matches!(
        err.kind(),
        ErrorKind::Io(std::io::ErrorKind::NotFound)
    ));
}

// gix misreads a worktree whose directory ends in `.git` as a bare git dir, so
// a plain `gix::open` fails on it. These tests pin that `GixBackend::open` (via
// the crate's single `open_worktree_repo` chokepoint) opens such repos.

#[test]
fn gix_backend_open_succeeds_for_dot_git_suffixed_worktree() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("myrepo.git");
    init_repo_with_commit(&repo);

    let backend = GixBackend;
    let opened = backend.open(&repo).expect("open .git-suffixed repository");
    assert_eq!(opened.spec().workdir, canonicalize_or_original(repo));
}

#[test]
fn gix_backend_open_succeeds_for_dot_git_suffixed_linked_worktree() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let main = dir.path().join("main");
    init_repo_with_commit(&main);

    // A linked worktree stores its `.git` as a `gitdir:` file, not a directory.
    let linked = dir.path().join("linked.git");
    run_git(&main, &["worktree", "add", linked.to_str().unwrap()]);
    assert!(
        linked.join(".git").is_file(),
        "linked worktree has a .git file"
    );

    let backend = GixBackend;
    let opened = backend
        .open(&linked)
        .expect("open .git-suffixed linked worktree");
    assert_eq!(opened.spec().workdir, canonicalize_or_original(linked));
}

// Regression guard for the submodule open path: enumerating a submodule whose
// worktree directory ends in `.git` must still resolve its checked-out head,
// which requires the nested repository to open successfully.
#[test]
fn list_submodules_opens_dot_git_suffixed_submodule() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let sub_src = dir.path().join("sub-src");
    init_repo_with_commit(&sub_src);

    let superproject = dir.path().join("super");
    init_repo_with_commit(&superproject);
    run_git(
        &superproject,
        &[
            "-c",
            "protocol.file.allow=always",
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "submodule",
            "add",
            sub_src.to_str().unwrap(),
            "nested.git",
        ],
    );
    run_git(
        &superproject,
        &[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "add submodule",
        ],
    );

    let backend = GixBackend;
    let opened = backend.open(&superproject).expect("open superproject");
    let submodules = opened.list_submodules().expect("list submodules");

    let nested = submodules
        .iter()
        .find(|s| s.path == Path::new("nested.git"))
        .expect("submodule at nested.git");
    assert!(
        nested.checked_out_head.is_some(),
        "nested .git-suffixed submodule must open to report its checked-out head",
    );
}
