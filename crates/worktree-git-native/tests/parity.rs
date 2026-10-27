//! Parity tests: the native object reader must return bytes identical to
//! `git cat-file <type> <oid>` for every object in a repository — commits,
//! trees, blobs, and tags, whether loose, packed, or stored as ref/ofs deltas.
//!
//! Run with: `cargo test -p worktree-git-native --test parity`

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir);
    c.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_CONFIG_SYSTEM");
    c
}

fn run(dir: &Path, args: &[&str]) -> String {
    let out = git(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

fn run_bytes(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = git(dir).args(args).output().expect("spawn git");
    assert!(out.status.success(), "git {:?} failed", args);
    out.stdout
}

/// Build a repo with a mix of loose, packed, and delta objects, then return its
/// `.git` directory and the list of `(oid, type)` for every object in it.
fn build_fixture() -> (tempfile::TempDir, PathBuf, Vec<(String, String)>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();

    run(repo, &["init", "-q"]);
    run(repo, &["config", "user.name", "parity"]);
    run(repo, &["config", "user.email", "parity@example.invalid"]);
    run(repo, &["config", "commit.gpgsign", "false"]);

    // A first commit (this content becomes a loose blob before we repack).
    std::fs::write(repo.join("README.md"), b"hello parity\n").unwrap();
    std::fs::create_dir(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/lib.rs"), b"pub fn x() {}\n").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "-q", "-m", "first"]);

    // A second commit that changes the blob and adds a nested tree, so the
    // repack below produces OFS/REF deltas against the first commit's objects.
    std::fs::write(repo.join("README.md"), b"hello parity v2\n").unwrap();
    std::fs::write(repo.join("src/lib.rs"), b"pub fn x() -> u8 { 1 }\n").unwrap();
    std::fs::write(repo.join("src/util.rs"), b"pub fn y() {}\n").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "-q", "-m", "second"]);

    // A lightweight + annotated tag so both are represented.
    run(repo, &["tag", "v1.0-light"]);
    run(repo, &["tag", "-a", "v1.0-annot", "-m", "annotated"]);

    // Pack everything with deep deltas (exercises OFS_DELTA / REF_DELTA paths).
    run(repo, &["repack", "-a", "-d", "-q", "--depth=50", "--window=20"]);

    // Touch one more commit *after* the repack so a fresh loose object exists
    // alongside the packs (exercises the loose-object fallback path).
    std::fs::write(repo.join("LICENSE"), b"MIT\n").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "-q", "-m", "third loose"]);

    let git_dir = {
        let out = run(repo, &["rev-parse", "--absolute-git-dir"]);
        PathBuf::from(out.trim())
    };

    // Enumerate every object with its type. `--batch-all-objects` covers loose
    // and packed objects (and the deltas' base objects) all at once.
    let listing = run(repo, &["cat-file", "--batch-all-objects", "--batch-check"]);
    let mut objects = Vec::new();
    for line in listing.lines() {
        // "<oid> <type> <size>" (or "<oid> missing" which we skip).
        let mut parts = line.split_whitespace();
        let oid = parts.next().unwrap_or_default().to_string();
        let ty = parts.next().unwrap_or_default().to_string();
        if ty == "missing" || oid.is_empty() {
            continue;
        }
        objects.push((oid, ty));
    }
    assert!(!objects.is_empty(), "fixture produced no objects");

    (tmp, git_dir, objects)
}

#[test]
fn native_read_matches_git_cat_file() {
    let (_tmp, git_dir, objects) = build_fixture();

    for (oid, ty) in &objects {
        // Reference answer from the real git binary.
        let expected = run_bytes(&git_dir, &["cat-file", ty, oid]);

        // Native reader (the thing under test).
        let got = worktree_git_native::read_object(&git_dir, oid, ty)
            .unwrap_or_else(|| panic!("native read failed for {oid} ({ty})"));

        assert_eq!(
            got, expected,
            "byte mismatch for {oid} ({ty}): native != git cat-file",
        );
    }
}

#[test]
fn native_read_is_type_aware() {
    let (_tmp, git_dir, objects) = build_fixture();

    // Asking for the wrong type must NOT return an object (mirrors `git
    // cat-file <wrong> <oid>` failing). This guards against accidentally
    // returning a blob body for a `commit` request, etc.
    let wrong = match objects[0].1.as_str() {
        "commit" => "blob",
        _ => "commit",
    };
    let oid = &objects[0].0;
    assert!(
        worktree_git_native::read_object(&git_dir, oid, wrong).is_none(),
        "native read should reject type mismatch for {oid}",
    );
}

#[test]
fn native_read_missing_object_is_none() {
    let (_tmp, git_dir, _objects) = build_fixture();
    let bogus = "0000000000000000000000000000000000000000";
    assert!(
        worktree_git_native::read_object(&git_dir, bogus, "commit").is_none(),
        "native read should return None for a missing object",
    );
}
