//! On-disk, ref-fingerprint-keyed history cache.
//!
//! Mirrors the approach rgitui uses for instant repo reopens: the first history
//! page is serialized to disk keyed by a fingerprint of the repository's refs.
//! When the refs have not changed since the cache was written, the page is
//! rehydrated from disk instead of re-walking the commit graph — every object
//! read is skipped. The in-process [`super::log::GixRepo`] LRU cache already
//! covers repeats *within* a session; this layer covers the cold start after a
//! process restart, which is what "open fast" is about.
//!
//! The page is stored as a dedicated serializable projection ([`CachedLogPage`])
//! rather than `worktree_core::domain::LogPage` directly, so no `serde` derives
//! are added to the shared domain types. The page is reconstructed on a hit.

use rustc_hash::FxHasher;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gix::bstr::ByteSlice as _;
use gix::Repository;
use worktree_core::domain::{
    Commit, CommitId, CommitParentIds, HistoryMode, LogCursor, LogPage,
};

use crate::util::unix_seconds_to_system_time_or_epoch;
use super::history::gix_head_id_or_none;

const SCHEMA_VERSION: u32 = 1;
/// Keep at most this many cached generations per repository on disk.
const MAX_GENERATIONS_PER_REPO: usize = 4;

// ---------------------------------------------------------------------------
// Serialized projection
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct CachedCursor {
    last_seen: String,
    resume_from: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedCommit {
    id: String,
    parent_ids: Vec<String>,
    summary: String,
    author: String,
    time: i64,
    signed: bool,
}

#[derive(Serialize, Deserialize)]
struct CachedLogPage {
    schema_version: u32,
    ref_fingerprint: u64,
    mode: u8,
    limit: u32,
    author: Option<String>,
    /// `last_seen` oid of the request cursor, or empty for the first page.
    cursor: String,
    commits: Vec<CachedCommit>,
    next_cursor: Option<CachedCursor>,
}

// ---------------------------------------------------------------------------
// Fingerprinting
// ---------------------------------------------------------------------------

/// A stable hash of the repository's refs. Any commit, branch switch, or tag
/// change alters the fingerprint, which is exactly when a cached page is stale.
fn ref_fingerprint(repo: &Repository) -> u64 {
    let mut hasher = FxHasher::default();

    // HEAD: its resolved commit id, and whether it is detached. A branch rename
    // at the same commit must still count, so the symbolic name matters too.
    if let Ok(Some(head_id)) = gix_head_id_or_none(repo) {
        head_id.to_string().hash(&mut hasher);
    }
    if let Ok(head) = repo.head() {
        head.is_detached().hash(&mut hasher);
        head.name().as_bstr().as_bytes().hash(&mut hasher);
    }

    // Every reference under refs/, as (name bytes, target). Sorting makes the
    // hash order-independent; the target is what actually changes when history
    // moves forward, so it is the dominant invalidation signal.
    if let Ok(refs) = repo.references() {
        if let Ok(iter) = refs.all() {
            let mut entries: Vec<(Vec<u8>, Option<String>)> = Vec::new();
            for reference in iter {
                let Ok(reference) = reference else {
                    continue;
                };
                let name = reference.name().as_bstr().as_bytes().to_vec();
                let target = reference.target().try_id().map(|oid| oid.to_string());
                entries.push((name, target));
            }
            entries.sort();
            entries.hash(&mut hasher);
        }
    }

    hasher.finish()
}

/// Stable hash of the workdir path, used as the per-repo namespace on disk.
fn repo_hash(repo_path: &Path) -> u64 {
    let canonical = repo_path
        .canonicalize()
        .unwrap_or_else(|_| repo_path.to_path_buf());
    let mut hasher = FxHasher::default();
    canonical.hash(&mut hasher);
    hasher.finish()
}

fn cache_dir() -> PathBuf {
    std::env::temp_dir().join("gitcomet-history")
}

fn cache_path(repo_path: &Path, fingerprint: u64, request_hash: u64) -> PathBuf {
    cache_dir().join(format!(
        "v{}-r{:016x}-f{:016x}-{:016x}.json",
        SCHEMA_VERSION,
        repo_hash(repo_path),
        fingerprint,
        request_hash,
    ))
}

/// Hash of the request itself (everything that selects *which* page this is).
fn request_hash(
    mode_idx: u8,
    limit: usize,
    author: Option<&str>,
    cursor_oid: Option<&str>,
) -> u64 {
    let mut hasher = FxHasher::default();
    mode_idx.hash(&mut hasher);
    limit.hash(&mut hasher);
    author.hash(&mut hasher);
    cursor_oid.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn mode_index(mode: &HistoryMode) -> u8 {
    match mode {
        HistoryMode::FullReachable => 0,
        HistoryMode::FirstParent => 1,
        HistoryMode::NoMerges => 2,
        HistoryMode::MergesOnly => 3,
        HistoryMode::AllBranches => 4,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a cached history page for the given request, if a fresh one exists.
///
/// Returns `None` when there is no cache, the cache is stale (refs changed),
/// the schema version differs, or deserialization fails for any reason.
pub(super) fn load_log_page(
    repo: &Repository,
    repo_path: &Path,
    mode_idx: u8,
    limit: usize,
    author: Option<&str>,
    cursor_oid: Option<&str>,
) -> Option<LogPage> {
    let fingerprint = ref_fingerprint(repo);
    let path = cache_path(repo_path, fingerprint, request_hash(mode_idx, limit, author, cursor_oid));

    let bytes = std::fs::read(&path).ok()?;
    let parsed: CachedLogPage = serde_json::from_slice(&bytes).ok()?;

    // Stale / mismatched cache: ignore and let the caller re-walk.
    if parsed.schema_version != SCHEMA_VERSION
        || parsed.ref_fingerprint != fingerprint
        || parsed.mode != mode_idx
        || parsed.limit as usize != limit
        || parsed.author.as_deref() != author
        || parsed.cursor != cursor_oid.unwrap_or("")
    {
        return None;
    }

    Some(reconstruct(&parsed))
}

/// Persist a freshly produced history page to disk, keyed by the current refs.
/// Failures are non-fatal: a missed cache only costs the next cold open.
pub(super) fn store_log_page(
    repo: &Repository,
    repo_path: &Path,
    page: &LogPage,
    mode_idx: u8,
    limit: usize,
    author: Option<&str>,
    cursor_oid: Option<&str>,
) {
    let fingerprint = ref_fingerprint(repo);
    let path = cache_path(repo_path, fingerprint, request_hash(mode_idx, limit, author, cursor_oid));

    let cached = CachedLogPage {
        schema_version: SCHEMA_VERSION,
        ref_fingerprint: fingerprint,
        mode: mode_idx,
        limit: limit as u32,
        author: author.map(str::to_owned),
        cursor: cursor_oid.unwrap_or("").to_owned(),
        commits: page.commits.iter().map(project_commit).collect(),
        next_cursor: page.next_cursor.as_ref().map(project_cursor),
    };

    let bytes = match serde_json::to_vec(&cached) {
        Ok(b) => b,
        Err(_) => return,
    };

    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }

    let tmp = dir.join(format!(
        "tmp-{}-{:016x}-{:016x}.json",
        std::process::id(),
        fingerprint,
        request_hash(mode_idx, limit, author, cursor_oid),
    ));
    if std::fs::write(&tmp, &bytes).is_err() {
        return;
    }
    // Atomic publish; a partial write from a crashed process is left as a stray
    // temp file and simply ignored on the next read.
    let _ = std::fs::rename(&tmp, &path);
    prune_old_generations(repo_path, fingerprint);
}

// ---------------------------------------------------------------------------
// Projection <-> domain
// ---------------------------------------------------------------------------

fn project_commit(c: &Commit) -> CachedCommit {
    CachedCommit {
        id: c.id.0.to_string(),
        parent_ids: c.parent_ids.iter().map(|p| p.0.to_string()).collect(),
        summary: c.summary.to_string(),
        author: c.author.to_string(),
        time: c
            .time
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        signed: c.signed,
    }
}

fn project_cursor(c: &LogCursor) -> CachedCursor {
    CachedCursor {
        last_seen: c.last_seen.0.to_string(),
        resume_from: c.resume_from.as_ref().map(|r| r.0.to_string()),
    }
}

fn reconstruct(cached: &CachedLogPage) -> LogPage {
    let commits = cached
        .commits
        .iter()
        .map(|c| Commit {
            id: CommitId(Arc::from(c.id.as_str())),
            parent_ids: CommitParentIds::from(
                c.parent_ids
                    .iter()
                    .map(|p| CommitId(Arc::from(p.as_str())))
                    .collect::<Vec<_>>(),
            ),
            summary: Arc::from(c.summary.as_str()),
            author: Arc::from(c.author.as_str()),
            time: unix_seconds_to_system_time_or_epoch(c.time),
            signed: c.signed,
        })
        .collect();

    let next_cursor = cached.next_cursor.as_ref().map(|c| LogCursor {
        last_seen: CommitId(Arc::from(c.last_seen.as_str())),
        resume_from: c.resume_from.as_ref().map(|r| CommitId(Arc::from(r.as_str()))),
        // The gix walk state behind a resume token is gone after a restart; fall
        // back to `last_seen` semantics, which the consumer also supports.
        resume_token: None,
    });

    LogPage {
        commits,
        next_cursor,
    }
}

// ---------------------------------------------------------------------------
// Pruning
// ---------------------------------------------------------------------------

/// Remove all but the newest [`MAX_GENERATIONS_PER_REPO`] cache files for this
/// repository, keyed by repo hash. Old generations are simply wasted disk once
/// refs have moved on, so we do not let them accumulate.
fn prune_old_generations(repo_path: &Path, _fingerprint: u64) {
    let dir = cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let prefix = format!("v{}-r{:016x}-", SCHEMA_VERSION, repo_hash(repo_path));
    let mut paths: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".json"))
        })
        .filter_map(|p| std::fs::metadata(&p).ok().map(|m| (m.modified().unwrap_or(UNIX_EPOCH), p)))
        .collect();
    if paths.len() <= MAX_GENERATIONS_PER_REPO {
        return;
    }
    paths.sort_by_key(|(t, _)| *t);
    for (_, path) in paths.drain(..paths.len() - MAX_GENERATIONS_PER_REPO) {
        let _ = std::fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn sample_page() -> LogPage {
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        LogPage {
            commits: vec![
                Commit {
                    id: CommitId(Arc::from(
                        "1111111111111111111111111111111111111111",
                    )),
                    parent_ids: CommitParentIds::from(vec![CommitId(Arc::from(
                        "2222222222222222222222222222222222222222",
                    ))]),
                    summary: Arc::from("initial commit"),
                    author: Arc::from("Alice"),
                    time: t,
                    signed: false,
                },
                Commit {
                    id: CommitId(Arc::from(
                        "3333333333333333333333333333333333333333",
                    )),
                    parent_ids: CommitParentIds::from(vec![]),
                    summary: Arc::from("second"),
                    author: Arc::from("Bob"),
                    time: t + Duration::from_secs(60),
                    signed: true,
                },
            ],
            next_cursor: Some(LogCursor {
                last_seen: CommitId(Arc::from(
                    "3333333333333333333333333333333333333333",
                )),
                resume_from: Some(CommitId(Arc::from(
                    "2222222222222222222222222222222222222222",
                ))),
                // Intentionally dropped on rebuild; the consumer falls back to
                // `last_seen` semantics. Asserted below.
                resume_token: Some(Arc::from("opaque-token")),
            }),
        }
    }

    /// The page survives a full serialize -> deserialize -> reconstruct cycle,
    /// which is exactly what the on-disk cache does on a cold open.
    #[test]
    fn page_round_trips_through_cached_projection() {
        let page = sample_page();
        let cached = CachedLogPage {
            schema_version: SCHEMA_VERSION,
            ref_fingerprint: 0xabcdef,
            mode: 1,
            limit: 50,
            author: Some("alice".to_string()),
            cursor: "4444444444444444444444444444444444444444".to_string(),
            commits: page.commits.iter().map(project_commit).collect(),
            next_cursor: page.next_cursor.as_ref().map(project_cursor),
        };

        let json = serde_json::to_string(&cached).expect("serialize");
        let parsed: CachedLogPage = serde_json::from_str(&json).expect("deserialize");
        let rebuilt = reconstruct(&parsed);

        // Commits are bit-for-bit equivalent (ids, parents, summary, author,
        // time, signed).
        assert_eq!(rebuilt.commits, page.commits);
        // Cursor identity survives; only the opaque resume token is intentionally
        // discarded.
        let rebuilt_cursor = rebuilt.next_cursor.expect("next_cursor");
        let original_cursor = page.next_cursor.expect("next_cursor");
        assert_eq!(rebuilt_cursor.last_seen, original_cursor.last_seen);
        assert_eq!(rebuilt_cursor.resume_from, original_cursor.resume_from);
        assert!(rebuilt_cursor.resume_token.is_none());
    }

    /// Encoding then decoding a second time yields the same page again, so the
    /// on-disk form is stable across repeated cold opens.
    #[test]
    fn cached_projection_is_idempotent() {
        let page = sample_page();
        let cached = CachedLogPage {
            schema_version: SCHEMA_VERSION,
            ref_fingerprint: 1,
            mode: 0,
            limit: 10,
            author: None,
            cursor: String::new(),
            commits: page.commits.iter().map(project_commit).collect(),
            next_cursor: page.next_cursor.as_ref().map(project_cursor),
        };
        let once = serde_json::to_string(&cached).unwrap();
        let parsed: CachedLogPage = serde_json::from_str(&once).unwrap();
        let twice = serde_json::to_string(&parsed).unwrap();
        assert_eq!(once, twice);
    }
}
