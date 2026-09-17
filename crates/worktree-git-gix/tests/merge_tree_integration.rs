use worktree_core::services::{GitBackend, GitRepository};
use worktree_git_gix::GixBackend;
#[path = "support/test_git_env.rs"]
mod test_git_env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn run_git(repo: &Path, args: &[&str]) {
    worktree_test_support::run_git_with(repo, args, |cmd| {
        test_git_env::apply(cmd);
        cmd.env("GIT_EDITOR", "true").env("EDITOR", "true");
    });
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    test_git_env::apply(&mut cmd);
    let output = cmd
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git command to run");
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo(repo: &Path) {
    fs::create_dir_all(repo).expect("create repo directory");
    run_git(repo, &["init"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
}

fn commit_file(repo: &Path, name: &str, content: &str, message: &str) {
    fs::write(repo.join(name), content).expect("write file");
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

fn rev_parse(repo: &Path, spec: &str) -> String {
    git_stdout(repo, &["rev-parse", spec])
}

fn open_backend(repo: &Path) -> Arc<dyn GitRepository> {
    GixBackend.open(repo).expect("open repository")
}

fn is_hex_oid(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn merge_tree_preview_reports_a_clean_merge() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    init_repo(&repo);
    commit_file(&repo, "file.txt", "base\n", "Base");
    let main_tip = rev_parse(&repo, "HEAD");
    let default_branch = git_stdout(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);

    run_git(&repo, &["checkout", "-b", "feature"]);
    commit_file(&repo, "feature.txt", "feature\n", "Add feature");
    let feature_tip = rev_parse(&repo, "HEAD");

    let backend = open_backend(&repo);
    let preview = backend
        .merge_tree_preview(&main_tip, &feature_tip)
        .expect("preview a clean merge");

    assert!(!preview.has_conflict);
    assert!(preview.conflicts.is_empty());
    assert!(
        is_hex_oid(&preview.result_tree),
        "result tree: {}",
        preview.result_tree
    );
    // The merge brings feature.txt in, so the result tree differs from HEAD's.
    assert_ne!(
        preview.result_tree,
        rev_parse(&repo, &format!("{main_tip}^{{tree}}"))
    );

    // Read-only: neither branch moved.
    assert_eq!(
        rev_parse(&repo, &format!("refs/heads/{default_branch}")),
        main_tip
    );
    assert_eq!(rev_parse(&repo, "refs/heads/feature"), feature_tip);
}

#[test]
fn merge_tree_preview_reports_conflicts() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    init_repo(&repo);
    commit_file(&repo, "file.txt", "base\n", "Base");
    let base = rev_parse(&repo, "HEAD");

    run_git(&repo, &["checkout", "-b", "ours"]);
    commit_file(&repo, "file.txt", "ours\n", "Ours");
    let ours = rev_parse(&repo, "HEAD");

    run_git(&repo, &["checkout", "-b", "theirs", base.as_str()]);
    commit_file(&repo, "file.txt", "theirs\n", "Theirs");
    let theirs = rev_parse(&repo, "HEAD");

    let backend = open_backend(&repo);
    let preview = backend
        .merge_tree_preview(&ours, &theirs)
        .expect("preview a conflicted merge");

    assert!(preview.has_conflict);
    assert!(is_hex_oid(&preview.result_tree));
    assert_eq!(preview.conflicts.len(), 1, "{:?}", preview.conflicts);
    assert_eq!(preview.conflicts[0].path, "file.txt");
    assert!(!preview.conflicts[0].conflict_type.is_empty());
}

#[test]
fn merge_tree_preview_errors_without_common_history() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path().join("repo");
    init_repo(&repo);
    commit_file(&repo, "file.txt", "one\n", "One");
    let one = rev_parse(&repo, "HEAD");

    run_git(&repo, &["checkout", "--orphan", "other"]);
    run_git(&repo, &["rm", "-rf", "."]);
    commit_file(&repo, "other.txt", "two\n", "Two");
    let two = rev_parse(&repo, "HEAD");

    let backend = open_backend(&repo);
    assert!(
        backend.merge_tree_preview(&one, &two).is_err(),
        "unrelated histories must not produce a preview"
    );
}
