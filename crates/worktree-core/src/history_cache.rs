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
#[derive(Clone, Copy)]
pub struct HistoryCacheHooks {
    /// Where the cache lives.
    pub dir: fn() -> PathBuf,
    /// Current `(bytes, entries)`.
    pub usage: fn() -> HistoryCacheUsage,
    /// Delete everything; returns the number of entries removed.
    pub clear: fn() -> usize,
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

/// `None` when no backend registered a cache — the caller cannot distinguish
/// "nothing to clear" from "no cache", which is the point: the settings surface
/// hides the whole section in that case.
pub fn clear_history_cache() -> Option<usize> {
    HOOKS.get().map(|hooks| (hooks.clear)())
}
