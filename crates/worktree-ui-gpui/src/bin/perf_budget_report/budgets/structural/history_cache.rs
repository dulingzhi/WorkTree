use super::super::*;

/// History-cache hit-rate budgets for the *reachable* domains.
///
/// The metrics (`history_cache.hit_rate.{log,search}`) are emitted by the perf
/// sidecar on the `monorepo_open_and_history_load_repeat` real-repo bench; on
/// the hosted runner they are skipped entirely because `real_repo/*` is
/// excluded from the subset. The thresholds are the hard floors from the
/// iteration-06 plan: below them the cache is net harmful (commit-search breaks
/// even at ~3% hit rate) or a domain's key has regressed to a permanent miss.
///
/// REFLOG and BLAME are intentionally *not* budgeted here: their on-disk cache
/// entry points (`load_reflog_window`/`load_blame`) are `pub(super)` and are
/// not exposed on `GitRepositoryLog`, so a bench cannot trigger a hit. Their
/// budgets live in [`DEFERRED_STRUCTURAL_BUDGETS`] and will be promoted once the
/// cache entry points are surfaced on the trait.
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
];

/// History-cache hit-rate budgets that cannot be exercised from a bench yet.
///
/// Documented here so the intent (and the reason for deferral) is not lost.
/// These are *not* collected into [`STRUCTURAL_BUDGETS`] — doing so would emit a
/// missing-metric alert on the dedicated runner, because the fixture cannot
/// reach the reflog/blame cache entry points. Promote them once
/// `GitRepositoryLog` exposes `reflog_head`/`load_blame` (or an equivalent).
#[allow(dead_code)]
pub(crate) const DEFERRED_STRUCTURAL_BUDGETS: &[StructuralBudgetSpec] = &[
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.reflog",
        comparator: StructuralBudgetComparator::AtLeast,
        threshold: 10.0,
    },
    StructuralBudgetSpec {
        bench: "real_repo/monorepo_open_and_history_load_repeat",
        metric: "history_cache.hit_rate.blame",
        comparator: StructuralBudgetComparator::AtLeast,
        // Blame is content-addressed and zero-expiry, so repeat views of the
        // same file/commit should hit near-always; a low floor catches a key
        // regression that turns every read into a cold walk.
        threshold: 30.0,
    },
];
