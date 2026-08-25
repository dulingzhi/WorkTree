//! Colocated-Jujutsu adapter over the gix backend (Plan A, P1).
//!
//! A colocated repo shares the git object database and refs with jj, so every
//! read keeps working through [`GixRepo`] unchanged. Writes are the problem:
//! jj snapshots the working copy and syncs the git index/HEAD around every
//! command, so a git-side write would race it. Commit (`describe` + `new`),
//! bookmark, and network (`jj git fetch`/`push`) writes are routed through
//! the jj CLI; every other write returns [`ErrorKind::Unsupported`] until its
//! routing lands — the `RepoCapabilities::read_only` flag in the reducer
//! keeps those paths from being reached in the first place; the Unsupported
//! error is the second lock on the same door. On identities: jj 0.44 does
//! not read git config, so [`JjRepository::jj_cmd`] bridges any missing
//! `user.name`/`user.email` halves from git config onto every jj invocation.
//! One residual case cannot be repaired from here — a change created before
//! any identity existed keeps its empty author (jj offers no command to
//! rewrite it), and `jj git push` refuses to publish it; configuring jj's
//! identity is the user-side fix.
//!
//! # How a colocated repo actually syncs (verified against jj 0.44)
//!
//! Every jj command runs snapshot + import git head/refs + export bookmarks,
//! but the working-copy commit `@` itself stays *invisible to git*: a plain
//! `jj st` after a worktree edit moves nothing on the git side — git HEAD
//! keeps pointing at `@`'s parent and the index keeps that commit's tree, so
//! plain-git status keeps showing the edit as unstaged. Git HEAD (and the
//! index) only move when `@`'s parent moves (`jj new`, `jj edit`, …), and
//! bookmark moves are exported to git refs immediately. Both write to `.git`,
//! so the existing file-watcher → `RepoExternallyChanged` refresh pipeline
//! picks them up with no help from us.
//!
//! Consequences for this adapter: gix reads never *need* a snapshot to stay
//! fresh — `status` naturally reads as the jj working-copy change
//! (diff(`@`'s tree, worktree)) with an empty staged lane. One nuance: a
//! snapshot records brand-new working-copy files in the git index as
//! intent-to-add entries, so their unstaged classification flips
//! `Untracked` → `Added` across a snapshot (`git status` shows the same ` A`).
//! The lane never changes — only the label. What the snapshot trigger below
//! buys is jj-side freshness while GitComet is open: edits get absorbed into
//! `@` periodically, so the user's other jj tooling (terminal `jj log`,
//! op log) sees current state even if they never run a jj command themselves.
//! It is rate-limited because a snapshot is a process spawn.

