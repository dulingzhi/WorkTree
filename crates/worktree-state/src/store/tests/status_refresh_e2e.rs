//! End-to-end status-refresh coverage against the real gix backend.
//!
//! The reducer/effects units are covered elsewhere; these tests drive a real
//! `AppStore` over a real temporary repository so the full production
//! interplay — action effect, `RepoActionFinished` targeted refresh, the
//! file-system watcher's own refresh, and the coarse replay scans — runs the
//! way it does in the app. The staged list is sampled continuously and its
//! history asserted, so a wrong end state, a transient clear, and a late
//! clear (a stale coarse scan landing after convergence) all fail.

use super::*;
use worktree_git_gix::GixBackend;

/// One sampled observation of the two status lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LanesSample {
    staged: Option<Vec<PathBuf>>,
    unstaged: Option<Vec<PathBuf>>,
}

struct LaneHistory {
    samples: Vec<(Duration, LanesSample)>,
    last: Option<LanesSample>,
    start: Instant,
}

impl LaneHistory {
    fn new() -> Self {
        Self {
            samples: Vec::new(),
            last: None,
            start: Instant::now(),
        }
    }

    fn record(&mut self, sample: LanesSample) {
        if self.last.as_ref() != Some(&sample) {
            self.samples.push((self.start.elapsed(), sample.clone()));
            self.last = Some(sample);
        }
    }
}

fn sample_lanes(store: &AppStore) -> LanesSample {
    let snapshot = store.snapshot();
    let Some(repo) = snapshot.repos.first() else {
        return LanesSample {
            staged: None,
            unstaged: None,
        };
    };
    LanesSample {
        staged: repo
            .staged_status_entries()
            .map(|entries| entries.iter().map(|entry| entry.path.clone()).collect()),
        unstaged: repo
            .worktree_status_entries()
            .map(|entries| entries.iter().map(|entry| entry.path.clone()).collect()),
    }
}

/// Poll until `ready` holds, recording every distinct lane transition into
/// `history` along the way so failures print the observed sequence.
fn poll_lanes(
    store: &AppStore,
    history: &mut LaneHistory,
    description: &str,
    ready: impl Fn(&LanesSample) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let sample = sample_lanes(store);
        history.record(sample.clone());
        if ready(&sample) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}; lane history so far:\n{:?}",
            history.samples
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Keep sampling for a grace window after the expected state was reached, so
/// a stale coarse scan that lands late and clears the panel still fails.
fn sample_grace_window(store: &AppStore, history: &mut LaneHistory) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        history.record(sample_lanes(store));
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_staged_never_cleared(history: &LaneHistory, phase: &str) {
    for (elapsed, sample) in &history.samples {
        assert!(
            sample
                .staged
                .as_ref()
                .is_some_and(|staged| !staged.is_empty()),
            "{phase}: staged lane wrongly cleared at {elapsed:?} (history:\n{:?})",
            history.samples
        );
    }
}

/// Like [`assert_staged_never_cleared`], but for phases whose starting state
/// is legitimately empty (right after a commit): only transitions away from
/// a non-empty list are failures.
fn assert_staged_never_re_cleared(history: &LaneHistory, phase: &str) {
    let Some(first_non_empty) = history
        .samples
        .iter()
        .position(|(_, sample)| sample.staged.as_ref().is_some_and(|s| !s.is_empty()))
    else {
        return;
    };
    for (elapsed, sample) in &history.samples[first_non_empty..] {
        assert!(
            sample
                .staged
                .as_ref()
                .is_some_and(|staged| !staged.is_empty()),
            "{phase}: staged lane wrongly cleared at {elapsed:?} (history:\n{:?})",
            history.samples
        );
    }
}

fn write_worktree_file(workdir: &Path, name: &str, contents: &str) {
    fs::write(workdir.join(name), contents).expect("worktree file write");
}

