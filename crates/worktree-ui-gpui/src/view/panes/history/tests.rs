use super::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use worktree_core::domain::{CommitId, LogCursor, LogPage, RepoSpec};
use worktree_core::services::{GitBackend, GitRepository, Result};
use worktree_state::model::AppState;
use worktree_state::store::AppStore;

/// The linked-worktree rows live in this table, so the two revs behind them
/// have to move the fingerprint. Without them a finished scan -- or a row
/// being selected -- changed nothing the pane hashed, and the rows sat stale
/// until some unrelated rev happened to move. Not reachable from a
/// `#[gpui::test]`: `stable_cached_view` returns the uncached view under
/// `cfg!(test)`, so the missed repaint is invisible there.
#[test]
fn the_history_fingerprint_tracks_the_worktree_revs() {
    let mut state = AppState::default();
    state
        .repos
        .push(worktree_state::model::RepoState::new_opening(
            worktree_state::model::RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        ));
    state.active_repo = Some(worktree_state::model::RepoId(1));

    let fingerprint = |state: &AppState| HistoryView::notify_fingerprint_for(state, false);
    let before = fingerprint(&state);

    // The revs stand in for the writes that bump them: those setters are
    // `pub(crate)` to `worktree-state`, and what is being asserted here is
    // that the fingerprint reads them at all.
    state.repos[0].worktree_dirty_rev += 1;
    let after_scan = fingerprint(&state);
    assert_ne!(
        before, after_scan,
        "a finished worktree scan must repaint the rows it feeds"
    );

    state.repos[0].history_state.worktree_selection_rev += 1;
    assert_ne!(
        after_scan,
        fingerprint(&state),
        "selecting a worktree row must repaint the row that shows it"
    );
}

struct BlockingBackend;

impl GitBackend for BlockingBackend {
    fn open(&self, _workdir: &Path) -> Result<Arc<dyn GitRepository>> {
        loop {
            std::thread::park();
        }
    }
}

fn wait_until(
    cx: &mut gpui::VisualTestContext,
    description: &str,
    ready: impl Fn(&mut gpui::VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        cx.update(|window, app| {
            let _ = window.draw(app);
        });
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {description}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn set_history_view_state_for_tests(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<WorkTreeView>,
    state: Arc<AppState>,
) {
    cx.update(|window, app| {
        let history_view = view.read(app).main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            history.notify_fingerprint =
                HistoryView::notify_fingerprint_for(&state, history.history_show_tags);
            history.state = Arc::clone(&state);
            cx.notify();
        });
        window.refresh();
        let _ = window.draw(app);
    });
    cx.run_until_parked();
}

fn ensure_history_cache_for_tests(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<WorkTreeView>,
    state: Arc<AppState>,
) {
    set_history_view_state_for_tests(cx, view, state);
    cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| history.ensure_history_cache(cx));
        window.refresh();
        let _ = window.draw(app);
    });
    cx.run_until_parked();
}

fn commit(id: &str, parents: &[&str], summary: &str) -> Commit {
    Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: parents.iter().map(|p| CommitId((*p).into())).collect(),
        summary: summary.into(),
        author: "a".into(),
        time: SystemTime::UNIX_EPOCH,
    }
}

/// Anchor placement is the part of the plan that depends on repo data: a
/// dirty worktree earns a row only when its HEAD is a commit currently on
/// screen.
fn worktree_anchors_for(
    commits: &[Commit],
    worktrees: &[(&str, &str)],
    dirty_paths: &[&str],
) -> Vec<usize> {
    let visible = HistoryVisibleIndices::all(commits.len());
    let mut visible_ix_by_commit: FxHashMap<&str, usize> = FxHashMap::default();
    for (visible_ix, commit_ix) in visible.iter().enumerate() {
        visible_ix_by_commit
            .entry(commits[commit_ix].id.as_ref())
            .or_insert(visible_ix);
    }
    dirty_paths
        .iter()
        .filter_map(|path| {
            let head = worktrees.iter().find(|(p, _)| p == path)?.1;
            visible_ix_by_commit.get(head).copied()
        })
        .collect()
}

#[test]
fn a_dirty_worktree_anchors_to_its_head_commit() {
    let commits = vec![
        commit("c0", &["c1"], "newest"),
        commit("c1", &["c2"], "middle"),
        commit("c2", &[], "oldest"),
    ];
    let worktrees = [("/wt/a", "c1"), ("/wt/b", "c2")];
    assert_eq!(
        worktree_anchors_for(&commits, &worktrees, &["/wt/a"]),
        vec![1]
    );
    assert_eq!(
        worktree_anchors_for(&commits, &worktrees, &["/wt/a", "/wt/b"]),
        vec![1, 2]
    );
}

#[test]
fn a_worktree_whose_head_is_not_on_screen_gets_no_row() {
    let commits = vec![commit("c0", &["c1"], "newest"), commit("c1", &[], "older")];
    // `c9` is on a branch outside the current scope, or past the loaded page.
    let worktrees = [("/wt/offscreen", "c9")];
    assert!(
        worktree_anchors_for(&commits, &worktrees, &["/wt/offscreen"]).is_empty(),
        "a worktree with no visible HEAD must not be anchored anywhere"
    );
}

#[test]
fn a_clean_worktree_gets_no_row_even_though_it_is_listed() {
    let commits = vec![commit("c0", &[], "only")];
    let worktrees = [("/wt/clean", "c0")];
    // `dirty_paths` is the scan's output, which only ever lists dirty trees.
    assert!(worktree_anchors_for(&commits, &worktrees, &[]).is_empty());
}

/// The plan must place the rows the anchors describe, in log order.
#[test]
fn anchors_become_rows_above_their_commits() {
    let commits = vec![
        commit("c0", &["c1"], "newest"),
        commit("c1", &["c2"], "middle"),
        commit("c2", &[], "oldest"),
    ];
    let worktrees = [("/wt/a", "c2"), ("/wt/b", "c0")];
    let anchors = worktree_anchors_for(&commits, &worktrees, &["/wt/a", "/wt/b"]);
    let plan = HistoryListPlan::new(
        true,
        anchors
            .iter()
            .enumerate()
            .map(|(worktree_ix, &visible_ix)| HistoryWorktreeRowAnchor {
                visible_ix,
                worktree_ix,
            })
            .collect(),
    );

    // working tree row, wt/b above c0, c0, c1, wt/a above c2, c2
    assert_eq!(plan.list_len(3), 6);
    assert_eq!(plan.list_ix_for_visible(0), 2);
    assert_eq!(plan.list_ix_for_visible(2), 5);
    assert_eq!(plan.list_ix_for_worktree(1), Some(1));
    assert_eq!(plan.list_ix_for_worktree(0), Some(4));
}

fn all_columns_visible_drag_layout() -> HistoryColumnDragLayout {
    HistoryColumnDragLayout {
        show_graph: true,
        show_author: true,
        show_date: true,
        show_sha: true,
        branch_w: px(HISTORY_COL_BRANCH_PX),
        graph_w: px(HISTORY_COL_GRAPH_PX),
        author_w: px(HISTORY_COL_AUTHOR_PX),
        date_w: px(HISTORY_COL_DATE_PX),
        sha_w: px(HISTORY_COL_SHA_PX),
    }
}

fn branch(name: &str, target: &str) -> Branch {
    Branch {
        name: name.into(),
        target: CommitId(target.into()),
        upstream: None,
        divergence: None,
    }
}

fn remote_branch(remote: &str, name: &str, target: &str) -> RemoteBranch {
    RemoteBranch {
        remote: remote.into(),
        name: name.into(),
        target: CommitId(target.into()),
    }
}

fn log_page(commits: Vec<Commit>, next_cursor: Option<&str>) -> LogPage {
    LogPage {
        commits,
        next_cursor: next_cursor.map(|last_seen| LogCursor {
            last_seen: CommitId(last_seen.into()),
            resume_from: None,
            resume_token: None,
        }),
    }
}

/// The commit-id index the base cache carries agrees with the visible order it
/// was built from.
///
/// Its readers -- the worktree row anchors and the selected lane's colour --
/// look commits up during layout, and both used to scan the page instead. A
/// map that disagrees with `visible_indices` would anchor rows on the wrong
/// commits, so this pins the two together.
#[test]
fn the_base_cache_indexes_every_visible_commit_by_id() {
    let commits = vec![
        commit("c0", &["c1"], "newest"),
        commit("c1", &["c2"], "middle"),
        commit("c2", &[], "oldest"),
    ];
    let page = log_page(commits, None);
    let base = build_history_base_cache(
        HistoryBaseCacheRequest {
            repo_id: RepoId(1),
            history_scope: LogScope::AllBranches,
            log_fingerprint: 0,
            head_branch_rev: 0,
            detached_head_commit: None,
            head_branch_target: None,
            branches_rev: 0,
            remote_branches_rev: 0,
            stashes_rev: 0,
        },
        &page,
        AppTheme::worktree_dark(),
        None,
        &[],
        &[],
        &[],
    );

    for (visible_ix, commit_ix) in base.visible_indices.iter().enumerate() {
        let id = &page.commits[commit_ix].id;
        assert_eq!(
            base.visible_ix_by_commit.get(id).copied(),
            Some(visible_ix),
            "{id:?} should resolve to the row it renders at"
        );
    }
    assert_eq!(base.visible_ix_by_commit.len(), base.visible_indices.len());
    assert_eq!(
        base.visible_ix_by_commit.get(&CommitId("absent".into())),
        None
    );
}

/// A merge topology the selection tests share: `m` merged `feature` in.
///
/// ```text
/// m0 (merge m1 + f2)      row 0
/// m1                      row 1
/// f2 (feature tip)        row 2
/// m2                      row 3
/// f1                      row 4
/// base                    row 5
/// ```
fn merge_topology_commits() -> Vec<Commit> {
    vec![
        commit("m0", &["m1", "f2"], "merge feature into main"),
        commit("m1", &["m2"], "main one"),
        commit("f2", &["f1"], "feature two"),
        commit("m2", &["base"], "main two"),
        commit("f1", &["base"], "feature one"),
        commit("base", &[], "base"),
    ]
}

/// The merge topology as a built cache, for tests that inspect the graph.
fn merge_topology_cache() -> HistoryCache {
    let page = log_page(merge_topology_commits(), None);
    let base_request = HistoryBaseCacheRequest {
        repo_id: RepoId(1),
        history_scope: LogScope::AllBranches,
        log_fingerprint: 0,
        head_branch_rev: 0,
        detached_head_commit: None,
        head_branch_target: None,
        branches_rev: 0,
        remote_branches_rev: 0,
        stashes_rev: 0,
    };
    let base = build_history_base_cache(
        base_request.clone(),
        &page,
        AppTheme::worktree_dark(),
        None,
        &[],
        &[],
        &[],
    );
    let decorations = build_history_decoration_cache(
        HistoryDecorationCacheRequest {
            base_request,
            head_branch_rev: 0,
            detached_head_commit: None,
            branches_rev: 0,
            remote_branches_rev: 0,
            tags_rev: 0,
        },
        &page,
        &base,
        None,
        &[],
        &[],
        &[],
    );
    HistoryCache { base, decorations }
}

/// The whole point of the reachability walk: commits that arrived through
/// a merge belong to the branch even though they sit on a lane of their
/// own, and the lane-following highlight used to wash them out.
#[test]
fn merged_in_commits_belong_to_the_selected_branch() {
    let cache = merge_topology_cache();
    let highlight =
        build_selection_highlight(&cache, HistoryLaneAnchor::Commit(CommitId("m0".into())));
    let related = highlight.related_rows.expect("the anchor is on screen");

    // Every row is reachable from the merge, feature side included.
    assert!(
        related.iter().all(|lit| *lit),
        "the merge reaches every commit on the page: {related:?}"
    );

    // Sanity, from the graph itself: the feature rows really do sit on a
    // lane the selected lane does not cover, so this fixture genuinely
    // exercises the gap the walk closes.
    let theme = AppTheme::worktree_dark();
    let lane = highlight.lane.expect("the anchor's lane resolves");
    for row_ix in [2usize, 4usize] {
        let node_color_ix = cache.base.graph_rows[row_ix].node_color_ix;
        assert!(
            !lane.covers(theme, row_ix, node_color_ix),
            "row {row_ix} must be off the selected lane for this test to mean anything"
        );
    }
}

/// Anchoring on the branch tip keeps the other side's commits out: the
/// highlight is membership in one branch, not the whole page.
#[test]
fn selecting_the_feature_side_leaves_main_side_out() {
    let cache = merge_topology_cache();
    let highlight =
        build_selection_highlight(&cache, HistoryLaneAnchor::Commit(CommitId("f2".into())));
    let related = highlight.related_rows.expect("the anchor is on screen");

    // feature: f2, f1, base
    assert!(related[2] && related[4] && related[5]);
    // main's own commits and the merge are not on the feature branch
    assert!(!related[0] && !related[1] && !related[3]);
}

