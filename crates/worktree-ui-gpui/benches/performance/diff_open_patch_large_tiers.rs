use super::common::*;

/// T6 large-diff tiers.
///
/// Two synthetic tiers cover the extremes the directory-diff view must stay
/// responsive against:
///
/// - `10mb_single_file`: a single modified file whose unified-diff text exceeds
///   10MB (`new_with_line_bytes`). Exercises the paged row provider on very wide
///   payloads.
/// - `50k_additions`: a whole-file drop-in of >50k purely-added lines
///   (`new_additions_only`). Exercises first-window paging on very tall diffs.
///
/// Both assert the structural invariant the directory-diff view relies on: the
/// first visible window is paged (no full-text materialization) and still paints
/// the requested window of rows. See `diff_repo::STRUCTURAL_BUDGETS`.
pub(crate) fn bench_diff_open_patch_large_tiers(c: &mut Criterion) {
    let window = env_usize("WORKTREE_BENCH_PATCH_DIFF_WINDOW", 200);

    let ten_mb = PatchDiffPagedRowsFixture::new_with_line_bytes(
        env_usize("WORKTREE_BENCH_PATCH_DIFF_10MB_LINES", 50_000),
        env_usize("WORKTREE_BENCH_PATCH_DIFF_10MB_LINE_BYTES", 256),
    );
    let fifty_k = PatchDiffPagedRowsFixture::new_additions_only(env_usize(
        "WORKTREE_BENCH_PATCH_DIFF_50K_LINES",
        50_000,
    ));

    let tiers: [(&str, &PatchDiffPagedRowsFixture); 2] =
        [("10mb_single_file", &ten_mb), ("50k_additions", &fifty_k)];

    let mut group = c.benchmark_group("diff_open_patch_large_tiers");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    for (tier, fixture) in tiers.iter() {
        group.bench_with_input(
            BenchmarkId::new(*tier, window),
            &window,
            |b, &window| b.iter(|| fixture.run_paged_first_window_step(window)),
        );
    }
    group.finish();

    // Emit structural sidecar metrics for each tier using the standard patch
    // diff format (rows_painted == split_rows_painted, no full-text materialize).
    for (tier, fixture) in tiers.iter() {
        let sidecar_started_at = Instant::now();
        let metrics = measure_sidecar_allocations(|| fixture.measure_paged_first_window_step(window));
        let first_window_ns = sidecar_started_at
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;

        let mut payload = Map::new();
        payload.insert("first_window_ns".to_string(), json!(first_window_ns));
        payload.insert("rows_requested".to_string(), json!(metrics.rows_requested));
        payload.insert(
            "rows_painted".to_string(),
            json!(metrics.split_rows_painted),
        );
        payload.insert(
            "rows_materialized".to_string(),
            json!(metrics.split_rows_materialized),
        );
        payload.insert(
            "patch_page_cache_entries".to_string(),
            json!(metrics.patch_page_cache_entries),
        );
        payload.insert(
            "full_text_materializations".to_string(),
            json!(metrics.full_text_materializations),
        );
        emit_sidecar_metrics(
            &format!("diff_open_patch_large_tiers/{tier}/{window}"),
            payload,
        );
    }
}