#[test]
fn staging_and_unstaging_one_file_keep_the_other_staged_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workdir = dir.path().to_path_buf();
    run_git(&workdir, &["init"]);
    run_git(&workdir, &["config", "user.name", "Store Test"]);
    run_git(&workdir, &["config", "user.email", "store@test.local"]);
    write_worktree_file(&workdir, "a.txt", "base\n");
    write_worktree_file(&workdir, "b.txt", "base\n");
    run_git(&workdir, &["add", "."]);
    run_git(&workdir, &["commit", "-m", "base"]);

    // The reported scenario: one file already staged, a second one modified
    // but unstaged, before the user acts on a single file.
    write_worktree_file(&workdir, "a.txt", "staged change\n");
    write_worktree_file(&workdir, "b.txt", "worktree change\n");
    run_git(&workdir, &["add", "a.txt"]);

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the initial staged list to land with a.txt",
            |sample| sample.staged.as_deref() == Some(&[PathBuf::from("a.txt")]),
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // Stage the one unstaged file; the staged list must grow to both files.
    let mut history = LaneHistory::new();
    store.dispatch(Msg::StagePath {
        repo_id,
        path: PathBuf::from("b.txt"),
    });
    poll_lanes(
        &store,
        &mut history,
        "staging b.txt to land both staged entries",
        |sample| {
            sample.staged.as_deref() == Some(&[PathBuf::from("a.txt"), PathBuf::from("b.txt")])
        },
    );
    sample_grace_window(&store, &mut history);
    assert_staged_never_cleared(&history, "after staging b.txt");

    // Unstage the other file; the staged list must keep b.txt.
    let mut history = LaneHistory::new();
    store.dispatch(Msg::UnstagePath {
        repo_id,
        path: PathBuf::from("a.txt"),
    });
    poll_lanes(
        &store,
        &mut history,
        "unstaging a.txt to leave b.txt staged",
        |sample| sample.staged.as_deref() == Some(&[PathBuf::from("b.txt")]),
    );
    sample_grace_window(&store, &mut history);
    assert_staged_never_cleared(&history, "after unstaging a.txt");
}

/// The reported regression shape, repeated: a repo with many files and
/// non-ASCII names, several entries already staged, and the user rapidly
/// staging and unstaging single files. Coarse scans take longer here than in
/// the two-file tests, so watcher/action/coarse interleavings that a small
/// repo cannot express get exercised. The staged lane must keep the
/// untouched entries through every interleave.
#[test]
fn rapid_single_file_stage_unstage_never_clears_the_staged_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workdir = dir.path().to_path_buf();
    run_git(&workdir, &["init"]);
    run_git(&workdir, &["config", "user.name", "Store Test"]);
    run_git(&workdir, &["config", "user.email", "store@test.local"]);
    run_git(&workdir, &["config", "core.quotepath", "false"]);

    let mut names: Vec<String> = Vec::new();
    for ix in 0..40 {
        names.push(format!("src/file_{ix:03}.txt"));
    }
    // Non-ASCII names, as a user's real repo commonly has: the targeted
    // scan's pathspec round-trip must keep them matching the coarse lanes'
    // entries.
    names.push("src/文件甲.txt".to_string());
    names.push("src/文件乙.txt".to_string());
    for name in &names {
        let path = workdir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("source dir");
        }
        fs::write(&path, format!("base {name}\n")).expect("base write");
    }
    run_git(&workdir, &["add", "."]);
    run_git(&workdir, &["commit", "-m", "base"]);

    // Start with three files staged, everything else clean, then modify a
    // rotating single file before each action so both lanes always have
    // content.
    let staged_names: Vec<String> = names[0..3].to_vec();
    for name in &staged_names {
        fs::write(workdir.join(name), "staged change\n").expect("staged write");
    }
    let mut add_args: Vec<&str> = vec!["add", "--"];
    add_args.extend(staged_names.iter().map(String::as_str));
    run_git(&workdir, &add_args);

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let expected_initial: Vec<PathBuf> = staged_names.iter().map(PathBuf::from).collect();
    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the initial staged list to land with the three staged files",
            |sample| sample.staged.as_deref() == Some(expected_initial.as_slice()),
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // Alternate: stage one fresh file, unstage it again, next file. The
    // staged list must always keep the untouched pre-staged entries.
    for round in 0..12u32 {
        let target = &names[3 + (round as usize) % 8];
        fs::write(workdir.join(target), "worktree change\n").expect("worktree write");

        let mut history = LaneHistory::new();
        store.dispatch(Msg::StagePaths {
            repo_id,
            paths: vec![PathBuf::from(target)].into(),
        });
        let mut expected: Vec<PathBuf> = staged_names.iter().map(PathBuf::from).collect();
        expected.push(PathBuf::from(target));
        expected.sort();
        poll_lanes(
            &store,
            &mut history,
            &format!("round {round}: staging {target} to land four staged entries"),
            |sample| sample.staged.as_deref() == Some(expected.as_slice()),
        );
        sample_grace_window(&store, &mut history);
        assert_staged_never_cleared(&history, &format!("round {round} after staging {target}"));

        let mut history = LaneHistory::new();
        store.dispatch(Msg::UnstagePaths {
            repo_id,
            paths: vec![PathBuf::from(target)].into(),
        });
        poll_lanes(
            &store,
            &mut history,
            &format!("round {round}: unstaging {target} to restore the staged list"),
            |sample| sample.staged.as_deref() == Some(expected_initial.as_slice()),
        );
        sample_grace_window(&store, &mut history);
        assert_staged_never_cleared(&history, &format!("round {round} after unstaging {target}"));
    }
}

