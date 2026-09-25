//! On-disk, ref-fingerprint-keyed history cache.
//!
//! Mirrors the approach rgitui uses for instant repo reopens: the first history
//! page is serialized to disk keyed by a fingerprint of the repository's refs.
//! When the refs have not changed since the cache was written, the page is
//! rehydrated from disk instead of re-walking the commit graph — every object
//! read is skipped.
//!
//! The fingerprint is *scope-aware*: `AllBranches` walks every ref so it is
//! keyed off all of them, while the head-page modes walk from HEAD alone and are
//! keyed off HEAD alone. Otherwise `git fetch` moving remotes would evict the
//! default first screen, which is the page this cache exists to serve. The in-process [`super::log::GixRepo`] LRU cache already
//! covers repeats *within* a session; this layer covers the cold start after a
//! process restart, which is what "open fast" is about.
//!
//! A cache entry is a *snapshot*: one contiguous run of commits for a
//! (mode, author) pair, not a single page. Any request inside that run — any
//! page size, any "load more" offset — is served by slicing it, so one entry
//! covers a whole stretch of navigation instead of one exact request.
//!
//! It is stored as a dedicated serializable projection ([`CachedSnapshot`])
//! rather than `worktree_core::domain::LogPage` directly, so no `serde` derives
//! are added to the shared domain types. Commits are reconstructed on a hit.

use rustc_hash::FxHasher;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gix::Repository;
use gix::bstr::ByteSlice as _;
use worktree_core::applog::{self, Level};
use worktree_core::domain::{
    Commit, CommitId, CommitParentIds, HistoryMode, LogCursor, LogPage, ReflogEntry,
};
use worktree_core::services::BlameLine;

use super::history::gix_head_id_or_none;
use super::oid_to_arc_str;
use crate::util::{unix_seconds_to_system_time, unix_seconds_to_system_time_or_epoch};

/// Bumped to 3 when the reflog domain landed: cache files are now addressed by
/// a request hash that also mixes in the cache domain, so a v2 file can no
/// longer collide with a v3 one. Foreign-schema files are swept, not read.
const SCHEMA_VERSION: u32 = 3;

/// Which projection a cache file holds. Every domain shares one directory and
/// one byte budget, so the request hash has to tell them apart — nothing else
/// in the file name does.
const DOMAIN_LOG: u8 = 0;
const DOMAIN_REFLOG: u8 = 1;
const DOMAIN_SEARCH: u8 = 2;
const DOMAIN_BLAME: u8 = 3;
/// How many commits one snapshot holds — the fetch window for a cold first page.
///
/// Walking this many instead of just `limit` costs marginally more once, and in
/// exchange every page size and every "load more" inside the window is served by
/// slicing that one file instead of re-walking.
pub(super) const SNAPSHOT_COMMITS: usize = 500;
/// The `AllBranches` window. That walk seeds from every ref, so each extra
/// commit costs visibly more than it does from HEAD alone — small enough to
/// keep the cold open honest, large enough to cover a couple of pages.
pub(super) const SNAPSHOT_COMMITS_ALL_BRANCHES: usize = 200;
/// Keep at most this many cached files per repository on disk.
///
/// Deliberately well above rgitui's single generation: the fingerprint includes
/// HEAD, so **switching branches yields a new fingerprint**, and keeping several
/// generations is what lets "switch back to a branch" hit the cache instead of
/// re-walking.
///
/// Raised from 16 to 64 when blame landed. 16 was set when every entry was a few
/// KB; a blame entry is a whole file's worth of lines, so 16 could not even hold
/// one screenful of blamed files. The real ceiling is [`MAX_BYTES_PER_REPO`],
/// which `sweep_cache` enforces — this cap only stops *stale* files piling up.
const MAX_GENERATIONS_PER_REPO: usize = 64;

// ---------------------------------------------------------------------------
// Disk guardrails
//
// The generation cap alone is not a budget: one entry is a few KB for the log
// domain but will be orders of magnitude larger once blame lands, and nothing
// previously removed a file whose refs never moved again. These three limits
// mirror the image-diff cache (`view/panes/main/diff_cache/image_cache.rs`) so
// every cache in the app answers to the same ceiling question.
// ---------------------------------------------------------------------------

/// Ceiling for the whole cache directory, every repository included.
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Ceiling for one repository. A single blame-sized entry must not be able to
/// evict every log snapshot in the app.
const MAX_BYTES_PER_REPO: u64 = 32 * 1024 * 1024;
/// Entries untouched for this long are dead weight: refs have moved on, or the
/// repository is gone from the session entirely.
const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(60 * 60 * 24 * 7);
/// The byte/TTL sweep reads the whole cache directory, so it runs every N-th
/// store rather than on every one. Bounded overshoot: N × largest entry.
const SWEEP_EVERY_STORES: u64 = 16;
/// Counts stores since the last sweep.
static STORES_SINCE_SWEEP: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Serialized projection
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct CachedCommit {
    id: String,
    parent_ids: Vec<String>,
    summary: String,
    author: String,
    /// Unix seconds; `SystemTime` is not serializable in the domain type.
    time: i64,
    signed: bool,
}

#[derive(Serialize, Deserialize)]
struct CachedSnapshot {
    schema_version: u32,
    /// [`DOMAIN_LOG`] — validated on load so a reflog file can never be
    /// rehydrated as a history snapshot.
    domain: u8,
    ref_fingerprint: u64,
    mode: u8,
    author: Option<String>,
    /// Whether the walk that produced this run had commits beyond the last one.
    /// Decides whether a request that runs off the end may still have more.
    has_more: bool,
    /// The contiguous run, oldest-request-first (i.e. starting at the walk's
    /// start point). Requests address it by offset, not by page identity.
    commits: Vec<CachedCommit>,
}

/// How many reflog entries one cached window holds, newest-first.
///
/// Reflogs are append-only and read from the top, so — like the log snapshot —
/// one file serves every page size inside the window.
pub(super) const REFLOG_WINDOW: usize = 200;

#[derive(Serialize, Deserialize)]
struct CachedReflog {
    schema_version: u32,
    domain: u8,
    ref_fingerprint: u64,
    /// Whether the read found more entries than the window holds. A window that
    /// ends inside a longer reflog cannot answer a deeper request.
    has_more: bool,
    entries: Vec<CachedReflogEntry>,
}

/// [`ReflogEntry`] minus everything positional: `index` and the `HEAD@{n}`
/// selector are just the offset in the window, so storing them would only give
/// them a second place to go wrong.
#[derive(Serialize, Deserialize)]
struct CachedReflogEntry {
    new_id: String,
    message: String,
    time: i64,
    author: String,
}

/// A completed `git log --all --grep/--author` run: the merged, deduplicated
/// matches for one query, newest-first.
///
/// Reuses [`CachedCommit`] — search returns the same `Commit` projection as the
/// history list, so the two domains share one serialization and one
/// reconstruction path.
#[derive(Serialize, Deserialize)]
struct CachedSearch {
    schema_version: u32,
    domain: u8,
    ref_fingerprint: u64,
    /// Not part of the lookup — the query is already in the file name's request
    /// hash. Kept so a stray file can be identified from its contents alone.
    query: String,
    /// Whether the run returned as many commits as it was allowed to, i.e. the
    /// reflog-equivalent of "there may be more".
    has_more: bool,
    commits: Vec<CachedCommit>,
}

/// A blamed file, stored as a **commit table plus per-line references**.
///
/// The domain `BlameLine` carries the blamed commit's author, time, subject and
/// body on *every line*. Serialized as-is, a 5,000-line file would repeat each
/// commit message 5,000 times. The table holds each distinct (commit, historical
/// path) once — which is also what `blame_commit_metadata` keys its own cache on,
/// since `prior_exists` depends on the historical path, not just the commit.
#[derive(Serialize, Deserialize)]
struct CachedBlame {
    schema_version: u32,
    domain: u8,
    /// Not part of the lookup; kept so a stray file can be identified from its
    /// contents alone.
    commit_id: String,
    /// Likewise: the blamed path, for the same reason.
    path: String,
    commits: Vec<CachedBlameCommit>,
    lines: Vec<CachedBlameLine>,
}