/// The walk itself: diamond parents, parents past the page, an anchor off
/// the end of the page.
#[test]
fn rows_reachable_from_follows_every_parent_link() {
    use smallvec::smallvec;
    // 0 → 1 → {3, 4} → 5, and 2 → 4: a diamond through 4 plus an
    // independent chain 0 → 1.
    let adjacency = vec![
        smallvec![1usize],
        smallvec![3usize, 4usize],
        smallvec![4usize],
        smallvec![5usize],
        smallvec![5usize],
        smallvec![],
    ];

    let related = rows_reachable_from(&adjacency, 0);
    assert_eq!(
        related.as_ref(),
        &[true, true, false, true, true, true],
        "the diamond is reachable, the unrelated chain is not"
    );

    // From the side entry, only its own chain lights.
    let related = rows_reachable_from(&adjacency, 2);
    assert_eq!(related.as_ref(), &[false, false, true, false, true, true]);

    // A parent index past the page (defensive: the adjacency is built from
    // the page, so this cannot happen, but the walk must not panic).
    let dangling = vec![smallvec![9usize]];
    assert_eq!(rows_reachable_from(&dangling, 0).as_ref(), &[true]);

    // An anchor off the page marks nothing.
    assert_eq!(rows_reachable_from(&adjacency, 42).as_ref(), &[false; 6]);
}

/// Branch attributed to each visible row, in row order.
fn lane_branch_labels(
    commits: Vec<Commit>,
    branches: &[Branch],
    remote_branches: &[RemoteBranch],
    head_branch: Option<&str>,
) -> Vec<Option<String>> {
    let page = log_page(commits, None);
    let base_request = HistoryBaseCacheRequest {
        repo_id: RepoId(1),
        history_scope: LogScope::AllBranches,
        log_fingerprint: 0,
        head_branch_rev: 0,
        detached_head_commit: None,
        head_branch_target: None,
        branches_rev: 0,
        remote_branches_rev: 0,
        stashes_rev: 0,
    };
    let base = build_history_base_cache(
        base_request.clone(),
        &page,
        AppTheme::worktree_dark(),
        head_branch,
        branches,
        remote_branches,
        &[],
    );
    let decorations = build_history_decoration_cache(
        HistoryDecorationCacheRequest {
            base_request,
            head_branch_rev: 0,
            detached_head_commit: None,
            branches_rev: 0,
            remote_branches_rev: 0,
            tags_rev: 0,
        },
        &page,
        &base,
        head_branch,
        branches,
        remote_branches,
        &[],
    );

    decorations
        .row_vms
        .iter()
        .map(|row| {
            row.lane_branch
                .and_then(|ix| decorations.branch_names.get(usize::from(ix)))
                .map(|name| name.to_string())
        })
        .collect()
}

#[test]
fn lane_branch_attribution_flows_down_from_the_branch_head() {
    // Only `feature` and `main` carry a ref; the commits below them inherit
    // the branch through their lane.
    let labels = lane_branch_labels(
        vec![
            commit("f2", &["f1"], "feature work"),
            commit("f1", &["base"], "feature start"),
            commit("m1", &["base"], "main work"),
            commit("base", &[], "base"),
        ],
        &[branch("feature", "f2"), branch("main", "m1")],
        &[],
        None,
    );

    assert_eq!(labels[0].as_deref(), Some("feature"));
    assert_eq!(labels[1].as_deref(), Some("feature"));
    assert_eq!(labels[2].as_deref(), Some("main"));
}

#[test]
fn a_feature_branch_parked_on_dev_does_not_claim_dev_s_history() {
    // The reported case: a freshly cut feature branch and `dev` point at the
    // very same commit, so nothing in the graph separates them. Attribution
    // has to prefer `dev`, or the whole history below is labelled with a
    // branch that has not added a single commit yet.
    let ref_items: Vec<HistoryRefListItem> = vec![
        HistoryRefListItem {
            text: HistoryTextVm::new("HEAD -> feat/thing".into()),
            kind: HistoryRefListItemKind::AttachedHead {
                branch: "feat/thing".to_string(),
            },
        },
        HistoryRefListItem {
            text: HistoryTextVm::new("dev".into()),
            kind: HistoryRefListItemKind::LocalBranch {
                name: "dev".to_string(),
            },
        },
    ];
    let tracked = FxHashSet::from_iter(["dev"]);
    assert_eq!(
        history_row_attribution_branch(&ref_items, &tracked),
        Some("dev")
    );

    // ...and it must still hold when the feature branch has been pushed, so
    // "is tracked" alone cannot separate them.
    let tracked = FxHashSet::from_iter(["dev", "feat/thing"]);
    assert_eq!(
        history_row_attribution_branch(&ref_items, &tracked),
        Some("dev")
    );
}

#[test]
fn attribution_prefers_a_pushed_branch_over_a_local_only_one() {
    // Neither is a conventional integration name, so the tie falls to the
    // branch whose history is actually shared.
    let ref_items: Vec<HistoryRefListItem> = vec![
        HistoryRefListItem {
            text: HistoryTextVm::new("scratch".into()),
            kind: HistoryRefListItemKind::LocalBranch {
                name: "scratch".to_string(),
            },
        },
        HistoryRefListItem {
            text: HistoryTextVm::new("release/24".into()),
            kind: HistoryRefListItemKind::LocalBranch {
                name: "release/24".to_string(),
            },
        },
    ];
    let tracked = FxHashSet::from_iter(["release/24"]);
    assert_eq!(
        history_row_attribution_branch(&ref_items, &tracked),
        Some("release/24")
    );

    // With nothing to separate them, the rendered order decides.
    let tracked = FxHashSet::default();
    assert_eq!(
        history_row_attribution_branch(&ref_items, &tracked),
        Some("scratch")
    );
}

#[test]
fn attribution_reads_origin_prefixed_remotes_as_their_branch() {
    let ref_items: Vec<HistoryRefListItem> = vec![
        HistoryRefListItem {
            text: HistoryTextVm::new("feat/thing".into()),
            kind: HistoryRefListItemKind::LocalBranch {
                name: "feat/thing".to_string(),
            },
        },
        HistoryRefListItem {
            text: HistoryTextVm::new("origin/dev".into()),
            kind: HistoryRefListItemKind::RemoteBranch {
                name: "origin/dev".to_string(),
            },
        },
    ];
    assert_eq!(
        history_row_attribution_branch(&ref_items, &FxHashSet::default()),
        Some("origin/dev")
    );
}

#[test]
fn dev_keeps_its_commits_however_the_feature_lane_is_drawn() {
    // The reported case: `feature` has diverged and its tip sits above dev's
    // in the log, so the lane that reaches the fork first is the feature's.
    // Containment has to win regardless -- every commit below the fork is
    // still in `dev`.
    let labels = lane_branch_labels(
        vec![
            commit("f2", &["f1"], "feature work"),
            commit("f1", &["base"], "feature start"),
            commit("d2", &["d1"], "dev work"),
            commit("d1", &["base"], "dev start"),
            commit("base", &["root"], "shared base"),
            commit("root", &[], "root"),
        ],
        &[branch("feature", "f2"), branch("dev", "d2")],
        &[],
        Some("feature"),
    );

    assert_eq!(labels[0].as_deref(), Some("feature"), "feature-only commit");
    assert_eq!(labels[1].as_deref(), Some("feature"), "feature-only commit");
    assert_eq!(labels[2].as_deref(), Some("dev"));
    assert_eq!(labels[3].as_deref(), Some("dev"));
    assert_eq!(labels[4].as_deref(), Some("dev"), "the fork point is dev's");
    assert_eq!(labels[5].as_deref(), Some("dev"), "and so is the root");
}

#[test]
fn dev_wins_even_when_its_tip_is_the_lower_row() {
    // The mirror ordering, which the previous "nearest branch head above"
    // rule got backwards.
    let labels = lane_branch_labels(
        vec![
            commit("d2", &["d1"], "dev work"),
            commit("d1", &["base"], "dev start"),
            commit("f2", &["f1"], "feature work"),
            commit("f1", &["base"], "feature start"),
            commit("base", &["root"], "shared base"),
            commit("root", &[], "root"),
        ],
        &[branch("feature", "f2"), branch("dev", "d2")],
        &[],
        Some("feature"),
    );

    assert_eq!(labels[2].as_deref(), Some("feature"));
    assert_eq!(labels[3].as_deref(), Some("feature"));
    assert_eq!(labels[4].as_deref(), Some("dev"), "the fork point is dev's");
    assert_eq!(labels[5].as_deref(), Some("dev"));
}

#[test]
fn shared_history_below_a_fork_is_attributed_to_the_base_branch() {
    // The reported shape: `feature` cut from `dev`, `dev` has moved on. Both
    // branches contain `base` and everything under it, and labelling those
    // rows with the checked-out feature branch reads as wrong -- they are
    // dev's history, which feature merely sits on top of.
    let labels = lane_branch_labels(
        vec![
            commit("f2", &["f1"], "feature work"),
            commit("f1", &["base"], "feature start"),
            commit("d2", &["d1"], "dev work"),
            commit("d1", &["base"], "dev start"),
            commit("base", &["root"], "shared base"),
            commit("root", &[], "root"),
        ],
        &[branch("feature", "f2"), branch("dev", "d2")],
        &[],
        Some("feature"),
    );

    assert_eq!(labels[0].as_deref(), Some("feature"));
    assert_eq!(labels[1].as_deref(), Some("feature"));
    assert_eq!(labels[2].as_deref(), Some("dev"));
    assert_eq!(labels[3].as_deref(), Some("dev"));
    // The fork point and everything below it belong to dev, not feature.
    assert_eq!(labels[4].as_deref(), Some("dev"));
    assert_eq!(labels[5].as_deref(), Some("dev"));
}

#[test]
fn lane_branch_attribution_reads_remote_branches_too() {
    let labels = lane_branch_labels(
        vec![
            commit("r2", &["r1"], "remote work"),
            commit("r1", &[], "remote start"),
        ],
        &[],
        &[remote_branch("origin", "topic", "r2")],
        None,
    );

    assert_eq!(labels[0].as_deref(), Some("origin/topic"));
    assert_eq!(labels[1].as_deref(), Some("origin/topic"));
}

#[test]
fn lane_branch_attribution_is_absent_without_any_branch_ref() {
    let labels = lane_branch_labels(
        vec![commit("c1", &["c0"], "one"), commit("c0", &[], "zero")],
        &[],
        &[],
        None,
    );

    assert!(labels.iter().all(Option::is_none));
}

#[test]
fn stash_tip_detection_requires_stash_like_message_and_multiple_parents() {
    assert!(is_probable_stash_tip(&commit(
        "s",
        &["p0", "p1"],
        "On main: quick stash"
    )));
    assert!(is_probable_stash_tip(&commit(
        "s",
        &["p0", "p1"],
        "WIP on main: quick stash"
    )));
    assert!(!is_probable_stash_tip(&commit(
        "c",
        &["p0"],
        "On main: normal commit"
    )));
    assert!(!is_probable_stash_tip(&commit(
        "c",
        &["p0", "p1"],
        "Regular summary"
    )));
}

#[test]
fn stash_summary_parser_extracts_tail_after_prefix() {
    assert_eq!(
        stash_summary_from_log_summary("On feature/x: savepoint"),
        Some("savepoint")
    );
    assert_eq!(
        stash_summary_from_log_summary("WIP on main: keep this"),
        Some("keep this")
    );
    assert_eq!(stash_summary_from_log_summary("no delimiter"), None);
}

#[test]
fn graph_branch_heads_are_hidden_for_current_branch_scope() {
    let branches = vec![branch("main", "local-head")];
    let remote_branches = vec![remote_branch("origin", "feature/x", "remote-head")];

    let mut current_branch_heads =
        graph_branch_heads(LogScope::CurrentBranch, &branches, &remote_branches);
    assert!(current_branch_heads.next().is_none());

    let all_branch_heads =
        graph_branch_heads(LogScope::AllBranches, &branches, &remote_branches).collect::<Vec<_>>();
    assert_eq!(all_branch_heads.len(), 2);
    assert!(all_branch_heads.contains(&"local-head"));
    assert!(all_branch_heads.contains(&"remote-head"));
}

#[test]
fn selected_branch_for_history_row_carries_branch_identity() {
    let selected_branch = SelectedBranch {
        repo_id: RepoId(7),
        section: BranchSection::Local,
        name: "main".into(),
    };

    assert_eq!(
        selected_branch_for_history_row(Some(&selected_branch), RepoId(7), true),
        Some(SelectedHistoryBranch {
            section: BranchSection::Local,
            name: "main".into(),
        })
    );
}

#[test]
fn selected_branch_for_history_row_keeps_the_remote_section() {
    let selected_branch = SelectedBranch {
        repo_id: RepoId(7),
        section: BranchSection::Remote,
        name: "origin/feature/topic".into(),
    };

    assert_eq!(
        selected_branch_for_history_row(Some(&selected_branch), RepoId(7), true),
        Some(SelectedHistoryBranch {
            section: BranchSection::Remote,
            name: "origin/feature/topic".into(),
        })
    );
}