use crate::repo::GixRepo;
use crate::util::{bytes_to_text_preserving_utf8, validate_hex_commit_id, validate_ref_like_arg};
use gitcomet_core::domain::{
    Branch, CommitDetails, CommitFileChange, CommitId, DiffArea, DiffPreviewTextSide, DiffTarget,
    FileDiffImage, FileDiffText, FileEntry, LogCursor, LogPage, RecentCommitMessage, RefMetadata,
    ReflogEntry, Remote, RemoteBranch, RemoteTag, RepoSpec, RepoStatus, StashEntry, Submodule,
    SubmoduleDiffSummary, Tag, UpstreamDivergence, Worktree,
};
use gitcomet_core::error::{Error, ErrorKind};
use gitcomet_core::jj::{self, JjRuntimeAvailability};
use gitcomet_core::process::background_command;
use gitcomet_core::services::{
    BlameLine, CommandOutput, GitRepository, PullMode, RepoCapabilities, Result,
    SubmoduleTrustDecision,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Minimum spacing between working-copy snapshot attempts. A snapshot is a
/// `jj st` process spawn, and the refresh pipelines that reach
/// [`JjRepository::status`] can fire in bursts (watcher flushes are debounced
/// to 250ms/2s; activation is throttled to 5s upstream), so each burst pays at
/// most one spawn. Between attempts the user's jj history may lag the working
/// copy by at most this interval plus the upstream debounce.
const JJ_SNAPSHOT_MIN_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct JjRepository {
    inner: GixRepo,
    last_snapshot: Mutex<Option<Instant>>,
    identity_config_args: OnceLock<Vec<String>>,
}

impl JjRepository {
    pub(crate) fn new(inner: GixRepo) -> Self {
        Self {
            inner,
            last_snapshot: Mutex::new(None),
            identity_config_args: OnceLock::new(),
        }
    }

    /// The gix-backed reader every read delegates to.
    #[allow(dead_code)] // status-mapping hook in a later P1 task
    pub(crate) fn gix(&self) -> &GixRepo {
        &self.inner
    }

    /// A `jj` command pre-configured with `--repository <workdir>`, the
    /// background-console treatment, and the bridged identity config (see
    /// [`Self::identity_config_args`]). Every jj invocation snapshots the
    /// working copy and imports git refs first — that is the point of routing
    /// writes through it.
    pub(super) fn jj_cmd(&self) -> Command {
        let mut cmd = jj_workdir_cmd_for(&self.inner.spec().workdir);
        for config in self.identity_config_args() {
            cmd.arg("--config").arg(config);
        }
        cmd
    }

    /// Identity `--config` args bridged from git config, computed once.
    ///
    /// jj 0.44 resolves the commit identity from its own config only — it
    /// does not read gitconfig — and an unconfigured identity silently
    /// poisons routed writes: `jj describe` rewrites the change with an empty
    /// committer, and `jj git push` later refuses to publish it ("no author
    /// and/or committer set"). Bridging the values jj has not configured
    /// keeps GitComet's git-config identity working in jj mode; an explicit
    /// jj identity always wins. Best-effort throughout: without a resolvable
    /// git identity the args are empty and jj's own warnings stand.
    fn identity_config_args(&self) -> &[String] {
        self.identity_config_args
            .get_or_init(|| self.compute_identity_config_args())
    }

    fn compute_identity_config_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let jj_name = self.jj_config_get("user.name");
        let jj_email = self.jj_config_get("user.email");
        if jj_name.is_empty() || jj_email.is_empty() {
            if let Some((name, email)) = self.git_config_identity() {
                if jj_name.is_empty() {
                    args.push(format!("user.name={name}"));
                }
                if jj_email.is_empty() {
                    args.push(format!("user.email={email}"));
                }
            }
        }
        args
    }

    /// `jj config get <key>` as a best-effort probe: the empty string when
    /// the key is unset or when jj cannot run at all.
    fn jj_config_get(&self, key: &str) -> String {
        let mut cmd = jj_workdir_cmd_for(&self.inner.spec().workdir);
        cmd.arg("config").arg("get").arg(key);
        cmd.output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_default()
    }

    /// The gix-resolved git identity (repo-local + global gitconfig), when
    /// both halves exist.
    fn git_config_identity(&self) -> Option<(String, String)> {
        let repo = self.inner.reopen_repo().ok()?;
        let config = repo.config_snapshot();
        let value = |key: &str| {
            config
                .string(key)
                .map(|value| bytes_to_text_preserving_utf8(&value))
                .filter(|value| !value.is_empty())
        };
        Some((value("user.name")?, value("user.email")?))
    }

    /// Delete a bookmark locally; shared by the plain and force trait
    /// methods, which are the same operation under jj semantics.
    fn delete_bookmark(&self, name: &str) -> Result<()> {
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark").arg("delete").arg(name);
        run_jj_command_with_output(cmd, "jj bookmark delete")?;
        Ok(())
    }

    /// Track same-named remote bookmarks before a fetch or push.
    ///
    /// Colocated repos cloned with plain `git` (rather than `jj git clone`)
    /// import their `origin/*` refs as *untracked* remote bookmarks, and jj
    /// refuses to touch those: `jj git push` answers "Nothing changed" with a
    /// successful exit status, and fetches never advance the local bookmark.
    /// Tracking the pairs where a local branch shares a remote bookmark's name
    /// restores git-shaped behavior. `jj bookmark track` is idempotent —
    /// already-tracked pairs only log a warning and still exit 0.
    fn track_same_named_remote_bookmarks(&self) -> Result<()> {
        let local_names: FxHashSet<String> = self
            .inner
            .list_branches()?
            .into_iter()
            .map(|branch| branch.name)
            .collect();
        let pairs: Vec<String> = self
            .inner
            .list_remote_branches()?
            .into_iter()
            .filter(|remote| local_names.contains(&remote.name))
            .map(|remote| format!("{}@{}", remote.name, remote.remote))
            .collect();
        if pairs.is_empty() {
            return Ok(());
        }
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark").arg("track");
        for pair in &pairs {
            cmd.arg(pair);
        }
        run_jj_command_with_output(cmd, "jj bookmark track")?;
        Ok(())
    }

    /// Absorb working-copy edits into `@` if a snapshot is due.
    ///
    /// Hooked into [`Self::status`] only: every trigger — the watcher's
    /// worktree flushes, the `git_state` full refresh, and activation (which
    /// reduces to that same refresh) — funnels a status load through here, and
    /// inlining the snapshot is the only way to order it against the read
    /// (sibling effects execute on a thread pool in parallel). Between
    /// snapshots the git view stays correct regardless (see the module docs),
    /// so a skipped snapshot costs jj-side freshness, never read correctness.
    ///
    /// The throttle lock is held across the spawn on purpose: concurrent
    /// reads then block and skip instead of racing a second snapshot. Failures
    /// are traced and swallowed — the read still answers from the previous
    /// snapshot and the next due read retries.
    fn snapshot_if_due(&self, reason: &'static str) {
        let mut last_snapshot = self
            .last_snapshot
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if !claim_snapshot_slot(&mut last_snapshot, Instant::now(), JJ_SNAPSHOT_MIN_INTERVAL) {
            return;
        }
        if let Some(JjRuntimeAvailability::Unavailable { detail }) =
            jj::current_jj_runtime().map(|state| state.availability)
        {
            jj::trace(format_args!(
                "snapshot skipped after {reason}: jj unavailable ({detail})"
            ));
            return;
        }
        let started = Instant::now();
        let mut cmd = self.jj_cmd();
        cmd.arg("st");
        let outcome = run_jj_command_with_output(cmd, "jj st");
        jj::trace(format_args!(
            "snapshot after {reason}: {} in {:?}",
            match &outcome {
                Ok(_) => "ok".to_string(),
                Err(err) => err.to_string(),
            },
            started.elapsed()
        ));
    }
}

