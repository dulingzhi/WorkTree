//! The path-targeted status rescan must agree with the full scan on the
//! paths it covers — that equivalence is what lets the incremental lane
//! splice its result into a previous snapshot. Rename records are the one
//! documented exception: they answer `NeedsFullScan`.

use repositorytree_core::domain::{DiffArea, DiffTarget, FileStatusKind};
use repositorytree_core::services::{GitBackend, StatusForPaths};
use repositorytree_git_gix::GixBackend;
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

fn fixture_repo(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "repositorytree_gix_status_paths_{}_{}_{}",
        tag,
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init", "--initial-branch=main"]);
    run_git(&dir, &["config", "user.email", "test@example.com"]);
    run_git(&dir, &["config", "user.name", "Test"]);

    std::fs::write(dir.join("committed.txt"), "base\n").unwrap();
    std::fs::write(dir.join("clean.txt"), "clean\n").unwrap();
    run_git(&dir, &["add", "--", "."]);
    run_git(&dir, &["-c", "commit.gpgsign=false", "commit", "-m", "base"]);

    // Every lane at once: staged modification with a further unstaged edit,
    // a plain untracked file, and a new directory with an untracked file.
    std::fs::write(dir.join("committed.txt"), "staged edit\n").unwrap();
    run_git(&dir, &["add", "--", "committed.txt"]);
    std::fs::write(dir.join("committed.txt"), "staged + unstaged edit\n").unwrap();
    std::fs::write(dir.join("untracked.txt"), "new\n").unwrap();
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("nested/inner.txt"), "inner\n").unwrap();
    dir
}

fn kinds_for(status: &StatusForPaths, path: &str) -> Vec<FileStatusKind> {
    let (unstaged, staged): (&[repositorytree_core::domain::FileStatus], _) = match status {
        StatusForPaths::Lists { unstaged, staged } => (unstaged, staged),
        StatusForPaths::NeedsFullScan => panic!("expected lists"),
    };
    let mut out: Vec<FileStatusKind> = staged
        .iter()
        .filter(|entry| entry.path == Path::new(path))
        .map(|entry| entry.kind)
        .collect();
    out.extend(
        unstaged
            .iter()
            .filter(|entry| entry.path == Path::new(path))
            .map(|entry| entry.kind),
    );
    out
}

#[test]
fn status_for_paths_agrees_with_the_full_scan_on_covered_paths() {
    let dir = fixture_repo("agree");
    let backend = GixBackend;
    let opened = backend.open(&dir).unwrap();

    let paths: Vec<PathBuf> = vec![
        PathBuf::from("committed.txt"),
        PathBuf::from("untracked.txt"),
        // A directory pathspec matches recursively; the full scan may show
        // the dir collapsed, so nested coverage is asserted separately.
        PathBuf::from("nested"),
    ];
    let targeted = opened.status_for_paths(&paths).unwrap();
    let StatusForPaths::Lists { unstaged, staged } = targeted else {
        panic!("expected lists");
    };

    // Full-scan parity per lane for the covered paths.
    let file_paths: Vec<PathBuf> = vec![
        PathBuf::from("committed.txt"),
        PathBuf::from("untracked.txt"),
    ];
    let full = opened.status().unwrap();
    let full_for = |entries: &[repositorytree_core::domain::FileStatus]| {
        let mut covered: Vec<_> = entries
            .iter()
            .filter(|entry| file_paths.contains(&entry.path))
            .cloned()
            .collect();
        covered.sort_by(|a, b| a.path.cmp(&b.path));
        covered
    };
    let mut targeted_unstaged: Vec<_> = unstaged
        .iter()
        .filter(|entry| file_paths.contains(&entry.path))
        .cloned()
        .collect();
    let mut targeted_staged: Vec<_> = staged
        .iter()
        .filter(|entry| file_paths.contains(&entry.path))
        .cloned()
        .collect();
    targeted_unstaged.sort_by(|a, b| a.path.cmp(&b.path));
    targeted_staged.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(targeted_unstaged, full_for(&full.unstaged));
    assert_eq!(targeted_staged, full_for(&full.staged));

    // The directory pathspec covers its contents individually, and the
    // untouched path is absent from both lanes of the targeted scan.
    let combined = StatusForPaths::Lists { unstaged, staged };
    assert_eq!(
        kinds_for(&combined, "nested/inner.txt"),
        vec![FileStatusKind::Untracked]
    );
    assert!(kinds_for(&combined, "clean.txt").is_empty());

    // A path set beyond the fixture still answers cleanly.
    let empty = opened
        .status_for_paths(&[PathBuf::from("does-not-exist.txt")])
        .unwrap();
    assert!(matches!(
        empty,
        StatusForPaths::Lists {
            unstaged,
            staged,
        } if unstaged.is_empty() && staged.is_empty()
    ));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn status_for_paths_answers_needs_full_scan_for_renames() {
    let dir = fixture_repo("rename");
    run_git(&dir, &["add", "--", "untracked.txt"]);
    run_git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add untracked"],
    );
    // A staged rename produces a `2` record in porcelain v2.
    run_git(&dir, &["mv", "untracked.txt", "renamed.txt"]);

    let backend = GixBackend;
    let opened = backend.open(&dir).unwrap();
    let targeted = opened
        .status_for_paths(&[PathBuf::from("untracked.txt"), PathBuf::from("renamed.txt")])
        .unwrap();
    assert_eq!(targeted, StatusForPaths::NeedsFullScan);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn status_for_paths_reports_conflicted_paths_with_their_kind() {
    let dir = fixture_repo("conflict");
    run_git(&dir, &["add", "--", "untracked.txt"]);
    run_git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "add untracked"],
    );
    run_git(&dir, &["checkout", "-b", "side"]);
    run_git(&dir, &["-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "side"]);
    run_git(&dir, &["checkout", "main"]);
    std::fs::write(dir.join("committed.txt"), "main edit\n").unwrap();
    run_git(&dir, &["add", "--", "committed.txt"]);
    run_git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "main edits"],
    );
    run_git(&dir, &["checkout", "side"]);
    std::fs::write(dir.join("committed.txt"), "side edit\n").unwrap();
    run_git(&dir, &["add", "--", "committed.txt"]);
    run_git(
        &dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "side edits"],
    );
    let _ = git_command()
        .arg("-C")
        .arg(&dir)
        .args(["merge", "main"])
        .output();

    let backend = GixBackend;
    let opened = backend.open(&dir).unwrap();
    let targeted = opened
        .status_for_paths(&[PathBuf::from("committed.txt")])
        .unwrap();
    let StatusForPaths::Lists { unstaged, staged } = targeted else {
        panic!("expected lists");
    };
    let conflicted = unstaged
        .iter()
        .find(|entry| entry.path == Path::new("committed.txt"))
        .expect("the conflicted path is in the unstaged lane");
    assert_eq!(conflicted.kind, FileStatusKind::Conflicted);
    assert_eq!(
        conflicted.conflict,
        Some(repositorytree_core::domain::FileConflictKind::BothModified)
    );
    assert!(
        staged.is_empty(),
        "conflicted paths live in the unstaged lane only"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// Keep the DiffTarget import honest for future pathspec-aware callers.
#[allow(dead_code)]
fn _diff_area_target(area: DiffArea) -> DiffTarget {
    DiffTarget::WorkingTree {
        path: PathBuf::new(),
        area,
    }
}