#[test]
fn committing_keeps_the_staged_list_refreshing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let workdir = dir.path().to_path_buf();
    run_git(&workdir, &["init"]);
    run_git(&workdir, &["config", "user.name", "Store Test"]);
    run_git(&workdir, &["config", "user.email", "store@test.local"]);
    write_worktree_file(&workdir, "a.txt", "base\n");
    write_worktree_file(&workdir, "b.txt", "base\n");
    run_git(&workdir, &["add", "."]);
    run_git(&workdir, &["commit", "-m", "base"]);

    write_worktree_file(&workdir, "a.txt", "first\n");
    write_worktree_file(&workdir, "b.txt", "second\n");
    run_git(&workdir, &["add", "a.txt", "b.txt"]);

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the initial staged list to land with both files",
            |sample| {
                sample.staged.as_deref() == Some(&[PathBuf::from("a.txt"), PathBuf::from("b.txt")])
            },
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // After the commit the staged list must settle empty because everything
    // was committed — and it must get there through the refresh, not by
    // lingering stale. A regression here shows up as the list still holding
    // the committed files long after, or the pre-commit state disappearing
    // while the entries were still staged.
    let mut history = LaneHistory::new();
    store.dispatch(Msg::Commit {
        repo_id,
        message: "commit both".to_string(),
        push_after_commit: false,
    });
    poll_lanes(
        &store,
        &mut history,
        "the commit to clear the staged list",
        |sample| sample.staged.as_deref() == Some(&[]),
    );
    sample_grace_window(&store, &mut history);
    // The lane must stay Ready throughout: the commit legitimately empties
    // it, but a `None` here means the panel data was wiped to NotLoaded on
    // the way.
    for (elapsed, sample) in &history.samples {
        assert!(
            sample.staged.is_some(),
            "staged lane wiped to NotLoaded at {elapsed:?} after the commit (history:\n{:?})",
            history.samples
        );
    }
}

/// The reported regression: after committing, staging a file again never
/// shows up — the staged list appears permanently cleared until the repo is
/// reopened. Runs against a local clone of this repository itself (the
/// reporter's reproducing content: 2700+ files, renames, real attribute
/// mix), because the bug did not reproduce on a small synthetic repo.
#[test]
fn staging_after_a_commit_still_lands_in_the_staged_list() {
    // The test binary's cwd is the crate dir; the repository root is above it.
    let mut source = std::env::current_dir().expect("cwd");
    while !source.join(".git").exists() && source.pop() {}
    assert!(
        source.join(".git").exists(),
        "test must run from inside a git worktree"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let workdir = dir.path().join("clone");
    // No `--local`: its hardlinks cannot cross drives (repo on D:, temp on C:).
    let clone_status = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg(&source)
        .arg(&workdir)
        .status()
        .expect("git clone to run");
    assert!(clone_status.success(), "git clone --local failed");
    run_git(&workdir, &["config", "user.name", "Store Test"]);
    run_git(&workdir, &["config", "user.email", "store@test.local"]);

    // Two tracked files with real content: Cargo.toml pre-staged, Cargo.lock
    // modified but unstaged — the multi-file scenario the report describes.
    let file_a = workdir.join("Cargo.toml");
    let file_b = workdir.join("Cargo.lock");
    let a_base = fs::read_to_string(&file_a).expect("read Cargo.toml");
    let b_base = fs::read_to_string(&file_b).expect("read Cargo.lock");
    fs::write(&file_a, format!("{a_base}\n# staged marker\n")).expect("stage edit");
    fs::write(&file_b, format!("{b_base}\n# unstaged marker\n")).expect("unstaged edit");
    run_git(&workdir, &["add", "--", "Cargo.toml"]);

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let a = PathBuf::from("Cargo.toml");
    let b = PathBuf::from("Cargo.lock");
    let both_sorted = [b.clone(), a.clone()];
    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the initial staged list to land with Cargo.toml",
            |sample| sample.staged.as_deref() == Some(&[a.clone()]),
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // Stage the one unstaged file; both must be staged.
    let mut history = LaneHistory::new();
    store.dispatch(Msg::StagePaths {
        repo_id,
        paths: vec![b.clone()].into(),
    });
    poll_lanes(
        &store,
        &mut history,
        "staging Cargo.lock to land both staged entries",
        |sample| sample.staged.as_deref() == Some(both_sorted.as_slice()),
    );
    sample_grace_window(&store, &mut history);
    assert_staged_never_cleared(&history, "after staging Cargo.lock");

    // Commit both, then stage a fresh change: the staged list must come back
    // with the new entry. The frozen-empty failure mode times out here with
    // the lane history showing the stuck state.
    store.dispatch(Msg::Commit {
        repo_id,
        message: "commit both markers".to_string(),
        push_after_commit: false,
    });
    {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the commit to clear the staged list",
            |sample| sample.staged.as_deref() == Some(&[]),
        );
    }

    fs::write(&file_a, format!("{a_base}\n# second marker\n")).expect("second edit");
    let mut history = LaneHistory::new();
    store.dispatch(Msg::StagePaths {
        repo_id,
        paths: vec![a.clone()].into(),
    });
    poll_lanes(
        &store,
        &mut history,
        "staging Cargo.toml after the commit to land in the staged list",
        |sample| sample.staged.as_deref() == Some(&[a.clone()]),
    );
    sample_grace_window(&store, &mut history);
    assert_staged_never_re_cleared(&history, "after staging Cargo.toml post-commit");
}

