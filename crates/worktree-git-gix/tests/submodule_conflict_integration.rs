//! Submodule (gitlink) merge conflicts resolve as a side pick between two
//! commit pointers — the C# client's USE THEIRS / USE MINE for submodules.
//! The stage payloads are the submodule commit ids (the objects live in the
//! submodule's ODB), the session routes to the binary side-pick strategy,
//! and `checkout_conflict_side` performs the `checkout --ours/--theirs` +
//! `add` pair that resolves the pointer.

use worktree_core::conflict_session::ConflictResolverStrategy;
use worktree_core::services::{ConflictSide, GitBackend};
use worktree_git_gix::GixBackend;
#[path = "support/test_git_env.rs"]
mod test_git_env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git_command() -> Command {
    let mut cmd = Command::new("git");
    test_git_env::apply(&mut cmd);
    cmd
}

fn run_git(repo: &Path, args: &[&str]) {
    worktree_test_support::run_git_with(repo, args, test_git_env::apply);
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let output = git_command()
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git command to run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout is utf-8")
        .trim()
        .to_string()
}

/// A superproject whose `sub` pointer conflicts on merge: main moved it to
/// `ours_tip`, branch `side` moved it to `theirs_tip`, both from `base_tip`.
fn conflicted_submodule_superproject() -> (PathBuf, String, String, String) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "worktree_gix_submodule_conflict_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&root).ok();
    let sub = root.join("sub-repo");
    let super_repo = root.join("super");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&super_repo).unwrap();

    // The submodule's default branch stays at `base` for the clone — the
    // superproject records that pointer — while `side` and `wip` carry the
    // two divergent tips the branches will move it to.
    run_git(&sub, &["init", "--initial-branch=main"]);
    run_git(
        &sub,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "base",
        ],
    );
    let base_tip = git_stdout(&sub, &["rev-parse", "HEAD"]);

    run_git(&sub, &["checkout", "-b", "side"]);
    run_git(
        &sub,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "theirs",
        ],
    );
    let theirs_tip = git_stdout(&sub, &["rev-parse", "HEAD"]);

    run_git(&sub, &["checkout", "-b", "wip", "main"]);
    run_git(
        &sub,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "ours",
        ],
    );
    let ours_tip = git_stdout(&sub, &["rev-parse", "HEAD"]);

    run_git(&sub, &["checkout", "main"]);
    run_git(&super_repo, &["init", "--initial-branch=main"]);
    run_git(&super_repo, &["config", "user.email", "test@example.com"]);
    run_git(&super_repo, &["config", "user.name", "Test"]);
    let mut add = git_command();
    let add_output = add
        .arg("-C")
        .arg(&super_repo)
        .arg("-c")
        .arg("protocol.file.allow=always")
        .arg("submodule")
        .arg("add")
        .arg(&sub)
        .arg("sub")
        .output()
        .expect("git submodule add to run");
    assert!(
        add_output.status.success(),
        "git submodule add failed\nstderr: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );
    run_git(
        &super_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "record submodule",
        ],
    );

    // side: move the pointer to theirs_tip.
    run_git(&super_repo, &["checkout", "-b", "side"]);
    run_git(super_repo.join("sub").as_path(), &["checkout", &theirs_tip]);
    run_git(&super_repo, &["add", "--", "sub"]);
    run_git(
        &super_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "side moves sub",
        ],
    );

    // main: move the pointer to ours_tip, then merge side → conflict.
    run_git(&super_repo, &["checkout", "main"]);
    run_git(super_repo.join("sub").as_path(), &["checkout", &ours_tip]);
    run_git(&super_repo, &["add", "--", "sub"]);
    run_git(
        &super_repo,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "main moves sub",
        ],
    );
    let _ = git_command()
        .arg("-C")
        .arg(&super_repo)
        .args(["merge", "side"])
        .output();

    (super_repo, base_tip, ours_tip, theirs_tip)
}

#[test]
fn submodule_conflict_stages_carry_the_pointer_commits_as_text() {
    let (super_repo, _base_tip, ours_tip, theirs_tip) = conflicted_submodule_superproject();
    let backend = GixBackend;
    let opened = backend.open(&super_repo).unwrap();

    // Pre-fix this returned None: the submodule path is a directory, and the
    // loader bailed on directories before ever consulting the index.
    let stages = opened
        .conflict_file_stages(Path::new("sub"))
        .expect("stages call to succeed")
        .expect("a submodule conflict has index stages");
    assert_eq!(stages.ours.as_deref(), Some(ours_tip.as_str()));
    assert_eq!(stages.theirs.as_deref(), Some(theirs_tip.as_str()));
    assert_eq!(
        stages.base.as_deref(),
        Some(_base_tip.as_str()),
        "the base stage is the shared pre-divergence pointer"
    );

    std::fs::remove_dir_all(super_repo.parent().expect("temp root").to_path_buf()).ok();
}

#[test]
fn submodule_conflict_routes_to_the_side_pick_strategy() {
    let (super_repo, _base, _ours, _theirs) = conflicted_submodule_superproject();
    let backend = GixBackend;
    let opened = backend.open(&super_repo).unwrap();

    let session = opened
        .conflict_session(Path::new("sub"))
        .expect("session call to succeed")
        .expect("a session for the conflicted submodule");
    assert_eq!(session.strategy, ConflictResolverStrategy::BinarySidePick);

    std::fs::remove_dir_all(super_repo.parent().expect("temp root").to_path_buf()).ok();
}

#[test]
fn submodule_conflict_side_checkout_resolves_the_pointer() {
    let (super_repo, _base, ours_tip, _theirs) = conflicted_submodule_superproject();
    let backend = GixBackend;
    let opened = backend.open(&super_repo).unwrap();

    let resolution = opened
        .checkout_conflict_side(Path::new("sub"), ConflictSide::Ours)
        .expect("side checkout to run");
    assert!(
        resolution.exit_code.is_none() || resolution.exit_code.unwrap() == 0,
        "checkout --ours + add must succeed for a gitlink"
    );

    assert_eq!(
        git_stdout(&super_repo, &["ls-files", "-u", "--", "sub"]),
        "",
        "no unmerged stages remain for the submodule"
    );
    let recorded = git_stdout(&super_repo, &["ls-files", "-s", "--", "sub"]);
    assert!(
        recorded.starts_with("160000") && recorded.contains(&ours_tip),
        "the resolved stage-0 gitlink must point at ours ({ours_tip}); got: {recorded}"
    );

    std::fs::remove_dir_all(super_repo.parent().expect("temp root").to_path_buf()).ok();
}
