use repositorytree_core::domain::CommitId;
use repositorytree_core::services::{BisectVerdict, GitBackend, GitRepository};
use repositorytree_git_gix::GixBackend;
#[path = "support/test_git_env.rs"]
mod test_git_env;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn run_git(repo: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    test_git_env::apply(&mut cmd);
    let status = cmd
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .status()
        .expect("git command to run");
    assert!(status.success(), "git {:?} failed", args);
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    test_git_env::apply(&mut cmd);
    let output = cmd
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("EDITOR", "true")
        .env("VISUAL", "true")
        .output()
        .expect("git command to run");
    assert!(output.status.success(), "git {:?} failed", args);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo(repo: &Path) {
    fs::create_dir_all(repo).expect("create repo directory");
    run_git(repo, &["init", "-b", "main"]);
    run_git(repo, &["config", "user.email", "you@example.com"]);
    run_git(repo, &["config", "user.name", "You"]);
}

fn commit_file(repo: &Path, name: &str, content: &str, message: &str) -> String {
    fs::write(repo.join(name), content).expect("write file");
    run_git(repo, &["add", "."]);
    run_git(
        repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
    git_stdout(repo, &["rev-parse", "HEAD"])
}

fn open_backend(repo: &Path) -> Arc<dyn GitRepository> {
    GixBackend.open(repo).expect("open repository")
}

/// Seven linear commits c1..c7 on `main`; shas returned oldest-first.
fn setup_linear_history(repo: &Path) -> Vec<String> {
    init_repo(repo);
    (1..=7)
        .map(|i| {
            commit_file(repo, "file.txt", &format!("line {i}\n"), &format!("c{i}"))
        })
        .collect()
}

fn commit_id(sha: &str) -> CommitId {
    CommitId(sha.into())
}

#[test]
fn bisect_state_is_none_when_not_bisecting() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path();
    setup_linear_history(repo);

    let backend = open_backend(repo);
    assert_eq!(backend.bisect_state().unwrap(), None);
}

#[test]
fn bisect_round_trip_start_skip_mark_reset() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path();
    let shas = setup_linear_history(repo);
    let oldest = &shas[0];
    let newest = &shas[6];

    let backend = open_backend(repo);
    let output = backend
        .bisect_start_with_output(Some(newest), &[oldest.clone()])
        .expect("start bisect");
    assert_eq!(output.command, format!("git bisect start {newest} {oldest}"));
    assert_eq!(
        git_stdout(repo, &["branch", "--show-current"]),
        "",
        "bisect checks out a detached candidate, not a branch"
    );

    let state = backend.bisect_state().unwrap().expect("bisect state");
    assert_eq!(state.original_branch.as_deref(), Some("main"));
    assert_eq!(state.bad, Some(commit_id(newest)));
    assert_eq!(state.good, vec![commit_id(oldest)]);
    assert!(state.skipped.is_empty());
    let midpoint = git_stdout(repo, &["rev-parse", "HEAD"]);
    assert_eq!(state.current, Some(commit_id(&midpoint)));

    // Skip the checked-out candidate, then mark an explicit commit good —
    // both the HEAD and the named-commit forms of `git bisect <word>`.
    backend
        .bisect_mark_with_output(BisectVerdict::Skip, None)
        .expect("skip candidate");
    let state = backend.bisect_state().unwrap().expect("bisect state");
    assert_eq!(state.skipped, vec![commit_id(&midpoint)]);

    let mark_output = backend
        .bisect_mark_with_output(BisectVerdict::Good, Some(&shas[3]))
        .expect("mark explicit commit good");
    assert_eq!(mark_output.command, format!("git bisect good {}", shas[3]));
    let state = backend.bisect_state().unwrap().expect("bisect state");
    assert!(state.good.contains(&commit_id(&shas[3])));

    let reset_output = backend.bisect_reset_with_output().expect("reset bisect");
    assert_eq!(reset_output.command, "git bisect reset");
    assert_eq!(backend.bisect_state().unwrap(), None);
    assert_eq!(
        git_stdout(repo, &["branch", "--show-current"]),
        "main",
        "reset returns to the branch recorded in BISECT_START"
    );
}

#[test]
fn bisect_start_bare_then_mark_converges_to_first_bad_commit() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let repo = dir.path();
    let shas = setup_linear_history(repo);
    let oldest = &shas[0];

    let backend = open_backend(repo);
    backend
        .bisect_start_with_output(None, &[])
        .expect("start bare bisect");
    let state = backend.bisect_state().unwrap().expect("bisect state");
    assert_eq!(state.original_branch.as_deref(), Some("main"));
    assert_eq!(state.bad, None);
    assert!(state.good.is_empty());

    // The UI flow for "start here as bad": mark the tip bad, then anchor the
    // known-good base. Until a good mark exists git checks out no candidate
    // ("waiting for good commit(s), bad commit known") and HEAD stays on the
    // bad tip, so the good anchor must come before the candidate questions.
    // Convergence keeps the session open until reset, HEAD stays checked out
    // on the first bad commit, and the state must still parse.
    backend
        .bisect_mark_with_output(BisectVerdict::Bad, Some(&shas[6]))
        .expect("mark tip bad");
    backend
        .bisect_mark_with_output(BisectVerdict::Good, Some(oldest))
        .expect("mark base good");
    let mut marks = 0;
    loop {
        let state = backend.bisect_state().unwrap().expect("bisect state");
        // Converged when the newest bad is itself the checked-out commit:
        // mid-session a fresh candidate sits between good and bad instead.
        if state.bad == state.current
            && state.bad.as_ref().map(AsRef::<str>::as_ref) == Some(shas[4].as_str())
        {
            break;
        }
        let Some(current) = state.current.clone() else {
            panic!("bisect lost its candidate before converging");
        };
        // Introduce the regression at c5: everything from shas[4] on is bad.
        let is_bad = shas[4..].iter().any(|sha| sha == current.as_ref());
        backend
            .bisect_mark_with_output(
                if is_bad {
                    BisectVerdict::Bad
                } else {
                    BisectVerdict::Good
                },
                None,
            )
            .expect("mark candidate");
        marks += 1;
        assert!(marks <= 7, "bisect should converge within the history size");
    }

    let state = backend.bisect_state().unwrap().expect("bisect state");
    assert_eq!(
        state.bad.as_ref().map(AsRef::<str>::as_ref),
        Some(shas[4].as_str())
    );
    assert_eq!(state.current, state.bad, "converged session sits on the first bad commit");

    backend.bisect_reset_with_output().expect("reset bisect");
    assert_eq!(backend.bisect_state().unwrap(), None);
}
