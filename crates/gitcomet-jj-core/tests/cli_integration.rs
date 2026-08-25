//! Integration tests against a real `jj` CLI over a throwaway colocated
//! repo. Every test skips (with a note on stderr) when `jj` or `git` is
//! unavailable or jj is older than the supported floor, so the suite runs
//! green on machines without jj.

use std::path::Path;
use std::process::Command;

use gitcomet_core::jj::parse_jj_version;
use gitcomet_core::process::background_command;
use gitcomet_jj_core::version::MIN_SUPPORTED_JJ_VERSION;
use gitcomet_jj_core::{ChangeId, JjCliRepository, JjLogQuery, JjRepository};

/// Whether a usable jj + git pair is on PATH, probed once per process.
fn tooling_ready() -> bool {
    let Ok(output) = background_command("jj").arg("--version").output() else {
        eprintln!("skipping: jj is not on PATH");
        return false;
    };
    if !output.status.success() {
        eprintln!("skipping: `jj --version` failed");
        return false;
    }
    let version_output = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match parse_jj_version(&version_output) {
        Some(version) if version >= MIN_SUPPORTED_JJ_VERSION => {
            if which_git().is_none() {
                eprintln!("skipping: git is not on PATH");
                return false;
            }
            true
        }
        Some(version) => {
            eprintln!("skipping: jj {version:?} is older than {MIN_SUPPORTED_JJ_VERSION:?}");
            false
        }
        None => {
            eprintln!("skipping: unrecognized `jj --version` output: {version_output}");
            false
        }
    }
}

fn which_git() -> Option<()> {
    background_command("git")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| ())
}

fn run(mut cmd: Command) -> String {
    let label = format!("{:?}", cmd);
    let output = cmd
        .output()
        .unwrap_or_else(|err| panic!("spawn {label}: {err}"));
    assert!(
        output.status.success(),
        "{label} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Create a colocated git+jj repo in `dir` with a resolvable identity.
fn init_colocated_repo(dir: &Path) {
    let mut git_init = background_command("git");
    git_init.arg("init").arg(dir);
    run(git_init);
    let mut git_config = background_command("git");
    git_config
        .arg("-C")
        .arg(dir)
        .arg("config")
        .arg("user.name")
        .arg("GitComet Test");
    run(git_config);
    let mut git_config = background_command("git");
    git_config
        .arg("-C")
        .arg(dir)
        .arg("config")
        .arg("user.email")
        .arg("test@gitcomet.example");
    run(git_config);
    // `--repository <dir>` refuses to bootstrap a repo that does not exist
    // yet, so the init runs with the target as its working directory.
    let mut jj_init = background_command("jj");
    jj_init
        .current_dir(dir)
        .arg("git")
        .arg("init")
        .arg("--colocate");
    run(jj_init);
}

fn open_repo(dir: &Path) -> JjCliRepository {
    JjCliRepository::open(dir).expect("repo opens")
}

/// Describe `@`, then open a fresh change — the jj commit gesture.
fn commit_change(repo: &JjCliRepository, message: &str) -> gitcomet_jj_core::JjChange {
    let current = repo.working_copy().expect("working copy");
    repo.describe(&current.change_id, message)
        .expect("describe");
    repo.new_change(None).expect("new change")
}

#[test]
fn log_describe_new_roundtrip() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());

    repo.snapshot().expect("snapshot");

    let first = repo.working_copy().expect("working copy");
    assert!(first.is_working_copy);
    assert!(first.description.is_empty());
    assert!(!first.change_id.0.is_empty());
    assert!(!first.commit_id.0.is_empty());

    repo.describe(&first.change_id, "first change\n\nbody line")
        .expect("describe");
    let second = repo.new_change(Some("second change")).expect("new change");
    assert!(second.is_working_copy);
    assert_eq!(second.description, "second change");
    assert_ne!(second.change_id, first.change_id);

    let page = repo.log(&JjLogQuery::new("all()", 10)).expect("log");
    let described = page
        .changes
        .iter()
        .find(|change| change.change_id == first.change_id)
        .expect("described change is in the log");
    // Multi-line descriptions survive the record framing verbatim.
    assert_eq!(described.description, "first change\n\nbody line");
    assert_eq!(described.author_name, "GitComet Test");
    assert_eq!(described.author_email, "test@gitcomet.example");
    assert!(described.committed_at_unix > 0);
    assert!(!described.is_working_copy);
}

