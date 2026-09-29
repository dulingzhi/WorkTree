use super::super::*;

/// History-cache hit-rate budgets for the *reachable* domains.
///
/// The metrics (`history_cache.hit_rate.{log,search,reflog}`) are emitted by
/// the perf sidecar on the `monorepo_open_and_history_load_repeat` real-repo
/// bench; on the hosted runner they are skipped entirely because `real_repo/*`
/// is excluded from the subset. The thresholds are the hard floors from the
/// iteration-06 plan: below them the cache is net harmful (commit-search breaks
/// even at ~3% hit rate) or a domain's key has regressed to a permanent miss.
///
/// BLAME is intentionally *not* budgeted here: its on-disk cache entry point
/// (`load_blame`) is `pub(super)` and is not exposed on `GitRepositoryLog`, so
/// a bench cannot trigger a hit. Its budget lives in
/// [`DEFERRED_STRUCTURAL_BUDGETS`] and will be promoted once `GitRepositoryLog`
/// exposes a blame method (or an equivalent). REFLOG was promoted from
/// [`DEFERRED_STRUCTURAL_BUDGETS`] because `reflog_head` is exposed on
/// `GitRepositoryLog` and `run_cache_repeat` drives a cold+hot read.
pub(crate) const STRUCTURAL_BUDGETS: &[StructuralBudgetSpec] = &[
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.log",
        comparator: StructuralBudgetComparator::AtLeast,
        // History view reuses the same pages heavily; a low floor only catches a
        // gross key/fingerprint regression.
        threshold: 20.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.search",
        comparator: StructuralBudgetComparator::AtLeast,
        // Break-even for commit-search is ~3% (saves ~800ms vs ~23ms miss);
        // below this the cache costs more than it saves.
        threshold: 3.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.reflog",
        comparator: StructuralBudgetComparator::AtLeast,
        // Reflog window is served from disk on a second open; a low floor only
        // catches a fingerprint/schema regression that turns every read into a
        // cold walk. `reflog_head` is exposed on `GitRepositoryLog`, so the
        // repeat bench exercises it (cold store + hot hit).
        threshold: 10.0,
    },
];

/// History-cache hit-rate budget that cannot be exercised from a bench yet.
///
/// Documented here so the intent (and the reason for deferral) is not lost.
/// This is *not* collected into [`STRUCTURAL_BUDGETS`] — doing so would emit a
/// missing-metric alert on the dedicated runner, because the fixture cannot
/// reach the blame cache entry point (`load_blame` is `pub(super)` and not on
/// `GitRepositoryLog`). Promote it once `GitRepositoryLog` exposes a blame
/// method (or an equivalent). REFLOG was promoted to [`STRUCTURAL_BUDGETS`]:
/// `reflog_head` is on the trait and `run_cache_repeat` drives a cold+hot read.
#[allow(dead_code)]
pub(crate) const DEFERRED_STRUCTURAL_BUDGETS: &[StructuralBudgetSpec] = &[StructuralBudgetSpec {
    bench: "real_repo/monorepo_open_and_history_load_repeat",
    metric: "history_cache.hit_rate.blame",
    comparator: StructuralBudgetComparator::AtLeast,
    // Blame is content-addressed and zero-expiry, so repeat views of the
    // same file/commit should hit near-always; a low floor catches a key
    // regression that turns every read into a cold walk.
    threshold: 30.0,
}];