/// The other environmental factor of the reproducing repo: it is under
/// active development, so the file watcher fires continuously while the
/// user stages and unstages. A background writer churns other tracked files
/// throughout the stage/unstage cycle to exercise the storm path.
#[test]
fn stage_unstage_under_watch_storm_keeps_the_staged_list() {
    let mut source = std::env::current_dir().expect("cwd");
    while !source.join(".git").exists() && source.pop() {}
    assert!(
        source.join(".git").exists(),
        "test must run from inside a git worktree"
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let workdir = dir.path().join("clone");
    let clone_status = Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg(&source)
        .arg(&workdir)
        .status()
        .expect("git clone to run");
    assert!(clone_status.success(), "git clone failed");
    run_git(&workdir, &["config", "user.name", "Store Test"]);
    run_git(&workdir, &["config", "user.email", "store@test.local"]);

    let file_a = workdir.join("Cargo.toml");
    let file_b = workdir.join("Cargo.lock");
    let a_base = fs::read_to_string(&file_a).expect("read Cargo.toml");
    let b_base = fs::read_to_string(&file_b).expect("read Cargo.lock");
    fs::write(&file_a, format!("{a_base}\n# staged marker\n")).expect("stage edit");
    fs::write(&file_b, format!("{b_base}\n# unstaged marker\n")).expect("unstaged edit");
    run_git(&workdir, &["add", "--", "Cargo.toml"]);

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let a = PathBuf::from("Cargo.toml");
    let b = PathBuf::from("Cargo.lock");
    let both_sorted = [b.clone(), a.clone()];
    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the initial staged list to land with Cargo.toml",
            |sample| sample.staged.as_deref() == Some(&[a.clone()]),
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // Churn another tracked file the way an IDE does while the user works.
    let churn_path = workdir.join("README.md");
    let churn_base = fs::read_to_string(&churn_path).unwrap_or_default();
    let churning = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let churn_handle = {
        let churning = std::sync::Arc::clone(&churning);
        let churn_path = churn_path.clone();
        std::thread::spawn(move || {
            let mut tick: u32 = 0;
            while churning.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = fs::write(&churn_path, format!("{churn_base}\nchurn {tick}\n"));
                tick += 1;
                std::thread::sleep(Duration::from_millis(15));
            }
        })
    };

    // Every round stages then unstages the SAME file while the other stays
    // staged throughout — the reported scenario: touching one file's staging
    // must never disturb the other file's staged entry.
    for round in 0..6u32 {
        let mut history = LaneHistory::new();
        store.dispatch(Msg::StagePaths {
            repo_id,
            paths: vec![b.clone()].into(),
        });
        poll_lanes(
            &store,
            &mut history,
            &format!("storm round {round}: staging {b:?} to land the expected list"),
            |sample| sample.staged.as_deref() == Some(both_sorted.as_slice()),
        );
        sample_grace_window(&store, &mut history);
        assert_staged_never_cleared(&history, &format!("storm round {round} staged"));

        let mut history = LaneHistory::new();
        store.dispatch(Msg::UnstagePaths {
            repo_id,
            paths: vec![b.clone()].into(),
        });
        poll_lanes(
            &store,
            &mut history,
            &format!("storm round {round}: unstaging {b:?} to restore the list"),
            |sample| sample.staged.as_deref() == Some(&[a.clone()]),
        );
        sample_grace_window(&store, &mut history);
        assert_staged_never_cleared(&history, &format!("storm round {round} unstaged"));
    }

    churning.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = churn_handle.join();
}