#[test]
fn log_pages_through_the_cursor_without_overlap() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    commit_change(&repo, "change one");
    commit_change(&repo, "change two");
    commit_change(&repo, "change three");

    let mut seen: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for _ in 0..10 {
        let query = JjLogQuery::new("all()", 2).after(cursor);
        let page = repo.log(&query).expect("log page");
        assert!(page.changes.len() <= 2);
        for change in &page.changes {
            assert!(
                !seen.contains(&change.change_id.0),
                "change {} appeared on two pages",
                change.change_id.0
            );
            seen.push(change.change_id.0.clone());
        }
        match page.next_cursor {
            Some(next) => cursor = next,
            None => break,
        }
    }
    // root + 3 described changes + the open change on top.
    assert_eq!(seen.len(), 5, "paged over exactly the whole revset");
}

#[test]
fn bookmarks_roundtrip() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let change = repo.working_copy().expect("working copy");

    repo.bookmark_create("feature", &change.change_id)
        .expect("bookmark create");
    let bookmarks = repo.bookmarks().expect("bookmarks");
    let feature = bookmarks
        .iter()
        .find(|bookmark| bookmark.name == "feature")
        .expect("feature bookmark exists");
    assert!(feature.is_local());
    assert_eq!(feature.target_commit_id, change.commit_id);
    assert!(!feature.conflicted);

    repo.bookmark_rename("feature", "renamed")
        .expect("bookmark rename");
    let bookmarks = repo.bookmarks().expect("bookmarks");
    assert!(bookmarks.iter().all(|bookmark| bookmark.name != "feature"));
    assert!(bookmarks.iter().any(|bookmark| bookmark.name == "renamed"));

    repo.bookmark_delete("renamed").expect("bookmark delete");
    let bookmarks = repo.bookmarks().expect("bookmarks");
    assert!(bookmarks.iter().all(|bookmark| bookmark.name != "renamed"));
}

#[test]
fn op_log_undo_reverts_the_last_operation() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());

    let ops = repo.op_log(10).expect("op log");
    assert!(!ops.is_empty());
    assert!(!ops[0].op_id.is_empty());
    assert!(ops[0].started_at_unix > 0);

    repo.new_change(Some("to be undone")).expect("new change");
    assert_eq!(
        repo.working_copy().expect("working copy").description,
        "to be undone"
    );

    repo.op_undo().expect("op undo");
    // The undo rewrote the operation log; `@` is back to the change the
    // `jj new` replaced, whose description was never set.
    let working_copy = repo.working_copy().expect("working copy after undo");
    assert_ne!(working_copy.description, "to be undone");
}

#[test]
fn conflicts_empty_on_a_clean_repo() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    // `jj resolve --list` exits non-zero with "No conflicts found" — the
    // trait maps that to an empty list rather than an error.
    assert!(repo.conflicts().expect("conflicts").is_empty());
}

#[test]
fn abandon_removes_a_change() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let first = repo.working_copy().expect("working copy");
    repo.describe(&first.change_id, "kept").expect("describe");
    repo.new_change(None).expect("new change");
    let second = repo.working_copy().expect("working copy");
    repo.describe(&second.change_id, "abandoned soon")
        .expect("describe");
    repo.new_change(None).expect("new change");

    repo.abandon(&second.change_id).expect("abandon");
    let page = repo.log(&JjLogQuery::new("all()", 20)).expect("log");
    assert!(
        page.changes
            .iter()
            .all(|change| change.change_id != second.change_id),
        "abandoned change disappeared from the log"
    );
    assert!(
        page.changes
            .iter()
            .any(|change| change.change_id == first.change_id),
        "sibling change survives the abandon"
    );
}