#[derive(Serialize, Deserialize)]
struct CachedBlameCommit {
    id: String,
    author: String,
    author_time_unix: Option<i64>,
    summary: String,
    body: Option<String>,
    prior_exists: bool,
    /// The file's path at this commit, when rename tracking attributed lines to
    /// it under a different name. Part of this entry's identity — the same
    /// commit can appear twice with different historical paths.
    source_path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedBlameLine {
    /// Index into [`CachedBlame::commits`].
    commit: u32,
    line: String,
    /// Only uncommitted ("Not Committed Yet") lines carry this: the revision the
    /// working-tree change is based on.
    prior_commit: Option<String>,
}

// ---------------------------------------------------------------------------
// Fingerprinting
// ---------------------------------------------------------------------------

/// Hash the parts of HEAD that can change a history page: its resolved commit
/// id, whether it is detached, and its symbolic name. A branch rename at the
/// same commit must still invalidate, so the name matters as well as the id.
fn hash_head(repo: &Repository, hasher: &mut FxHasher) {
    if let Ok(Some(head_id)) = gix_head_id_or_none(repo) {
        head_id.to_string().hash(hasher);
    }
    if let Ok(head) = repo.head() {
        head.is_detached().hash(hasher);
        head.name().as_bstr().as_bytes().hash(hasher);
    }
}

/// Fingerprint for the head-page modes (everything except `AllBranches`).
///
/// Those walks seed from HEAD alone — see `Arc::from(vec![head_id])` in
/// `log_history_mode_page_inner` — so only a HEAD change can alter the result.
/// Keying them off HEAD alone means ordinary ref churn (`git fetch` moving
/// remotes, creating a branch, tagging) no longer invalidates the default first
/// screen, which is the whole point of the cache.
pub(super) fn head_fingerprint(repo: &Repository) -> u64 {
    let mut hasher = FxHasher::default();
    hash_head(repo, &mut hasher);
    hasher.finish()
}

/// Finish an all-refs fingerprint from entries collected during some other
/// pass over the refs.
///
/// Exists so the `AllBranches` walk can derive its fingerprint from the very
/// same enumeration it uses to build walk tips. Enumerating loose refs is real
/// file I/O — on a repository with ~300 loose refs a single pass costs tens of
/// milliseconds, so paying for it two or three times per cold open dominated
/// the whole request.
///
/// The entries are sorted so the hash does not depend on enumeration order.
pub(super) fn all_refs_fingerprint_from_entries(
    repo: &Repository,
    entries: &mut Vec<(Vec<u8>, Option<String>)>,
) -> u64 {
    let mut hasher = FxHasher::default();
    hash_head(repo, &mut hasher);
    entries.sort();
    entries.hash(&mut hasher);
    hasher.finish()
}

/// Standalone all-refs fingerprint, for the probe harness's timing breakdown.
#[allow(dead_code)]
pub(super) fn all_refs_fingerprint(repo: &Repository) -> u64 {
    all_refs_fingerprint_inner(repo, true)
}

/// Fingerprint over every ref, optionally leaving tags out.
///
/// `AllBranches` history walks seed from branches and remotes, so a tag moving
/// cannot change their result. Commit search runs `git log --all`, which *does*
/// read tags — a commit reachable only through a tag is a real search hit — so
/// its fingerprint has to include them, or a tagged-only commit would be missing
/// from every later search for the same query.
fn all_refs_fingerprint_inner(repo: &Repository, skip_tags: bool) -> u64 {
    let mut entries: Vec<(Vec<u8>, Option<String>)> = Vec::new();
    // Two `let else` rather than a chained `and_then`: the iterator borrows the
    // ref store, so the two steps cannot be collapsed into one expression.
    let Ok(refs) = repo.references() else {
        return all_refs_fingerprint_from_entries(repo, &mut entries);
    };
    let Ok(iter) = refs.all() else {
        return all_refs_fingerprint_from_entries(repo, &mut entries);
    };
    for reference in iter.flatten() {
        if skip_tags
            && matches!(
                reference.name().category(),
                Some(gix::reference::Category::Tag)
            )
        {
            continue;
        }
        let name = reference.name().as_bstr().as_bytes().to_vec();
        let target = reference.target().try_id().map(|oid| oid.to_string());
        entries.push((name, target));
    }
    all_refs_fingerprint_from_entries(repo, &mut entries)
}

/// A stable hash of exactly the refs that can invalidate `mode_idx`'s page.
///
/// Scope-aware on purpose: `AllBranches` reads every ref, while the head-page
/// modes read only HEAD. Using one shared fingerprint would let unrelated ref
/// churn evict the default first screen.
fn ref_fingerprint(repo: &Repository, mode_idx: u8) -> u64 {
    if mode_idx == MODE_ALL_BRANCHES {
        all_refs_fingerprint(repo)
    } else {
        head_fingerprint(repo)
    }
}

/// Fingerprint for a commit-search result: every ref, tags included, because
/// `git log --all` reads tags too.
pub(super) fn search_fingerprint(repo: &Repository) -> u64 {
    all_refs_fingerprint_inner(repo, false)
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

/// Overrides the cache root for the whole process.
///
/// The default is the system temp directory, which on Linux may be a tmpfs that
/// the OS clears behind our back — the opposite of the local-first "the user can
/// see and clear this" contract. The app installs an `app_data_dir()`-rooted
/// path at startup; tests install a scratch directory.
static CACHE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn install_cache_root(root: PathBuf) {
    let _ = CACHE_ROOT.set(root);
}

/// Where the cache lives when nobody has installed anything: the system temp
/// directory, except in test builds, where it is a per-process scratch
/// directory so a test run never reads or deletes the real cache.
fn default_cache_root() -> PathBuf {
    #[cfg(test)]
    let dir = std::env::temp_dir().join(format!(
        "gitcomet-history-test-{}-{}",
        SCHEMA_VERSION,
        std::process::id()
    ));
    #[cfg(not(test))]
    let dir = std::env::temp_dir().join("gitcomet-history");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub(crate) fn cache_dir() -> PathBuf {
    // A test that sets a thread root must not see any other test's files: tests
    // run in parallel threads, and `CACHE_ROOT` is a process-wide `OnceLock`, so
    // a test that counts or clears the whole directory would otherwise race the
    // ones doing real reads and writes.
    #[cfg(test)]
    if let Some(dir) = TEST_ROOT.with(|slot| slot.borrow().clone()) {
        return dir;
    }
    CACHE_ROOT.get_or_init(default_cache_root).clone()
}

/// A cache root private to this thread, created on first call.
///
/// Returns the directory so a test can also write fixture files into it.
#[cfg(test)]
pub(crate) fn cache_test_root() -> PathBuf {
    TEST_ROOT.with(|slot| {
        if let Some(dir) = slot.borrow().as_ref() {
            return dir.clone();
        }
        let dir = std::env::temp_dir().join(format!(
            "gitcomet-history-test-{}-{}-{}",
            SCHEMA_VERSION,
            std::process::id(),
            TEST_ROOT_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::create_dir_all(&dir);
        *slot.borrow_mut() = Some(dir.clone());
        dir
    })
}

#[cfg(test)]
thread_local! {
    static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
static TEST_ROOT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Cumulative counters, in the shape `worktree_core` reports to the perf
/// sidecar.
pub(crate) fn cache_stats() -> worktree_core::history_cache::HistoryCacheStats {
    let (hits, cold, stale, corrupt, stores) = CACHE_STATS.snapshot();
    worktree_core::history_cache::HistoryCacheStats {
        hits,
        misses_cold: cold,
        misses_stale: stale,
        misses_corrupt: corrupt,
        stores,
    }
}

/// Every cache file under the current root, as `(bytes, entries)`.
///
/// Counts the current schema only — older schemas are somebody else's leftovers
/// and are claimed by `sweep_cache` on the next store.
fn cache_entries_now() -> Vec<(PathBuf, u64)> {
    let schema_prefix = format!("v{}-", SCHEMA_VERSION);
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_owned();
            if !name.starts_with(&schema_prefix) || !name.ends_with(".json") {
                return None;
            }
            let size = std::fs::metadata(&path).ok()?.len();
            Some((path, size))
        })
        .collect()
}

/// Total size and entry count of the cache — what the storage surface reports.
pub(crate) fn cache_usage() -> (u64, usize) {
    let entries = cache_entries_now();
    let bytes = entries.iter().map(|(_, size)| *size).sum();
    (bytes, entries.len())
}

/// Delete every cache entry, regardless of repository. Returns how many went.
///
/// This is the production counterpart of the test-only `clear_repo_cache`: the
/// settings page needs a "clear" affordance that works for the whole app, and
/// a cache the user cannot delete is not a cache the user controls.
pub(crate) fn clear_all() -> usize {
    let entries = cache_entries_now();
    let mut removed = 0usize;
    for (path, _) in entries {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
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

/// Hash of what a snapshot is scoped by: the mode and the author filter.
///
/// Deliberately *not* `limit` or the cursor — those say where to slice inside a
/// snapshot, not which snapshot to open. Dropping them is what lets one file
/// serve every page size and every "load more" offset.
fn request_hash(domain: u8, mode_idx: u8, author: Option<&str>) -> u64 {
    let mut hasher = FxHasher::default();
    domain.hash(&mut hasher);
    mode_idx.hash(&mut hasher);
    author.hash(&mut hasher);
    hasher.finish()
}

fn log_request_hash(mode_idx: u8, author: Option<&str>) -> u64 {
    request_hash(DOMAIN_LOG, mode_idx, author)
}

/// Reflog windows are scoped by nothing but the ref they belong to, so the
/// hash is the domain alone.
fn reflog_request_hash() -> u64 {
    request_hash(DOMAIN_REFLOG, 0, None)
}

/// `mode_index` of `HistoryMode::AllBranches` — the only mode whose walk seeds
/// from every ref rather than from HEAD alone, and therefore the only one whose
/// fingerprint has to cover all refs.
const MODE_ALL_BRANCHES: u8 = 4;

pub(super) fn mode_index(mode: &HistoryMode) -> u8 {
    match mode {
        HistoryMode::FullReachable => 0,
        HistoryMode::FirstParent => 1,
        HistoryMode::NoMerges => 2,
        HistoryMode::MergesOnly => 3,
        HistoryMode::AllBranches => MODE_ALL_BRANCHES,
    }
}

// ---------------------------------------------------------------------------
// Hit-rate instrumentation
// ---------------------------------------------------------------------------

/// Cumulative cache counters.
///
/// The question this answers in the field is "what fraction of history reads
/// were served from disk, and why did the rest miss?" — that is what tells us
/// whether a fingerprint is too broad (stale) or a view is never reused (cold).
/// Relaxed ordering is fine: these are diagnostics, never control flow.
#[derive(Debug, Default)]
pub(crate) struct CacheStats {
    /// A valid page was rehydrated from disk; no walk happened.
    pub hits: AtomicU64,
    /// No cache file existed (or could not be read) — the true cold path.
    pub misses_cold: AtomicU64,
    /// A file existed but its fingerprint or request parameters did not match.
    pub misses_stale: AtomicU64,
    /// A file existed but could not be deserialized.
    pub misses_corrupt: AtomicU64,
    /// Pages successfully published to disk.
    pub stores: AtomicU64,
}

/// Same counters for the reflog domain. Kept apart so a hit rate can be read
/// per domain: "the log cache works" must not be masked by "reflog never hits".
pub(crate) static REFLOG_CACHE_STATS: CacheStats = CacheStats {
    hits: AtomicU64::new(0),
    misses_cold: AtomicU64::new(0),
    misses_stale: AtomicU64::new(0),
    misses_corrupt: AtomicU64::new(0),
    stores: AtomicU64::new(0),
};

/// Same counters for the commit-search domain — see [`REFLOG_CACHE_STATS`].
pub(crate) static SEARCH_CACHE_STATS: CacheStats = CacheStats {
    hits: AtomicU64::new(0),
    misses_cold: AtomicU64::new(0),
    misses_stale: AtomicU64::new(0),
    misses_corrupt: AtomicU64::new(0),
    stores: AtomicU64::new(0),
};

/// Same counters for the blame domain — see [`REFLOG_CACHE_STATS`].
pub(crate) static BLAME_CACHE_STATS: CacheStats = CacheStats {
    hits: AtomicU64::new(0),
    misses_cold: AtomicU64::new(0),
    misses_stale: AtomicU64::new(0),
    misses_corrupt: AtomicU64::new(0),
    stores: AtomicU64::new(0),
};

pub(crate) static CACHE_STATS: CacheStats = CacheStats {
    hits: AtomicU64::new(0),
    misses_cold: AtomicU64::new(0),
    misses_stale: AtomicU64::new(0),
    misses_corrupt: AtomicU64::new(0),
    stores: AtomicU64::new(0),
};

// Deliberately not wired to a UI surface yet; `reset`/`snapshot` are for the
// probe harness today and app-level reporting later.
#[allow(dead_code)]
impl CacheStats {
    pub(crate) fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses_cold.store(0, Ordering::Relaxed);
        self.misses_stale.store(0, Ordering::Relaxed);
        self.misses_corrupt.store(0, Ordering::Relaxed);
        self.stores.store(0, Ordering::Relaxed);
    }

    /// `(hits, cold, stale, corrupt, stores)`
    pub(crate) fn snapshot(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses_cold.load(Ordering::Relaxed),
            self.misses_stale.load(Ordering::Relaxed),
            self.misses_corrupt.load(Ordering::Relaxed),
            self.stores.load(Ordering::Relaxed),
        )
    }

    /// Hits as a fraction of all reads; 0.0 when nothing has been read.
    pub(crate) fn hit_rate(&self) -> f64 {
        let (hits, cold, stale, corrupt, _) = self.snapshot();
        let reads = hits + cold + stale + corrupt;
        if reads == 0 {
            0.0
        } else {
            hits as f64 / reads as f64
        }
    }

    /// One-line diagnostic summary.
    pub(crate) fn summary(&self) -> String {
        let (hits, cold, stale, corrupt, stores) = self.snapshot();
        format!(
            "hits={hits} cold={cold} stale={stale} corrupt={corrupt} \
             stores={stores} hit_rate={:.1}%",
            self.hit_rate() * 100.0
        )
    }
}

/// Emit one record to the app log.
///
/// A no-op until `applog::init()` has opened today's file, so early startup and
/// tests are unaffected. Volume is bounded by the in-process LRU: the disk cache
/// is only consulted when that misses, so this fires once per distinct page
/// rather than once per redraw.
fn log_event(message: std::fmt::Arguments<'_>) {
    applog::log(Level::Info, "history_cache", message);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Serve a history page out of the cached snapshot, if it can.
///
/// The snapshot is a contiguous run of commits; this resolves the requested
/// cursor to an offset in that run and slices `limit` commits from it. Returns
/// `None` when there is no usable snapshot — stale refs, a different schema, a
/// cursor that is not inside the run, or a request that would run past the end
/// while more commits still exist (those genuinely need a walk).
pub(super) fn load_log_page(
    repo: &Repository,
    repo_path: &Path,
    mode_idx: u8,
    limit: usize,
    author: Option<&str>,
    cursor_oid: Option<&str>,
    // Pre-computed ref fingerprint, when the caller already enumerated refs
    // for its own purposes. `None` means "compute it here".
    //
    // Enumerating a repository with a few hundred loose refs costs tens of
    // milliseconds — measured on a 281-ref repo at ~33ms, and the cost is
    // filesystem syscalls rather than parsing, so it does not get cheaper by
    // going around gix. A caller that already has the refs in hand must
    // therefore be able to hand the fingerprint over instead of paying twice.
    fingerprint: Option<u64>,
) -> Option<LogPage> {
    let fingerprint = fingerprint.unwrap_or_else(|| ref_fingerprint(repo, mode_idx));
    let path = cache_path(repo_path, fingerprint, log_request_hash(mode_idx, author));

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            CACHE_STATS.misses_cold.fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "miss kind=cold mode={mode_idx} limit={limit} rate={:.0}%",
                CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };
    let parsed: CachedSnapshot = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            CACHE_STATS.misses_corrupt.fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "miss kind=corrupt mode={mode_idx} limit={limit} rate={:.0}%",
                CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };

    // Stale / mismatched cache: ignore and let the caller re-walk.
    if parsed.schema_version != SCHEMA_VERSION
        || parsed.domain != DOMAIN_LOG
        || parsed.ref_fingerprint != fingerprint
        || parsed.mode != mode_idx
        || parsed.author.as_deref() != author
    {
        CACHE_STATS.misses_stale.fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "miss kind=stale mode={mode_idx} limit={limit} rate={:.0}%",
            CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    let page = match slice_snapshot(&parsed, limit, cursor_oid) {
        Ok(page) => page,
        Err(SliceMiss::Outside) => {
            CACHE_STATS.misses_cold.fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "miss kind=outside mode={mode_idx} limit={limit} rate={:.0}%",
                CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
        Err(SliceMiss::Short { available }) => {
            CACHE_STATS.misses_cold.fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "miss kind=short mode={mode_idx} limit={limit} available={available} rate={:.0}%",
                CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };

    CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
    // Real LRU: without this the mtime only ever records the *write*, so
    // "oldest" degenerates into "written first" and a snapshot that is read on
    // every single startup is the first one evicted.
    touch_cache_file(&path);
    log_event(format_args!(
        "hit mode={mode_idx} limit={limit} served={} rate={:.0}%",
        page.commits.len(),
        CACHE_STATS.hit_rate() * 100.0
    ));
    Some(page)
}

/// Stamp a cache file as just-used so eviction order is genuinely LRU.
///
/// A read that fails to stamp is harmless — the entry simply ages out by write
/// time, which is the behaviour we had before.
fn touch_cache_file(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Retry a filesystem mutation on transient sharing-violation errors. On
/// Windows these surface as `PermissionDenied` when an antivirus scanner or
/// another git process briefly holds an exclusive lock on the cache file. A
/// missed cache write is non-fatal, but we ride out a short-lived lock instead
/// of silently dropping the entry.
fn retry_on_transient_fs(op: impl Fn() -> std::io::Result<()>) -> bool {
    const ATTEMPTS: usize = 4;
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        match op() {
            Ok(()) => return true,
            Err(err)
                if attempt + 1 < ATTEMPTS && err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                let backoff_ms = 25u64.saturating_mul(1 << attempt.min(3)).min(300);
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                last = Some(err);
            }
            Err(_) => return false,
        }
    }
    last.is_some()
}

/// Persist a freshly walked run of commits as the snapshot for this
/// (mode, author) pair, keyed by the current refs.
///
/// Only first-page walks are stored. A walk resumed from a cursor starts part
/// way through history, so its run is not addressable by offset from the start
/// and would only ever match that one exact request.
///
/// Failures are non-fatal: a missed cache only costs the next cold open.
pub(super) fn store_log_page(
    repo: &Repository,
    repo_path: &Path,
    page: &LogPage,
    mode_idx: u8,
    limit: usize,
    author: Option<&str>,
    cursor_oid: Option<&str>,
    // See [`load_log_page`]: pass the fingerprint when the caller has already
    // enumerated refs, so storing does not enumerate them again.
    fingerprint: Option<u64>,
) {
    if cursor_oid.is_some() {
        return;
    }

    let fingerprint = fingerprint.unwrap_or_else(|| ref_fingerprint(repo, mode_idx));
    let key = log_request_hash(mode_idx, author);
    let path = cache_path(repo_path, fingerprint, key);

    let cached = CachedSnapshot {
        schema_version: SCHEMA_VERSION,
        domain: DOMAIN_LOG,
        ref_fingerprint: fingerprint,
        mode: mode_idx,
        author: author.map(str::to_owned),
        has_more: page.next_cursor.is_some(),
        commits: page.commits.iter().map(project_commit).collect(),
    };

    let bytes = match serde_json::to_vec(&cached) {
        Ok(b) => b,
        Err(_) => return,
    };

    let dir = cache_dir();
    if !retry_on_transient_fs(|| std::fs::create_dir_all(&dir)) {
        return;
    }

    let tmp = dir.join(format!(
        "tmp-{}-{:016x}-{:016x}.json",
        std::process::id(),
        fingerprint,
        key,
    ));
    if !retry_on_transient_fs(|| std::fs::write(&tmp, &bytes)) {
        return;
    }
    // Atomic publish; a partial write from a crashed process is left as a stray
    // temp file and simply ignored on the next read.
    if retry_on_transient_fs(|| std::fs::rename(&tmp, &path)) {
        CACHE_STATS.stores.fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "store mode={mode_idx} limit={limit} commits={} bytes={} generations<={MAX_GENERATIONS_PER_REPO}",
            cached.commits.len(),
            bytes.len()
        ));
        maybe_sweep_cache();
    }
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

/// Why a snapshot cannot serve a request, so a walk is needed instead.
#[derive(Debug, PartialEq, Eq)]
enum SliceMiss {
    /// The cursor's last-seen commit is not in the run, so it cannot be located.
    Outside,
    /// The run ends before the page is full, but more commits do exist.
    Short { available: usize },
}

/// Slice one page out of a snapshot run.
///
/// Pure — no filesystem, no repository — so the mechanism that lets one file
/// serve every page size and every "load more" offset can be tested directly.
fn slice_snapshot(
    snapshot: &CachedSnapshot,
    limit: usize,
    cursor_oid: Option<&str>,
) -> Result<LogPage, SliceMiss> {
    let start = match cursor_oid {
        None => 0,
        Some(oid) => match snapshot.commits.iter().position(|c| c.id == oid) {
            Some(index) => index + 1,
            None => return Err(SliceMiss::Outside),
        },
    };

    let available = snapshot.commits.len().saturating_sub(start);
    // Not enough cached to fill the page while more commits do exist: slicing
    // here would silently truncate the page, so walk instead.
    if available < limit && snapshot.has_more {
        return Err(SliceMiss::Short { available });
    }

    let end = (start + limit).min(snapshot.commits.len());
    let commits: Vec<Commit> = snapshot.commits[start..end]
        .iter()
        .map(reconstruct_commit)
        .collect();

    // A next page exists whenever commits remain in the run, or the run was cut
    // short by the snapshot window while the walk still had more.
    let next_cursor = if end < snapshot.commits.len() || snapshot.has_more {
        snapshot.commits[..end].last().map(|last| LogCursor {
            last_seen: CommitId(Arc::from(last.id.as_str())),
            resume_from: snapshot
                .commits
                .get(end)
                .map(|next| CommitId(Arc::from(next.id.as_str()))),
            // The gix walk state behind a resume token is gone after a restart;
            // fall back to `last_seen`, which the consumer also supports.
            resume_token: None,
        })
    } else {
        None
    };

    Ok(LogPage {
        commits,
        next_cursor,
    })
}

fn reconstruct_commit(c: &CachedCommit) -> Commit {
    Commit {
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
    }
}

// ---------------------------------------------------------------------------
// Reflog domain
//
// Same mechanism as the log domain, one domain byte over: a window of the HEAD
// reflog written once and sliced per request. Reflogs are append-only and read
// from the top, so a window anchored at the newest entry serves every page size
// inside it.
// ---------------------------------------------------------------------------

/// The largest reflog window that will ever be written to disk.
///
/// A caller may ask for more than [`REFLOG_WINDOW`] (`usize::MAX` reads as "all
/// entries"), and it must get all of them — so the *read* is never capped. Only
/// the write is: one unbounded request must not be able to park a 100k-entry
/// reflog in the cache directory.
const MAX_REFLOG_WINDOW: usize = 2000;

/// Fingerprint for the HEAD reflog window.
///
/// HEAD's own id is not enough. `git stash` — and any `git reset` back to the
/// commit HEAD already points at — appends a reflog entry *without* moving
/// HEAD, so an id-only fingerprint would serve a reflog missing its newest
/// entry, and the reflog is what `classify_undo` reads. The reflog file's length
/// and mtime catch every append for the price of one `stat`.
///
/// A missing file (reflogs disabled, or a repo with no HEAD movement yet) leaves
/// the hash on HEAD alone, which is the best available answer.
fn reflog_fingerprint(repo: &Repository) -> u64 {
    let mut hasher = FxHasher::default();
    hash_head(repo, &mut hasher);
    if let Ok(metadata) = std::fs::metadata(repo.git_dir().join("logs").join("HEAD")) {
        metadata.len().hash(&mut hasher);
        if let Ok(modified) = metadata.modified() {
            modified
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Serve the newest `limit` HEAD reflog entries out of the cached window.
///
/// `None` means "walk instead": no window, a stale one, or a request that runs
/// past a window we know is not the whole reflog.
pub(super) fn load_reflog_window(
    repo: &Repository,
    repo_path: &Path,
    limit: usize,
) -> Option<Vec<ReflogEntry>> {
    let fingerprint = reflog_fingerprint(repo);
    let path = cache_path(repo_path, fingerprint, reflog_request_hash());

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            REFLOG_CACHE_STATS
                .misses_cold
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "reflog miss kind=cold limit={limit} rate={:.0}%",
                REFLOG_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };
    let parsed: CachedReflog = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            REFLOG_CACHE_STATS
                .misses_corrupt
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "reflog miss kind=corrupt limit={limit} rate={:.0}%",
                REFLOG_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };

    if parsed.schema_version != SCHEMA_VERSION
        || parsed.domain != DOMAIN_REFLOG
        || parsed.ref_fingerprint != fingerprint
    {
        REFLOG_CACHE_STATS
            .misses_stale
            .fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "reflog miss kind=stale limit={limit} rate={:.0}%",
            REFLOG_CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    // The window is the newest N entries; a deeper request needs the real thing.
    if limit > parsed.entries.len() && parsed.has_more {
        REFLOG_CACHE_STATS
            .misses_cold
            .fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "reflog miss kind=short limit={limit} available={} rate={:.0}%",
            parsed.entries.len(),
            REFLOG_CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    REFLOG_CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
    touch_cache_file(&path);
    log_event(format_args!(
        "reflog hit limit={limit} rate={:.0}%",
        REFLOG_CACHE_STATS.hit_rate() * 100.0
    ));
    Some(reconstruct_reflog(parsed.entries, limit))
}

/// Persist a freshly read reflog window, keyed by the current HEAD reflog.
///
/// `requested` is how many entries the read asked for — the same number the
/// caller truncated to, and the only way to know whether the window reached the
/// end of the reflog.
pub(super) fn store_reflog_window(
    repo: &Repository,
    repo_path: &Path,
    lines: &[gix::refs::log::Line],
    requested: usize,
) {
    if lines.is_empty() {
        return;
    }

    let fingerprint = reflog_fingerprint(repo);
    let key = reflog_request_hash();
    let path = cache_path(repo_path, fingerprint, key);

    let keep = lines.len().min(MAX_REFLOG_WINDOW);
    // Reached the read's cap, or the store cap — either way the reflog may
    // continue past what we are writing.
    let has_more = lines.len() > keep || lines.len() >= requested;
    let cached = CachedReflog {
        schema_version: SCHEMA_VERSION,
        domain: DOMAIN_REFLOG,
        ref_fingerprint: fingerprint,
        has_more,
        entries: lines[..keep].iter().map(project_reflog_line).collect(),
    };

    let bytes = match serde_json::to_vec(&cached) {
        Ok(b) => b,
        Err(_) => return,
    };

    let dir = cache_dir();
    if !retry_on_transient_fs(|| std::fs::create_dir_all(&dir)) {
        return;
    }

    let tmp = dir.join(format!(
        "tmp-{}-{:016x}-{:016x}.json",
        std::process::id(),
        fingerprint,
        key,
    ));
    if !retry_on_transient_fs(|| std::fs::write(&tmp, &bytes)) {
        return;
    }
    if retry_on_transient_fs(|| std::fs::rename(&tmp, &path)) {
        REFLOG_CACHE_STATS.stores.fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "reflog store entries={} bytes={} has_more={has_more}",
            cached.entries.len(),
            bytes.len()
        ));
        maybe_sweep_cache();
    }
    prune_old_generations(repo_path, fingerprint);
}

/// Rebuild domain entries from a stored window, newest first.
///
/// Pure — no filesystem, no repository — so the positional fields can be tested
/// without a repository: `index` and the `HEAD@{n}` selector are the offset in
/// the window, so they are derived here rather than stored.
fn reconstruct_reflog(entries: Vec<CachedReflogEntry>, limit: usize) -> Vec<ReflogEntry> {
    entries
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, entry)| ReflogEntry {
            index,
            new_id: CommitId(Arc::from(entry.new_id.as_str())),
            message: Arc::from(entry.message.as_str()),
            time: unix_seconds_to_system_time(entry.time),
            selector: Arc::from(format!("HEAD@{{{index}}}").as_str()),
            author: Arc::from(entry.author.as_str()),
        })
        .collect()
}

fn project_reflog_line(line: &gix::refs::log::Line) -> CachedReflogEntry {
    CachedReflogEntry {
        new_id: oid_to_arc_str(&line.new_oid).to_string(),
        message: String::from_utf8_lossy(line.message.as_ref()).into_owned(),
        time: line.signature.time.seconds,
        author: String::from_utf8_lossy(line.signature.name.as_ref()).into_owned(),
    }
}

// ---------------------------------------------------------------------------
// Blame domain
//
// The one domain that needs no ref fingerprint at all. A blame is keyed by the
// *resolved commit id*, and commit ids are content-addressed: the tree they
// point at, and every ancestor it was reached through, are immutable. So a blame
// cache cannot go stale — there is no "refs moved" case to detect, and no
// enumeration to pay for on lookup.
// ---------------------------------------------------------------------------

/// Skip writing a single blame larger than this. One entry is a whole file's
/// worth of lines, so an enormous generated file must not be able to evict
/// everything else the repository has cached — [`MAX_BYTES_PER_REPO`] is 32 MiB,
/// so this leaves room for several alongside the other domains.
const MAX_BLAME_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// The commit id plus whatever configurable options change a blame — see
/// `blame_options_hash` in `blame.rs`, which owns the config reading.
fn blame_fingerprint(commit_id: &str, options: u64) -> u64 {
    let mut hasher = FxHasher::default();
    commit_id.hash(&mut hasher);
    options.hash(&mut hasher);
    hasher.finish()
}

fn blame_request_hash(path: &Path) -> u64 {
    request_hash(DOMAIN_BLAME, 0, Some(&path.to_string_lossy()))
}

/// Serve a cached blame for `path` at `commit_id`, if there is one.
///
/// No `Repository`, no fingerprinting: the caller already resolved the commit.
pub(super) fn load_blame(
    repo_path: &Path,
    path: &Path,
    commit_id: &str,
    options: u64,
) -> Option<Vec<BlameLine>> {
    let fingerprint = blame_fingerprint(commit_id, options);
    let cache_path = cache_path(repo_path, fingerprint, blame_request_hash(path));

    let bytes = match std::fs::read(&cache_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            BLAME_CACHE_STATS
                .misses_cold
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "blame miss kind=cold rate={:.0}%",
                BLAME_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };
    let parsed: CachedBlame = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            BLAME_CACHE_STATS
                .misses_corrupt
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "blame miss kind=corrupt rate={:.0}%",
                BLAME_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };
    if parsed.schema_version != SCHEMA_VERSION || parsed.domain != DOMAIN_BLAME {
        BLAME_CACHE_STATS
            .misses_stale
            .fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "blame miss kind=stale rate={:.0}%",
            BLAME_CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    let lines = reconstruct_blame(&parsed);
    BLAME_CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
    touch_cache_file(&cache_path);
    log_event(format_args!(
        "blame hit lines={} rate={:.0}%",
        lines.len(),
        BLAME_CACHE_STATS.hit_rate() * 100.0
    ));
    Some(lines)
}

/// Persist a blame for `path` at `commit_id`.
pub(super) fn store_blame(
    repo_path: &Path,
    path: &Path,
    commit_id: &str,
    options: u64,
    lines: &[BlameLine],
) {
    if lines.is_empty() {
        return;
    }

    let fingerprint = blame_fingerprint(commit_id, options);
    let key = blame_request_hash(path);
    let cache_path = cache_path(repo_path, fingerprint, key);

    let cached = project_blame(commit_id, path, lines);
    let bytes = match serde_json::to_vec(&cached) {
        Ok(b) => b,
        Err(_) => return,
    };
    if bytes.len() as u64 > MAX_BLAME_ENTRY_BYTES {
        log_event(format_args!(
            "blame skip bytes={} over the {}-byte entry cap",
            bytes.len(),
            MAX_BLAME_ENTRY_BYTES
        ));
        return;
    }

    let dir = cache_dir();
    if !retry_on_transient_fs(|| std::fs::create_dir_all(&dir)) {
        return;
    }

    let tmp = dir.join(format!(
        "tmp-{}-{:016x}-{:016x}.json",
        std::process::id(),
        fingerprint,
        key,
    ));
    if !retry_on_transient_fs(|| std::fs::write(&tmp, &bytes)) {
        return;
    }
    if retry_on_transient_fs(|| std::fs::rename(&tmp, &cache_path)) {
        BLAME_CACHE_STATS.stores.fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "blame store lines={} commits={} bytes={}",
            cached.lines.len(),
            cached.commits.len(),
            bytes.len()
        ));
        maybe_sweep_cache();
    }
    prune_old_generations(repo_path, fingerprint);
}

fn project_blame(commit_id: &str, path: &Path, lines: &[BlameLine]) -> CachedBlame {
    let mut commits: Vec<CachedBlameCommit> = Vec::new();
    let mut index: std::collections::HashMap<(String, Option<String>), u32> =
        std::collections::HashMap::new();
    let mut cached_lines = Vec::with_capacity(lines.len());

    for line in lines {
        let source = line
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let identity = (line.commit_id.to_string(), source.clone());
        let slot = match index.get(&identity) {
            Some(slot) => *slot,
            None => {
                let slot = commits.len() as u32;
                commits.push(CachedBlameCommit {
                    id: line.commit_id.to_string(),
                    author: line.author.to_string(),
                    author_time_unix: line.author_time_unix,
                    summary: line.summary.to_string(),
                    body: line.body.as_ref().map(|b| b.to_string()),
                    prior_exists: line.prior_exists,
                    source_path: source,
                });
                index.insert(identity, slot);
                slot
            }
        };
        cached_lines.push(CachedBlameLine {
            commit: slot,
            line: line.line.clone(),
            prior_commit: line.prior_commit.as_ref().map(|c| c.to_string()),
        });
    }

    CachedBlame {
        schema_version: SCHEMA_VERSION,
        domain: DOMAIN_BLAME,
        commit_id: commit_id.to_owned(),
        path: path.to_string_lossy().into_owned(),
        commits,
        lines: cached_lines,
    }
}

/// Rebuild [`BlameLine`]s from the commit table.
///
/// Pure — no filesystem, no repository — so the table deduplication can be
/// tested without one.
fn reconstruct_blame(cached: &CachedBlame) -> Vec<BlameLine> {
    // Shared per commit, so a 5,000-line file clones 5,000 refcounts instead of
    // 5,000 copies of each commit message.
    let table: Vec<SharedBlameCommit> = cached.commits.iter().map(SharedBlameCommit::of).collect();
    cached
        .lines
        .iter()
        .filter_map(|line| {
            let entry = table.get(line.commit as usize)?;
            Some(BlameLine {
                commit_id: entry.id.clone(),
                author: entry.author.clone(),
                author_time_unix: entry.author_time_unix,
                summary: entry.summary.clone(),
                body: entry.body.clone(),
                line: line.line.clone(),
                prior_exists: entry.prior_exists,
                source_path: entry.source_path.clone(),
                prior_commit: line.prior_commit.as_deref().map(Arc::<str>::from),
            })
        })
        .collect()
}

struct SharedBlameCommit {
    id: Arc<str>,
    author: Arc<str>,
    author_time_unix: Option<i64>,
    summary: Arc<str>,
    body: Option<Arc<str>>,
    prior_exists: bool,
    source_path: Option<PathBuf>,
}

impl SharedBlameCommit {
    fn of(c: &CachedBlameCommit) -> Self {
        Self {
            id: Arc::from(c.id.as_str()),
            author: Arc::from(c.author.as_str()),
            author_time_unix: c.author_time_unix,
            summary: Arc::from(c.summary.as_str()),
            body: c.body.as_ref().map(|b| Arc::from(b.as_str())),
            prior_exists: c.prior_exists,
            source_path: c.source_path.as_deref().map(PathBuf::from),
        }
    }
}

// ---------------------------------------------------------------------------
// Search domain
//
// A commit search is `git log --all` twice — message then author, up to
// `limit` matches each — so on a 37k-commit repository it costs ~940ms. The
// query is what the user typed, and the result is a pure function of (query,
// refs), so the same reasoning as the other two domains applies: cache the run,
// key it by the refs, slice it per request.
//
// Unlike the log domain this is not on the cold-open path — a search is a
// deliberate action, and its results are not restored across restarts. It is
// worth caching anyway because the win is large (37x on a hit: ~940ms -> ~25ms)
// and the overhead is small: the all-refs fingerprint is ~23ms once the OS file
// cache is warm, so break-even is a ~3% hit rate.
// ---------------------------------------------------------------------------

/// The largest search result that will be written to disk.
const MAX_SEARCH_WINDOW: usize = 2000;

fn search_request_hash(query: &str) -> u64 {
    // Trimmed here rather than by the caller so a lookup and a store can never
    // disagree about which query they are talking about.
    request_hash(DOMAIN_SEARCH, 0, Some(query.trim()))
}

/// Serve a completed search for `query` out of the cache, if it can.
///
/// `None` means "run the search": no result cached, a stale one, or a request
/// that wants more than a result we know was truncated.
pub(super) fn load_search(
    repo: &Repository,
    repo_path: &Path,
    query: &str,
    limit: usize,
    // See [`load_log_page`]: pass the fingerprint when the caller has already
    // enumerated refs — here it means one enumeration for both the lookup and
    // the store that follows a miss.
    fingerprint: Option<u64>,
) -> Option<Vec<Commit>> {
    let fingerprint = fingerprint.unwrap_or_else(|| all_refs_fingerprint_inner(repo, false));
    let path = cache_path(repo_path, fingerprint, search_request_hash(query));

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            SEARCH_CACHE_STATS
                .misses_cold
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "search miss kind=cold limit={limit} rate={:.0}%",
                SEARCH_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };
    let parsed: CachedSearch = match serde_json::from_slice(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            SEARCH_CACHE_STATS
                .misses_corrupt
                .fetch_add(1, Ordering::Relaxed);
            log_event(format_args!(
                "search miss kind=corrupt limit={limit} rate={:.0}%",
                SEARCH_CACHE_STATS.hit_rate() * 100.0
            ));
            return None;
        }
    };

    if parsed.schema_version != SCHEMA_VERSION
        || parsed.domain != DOMAIN_SEARCH
        || parsed.ref_fingerprint != fingerprint
    {
        SEARCH_CACHE_STATS
            .misses_stale
            .fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "search miss kind=stale limit={limit} rate={:.0}%",
            SEARCH_CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    if limit > parsed.commits.len() && parsed.has_more {
        SEARCH_CACHE_STATS
            .misses_cold
            .fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "search miss kind=short limit={limit} available={} rate={:.0}%",
            parsed.commits.len(),
            SEARCH_CACHE_STATS.hit_rate() * 100.0
        ));
        return None;
    }

    let commits = parsed
        .commits
        .iter()
        .take(limit)
        .map(reconstruct_commit)
        .collect();

    SEARCH_CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
    touch_cache_file(&path);
    log_event(format_args!(
        "search hit limit={limit} rate={:.0}%",
        SEARCH_CACHE_STATS.hit_rate() * 100.0
    ));
    Some(commits)
}