#[test]
fn selected_branch_for_history_row_requires_selected_row_and_matching_repo() {
    let selected_branch = SelectedBranch {
        repo_id: RepoId(7),
        section: BranchSection::Local,
        name: "main".into(),
    };

    assert_eq!(
        selected_branch_for_history_row(Some(&selected_branch), RepoId(8), true),
        None
    );
    assert_eq!(
        selected_branch_for_history_row(Some(&selected_branch), RepoId(7), false),
        None
    );
}

#[test]
fn history_columns_available_width_reserves_scrollbar_gutter() {
    let gutter = history_scrollbar_gutter();
    assert_eq!(
        history_columns_available_width(px(200.0)),
        px(200.0) - gutter
    );
    assert_eq!(history_columns_available_width(gutter), px(0.0));
}

#[test]
fn history_column_drag_clamp_respects_static_maximums() {
    let available = history_columns_available_width(px(1436.0));
    let layout = all_columns_visible_drag_layout();
    let next = history_column_drag_clamped_width(
        HistoryColResizeHandle::Branch,
        px(900.0),
        available,
        layout,
        100,
    );
    assert_eq!(next, px(HISTORY_COL_BRANCH_MAX_PX));
}

#[test]
fn history_column_drag_clamp_preserves_message_space() {
    let available = history_columns_available_width(px(836.0));
    let layout = all_columns_visible_drag_layout();
    let next = history_column_drag_clamped_width(
        HistoryColResizeHandle::Branch,
        px(500.0),
        available,
        layout,
        100,
    );

    let next_f: f32 = next.into();
    assert!((next_f - 132.0).abs() < 1e-3);
}

#[test]
fn history_column_drag_clamp_never_goes_below_minimum() {
    let available = history_columns_available_width(px(1436.0));
    let layout = all_columns_visible_drag_layout();
    let next = history_column_drag_clamped_width(
        HistoryColResizeHandle::Sha,
        px(0.0),
        available,
        layout,
        100,
    );
    assert_eq!(next, px(HISTORY_COL_SHA_MIN_PX));
}

#[test]
fn history_column_widths_recompute_from_design_units_with_ui_scale_percent() {
    let widths = scaled_history_column_widths(
        default_history_column_design_widths(),
        ui_scale::UiScale::from_percent(200),
    );
    assert_eq!(
        widths,
        HistoryColumnWidths {
            branch: px(HISTORY_COL_BRANCH_PX * 2.0),
            graph: px(HISTORY_COL_GRAPH_PX * 2.0),
            author: px(HISTORY_COL_AUTHOR_PX * 2.0),
            date: px(HISTORY_COL_DATE_PX * 2.0),
            sha: px(HISTORY_COL_SHA_PX * 2.0),
        }
    );
}

#[test]
fn graph_drag_ignores_auto_hidden_optional_columns() {
    let available = history_columns_available_width(px(500.0));
    let widths = default_history_column_widths(100);
    let preferred = (true, true, true);

    assert_eq!(
        history_visible_columns_for_width(available, true, preferred, widths, 100),
        (false, false, false)
    );

    let next = history_column_drag_next_width(
        HistoryColResizeHandle::Graph,
        px(90.0),
        available,
        true,
        preferred,
        widths,
        100,
    );

    assert_eq!(next, px(90.0));
}

#[test]
fn reset_widths_clamp_default_graph_in_narrow_windows() {
    let widths = history_reset_widths_for_available_width(
        history_columns_available_width(px(396.0)),
        true,
        (true, true, true),
        100,
    );

    assert_eq!(widths.branch, px(116.0));
    assert_eq!(widths.graph, px(HISTORY_COL_GRAPH_MIN_PX));
}

#[test]
fn reset_widths_clamp_branch_after_graph_reaches_minimum() {
    let widths = history_reset_widths_for_available_width(
        history_columns_available_width(px(360.0)),
        true,
        (true, true, true),
        100,
    );

    assert_eq!(widths.graph, px(HISTORY_COL_GRAPH_MIN_PX));
    assert_eq!(widths.branch, px(80.0));
}

#[test]
fn history_resize_state_uses_actual_visible_columns_in_narrow_windows() {
    let available = history_columns_available_width(px(500.0));
    let layout = all_columns_visible_drag_layout();
    let state = history_column_resize_state(
        HistoryColResizeHandle::Graph,
        px(0.0),
        available,
        layout,
        100,
    );

    assert_eq!(
        history_resize_state_visible_columns(available, Some(&state)),
        Some((false, false, false))
    );
}

#[test]
fn history_resize_state_preserves_visible_columns_within_drag_bounds() {
    let available = history_columns_available_width(px(836.0));
    let layout = all_columns_visible_drag_layout();
    let state = history_column_resize_state(
        HistoryColResizeHandle::Graph,
        px(0.0),
        available,
        layout,
        100,
    );

    assert!(history_resize_state_preserves_visible_columns(
        available,
        layout,
        Some(&state)
    ));
    assert_eq!(
        history_visible_columns_for_layout_with_resize_state(available, layout, Some(&state), 100,),
        (true, true, true)
    );
}

#[test]
fn history_resize_state_visibility_fast_path_falls_back_for_out_of_bounds_layout() {
    let available = history_columns_available_width(px(836.0));
    let state = history_column_resize_state(
        HistoryColResizeHandle::Graph,
        px(0.0),
        available,
        all_columns_visible_drag_layout(),
        100,
    );
    let layout = HistoryColumnDragLayout {
        graph_w: px(140.0),
        ..all_columns_visible_drag_layout()
    };

    assert!(!history_resize_state_preserves_visible_columns(
        available,
        layout,
        Some(&state)
    ));
    assert_eq!(
        history_visible_columns_for_layout_with_resize_state(available, layout, Some(&state), 100,),
        history_visible_columns_for_layout(available, layout, 100)
    );
}

#[test]
fn history_resize_state_visible_columns_fast_path_rejects_stale_current_width() {
    let available = history_columns_available_width(px(836.0));
    let layout = all_columns_visible_drag_layout();
    let state = history_column_resize_state(
        HistoryColResizeHandle::Date,
        px(0.0),
        available,
        layout,
        100,
    );

    assert_eq!(
        history_resize_state_visible_columns_for_current_width(
            available,
            px(HISTORY_COL_DATE_PX),
            Some(&state),
        ),
        Some((true, true, true))
    );
    assert_eq!(
        history_resize_state_visible_columns_for_current_width(
            available,
            px(HISTORY_COL_DATE_PX + 1.0),
            Some(&state),
        ),
        None
    );
}

/// The whole focus rule for a sidebar worktree click, in one table.
#[test]
fn a_worktree_click_focuses_its_changes_or_the_commit_it_sits_on() {
    let head = CommitId("head-sha".into());

    // This tab's own changes are the pinned row at the top of the log.
    assert_eq!(
        worktree_reveal_target(true, true, Some(false), Some(head.clone())),
        WorktreeRevealTarget::WorkingTreeSummaryRow
    );
    // Clean, so there is no row -- land on what it is checked out at. No
    // fallback scope: the current worktree's HEAD is in scope by definition.
    assert_eq!(
        worktree_reveal_target(true, false, Some(false), Some(head.clone())),
        WorktreeRevealTarget::Commit {
            head: head.clone(),
            fallback_scope: None,
        }
    );
    // A linked worktree's changes live in a row of their own.
    assert_eq!(
        worktree_reveal_target(false, false, Some(true), Some(head.clone())),
        WorktreeRevealTarget::WorktreeRow {
            head: head.clone(),
            fallback_scope: Some(LogScope::AllBranches),
        }
    );
    // Clean linked worktree: its branch may sit outside the current scope.
    assert_eq!(
        worktree_reveal_target(false, false, Some(false), Some(head.clone())),
        WorktreeRevealTarget::Commit {
            head: head.clone(),
            fallback_scope: Some(LogScope::AllBranches),
        }
    );
}

/// The first scan has not replied when a repo opens, and "no answer yet" is
/// not the answer that the worktree is clean. Aiming at the commit on an
/// unknown fixes the reveal against a row set that is about to grow.
#[test]
fn an_unscanned_worktree_is_revealed_as_a_row_not_as_its_commit() {
    let head = CommitId("head-sha".into());
    assert_eq!(
        worktree_reveal_target(false, false, None, Some(head.clone())),
        WorktreeRevealTarget::WorktreeRow {
            head,
            fallback_scope: Some(LogScope::AllBranches),
        }
    );
}

/// The current worktree's own changes never appear as a linked-worktree row,
/// so a dirty *other* worktree must not divert this tab's click.
#[test]
fn the_current_worktree_ignores_other_worktrees_dirt() {
    let head = CommitId("head-sha".into());
    assert_eq!(
        worktree_reveal_target(true, true, Some(true), Some(head)),
        WorktreeRevealTarget::WorkingTreeSummaryRow
    );
}

#[test]
fn a_clean_worktree_with_no_resolvable_head_focuses_nothing() {
    assert_eq!(
        worktree_reveal_target(false, false, Some(false), None),
        WorktreeRevealTarget::Nothing
    );
    // Even a dirty one: its row is anchored by that same HEAD.
    assert_eq!(
        worktree_reveal_target(false, false, Some(true), None),
        WorktreeRevealTarget::Nothing
    );
}

/// Selecting a worktree row also leaves the commit selection empty, which is
/// the state the working-tree row uses to decide it is selected. Claiming
/// index 0 here is what made both rows light up at once.
#[test]
fn a_selected_worktree_row_does_not_claim_the_working_tree_row() {
    let plan = HistoryListPlan::new(true, Vec::new());
    let commits = vec![commit("aaa", &[], "tip")];
    let visible = HistoryVisibleIndices::all(1);

    let working_tree = peek_history_selected_list_index(
        None,
        RepoId(1),
        1,
        1,
        LogScope::AllBranches,
        &plan,
        HistorySelectionRef {
            commit: None,
            worktree_selected: false,
        },
        &visible,
        &commits,
    );
    assert_eq!(
        working_tree,
        Some(0),
        "with nothing else selected the working-tree row owns index 0"
    );

    let worktree = peek_history_selected_list_index(
        None,
        RepoId(1),
        1,
        1,
        LogScope::AllBranches,
        &plan,
        HistorySelectionRef {
            commit: None,
            worktree_selected: true,
        },
        &visible,
        &commits,
    );
    assert_eq!(
        worktree, None,
        "a selected worktree row must not report the working-tree row's index"
    );
}

#[test]
fn resolve_history_selected_list_index_populates_cache_for_commit_selection() {
    let commits = vec![
        commit("a", &["p0"], "a"),
        commit("b", &["a"], "b"),
        commit("c", &["b"], "c"),
    ];
    let selected = CommitId("c".into());
    let mut cache = None;

    let list_ix = resolve_history_selected_list_index(
        &mut cache,
        RepoId(7),
        11,
        13,
        LogScope::AllBranches,
        &HistoryListPlan::new(true, Vec::new()),
        HistorySelectionRef {
            commit: Some(&selected),
            worktree_selected: false,
        },
        &HistoryVisibleIndices::Filtered(vec![0, 2].into()),
        &commits,
    );

    assert_eq!(list_ix, Some(2));
    assert_eq!(
        cache,
        Some(HistorySelectedListIndexCache {
            repo_id: RepoId(7),
            log_rev: 11,
            stashes_rev: 13,
            history_scope: LogScope::AllBranches,
            show_working_tree_summary_row: true,
            plan_fingerprint: HistoryListPlan::new(true, Vec::new()).fingerprint(),
            selected_commit: Some(selected),
            list_ix: 2,
        })
    );
}

#[test]
fn resolve_history_selected_list_index_reuses_matching_cache() {
    let selected = CommitId("cached".into());
    let mut cache = Some(HistorySelectedListIndexCache {
        repo_id: RepoId(3),
        log_rev: 21,
        stashes_rev: 34,
        history_scope: LogScope::CurrentBranch,
        show_working_tree_summary_row: false,
        plan_fingerprint: HistoryListPlan::new(false, Vec::new()).fingerprint(),
        selected_commit: Some(selected.clone()),
        list_ix: 5,
    });

    let list_ix = resolve_history_selected_list_index(
        &mut cache,
        RepoId(3),
        21,
        34,
        LogScope::CurrentBranch,
        &HistoryListPlan::new(false, Vec::new()),
        HistorySelectionRef {
            commit: Some(&selected),
            worktree_selected: false,
        },
        &HistoryVisibleIndices::all(0),
        &[],
    );

    assert_eq!(list_ix, Some(5));
}