#[test]
fn squash_folds_a_change_into_the_working_copy() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let first = repo.working_copy().expect("working copy");
    repo.describe(&first.change_id, "squash me")
        .expect("describe");
    repo.new_change(None).expect("new change");

    repo.squash(&first.change_id, None).expect("squash");
    // The squashed change is gone; the working copy absorbed it.
    let page = repo.log(&JjLogQuery::new("all()", 20)).expect("log");
    assert!(
        page.changes
            .iter()
            .all(|change| change.change_id != first.change_id)
    );
}

#[test]
fn split_is_unsupported_in_the_cli_implementation() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let change = repo.working_copy().expect("working copy");
    let err = repo.split(&change.change_id).expect_err("split refuses");
    assert!(matches!(
        err.kind(),
        gitcomet_core::error::ErrorKind::Unsupported(_)
    ));
}

/// `ChangeId` values from outside jj's output (e.g. hand-built) are still
/// valid arguments; this also pins the newtype's public surface.
#[test]
fn change_id_newtype_displays_as_its_value() {
    let id = ChangeId("wqnwyzpk".to_string());
    assert_eq!(id.to_string(), "wqnwyzpk");
    assert_eq!(id.as_ref(), "wqnwyzpk");
}

/// The change-detail reads: `--summary` lists the touched paths with their
/// status letters (including jj's brace rename notation), `--git` returns
/// one file's unified diff, and the empty change has no files at all.
#[test]
fn change_files_and_file_diff_text_read_a_change() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let root = temp.path().to_path_buf();

    // A rename is only detected against a committed baseline, so the flow
    // commits the file first, then moves it and adds a second file.
    std::fs::write(root.join("tracked.txt"), "one\n").expect("write tracked");
    commit_change(&repo, "baseline");
    std::fs::write(root.join("added.txt"), "brand new\n").expect("write added");
    let mut mv = background_command("mv");
    mv.arg(root.join("tracked.txt")).arg(root.join("moved.txt"));
    run(mv);
    let change = repo.working_copy().expect("working copy");

    let files = repo.change_files(&change.change_id).expect("change files");
    assert_eq!(files.len(), 2, "added + renamed: {files:?}");
    let added = files
        .iter()
        .find(|file| file.path == "added.txt")
        .expect("added file listed");
    assert_eq!(added.status, gitcomet_jj_core::JjFileStatus::Added);
    let renamed = files
        .iter()
        .find(|file| file.path == "moved.txt")
        .expect("rename target listed");
    assert_eq!(
        renamed.status,
        gitcomet_jj_core::JjFileStatus::Renamed {
            from: "tracked.txt".to_string()
        }
    );

    // The added file's unified diff carries its content line.
    let diff = repo
        .file_diff_text(&change.change_id, "added.txt")
        .expect("file diff");
    assert!(diff.contains("+++ b/added.txt"), "diff header: {diff}");
    assert!(diff.contains("+brand new"), "diff body: {diff}");

    // A path outside the change reads as an empty diff, not an error.
    assert_eq!(
        repo.file_diff_text(&change.change_id, "no-such-file.txt")
            .expect("empty diff"),
        ""
    );
}

/// The freshly opened working copy has no edits, so its file list is empty.
#[test]
fn change_files_empty_on_the_open_change() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let change = repo.working_copy().expect("working copy");
    assert!(
        repo.change_files(&change.change_id)
            .expect("change files")
            .is_empty()
    );
}

/// `new_change_at` opens a fresh change on top of any log row (not just @)
/// and moves the working copy there — the "start work here" gesture.
#[test]
fn new_change_at_opens_a_change_on_top_of_a_selected_row() {
    if !tooling_ready() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    init_colocated_repo(temp.path());
    let repo = open_repo(temp.path());
    let base = commit_change(&repo, "base change");
    commit_change(&repo, "tip change");

    let here = repo.new_change_at(&base.change_id).expect("new change at");
    assert!(here.is_working_copy);
    assert_ne!(here.change_id, base.change_id);
    // The new change sits directly on top of the target: the target is @'s
    // only parent.
    let page = repo.log(&JjLogQuery::new("parents(@)", 10)).expect("log");
    assert_eq!(page.changes.len(), 1, "the new @ has exactly one parent");
    assert_eq!(page.changes[0].change_id, base.change_id);
}