/// Restores a real repository's index and worktree after the diagnostic storm
/// below, even when the test panics mid-run.
struct RealRepoRestore {
    workdir: PathBuf,
    scratch: Vec<PathBuf>,
}

impl Drop for RealRepoRestore {
    fn drop(&mut self) {
        let mut reset = Command::new("git");
        reset.arg("-C").arg(&self.workdir).arg("reset").arg("--");
        for path in &self.scratch {
            reset.arg(path);
        }
        if !reset
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            eprintln!(
                "real-repo storm cleanup: git reset failed for {:?}",
                self.scratch
            );
        }
        for path in &self.scratch {
            let _ = fs::remove_file(self.workdir.join(path));
        }
    }
}

/// Isolation probe for the real-repo storm: calls the dedicated staged read
/// and the combined status read directly, with no store, no watcher, no
/// concurrency, printing every payload. If the dedicated read alone returns
/// the truncated set, the defect is inside the gix read path; if both are
/// full, the storm's interleaving is required.
#[test]
fn real_repo_staged_read_probe() {
    let Some(workdir) = std::env::var_os("WORKTREE_STORM_REAL_REPO").map(PathBuf::from) else {
        eprintln!("skipping: WORKTREE_STORM_REAL_REPO not set");
        return;
    };
    let repo = GixBackend.open(&workdir).expect("open real repo for probe");
    for round in 0..5u32 {
        let staged = repo.staged_status().expect("probe staged_status");
        eprintln!(
            "probe round {round}: staged_status count={} paths={:?}",
            staged.len(),
            staged
                .iter()
                .map(|entry| entry.path.display().to_string())
                .collect::<Vec<_>>()
        );
        let combined = repo.status().expect("probe status");
        eprintln!(
            "probe round {round}: status staged count={} unstaged count={}",
            combined.staged.len(),
            combined.unstaged.len(),
        );
    }
}