/// Whether a snapshot attempt is due given the last one.
fn snapshot_due(last: Option<Instant>, now: Instant, min_interval: Duration) -> bool {
    last.is_none_or(|last| now.saturating_duration_since(last) >= min_interval)
}

/// Gate + record in one step: claims the slot (throttling concurrent and
/// subsequent attempts) exactly when a snapshot is due. Split out from the
/// runner so the throttle logic is testable without spawning jj.
fn claim_snapshot_slot(last: &mut Option<Instant>, now: Instant, min_interval: Duration) -> bool {
    if !snapshot_due(*last, now, min_interval) {
        return false;
    }
    *last = Some(now);
    true
}

/// Build a `jj --repository <workdir> …` background command.
pub(crate) fn jj_workdir_cmd_for(workdir: &Path) -> Command {
    let mut cmd = background_command("jj");
    cmd.arg("--repository").arg(workdir);
    cmd
}

/// Run a prepared jj command and capture its output.
///
/// Mirrors `util::run_git_with_output` minus the git-auth plumbing: jj owns
/// its own credentials, so there is no askpass hook to install. A non-zero
/// exit becomes a `Backend` error carrying the first non-empty stream, which
/// the command log and error banners surface verbatim.
pub(crate) fn run_jj_command_with_output(mut cmd: Command, label: &str) -> Result<CommandOutput> {
    let output = cmd
        .output()
        .map_err(|err| Error::new(ErrorKind::Backend(format!("spawn `{label}`: {err}"))))?;
    if !output.status.success() {
        return Err(jj_command_failed_error(label, output));
    }
    Ok(CommandOutput {
        command: label.to_string(),
        stdout: bytes_to_text_preserving_utf8(&output.stdout),
        stderr: bytes_to_text_preserving_utf8(&output.stderr),
        exit_code: output.status.code(),
    })
}