#[test]
fn pending_history_reveal_visible_target_scrolls_and_clears() {
    let commits = vec![
        commit("a", &["p0"], "a"),
        commit("b", &["a"], "b"),
        commit("c", &["b"], "c"),
    ];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId("c".into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        11,
        13,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::Filtered(vec![0, 2].into())),
        &HistoryListPlan::new(true, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: Some(CommitId("c".into())),
            scroll_to_list_ix: Some(2),
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn pending_history_reveal_missing_target_requests_load_more() {
    let commits = vec![commit("a", &["p0"], "a"), commit("b", &["a"], "b")];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId("c".into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        11,
        13,
        false,
        Some(&log_page(commits, Some("b"))),
        Some(true),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            // Selecting is `Msg::RevealCommit`'s job; a target that has not
            // been paged in yet is nobody's cue to touch the selection.
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: true,
            clear_pending: false,
        }
    );
}

#[test]
fn pending_history_reveal_switches_to_fallback_scope_after_exhausting_current_mode() {
    let commits = vec![commit("a", &["p0"], "a"), commit("b", &["a"], "b")];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId("c".into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        11,
        13,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: Some(LogScope::AllBranches),
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: false,
            clear_pending: false,
        }
    );
}

#[test]
fn pending_history_reveal_missing_target_with_exhausted_history_and_no_fallback_clears() {
    let commits = vec![commit("a", &["p0"], "a"), commit("b", &["a"], "b")];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId("c".into()),
        fallback_scope: None,
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        11,
        13,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn pending_history_reveal_already_selected_commit_still_scrolls() {
    let commits = vec![commit("a", &["p0"], "a"), commit("b", &["a"], "b")];
    let selected = CommitId("b".into());
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: selected.clone(),
        fallback_scope: None,
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        Some(&selected),
        21,
        34,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: None,
            scroll_to_list_ix: Some(1),
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn pending_history_reveal_unique_abbreviated_commit_scrolls_and_selects_full_id() {
    let full = "abcdef0123456789abcdef0123456789abcdef01";
    let other = "1234567890abcdef1234567890abcdef12345678";
    let commits = vec![
        commit(other, &["p0"], "other"),
        commit(full, &[other], "target"),
    ];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId(full[..8].into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: Some(CommitId(full.into())),
            scroll_to_list_ix: Some(1),
            load_more: false,
            clear_pending: true,
        }
    );
}

/// An abbreviation used to force loading the *entire* history before it
/// could be trusted as unambiguous. `Msg::RevealCommit` settles ambiguity
/// against the object database instead, so a visible match is taken at once
/// even with pages left to load.
#[test]
fn pending_history_reveal_abbreviated_commit_takes_a_visible_match_with_pages_left() {
    let full = "abcdef0123456789abcdef0123456789abcdef01";
    let other = "1234567890abcdef1234567890abcdef12345678";
    let commits = vec![
        commit(other, &["p0"], "other"),
        commit(full, &[other], "target"),
    ];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId(full[..8].into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        Some(&log_page(commits, Some("next"))),
        Some(true),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: Some(CommitId(full.into())),
            scroll_to_list_ix: Some(1),
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn pending_history_reveal_abbreviated_commit_waits_for_display_page_before_selecting() {
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId("abcdef01".into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        None,
        None,
        true,
        None,
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: false,
            clear_pending: false,
        }
    );
}

#[test]
fn pending_history_reveal_abbreviated_commit_waits_for_matching_cache_before_selecting() {
    let full = "abcdef0123456789abcdef0123456789abcdef01";
    let commits = vec![commit(full, &["p0"], "target")];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId(full[..8].into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        false,
        Some(&HistoryVisibleIndices::all(1)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: false,
            clear_pending: false,
        }
    );
}

#[test]
fn pending_history_reveal_uppercase_abbreviated_commit_scrolls_and_selects_full_id() {
    let full = "abcdef0123456789abcdef0123456789abcdef01";
    let other = "1234567890abcdef1234567890abcdef12345678";
    let commits = vec![
        commit(other, &["p0"], "other"),
        commit(full, &[other], "target"),
    ];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId(full[..8].to_ascii_uppercase().into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: Some(CommitId(full.into())),
            scroll_to_list_ix: Some(1),
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn pending_history_reveal_ambiguous_abbreviated_commit_clears_without_selecting() {
    let first = "abcdef0123456789abcdef0123456789abcdef01";
    let second = "abcdef0123456789abcdef0123456789abcdef02";
    let commits = vec![
        commit(first, &["p0"], "first"),
        commit(second, &["p0"], "second"),
    ];
    let pending = PendingHistoryReveal {
        worktree_path: None,
        repo_id: RepoId(7),
        commit_id: CommitId(first[..8].into()),
        fallback_scope: Some(LogScope::AllBranches),
    };

    let decision = decide_pending_history_reveal(
        &pending,
        Some(RepoId(7)),
        Some(LogScope::CurrentBranch),
        None,
        21,
        34,
        false,
        Some(&log_page(commits, None)),
        Some(false),
        true,
        Some(&HistoryVisibleIndices::all(2)),
        &HistoryListPlan::new(false, Vec::new()),
        None,
    );

    assert_eq!(
        decision,
        PendingHistoryRevealDecision {
            set_scope: None,
            select_commit: None,
            scroll_to_list_ix: None,
            load_more: false,
            clear_pending: true,
        }
    );
}

#[test]
fn display_log_page_uses_retained_page_while_loading() {
    let mut repo = RepoState::new_opening(
        RepoId(9),
        RepoSpec {
            workdir: "/tmp/repo".into(),
        },
    );
    let page = Arc::new(log_page(vec![commit("a", &[], "a")], None));
    repo.log = Loadable::Loading;
    repo.history_state.log = Loadable::Loading;
    repo.history_state.retained_log_while_loading = Some(Arc::clone(&page));

    let display = HistoryView::display_log_page_for_repo(&repo)
        .expect("retained log should remain available while loading");
    assert!(Arc::ptr_eq(&display, &page));
}

/// A worktree reveal scrolls to the worktree's own row, which sits one line
/// *above* the commit that located it. The selected-list-index cache it
/// writes is keyed on that commit, though, so it has to remember the
/// commit's row: caching the row we scrolled to hands the commit its
/// neighbour's index, and the first arrow step off that commit computes
/// `neighbour + 1` and lands back on the commit itself.
#[gpui::test]
fn a_worktree_reveal_caches_the_commits_row_not_the_worktree_row(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let worktree_path = PathBuf::from("/tmp/history-worktree-reveal/linked");
    let page = Arc::new(log_page(vec![commit("tip", &[], "tip")], None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-worktree-reveal"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    repo.branches_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;
    repo.worktree_dirty = Loadable::Ready(Arc::new(vec![
        worktree_core::domain::WorktreeDirtySummary {
            path: worktree_path.clone(),
            head: Some(CommitId("tip".into())),
            branch: Some("side".into()),
            detached: false,
            added: 1,
            modified: 0,
            deleted: 0,
            staged: Vec::new(),
            unstaged: Vec::new(),
        },
    ]));
    repo.worktree_dirty_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);
    wait_until(cx, "history cache for the worktree reveal", |cx| {
        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view
                .read(app)
                .history_cache
                .as_ref()
                .is_some_and(|cache| cache.base.row_vms.len() == 1)
        })
    });

    cx.update(|_window, app| {
        let history_view = view.read(app).main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            let plan = history.ensure_history_list_plan();
            let worktree_row_ix =
                worktree_row_list_ix(&plan, history.active_repo(), &worktree_path)
                    .expect("the dirty worktree should have a row");
            let commit_row_ix = plan.list_ix_for_visible(0);
            assert_eq!(
                commit_row_ix,
                worktree_row_ix + 1,
                "fixture must put the worktree row directly above its commit"
            );

            history.pending_history_reveal = Some(PendingHistoryReveal {
                worktree_path: Some(worktree_path.clone()),
                repo_id,
                commit_id: CommitId("tip".into()),
                fallback_scope: None,
            });
            history.drive_pending_history_reveal(cx);

            let cache = history
                .history_selected_list_index_cache
                .as_ref()
                .expect("the reveal should leave a list-index cache");
            assert_eq!(
                cache.selected_commit.as_ref().map(|id| id.as_ref()),
                Some("tip")
            );
            assert_eq!(
                cache.list_ix, commit_row_ix,
                "the cache is keyed on the commit, so it holds the commit's row"
            );
        });
    });
}

/// `list_ix_for_worktree` returns `None` once the worktree goes clean or its
/// HEAD leaves the loaded page, and a selected row with no index is not the
/// same as nothing being selected. Falling through to the no-selection arms
/// wrapped the selection to the far end of the log instead of moving it by
/// one, and the user lost their place.
#[gpui::test]
fn arrowing_off_a_worktree_row_with_no_index_does_not_jump_to_the_end(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let worktree_path = PathBuf::from("/tmp/history-worktree-nav/linked");
    let page = Arc::new(log_page(
        vec![commit("tip", &["base"], "tip"), commit("base", &[], "base")],
        None,
    ));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-worktree-nav"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    repo.branches_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;
    // Selected, but with no row: the scan that would list it has not landed,
    // which is exactly the state the reducer refuses to read as "clean".
    repo.history_state.worktree_selection = Some(worktree_path.clone());
    repo.worktree_dirty = Loadable::Ready(Arc::new(Vec::new()));
    repo.worktree_dirty_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);
    wait_until(cx, "history cache for the worktree nav", |cx| {
        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view
                .read(app)
                .history_cache
                .as_ref()
                .is_some_and(|cache| cache.base.row_vms.len() == 2)
        })
    });

    cx.update(|_window, app| {
        let history_view = view.read(app).main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            let plan = history.ensure_history_list_plan();
            assert!(
                worktree_row_list_ix(&plan, history.active_repo(), &worktree_path).is_none(),
                "fixture must leave the selected worktree without a row"
            );

            assert!(
                !history.history_select_adjacent_commit(-1, cx),
                "there is nothing to step from, so the key is not handled"
            );
            assert!(
                history
                    .active_repo()
                    .is_none_or(|repo| repo.history_state.selected_commit.is_none()),
                "and nothing at the far end of the log may be selected in its place"
            );
        });
    });
}

/// The commit set never changes here -- only the stash list does -- so the
/// log fingerprint is identical across both halves of this test. That is the
/// point: the plan's anchors are `visible_ix_by_commit` lookups, and that map
/// is renumbered when stash helper commits are filtered out of the page. A
/// plan cache keyed on the fingerprint alone hands back the pre-filter
/// indices, which puts every worktree row above the wrong commit and leaves a
/// blank gap wherever the stale index ran past the end of `graph_rows`.
#[gpui::test]
fn a_stash_list_arriving_replans_the_worktree_rows(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let worktree_path = PathBuf::from("/tmp/history-stash-replan/linked");
    // `helper` is the stash's second parent, so it disappears from the page
    // once the stash list names `wip` as a stash tip. `base` -- the commit the
    // worktree is anchored on -- moves up a row when it does.
    let page = Arc::new(log_page(
        vec![
            commit("wip", &["base", "helper"], "stash push"),
            commit("helper", &["base"], "index on main"),
            commit("base", &[], "base"),
        ],
        None,
    ));

    let state_with_stashes = |stashes: Vec<StashEntry>, stashes_rev: u64| {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/history-stash-replan"),
            },
        );
        repo.history_state.history_scope = LogScope::AllBranches;
        repo.head_branch = Loadable::Ready("main".to_string());
        repo.head_branch_rev = 1;
        repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "wip")]));
        repo.branches_rev = 1;
        repo.log = Loadable::Ready(Arc::clone(&page));
        repo.log_rev = 1;
        repo.history_state.log = Loadable::Ready(Arc::clone(&page));
        repo.history_state.log_rev = 1;
        repo.stashes = Loadable::Ready(Arc::new(stashes));
        repo.stashes_rev = stashes_rev;
        repo.worktree_dirty = Loadable::Ready(Arc::new(vec![
            worktree_core::domain::WorktreeDirtySummary {
                path: worktree_path.clone(),
                head: Some(CommitId("base".into())),
                branch: Some("side".into()),
                detached: false,
                added: 1,
                modified: 0,
                deleted: 0,
                staged: Vec::new(),
                unstaged: Vec::new(),
            },
        ]));
        repo.worktree_dirty_rev = 1;
        Arc::new(AppState {
            repos: vec![repo],
            active_repo: Some(RepoId(1)),
            ..Default::default()
        })
    };

    /// The visible row `base` renders on, and the list row its worktree
    /// sits on, read back after the cache has settled at `visible_len` rows.
    fn anchored_rows(
        cx: &mut gpui::VisualTestContext,
        view: &gpui::Entity<WorkTreeView>,
        visible_len: usize,
    ) -> (usize, usize, usize) {
        wait_until(cx, "history cache to match the stash list", |cx| {
            cx.update(|_window, app| {
                let history_view = view.read(app).main_pane.read(app).history_view.clone();
                history_view
                    .read(app)
                    .history_cache
                    .as_ref()
                    .is_some_and(|cache| cache.base.row_vms.len() == visible_len)
            })
        });

        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view.update(app, |history, _cx| {
                let plan = history.ensure_history_list_plan();
                let base_visible_ix = history
                    .history_cache
                    .as_ref()
                    .expect("cache")
                    .base
                    .visible_ix_by_commit
                    .get(&CommitId("base".into()))
                    .copied()
                    .expect("the anchored commit is on screen");
                (
                    base_visible_ix,
                    plan.list_ix_for_worktree(0)
                        .expect("the dirty worktree keeps its row"),
                    plan.list_ix_for_visible(base_visible_ix),
                )
            })
        })
    }

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, state_with_stashes(Vec::new(), 1));
    let (before_visible_ix, before_worktree_ix, before_commit_ix) = anchored_rows(cx, &view, 3);
    assert_eq!(
        before_visible_ix, 2,
        "with no stashes every commit is on screen"
    );
    assert_eq!(
        before_worktree_ix + 1,
        before_commit_ix,
        "the worktree row sits directly above the commit it is anchored on"
    );

    ensure_history_cache_for_tests(
        cx,
        &view,
        state_with_stashes(
            vec![StashEntry {
                index: 0,
                id: CommitId("wip".into()),
                message: "WIP on main: base".into(),
                created_at: None,
            }],
            2,
        ),
    );
    let (after_visible_ix, after_worktree_ix, after_commit_ix) = anchored_rows(cx, &view, 2);
    assert_eq!(
        after_visible_ix, 1,
        "the stash helper commit must have been filtered out of the page"
    );
    assert_eq!(
        after_worktree_ix + 1,
        after_commit_ix,
        "the replanned worktree row must follow its commit up the renumbered page"
    );
}

