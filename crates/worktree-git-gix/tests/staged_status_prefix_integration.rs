//! `tree_index_status()` limits its diff to the repository *prefix* — the
//! process CWD relative to the worktree root, captured when the repository is
//! opened — whenever it builds its own pathspec. The staged reads must instead
//! cover the whole tree no matter where the process was started from: a GUI
//! launched from a build directory would otherwise show a staged panel that
//! only ever lists files under that directory, or nothing at all.
//!
//! The CWD is process-global, so this file keeps exactly one test: moving the
//! CWD must never race another test's `gix::open` in the same binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use worktree_core::services::GitBackend;
use worktree_git_gix::GixBackend;

#[path = "support/test_git_env.rs"]
mod test_git_env;

fn git_command() -> Command {
    let mut cmd = Command::new("git");
    test_git_env::apply(&mut cmd);
    cmd
}

fn run_git(repo: &Path, args: &[&str]) {
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
}

fn fixture_repo() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "worktree_gix_staged_prefix_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init", "--initial-branch=main"]);
    run_git(&dir, &["config", "user.email", "test@example.com"]);
    run_git(&dir, &["config", "user.name", "Test"]);

    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("top.txt"), "base\n").unwrap();
    std::fs::write(dir.join("nested/a.txt"), "base\n").unwrap();
    std::fs::write(dir.join("nested/b.txt"), "base\n").unwrap();
    run_git(&dir, &["add", "--", "."]);
    run_git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    );

    // Staged changes at the root, inside `nested/`, and in a third directory,
    // so no single subdirectory prefix can cover them all.
    std::fs::write(dir.join("top.txt"), "staged edit\n").unwrap();
    std::fs::write(dir.join("nested/a.txt"), "staged edit\n").unwrap();
    std::fs::create_dir_all(dir.join("other")).unwrap();
    std::fs::write(dir.join("other/c.txt"), "new\n").unwrap();
    run_git(
        &dir,
        &["add", "--", "top.txt", "nested/a.txt", "other/c.txt"],
    );
    dir
}

/// Restores the process CWD on drop, so an assertion failure cannot leave the
/// test binary running from inside a deleted temp directory.
struct RestoreCurrentDir(PathBuf);

impl Drop for RestoreCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

#[test]
fn staged_reads_cover_the_whole_tree_when_opened_from_a_subdirectory() {
    let dir = fixture_repo();

    // Open the backend while the process CWD sits inside `nested/`, the way a
    // GUI process started from a worktree subdirectory would.
    let guard = RestoreCurrentDir(std::env::current_dir().expect("read the original CWD"));
    std::env::set_current_dir(dir.join("nested")).expect("enter the nested subdirectory");
    let backend = GixBackend;
    let opened = backend.open(&dir).unwrap();
    std::env::set_current_dir(&guard.0).expect("restore the original CWD");
    drop(guard);

    let staged = opened.staged_status().unwrap();
    let mut paths: Vec<PathBuf> = staged.into_iter().map(|entry| entry.path).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("nested/a.txt"),
            PathBuf::from("other/c.txt"),
            PathBuf::from("top.txt"),
        ],
        "the staged list must not be limited to the subdirectory the process was opened from"
    );

    // Unstaging one file away from the prefix leaves the others staged.
    opened.unstage(&[Path::new("other/c.txt")]).unwrap();
    let staged = opened.staged_status().unwrap();
    let mut paths: Vec<PathBuf> = staged.into_iter().map(|entry| entry.path).collect();
    paths.sort();
    assert_eq!(
        paths,
        vec![PathBuf::from("nested/a.txt"), PathBuf::from("top.txt")]
    );

    // "Unstage everything" enumerates the staged paths from the tree/index
    // diff too, so it must also see past the prefix.
    opened.unstage(&[]).unwrap();
    let staged = opened.staged_status().unwrap();
    assert!(
        staged.is_empty(),
        "unstage-all must reset staged files outside the open-time prefix as well"
    );

    std::fs::remove_dir_all(&dir).ok();
}
