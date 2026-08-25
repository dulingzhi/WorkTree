//! The jj CLI implementation of [`crate::JjRepository`].
//!
//! Every operation shells out to `jj --repository <workdir> …` — the same
//! invocation layer the P1 colocated adapter validated against jj 0.44 —
//! and parses strict `-T` templates (see [`crate::parse`]). This crate
//! covers the jj-semantic surface: revsets, changes, bookmarks, the
//! operation log, snapshots, conflicts, and the change-detail reads
//! (`jj diff --summary` / `jj diff --git`) the native panels render.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use gitcomet_core::domain::RepoSpec;
use gitcomet_core::error::{Error, ErrorKind};
use gitcomet_core::jj;
use gitcomet_core::process::background_command;
use gitcomet_core::services::{CommandOutput, Result};

use crate::JjRepository;
use crate::domain::{
    ChangeId, JjBookmark, JjChange, JjConflict, JjFileStat, JjLogPage, JjLogQuery, JjOp,
};
use crate::parse;
use crate::version::{self, JjVersionSupport, MAX_TESTED_JJ_VERSION, MIN_SUPPORTED_JJ_VERSION};

/// One log row: fields joined by `\x1f`, the record closed by `\x1e`.
/// Flag cells are single letters so a template change cannot silently
/// shift meaning; the parser rejects anything else. Validated against
/// jj 0.44 (which supports `\x1f`/`\x1e` escapes but not `\u{..}`).
const LOG_TEMPLATE: &str = r#"change_id.short() ++ "\x1f" ++ commit_id.short() ++ "\x1f" ++ if(divergent, "D", "N") ++ "\x1f" ++ bookmarks.join(",") ++ "\x1f" ++ if(current_working_copy, "at", "no") ++ "\x1f" ++ author.name() ++ "\x1f" ++ author.email() ++ "\x1f" ++ committer.timestamp().format("%s") ++ "\x1f" ++ if(conflict, "C", "N") ++ "\x1f" ++ description ++ "\x1e""#;

/// `jj bookmark list` rows. `--all` is required for remote refs to appear;
/// `remote` is the empty string for the local half of a pair. There is no
/// `target`/`commit_id` keyword — `normal_target.commit_id().short()` is
/// the validated spelling, and the `Option<Commit>` auto-unwraps on the
/// method call.
const BOOKMARK_TEMPLATE: &str = r#"name ++ "\x1f" ++ remote ++ "\x1f" ++ normal_target.commit_id().short() ++ "\x1f" ++ if(conflict, "C", "N") ++ "\x1e""#;

/// `jj op log` rows.
const OP_TEMPLATE: &str = r#"id.short() ++ "\x1f" ++ description ++ "\x1f" ++ user ++ "\x1f" ++ time.start().format("%s") ++ "\x1e""#;

/// A `JjRepository` backed by the `jj` CLI.
pub struct JjCliRepository {
    spec: RepoSpec,
    version: (u64, u64, u64),
    identity_config_args: OnceLock<Vec<String>>,
}

impl JjCliRepository {
    /// Open a repository for `workdir`.
    ///
    /// Probes `jj --version` against the whitelist: a jj older than
    /// [`MIN_SUPPORTED_JJ_VERSION`] is refused here — its template language
    /// may not parse the format strings above — while a jj at or above
    /// [`MAX_TESTED_JJ_VERSION`] is allowed through with a trace note.
    /// Whether `workdir` is a jj repo at all is the caller's determination
    /// (the state layer's `.jj` detection), not this probe's.
    pub fn open(workdir: &Path) -> Result<Self> {
        if !workdir.is_dir() {
            return Err(Error::new(ErrorKind::Io(std::io::ErrorKind::NotADirectory)));
        }
        let (support, version) = version::probe_jj_version_support()
            .map_err(|detail| Error::new(ErrorKind::Backend(detail)))?
            .ok_or_else(|| {
                Error::new(ErrorKind::Backend(
                    "unrecognized `jj --version` output".to_string(),
                ))
            })?;
        match support {
            JjVersionSupport::Supported => {}
            JjVersionSupport::Untested => jj::trace(format_args!(
                "jj {version:?} is newer than the newest tested version \
                 {MAX_TESTED_JJ_VERSION:?}; proceeding"
            )),
            JjVersionSupport::Unsupported => {
                return Err(Error::new(ErrorKind::Backend(format!(
                    "jj {version:?} is older than the minimum supported \
                     {MIN_SUPPORTED_JJ_VERSION:?}"
                ))));
            }
        }
        Ok(Self {
            spec: RepoSpec {
                workdir: PathBuf::from(workdir),
            },
            version,
            identity_config_args: OnceLock::new(),
        })
    }