/// The lane colour is read out of `graph_rows`, which `force_branch_head_lane`
/// reshapes whenever the branch list changes -- again without touching the log
/// fingerprint. A fingerprint-keyed memo keeps saturating whichever lane held
/// that colour index before the branch appeared.
#[gpui::test]
fn a_new_branch_recolours_the_selected_lane(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    // `behind` sits on the main lane. Pointing a branch at it makes
    // `force_branch_head_lane` fork a whisker lane for the head, and that fork
    // takes a palette slot -- so `other`, whose lane is born on the row *below*
    // it, draws a different colour than it did before the branch existed.
    let page = Arc::new(log_page(
        vec![
            commit("tip", &["behind"], "tip"),
            commit("behind", &["base"], "behind"),
            commit("other", &["base"], "other"),
            commit("base", &[], "base"),
        ],
        None,
    ));

    let state_with_branches = |branches: Vec<Branch>, branches_rev: u64| {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/history-lane-recolour"),
            },
        );
        repo.history_state.history_scope = LogScope::AllBranches;
        repo.head_branch = Loadable::Ready("main".to_string());
        repo.head_branch_rev = 1;
        repo.branches = Loadable::Ready(Arc::new(branches));
        repo.branches_rev = branches_rev;
        repo.log = Loadable::Ready(Arc::clone(&page));
        repo.log_rev = 1;
        repo.history_state.log = Loadable::Ready(Arc::clone(&page));
        repo.history_state.log_rev = 1;
        repo.history_state.selected_commit = Some(CommitId("other".into()));
        Arc::new(AppState {
            repos: vec![repo],
            active_repo: Some(RepoId(1)),
            ..Default::default()
        })
    };

    fn selected_lane_colour(
        cx: &mut gpui::VisualTestContext,
        view: &gpui::Entity<WorkTreeView>,
    ) -> (
        Option<crate::view::rows::history_graph_paint::SelectedLane>,
        Option<crate::view::rows::history_graph_paint::SelectedLane>,
    ) {
        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view.update(app, |history, _cx| {
                let memoised = history.history_selected_lane(false);
                // The same answer computed from scratch. The memo is the only
                // thing that can make these two disagree.
                history.history_selected_lane_color_cache = None;
                let fresh = history.history_selected_lane(false);
                (memoised, fresh)
            })
        })
    }

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(
        cx,
        &view,
        state_with_branches(vec![branch("main", "tip")], 1),
    );
    wait_until(cx, "history cache for the unbranched graph", |cx| {
        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view
                .read(app)
                .history_cache
                .as_ref()
                .is_some_and(|cache| cache.base.request.branches_rev == 1)
        })
    });
    let (before, before_fresh) = selected_lane_colour(cx, &view);
    assert_eq!(before, before_fresh, "the memo must start out agreeing");
    let before = before.expect("the selected commit is on a lane");

    ensure_history_cache_for_tests(
        cx,
        &view,
        state_with_branches(vec![branch("main", "tip"), branch("behind", "behind")], 2),
    );
    wait_until(cx, "history cache for the branched graph", |cx| {
        cx.update(|_window, app| {
            let history_view = view.read(app).main_pane.read(app).history_view.clone();
            history_view
                .read(app)
                .history_cache
                .as_ref()
                .is_some_and(|cache| cache.base.request.branches_rev == 2)
        })
    });
    let (after, after_fresh) = selected_lane_colour(cx, &view);
    let after_fresh = after_fresh.expect("the selected commit is still on a lane");
    assert_ne!(
        before.color_ix, after_fresh.color_ix,
        "fixture must actually recolour the selected lane, or this test proves \
         nothing about the memo"
    );
    assert_eq!(
        after,
        Some(after_fresh),
        "the memo must be reissued when the graph it read is rebuilt"
    );
}

#[gpui::test]
fn date_time_changes_reuse_history_cache_and_rows_still_render(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let page = Arc::new(log_page(vec![commit("tip", &[], "tip")], None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-date-time-reuse"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    repo.branches_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "initial history cache for date-time reuse", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.row_vms.len() == 1
                    && cache.base.row_vms[0].summary.as_ref() == "tip"
                    && cache.decorations.row_vms.len() == 1
            })
        })
    });

    let (before_graph_rows, before_base_request, before_decoration_request, before_when_text) = cx
        .update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let rows_len = history_view.update(app, |history, cx| {
                HistoryView::render_history_table_rows(history, 0..1, window, cx).len()
            });
            assert_eq!(rows_len, 1, "initial history row should render");

            let history = history_view.read(app);
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache.decorations.request.clone(),
                cache.base.row_vms[0]
                    .when
                    .resolve(HistoryDisplayKey::new(
                        DateTimeFormat::YmdHm,
                        Timezone::Utc,
                        true,
                        false,
                    ))
                    .as_ref()
                    .to_owned(),
            )
        });

    assert_eq!(
        before_when_text,
        format_datetime(
            SystemTime::UNIX_EPOCH,
            DateTimeFormat::YmdHm,
            Timezone::Utc,
            true,
        )
    );

    cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            history.set_date_time_format(DateTimeFormat::MdyHm, cx);
            history.ensure_history_cache(cx);
            let rows = HistoryView::render_history_table_rows(history, 0..1, window, cx);
            assert_eq!(
                rows.len(),
                1,
                "history row should still render after date change"
            );
        });
        window.refresh();
        let _ = window.draw(app);
    });
    cx.run_until_parked();

    let (after_graph_rows, after_base_request, after_decoration_request, after_when_text) = cx
        .update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            assert!(
                history.history_cache_inflight.is_none(),
                "display-only changes should not enqueue a cache rebuild"
            );
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should still be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache.decorations.request.clone(),
                cache.base.row_vms[0]
                    .when
                    .resolve(HistoryDisplayKey::new(
                        DateTimeFormat::MdyHm,
                        Timezone::Utc,
                        true,
                        false,
                    ))
                    .as_ref()
                    .to_owned(),
            )
        });

    assert!(
        Arc::ptr_eq(&before_graph_rows, &after_graph_rows),
        "date/time changes should keep the heavy graph cache"
    );
    assert_eq!(after_base_request, before_base_request);
    assert_eq!(after_decoration_request, before_decoration_request);
    assert_eq!(
        after_when_text,
        format_datetime(
            SystemTime::UNIX_EPOCH,
            DateTimeFormat::MdyHm,
            Timezone::Utc,
            true,
        )
    );
    assert_ne!(after_when_text, before_when_text);
}

