use repositorytree_core::domain::{CommitId, DiffArea, DiffTarget};
use repositorytree_core::services::GitBackend;
use repositorytree_git_gix::GixBackend;
#[path = "support/test_git_env.rs"]
mod test_git_env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn git_command() -> Command {
    let mut cmd = Command::new("git");
    // Keep tests deterministic by isolating from host git config.
    test_git_env::apply(&mut cmd);
    cmd
}

fn run_git(repo: &Path, args: &[&str]) {
    let status = git_command()
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()
        .expect("git command to run");
    assert!(status.success(), "git {:?} failed", args);
}

fn run_git_output(repo: &Path, args: &[&str]) -> String {
    let output = git_command()
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git command to run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn init_repo(repo: &Path) {
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    run_git(repo, &["config", "core.autocrlf", "false"]);
}

fn commit_all(repo: &Path, message: &str) {
    run_git(repo, &["add", "-A"]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

fn head_commit_id(repo: &Path) -> CommitId {
    CommitId(run_git_output(repo, &["rev-parse", "HEAD"]).trim().to_string().into())
}

fn pointer_file(oid: &str, size: u64) -> String {
    format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"
    )
}

#[test]
fn lfs_enabled_follows_pre_push_hook_presence() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);
    fs::write(repo.join("a.txt"), "one\n").unwrap();
    commit_all(repo, "init");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    assert!(!opened.lfs_enabled().unwrap());

    let hooks = repo.join(".git").join("hooks");
    fs::create_dir_all(&hooks).unwrap();
    fs::write(
        hooks.join("pre-push"),
        "#!/bin/sh\ncommand -v git-lfs >/dev/null 2>&1 || exit 0\ngit lfs pre-push \"$@\"\n",
    )
    .unwrap();
    assert!(opened.lfs_enabled().unwrap());
}

#[test]
fn lfs_is_filtered_reads_filter_attribute() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);
    fs::write(repo.join(".gitattributes"), "*.bin filter=lfs diff=lfs merge=lfs -text\n").unwrap();
    fs::write(repo.join("art.bin"), "placeholder\n").unwrap();
    fs::write(repo.join("a.txt"), "text\n").unwrap();
    commit_all(repo, "attributes + files");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    assert!(opened.lfs_is_filtered(Path::new("art.bin")).unwrap());
    assert!(!opened.lfs_is_filtered(Path::new("a.txt")).unwrap());
}

#[test]
fn lfs_pointer_change_reads_commit_range() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);

    let old_oid = "1".repeat(64);
    let new_oid = "2".repeat(64);
    fs::write(repo.join("art.bin"), pointer_file(&old_oid, 4096)).unwrap();
    commit_all(repo, "old pointer");
    let from = head_commit_id(repo);

    fs::write(repo.join("art.bin"), pointer_file(&new_oid, 8192)).unwrap();
    commit_all(repo, "new pointer");
    let to = head_commit_id(repo);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let target = DiffTarget::CommitRange {
        from_commit_id: from,
        to_commit_id: Some(to),
        path: Some("art.bin".into()),
    };
    let change = opened
        .lfs_pointer_change(&target)
        .unwrap()
        .expect("pointer change parsed");

    let old = change.old.expect("old side");
    let new = change.new.expect("new side");
    assert_eq!(old.oid.as_deref(), Some(old_oid.as_str()));
    assert_eq!(old.size, Some(4096));
    assert_eq!(new.oid.as_deref(), Some(new_oid.as_str()));
    assert_eq!(new.size, Some(8192));
}

#[test]
fn lfs_pointer_change_is_none_for_ordinary_text() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);

    fs::write(repo.join("a.txt"), "one\n").unwrap();
    commit_all(repo, "base");
    let from = head_commit_id(repo);

    fs::write(repo.join("a.txt"), "two\n").unwrap();
    commit_all(repo, "edit");
    let to = head_commit_id(repo);

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let target = DiffTarget::CommitRange {
        from_commit_id: from,
        to_commit_id: Some(to),
        path: Some("a.txt".into()),
    };
    assert!(opened.lfs_pointer_change(&target).unwrap().is_none());
}

#[test]
fn cleanup_runs_gc_without_lfs_prune_when_not_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);
    fs::write(repo.join("a.txt"), "one\n").unwrap();
    commit_all(repo, "init");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    let output = opened.cleanup_with_output().unwrap();
    // gc on a fresh tiny repo is quiet; the contract is Ok with no
    // prune-failure note, since the pre-push hook (and thus LFS) is absent.
    assert!(
        !output.stderr.contains("git lfs prune"),
        "prune must not run without LFS wiring: {}",
        output.stderr
    );
}