/// Persist a finished search for `query`, keyed by the current refs.
///
/// `requested` is the `limit` the search was allowed to return — the only way to
/// know whether the result reached the end of the matches.
pub(super) fn store_search(
    repo: &Repository,
    repo_path: &Path,
    query: &str,
    results: &[Commit],
    requested: usize,
    fingerprint: Option<u64>,
) {
    // An empty result is cached too: "nothing matches" is a real answer, and
    // re-running a negative search costs exactly as much as a positive one.
    let fingerprint = fingerprint.unwrap_or_else(|| all_refs_fingerprint_inner(repo, false));
    let key = search_request_hash(query);
    let path = cache_path(repo_path, fingerprint, key);

    let keep = results.len().min(MAX_SEARCH_WINDOW);
    let has_more = results.len() > keep || results.len() >= requested;
    let cached = CachedSearch {
        schema_version: SCHEMA_VERSION,
        domain: DOMAIN_SEARCH,
        ref_fingerprint: fingerprint,
        query: query.trim().to_owned(),
        has_more,
        commits: results[..keep].iter().map(project_commit).collect(),
    };

    let bytes = match serde_json::to_vec(&cached) {
        Ok(b) => b,
        Err(_) => return,
    };

    let dir = cache_dir();
    if !retry_on_transient_fs(|| std::fs::create_dir_all(&dir)) {
        return;
    }

    let tmp = dir.join(format!(
        "tmp-{}-{:016x}-{:016x}.json",
        std::process::id(),
        fingerprint,
        key,
    ));
    if !retry_on_transient_fs(|| std::fs::write(&tmp, &bytes)) {
        return;
    }
    if retry_on_transient_fs(|| std::fs::rename(&tmp, &path)) {
        SEARCH_CACHE_STATS.stores.fetch_add(1, Ordering::Relaxed);
        log_event(format_args!(
            "search store commits={} bytes={} has_more={has_more}",
            cached.commits.len(),
            bytes.len()
        ));
        maybe_sweep_cache();
    }
    prune_old_generations(repo_path, fingerprint);
}