#[gpui::test]
fn history_refs_hover_lists_refs_and_opens_item_menus(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commit_id = CommitId("tip".into());
    let base_commit_id = CommitId("base".into());
    let page = Arc::new(log_page(
        vec![
            commit("tip", &[base_commit_id.as_ref()], "tip"),
            commit(base_commit_id.as_ref(), &[], "base"),
        ],
        None,
    ));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.branches = Loadable::Ready(Arc::new(vec![
        branch("main", "tip"),
        branch("feature", "tip"),
    ]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(vec![remote_branch("origin", "main", "tip")]));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(vec![
        worktree_core::domain::Tag {
            name: "release".to_string(),
            target: commit_id.clone(),
            created_at: None,
        },
        worktree_core::domain::Tag {
            name: "old-release".to_string(),
            target: base_commit_id.clone(),
            created_at: None,
        },
    ]));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history row with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some()
    });
    wait_until(cx, "history second row with displayed refs", |cx| {
        cx.debug_bounds("history_row_1").is_some()
    });

    let redraw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, app| {
            let _ = window.draw(app);
        });
    };

    let refs_column_point = |cx: &mut gpui::VisualTestContext, row_ix: usize| {
        let selector = match row_ix {
            0 => "history_row_0",
            1 => "history_row_1",
            _ => panic!("unsupported row index {row_ix}"),
        };
        let row = cx
            .debug_bounds(selector)
            .expect("history row should be rendered");
        point(row.left() + px(24.0), row.center().y)
    };

    let away_from_refs_column_point = |cx: &mut gpui::VisualTestContext| {
        let row = cx
            .debug_bounds("history_row_0")
            .expect("history row should be rendered");
        point(row.right() - px(8.0), row.center().y)
    };

    let move_to_refs_column = |cx: &mut gpui::VisualTestContext| {
        let point = refs_column_point(cx, 0);
        cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
        cx.run_until_parked();
        redraw(cx);
    };

    let open_refs_hover = |cx: &mut gpui::VisualTestContext| {
        move_to_refs_column(cx);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        redraw(cx);
    };

    move_to_refs_column(cx);
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
    cx.update(|_window, app| {
        assert!(!crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });

    let away = away_from_refs_column_point(cx);
    cx.simulate_mouse_move(away, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    redraw(cx);
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
    cx.update(|_window, app| {
        assert!(!crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });

    open_refs_hover(cx);
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
    });

    let feature_center = cx
        .debug_bounds("history_refs_hover_item_local_branch_feature")
        .expect("expected feature ref item in debug bounds")
        .center();
    cx.simulate_mouse_move(feature_center, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    redraw(cx);
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
    });

    let click_hover_item =
        |cx: &mut gpui::VisualTestContext, selector: &'static str, button: gpui::MouseButton| {
            let center = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("expected {selector} in debug bounds"))
                .center();
            cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
            cx.simulate_mouse_down(center, button, gpui::Modifiers::default());
            cx.simulate_mouse_up(center, button, gpui::Modifiers::default());
            cx.run_until_parked();
            redraw(cx);
        };

    click_hover_item(
        cx,
        "history_refs_hover_item_local_branch_feature",
        gpui::MouseButton::Left,
    );
    let feature_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::BranchMenu {
                repo_id,
                section: BranchSection::Local,
                name: "feature".to_string(),
            })
        );
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            feature_pinned_ix
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("feature".into())
        );
    });

    click_hover_item(
        cx,
        "history_refs_hover_item_tag_release",
        gpui::MouseButton::Left,
    );
    let release_left_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::TagRefMenu {
                repo_id,
                commit_id: commit_id.clone(),
                name: "release".to_string()
            })
        );
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            release_left_pinned_ix
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("release".into())
        );
    });

    cx.update(|_window, app| {
        let popover_host = view.read(app).popover_host.clone();
        popover_host.update(app, |host, cx| host.close_popover(cx));
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
    });

    open_refs_hover(cx);
    click_hover_item(
        cx,
        "history_refs_hover_item_local_branch_feature",
        gpui::MouseButton::Right,
    );
    let feature_context_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::BranchMenu {
                repo_id,
                section: BranchSection::Local,
                name: "feature".to_string(),
            })
        );
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            feature_context_pinned_ix
        );
    });

    click_hover_item(
        cx,
        "history_refs_hover_item_tag_release",
        gpui::MouseButton::Right,
    );
    let release_context_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::TagRefMenu {
                repo_id,
                commit_id: commit_id.clone(),
                name: "release".to_string()
            })
        );
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            release_context_pinned_ix
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("release".into())
        );
    });

    cx.update(|_window, app| {
        let popover_host = view.read(app).popover_host.clone();
        popover_host.update(app, |host, cx| host.close_popover(cx));
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
    });

    open_refs_hover(cx);
    click_hover_item(
        cx,
        "history_refs_hover_item_tag_release",
        gpui::MouseButton::Left,
    );
    let release_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::TagRefMenu {
                repo_id,
                commit_id: commit_id.clone(),
                name: "release".to_string()
            })
        );
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            release_pinned_ix
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("release".into())
        );
    });

    cx.update(|_window, app| {
        let popover_host = view.read(app).popover_host.clone();
        popover_host.update(app, |host, cx| host.close_popover(cx));
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
    });

    open_refs_hover(cx);
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    let source_bounds = cx
        .update(|_window, app| {
            crate::view::test_support::history_refs_hover_source_bounds(view.read(app), app)
        })
        .expect("history refs hover should expose source bounds");
    click_hover_item(
        cx,
        "history_refs_hover_item_local_branch_feature",
        gpui::MouseButton::Right,
    );
    let frozen_feature_pinned_ix = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app)
    });
    let frozen_source_bounds = cx
        .update(|_window, app| {
            assert_eq!(
                crate::view::test_support::popover_kind(view.read(app), app),
                Some(PopoverKind::BranchMenu {
                    repo_id,
                    section: BranchSection::Local,
                    name: "feature".to_string(),
                })
            );
            assert_eq!(
                crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
                frozen_feature_pinned_ix
            );
            assert_eq!(
                crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
                Some("feature".into())
            );
            crate::view::test_support::history_refs_hover_source_bounds(view.read(app), app)
        })
        .expect("history refs hover should remain open while menu is open");

    let other_commit_ref_point = refs_column_point(cx, 1);
    cx.simulate_mouse_move(other_commit_ref_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.run_until_parked();
    redraw(cx);
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::BranchMenu {
                repo_id,
                section: BranchSection::Local,
                name: "feature".to_string(),
            })
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_source_bounds(view.read(app), app),
            Some(frozen_source_bounds)
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            frozen_feature_pinned_ix
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("feature".into())
        );
    });

    cx.update(|_window, app| {
        let popover_host = view.read(app).popover_host.clone();
        popover_host.update(app, |host, cx| host.close_popover(cx));
    });
    cx.run_until_parked();
    redraw(cx);
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_ix(view.read(app), app),
            None
        );
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            None
        );
    });

    let row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let away_x = if source_bounds.right() + px(8.0) < row.right() {
        source_bounds.right() + px(8.0)
    } else {
        source_bounds.left() - px(8.0)
    };
    let away = point(away_x, source_bounds.center().y);
    assert!(!source_bounds.contains(&away));
    cx.simulate_mouse_move(away, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(!hover_open, "history refs hover host should be closed");
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
    cx.update(|_window, app| {
        assert!(!crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });
}

/// Commit rows open their hover and context menu from window-level mouse
/// listeners, which run for every event no matter what is painted over the
/// history. They must therefore defer to the hit test: a click that landed
/// on the collapsed sidebar's popover — or on the scrim that dismisses it —
/// belongs to that popover, not to the row it happens to cover.
#[gpui::test]
fn history_row_selection_follows_the_press_not_the_release(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let store_for_assert = store.clone();
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let repo_path = PathBuf::from(format!(
        "/tmp/history-press-selects-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let commits = (0..12)
        .map(|ix| {
            let id = format!("c{ix:02}");
            commit(&id, &[], &format!("commit {ix:02}"))
        })
        .collect::<Vec<_>>();
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    // Everything the panes read is already loaded, so rendering never has
    // to ask the store (and its worker threads) for data.
    repo.open = Loadable::Ready(());
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c00")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.worktrees = Loadable::Ready(Arc::new(Vec::new()));
    repo.submodules = Loadable::Ready(Arc::new(Vec::new()));
    repo.stashes = Loadable::Ready(Arc::new(Vec::new()));
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    // The rows dispatch into the store, so it has to hold the same repo the
    // view renders; the reducer thread mutates exactly this state.
    store_for_assert.replace_snapshot_for_test(Arc::clone(&state));
    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);
    wait_until(cx, "history rows", |cx| {
        cx.debug_bounds("history_row_3").is_some()
    });

    let selected = |store: &AppStore| {
        store
            .snapshot()
            .repos
            .iter()
            .find(|repo| repo.id == repo_id)
            .and_then(|repo| repo.history_state.selected_commit.clone())
    };
    let row = |cx: &mut gpui::VisualTestContext, selector: &'static str| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("expected {selector} to be rendered"))
            .center()
    };

    // Positive control: an ordinary click selects, and the dispatch really
    // does reach the store, so the assertions below are not vacuous.
    let row_3 = row(cx, "history_row_3");
    cx.simulate_mouse_move(row_3, None, gpui::Modifiers::default());
    cx.simulate_click(row_3, gpui::Modifiers::default());
    wait_until(cx, "row 3 selected by a click", |_cx| {
        selected(&store_for_assert) == Some(CommitId("c03".into()))
    });

    // Press on one row, release on another: the press decides.
    let row_1 = row(cx, "history_row_1");
    let row_5 = row(cx, "history_row_5");
    cx.simulate_mouse_move(row_1, None, gpui::Modifiers::default());
    cx.simulate_mouse_down(row_1, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(row_5, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(row_5, gpui::MouseButton::Left, gpui::Modifiers::default());

    wait_until(cx, "row 1 selected by the press", |_cx| {
        selected(&store_for_assert) == Some(CommitId("c01".into()))
    });
    // A release-driven selection would have been queued before this point,
    // so a short settle is enough to prove none was.
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(10));
        cx.run_until_parked();
        assert_eq!(
            selected(&store_for_assert),
            Some(CommitId("c01".into())),
            "releasing over another row must not move the selection"
        );
    }
}

#[gpui::test]
fn history_rows_ignore_clicks_that_landed_on_the_collapsed_sidebar_popover(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commits = (0..12)
        .map(|ix| {
            let id = format!("c{ix:02}");
            commit(&id, &[], &format!("commit {ix:02}"))
        })
        .collect::<Vec<_>>();
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-collapsed-popover-clicks"),
        },
    );
    // Everything the sidebar reads is already loaded, so opening a section
    // popover never has to ask the store (and its worker threads) for data.
    repo.open = Loadable::Ready(());
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c00")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.worktrees = Loadable::Ready(Arc::new(Vec::new()));
    repo.submodules = Loadable::Ready(Arc::new(Vec::new()));
    repo.stashes = Loadable::Ready(Arc::new(Vec::new()));
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);
    wait_until(cx, "history rows", |cx| {
        cx.debug_bounds("history_row_3").is_some()
    });

    // Draw only: every step here is synchronous, and pumping the executor
    // (or advancing the clock) would let store background work race the
    // deliberately deterministic test scheduler.
    let settle = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, app| {
            let _ = window.draw(app);
        });
    };
    let right_click = |cx: &mut gpui::VisualTestContext, at: Point<Pixels>| {
        cx.simulate_mouse_move(at, None, gpui::Modifiers::default());
        cx.simulate_mouse_down(at, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.simulate_mouse_up(at, gpui::MouseButton::Right, gpui::Modifiers::default());
        settle(cx);
    };

    cx.update(|_window, app| {
        view.update(app, |this, cx| {
            this.set_sidebar_collapsed(true, cx);
            this.open_sidebar_collapsed_popover(
                crate::view::panes::sidebar::CollapsedSidebarSection::Local,
                cx,
            );
        });
    });
    settle(cx);
    settle(cx);

    let panel = cx
        .debug_bounds("collapsed_sidebar_popover")
        .expect("expected the collapsed sidebar popover");
    let row = cx
        .debug_bounds("history_row_3")
        .expect("history row should be rendered");

    // Right of the popover, over the dismiss scrim, on a commit row: the
    // click dismisses the popover and stops there. That it dismisses at all
    // is what proves the event reached this point, so a silent commit menu
    // cannot be mistaken for nothing having been clicked.
    let on_scrim = point(panel.right() + px(120.0), row.center().y);
    assert!(
        row.contains(&on_scrim),
        "expected the test point to sit on a commit row (row={row:?}, point={on_scrim:?})"
    );
    right_click(cx, on_scrim);

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            None,
            "dismissing the popover must not open the commit menu underneath it"
        );
        assert_eq!(
            view.read(app).sidebar_collapsed_popover,
            None,
            "the click must still dismiss the popover"
        );
    });
}

#[gpui::test]
fn history_refs_hover_closes_when_history_scrolls_without_mouse_move(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commits = (0..80)
        .map(|ix| {
            let id = format!("c{ix:02}");
            commit(&id, &[], &format!("commit {ix:02}"))
        })
        .collect::<Vec<_>>();
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-scroll"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c00")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history row with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some()
    });

    let row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let hover_point = point(row.left() + px(24.0), row.center().y);
    cx.simulate_mouse_move(hover_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });

    let scroll_y = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_scroll.0.borrow().base_handle.offset().y
        })
    };
    let before_scroll_y = scroll_y(cx);
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: hover_point,
        delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-240.0))),
        ..Default::default()
    });
    cx.run_until_parked();
    wait_until(cx, "history list to scroll", |cx| {
        scroll_y(cx) != before_scroll_y
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(
        !hover_open,
        "history refs hover should close when history scrolls without a mouse move"
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
}

#[gpui::test]
fn history_refs_hover_does_not_open_while_overlay_is_open(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let page = Arc::new(log_page(vec![commit("c00", &[], "commit 00")], None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-overlay"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c00")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history row with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some()
    });

    let row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let refs_column_point = point(row.left() + px(24.0), row.center().y);

    // Open a context menu (an overlay) via right-click, away from the refs column.
    let menu_point = point(row.right() - px(8.0), row.center().y);
    cx.simulate_mouse_down(
        menu_point,
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        menu_point,
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    cx.update(|_window, app| {
        assert!(
            crate::view::test_support::popover_is_open(view.read(app), app),
            "right-click should have opened a context menu overlay"
        );
    });

    // Hovering the refs column while the overlay is open must not open the hover:
    // the history canvas handles mouse-move at the window level, so it still fires
    // under the overlay, but the trigger is now guarded.
    cx.simulate_mouse_move(refs_column_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(
        !hover_open,
        "history refs hover must not open while an overlay is open on top of it"
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
}

#[gpui::test]
fn history_refs_hover_closes_when_click_selects_another_commit_without_mouse_move(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let commits = vec![
        commit("c1", &["c0"], "commit 1"),
        commit("c0", &[], "commit 0"),
    ];
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-click-close"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c1")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history rows with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some() && cx.debug_bounds("history_row_1").is_some()
    });

    let hover_row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let hover_point = point(hover_row.left() + px(24.0), hover_row.center().y);
    cx.simulate_mouse_move(hover_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });

    let other_row = cx
        .debug_bounds("history_row_1")
        .expect("second history row should be rendered");
    let click_point = point(other_row.right() - px(8.0), other_row.center().y);
    cx.simulate_mouse_down(
        click_point,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        click_point,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(
        !hover_open,
        "history refs hover should close when another commit is clicked without a mouse move"
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
}

#[gpui::test]
fn history_refs_hover_item_click_keeps_existing_history_selection(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let selected_commit = CommitId("c0".into());
    let hovered_commit = CommitId("c1".into());
    let commits = vec![
        commit(
            hovered_commit.as_ref(),
            &[selected_commit.as_ref()],
            "commit 1",
        ),
        commit(selected_commit.as_ref(), &[], "commit 0"),
    ];
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-selection-priority"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.history_state.selected_commit = Some(selected_commit.clone());
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.branches = Loadable::Ready(Arc::new(vec![
        branch("main", hovered_commit.as_ref()),
        branch("feature", hovered_commit.as_ref()),
    ]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history rows with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some() && cx.debug_bounds("history_row_1").is_some()
    });

    let hover_row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let hover_point = point(hover_row.left() + px(24.0), hover_row.center().y);
    cx.simulate_mouse_move(hover_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert_eq!(
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history
                .active_repo()
                .and_then(|repo| repo.history_state.selected_commit.clone())
        }),
        Some(selected_commit.clone())
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());

    let item_center = cx
        .debug_bounds("history_refs_hover_item_local_branch_feature")
        .expect("expected feature ref item in debug bounds")
        .center();
    cx.simulate_mouse_move(item_center, None, gpui::Modifiers::default());
    cx.simulate_mouse_down(
        item_center,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        item_center,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::BranchMenu {
                repo_id,
                section: BranchSection::Local,
                name: "feature".to_string(),
            })
        );
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::history_refs_hover_pinned_item_text(view.read(app), app),
            Some("feature".into())
        );
    });
    assert_eq!(
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history
                .active_repo()
                .and_then(|repo| repo.history_state.selected_commit.clone())
        }),
        Some(selected_commit)
    );
}