    /// The jj version this handle was opened against.
    pub fn jj_version(&self) -> (u64, u64, u64) {
        self.version
    }

    /// A `jj` command with `--repository <workdir>`, the working directory
    /// pinned to the workdir, and the bridged identity config. Every
    /// invocation snapshots the working copy and imports git refs first —
    /// that is inherent to running jj. Pinning the cwd matters beyond
    /// tidiness: jj prints diff paths and parses fileset patterns relative
    /// to the *process* cwd (not `--repository`), so running from anywhere
    /// else would turn `jj diff --summary` paths cwd-relative and make
    /// repo-relative `-- <path>` arguments unresolvable.
    fn jj_cmd(&self) -> Command {
        let mut cmd = background_command("jj");
        cmd.arg("--repository").arg(&self.spec.workdir);
        cmd.current_dir(&self.spec.workdir);
        for config in self.identity_config_args() {
            cmd.arg("--config").arg(config);
        }
        cmd
    }

    /// Identity `--config` args bridged from git config, computed once.
    ///
    /// jj does not read gitconfig, and an unconfigured identity silently
    /// poisons writes: `jj describe` rewrites the change with an empty
    /// committer and `jj git push` later refuses to publish it. Bridging
    /// the halves jj has not configured keeps GitComet's git-config
    /// identity working; an explicit jj identity always wins. Best-effort:
    /// without any resolvable identity the args are empty and jj's own
    /// warnings stand. (Ported from the P1 adapter, where this was
    /// validated against jj 0.44.)
    fn identity_config_args(&self) -> &[String] {
        self.identity_config_args
            .get_or_init(|| self.compute_identity_config_args())
    }

    fn compute_identity_config_args(&self) -> Vec<String> {
        let jj_name = self.jj_config_get("user.name");
        let jj_email = self.jj_config_get("user.email");
        let mut args = Vec::new();
        if (jj_name.is_empty() || jj_email.is_empty())
            && let Some((name, email)) = self.git_config_identity()
        {
            if jj_name.is_empty() {
                args.push(format!("user.name={name}"));
            }
            if jj_email.is_empty() {
                args.push(format!("user.email={email}"));
            }
        }
        args
    }

    /// `jj config get <key>` as a best-effort probe: empty when the key is
    /// unset or jj cannot run.
    fn jj_config_get(&self, key: &str) -> String {
        let mut cmd = background_command("jj");
        cmd.arg("--repository")
            .arg(&self.spec.workdir)
            .arg("config")
            .arg("get")
            .arg(key);
        cmd.output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_default()
    }

    /// The git-config identity (repo-local + global), when both halves
    /// exist. Read through the `git` CLI because this crate deliberately
    /// carries no gix dependency.
    fn git_config_identity(&self) -> Option<(String, String)> {
        let get = |key: &str| -> Option<String> {
            let mut cmd = background_command("git");
            cmd.arg("-C").arg(&self.spec.workdir).arg("config").arg(key);
            let output = cmd.output().ok()?;
            if !output.status.success() {
                return None;
            }
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!value.is_empty()).then_some(value)
        };
        Some((get("user.name")?, get("user.email")?))
    }

    /// Run a prepared jj command and capture its output. A non-zero exit
    /// becomes a `Backend` error carrying the first non-empty stream.
    fn run(&self, mut cmd: Command, label: &str) -> Result<CommandOutput> {
        let output = cmd
            .output()
            .map_err(|err| Error::new(ErrorKind::Backend(format!("spawn `{label}`: {err}"))))?;
        if !output.status.success() {
            let detail = [output.stderr.as_slice(), output.stdout.as_slice()]
                .into_iter()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .find(|text| !text.is_empty())
                .unwrap_or_else(|| format!("exited with {}", output.status));
            return Err(Error::new(ErrorKind::Backend(format!(
                "`{label}` failed: {detail}"
            ))));
        }
        Ok(CommandOutput {
            command: label.to_string(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        })
    }

    /// Track local/remote bookmark pairs that share a name before a fetch
    /// or push. Colocated repos cloned with plain `git` import their
    /// `origin/*` refs as untracked remote bookmarks, and jj refuses to
    /// touch those: `jj git push` exits 0 having done nothing. Tracking
    /// restores git-shaped behavior; the command is idempotent.
    fn track_same_named_remote_bookmarks(&self) -> Result<()> {
        let bookmarks = self.bookmarks()?;
        let local_names: HashSet<&str> = bookmarks
            .iter()
            .filter(|bookmark| bookmark.is_local())
            .map(|bookmark| bookmark.name.as_str())
            .collect();
        let pairs: Vec<String> = bookmarks
            .iter()
            .filter(|bookmark| match &bookmark.remote {
                Some(remote) => local_names.contains(bookmark.name.as_str()) && !remote.is_empty(),
                None => false,
            })
            .map(|bookmark| {
                format!(
                    "{}@{}",
                    bookmark.name,
                    bookmark.remote.as_deref().unwrap_or_default()
                )
            })
            .collect();
        if pairs.is_empty() {
            return Ok(());
        }
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark").arg("track");
        for pair in &pairs {
            cmd.arg(pair);
        }
        self.run(cmd, "jj bookmark track")?;
        Ok(())
    }
}