fn jj_command_failed_error(label: &str, output: Output) -> Error {
    let detail = [output.stderr.as_slice(), output.stdout.as_slice()]
        .into_iter()
        .map(bytes_to_text_preserving_utf8)
        .map(|text| text.trim().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_else(|| format!("exited with {}", output.status));
    Error::new(ErrorKind::Backend(format!("`{label}` failed: {detail}")))
}

/// `ErrorKind::Unsupported` carries a `&'static str`, so the per-operation
/// message is stitched at compile time instead of formatted.
macro_rules! jj_write_unsupported {
    ($operation:literal) => {
        Error::new(ErrorKind::Unsupported(concat!(
            $operation,
            " is not available in a colocated Jujutsu repository yet; \
             use the jj CLI for this operation"
        )))
    };
}

impl GitRepository for JjRepository {
    fn capabilities(&self) -> RepoCapabilities {
        // The inner gix repo owns the `.jj` detection; this adapter routes
        // commit (describe + new), bookmark writes (create/delete/rename),
        // and network operations (fetch/pull/push) through the jj CLI, so
        // all three re-enable on top of the detected (read-only) set.
        // `read_only` stays true — the reducer's write gate admits exactly
        // these messages, keyed on `commits`/`branches`/`network`.
        RepoCapabilities {
            commits: true,
            branches: true,
            network: true,
            ..self.inner.capabilities()
        }
    }

    fn spec(&self) -> &RepoSpec {
        self.inner.spec()
    }

    fn log_head_page(&self, limit: usize, cursor: Option<&LogCursor>) -> Result<LogPage> {
        self.inner.log_head_page(limit, cursor)
    }

    fn commit_details(&self, id: &CommitId) -> Result<CommitDetails> {
        self.inner.commit_details(id)
    }

    fn reflog_head(&self, limit: usize) -> Result<Vec<ReflogEntry>> {
        self.inner.reflog_head(limit)
    }

    fn current_branch(&self) -> Result<String> {
        self.inner.current_branch()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        self.inner.list_branches()
    }

    fn list_remotes(&self) -> Result<Vec<Remote>> {
        self.inner.list_remotes()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        self.inner.list_remote_branches()
    }

    /// jj status semantics, provided by delegation (see module docs):
    ///
    /// * staged lane — always empty: HEAD and the index both sit at `@`'s
    ///   parent and jj has no staging area, so `diff(HEAD, index)` is empty.
    /// * unstaged lane — the jj working-copy change, `diff(@^, worktree)`:
    ///   the index keeps `@`'s parent tree while gix diffs it against the
    ///   worktree directly, so it is fresh even between snapshots (snapshot
    ///   state only affects what the user's *other* jj tooling sees).
    ///
    /// The snapshot beforehand is for jj-side freshness only, never read
    /// correctness — which is why a throttled or failed snapshot is safe to
    /// swallow.
    fn status(&self) -> Result<RepoStatus> {
        self.snapshot_if_due("status read");
        self.inner.status()
    }

    fn diff_unified(&self, target: &DiffTarget) -> Result<String> {
        self.inner.diff_unified(target)
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        self.inner.stash_list()
    }

    // Reads whose trait defaults are empty/Unsupported are overridden here so
    // browsing fidelity on a colocated repo matches a plain git repo: tags,
    // worktrees, submodules, blame, file history, upstream divergence, author
    // emails, and the all-branches/filtered log walks all stay live through
    // gix. Cancellable variants inherit these through their default
    // delegations in the trait.

    fn log_all_branches_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        self.inner.log_all_branches_page(_limit, _cursor)
    }

    fn log_file_page(
        &self,
        _path: &Path,
        _limit: usize,
        _cursor: Option<&LogCursor>,
    ) -> Result<LogPage> {
        self.inner.log_file_page(_path, _limit, _cursor)
    }

    fn diff_range_files(
        &self,
        _from: &CommitId,
        _to: Option<&CommitId>,
    ) -> Result<Vec<CommitFileChange>> {
        self.inner.diff_range_files(_from, _to)
    }

    fn topologically_order_commits(&self, ids: &[CommitId]) -> Result<Vec<CommitId>> {
        self.inner.topologically_order_commits(ids)
    }

    fn recent_commit_messages(&self, limit: usize) -> Result<Vec<RecentCommitMessage>> {
        self.inner.recent_commit_messages(limit)
    }

    fn head_commit_id(&self) -> Result<Option<CommitId>> {
        self.inner.head_commit_id()
    }

    fn list_tags(&self) -> Result<Vec<Tag>> {
        self.inner.list_tags()
    }

    fn list_remote_tags(&self) -> Result<Vec<RemoteTag>> {
        self.inner.list_remote_tags()
    }

    fn staged_diff_unified(&self) -> Result<String> {
        self.inner.staged_diff_unified()
    }

    fn diff_file_text(&self, target: &DiffTarget) -> Result<Option<FileDiffText>> {
        self.inner.diff_file_text(target)
    }

    fn diff_preview_text_file(
        &self,
        target: &DiffTarget,
        side: DiffPreviewTextSide,
    ) -> Result<Option<PathBuf>> {
        self.inner.diff_preview_text_file(target, side)
    }

    fn diff_file_image(&self, target: &DiffTarget) -> Result<Option<FileDiffImage>> {
        self.inner.diff_file_image(target)
    }

    fn blame_file(&self, path: &Path, rev: Option<&str>) -> Result<Vec<BlameLine>> {
        self.inner.blame_file(path, rev)
    }

    fn blame_worktree_file(&self, path: &Path, area: DiffArea) -> Result<Vec<BlameLine>> {
        self.inner.blame_worktree_file(path, area)
    }

    fn list_worktrees(&self) -> Result<Vec<Worktree>> {
        self.inner.list_worktrees()
    }

    fn list_ref_metadata(&self) -> Result<Vec<(String, RefMetadata)>> {
        self.inner.list_ref_metadata()
    }

    fn list_submodules(&self) -> Result<Vec<Submodule>> {
        self.inner.list_submodules()
    }

    fn list_worktree_files(&self) -> Result<Vec<FileEntry>> {
        self.inner.list_worktree_files()
    }

    fn list_tree_files_at_commit(&self, commit_id: &CommitId) -> Result<Vec<FileEntry>> {
        self.inner.list_tree_files_at_commit(commit_id)
    }

    fn submodule_diff_summary(&self, target: &DiffTarget) -> Result<SubmoduleDiffSummary> {
        self.inner.submodule_diff_summary(target)
    }

    fn upstream_divergence(&self) -> Result<Option<UpstreamDivergence>> {
        self.inner.upstream_divergence()
    }

    fn author_email_map(&self) -> Result<FxHashMap<String, String>> {
        self.inner.author_email_map()
    }

    fn resolve_file_path_at_commit(
        &self,
        path: &Path,
        commit: &CommitId,
    ) -> Result<Option<PathBuf>> {
        self.inner.resolve_file_path_at_commit(path, commit)
    }

    fn check_submodule_add_trust(&self, url: &str, path: &Path) -> Result<SubmoduleTrustDecision> {
        self.inner.check_submodule_add_trust(url, path)
    }

    fn check_submodule_update_trust(&self) -> Result<SubmoduleTrustDecision> {
        self.inner.check_submodule_update_trust()
    }

    fn check_submodule_load_trust(&self, path: &Path) -> Result<SubmoduleTrustDecision> {
        self.inner.check_submodule_load_trust(path)
    }

    fn squash_message_preview(&self, oldest: &CommitId, head: &CommitId) -> Result<String> {
        self.inner.squash_message_preview(oldest, head)
    }

    // Required write methods: every one stays Unsupported until it is routed
    // through the jj CLI. The optional write methods in the trait default to
    // Unsupported or funnel into these, so nothing else is needed here.

    /// Bookmark writes route through `jj bookmark …`. Each invocation
    /// snapshots + exports, so `refs/heads/<name>` is updated on the git
    /// side by the same command and gix reads (the branch panel refresh)
    /// see the new state immediately — no extra sync needed.
    ///
    /// Divergence presentation degrades, not breaks: jj never writes
    /// `branch.<name>.remote` tracking config and git HEAD is usually
    /// detached, so `upstream_divergence` reads `None` and the panel simply
    /// shows no ahead/behind chip. A jj-native bookmark panel (P3) restores
    /// it from jj's own tracked-remote model.
    fn create_branch(&self, name: &str, target: &CommitId) -> Result<()> {
        validate_ref_like_arg(name, "bookmark name")?;
        validate_hex_commit_id(target)?;
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark")
            .arg("create")
            .arg(name)
            .arg("-r")
            .arg(target.as_ref());
        run_jj_command_with_output(cmd, "jj bookmark create")?;
        Ok(())
    }

    fn rename_branch(&self, old_name: &str, new_name: &str) -> Result<()> {
        validate_ref_like_arg(old_name, "bookmark name")?;
        validate_ref_like_arg(new_name, "bookmark name")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark")
            .arg("rename")
            .arg(old_name)
            .arg(new_name);
        run_jj_command_with_output(cmd, "jj bookmark rename")?;
        Ok(())
    }

    fn delete_branch(&self, name: &str) -> Result<()> {
        validate_ref_like_arg(name, "bookmark name")?;
        self.delete_bookmark(name)
    }

    /// jj bookmarks carry no "merged-only" protection, so force and plain
    /// delete are the same operation.
    fn delete_branch_force(&self, name: &str) -> Result<()> {
        validate_ref_like_arg(name, "bookmark name")?;
        self.delete_bookmark(name)
    }

    fn checkout_branch(&self, _name: &str) -> Result<()> {
        Err(jj_write_unsupported!("checkout_branch"))
    }

    fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
        Err(jj_write_unsupported!("checkout_commit"))
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        Err(jj_write_unsupported!("cherry_pick"))
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        Err(jj_write_unsupported!("revert"))
    }

    fn stash_create(&self, _message: &str, _include_untracked: bool) -> Result<()> {
        Err(jj_write_unsupported!("stash_create"))
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        Err(jj_write_unsupported!("stash_apply"))
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        Err(jj_write_unsupported!("stash_drop"))
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        Err(jj_write_unsupported!("stage"))
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        Err(jj_write_unsupported!("unstage"))
    }

    /// jj commit = describe the working-copy commit, then open a fresh one.
    ///
    /// `jj describe` snapshots the working copy first (every jj command
    /// does), so the message lands on a change that contains the current
    /// edits — no pre-write sync needed for the same reason. `jj new` then
    /// leaves `@` empty and moves git HEAD to the described commit
    /// (detached), which the watcher → `RepoExternallyChanged` pipeline
    /// refreshes like any external change. The staged-lane semantics do not
    /// apply: jj commits the whole working-copy change.
    fn commit(&self, message: &str) -> Result<()> {
        let mut describe = self.jj_cmd();
        describe.arg("describe").arg("-m").arg(message);
        run_jj_command_with_output(describe, "jj describe")?;
        let mut new = self.jj_cmd();
        new.arg("new");
        run_jj_command_with_output(new, "jj new")?;
        Ok(())
    }

    /// GitComet's amend maps to describing the working-copy commit: in jj
    /// the working copy IS the change under construction, so setting its
    /// message is the whole operation. No `jj new` — the change stays open
    /// for further edits, and git HEAD does not move.
    fn commit_amend(&self, message: &str) -> Result<()> {
        let mut describe = self.jj_cmd();
        describe.arg("describe").arg("-m").arg(message);
        run_jj_command_with_output(describe, "jj describe")?;
        Ok(())
    }

    /// Network operations route through `jj git …`. jj owns the transport
    /// and credentials, so git's askpass hooks do not apply — auth failures
    /// surface as `Backend` errors rather than in-app prompts. Fetching all
    /// remotes mirrors `git fetch --all`; jj prunes gone remote bookmarks
    /// as part of its fetch model, so a separate prune flag is inherent.
    /// Tracking is established first so the local bookmarks actually follow
    /// the fetched tips (see [`Self::track_same_named_remote_bookmarks`]).
    fn fetch_all_with_output(&self) -> Result<CommandOutput> {
        self.track_same_named_remote_bookmarks()?;
        let mut cmd = self.jj_cmd();
        cmd.arg("git").arg("fetch").arg("--all-remotes");
        run_jj_command_with_output(cmd, "jj git fetch --all-remotes")
    }

    fn fetch_all(&self) -> Result<()> {
        self.fetch_all_with_output().map(|_| ())
    }

    /// jj has no pull: fetching IS pulling. New commits simply appear, and
    /// jj rebases local descendants (and `@`) automatically — every
    /// [`PullMode`] maps to the same fetch and no merge commits are ever
    /// produced.
    fn pull_with_output(&self, _mode: PullMode) -> Result<CommandOutput> {
        self.fetch_all_with_output()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        self.fetch_all()
    }

    /// Pull a single remote bookmark: `jj git fetch --remote <r> --branch <b>`.
    fn pull_branch_with_output(&self, remote: &str, branch: &str) -> Result<CommandOutput> {
        validate_ref_like_arg(remote, "remote name")?;
        validate_ref_like_arg(branch, "bookmark name")?;
        self.track_same_named_remote_bookmarks()?;
        let mut cmd = self.jj_cmd();
        cmd.arg("git")
            .arg("fetch")
            .arg("--remote")
            .arg(remote)
            .arg("--branch")
            .arg(branch);
        run_jj_command_with_output(
            cmd,
            &format!("jj git fetch --remote {remote} --branch {branch}"),
        )
    }

    /// Push uses jj's own default: tracking bookmarks that moved ahead of
    /// their remote, guarded by jj's built-in force-with-lease-style safety
    /// checks. New, untracked bookmarks are not published by this operation
    /// (matching `git push`, which also does not create remote branches).
    /// Tracking is established first — without it a bookmark whose remote
    /// counterpart jj never tracked (plain-`git`-cloned colocated repo) would
    /// make the push a silent "Nothing changed" no-op. Force and lease
    /// variants stay `Unsupported` — jj's safety model has no equivalent to
    /// route them to yet.
    fn push_with_output(&self) -> Result<CommandOutput> {
        self.track_same_named_remote_bookmarks()?;
        let mut cmd = self.jj_cmd();
        cmd.arg("git").arg("push");
        run_jj_command_with_output(cmd, "jj git push")
    }

    fn push(&self) -> Result<()> {
        self.push_with_output().map(|_| ())
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        Err(jj_write_unsupported!("discard_worktree_changes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_jj_command_with_output_captures_success_output() {
        // `git` stands in for `jj`: the runner only cares about exit status
        // and streams, and git is a hard test dependency already.
        let mut cmd = background_command("git");
        cmd.arg("--version");
        let output =
            run_jj_command_with_output(cmd, "git --version").expect("git --version should succeed");
        assert_eq!(output.command, "git --version");
        assert!(output.stdout.contains("git version"), "{output:?}");
        assert_eq!(output.exit_code, Some(0));
    }

    #[test]
    fn run_jj_command_with_output_maps_nonzero_exit_to_backend_error() {
        let mut cmd = background_command("git");
        cmd.arg("not-a-real-subcommand");
        let err = match run_jj_command_with_output(cmd, "git not-a-real-subcommand") {
            Ok(_) => panic!("expected failure"),
            Err(err) => err,
        };
        assert!(matches!(err.kind(), ErrorKind::Backend(_)), "{err:?}");
        let text = err.to_string();
        assert!(text.contains("not-a-real-subcommand"), "{text}");
    }

    #[test]
    fn run_jj_command_with_output_maps_spawn_failure_to_backend_error() {
        let cmd = background_command("gitcomet-jj-test-missing-binary");
        let err = match run_jj_command_with_output(cmd, "gitcomet-jj-test-missing-binary") {
            Ok(_) => panic!("expected spawn failure"),
            Err(err) => err,
        };
        assert!(matches!(err.kind(), ErrorKind::Backend(_)), "{err:?}");
    }

    #[test]
    fn jj_write_unsupported_mentions_the_operation() {
        let err = jj_write_unsupported!("commit");
        assert!(matches!(err.kind(), ErrorKind::Unsupported(_)));
        assert!(err.to_string().contains("commit"), "{err}");
    }

    #[test]
    fn jj_workdir_cmd_targets_the_repository() {
        let cmd = jj_workdir_cmd_for(Path::new("/tmp/somewhere"));
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec!["--repository".to_string(), "/tmp/somewhere".to_string()]
        );
    }

    #[test]
    fn snapshot_due_gates_by_min_interval() {
        let interval = Duration::from_secs(5);
        let now = Instant::now();

        // The first attempt is always due.
        assert!(snapshot_due(None, now, interval));
        // A recent snapshot blocks, one from before the interval reopens.
        assert!(!snapshot_due(
            Some(now),
            now + Duration::from_secs(4),
            interval
        ));
        assert!(snapshot_due(
            Some(now),
            now + Duration::from_secs(5),
            interval
        ));
        // A clock at or before the last attempt (saturating, never panics).
        assert!(!snapshot_due(
            Some(now),
            now - Duration::from_secs(1),
            interval
        ));
    }

    #[test]
    fn claim_snapshot_slot_records_only_when_due() {
        let interval = Duration::from_secs(5);
        let t0 = Instant::now();
        let mut last = None;

        assert!(claim_snapshot_slot(&mut last, t0, interval));
        assert_eq!(last, Some(t0), "a claimed slot records its timestamp");

        // Within the interval the claim is refused and the slot is untouched.
        let t1 = t0 + Duration::from_secs(1);
        assert!(!claim_snapshot_slot(&mut last, t1, interval));
        assert_eq!(last, Some(t0));

        // Once the interval elapsed the claim succeeds and moves the slot.
        let t2 = t0 + interval;
        assert!(claim_snapshot_slot(&mut last, t2, interval));
        assert_eq!(last, Some(t2));
    }
}
