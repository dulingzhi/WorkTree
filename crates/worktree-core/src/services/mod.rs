use crate::domain::*;
use crate::error::{Error, ErrorKind};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check_cancelled(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::new(ErrorKind::Cancelled))
        } else {
            Ok(())
        }
    }
}

/// A partially built log page, reported while a walk is still running.
///
/// `commits` is the page so far — every chunk is a prefix of the next one and
/// of the final page — and `scanned` counts the commits the walk has visited,
/// matching or not, so a filter that is finding nothing still shows progress.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogChunk {
    pub commits: Vec<crate::domain::Commit>,
    pub scanned: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

impl CommandOutput {
    pub fn empty_success(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    pub fn combined(&self) -> String {
        let mut out = String::new();
        if !self.stdout.trim().is_empty() {
            out.push_str(self.stdout.trim_end());
            out.push('\n');
        }
        if !self.stderr.trim().is_empty() {
            out.push_str(self.stderr.trim_end());
            out.push('\n');
        }
        out.trim_end().to_string()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictSide {
    Ours,
    Theirs,
}

/// Result of launching an external mergetool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergetoolResult {
    /// The tool command that was invoked.
    pub tool_name: String,
    /// Whether the tool reported success (exit code 0 or trust-exit-code semantics).
    pub success: bool,
    /// The merged file contents read back after the tool exited, if available.
    pub merged_contents: Option<Vec<u8>>,
    /// Combined stdout/stderr from the tool invocation for diagnostics.
    pub output: CommandOutput,
}

/// Try to decode optional bytes as UTF-8. Returns `None` if the bytes are
/// `None` or not valid UTF-8.
pub fn decode_utf8_optional(bytes: Option<&[u8]>) -> Option<String> {
    bytes.and_then(|b| std::str::from_utf8(b).ok().map(str::to_owned))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConflictTextValidation {
    pub has_conflict_markers: bool,
    pub marker_lines: usize,
}

/// Validate merged text before staging by scanning for unresolved
/// conflict marker lines.
pub fn validate_conflict_resolution_text(text: &str) -> ConflictTextValidation {
    let marker_lines = text
        .lines()
        .filter(|line| {
            line.starts_with("<<<<<<<")
                || line.starts_with(">>>>>>>")
                || line.starts_with("=======")
                || line.starts_with("|||||||")
        })
        .count();

    ConflictTextValidation {
        has_conflict_markers: marker_lines > 0,
        marker_lines,
    }
}

/// The path-targeted status rescan result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusForPaths {
    /// Fresh entries covering exactly the requested paths, per lane.
    Lists {
        unstaged: Vec<FileStatus>,
        staged: Vec<FileStatus>,
    },
    /// The targeted scan hit a shape incremental merge does not replicate
    /// (rename/copy records); a full scan is the honest answer.
    NeedsFullScan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictFileStages {
    pub path: PathBuf,
    pub base_bytes: Option<Arc<[u8]>>,
    pub ours_bytes: Option<Arc<[u8]>>,
    pub theirs_bytes: Option<Arc<[u8]>>,
    pub base: Option<Arc<str>>,
    pub ours: Option<Arc<str>>,
    pub theirs: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteUrlKind {
    Fetch,
    Push,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractiveRebaseAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl InteractiveRebaseAction {
    pub fn to_todo_str(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }

    /// Inverse of [`Self::to_todo_str`], also accepting git's single-letter
    /// abbreviations. `None` for todo commands that are not entry actions
    /// (`exec`, `merge`, `label`, …).
    pub fn from_todo_word(word: &str) -> Option<Self> {
        Some(match word {
            "pick" | "p" => Self::Pick,
            "reword" | "r" => Self::Reword,
            "squash" | "s" => Self::Squash,
            "fixup" | "f" => Self::Fixup,
            "drop" | "d" => Self::Drop,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SequencerState {
    #[default]
    None,
    RebaseOrApply,
    CherryPick,
}

/// A `git bisect good|bad|skip` verdict handed to `bisect_mark_with_output`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BisectVerdict {
    Good,
    Bad,
    Skip,
}

impl BisectVerdict {
    /// The subcommand word git itself uses (`git bisect <word>`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Bad => "bad",
            Self::Skip => "skip",
        }
    }
}

/// Snapshot of an in-progress `git bisect` session, mirroring how
/// [`SequencerState`] models a rebase or cherry-pick in progress. Marks come
/// from the `# bad|good|skip: [<sha>]` summary lines of `git bisect log`,
/// which carry resolved shas (the replayable `git bisect <word> <term>` lines
/// repeat whatever terms the user originally typed, so they are not parsed).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BisectState {
    /// Branch name (or detached sha) checked out before the bisect started,
    /// from `.git/BISECT_START` — where `git bisect reset` returns to.
    pub original_branch: Option<String>,
    /// The latest commit marked bad (`refs/bisect/bad`).
    pub bad: Option<CommitId>,
    /// Every commit marked good so far (`refs/bisect/good-*`).
    pub good: Vec<CommitId>,
    /// Every commit marked skip so far, in mark order.
    pub skipped: Vec<CommitId>,
    /// The candidate commit currently checked out for testing (HEAD).
    pub current: Option<CommitId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractiveRebaseEntry {
    pub action: InteractiveRebaseAction,
    pub commit_id: String,
    /// Single-line original commit subject (git's `%s`), used for list display
    /// and autosquash grouping. Never reflects `new_message` — display code
    /// derives an edited subject via `squash::split_subject_body`.
    pub summary: String,
    /// Full original commit message (subject + body). Seeds the reword dialog
    /// (`squash::reword_seed_message`) and contributes to combined squash
    /// messages; never edited in place.
    pub message: String,
    /// Full replacement message (subject + body), set only when action is
    /// Reword. Its subject may differ from `summary`.
    pub new_message: Option<String>,
}

/// One conflicted path reported by `git merge-tree --write-tree`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeConflictFile {
    /// Repo-relative path git named for the conflict (`Merge conflict in
    /// <path>`). Falls back to the full conflict detail for kinds git does not
    /// phrase that way (e.g. rename/rename).
    pub path: String,
    /// Git's conflict classification, e.g. `add/add`, `content`,
    /// `rename/rename`, `modify/delete`.
    pub conflict_type: String,
}

/// Read-only result of previewing a merge (`git merge-tree --write-tree <head>
/// <other>`): the toplevel tree the merge *would* produce — conflicted files
/// carry conflict markers — plus the conflict list. Never touches the worktree,
/// index or refs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeTreePreview {
    /// The 40-hex toplevel tree OID the merge would produce.
    pub result_tree: String,
    /// True when the merge would leave conflicts (git exited non-zero).
    pub has_conflict: bool,
    /// Conflicted paths, in git's reporting order.
    pub conflicts: Vec<MergeConflictFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmoduleTrustTarget {
    pub submodule_path: PathBuf,
    pub display_source: String,
    pub local_source_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmoduleTrustDecision {
    Proceed,
    Prompt { sources: Vec<SubmoduleTrustTarget> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlameLine {
    pub commit_id: Arc<str>,
    pub author: Arc<str>,
    pub author_time_unix: Option<i64>,
    pub summary: Arc<str>,
    pub body: Option<Arc<str>>,
    pub line: String,
    /// Whether the blamed file existed in the first parent of `commit_id`.
    /// When `false`, "view file at parent commit" is a dead end (this commit
    /// introduced the file), so the UI hides that affordance.
    pub prior_exists: bool,
    /// The file's path at `commit_id`, when it differs from the blamed path
    /// because the file was renamed at/after that commit. `None` means the path
    /// is the same as the blamed path. Used so "view file at this commit" and
    /// "prior revision" navigate using the historical name rather than the
    /// current one (which may not exist in that older tree).
    pub source_path: Option<PathBuf>,
    /// For uncommitted ("Not Committed Yet") lines, the commit the working-tree
    /// change is based on (git blame porcelain `previous`), i.e. the revision to
    /// open for "view file at parent commit". `None` for committed lines (which
    /// resolve their parent from `commit_id`) and for uncommitted lines with no
    /// base (newly added files).
    pub prior_commit: Option<Arc<str>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitOperationOutcome {
    pub local_branch: Option<String>,
    pub pre_head: Option<CommitId>,
    pub post_head: Option<CommitId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafePushAfterCommitContext {
    pub amend: bool,
    pub local_branch: Option<String>,
    pub pre_head: Option<CommitId>,
    pub post_head: Option<CommitId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafePushAfterCommitTarget {
    pub remote: String,
    pub branch: String,
    pub local_branch: String,
    pub local_head: CommitId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForcePushLease {
    pub remote: String,
    pub branch: String,
    pub expected: CommitId,
    pub local_branch: String,
    pub local_head: CommitId,
}

/// GitLab merge-request push options, translated to `git push -o
/// merge_request.*` flags. `push_to_mr_branch` covers the "create MR branch"
/// gesture from the C# client: push HEAD to `MR/<branch>` on the remote so
/// the merge request is opened against that throwaway branch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergeRequestPushOptions {
    /// Pass `-o merge_request.create`.
    pub create: bool,
    /// Pass `-o merge_request.target=<branch>`; `None` lets the server use
    /// its default branch.
    pub target_branch: Option<String>,
    /// Pass `-o merge_request.merge_when_pipeline_succeeds`.
    pub merge_when_pipeline_succeeds: bool,
    /// Pass `-o merge_request.remove_source_branch`.
    pub remove_source_branch: bool,
    /// Push HEAD to `MR/<current-branch>` instead of the branch's own ref.
    pub push_to_mr_branch: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SafePushAfterCommitDecision {
    Push {
        target: SafePushAfterCommitTarget,
    },
    PushSetUpstream {
        target: SafePushAfterCommitTarget,
    },
    Blocked {
        summary: String,
        lease: Option<ForcePushLease>,
    },
}

mod diff;
mod history;
mod log;
mod porcelain;
mod remotes;
mod status;
mod worktree;
pub use diff::GitRepositoryDiff;
pub use history::GitRepositoryHistory;
pub use log::GitRepositoryLog;
pub use porcelain::GitRepositoryPorcelain;
pub use remotes::GitRepositoryRemotes;
pub use status::GitRepositoryStatus;
pub use worktree::GitRepositoryWorktree;
/// Aggregate trait: every `GitRepository` implementor provides every domain
/// trait. Keeping the single `GitRepository` name means `dyn GitRepository`,
/// `Arc<dyn GitRepository>` and every existing generic bound are unchanged by
/// the domain split.
///
/// Two default bodies stay here rather than in a domain trait because they are
/// genuinely cross-domain: `validate_safe_push_after_commit_target` reads
/// `current_branch` (Porcelain) and `head_commit_id` (Log).
pub trait GitRepository:
    GitRepositoryLog
    + GitRepositoryHistory
    + GitRepositoryRemotes
    + GitRepositoryStatus
    + GitRepositoryDiff
    + GitRepositoryPorcelain
    + GitRepositoryWorktree
    + Send
    + Sync
{
    fn spec(&self) -> &RepoSpec;

    fn push_after_commit_with_output(
        &self,
        target: &SafePushAfterCommitTarget,
    ) -> Result<CommandOutput> {
        validate_safe_push_after_commit_target(self, target)?;
        self.push_with_output()
    }

    fn push_after_commit_set_upstream_with_output(
        &self,
        target: &SafePushAfterCommitTarget,
    ) -> Result<CommandOutput> {
        validate_safe_push_after_commit_target(self, target)?;
        self.push_set_upstream_with_output(&target.remote, &target.branch)
    }
}

fn validate_safe_push_after_commit_target<R: GitRepository + ?Sized>(
    repo: &R,
    target: &SafePushAfterCommitTarget,
) -> Result<()> {
    let current_branch = repo.current_branch()?;
    if current_branch != target.local_branch {
        return Err(Error::new(ErrorKind::Backend(format!(
            "stale push-after-commit target: expected branch {}, but current branch is {}",
            target.local_branch, current_branch
        ))));
    }

    let current_head = repo.head_commit_id()?.ok_or_else(|| {
        Error::new(ErrorKind::Backend(
            "stale push-after-commit target: current HEAD does not point to a commit".to_string(),
        ))
    })?;
    if current_head != target.local_head {
        return Err(Error::new(ErrorKind::Backend(format!(
            "stale push-after-commit target: expected HEAD {}, but current HEAD is {}",
            target.local_head, current_head
        ))));
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PullMode {
    Default,
    Merge,
    FastForwardIfPossible,
    FastForwardOnly,
    Rebase,
}

pub trait GitBackend: Send + Sync {
    fn open(&self, workdir: &Path) -> Result<Arc<dyn GitRepository>>;

    fn open_cancellable(
        &self,
        workdir: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Arc<dyn GitRepository>> {
        cancellation.check_cancelled()?;
        let repo = self.open(workdir)?;
        cancellation.check_cancelled()?;
        Ok(repo)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BlameLine, CommandOutput, GitRepository, GitRepositoryDiff, GitRepositoryHistory,
        GitRepositoryLog, GitRepositoryPorcelain, GitRepositoryRemotes, GitRepositoryStatus,
        GitRepositoryWorktree, decode_utf8_optional, validate_conflict_resolution_text,
    };
    use crate::domain::{
        Branch, CommitDetails, CommitId, DiffTarget, HistoryMode, LogCursor, LogPage, ReflogEntry,
        Remote, RemoteBranch, RepoSpec, RepoStatus, StashEntry,
    };
    use crate::error::{Error, ErrorKind};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    fn unsupported<T>() -> super::Result<T> {
        Err(Error::new(ErrorKind::Unsupported(
            "unused in services history-mode delegation test",
        )))
    }

    struct RecordingHistoryModeRepo {
        spec: RepoSpec,
        calls: Mutex<Vec<(&'static str, usize, Option<String>)>>,
    }

    impl RecordingHistoryModeRepo {
        fn new() -> Self {
            Self {
                spec: RepoSpec {
                    workdir: PathBuf::from("/tmp/recording-history-mode-repo"),
                },
                calls: Mutex::new(Vec::new()),
            }
        }

        fn record(&self, method: &'static str, limit: usize, cursor: Option<&LogCursor>) {
            self.calls.lock().expect("recording mutex").push((
                method,
                limit,
                cursor.map(|cursor| cursor.last_seen.as_ref().to_string()),
            ));
        }

        fn calls(&self) -> Vec<(&'static str, usize, Option<String>)> {
            self.calls.lock().expect("recording mutex").clone()
        }
    }

    impl GitRepository for RecordingHistoryModeRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for RecordingHistoryModeRepo {
        fn log_head_page(
            &self,
            limit: usize,
            cursor: Option<&LogCursor>,
        ) -> super::Result<LogPage> {
            self.record("head", limit, cursor);
            Ok(LogPage {
                commits: Vec::new(),
                next_cursor: None,
            })
        }

        fn log_all_branches_page(
            &self,
            limit: usize,
            cursor: Option<&LogCursor>,
        ) -> super::Result<LogPage> {
            self.record("all", limit, cursor);
            Ok(LogPage {
                commits: Vec::new(),
                next_cursor: None,
            })
        }

        fn commit_details(&self, _id: &CommitId) -> super::Result<CommitDetails> {
            unsupported()
        }

        fn reflog_head(&self, _limit: usize) -> super::Result<Vec<ReflogEntry>> {
            unsupported()
        }
    }
    impl GitRepositoryHistory for RecordingHistoryModeRepo {}
    impl GitRepositoryRemotes for RecordingHistoryModeRepo {
        fn list_remotes(&self) -> super::Result<Vec<Remote>> {
            unsupported()
        }

        fn list_remote_branches(&self) -> super::Result<Vec<RemoteBranch>> {
            unsupported()
        }

        fn fetch_all(&self) -> super::Result<()> {
            unsupported()
        }

        fn pull(&self, _mode: super::PullMode) -> super::Result<()> {
            unsupported()
        }

        fn push(&self) -> super::Result<()> {
            unsupported()
        }
    }
    impl GitRepositoryStatus for RecordingHistoryModeRepo {
        fn status(&self) -> super::Result<RepoStatus> {
            unsupported()
        }
    }
    impl GitRepositoryDiff for RecordingHistoryModeRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> super::Result<String> {
            unsupported()
        }
    }
    impl GitRepositoryPorcelain for RecordingHistoryModeRepo {
        fn current_branch(&self) -> super::Result<String> {
            unsupported()
        }

        fn list_branches(&self) -> super::Result<Vec<Branch>> {
            unsupported()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> super::Result<()> {
            unsupported()
        }

        fn delete_branch(&self, _name: &str) -> super::Result<()> {
            unsupported()
        }

        fn checkout_branch(&self, _name: &str) -> super::Result<()> {
            unsupported()
        }

        fn checkout_commit(&self, _id: &CommitId) -> super::Result<()> {
            unsupported()
        }

        fn cherry_pick(&self, _id: &CommitId) -> super::Result<()> {
            unsupported()
        }

        fn revert(&self, _id: &CommitId) -> super::Result<()> {
            unsupported()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> super::Result<()> {
            unsupported()
        }

        fn stash_list(&self) -> super::Result<Vec<StashEntry>> {
            unsupported()
        }

        fn stash_apply(&self, _index: usize) -> super::Result<()> {
            unsupported()
        }

        fn stash_drop(&self, _index: usize) -> super::Result<()> {
            unsupported()
        }

        fn stage(&self, _paths: &[&Path]) -> super::Result<()> {
            unsupported()
        }

        fn unstage(&self, _paths: &[&Path]) -> super::Result<()> {
            unsupported()
        }

        fn commit(&self, _message: &str) -> super::Result<()> {
            unsupported()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> super::Result<()> {
            unsupported()
        }
    }
    impl GitRepositoryWorktree for RecordingHistoryModeRepo {}

    // ── validate_conflict_resolution_text ────────────────────────────

    #[test]
    fn validate_conflict_resolution_text_reports_no_markers() {
        let validation = validate_conflict_resolution_text("line 1\nline 2\n");
        assert!(!validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 0);
    }

    #[test]
    fn validate_conflict_resolution_text_counts_marker_lines() {
        let text = "<<<<<<< ours\nx\n=======\ny\n>>>>>>> theirs\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 3);
    }

    #[test]
    fn validate_empty_text_reports_no_markers() {
        let validation = validate_conflict_resolution_text("");
        assert!(!validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 0);
    }

    #[test]
    fn validate_diff3_markers_detected() {
        let text = "<<<<<<< ours\na\n||||||| base\nb\n=======\nc\n>>>>>>> theirs\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 4);
    }

    #[test]
    fn validate_markers_with_branch_annotations_detected() {
        let text = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature/my-branch\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 3);
    }

    #[test]
    fn validate_partial_marker_set_detected() {
        // Only start marker — still detects it
        let text = "some code\n<<<<<<< HEAD\nmore code\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 1);
    }

    #[test]
    fn validate_markers_not_at_start_of_line_ignored() {
        // Markers must be at line start to count
        let text = "  <<<<<<< not a marker\n  ======= not a marker\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(!validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 0);
    }

    #[test]
    fn validate_multiple_conflicts_counts_all_markers() {
        let text = "\
<<<<<<< HEAD\na\n=======\nb\n>>>>>>> branch1\n\
<<<<<<< HEAD\nc\n=======\nd\n>>>>>>> branch2\n";
        let validation = validate_conflict_resolution_text(text);
        assert!(validation.has_conflict_markers);
        assert_eq!(validation.marker_lines, 6);
    }

    // ── decode_utf8_optional ─────────────────────────────────────────

    #[test]
    fn decode_utf8_none_returns_none() {
        assert_eq!(decode_utf8_optional(None), None);
    }

    #[test]
    fn decode_utf8_valid_returns_string() {
        let bytes = b"hello world";
        assert_eq!(
            decode_utf8_optional(Some(bytes.as_slice())),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn decode_utf8_invalid_returns_none() {
        let bytes = &[0xff, 0xfe, 0x00, 0x01];
        assert_eq!(decode_utf8_optional(Some(bytes.as_slice())), None);
    }

    #[test]
    fn decode_utf8_empty_bytes_returns_empty_string() {
        let bytes: &[u8] = b"";
        assert_eq!(decode_utf8_optional(Some(bytes)), Some(String::new()));
    }

    #[test]
    fn decode_utf8_multibyte_chars_preserved() {
        let text = "héllo wörld 日本語";
        assert_eq!(
            decode_utf8_optional(Some(text.as_bytes())),
            Some(text.to_string())
        );
    }

    // ── CommandOutput ────────────────────────────────────────────────

    #[test]
    fn command_output_empty_success_has_zero_exit_code() {
        let out = CommandOutput::empty_success("git status");
        assert_eq!(out.command, "git status");
        assert_eq!(out.stdout, "");
        assert_eq!(out.stderr, "");
        assert_eq!(out.exit_code, Some(0));
    }

    #[test]
    fn command_output_combined_stdout_only() {
        let out = CommandOutput {
            command: "test".into(),
            stdout: "output line\n".into(),
            stderr: String::new(),
            exit_code: Some(0),
        };
        assert_eq!(out.combined(), "output line");
    }

    #[test]
    fn command_output_combined_stderr_only() {
        let out = CommandOutput {
            command: "test".into(),
            stdout: String::new(),
            stderr: "error message\n".into(),
            exit_code: Some(1),
        };
        assert_eq!(out.combined(), "error message");
    }

    #[test]
    fn command_output_combined_both_streams() {
        let out = CommandOutput {
            command: "test".into(),
            stdout: "output\n".into(),
            stderr: "warning\n".into(),
            exit_code: Some(0),
        };
        assert_eq!(out.combined(), "output\nwarning");
    }

    #[test]
    fn command_output_combined_empty_when_both_blank() {
        let out = CommandOutput {
            command: "test".into(),
            stdout: "   \n".into(),
            stderr: "  \n".into(),
            exit_code: Some(0),
        };
        assert_eq!(out.combined(), "");
    }

    #[test]
    fn command_output_combined_trims_trailing_whitespace() {
        let out = CommandOutput {
            command: "test".into(),
            stdout: "line1\nline2\n\n".into(),
            stderr: "err\n\n".into(),
            exit_code: Some(0),
        };
        assert_eq!(out.combined(), "line1\nline2\nerr");
    }

    #[test]
    fn command_output_default_has_no_exit_code() {
        let out = CommandOutput::default();
        assert_eq!(out.command, "");
        assert_eq!(out.exit_code, None);
    }

    #[test]
    fn blame_line_clone_shares_arc_metadata() {
        let line = BlameLine {
            commit_id: "deadbeef".into(),
            author: "Alice".into(),
            author_time_unix: Some(1_700_000_000),
            summary: "Initial import".into(),
            body: Some("detailed body".into()),
            line: "hello".to_string(),
            prior_exists: true,
            source_path: None,
            prior_commit: None,
        };

        let cloned = line.clone();
        assert!(Arc::ptr_eq(&line.commit_id, &cloned.commit_id));
        assert!(Arc::ptr_eq(&line.author, &cloned.author));
        assert!(Arc::ptr_eq(&line.summary, &cloned.summary));
        assert_eq!(line.line, cloned.line);
    }

    #[test]
    fn log_history_mode_page_delegates_current_branch_modes_to_head_log() {
        let repo = RecordingHistoryModeRepo::new();
        let cursor = LogCursor {
            last_seen: CommitId("cursor".into()),
            resume_from: Some(CommitId("resume".into())),
            resume_token: Some(Arc::from("token")),
        };

        for mode in [
            HistoryMode::FullReachable,
            HistoryMode::FirstParent,
            HistoryMode::NoMerges,
            HistoryMode::MergesOnly,
            HistoryMode::AllBranches,
        ] {
            repo.log_history_mode_page(mode, 7, Some(&cursor))
                .expect("history mode delegation should succeed");
        }

        assert_eq!(
            repo.calls(),
            vec![
                ("head", 7, Some("cursor".to_string())),
                ("head", 7, Some("cursor".to_string())),
                ("head", 7, Some("cursor".to_string())),
                ("head", 7, Some("cursor".to_string())),
                ("all", 7, Some("cursor".to_string())),
            ]
        );
    }
}