#[gpui::test]
fn history_refs_hover_and_item_menu_close_when_history_page_changes_without_mouse_move(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let base_commit_id = CommitId("base".into());
    let initial_page = Arc::new(log_page(
        vec![
            commit("tip", &[base_commit_id.as_ref()], "tip"),
            commit(base_commit_id.as_ref(), &[], "base"),
        ],
        None,
    ));
    let mut initial_repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-page-change"),
        },
    );
    initial_repo.history_state.history_scope = LogScope::AllBranches;
    initial_repo.head_branch = Loadable::Ready("main".to_string());
    initial_repo.head_branch_rev = 1;
    initial_repo.branches = Loadable::Ready(Arc::new(vec![
        branch("main", "tip"),
        branch("feature", "tip"),
    ]));
    initial_repo.branches_rev = 1;
    initial_repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    initial_repo.remote_branches_rev = 1;
    initial_repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    initial_repo.tags_rev = 1;
    initial_repo.log = Loadable::Ready(Arc::clone(&initial_page));
    initial_repo.log_rev = 1;
    initial_repo.history_state.log = Loadable::Ready(Arc::clone(&initial_page));
    initial_repo.history_state.log_rev = 1;

    let initial_state = Arc::new(AppState {
        repos: vec![initial_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let switched_page = Arc::new(log_page(vec![commit("main-tip", &[], "main tip")], None));
    let mut switched_repo = initial_repo;
    switched_repo.history_state.history_scope = LogScope::CurrentBranch;
    switched_repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "main-tip")]));
    switched_repo.branches_rev = 2;
    switched_repo.log = Loadable::Ready(Arc::clone(&switched_page));
    switched_repo.log_rev = 2;
    switched_repo.history_state.log = Loadable::Ready(Arc::clone(&switched_page));
    switched_repo.history_state.log_rev = 2;

    let switched_state = Arc::new(AppState {
        repos: vec![switched_repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let apply_state = |cx: &mut gpui::VisualTestContext, state: Arc<AppState>| {
        cx.update(|window, app| {
            let ui_model = view.read(app)._ui_model.clone();
            ui_model.update(app, |model, cx| {
                model.set_state(Arc::clone(&state), cx);
            });
            window.refresh();
            let _ = window.draw(app);
        });
        cx.run_until_parked();
        cx.update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            history_view.update(app, |history, cx| history.ensure_history_cache(cx));
            window.refresh();
            let _ = window.draw(app);
        });
        cx.run_until_parked();
    };

    apply_state(cx, initial_state);

    wait_until(cx, "history rows with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some() && cx.debug_bounds("history_row_1").is_some()
    });

    let refs_column_point = |cx: &mut gpui::VisualTestContext| {
        let row = cx
            .debug_bounds("history_row_0")
            .expect("history row should be rendered");
        point(row.left() + px(24.0), row.center().y)
    };
    let hover_point = refs_column_point(cx);
    cx.simulate_mouse_move(hover_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let feature_center = cx
        .debug_bounds("history_refs_hover_item_local_branch_feature")
        .expect("expected feature ref item in debug bounds")
        .center();
    cx.simulate_mouse_move(feature_center, None, gpui::Modifiers::default());
    cx.simulate_mouse_down(
        feature_center,
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.simulate_mouse_up(
        feature_center,
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            Some(PopoverKind::BranchMenu {
                repo_id,
                section: BranchSection::Local,
                name: "feature".to_string(),
            })
        );
    });

    apply_state(cx, switched_state);

    wait_until(cx, "switched history row", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.row_vms.len() == 1
                    && cache.base.row_vms[0].summary.as_ref() == "main tip"
            })
        })
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(
        !hover_open,
        "history refs hover should close when the history page changes without a mouse move"
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
    cx.update(|_window, app| {
        assert_eq!(
            crate::view::test_support::popover_kind(view.read(app), app),
            None,
            "history refs item menu should close when the history page changes"
        );
    });
}

#[gpui::test]
fn history_refs_hover_closes_when_history_scrolls_programmatically(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let selected_commit = CommitId("c50".into());
    let commits = (0..80)
        .map(|ix| {
            let id = format!("c{ix:02}");
            commit(&id, &[], &format!("commit {ix:02}"))
        })
        .collect::<Vec<_>>();
    let page = Arc::new(log_page(commits, None));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/history-refs-hover-programmatic-scroll"),
        },
    );
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.history_state.selected_commit = Some(selected_commit.clone());
    repo.history_state.commit_details =
        Loadable::Ready(Arc::new(worktree_core::domain::CommitDetails {
            id: selected_commit.clone(),
            message: "commit 50".into(),
            author_name: String::new(),
            author_email: String::new(),
            authored_at_unix: 0,
            committed_at: "2026-05-26 12:00:00 +0300".into(),
            committed_at_unix: 0,
            parent_ids: vec![],
            files: vec![],
            signed: false,
        }));
    repo.branches = Loadable::Ready(Arc::new(vec![branch("feature", "c00")]));
    repo.branches_rev = 1;
    repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    repo.remote_branches_rev = 1;
    repo.tags = Loadable::Ready(Arc::new(Vec::new()));
    repo.tags_rev = 1;
    repo.log = Loadable::Ready(Arc::clone(&page));
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Ready(page);
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|_window, app| {
        let ui_model = view.read(app)._ui_model.clone();
        ui_model.update(app, |model, cx| {
            model.set_state(Arc::clone(&state), cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "history row with displayed refs", |cx| {
        cx.debug_bounds("history_row_0").is_some()
    });

    let row = cx
        .debug_bounds("history_row_0")
        .expect("history row should be rendered");
    let hover_point = point(row.left() + px(24.0), row.center().y);
    cx.simulate_mouse_move(hover_point, None, gpui::Modifiers::default());
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    assert!(cx.debug_bounds("history_refs_hover_panel").is_some());
    cx.update(|_window, app| {
        assert!(crate::view::test_support::history_refs_hover_is_open(
            view.read(app),
            app
        ));
    });

    let scroll_y = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_scroll.0.borrow().base_handle.offset().y
        })
    };
    let before_scroll_y = scroll_y(cx);

    cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            history.request_reveal_commit(repo_id, selected_commit.clone(), None, cx);
        });
        window.refresh();
        let _ = window.draw(app);
    });
    wait_until(cx, "history list to scroll programmatically", |cx| {
        scroll_y(cx) != before_scroll_y
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });
    let hover_open = cx.update(|_window, app| {
        crate::view::test_support::history_refs_hover_is_open(view.read(app), app)
    });
    assert!(
        !hover_open,
        "history refs hover should close when history scrolls programmatically"
    );
    assert!(cx.debug_bounds("history_refs_hover_panel").is_none());
}

#[gpui::test]
fn current_branch_remote_branch_changes_reuse_base_cache_and_refresh_decorations(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let page = Arc::new(log_page(vec![commit("tip", &[], "tip")], None));
    let repo_path = PathBuf::from("/tmp/history-current-branch-remote-reuse");

    let mut initial_repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    initial_repo.history_state.history_scope = LogScope::CurrentBranch;
    initial_repo.head_branch = Loadable::Ready("main".to_string());
    initial_repo.head_branch_rev = 1;
    initial_repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    initial_repo.branches_rev = 1;
    initial_repo.remote_branches =
        Loadable::Ready(Arc::new(vec![remote_branch("origin", "main", "tip")]));
    initial_repo.remote_branches_rev = 1;
    initial_repo.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.log_rev = 1;
    initial_repo.history_state.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.history_state.log_rev = 1;

    let initial_state = Arc::new(AppState {
        repos: vec![initial_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let mut updated_repo = initial_repo;
    updated_repo.remote_branches = Loadable::Ready(Arc::new(vec![
        remote_branch("origin", "main", "tip"),
        remote_branch("upstream", "main", "tip"),
    ]));
    updated_repo.remote_branches_rev = 2;

    let updated_state = Arc::new(AppState {
        repos: vec![updated_repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, initial_state);

    wait_until(cx, "initial current-branch history cache", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.remote_branches_rev == 0
                    && cache.decorations.row_vms.len() == 1
                    && cache.decorations.row_vms[0]
                        .branches_text
                        .as_ref()
                        .contains("origin/main")
            })
        })
    });

    let (before_graph_rows, before_base_request, before_branches_text) =
        cx.update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let rows_len = history_view.update(app, |history, cx| {
                HistoryView::render_history_table_rows(history, 0..1, window, cx).len()
            });
            assert_eq!(rows_len, 1, "initial current-branch row should render");

            let history = history_view.read(app);
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache.decorations.row_vms[0]
                    .branches_text
                    .as_ref()
                    .to_owned(),
            )
        });

    assert!(before_branches_text.contains("origin/main"));
    assert!(!before_branches_text.contains("upstream/main"));

    ensure_history_cache_for_tests(cx, &view, updated_state);

    wait_until(cx, "updated current-branch decorations", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.remote_branches_rev == 0
                    && cache.decorations.request.remote_branches_rev == 2
                    && cache.decorations.row_vms.len() == 1
                    && cache.decorations.row_vms[0]
                        .branches_text
                        .as_ref()
                        .contains("upstream/main")
            })
        })
    });

    let (after_graph_rows, after_base_request, after_branches_text) = cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        let rows_len = history_view.update(app, |history, cx| {
            HistoryView::render_history_table_rows(history, 0..1, window, cx).len()
        });
        assert_eq!(
            rows_len, 1,
            "updated current-branch row should still render"
        );

        let history = history_view.read(app);
        let cache = history
            .history_cache
            .as_ref()
            .expect("history cache should be available");
        (
            Arc::clone(&cache.base.graph_rows),
            cache.base.request.clone(),
            cache.decorations.row_vms[0]
                .branches_text
                .as_ref()
                .to_owned(),
        )
    });

    assert!(
        Arc::ptr_eq(&before_graph_rows, &after_graph_rows),
        "remote branch changes in current-branch mode should reuse the heavy base cache"
    );
    assert_eq!(after_base_request, before_base_request);
    assert!(after_branches_text.contains("origin/main"));
    assert!(after_branches_text.contains("upstream/main"));
}

#[gpui::test]
fn current_branch_local_branch_changes_reuse_base_cache_and_refresh_decorations(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let page = Arc::new(log_page(vec![commit("tip", &[], "tip")], None));
    let repo_path = PathBuf::from("/tmp/history-current-branch-local-reuse");

    let mut initial_repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    initial_repo.history_state.history_scope = LogScope::CurrentBranch;
    initial_repo.head_branch = Loadable::Ready("main".to_string());
    initial_repo.head_branch_rev = 1;
    initial_repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    initial_repo.branches_rev = 1;
    initial_repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    initial_repo.remote_branches_rev = 1;
    initial_repo.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.log_rev = 1;
    initial_repo.history_state.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.history_state.log_rev = 1;

    let initial_state = Arc::new(AppState {
        repos: vec![initial_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let mut updated_repo = initial_repo;
    updated_repo.branches = Loadable::Ready(Arc::new(vec![
        branch("main", "tip"),
        branch("feature", "tip"),
    ]));
    updated_repo.branches_rev = 2;

    let updated_state = Arc::new(AppState {
        repos: vec![updated_repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, initial_state);

    wait_until(cx, "initial current-branch local history cache", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.branches_rev == 0
                    && cache.decorations.row_vms.len() == 1
                    && cache.decorations.row_vms[0]
                        .branches_text
                        .as_ref()
                        .contains("main")
            })
        })
    });

    let (before_graph_rows, before_base_request, before_branches_text) =
        cx.update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let rows_len = history_view.update(app, |history, cx| {
                HistoryView::render_history_table_rows(history, 0..1, window, cx).len()
            });
            assert_eq!(rows_len, 1, "initial current-branch row should render");

            let history = history_view.read(app);
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache.decorations.row_vms[0]
                    .branches_text
                    .as_ref()
                    .to_owned(),
            )
        });

    assert!(before_branches_text.contains("main"));
    assert!(!before_branches_text.contains("feature"));

    ensure_history_cache_for_tests(cx, &view, updated_state);

    wait_until(cx, "updated current-branch local decorations", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.branches_rev == 0
                    && cache.decorations.request.branches_rev == 2
                    && cache.decorations.row_vms.len() == 1
                    && cache.decorations.row_vms[0]
                        .branches_text
                        .as_ref()
                        .contains("feature")
            })
        })
    });

    let (after_graph_rows, after_base_request, after_branches_text) = cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        let rows_len = history_view.update(app, |history, cx| {
            HistoryView::render_history_table_rows(history, 0..1, window, cx).len()
        });
        assert_eq!(
            rows_len, 1,
            "updated current-branch row should still render"
        );

        let history = history_view.read(app);
        let cache = history
            .history_cache
            .as_ref()
            .expect("history cache should be available");
        (
            Arc::clone(&cache.base.graph_rows),
            cache.base.request.clone(),
            cache.decorations.row_vms[0]
                .branches_text
                .as_ref()
                .to_owned(),
        )
    });

    assert!(
        Arc::ptr_eq(&before_graph_rows, &after_graph_rows),
        "local branch changes in current-branch mode should reuse the heavy base cache"
    );
    assert_eq!(after_base_request, before_base_request);
    assert!(after_branches_text.contains("main"));
    assert!(after_branches_text.contains("feature"));
}