// ---------------------------------------------------------------------------
// Pruning
// ---------------------------------------------------------------------------

/// One cache directory entry, as the sweep sees it.
#[derive(Clone, Debug)]
struct CacheFileEntry {
    modified: SystemTime,
    size: u64,
    /// `v{n}-r{hash}-` — the per-repository namespace inside the cache dir.
    repo_prefix: String,
    path: PathBuf,
}

/// Pick the files to delete so both byte ceilings hold, oldest first.
///
/// Kept separate from the filesystem so the budget arithmetic is testable:
/// "oldest" is least-recently-*used* because [`touch_cache_file`] stamps mtime
/// on every hit.
fn eviction_plan(entries: &[CacheFileEntry], per_repo_cap: u64, total_cap: u64) -> Vec<PathBuf> {
    let mut per_repo: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    for entry in entries {
        *per_repo.entry(entry.repo_prefix.as_str()).or_default() += entry.size;
    }
    let mut by_age: Vec<&CacheFileEntry> = entries.iter().collect();
    by_age.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.path.cmp(&b.path))
    });

    let mut doomed: Vec<PathBuf> = Vec::new();
    // Per-repository first: one busy repository must not be able to spend the
    // whole global budget.
    for (repo, bytes) in &per_repo {
        let mut remaining = *bytes;
        if remaining <= per_repo_cap {
            continue;
        }
        for entry in by_age.iter().filter(|e| e.repo_prefix == *repo) {
            if remaining <= per_repo_cap {
                break;
            }
            doomed.push(entry.path.clone());
            remaining = remaining.saturating_sub(entry.size);
        }
    }

    let doomed_paths: std::collections::HashSet<PathBuf> = doomed.iter().cloned().collect();
    let mut total: u64 = entries
        .iter()
        .filter(|e| !doomed_paths.contains(e.path.as_path()))
        .map(|e| e.size)
        .sum();
    if total > total_cap {
        for entry in &by_age {
            if total <= total_cap {
                break;
            }
            if doomed_paths.contains(entry.path.as_path()) {
                continue;
            }
            doomed.push(entry.path.clone());
            total = total.saturating_sub(entry.size);
        }
    }
    doomed
}