/// Reject arguments that could be mistaken for jj options or smuggle
/// control characters into the command line. Change ids arrive from jj's
/// own output and names from the UI, so this is a guardrail, not a parser.
fn safe_arg<'a>(value: &'a str, what: &str) -> Result<&'a str> {
    if value.is_empty() || value.starts_with('-') || value.contains(|ch: char| ch.is_control()) {
        return Err(Error::new(ErrorKind::Backend(format!(
            "invalid {what} {value:?}"
        ))));
    }
    Ok(value)
}

/// The path-shaped guard: paths come from jj's own `--summary` output and
/// travel inside a quoted fileset expression, so a leading `-` is harmless
/// and only emptiness and control characters are refused.
fn safe_path<'a>(value: &'a str) -> Result<&'a str> {
    if value.is_empty() || value.contains(|ch: char| ch.is_control()) {
        return Err(Error::new(ErrorKind::Backend(format!(
            "invalid file path {value:?}"
        ))));
    }
    Ok(value)
}

/// The base revset for a log query, defaulting an empty one to `all()`.
fn log_revset(base: &str) -> String {
    let base = base.trim();
    if base.is_empty() {
        "all()".to_string()
    } else {
        base.to_string()
    }
}

/// Slice a fetched prefix of the revset into the requested page.
///
/// Paging is positional on purpose: the log's display order is
/// newest-first, and the newest changes are *descendants* of the cursor
/// row, so an ancestry-exclusion revset like `~::<last>` would re-show
/// them on the next page (verified against jj 0.44). Each page therefore
/// refetches `skip + limit` rows from the top and slices locally —
/// correct in any topology, at the cost of re-reading the pages already
/// shown. `next_cursor` is set only when jj hit the fetch limit, i.e.
/// more rows may exist.
fn page_from_fetched(fetched: Vec<JjChange>, skip: usize, limit: usize) -> JjLogPage {
    let requested = skip.saturating_add(limit);
    let hit_limit = fetched.len() >= requested;
    let mut changes = fetched;
    if changes.len() > skip {
        changes.drain(..skip);
    } else {
        changes.clear();
    }
    changes.truncate(limit);
    let next_cursor = hit_limit.then_some(skip + changes.len());
    JjLogPage {
        changes,
        next_cursor,
    }
}

