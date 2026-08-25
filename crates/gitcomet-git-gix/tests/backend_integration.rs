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
    assert_eq!(opened.capabilities(), RepoCapabilities::jj_read_only());
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

/// The current working-copy commit id (`@`), read straight from jj.
fn jj_working_copy_commit_id(repo: &Path) -> String {
    let output = std::process::Command::new("jj")
        .arg("log")
        .arg("--no-graph")
        .arg("-r")
        .arg("@")
        .arg("-T")
        .arg("commit_id")
        .current_dir(repo)
        .output()
        .expect("run jj log");
    assert!(
        output.status.success(),
        "jj log -r @ failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
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
    assert_eq!(opened.capabilities(), RepoCapabilities::jj_read_only());

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
    // capability gate.
    for unsupported in [
        opened.commit("message").err(),
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
    assert_eq!(opened.capabilities(), RepoCapabilities::jj_read_only());

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
