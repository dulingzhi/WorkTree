use repositorytree_core::services::GitBackend;
use repositorytree_git_gix::GixBackend;
#[path = "support/test_git_env.rs"]
mod test_git_env;
use std::fs;
use std::path::{Path, PathBuf};
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

#[test]
fn assume_unchanged_round_trips_through_the_index() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);
    fs::write(repo.join("tracked.txt"), "one\n").unwrap();
    fs::write(repo.join("other.txt"), "two\n").unwrap();
    commit_all(repo, "init");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    assert!(opened.assume_unchanged_list().unwrap().is_empty());

    opened
        .set_assume_unchanged(Path::new("tracked.txt"), true)
        .unwrap();
    assert_eq!(
        opened.assume_unchanged_list().unwrap(),
        vec![PathBuf::from("tracked.txt")]
    );
    // The plumbing view of the same fact: git lowercases the tag exactly when
    // the assume-unchanged bit is set.
    assert!(
        run_git_output(repo, &["ls-files", "-v", "tracked.txt"])
            .starts_with('h'),
        "ls-files -v must report the lowercase tag"
    );

    opened
        .set_assume_unchanged(Path::new("tracked.txt"), false)
        .unwrap();
    assert!(opened.assume_unchanged_list().unwrap().is_empty());
    assert!(
        run_git_output(repo, &["ls-files", "-v", "tracked.txt"])
            .starts_with('H'),
        "ls-files -v must report the uppercase tag after clearing"
    );
}

#[test]
fn assume_unchanged_list_survives_modified_files() {
    // The flag is about freshness checking, not content: a modified file keeps
    // its mark until explicitly cleared.
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    init_repo(repo);
    fs::write(repo.join("a.txt"), "one\n").unwrap();
    commit_all(repo, "init");

    let backend = GixBackend;
    let opened = backend.open(repo).unwrap();
    opened.set_assume_unchanged(Path::new("a.txt"), true).unwrap();

    fs::write(repo.join("a.txt"), "changed\n").unwrap();
    assert_eq!(
        opened.assume_unchanged_list().unwrap(),
        vec![PathBuf::from("a.txt")]
    );
}
