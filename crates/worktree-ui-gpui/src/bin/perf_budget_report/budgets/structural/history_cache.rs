use super::super::*;

/// History-cache hit-rate budgets for every reachable domain.
///
/// The metrics (`history_cache.hit_rate.{log,search,reflog,blame}`) are emitted
/// by the perf sidecar on the `monorepo_open_and_history_load_repeat` real-repo
/// bench; on the hosted runner they are skipped entirely because `real_repo/*`
/// is excluded from the subset. The thresholds are the hard floors from the
/// iteration-06 plan: below them the cache is net harmful (commit-search breaks
/// even at ~3% hit rate) or a domain's key has regressed to a permanent miss.
///
/// Every domain is exercised by `RealRepoFixture::run_cache_repeat`, which runs
/// a cold-then-hot pair of reads so the bench reports a real hit rate instead of
/// a single cold pass: the commit-log pages via `run_monorepo_open_and_history`,
/// `search` via `search_commits`, `reflog` via `reflog_head` (on
/// `GitRepositoryLog`), and `blame` via `blame_file` (on `GitRepositoryDiff`).
/// All four entry points are reachable from the bench — `blame_file` was already
/// on `GitRepositoryDiff` and wired to the on-disk blame cache (`load_blame`/
/// `store_blame`), so nothing remains deferred.
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
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.blame",
        comparator: StructuralBudgetComparator::AtLeast,
        // Blame is content-addressed and zero-expiry, so repeat views of the
        // same file/commit hit near-always; a low floor catches a key
        // regression that turns every read into a cold walk. `blame_file` is on
        // `GitRepositoryDiff` and is served from the on-disk blame cache on a
        // repeat, so the bench exercises it (cold store + hot hit).
        threshold: 30.0,
    },
];
