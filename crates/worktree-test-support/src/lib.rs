//! Shared test helpers for the WorkTree workspace.
//!
//! This crate is the single canonical home for test helpers that were
//! historically copy-pasted across integration tests and `#[cfg(test)]`
//! modules. It must only ever appear in `[dev-dependencies]` — nothing here
//! may leak into production dependency graphs.

use std::path::Path;
use std::process::{Command, Output};

/// Run `git -c commit.gpgsign=false -C <repo> <args>` on a pre-configured
/// command and return the captured [`Output`] with no status assertion.
///
/// Signing is disabled explicitly so commits stay portable on hosts with
/// `commit.gpgsign=true` in the ambient git configuration.
///
/// This is the shared core behind every runner in this crate; call it
/// directly when the command must be built by the caller first (e.g. via a
/// no-window command constructor) and the caller wants to inspect the exit
/// status itself.
pub fn run_git_capture_command(cmd: &mut Command, repo: &Path, args: &[&str]) -> Output {
    cmd.arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {args:?}: {e}"))
}

/// Panic with the captured stdout/stderr unless `output` is a success.
fn assert_git_success(output: &Output, args: &[&str]) {
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run `git -c commit.gpgsign=false -C <repo> <args>` on a pre-configured
/// command, panicking with the captured stdout/stderr unless git exits
/// successfully.
///
/// This is the shared core behind [`run_git`] and [`run_git_with`]; call it
/// directly when the command must be built by the caller first (e.g. via a
/// no-window command constructor).
pub fn run_git_command(cmd: &mut Command, repo: &Path, args: &[&str]) {
    let output = run_git_capture_command(cmd, repo, args);
    assert_git_success(&output, args);
}

/// Canonical `run_git`: run `git -c commit.gpgsign=false -C <repo> <args>`
/// with no environment changes, panicking on failure.
///
/// Callers that need to adjust the command first (e.g. applying an isolated
/// git-config environment) should use [`run_git_with`]; callers that need to
/// build the command themselves should use [`run_git_command`].
pub fn run_git(repo: &Path, args: &[&str]) {
    run_git_with(repo, args, |_| {});
}

/// `run_git` on a command first adjusted by `configure`.
pub fn run_git_with(repo: &Path, args: &[&str], configure: impl FnOnce(&mut Command)) {
    let mut cmd = Command::new("git");
    configure(&mut cmd);
    run_git_command(&mut cmd, repo, args);
}

/// Run `git -c commit.gpgsign=false -C <repo> <args>` on a pre-configured
/// command, panicking with the captured stdout/stderr unless git exits
/// successfully, and return the captured stdout as a lossy string.
///
/// The string is NOT trimmed; callers that compare against expected output
/// should `.trim()` it themselves.
pub fn run_git_stdout_command(cmd: &mut Command, repo: &Path, args: &[&str]) -> String {
    let output = run_git_capture_command(cmd, repo, args);
    assert_git_success(&output, args);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `run_git_stdout_command` on a plain `git` command first adjusted by
/// `configure`.
pub fn run_git_stdout_with(
    repo: &Path,
    args: &[&str],
    configure: impl FnOnce(&mut Command),
) -> String {
    let mut cmd = Command::new("git");
    configure(&mut cmd);
    run_git_stdout_command(&mut cmd, repo, args)
}

/// Run `git -c commit.gpgsign=false -C <repo> <args>` on a pre-configured
/// command, panicking with the captured stdout/stderr if git exits
/// SUCCESSFULLY, and return the captured [`Output`] so the caller can assert
/// on the failure text.
pub fn run_git_expect_failure_command(cmd: &mut Command, repo: &Path, args: &[&str]) -> Output {
    let output = run_git_capture_command(cmd, repo, args);
    assert!(
        !output.status.success(),
        "git {args:?} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Detect the Git-for-Windows MSYS shell startup failure signature in
/// captured git output. Tests that would otherwise fail opaquely in such
/// environments use this to skip themselves.
///
/// Defined on all platforms (it is pure string matching); consumers that only
/// need it on Windows import it under `#[cfg(windows)]`.
pub fn is_git_shell_startup_failure(text: &str) -> bool {
    text.contains("sh.exe: *** fatal error -")
        && (text.contains("couldn't create signal pipe") || text.contains("CreateFileMapping"))
}

/// `git hash-object -w --stdin`: write `contents` as a blob into `repo`'s
/// object store and return its hex object id.
pub fn hash_blob(repo: &Path, contents: &[u8]) -> String {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git hash-object to run");

    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(contents)
        .expect("write blob contents");

    let output = child.wait_with_output().expect("wait for hash-object");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout)
        .expect("hash-object stdout utf8")
        .trim()
        .to_owned()
}

/// Pin a file's mtime to a fixed timestamp (2023-11-14T22:13:20Z) so
/// metadata-only change detection can be tested deterministically.
#[cfg(windows)]
pub fn set_fixed_mtime(path: &Path) {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Item -LiteralPath $env:WORKTREE_TARGET).LastWriteTimeUtc=[DateTimeOffset]::FromUnixTimeSeconds(1700000000).UtcDateTime",
        ])
        .env("WORKTREE_TARGET", path)
        .status()
        .expect("powershell to run");
    assert!(status.success());
}

/// Pin a file's mtime to a fixed timestamp (2023-11-14T22:13:20 local) so
/// metadata-only change detection can be tested deterministically.
///
/// `touch -d` is GNU-specific; `-t [[CC]YY]MMDDhhmm[.ss]` is supported on
/// both GNU/Linux and BSD/macOS.
#[cfg(not(windows))]
pub fn set_fixed_mtime(path: &Path) {
    let status = Command::new("touch")
        .arg("-t")
        .arg("202311142213.20")
        .arg(path)
        .status()
        .expect("touch to run");
    assert!(status.success());
}