/// Diagnostic harness against a REAL repository, opted into with
/// `WORKTREE_STORM_REAL_REPO=<workdir>`: it stages and unstages scratch files
/// in that repository's actual index, which the clone-based storm test cannot
/// fully stand in for (real `.git` scale, real watcher scope). Dispatches do
/// NOT wait for convergence between rounds — the way a user clicks through the
/// UI — and one scratch file stays staged throughout as the sentinel whose
/// disappearance is the reported bug. Pre-existing staged entries are treated
/// as part of the invariant (they are exactly the entries the report says get
/// cleared); the scratch files are removed and the index restored on exit.
#[test]
fn real_repo_stage_unstage_storm_keeps_the_staged_list() {
    let Some(workdir) = std::env::var_os("WORKTREE_STORM_REAL_REPO").map(PathBuf::from) else {
        eprintln!("skipping: WORKTREE_STORM_REAL_REPO not set");
        return;
    };
    assert!(
        workdir.join(".git").exists(),
        "WORKTREE_STORM_REAL_REPO must be a git worktree: {}",
        workdir.display()
    );
    // Baseline: whatever is already staged must survive the storm untouched.
    let baseline_status = Command::new("git")
        .arg("-C")
        .arg(&workdir)
        .arg("-c")
        .arg("core.quotepath=false")
        .args(["status", "--porcelain"])
        .output()
        .expect("baseline git status");
    assert!(
        baseline_status.status.success(),
        "baseline git status failed"
    );
    let baseline_staged: Vec<PathBuf> = String::from_utf8_lossy(&baseline_status.stdout)
        .lines()
        .filter(|line| {
            let bytes = line.as_bytes();
            bytes.len() > 3 && bytes[0] != b' ' && bytes[0] != b'?' && bytes[0] != b'!'
        })
        .map(|line| {
            let path = &line[3..];
            PathBuf::from(path.rsplit(" -> ").next().unwrap_or(path))
        })
        .collect();

    let sentinel = PathBuf::from("gitcomet-storm-sentinel.txt");
    let target = PathBuf::from("gitcomet-storm-target.txt");
    let churn_names: Vec<String> = (0..3)
        .map(|i| format!("gitcomet-storm-churn-{i}.txt"))
        .collect();
    let mut scratch: Vec<PathBuf> = vec![sentinel.clone(), target.clone()];
    scratch.extend(churn_names.iter().map(PathBuf::from));
    for (index, name) in churn_names.iter().enumerate() {
        fs::write(workdir.join(name), format!("churn {index} seed\n")).expect("churn seed write");
    }
    fs::write(workdir.join(&sentinel), "sentinel\n").expect("sentinel write");
    fs::write(workdir.join(&target), "target 0\n").expect("target write");
    run_git(
        &workdir,
        &["add", "--", sentinel.to_string_lossy().as_ref()],
    );
    let _restore = RealRepoRestore {
        workdir: workdir.clone(),
        scratch,
    };

    let expected_baseline: Vec<PathBuf> = {
        let mut expected = baseline_staged.clone();
        expected.push(sentinel.clone());
        expected.sort();
        expected
    };
    let staged_superset_of_baseline = |sample: &LanesSample| {
        sample
            .staged
            .as_ref()
            .is_some_and(|staged| expected_baseline.iter().all(|path| staged.contains(path)))
    };

    let (store, _event_rx) = AppStore::new(Arc::new(GixBackend));
    store.dispatch(Msg::OpenRepo(workdir.clone()));

    let repo_id = {
        let mut history = LaneHistory::new();
        poll_lanes(
            &store,
            &mut history,
            "the real repo's staged list to land with the sentinel and the pre-existing entries",
            staged_superset_of_baseline,
        );
        store.snapshot().repos.first().expect("opened repo").id
    };

    // Untracked-file churn across the tree the way an IDE session does.
    let churning = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let churn_handle = {
        let churning = std::sync::Arc::clone(&churning);
        let churn_files: Vec<PathBuf> = churn_names.iter().map(|name| workdir.join(name)).collect();
        std::thread::spawn(move || {
            let mut tick: u32 = 0;
            while churning.load(std::sync::atomic::Ordering::Relaxed) {
                let file = &churn_files[(tick as usize) % churn_files.len()];
                let _ = fs::write(file, format!("churn {tick}\n"));
                tick += 1;
                std::thread::sleep(Duration::from_millis(12));
            }
        })
    };

    // One continuous history across the whole storm: the sentinel must be in
    // every staged sample from its first landing to the end. Rounds do not
    // wait for convergence — dispatches fire on a timer like real clicks.
    let mut history = LaneHistory::new();
    let storm_deadline = Instant::now() + Duration::from_secs(30);
    for round in 0..12u32 {
        store.dispatch(Msg::StagePaths {
            repo_id,
            paths: vec![target.clone()].into(),
        });
        let mut round_deadline = Instant::now() + Duration::from_millis(350);
        while Instant::now() < round_deadline && Instant::now() < storm_deadline {
            history.record(sample_lanes(&store));
            std::thread::sleep(Duration::from_millis(5));
        }
        store.dispatch(Msg::UnstagePaths {
            repo_id,
            paths: vec![target.clone()].into(),
        });
        round_deadline = Instant::now() + Duration::from_millis(350);
        while Instant::now() < round_deadline && Instant::now() < storm_deadline {
            history.record(sample_lanes(&store));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // Let the last refresh settle, then require exactly the pre-storm staged
    // set plus the sentinel back (the target ends unstaged).
    let settle_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let sample = sample_lanes(&store);
        history.record(sample.clone());
        if sample.staged.as_deref() == Some(expected_baseline.as_slice()) {
            break;
        }
        assert!(
            Instant::now() < settle_deadline,
            "real-repo storm: staged list never settled back to the baseline + sentinel; history:\n{:?}",
            history.samples
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    churning.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = churn_handle.join();

    let first_landing = history
        .samples
        .iter()
        .position(|(_, sample)| {
            sample
                .staged
                .as_ref()
                .is_some_and(|staged| staged.contains(&sentinel))
        })
        .expect("the sentinel never landed in the staged list");
    for (elapsed, sample) in &history.samples[first_landing..] {
        let missing: Vec<&PathBuf> = expected_baseline
            .iter()
            .filter(|path| {
                !sample
                    .staged
                    .as_ref()
                    .is_some_and(|staged| staged.contains(path))
            })
            .collect();
        assert!(
            missing.is_empty(),
            "real-repo storm: staged lane wrongly lost {missing:?} at {elapsed:?} — the reported bug (history:\n{:?})",
            history.samples
        );
    }
}
