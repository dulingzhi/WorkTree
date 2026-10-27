//! Micro-benchmark (Stage-0): native object read vs. `git cat-file`.
//!
//! Purpose: prove that a cache-free, single-hash-lookup native read is at least
//! as fast as (and typically faster than) forking `git cat-file` for the same
//! object — the premise the refactor rests on. Run:
//!
//!   cargo bench -p worktree-git-native --bench object_read

use std::path::PathBuf;
use std::process::Command;

use criterion::{Criterion, criterion_group, criterion_main};

fn git(dir: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_CONFIG_SYSTEM")
        .args(args)
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {:?} failed", args);
    out.stdout
}

fn make_repo() -> (tempfile::TempDir, PathBuf, Vec<(String, String)>) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let run = |args: &[&str]| {
        let out = git(repo, args);
        let _ = out;
    };
    run(&["init", "-q"]);
    run(&["config", "user.name", "bench"]);
    run(&["config", "user.email", "bench@example.invalid"]);
    run(&["config", "commit.gpgsign", "false"]);

    // Build a few commits with enough history to populate packs with deltas.
    for i in 0..25 {
        std::fs::write(repo.join("f.txt"), format!("line {i}\n").as_bytes()).unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", &format!("commit {i}")]);
    }
    run(&["repack", "-a", "-d", "-q", "--depth=50", "--window=20"]);
    // one loose object after the repack
    std::fs::write(repo.join("loose.txt"), b"loose\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "loose commit"]);

    let git_dir = PathBuf::from(String::from_utf8(git(repo, &["rev-parse", "--absolute-git-dir"])).unwrap().trim());
    let listing = String::from_utf8(git(repo, &["cat-file", "--batch-all-objects", "--batch-check"])).unwrap();
    let objects: Vec<(String, String)> = listing
        .lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            let o = p.next()?;
            let t = p.next()?;
            if t == "missing" {
                None
            } else {
                Some((o.to_string(), t.to_string()))
            }
        })
        .collect();

    (tmp, git_dir, objects)
}

fn bench_object_read(c: &mut Criterion) {
    let (_tmp, git_dir, objects) = make_repo();
    assert!(!objects.is_empty());

    // Exercise the kinds of objects a history walk touches most: commits and
    // trees (and ensure at least one loose object is in the mix).
    let sample: Vec<(String, String)> = objects
        .iter()
        .filter(|(_, t)| *t == "commit" || *t == "tree")
        .cloned()
        .chain(objects.last().cloned())
        .collect();

    let mut group = c.benchmark_group("object_read");
    group.sample_size(50);

    // (1) The cache-free native reader under test — process-internal, single
    //     hash lookup per object.
    group.bench_function("native_read", |b| {
        b.iter(|| {
            for (oid, ty) in &sample {
                let _ = worktree_git_native::read_object(&git_dir, oid, ty);
            }
        });
    });

    // (2) The current GitComet hot path: the gix high-level object API, opened
    //     once and reused (best case for gix — no CLI forks, no per-call repo
    //     open). This is the implementation Stage-1 intends to replace.
    let repo = gix::open(&git_dir).expect("gix open");
    group.bench_function("gix_read", |b| {
        b.iter(|| {
            for (oid, ty) in &sample {
                let id = gix::ObjectId::from_hex(oid.as_bytes()).expect("hex");
                if let Ok(obj) = repo.find_object(id) {
                    let _ = obj.data;
                }
                let _ = ty;
            }
        });
    });

    // (3) Reference / worst case: fork `git cat-file` per object. Kept to show
    //     the process-spawn cost the old design sometimes paid.
    group.bench_function("git_cat_file", |b| {
        b.iter(|| {
            for (oid, ty) in &sample {
                let _ = git(&git_dir, &["cat-file", ty, oid]);
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_object_read);
criterion_main!(benches);