/// Delete expired entries and then enforce both byte ceilings.
fn sweep_cache(now: SystemTime) {
    let dir = cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let schema_prefix = format!("v{}-", SCHEMA_VERSION);
    let mut live: Vec<CacheFileEntry> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue;
        }
        // A file written by another schema version can never be read again, so
        // drop it here: otherwise every schema bump leaks disk forever.
        if !name.starts_with(&schema_prefix) {
            if name.starts_with("v") {
                let _ = std::fs::remove_file(&path);
            }
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        if now.duration_since(modified).unwrap_or_default() > MAX_AGE {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        // `v{n}-r{hash}-f{fp}-{req}.json`: the repo namespace ends at the `-f`
        // separator. Hex ref hashes contain no dash, so the first hit is it.
        let repo_prefix = match name.find("-f") {
            Some(index) => name[..index + 1].to_string(),
            None => name.clone(),
        };
        live.push(CacheFileEntry {
            modified,
            size: metadata.len(),
            repo_prefix,
            path,
        });
    }

    for path in eviction_plan(&live, MAX_BYTES_PER_REPO, MAX_TOTAL_BYTES) {
        let _ = std::fs::remove_file(path);
    }
}

/// Run [`sweep_cache`] every [`SWEEP_EVERY_STORES`] stores — it reads the whole
/// cache directory, so it must stay off the hot path.
fn maybe_sweep_cache() {
    if STORES_SINCE_SWEEP.fetch_add(1, Ordering::Relaxed) + 1 < SWEEP_EVERY_STORES {
        return;
    }
    STORES_SINCE_SWEEP.store(0, Ordering::Relaxed);
    sweep_cache(SystemTime::now());
}

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
        .filter_map(|p| {
            std::fs::metadata(&p)
                .ok()
                .map(|m| (m.modified().unwrap_or(UNIX_EPOCH), p))
        })
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