impl crate::JjRepository for JjCliRepository {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }

    /// `jj st`. Not throttled here: every jj command snapshots anyway, so
    /// this exists to buy jj-side freshness at moments the store chooses —
    /// the store owns the pacing policy (the P1 adapter throttled at 5s;
    /// #75 decides the equivalent for the jj store).
    fn snapshot(&self) -> Result<()> {
        let mut cmd = self.jj_cmd();
        cmd.arg("st");
        self.run(cmd, "jj st")?;
        Ok(())
    }

    fn log(&self, query: &JjLogQuery) -> Result<JjLogPage> {
        if query.limit == 0 {
            return Ok(JjLogPage {
                changes: Vec::new(),
                next_cursor: None,
            });
        }
        let revset = log_revset(&query.revset);
        let requested = query.skip.saturating_add(query.limit);
        let mut cmd = self.jj_cmd();
        cmd.arg("log")
            .arg("--no-graph")
            .arg("--limit")
            .arg(requested.to_string())
            .arg("-r")
            .arg(&revset)
            .arg("-T")
            .arg(LOG_TEMPLATE);
        let output = self.run(cmd, "jj log")?;
        let fetched = parse::parse_log_records(&output.stdout)?;
        Ok(page_from_fetched(fetched, query.skip, query.limit))
    }

    fn change_files(&self, change: &ChangeId) -> Result<Vec<JjFileStat>> {
        safe_arg(&change.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("diff").arg("-r").arg(&change.0).arg("--summary");
        let output = self.run(cmd, "jj diff --summary")?;
        parse::parse_diff_summary(&output.stdout)
    }

    fn file_diff_text(&self, change: &ChangeId, path: &str) -> Result<String> {
        safe_arg(&change.0, "change id")?;
        safe_path(path)?;
        // The `root:"…"` fileset form pins the path to repo-relative
        // (verified against jj 0.44): a bare word would be split on spaces,
        // and quoting escapes the two characters a path can inject into
        // the expression.
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        let pattern = format!("root:\"{escaped}\"");
        let mut cmd = self.jj_cmd();
        cmd.arg("diff")
            .arg("-r")
            .arg(&change.0)
            .arg("--git")
            .arg(&pattern);
        let output = self.run(cmd, "jj diff --git")?;
        Ok(output.stdout)
    }

    fn describe(&self, change: &ChangeId, message: &str) -> Result<()> {
        safe_arg(&change.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("describe")
            .arg("-r")
            .arg(&change.0)
            .arg("-m")
            .arg(message);
        self.run(cmd, "jj describe")?;
        Ok(())
    }

    fn new_change(&self, message: Option<&str>) -> Result<JjChange> {
        let mut cmd = self.jj_cmd();
        cmd.arg("new");
        if let Some(message) = message.filter(|message| !message.trim().is_empty()) {
            cmd.arg("-m").arg(message);
        }
        self.run(cmd, "jj new")?;
        self.working_copy()
    }

    fn new_change_at(&self, change: &ChangeId) -> Result<JjChange> {
        safe_arg(&change.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("new").arg(&change.0);
        self.run(cmd, "jj new")?;
        self.working_copy()
    }

    fn abandon(&self, change: &ChangeId) -> Result<()> {
        safe_arg(&change.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("abandon").arg(&change.0);
        self.run(cmd, "jj abandon")?;
        Ok(())
    }

    fn squash(&self, from: &ChangeId, into: Option<&ChangeId>) -> Result<()> {
        safe_arg(&from.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("squash").arg("--from").arg(&from.0);
        if let Some(into) = into {
            safe_arg(&into.0, "change id")?;
            cmd.arg("--into").arg(&into.0);
        }
        self.run(cmd, "jj squash")?;
        Ok(())
    }

    fn split(&self, _change: &ChangeId) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported(
            "jj split needs an interactive diff editor; run it in a terminal",
        )))
    }

    fn bookmarks(&self) -> Result<Vec<JjBookmark>> {
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark")
            .arg("list")
            .arg("--all")
            .arg("-T")
            .arg(BOOKMARK_TEMPLATE);
        let output = self.run(cmd, "jj bookmark list")?;
        parse::parse_bookmark_records(&output.stdout)
    }

    fn bookmark_create(&self, name: &str, target: &ChangeId) -> Result<()> {
        safe_arg(name, "bookmark name")?;
        safe_arg(&target.0, "change id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark")
            .arg("create")
            .arg(name)
            .arg("-r")
            .arg(&target.0);
        self.run(cmd, "jj bookmark create")?;
        Ok(())
    }

    fn bookmark_delete(&self, name: &str) -> Result<()> {
        safe_arg(name, "bookmark name")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark").arg("delete").arg(name);
        self.run(cmd, "jj bookmark delete")?;
        Ok(())
    }

    fn bookmark_rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        safe_arg(old_name, "bookmark name")?;
        safe_arg(new_name, "bookmark name")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark")
            .arg("rename")
            .arg(old_name)
            .arg(new_name);
        self.run(cmd, "jj bookmark rename")?;
        Ok(())
    }

    fn bookmark_track(&self, name: &str, remote: Option<&str>) -> Result<()> {
        safe_arg(name, "bookmark name")?;
        let mut pair = name.to_string();
        if let Some(remote) = remote {
            safe_arg(remote, "remote name")?;
            pair.push('@');
            pair.push_str(remote);
        }
        let mut cmd = self.jj_cmd();
        cmd.arg("bookmark").arg("track").arg(&pair);
        self.run(cmd, "jj bookmark track")?;
        Ok(())
    }

    fn op_log(&self, limit: usize) -> Result<Vec<JjOp>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut cmd = self.jj_cmd();
        cmd.arg("op")
            .arg("log")
            .arg("--no-graph")
            .arg("--limit")
            .arg(limit.to_string())
            .arg("-T")
            .arg(OP_TEMPLATE);
        let output = self.run(cmd, "jj op log")?;
        parse::parse_op_records(&output.stdout)
    }

    /// jj 0.44 has no `jj op undo`; reverting the newest operation is the
    /// equivalent gesture (`jj op revert <latest>` records a new op whose
    /// effects are the inverse of the latest one).
    fn op_undo(&self) -> Result<()> {
        let latest = self
            .op_log(1)?
            .pop()
            .ok_or_else(|| Error::new(ErrorKind::Backend("jj op log is empty".into())))?;
        safe_arg(&latest.op_id, "operation id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("op").arg("revert").arg(&latest.op_id);
        self.run(cmd, "jj op revert")?;
        Ok(())
    }

    fn op_restore(&self, op_id: &str) -> Result<()> {
        safe_arg(op_id, "operation id")?;
        let mut cmd = self.jj_cmd();
        cmd.arg("op").arg("restore").arg(op_id);
        self.run(cmd, "jj op restore")?;
        Ok(())
    }

    fn conflicts(&self) -> Result<Vec<JjConflict>> {
        let mut cmd = self.jj_cmd();
        cmd.arg("resolve").arg("--list");
        match self.run(cmd, "jj resolve --list") {
            Ok(output) => Ok(parse::parse_conflict_paths(&output.stdout)),
            Err(err) if parse::is_no_conflicts_message(&err.to_string()) => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }

    fn fetch_all_with_output(&self) -> Result<CommandOutput> {
        self.track_same_named_remote_bookmarks()?;
        let mut cmd = self.jj_cmd();
        cmd.arg("git").arg("fetch").arg("--all-remotes");
        self.run(cmd, "jj git fetch --all-remotes")
    }

    fn push_tracked_with_output(&self) -> Result<CommandOutput> {
        self.track_same_named_remote_bookmarks()?;
        let mut cmd = self.jj_cmd();
        cmd.arg("git").arg("push");
        self.run(cmd, "jj git push")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(suffix: &str) -> JjChange {
        JjChange {
            change_id: ChangeId(suffix.to_string()),
            commit_id: crate::domain::JjCommitId(format!("c{suffix}")),
            divergent: false,
            conflicted: false,
            is_working_copy: false,
            bookmarks: Vec::new(),
            author_name: String::new(),
            author_email: String::new(),
            committed_at_unix: 0,
            description: String::new(),
        }
    }

    fn change_ids(page: &JjLogPage) -> Vec<&str> {
        page.changes
            .iter()
            .map(|change| change.change_id.0.as_str())
            .collect()
    }

    #[test]
    fn log_revset_defaults_to_all() {
        assert_eq!(log_revset(""), "all()");
        assert_eq!(log_revset("   "), "all()");
        assert_eq!(log_revset("::@"), "::@");
        assert_eq!(log_revset("main | @"), "main | @");
    }

    #[test]
    fn paging_slices_the_first_page_without_a_cursor() {
        let fetched = vec![change("a"), change("b"), change("c")];
        let page = page_from_fetched(fetched, 0, 2);
        assert_eq!(change_ids(&page), vec!["a", "b"]);
        assert_eq!(page.next_cursor, Some(2));
    }

    #[test]
    fn paging_skips_already_shown_rows() {
        let fetched = vec![change("a"), change("b"), change("c"), change("d")];
        let page = page_from_fetched(fetched, 2, 2);
        assert_eq!(change_ids(&page), vec!["c", "d"]);
        assert_eq!(page.next_cursor, Some(4));
    }

    #[test]
    fn a_short_fetch_has_no_next_cursor() {
        let fetched = vec![change("a"), change("b")];
        let page = page_from_fetched(fetched, 0, 5);
        assert_eq!(change_ids(&page), vec!["a", "b"]);
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn skipping_past_the_end_yields_an_empty_page() {
        let fetched = vec![change("a"), change("b")];
        let page = page_from_fetched(fetched, 5, 2);
        assert!(page.changes.is_empty());
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn safe_arg_rejects_option_like_and_control_content() {
        assert!(safe_arg("wqnwyzpk", "change id").is_ok());
        assert!(safe_arg("main", "bookmark name").is_ok());
        assert!(safe_arg("", "change id").is_err());
        assert!(safe_arg("--exec", "change id").is_err());
        assert!(safe_arg("a\nb", "bookmark name").is_err());
        assert!(safe_arg("a\u{1f}b", "change id").is_err());
    }

    #[test]
    fn safe_path_allows_leading_dashes_but_refuses_controls() {
        assert!(safe_path("src/main.rs").is_ok());
        assert!(safe_path("-weird-but-real.txt").is_ok());
        assert!(safe_path("").is_err());
        assert!(safe_path("a\nb").is_err());
        assert!(safe_path("a\u{1f}b").is_err());
    }
}