#[gpui::test]
fn current_branch_head_target_changes_rebuild_base_cache_and_move_head_marker(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let page = Arc::new(log_page(
        vec![commit("tip", &["base"], "tip"), commit("base", &[], "base")],
        None,
    ));
    let repo_path = PathBuf::from("/tmp/history-current-branch-head-target");

    let mut initial_repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    initial_repo.history_state.history_scope = LogScope::CurrentBranch;
    initial_repo.head_branch = Loadable::Ready("main".to_string());
    initial_repo.head_branch_rev = 1;
    initial_repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "tip")]));
    initial_repo.branches_rev = 1;
    initial_repo.remote_branches = Loadable::Ready(Arc::new(Vec::new()));
    initial_repo.remote_branches_rev = 1;
    initial_repo.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.log_rev = 1;
    initial_repo.history_state.log = Loadable::Ready(Arc::clone(&page));
    initial_repo.history_state.log_rev = 1;

    let initial_state = Arc::new(AppState {
        repos: vec![initial_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let mut updated_repo = initial_repo;
    updated_repo.branches = Loadable::Ready(Arc::new(vec![branch("main", "base")]));
    updated_repo.branches_rev = 2;

    let updated_state = Arc::new(AppState {
        repos: vec![updated_repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, initial_state);

    wait_until(cx, "initial current-branch head target cache", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.branches_rev == 0
                    && cache
                        .base
                        .request
                        .head_branch_target
                        .as_ref()
                        .map(AsRef::as_ref)
                        == Some("tip")
                    && cache.base.row_vms.len() == 2
                    && cache.base.row_vms[0].is_head
                    && !cache.base.row_vms[1].is_head
                    && cache.decorations.row_vms[0]
                        .branches_text
                        .as_ref()
                        .contains("main")
            })
        })
    });

    let (before_graph_rows, before_base_request, before_head_rows, before_branches_text) = cx
        .update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let rows_len = history_view.update(app, |history, cx| {
                HistoryView::render_history_table_rows(history, 0..2, window, cx).len()
            });
            assert_eq!(rows_len, 2, "initial rows should render");

            let history = history_view.read(app);
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache
                    .base
                    .row_vms
                    .iter()
                    .map(|row| row.is_head)
                    .collect::<Vec<_>>(),
                cache
                    .decorations
                    .row_vms
                    .iter()
                    .map(|row| row.branches_text.as_ref().to_owned())
                    .collect::<Vec<_>>(),
            )
        });

    assert_eq!(before_head_rows, vec![true, false]);
    assert!(before_branches_text[0].contains("main"));
    assert!(before_branches_text[1].is_empty());

    ensure_history_cache_for_tests(cx, &view, updated_state);

    wait_until(cx, "updated current-branch head target cache", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::CurrentBranch
                    && cache.base.request.branches_rev == 0
                    && cache
                        .base
                        .request
                        .head_branch_target
                        .as_ref()
                        .map(AsRef::as_ref)
                        == Some("base")
                    && cache.base.row_vms.len() == 2
                    && !cache.base.row_vms[0].is_head
                    && cache.base.row_vms[1].is_head
                    && cache.decorations.row_vms[1]
                        .branches_text
                        .as_ref()
                        .contains("main")
            })
        })
    });

    let (after_graph_rows, after_base_request, after_head_rows, after_branches_text) =
        cx.update(|window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let rows_len = history_view.update(app, |history, cx| {
                HistoryView::render_history_table_rows(history, 0..2, window, cx).len()
            });
            assert_eq!(rows_len, 2, "updated rows should still render");

            let history = history_view.read(app);
            let cache = history
                .history_cache
                .as_ref()
                .expect("history cache should be available");
            (
                Arc::clone(&cache.base.graph_rows),
                cache.base.request.clone(),
                cache
                    .base
                    .row_vms
                    .iter()
                    .map(|row| row.is_head)
                    .collect::<Vec<_>>(),
                cache
                    .decorations
                    .row_vms
                    .iter()
                    .map(|row| row.branches_text.as_ref().to_owned())
                    .collect::<Vec<_>>(),
            )
        });

    assert!(
        !Arc::ptr_eq(&before_graph_rows, &after_graph_rows),
        "head target changes should rebuild the heavy base cache in current-branch mode"
    );
    assert_eq!(before_base_request.branches_rev, 0);
    assert_eq!(after_base_request.branches_rev, 0);
    assert_ne!(after_base_request, before_base_request);
    assert_eq!(
        before_base_request
            .head_branch_target
            .as_ref()
            .map(AsRef::as_ref),
        Some("tip")
    );
    assert_eq!(
        after_base_request
            .head_branch_target
            .as_ref()
            .map(AsRef::as_ref),
        Some("base")
    );
    assert_eq!(after_head_rows, vec![false, true]);
    assert!(after_branches_text[0].is_empty());
    assert!(after_branches_text[1].contains("main"));
}

#[gpui::test]
fn history_scope_switch_keeps_rows_visible_and_refreshes_automatically(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let initial_scope = LogScope::FullReachable;
    let switched_scope = LogScope::AllBranches;
    let repo_path = PathBuf::from("/tmp/history-scope-switch-test");
    let initial_page = Arc::new(log_page(vec![commit("main-tip", &[], "main tip")], None));
    let switched_page = Arc::new(log_page(
        vec![
            commit("all-tip", &[], "all branches tip"),
            commit("main-tip", &[], "main tip"),
        ],
        None,
    ));

    let mut initial_repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    initial_repo.history_state.history_scope = initial_scope;
    initial_repo.log = Loadable::Ready(Arc::clone(&initial_page));
    initial_repo.log_rev = 1;
    initial_repo.history_state.log = Loadable::Ready(Arc::clone(&initial_page));
    initial_repo.history_state.log_rev = 1;

    let initial_state = Arc::new(AppState {
        repos: vec![initial_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let mut loading_repo = initial_repo.clone();
    loading_repo.history_state.history_scope = switched_scope;
    loading_repo.log = Loadable::Loading;
    loading_repo.log_rev = 2;
    loading_repo.history_state.log = Loadable::Loading;
    loading_repo.history_state.log_rev = 2;
    loading_repo.history_state.retained_log_while_loading = Some(Arc::clone(&initial_page));

    let loading_state = Arc::new(AppState {
        repos: vec![loading_repo.clone()],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    let mut loaded_repo = loading_repo;
    loaded_repo.log = Loadable::Ready(Arc::clone(&switched_page));
    loaded_repo.log_rev = 3;
    loaded_repo.history_state.log = Loadable::Ready(Arc::clone(&switched_page));
    loaded_repo.history_state.log_rev = 3;
    loaded_repo.history_state.retained_log_while_loading = None;

    let loaded_state = Arc::new(AppState {
        repos: vec![loaded_repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    ensure_history_cache_for_tests(cx, &view, Arc::clone(&initial_state));

    wait_until(cx, "initial history rows", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == initial_scope
                    && cache.base.visible_indices.len() == 1
                    && cache.base.row_vms.len() == 1
                    && cache.base.row_vms[0].summary.as_ref() == "main tip"
            })
        })
    });

    ensure_history_cache_for_tests(cx, &view, Arc::clone(&loading_state));

    wait_until(cx, "retained history rows during loading", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.active_repo().is_some_and(|repo| {
                repo.history_state.history_scope == switched_scope
                    && matches!(repo.log, Loadable::Loading)
                    && repo
                        .history_state
                        .retained_log_while_loading
                        .as_ref()
                        .is_some_and(|page| Arc::ptr_eq(page, &initial_page))
            }) && history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.visible_indices.len() == 1
                    && cache.base.row_vms.len() == 1
                    && cache.base.row_vms[0].summary.as_ref() == "main tip"
            })
        })
    });

    cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            let rows = HistoryView::render_history_table_rows(history, 0..1, window, cx);
            assert_eq!(rows.len(), 1, "retained history row should still render");
        });
    });

    ensure_history_cache_for_tests(cx, &view, Arc::clone(&loaded_state));

    wait_until(cx, "history rows refresh after scope load", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == switched_scope
                    && cache.base.visible_indices.len() == 2
                    && cache.base.row_vms.len() == 2
                    && cache.base.row_vms[0].summary.as_ref() == "all branches tip"
                    && cache.base.row_vms[1].summary.as_ref() == "main tip"
            })
        })
    });
}

#[gpui::test]
fn filtered_modes_do_not_infer_detached_head_target_from_first_visible_row(
    cx: &mut gpui::TestAppContext,
) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    for (scope, commits, expected_summary) in [
        (
            LogScope::NoMerges,
            vec![commit("visible", &["hidden"], "visible non-merge")],
            "visible non-merge",
        ),
        (
            LogScope::MergesOnly,
            vec![commit("visible-merge", &["p0", "p1"], "visible merge")],
            "visible merge",
        ),
    ] {
        let page = Arc::new(log_page(commits, None));
        let mut repo = RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/history-detached-head-filtered"),
            },
        );
        repo.history_state.history_scope = scope;
        repo.head_branch = Loadable::Ready("HEAD".to_string());
        repo.head_branch_rev = 1;
        repo.log = Loadable::Ready(Arc::clone(&page));
        repo.log_rev = 1;
        repo.history_state.log = Loadable::Ready(page);
        repo.history_state.log_rev = 1;

        let state = Arc::new(AppState {
            repos: vec![repo],
            active_repo: Some(RepoId(1)),
            ..Default::default()
        });

        ensure_history_cache_for_tests(cx, &view, state);

        let description = format!("filtered {scope:?} history cache");
        wait_until(cx, &description, |cx| {
            cx.update(|_window, app| {
                let main_pane = view.read(app).main_pane.clone();
                let history_view = main_pane.read(app).history_view.clone();
                let history = history_view.read(app);
                history.history_cache.as_ref().is_some_and(|cache| {
                    cache.base.request.history_scope == scope
                        && cache.base.row_vms.len() == 1
                        && !cache.base.row_vms[0].is_head
                        && cache.base.row_vms[0].summary.as_ref() == expected_summary
                })
            })
        });
    }
}

#[gpui::test]
fn retained_history_rows_support_keyboard_navigation_while_loading(cx: &mut gpui::TestAppContext) {
    let _visual_guard = crate::test_support::lock_visual_test();
    let (store, events) = AppStore::new(Arc::new(BlockingBackend));
    let store_for_assert = store.clone();
    let (view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    let repo_id = RepoId(1);
    let first = CommitId("tip".into());
    let second = CommitId("base".into());
    let repo_path = PathBuf::from(format!(
        "/tmp/history-retained-nav-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    cx.update(|window, app| {
        let _ = window.draw(app);
    });

    store_for_assert.dispatch(Msg::OpenRepo(repo_path.clone()));
    wait_until(cx, "opened repo placeholder", |_cx| {
        let snapshot = store_for_assert.snapshot();
        snapshot.active_repo == Some(repo_id)
            && snapshot.repos.iter().any(|repo| repo.id == repo_id)
    });

    let page = Arc::new(log_page(
        vec![commit("tip", &["base"], "tip"), commit("base", &[], "base")],
        None,
    ));
    let mut repo = RepoState::new_opening(repo_id, RepoSpec { workdir: repo_path });
    repo.history_state.history_scope = LogScope::AllBranches;
    repo.history_state.selected_commit = Some(first.clone());
    repo.history_state.retained_log_while_loading = Some(Arc::clone(&page));
    repo.head_branch = Loadable::Ready("main".to_string());
    repo.head_branch_rev = 1;
    repo.log = Loadable::Loading;
    repo.log_rev = 1;
    repo.history_state.log = Loadable::Loading;
    repo.history_state.log_rev = 1;

    let state = Arc::new(AppState {
        repos: vec![repo],
        active_repo: Some(repo_id),
        ..Default::default()
    });

    ensure_history_cache_for_tests(cx, &view, state);

    wait_until(cx, "retained rows available during loading", |cx| {
        cx.update(|_window, app| {
            let main_pane = view.read(app).main_pane.clone();
            let history_view = main_pane.read(app).history_view.clone();
            let history = history_view.read(app);
            history.active_repo().is_some_and(|repo| {
                repo.history_state.history_scope == LogScope::AllBranches
                    && matches!(repo.log, Loadable::Loading)
                    && repo.history_state.retained_log_while_loading.is_some()
                    && repo.history_state.selected_commit.as_ref() == Some(&first)
            }) && history.history_cache.as_ref().is_some_and(|cache| {
                cache.base.request.history_scope == LogScope::AllBranches
                    && cache.base.row_vms.len() == 2
                    && cache.base.row_vms[0].summary.as_ref() == "tip"
                    && cache.base.row_vms[1].summary.as_ref() == "base"
            })
        })
    });

    cx.update(|window, app| {
        let main_pane = view.read(app).main_pane.clone();
        let history_view = main_pane.read(app).history_view.clone();
        history_view.update(app, |history, cx| {
            assert!(history.history_select_adjacent_commit(1, cx));
        });
        window.refresh();
        let _ = window.draw(app);
    });

    wait_until(cx, "selected second retained commit", |_cx| {
        let snapshot = store_for_assert.snapshot();
        let Some(repo) = snapshot.repos.iter().find(|repo| repo.id == repo_id) else {
            return false;
        };
        repo.history_state.selected_commit.as_ref() == Some(&second)
    });
}