/// Delete every cache file for this repository.
///
/// Test-only: lets the probe harness measure a genuine cold open instead of
/// reusing whatever a previous run left behind.
#[cfg(test)]
pub(super) fn clear_repo_cache(repo_path: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir()) else {
        return;
    };
    let prefix = format!("v{}-r{:016x}-", SCHEMA_VERSION, repo_hash(repo_path));
    for entry in entries.flatten() {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".json"));
        if matches {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn sample_page() -> LogPage {
        let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        LogPage {
            commits: vec![
                Commit {
                    id: CommitId(Arc::from("1111111111111111111111111111111111111111")),
                    parent_ids: CommitParentIds::from(vec![CommitId(Arc::from(
                        "2222222222222222222222222222222222222222",
                    ))]),
                    summary: Arc::from("initial commit"),
                    author: Arc::from("Alice"),
                    time: t,
                    signed: false,
                },
                Commit {
                    id: CommitId(Arc::from("3333333333333333333333333333333333333333")),
                    parent_ids: CommitParentIds::from(vec![]),
                    summary: Arc::from("second"),
                    author: Arc::from("Bob"),
                    time: t + Duration::from_secs(60),
                    signed: true,
                },
            ],
            next_cursor: Some(LogCursor {
                last_seen: CommitId(Arc::from("3333333333333333333333333333333333333333")),
                resume_from: Some(CommitId(Arc::from(
                    "2222222222222222222222222222222222222222",
                ))),
                // Intentionally dropped on rebuild; the consumer falls back to
                // `last_seen` semantics. Asserted below.
                resume_token: Some(Arc::from("opaque-token")),
            }),
        }
    }

    /// Build a snapshot whose commits are `c0..c{n-1}`, for slicing tests.
    fn snapshot_of(count: usize, has_more: bool) -> CachedSnapshot {
        CachedSnapshot {
            schema_version: SCHEMA_VERSION,
            domain: DOMAIN_LOG,
            ref_fingerprint: 1,
            mode: 1,
            author: None,
            has_more,
            commits: (0..count)
                .map(|i| CachedCommit {
                    id: format!("c{i}"),
                    parent_ids: Vec::new(),
                    summary: format!("commit {i}"),
                    author: "Test".to_string(),
                    time: 1_700_000_000 + i as i64,
                    signed: false,
                })
                .collect(),
        }
    }

    fn ids(page: &LogPage) -> Vec<String> {
        page.commits.iter().map(|c| c.id.0.to_string()).collect()
    }

    /// The commits survive a full serialize -> deserialize -> reconstruct cycle,
    /// which is exactly what the on-disk cache does on a cold open.
    #[test]
    fn commits_round_trip_through_cached_projection() {
        let page = sample_page();
        let cached = CachedSnapshot {
            schema_version: SCHEMA_VERSION,
            domain: DOMAIN_LOG,
            ref_fingerprint: 0xabcdef,
            mode: 1,
            author: Some("alice".to_string()),
            has_more: true,
            commits: page.commits.iter().map(project_commit).collect(),
        };

        let json = serde_json::to_string(&cached).expect("serialize");
        let parsed: CachedSnapshot = serde_json::from_str(&json).expect("deserialize");
        let rebuilt: Vec<Commit> = parsed.commits.iter().map(reconstruct_commit).collect();

        assert_eq!(rebuilt, page.commits);
    }

    /// Encoding then decoding a second time yields the same bytes, so the
    /// on-disk form is stable across repeated cold opens.
    #[test]
    fn cached_projection_is_idempotent() {
        let page = sample_page();
        let cached = CachedSnapshot {
            schema_version: SCHEMA_VERSION,
            domain: DOMAIN_LOG,
            ref_fingerprint: 1,
            mode: 0,
            author: None,
            has_more: false,
            commits: page.commits.iter().map(project_commit).collect(),
        };
        let once = serde_json::to_string(&cached).unwrap();
        let parsed: CachedSnapshot = serde_json::from_str(&once).unwrap();
        let twice = serde_json::to_string(&parsed).unwrap();
        assert_eq!(once, twice);
    }

    /// One snapshot serves every page size: this is the whole point of storing a
    /// run instead of a page.
    #[test]
    fn snapshot_serves_any_page_size() {
        let snapshot = snapshot_of(10, true);
        for limit in [1usize, 3, 10] {
            let page = slice_snapshot(&snapshot, limit, None).expect("slice");
            assert_eq!(page.commits.len(), limit, "limit={limit}");
            assert_eq!(ids(&page)[0], "c0");
        }
        // Asking for more than the run holds while more exist must walk.
        assert_eq!(
            slice_snapshot(&snapshot, 20, None).unwrap_err(),
            SliceMiss::Short { available: 10 }
        );
    }

    /// "Load more" is served by offset: the cursor names the last commit the
    /// caller saw, and the next page starts after it.
    #[test]
    fn snapshot_serves_load_more_by_offset() {
        let snapshot = snapshot_of(10, true);

        let first = slice_snapshot(&snapshot, 4, None).expect("first page");
        assert_eq!(ids(&first), vec!["c0", "c1", "c2", "c3"]);

        let cursor = first
            .next_cursor
            .expect("has a next page")
            .last_seen
            .0
            .to_string();
        assert_eq!(cursor, "c3");

        let second = slice_snapshot(&snapshot, 4, Some(&cursor)).expect("second page");
        assert_eq!(ids(&second), vec!["c4", "c5", "c6", "c7"]);
    }

    /// A cursor that is not inside the run cannot be served — it would silently
    /// return the wrong commits.
    #[test]
    fn snapshot_rejects_cursor_outside_the_run() {
        let snapshot = snapshot_of(10, false);
        assert_eq!(
            slice_snapshot(&snapshot, 4, Some("nope")).unwrap_err(),
            SliceMiss::Outside
        );
    }

    /// When the run is the whole history, a request past its end yields a short
    /// final page rather than a miss.
    #[test]
    fn snapshot_returns_short_final_page_when_history_ends() {
        let snapshot = snapshot_of(10, false);
        let page = slice_snapshot(&snapshot, 4, Some("c8")).expect("final page");
        assert_eq!(ids(&page), vec!["c9"]);
        assert!(
            page.next_cursor.is_none(),
            "nothing follows the last commit"
        );
    }

    /// A reflog window round-trips through the cached projection, and the
    /// positional fields (`index`, `HEAD@{n}`) are rebuilt from the offset
    /// rather than stored — storing them would give them a second place to
    /// disagree with the window they came from.
    #[test]
    fn reflog_window_round_trips_through_the_cached_projection() {
        let window = CachedReflog {
            schema_version: SCHEMA_VERSION,
            domain: DOMAIN_REFLOG,
            ref_fingerprint: 7,
            has_more: true,
            entries: vec![
                CachedReflogEntry {
                    new_id: "aaa".to_string(),
                    message: "commit: first".to_string(),
                    time: 1_700_000_000,
                    author: "Test User".to_string(),
                },
                CachedReflogEntry {
                    new_id: "bbb".to_string(),
                    message: "commit: second".to_string(),
                    time: 1_700_000_060,
                    author: "Test User".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&window).expect("serialize");
        let parsed: CachedReflog = serde_json::from_str(&json).expect("deserialize");
        let entries = reconstruct_reflog(parsed.entries, 10);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].selector.as_ref(), "HEAD@{0}");
        assert_eq!(entries[0].new_id.0.as_ref(), "aaa");
        assert_eq!(entries[0].message.as_ref(), "commit: first");
        assert_eq!(entries[0].author.as_ref(), "Test User");
        assert_eq!(
            entries[0].time,
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(entries[1].selector.as_ref(), "HEAD@{1}");
    }

    /// One window serves every page size, the same way one log snapshot does.
    #[test]
    fn reflog_window_serves_any_page_size() {
        for limit in [1usize, 3, 5] {
            // Rebuilt per iteration: the domain type owns its window, and the
            // projection is what the store hands over.
            let entries = reconstruct_reflog(
                (0..5)
                    .map(|i| CachedReflogEntry {
                        new_id: format!("id{i}"),
                        message: format!("entry {i}"),
                        time: 1_700_000_000 + i as i64,
                        author: "Test User".to_string(),
                    })
                    .collect(),
                limit,
            );
            assert_eq!(entries.len(), limit, "limit={limit}");
            assert_eq!(entries[0].new_id.0.as_ref(), "id0", "newest first");
        }
    }

    /// A blame round-trips, and the commit table is what keeps it affordable:
    /// the domain type repeats each commit's author/summary/body on every line,
    /// which a 5,000-line file would otherwise duplicate 5,000 times.
    #[test]
    fn blame_round_trips_with_one_table_entry_per_distinct_commit() {
        let lines = vec![
            BlameLine {
                commit_id: Arc::from("1111111111111111111111111111111111111111"),
                author: Arc::from("Ada"),
                author_time_unix: Some(1_700_000_000),
                summary: Arc::from("initial commit"),
                body: Some(Arc::from("a body that would otherwise repeat")),
                line: "one".to_string(),
                prior_exists: false,
                source_path: None,
                prior_commit: None,
            },
            BlameLine {
                commit_id: Arc::from("1111111111111111111111111111111111111111"),
                author: Arc::from("Ada"),
                author_time_unix: Some(1_700_000_000),
                summary: Arc::from("initial commit"),
                body: Some(Arc::from("a body that would otherwise repeat")),
                line: "two".to_string(),
                prior_exists: false,
                source_path: None,
                prior_commit: None,
            },
            BlameLine {
                // Same commit, different historical path: a distinct entry,
                // because `prior_exists` is answered per (commit, path).
                commit_id: Arc::from("1111111111111111111111111111111111111111"),
                author: Arc::from("Ada"),
                author_time_unix: Some(1_700_000_000),
                summary: Arc::from("initial commit"),
                body: Some(Arc::from("a body that would otherwise repeat")),
                line: "three".to_string(),
                prior_exists: true,
                source_path: Some(PathBuf::from("src/old.rs")),
                prior_commit: None,
            },
        ];

        let cached = project_blame(
            "1111111111111111111111111111111111111111",
            Path::new("src/new.rs"),
            &lines,
        );
        assert_eq!(cached.commits.len(), 2, "one entry per (commit, path)");
        assert_eq!(cached.lines.len(), 3);

        let json = serde_json::to_string(&cached).expect("serialize");
        let parsed: CachedBlame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(reconstruct_blame(&parsed), lines);
    }

    /// The settings surface reports the cache it can actually reach: the
    /// installed root, its size, and its entry count.
    #[test]
    fn cache_usage_and_clear_see_only_current_schema_entries() {
        let root = cache_test_root();
        assert_eq!(cache_dir(), root, "the thread root wins");
        clear_all();

        // Named after the current schema, so bumping it does not silently turn
        // this into a test about nothing.
        let current = format!("v{SCHEMA_VERSION}-r00000000000000aa-f1-2.json");
        let previous = format!("v{}-r00000000000000aa-f1-2.json", SCHEMA_VERSION - 1);
        let payload = b"{\"schema_version\":2}";
        std::fs::write(root.join(&current), payload).expect("write cache entry");
        // Old schema and unrelated files are somebody else's, not ours to count
        // or delete.
        std::fs::write(root.join(&previous), b"{}").expect("write old entry");
        std::fs::write(root.join("notes.txt"), b"x").expect("write stray file");

        let (bytes, entries) = cache_usage();
        assert_eq!(entries, 1, "only the current schema counts");
        assert_eq!(bytes, payload.len() as u64);

        assert_eq!(clear_all(), 1, "clear removes exactly what we counted");
        assert_eq!(cache_usage(), (0, 0));
    }

    fn entry(repo: &str, name: &str, seconds_ago: u64, size: u64) -> CacheFileEntry {
        CacheFileEntry {
            modified: UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 - seconds_ago),
            size,
            repo_prefix: repo.to_string(),
            path: PathBuf::from(name),
        }
    }

    /// A single repository may not spend the whole budget: blame-sized entries
    /// would otherwise evict every log snapshot in the app.
    #[test]
    fn eviction_plan_caps_each_repo_before_the_global_budget() {
        let entries = vec![
            entry("v2-ra-", "a-old.json", 30, 20),
            entry("v2-ra-", "a-new.json", 10, 20),
            entry("v2-rb-", "b-old.json", 20, 5),
        ];
        let doomed = eviction_plan(&entries, 32, 200);
        assert_eq!(
            doomed,
            vec![PathBuf::from("a-old.json")],
            "only the over-budget repo loses anything, oldest first"
        );
    }

    /// Once the per-repo caps hold, the global ceiling evicts oldest-first
    /// across repositories.
    #[test]
    fn eviction_plan_enforces_global_cap_oldest_first() {
        let entries = vec![
            entry("v2-ra-", "a-old.json", 30, 10),
            entry("v2-ra-", "a-new.json", 10, 10),
            entry("v2-rb-", "b-mid.json", 20, 10),
        ];
        let doomed = eviction_plan(&entries, 32, 15);
        assert_eq!(
            doomed,
            vec![PathBuf::from("a-old.json"), PathBuf::from("b-mid.json")],
            "oldest across repos goes first, and only until the cap holds"
        );
    }

    /// Eviction is LRU, not FIFO: a hit stamps mtime, so a snapshot that is
    /// still being read survives a newer-but-colder one.
    #[test]
    fn eviction_plan_spares_the_most_recently_used_entry() {
        let entries = vec![
            entry("v2-ra-", "a-cold.json", 30, 10),
            entry("v2-ra-", "a-hot.json", 1, 10),
        ];
        assert_eq!(
            eviction_plan(&entries, 32, 10),
            vec![PathBuf::from("a-cold.json")]
        );
    }
}
