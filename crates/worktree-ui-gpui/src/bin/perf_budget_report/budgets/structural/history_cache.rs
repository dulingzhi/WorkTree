use super::super::*;

/// History-cache hit-rate budgets, one per domain.
///
/// The metrics (`history_cache.hit_rate.{log,reflog,search,blame}`) are emitted
/// by the perf sidecar on real-repo benches; on the hosted runner they are
/// skipped entirely because `real_repo/*` is excluded from the subset. The
/// thresholds are the hard floors from the iteration-06 plan: below them the
/// cache is net harmful (commit-search breaks even at ~3% hit rate) or a
/// domain's key has regressed to a permanent miss (which a single aggregate
/// hit rate would hide behind "log always hits").
pub(crate) const STRUCTURAL_BUDGETS: &[StructuralBudgetSpec] = &[
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load",
        metric: "history_cache.hit_rate.log",
        comparator: StructuralBudgetComparator::AtLeast,
        // History view reuses the same pages heavily; a low floor only catches a
        // gross key/fingerprint regression.
        threshold: 20.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load",
        metric: "history_cache.hit_rate.reflog",
        comparator: StructuralBudgetComparator::AtLeast,
        threshold: 10.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load",
        metric: "history_cache.hit_rate.search",
        comparator: StructuralBudgetComparator::AtLeast,
        // Break-even for commit-search is ~3% (saves ~800ms vs ~23ms miss);
        // below this the cache costs more than it saves.
        threshold: 3.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load",
        metric: "history_cache.hit_rate.blame",
        comparator: StructuralBudgetComparator::AtLeast,
        // Blame is content-addressed and zero-expiry, so repeat views of the
        // same file/commit should hit near-always; a low floor catches a key
        // regression that turns every read into a cold walk.
        threshold: 30.0,
    },
];
