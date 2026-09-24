//! Registry for the optional on-disk history cache.
//!
//! `worktree-git-gix` is an *optional* dependency of both the UI crate and the
//! binary, so the settings page cannot call into it directly: with a
//! non-default feature set it may not even be linked. Instead the backend
//! publishes a tiny set of hooks here at startup, and every accessor degrades
//! to `None` when nothing has registered — the storage surface then simply has
//! nothing to report instead of failing to build.

use std::path::PathBuf;
use std::sync::OnceLock;

/// `(bytes, entries)` currently held on disk by the cache.
pub type HistoryCacheUsage = (u64, usize);

/// Operations the history cache exposes to the rest of the app.
///
/// Function pointers rather than a trait object: these are process-wide
/// singletons installed once, and keeping them `Copy` lets callers read them
/// without touching a lock.
/// Cumulative cache counters, for the perf sidecar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryCacheStats {
    /// A valid page was rehydrated from disk; no walk happened.
    pub hits: u64,
    /// No cache file existed — the true cold path.
    pub misses_cold: u64,
    /// A file existed but its fingerprint or request parameters did not match.
    pub misses_stale: u64,
    /// A file existed but could not be deserialized.
    pub misses_corrupt: u64,
    /// Pages successfully published to disk.
    pub stores: u64,
}

impl HistoryCacheStats {
    /// Reads served from disk as a percentage of all reads; `None` when nothing
    /// has been read, so a bench that never touched history reports nothing
    /// rather than a misleading 0%.
    pub fn hit_rate_pct(&self) -> Option<u64> {
        let reads = self.hits + self.misses_cold + self.misses_stale + self.misses_corrupt;
        if reads == 0 {
            return None;
        }
        Some((self.hits * 100) / reads)
    }

    pub fn reads(&self) -> u64 {
        self.hits + self.misses_cold + self.misses_stale + self.misses_corrupt
    }

    /// Counters seen since `earlier` — the sidecar reports per-bench deltas
    /// because Criterion runs every benchmark in one process.
    pub fn saturating_sub(self, earlier: Self) -> Self {
        Self {
            hits: self.hits.saturating_sub(earlier.hits),
            misses_cold: self.misses_cold.saturating_sub(earlier.misses_cold),
            misses_stale: self.misses_stale.saturating_sub(earlier.misses_stale),
            misses_corrupt: self.misses_corrupt.saturating_sub(earlier.misses_corrupt),
            stores: self.stores.saturating_sub(earlier.stores),
        }
    }
}

#[derive(Clone, Copy)]
pub struct HistoryCacheHooks {
    /// Where the cache lives.
    pub dir: fn() -> PathBuf,
    /// Current `(bytes, entries)`.
    pub usage: fn() -> HistoryCacheUsage,
    /// Delete everything; returns the number of entries removed.
    pub clear: fn() -> usize,
    /// Cumulative counters.
    pub stats: fn() -> HistoryCacheStats,
}

static HOOKS: OnceLock<HistoryCacheHooks> = OnceLock::new();

/// Install the cache hooks. Later installs are ignored rather than panicking —
/// the first backend to boot owns the cache directory for the process.
pub fn install_history_cache_hooks(hooks: HistoryCacheHooks) {
    let _ = HOOKS.set(hooks);
}

pub fn history_cache_registered() -> bool {
    HOOKS.get().is_some()
}

/// `None` when no backend registered a cache.
pub fn history_cache_dir() -> Option<PathBuf> {
    HOOKS.get().map(|hooks| (hooks.dir)())
}

/// `None` when no backend registered a cache.
pub fn history_cache_usage() -> Option<HistoryCacheUsage> {
    HOOKS.get().map(|hooks| (hooks.usage)())
}

/// `None` when no backend registered a cache.
pub fn history_cache_stats() -> Option<HistoryCacheStats> {
    HOOKS.get().map(|hooks| (hooks.stats)())
}

/// `None` when no backend registered a cache — the caller cannot distinguish
/// "nothing to clear" from "no cache", which is the point: the settings surface
/// hides the whole section in that case.
pub fn clear_history_cache() -> Option<usize> {
    HOOKS.get().map(|hooks| (hooks.clear)())
}

#[cfg(test)]
mod tests {
    use super::HistoryCacheStats;

    fn stats(hits: u64, cold: u64, stale: u64, corrupt: u64, stores: u64) -> HistoryCacheStats {
        HistoryCacheStats {
            hits,
            misses_cold: cold,
            misses_stale: stale,
            misses_corrupt: corrupt,
            stores,
        }
    }

    /// A bench that never touched history must report *nothing* rather than a
    /// misleading 0% — a missing key is skipped by the budget report instead of
    /// being judged.
    #[test]
    fn hit_rate_is_absent_until_something_is_read() {
        let empty = HistoryCacheStats::default();
        assert_eq!(empty.reads(), 0);
        assert_eq!(empty.hit_rate_pct(), None);
    }

    /// Criterion runs every bench in one process, so the sidecar reports deltas:
    /// the second bench must not be credited with the first bench's reads.
    #[test]
    fn delta_attributes_only_the_new_counters() {
        let after_first = stats(1, 4, 0, 0, 1);
        assert_eq!(after_first.hit_rate_pct(), Some(20));

        let after_second = stats(6, 4, 0, 0, 2);
        let second_only = after_second.saturating_sub(after_first);
        assert_eq!(
            second_only.reads(),
            5,
            "five reads happened after bench one"
        );
        assert_eq!(second_only.hits, 5);
        assert_eq!(second_only.stores, 1);
        assert_eq!(second_only.hit_rate_pct(), Some(100));
    }
}
