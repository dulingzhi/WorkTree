use super::*;

/// Production always reaches `LogLoaded` through `request_log`, which records the
/// walk as the active one so replies from a superseded walk can be dropped.
/// Tests that dispatch `LogLoaded` directly have to declare it the same way, and
/// send the sequence number it hands back.
/// The sequence number of the log walk the repository already has in flight —
/// what a reply from that walk has to carry. For tests where earlier reduces
/// dispatched the load; [`expect_log_reply`] is for tests that dispatch none.
fn active_log_seq(repo_state: &RepoState) -> crate::model::LogLoadSeq {
    repo_state
        .loads_in_flight
        .active_log_seq()
        .expect("a log walk is in flight")
}

fn commit_details_for(id: &CommitId, message: &str) -> worktree_core::domain::CommitDetails {
    worktree_core::domain::CommitDetails {
        id: id.clone(),
        message: message.to_string(),
        author_name: String::new(),
        author_email: String::new(),
        authored_at_unix: 0,
        committed_at: "2026-03-08 12:34:56 +0200".to_string(),
        committed_at_unix: 0,
        parent_ids: vec![],
        files: vec![],
        signed: false,
    }
}

fn expect_log_reply(
    repo_state: &mut RepoState,
    scope: LogScope,
    author: Option<&str>,
    cursor: Option<LogCursor>,
) -> crate::model::LogLoadSeq {
    repo_state.loads_in_flight.clear();
    repo_state
        .loads_in_flight
        .request_log(crate::model::PendingLogLoad {
            scope,
            author: author.map(str::to_owned),
            refs: Vec::new(),
            limit: 200,
            cursor,
        })
        .expect("a declared log walk starts immediately")
}

fn test_force_push_lease() -> worktree_core::services::ForcePushLease {
    worktree_core::services::ForcePushLease {
        remote: "origin".to_string(),
        branch: "main".to_string(),
        expected: CommitId("1111111111111111111111111111111111111111".into()),
        local_branch: "main".to_string(),
        local_head: CommitId("2222222222222222222222222222222222222222".into()),
    }
}

fn test_recent_commit_message() -> worktree_core::domain::RecentCommitMessage {
    worktree_core::domain::RecentCommitMessage {
        id: CommitId("1111111111111111111111111111111111111111".into()),
        summary: Arc::from("old message"),
        message: "old message\n\nbody".to_string(),
    }
}

#[test]
fn repo_activated_is_reducer_noop_by_itself() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(repo_id);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoActivated { repo_id },
    );

    assert!(effects.is_empty());
    assert!(!state.repos[0].status.is_loading());
    assert!(!state.repos[0].log.is_loading());
}

#[test]
fn repo_load_trace_names_repo_activation_and_refresh_messages() {
    let repo_id = RepoId(1);

    assert_eq!(
        repo_load_trace::msg_name(&Msg::RepoActivated { repo_id }),
        "RepoActivated"
    );
    assert_eq!(
        repo_load_trace::msg_name(&Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        }),
        "RepoExternallyChanged"
    );
    assert_eq!(
        repo_load_trace::msg_name(&Msg::ReloadRepo { repo_id }),
        "ReloadRepo"
    );
    assert_eq!(
        repo_load_trace::msg_repo_id(&Msg::RepoActivated { repo_id }),
        Some(repo_id)
    );
    assert_eq!(
        repo_load_trace::msg_external_change(&Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        }),
        Some(crate::msg::RepoExternalChange::GitState)
    );
}

#[test]
fn external_worktree_change_refreshes_status_and_selected_diff() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id: RepoId(1),
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectDiff {
            repo_id: RepoId(1),
            target: DiffTarget::WorkingTree {
                path: PathBuf::from("a.txt"),
                area: DiffArea::Unstaged,
            },
        },
    );

    // Complete the initial open-repo refresh so the external-change refresh isn't coalesced away.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );

    assert!(
        has_worktree_status_effect(&effects, RepoId(1)),
        "expected status refresh"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiff { repo_id, .. } if *repo_id == RepoId(1))),
        "expected diff refresh"
    );
    assert!(
        effects.iter().any(|e| {
            matches!(e, Effect::LoadDiffFile { repo_id, .. } if *repo_id == RepoId(1))
        }),
        "expected diff-file refresh"
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::LoadLog { .. })),
        "did not expect history refresh on pure worktree changes"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::LoadHeadBranch { .. })),
        "did not expect head-branch refresh on pure worktree changes"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::LoadUpstreamDivergence { .. })),
        "did not expect upstream divergence refresh on pure worktree changes"
    );
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::LoadBranches { .. } | Effect::LoadRemoteBranches { .. }
        )),
        "did not expect branch refresh on pure worktree changes"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::LoadRebaseState { .. })),
        "did not expect rebase state refresh on pure worktree changes"
    );
}

#[test]
fn external_index_change_refreshes_both_staged_and_unstaged_lanes() {
    // An external `git add` / `git reset` / `git restore --staged` rewrites `.git/index` without
    // touching any worktree file, so the monitor reports an index-only change. The index is one
    // side of BOTH the staged (HEAD↔index) and unstaged (index↔worktree) diffs, so both lanes
    // must refresh; otherwise a file that moved between the staged and unstaged sections lingers
    // (stale) in the lane that was not reloaded.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id,
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );

    // Complete the initial open-repo refresh so the external-change refresh isn't coalesced away.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Index,
            worktree_paths: None,
        },
    );

    assert!(
        has_status_refresh_effects(&effects, repo_id),
        "an index-only external change must refresh both the staged and unstaged lanes, got {effects:?}"
    );
}

#[test]
fn external_index_change_reloads_open_working_tree_diff() {
    // With a staged file's diff open, an external `git add` / `git reset` (index-only change)
    // must reload that working-tree diff so it reflects the new index content rather than showing
    // a stale diff.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id,
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectDiff {
            repo_id,
            target: DiffTarget::WorkingTree {
                path: PathBuf::from("a.txt"),
                area: DiffArea::Staged,
            },
        },
    );

    // Complete the initial open-repo refresh so the external-change refresh isn't coalesced away.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Index,
            worktree_paths: None,
        },
    );

    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiff { repo_id: rid, .. } if *rid == repo_id)),
        "an index change must reload the open staged working-tree diff, got {effects:?}"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiffFile { repo_id: rid, .. } if *rid == repo_id)),
        "an index change must reload the open diff's file content, got {effects:?}"
    );
}

#[test]
fn external_index_change_must_not_refresh_only_the_staged_lane() {
    // Regression test that fails against the previous behavior: an index-only external change
    // (`git add` / `git reset` / `git restore --staged`) used to emit exactly
    // `[LoadStagedStatus]`, refreshing only the staged lane and leaving a moved file stale in the
    // unstaged section. The change must also pursue the unstaged (worktree) lane.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id,
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );
    // Settle the initial refresh so the external-change refresh isn't coalesced away.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Index,
            worktree_paths: None,
        },
    );

    // The unstaged lane must be refreshed — either by the combined status load or a direct
    // worktree load.
    assert!(
        has_combined_status_effect(&effects, repo_id)
            || has_worktree_status_effect(&effects, repo_id),
        "an index-only change must refresh the unstaged lane, got {effects:?}"
    );
    // The exact old-behavior shape (staged lane only) must not occur.
    let staged_lane_only = has_staged_status_effect(&effects, repo_id)
        && !has_combined_status_effect(&effects, repo_id)
        && !has_worktree_status_effect(&effects, repo_id);
    assert!(
        !staged_lane_only,
        "an index-only change must not refresh only the staged lane, got {effects:?}"
    );
}

#[test]
fn external_git_state_change_preserves_pending_force_push_lease_and_clears_recent_messages() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    let mut repo_state = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    );
    repo_state.pending_force_push_lease = Some(test_force_push_lease());
    repo_state.set_recent_commit_messages(Loadable::Ready(vec![test_recent_commit_message()]));
    let recent_rev = repo_state.recent_commit_messages_rev;
    state.repos.push(repo_state);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );

    assert_eq!(
        state.repos[0].pending_force_push_lease,
        Some(test_force_push_lease())
    );
    assert!(matches!(
        &state.repos[0].recent_commit_messages,
        Loadable::NotLoaded
    ));
    assert!(state.repos[0].recent_commit_messages_rev > recent_rev);
}

#[test]
fn external_git_state_change_refreshes_history_and_selected_diff() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id: RepoId(1),
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectDiff {
            repo_id: RepoId(1),
            target: DiffTarget::WorkingTree {
                path: PathBuf::from("a.txt"),
                area: DiffArea::Unstaged,
            },
        },
    );

    // Complete the initial open-repo refresh so the external-change refresh isn't coalesced away.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded {
            repo_id: RepoId(1),
            result: Ok("main".to_string()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::UpstreamDivergenceLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RebaseStateLoaded {
            repo_id: RepoId(1),
            result: Ok(worktree_core::services::SequencerState::None),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::MergeCommitMessageLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    let history_scope = state.repos[0].history_state.history_scope;
    let seq = active_log_seq(&state.repos[0]);
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: history_scope,
            cursor: None,
            result: Ok(LogPage {
                commits: Vec::new(),
                next_cursor: None,
            }),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::BranchesLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RemoteBranchesLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );

    assert!(
        effects
            .iter()
            .any(|e| { matches!(e, Effect::LoadLog { repo_id, .. } if *repo_id == RepoId(1)) }),
        "expected history refresh"
    );
    assert!(
        has_status_refresh_effects(&effects, RepoId(1)),
        "expected status refresh"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadHeadBranch { repo_id } if *repo_id == RepoId(1))),
        "expected head-branch refresh"
    );
    assert!(
        effects.iter().any(|e| {
            matches!(e, Effect::LoadUpstreamDivergence { repo_id } if *repo_id == RepoId(1))
        }),
        "expected upstream divergence refresh"
    );
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::LoadRebaseAndMergeState { repo_id } if *repo_id == RepoId(1)
        )),
        "expected rebase state refresh"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadBranches { repo_id } if *repo_id == RepoId(1))),
        "expected local branches refresh"
    );
    assert!(
        effects.iter().any(|e| {
            matches!(e, Effect::LoadRemoteBranches { repo_id } if *repo_id == RepoId(1))
        }),
        "expected remote branches refresh"
    );
    assert!(
        effects.iter().any(|e| {
            matches!(
                e,
                Effect::LoadRebaseAndMergeState { repo_id } if *repo_id == RepoId(1)
            )
        }),
        "expected merge commit message refresh"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiff { repo_id, .. } if *repo_id == RepoId(1))),
        "expected diff refresh"
    );
}

#[test]
fn external_git_state_refresh_is_coalesced_and_replayed_once() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let effects1 = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );

    assert!(
        effects1
            .iter()
            .any(|e| matches!(e, Effect::LoadHeadBranch { .. }))
    );
    assert!(
        effects1
            .iter()
            .any(|e| matches!(e, Effect::LoadUpstreamDivergence { .. }))
    );
    assert!(
        effects1
            .iter()
            .any(|e| matches!(e, Effect::LoadRebaseAndMergeState { .. }))
    );
    assert!(
        effects1
            .iter()
            .any(|e| matches!(e, Effect::LoadRebaseAndMergeState { .. }))
    );
    assert!(has_status_refresh_effects(&effects1, RepoId(1)));
    assert!(effects1.iter().any(|e| matches!(e, Effect::LoadLog { .. })));

    // Second refresh request while the first one is in flight is coalesced into a single pending
    // refresh per load kind (no immediate duplicate effects).
    let effects2 = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );
    assert!(
        effects2.is_empty(),
        "expected coalescing/backpressure, got {effects2:?}"
    );

    // Completing each in-flight load replays exactly one more load for that kind.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded {
            repo_id: RepoId(1),
            result: Ok("main".to_string()),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadHeadBranch { repo_id: RepoId(1) }]
    ));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::UpstreamDivergenceLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadUpstreamDivergence { repo_id: RepoId(1) }]
    ));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RebaseStateLoaded {
            repo_id: RepoId(1),
            result: Ok(worktree_core::services::SequencerState::None),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadRebaseState { repo_id: RepoId(1) }]
    ));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::MergeCommitMessageLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadMergeCommitMessage { repo_id: RepoId(1) }]
    ));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadWorktreeStatus { repo_id: RepoId(1) }]
    ));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StagedStatusLoaded {
            repo_id: RepoId(1),
            result: Ok(Vec::new()),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadStagedStatus { repo_id: RepoId(1) }]
    ));

    let history_scope = state.repos[0].history_state.history_scope;
    let seq = active_log_seq(&state.repos[0]);
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: history_scope,
            cursor: None,
            result: Ok(LogPage {
                commits: Vec::new(),
                next_cursor: None,
            }),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadLog {
            repo_id: RepoId(1),
            scope,
            limit: 200,
            cursor: None,
            ..
        }] if *scope == history_scope
    ));
}

#[test]
fn external_worktree_refresh_replays_coalesced_change_then_settles() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].set_status(Loadable::Ready(Arc::new(RepoStatus::default())));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert!(
        has_worktree_status_effect(&effects, repo_id),
        "expected first worktree event to request status refresh"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert!(
        effects.is_empty(),
        "expected in-flight coalescing while status load is running, got {effects:?}"
    );

    // The in-flight load completes with an unchanged payload, but a second worktree event was
    // coalesced while it ran. That event is a genuine external change the load may have read just
    // before it landed, so the coalesced refresh must be replayed (not dropped) — otherwise the
    // uncommitted view keeps showing stale entries.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );
    assert!(
        has_worktree_status_effect(&effects, repo_id),
        "coalesced external change must replay a status load even when the payload is unchanged, got {effects:?}"
    );
    assert!(
        state.repos[0]
            .loads_in_flight
            .is_in_flight(crate::model::RepoLoadsInFlight::WORKTREE_STATUS),
        "the replayed load should re-arm the worktree status lane"
    );

    // The replayed load completes and nothing new is pending, so the lane settles instead of
    // looping forever (status reads are read-only and cannot manufacture their own events).
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(Vec::new()),
        }),
    );
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::LoadWorktreeStatus { repo_id: rid } if *rid == repo_id)),
        "with no pending change the lane should stop replaying, got {effects:?}"
    );
    assert!(
        !state.repos[0].loads_in_flight.any_in_flight(),
        "in-flight flags should settle once no refresh is pending"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert!(
        has_worktree_status_effect(&effects, repo_id),
        "subsequent real worktree events should still trigger status refresh"
    );
}

#[test]
fn external_worktree_refresh_coalesces_status_while_status_is_in_flight() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectDiff {
            repo_id: RepoId(1),
            target: DiffTarget::WorkingTree {
                path: PathBuf::from("crates/worktree-ui-gpui/src/smoke_tests.rs"),
                area: DiffArea::Unstaged,
            },
        },
    );

    let effects1 = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert!(
        has_worktree_status_effect(&effects1, RepoId(1)),
        "expected first refresh to request status"
    );
    assert!(
        effects1.iter().any(|e| matches!(
            e,
            Effect::LoadDiff {
                repo_id: RepoId(1),
                ..
            }
        )),
        "expected first refresh to request diff reload"
    );

    let effects2 = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id: RepoId(1),
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert!(
        !has_worktree_status_effect(&effects2, RepoId(1)),
        "coalesced worktree refresh should not emit duplicate status effects, got {effects2:?}"
    );
    assert!(
        effects2.iter().any(|e| matches!(
            e,
            Effect::LoadDiff {
                repo_id: RepoId(1),
                ..
            }
        )),
        "selected diff should still refresh on subsequent worktree changes"
    );
    assert!(
        effects2.iter().any(|e| matches!(
            e,
            Effect::LoadDiffFile {
                repo_id: RepoId(1),
                ..
            }
        )),
        "selected diff file should still refresh on subsequent worktree changes"
    );
}

#[test]
fn reload_repo_sets_sections_loading_and_emits_refresh_effects() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(RepoId(1));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::ReloadRepo { repo_id: RepoId(1) },
    );

    let repo_state = &state.repos[0];
    assert!(repo_state.head_branch.is_loading());
    assert!(repo_state.branches.is_loading());
    assert!(repo_state.tags.is_loading());
    assert!(repo_state.remote_tags.is_loading());
    assert!(repo_state.remotes.is_loading());
    assert!(repo_state.remote_branches.is_loading());
    assert!(repo_state.status.is_loading());
    assert!(repo_state.worktree_status_is_loading());
    assert!(repo_state.staged_status_is_loading());
    assert!(repo_state.log.is_loading());
    assert!(!repo_state.history_state.log_loading_more);
    assert!(repo_state.merge_commit_message.is_loading());
    assert!(repo_state.submodules.is_loading());
    assert!(has_status_refresh_effects(&effects, RepoId(1)));
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadTags { repo_id } if *repo_id == RepoId(1)
        )),
        "tags should auto-load in the background on repo reload"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadRemoteTags { repo_id } if *repo_id == RepoId(1)
        )),
        "remote tags should auto-load in the background on repo reload"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadSubmodules { repo_id } if *repo_id == RepoId(1)
        )),
        "submodules should auto-load in the background on repo reload"
    );
}

fn state_with_blamed_unstaged_diff() -> (AppState, RepoId) {
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].set_status(Loadable::Ready(Arc::new(RepoStatus::default())));
    state.repos[0].diff_state.diff_target = Some(DiffTarget::WorkingTree {
        path: PathBuf::from("src/lib.rs"),
        area: DiffArea::Unstaged,
    });
    state.repos[0].history_state.blame_path = Some(PathBuf::from("src/lib.rs"));
    state.repos[0].history_state.blame_source = Some(
        worktree_core::domain::BlameSource::WorkingTree(DiffArea::Unstaged),
    );
    state.repos[0].history_state.blame = Loadable::Ready(std::sync::Arc::new(vec![
        worktree_core::services::BlameLine {
            commit_id: Arc::from("1111111111111111111111111111111111111111"),
            author: Arc::from("Ada"),
            author_time_unix: Some(1_700_000_000),
            summary: Arc::from("initial"),
            body: None,
            line: "let x = 1;".to_string(),
            prior_exists: true,
            source_path: None,
            prior_commit: None,
        },
    ]));
    (state, repo_id)
}

#[test]
fn repo_externally_changed_worktree_keeps_blame_until_content_changes() {
    // A worktree/index event reloads the diff, but the reload may well find the
    // same bytes (a window-focus refresh, a touch, a save that changed nothing).
    // Blame is expensive — it shells out to `git blame` — so it must survive
    // until `diff_loaded`/`diff_file_loaded` sees the content actually differ.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_blamed_unstaged_diff();

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );

    // The working-tree diff still reloads against the (possibly new) content...
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiff { repo_id: id, .. } if *id == repo_id)),
        "external worktree change must reload the diff"
    );
    // ...but the annotations stay painted until that reload proves them stale.
    assert!(
        matches!(state.repos[0].history_state.blame, Loadable::Ready(_)),
        "a worktree event alone must not invalidate blame"
    );
}

#[test]
fn repo_externally_changed_git_state_invalidates_loaded_blame() {
    // A moved HEAD (external commit / checkout / rebase) can leave the patch
    // byte-identical while every line's attribution changes, and no downstream
    // content comparison can detect that — so git-state events drop blame up
    // front, preserving the target so it reloads for the same file.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_blamed_unstaged_diff();
    let previous = match &state.repos[0].history_state.blame {
        Loadable::Ready(lines) => Arc::clone(lines),
        other => panic!("expected a loaded blame, got {other:?}"),
    };

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::all(),
            worktree_paths: None,
        },
    );

    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadDiff { repo_id: id, .. } if *id == repo_id)),
        "a git-state change must reload the diff"
    );
    assert!(
        matches!(state.repos[0].history_state.blame, Loadable::NotLoaded),
        "blame must be invalidated when refs may have moved"
    );
    // The outgoing annotations are held over so the column does not blank while
    // the reload runs.
    assert!(
        state.repos[0]
            .history_state
            .retained_blame_while_loading
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &previous)),
        "the previous annotations must be retained while blame reloads"
    );
    assert_eq!(
        state.repos[0].history_state.blame_path.as_deref(),
        Some(std::path::Path::new("src/lib.rs"))
    );
    assert_eq!(
        state.repos[0].history_state.blame_source,
        Some(worktree_core::domain::BlameSource::WorkingTree(
            DiffArea::Unstaged
        ))
    );
}

#[test]
fn reload_repo_clears_stale_navigation_history() {
    // Regression: a full reload may rewrite history (rebase/amend), so saved
    // back/forward snapshots can reference commits that no longer resolve. The
    // nav stacks must start fresh rather than letting Back restore a dead view.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(repo_id);

    let commit_a = CommitId("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());
    let commit_b = CommitId("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into());
    let snap = |c: &CommitId| crate::model::MainViewSnapshot {
        diff_target: Some(DiffTarget::Commit {
            commit_id: c.clone(),
            path: Some(PathBuf::from("src/lib.rs")),
        }),
        content_preview: false,
        edit_mode: false,
        selected_commit: Some(c.clone()),
        range_selection: None,
        worktree_selection: None,
    };
    state.repos[0].nav_history.record(snap(&commit_a));
    state.repos[0].nav_history.record(snap(&commit_b));
    state.repos[0]
        .view_history
        .record(crate::model::ViewHistoryEntry {
            source: worktree_core::domain::FileSource::Commit(commit_a.clone()),
            path: PathBuf::from("src/lib.rs"),
        });
    // Make the live view match the nav tail so the reduce-wrapper's reconcile is
    // a no-op and the stack survives intact up to the point ReloadRepo runs.
    state.repos[0].diff_state.diff_target = Some(DiffTarget::Commit {
        commit_id: commit_b.clone(),
        path: Some(PathBuf::from("src/lib.rs")),
    });
    state.repos[0].set_selected_commit(Some(commit_b.clone()));
    assert_eq!(state.repos[0].nav_history.entries.len(), 2);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::ReloadRepo { repo_id },
    );

    // The stale back-stack entry (commit A) must be gone — without the clear it
    // would survive as `entries[0]` while only the tail gets folded over.
    assert!(
        !state.repos[0]
            .nav_history
            .entries
            .iter()
            .any(|s| s.selected_commit.as_ref() == Some(&commit_a)),
        "stale nav back-stack entry must be cleared on reload"
    );
    assert!(
        state.repos[0].nav_history.entries.len() <= 1,
        "only the post-reload current view may remain in nav_history"
    );
    assert!(
        state.repos[0].view_history.entries.is_empty(),
        "view_history must be cleared on reload"
    );
}

#[test]
fn reload_repo_clears_a_stale_comparison_mark() {
    // A reload can follow a reset or a dropped branch that took the marked
    // commit with it. Keeping the mark would leave the context menus offering a
    // "Compare with …" whose only possible outcome is a backend error.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(repo_id);
    state.repos[0].comparison_mark = Some(crate::model::ComparisonMark {
        commit_id: CommitId("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
        label: "feature".into(),
    });

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::ReloadRepo { repo_id },
    );

    assert!(
        state.repos[0].comparison_mark.is_none(),
        "a mark that a reload may have invalidated must not survive it"
    );
}

#[test]
fn load_more_history_emits_paginated_load_log_effect() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.log = Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("c1".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "s1".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    }));
    repo_state.history_state.log_loading_more = false;

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadMoreHistory { repo_id: RepoId(1) },
    );

    let repo_state = &state.repos[0];
    assert!(repo_state.history_state.log_loading_more);
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadLog {
            repo_id: RepoId(1),
            scope: LogScope::CurrentBranch,
            author: None,
            limit: 200,
            cursor: Some(_),
            ..
        }]
    ));
}

#[test]
fn set_history_scope_emits_load_log_effect_for_every_history_mode() {
    for target_scope in [
        LogScope::FullReachable,
        LogScope::FirstParent,
        LogScope::NoMerges,
        LogScope::MergesOnly,
        LogScope::AllBranches,
    ] {
        let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
        let id_alloc = AtomicU64::new(1);
        let mut state = AppState::default();
        state.repos.push(RepoState::new_opening(
            RepoId(1),
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        ));
        state.active_repo = Some(RepoId(1));

        let repo_state = &mut state.repos[0];
        repo_state.history_state.history_scope = if target_scope == LogScope::FullReachable {
            LogScope::FirstParent
        } else {
            LogScope::FullReachable
        };
        repo_state.set_log(Loadable::Ready(Arc::new(LogPage {
            commits: vec![Commit {
                signed: false,
                id: CommitId("old".into()),
                parent_ids: worktree_core::domain::CommitParentIds::new(),
                summary: "old".into(),
                author: "a".into(),
                time: SystemTime::UNIX_EPOCH,
            }],
            next_cursor: None,
        })));

        let effects = reduce(
            &mut repos,
            &id_alloc,
            &mut state,
            Msg::SetHistoryScope {
                repo_id: RepoId(1),
                scope: target_scope,
            },
        );

        let repo_state = &state.repos[0];
        assert_eq!(repo_state.history_state.history_scope, target_scope);
        assert!(repo_state.log.is_loading());
        assert!(
            repo_state
                .history_state
                .retained_log_while_loading
                .is_some(),
            "expected retained history page while switching to {target_scope:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::LoadLog {
                    repo_id: RepoId(1),
                    scope,
                    cursor: None,
                    ..
                } if *scope == target_scope
            )),
            "expected LoadLog({target_scope:?}) effect, got {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::PersistRepoHistoryMode {
                    repo_id: Some(RepoId(1)),
                    mode,
                    ..
                } if *mode == target_scope
            )),
            "expected async history mode persist effect for {target_scope:?}, got {effects:?}"
        );
    }
}

#[test]
fn set_history_scope_retains_ready_log_while_loading() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let retained_page = Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("c1".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "s1".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: None,
    });
    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.set_log(Loadable::Ready(Arc::clone(&retained_page)));

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryScope {
            repo_id: RepoId(1),
            scope: LogScope::AllBranches,
        },
    );

    let repo_state = &state.repos[0];
    assert!(repo_state.log.is_loading());
    let retained = repo_state
        .history_state
        .retained_log_while_loading
        .as_ref()
        .expect("scope switch should retain the previous ready log while loading");
    assert!(Arc::ptr_eq(retained, &retained_page));
}

#[test]
fn stale_log_loaded_result_replays_latest_pending_scope_switch() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::FullReachable;
    repo_state.set_log(Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("old".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "old".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: None,
    })));
    let seq = repo_state
        .loads_in_flight
        .request_log(crate::model::PendingLogLoad {
            scope: LogScope::FullReachable,
            author: None,
            refs: Vec::new(),
            limit: 200,
            cursor: None,
        })
        .expect("the first walk starts immediately");

    // Each switch supersedes the walk in flight and is dispatched at once,
    // rather than queueing behind a walk that may run for tens of seconds.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryScope {
            repo_id: RepoId(1),
            scope: LogScope::AllBranches,
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadLog {
                scope: LogScope::AllBranches,
                cursor: None,
                ..
            }
        )),
        "expected the scope switch to start its load immediately, got {effects:?}"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryScope {
            repo_id: RepoId(1),
            scope: LogScope::NoMerges,
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadLog {
                scope: LogScope::NoMerges,
                cursor: None,
                ..
            }
        )),
        "expected the second scope switch to start immediately too, got {effects:?}"
    );
    assert_eq!(
        state.repos[0].history_state.history_scope,
        LogScope::NoMerges
    );
    assert!(state.repos[0].log.is_loading());

    // The first walk finally answers. It is superseded, so it must neither land
    // in the log nor disturb the walk that replaced it.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::FullReachable,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![],
                next_cursor: None,
            }),
        }),
    );

    assert!(state.repos[0].log.is_loading());
    assert!(!state.repos[0].history_state.log_loading_more);
    assert!(
        effects.is_empty(),
        "a superseded reply must not schedule anything, got {effects:?}"
    );
    assert!(
        !state.repos[0].loads_in_flight.is_active_log_reply(seq),
        "the superseded walk must no longer be the active one"
    );
}

#[test]
fn load_more_history_noops_when_no_next_cursor() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.log = Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("c1".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "s1".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: None,
    }));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadMoreHistory { repo_id: RepoId(1) },
    );

    let repo_state = &state.repos[0];
    assert!(!repo_state.history_state.log_loading_more);
    assert!(effects.is_empty());
}

#[test]
fn log_loaded_appends_when_loading_more() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.log = Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("c1".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "s1".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    }));
    repo_state.history_state.log_loading_more = true;
    let log_before = (repo_state.log_rev, repo_state.history_state.log_rev);

    let seq = expect_log_reply(
        &mut state.repos[0],
        LogScope::CurrentBranch,
        None,
        Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    );

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::CurrentBranch,
            cursor: Some(LogCursor {
                last_seen: CommitId("c1".into()),
                resume_from: None,
                resume_token: None,
            }),
            result: Ok(LogPage {
                commits: vec![Commit {
                    signed: false,
                    id: CommitId("c2".into()),
                    parent_ids: worktree_core::domain::CommitParentIds::new(),
                    summary: "s2".into(),
                    author: "a".into(),
                    time: SystemTime::UNIX_EPOCH,
                }],
                next_cursor: None,
            }),
        }),
    );

    let repo_state = &state.repos[0];
    assert!(!repo_state.history_state.log_loading_more);
    assert!(repo_state.log_rev > log_before.0);
    assert!(repo_state.history_state.log_rev > log_before.1);
    let Loadable::Ready(page) = &repo_state.log else {
        panic!("expected log ready");
    };
    assert_eq!(page.commits.len(), 2);
    assert_eq!(page.commits[0].id.as_ref(), "c1");
    assert_eq!(page.commits[1].id.as_ref(), "c2");
    assert_eq!(page.next_cursor, None);
}

#[test]
fn log_loaded_reconciles_commit_multi_selection() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let commit = |id: &str| Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: worktree_core::domain::CommitParentIds::new(),
        summary: "s".into(),
        author: "a".into(),
        time: SystemTime::UNIX_EPOCH,
    };

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.history_state.multi_selection = crate::model::CommitMultiSelection {
        commits: vec![CommitId("kept".into()), CommitId("gone".into())],
        anchor: Some(CommitId("gone".into())),
        anchor_index: Some(1),
        anchor_log_rev: Some(repo_state.history_state.log_rev),
    };

    let seq = expect_log_reply(&mut state.repos[0], LogScope::CurrentBranch, None, None);

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::CurrentBranch,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![commit("kept"), commit("other")],
                next_cursor: None,
            }),
        }),
    );

    let sel = &state.repos[0].history_state.multi_selection;
    assert_eq!(sel.commits, vec![CommitId("kept".into())]);
    assert_eq!(sel.anchor, None);
    assert_eq!(sel.anchor_index, None);
    assert_eq!(sel.anchor_log_rev, None);
}

/// A reveal asks git to resolve the reference before touching the selection, so
/// an abbreviation lands on the full id and the details pane fills in without
/// the log having paged anywhere near the commit.
#[test]
fn reveal_commit_resolves_an_abbreviation_and_shows_it_immediately() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let reference = CommitId("deadbee".into());
    let full = CommitId("deadbeef0123456789abcdef0123456789abcdef".into());

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RevealCommit {
            repo_id: RepoId(1),
            reference: reference.clone(),
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::ResolveCommitForReveal { reference: r, .. } if *r == reference
        )),
        "asking to reveal should resolve the reference, got {effects:?}"
    );
    // Nothing is selected until it is known to exist.
    assert_eq!(state.repos[0].history_state.selected_commit, None);

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::CommitRevealResolved {
            repo_id: RepoId(1),
            reference,
            result: Ok(commit_details_for(&full, "the reland")),
        }),
    );

    let history = &state.repos[0].history_state;
    assert_eq!(history.selected_commit.as_ref(), Some(&full));
    assert_eq!(history.reveal_target.as_ref(), Some(&full));
    assert!(
        matches!(&history.commit_details, Loadable::Ready(details) if details.id == full),
        "the details fetched to resolve the reference should be the ones shown"
    );
}

/// A commit search asks the backend and stamps the query its results answer;
/// a response whose revision has been superseded must not land. The stored
/// query is what lets the picker tell current results from an earlier
/// search's leftovers.
#[test]
fn search_commits_stores_query_and_drops_superseded_results() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(repo_id);

    let commit = |id: &str| Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: worktree_core::domain::CommitParentIds::new(),
        summary: "found it".into(),
        author: "you".into(),
        time: SystemTime::UNIX_EPOCH,
    };

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SearchCommits {
            repo_id,
            query: "  fix  ".to_string(),
        },
    );
    let request_rev = state.repos[0].commit_search_rev;
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::SearchCommits { query, request_rev: rev, .. }
                if query == "fix" && *rev == request_rev
        )),
        "dispatching a search should ask the backend, got {effects:?}"
    );
    assert!(state.repos[0].commit_search.is_loading());
    assert_eq!(state.repos[0].commit_search_query.as_deref(), Some("fix"));

    // A second search supersedes the first before its response arrives.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SearchCommits {
            repo_id,
            query: "regression".to_string(),
        },
    );
    let superseded_rev = request_rev;

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::CommitsSearched {
            repo_id,
            request_rev: superseded_rev,
            result: Ok(vec![commit("1111111111111111111111111111111111111111")]),
        }),
    );
    assert!(
        state.repos[0].commit_search.is_loading(),
        "a superseded response must not land"
    );
    assert_eq!(
        state.repos[0].commit_search_query.as_deref(),
        Some("regression")
    );

    let current_rev = state.repos[0].commit_search_rev;
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::CommitsSearched {
            repo_id,
            request_rev: current_rev,
            result: Ok(vec![
                commit("2222222222222222222222222222222222222222"),
                commit("3333333333333333333333333333333333333333"),
            ]),
        }),
    );
    assert!(effects.is_empty());
    assert!(matches!(
        &state.repos[0].commit_search,
        Loadable::Ready(results) if results.len() == 2
    ));
    assert_eq!(
        state.repos[0].commit_search_query.as_deref(),
        Some("regression")
    );

    // An empty query is a reset, not a search.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SearchCommits {
            repo_id,
            query: "   ".to_string(),
        },
    );
    assert!(matches!(&state.repos[0].commit_search, Loadable::NotLoaded));
    assert_eq!(state.repos[0].commit_search_query, None);
}

/// A hex-looking run that is not a commit — a Gerrit change id, say — must fail
/// loudly and cheaply instead of sending the log walking to the root.
#[test]
fn reveal_commit_reports_an_unresolvable_reference_without_selecting() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let reference = CommitId("7a5d480873e839444e4e188ffa87f9c635e2fb81".into());
    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RevealCommit {
            repo_id: RepoId(1),
            reference: reference.clone(),
        },
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::CommitRevealResolved {
            repo_id: RepoId(1),
            reference,
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Backend("unknown revision".to_string()),
            )),
        }),
    );

    assert!(effects.is_empty(), "a dead reference starts no work");
    let history = &state.repos[0].history_state;
    assert_eq!(history.selected_commit, None);
    assert_eq!(history.reveal_target, None);
    assert!(
        state
            .notifications
            .iter()
            .any(|note| note.message.contains("Could not find commit")),
        "the user has to be told the reference went nowhere"
    );
}

/// A "load more" only ever grows the page, so a selection it does not contain
/// has not been paged in yet — it has not vanished. Clearing it there used to
/// make the details pane flip to the working tree once per batch for the whole
/// length of a reveal.
#[test]
fn log_loaded_keeps_a_not_yet_paged_selection_when_loading_more() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let commit = |id: &str| Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: worktree_core::domain::CommitParentIds::new(),
        summary: "s".into(),
        author: "a".into(),
        time: SystemTime::UNIX_EPOCH,
    };

    let deep = CommitId("deep".into());
    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.set_log(Loadable::Ready(Arc::new(LogPage {
        commits: vec![commit("c1")],
        next_cursor: Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    })));
    repo_state.set_selected_commit(Some(deep.clone()));
    repo_state.history_state.multi_selection = crate::model::CommitMultiSelection {
        commits: vec![deep.clone()],
        anchor: Some(deep.clone()),
        anchor_index: Some(0),
        anchor_log_rev: Some(repo_state.history_state.log_rev),
    };

    let seq = expect_log_reply(
        &mut state.repos[0],
        LogScope::CurrentBranch,
        None,
        Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    );

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::CurrentBranch,
            cursor: Some(LogCursor {
                last_seen: CommitId("c1".into()),
                resume_from: None,
                resume_token: None,
            }),
            result: Ok(LogPage {
                commits: vec![commit("c2")],
                next_cursor: Some(LogCursor {
                    last_seen: CommitId("c2".into()),
                    resume_from: None,
                    resume_token: None,
                }),
            }),
        }),
    );

    let history = &state.repos[0].history_state;
    assert_eq!(history.selected_commit.as_ref(), Some(&deep));
    assert_eq!(history.multi_selection.commits, vec![deep]);
}

/// A first page *replaces* the log, so it can genuinely retire a selection —
/// unless a reveal is still walking toward exactly that commit, which is what a
/// mid-reveal scope switch does.
#[test]
fn log_loaded_first_page_keeps_the_commit_a_reveal_is_walking_toward() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let commit = |id: &str| Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: worktree_core::domain::CommitParentIds::new(),
        summary: "s".into(),
        author: "a".into(),
        time: SystemTime::UNIX_EPOCH,
    };

    let target = CommitId("target".into());
    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.set_reveal_target(Some(target.clone()));
    repo_state.set_selected_commit(Some(target.clone()));
    repo_state.history_state.multi_selection = crate::model::CommitMultiSelection {
        commits: vec![target.clone()],
        anchor: Some(target.clone()),
        anchor_index: Some(0),
        anchor_log_rev: Some(repo_state.history_state.log_rev),
    };

    let seq = expect_log_reply(&mut state.repos[0], LogScope::CurrentBranch, None, None);

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::CurrentBranch,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![commit("other")],
                next_cursor: None,
            }),
        }),
    );

    let history = &state.repos[0].history_state;
    assert_eq!(history.selected_commit.as_ref(), Some(&target));
    assert_eq!(history.multi_selection.commits, vec![target]);
}

#[test]
fn log_loaded_appends_when_loading_more_re_shares_history_log_arc() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.set_log(Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("c1".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "s1".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    })));
    repo_state.history_state.log_loading_more = true;

    let seq = expect_log_reply(
        &mut state.repos[0],
        LogScope::CurrentBranch,
        None,
        Some(LogCursor {
            last_seen: CommitId("c1".into()),
            resume_from: None,
            resume_token: None,
        }),
    );

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::CurrentBranch,
            cursor: Some(LogCursor {
                last_seen: CommitId("c1".into()),
                resume_from: None,
                resume_token: None,
            }),
            result: Ok(LogPage {
                commits: vec![Commit {
                    signed: false,
                    id: CommitId("c2".into()),
                    parent_ids: worktree_core::domain::CommitParentIds::new(),
                    summary: "s2".into(),
                    author: "a".into(),
                    time: SystemTime::UNIX_EPOCH,
                }],
                next_cursor: Some(LogCursor {
                    last_seen: CommitId("c2".into()),
                    resume_from: None,
                    resume_token: None,
                }),
            }),
        }),
    );

    let repo_state = &state.repos[0];
    let Loadable::Ready(repo_log) = &repo_state.log else {
        panic!("expected repo log ready");
    };
    let Loadable::Ready(history_log) = &repo_state.history_state.log else {
        panic!("expected history log ready");
    };

    assert!(Arc::ptr_eq(repo_log, history_log));
    assert_eq!(repo_log.commits.len(), 2);
    assert_eq!(repo_log.commits[1].id.as_ref(), "c2");
    assert_eq!(
        repo_log
            .next_cursor
            .as_ref()
            .and_then(|cursor| cursor.last_seen.as_ref().strip_prefix('c')),
        Some("2")
    );
}

#[test]
fn log_loaded_clears_retained_scope_switch_log() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let repo_state = &mut state.repos[0];
    repo_state.history_state.history_scope = LogScope::CurrentBranch;
    repo_state.set_log(Loadable::Ready(Arc::new(LogPage {
        commits: vec![Commit {
            signed: false,
            id: CommitId("old".into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: "old".into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        }],
        next_cursor: None,
    })));

    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryScope {
            repo_id: RepoId(1),
            scope: LogScope::AllBranches,
        },
    );

    assert!(
        state.repos[0]
            .history_state
            .retained_log_while_loading
            .is_some()
    );

    let seq = expect_log_reply(&mut state.repos[0], LogScope::AllBranches, None, None);
    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: LogScope::AllBranches,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![Commit {
                    signed: false,
                    id: CommitId("new".into()),
                    parent_ids: worktree_core::domain::CommitParentIds::new(),
                    summary: "new".into(),
                    author: "a".into(),
                    time: SystemTime::UNIX_EPOCH,
                }],
                next_cursor: None,
            }),
        }),
    );

    let repo_state = &state.repos[0];
    assert!(matches!(repo_state.log, Loadable::Ready(_)));
    assert!(
        repo_state
            .history_state
            .retained_log_while_loading
            .is_none()
    );
}

#[test]
fn log_loaded_initial_paginated_page_keeps_append_slack() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let commits: Vec<Commit> = (0..600)
        .map(|ix| Commit {
            signed: false,
            id: CommitId(format!("{ix:040x}").into()),
            parent_ids: worktree_core::domain::CommitParentIds::new(),
            summary: format!("s{ix}").into(),
            author: "a".into(),
            time: SystemTime::UNIX_EPOCH,
        })
        .collect();
    let last_seen = commits.last().expect("last commit").id.clone();
    let history_scope = state.repos[0].history_state.history_scope;
    let seq = expect_log_reply(&mut state.repos[0], history_scope, None, None);

    let _effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope: history_scope,
            cursor: None,
            result: Ok(LogPage {
                commits,
                next_cursor: Some(LogCursor {
                    last_seen,
                    resume_from: None,
                    resume_token: None,
                }),
            }),
        }),
    );

    let Loadable::Ready(page) = &state.repos[0].log else {
        panic!("expected log ready");
    };
    assert!(page.commits.capacity() >= page.commits.len() + 512);
}

// --- Revision counter regression tests ---

#[test]
fn log_loaded_bumps_log_rev() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(2);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);

    let log_before = (state.repos[0].log_rev, state.repos[0].history_state.log_rev);
    let history_scope = state.repos[0].history_state.history_scope;
    let seq = expect_log_reply(&mut state.repos[0], history_scope, None, None);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id,
            seq,
            scope: history_scope,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![Commit {
                    signed: false,
                    id: CommitId("c1".into()),
                    parent_ids: worktree_core::domain::CommitParentIds::new(),
                    summary: "s1".into(),
                    author: "a".into(),
                    time: SystemTime::UNIX_EPOCH,
                }],
                next_cursor: None,
            }),
        }),
    );

    assert!(
        state.repos[0].log_rev > log_before.0,
        "repo log_rev should bump after LogLoaded"
    );
    assert!(
        state.repos[0].history_state.log_rev > log_before.1,
        "log_rev should bump after LogLoaded"
    );
}

#[test]
fn detached_head_target_tracks_current_branch_log_head() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(2);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].history_state.history_scope = LogScope::CurrentBranch;

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded {
            repo_id,
            result: Ok("HEAD".to_string()),
        }),
    );

    let seq = expect_log_reply(&mut state.repos[0], LogScope::CurrentBranch, None, None);
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id,
            seq,
            scope: LogScope::CurrentBranch,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![
                    Commit {
                        signed: false,
                        id: CommitId("c1".into()),
                        parent_ids: smallvec::smallvec![CommitId("c0".into())],
                        summary: "s1".into(),
                        author: "a".into(),
                        time: SystemTime::UNIX_EPOCH,
                    },
                    Commit {
                        signed: false,
                        id: CommitId("c0".into()),
                        parent_ids: worktree_core::domain::CommitParentIds::new(),
                        summary: "s0".into(),
                        author: "a".into(),
                        time: SystemTime::UNIX_EPOCH,
                    },
                ],
                next_cursor: None,
            }),
        }),
    );

    assert_eq!(
        state.repos[0].detached_head_commit,
        Some(CommitId("c1".into()))
    );
}

#[test]
fn filtered_current_branch_logs_do_not_backfill_detached_head_target() {
    for (scope, commits, expected_first_visible) in [
        (
            LogScope::NoMerges,
            vec![Commit {
                signed: false,
                id: CommitId("visible-non-merge".into()),
                parent_ids: smallvec::smallvec![CommitId("hidden-head".into())],
                summary: "visible".into(),
                author: "a".into(),
                time: SystemTime::UNIX_EPOCH,
            }],
            CommitId("visible-non-merge".into()),
        ),
        (
            LogScope::MergesOnly,
            vec![Commit {
                signed: false,
                id: CommitId("visible-merge".into()),
                parent_ids: smallvec::smallvec![CommitId("p0".into()), CommitId("p1".into())],
                summary: "merge".into(),
                author: "a".into(),
                time: SystemTime::UNIX_EPOCH,
            }],
            CommitId("visible-merge".into()),
        ),
    ] {
        let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
        let id_alloc = AtomicU64::new(2);
        let mut state = AppState::default();
        let repo_id = RepoId(1);
        repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
        state.repos.push(RepoState::new_opening(
            repo_id,
            RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        ));
        state.active_repo = Some(repo_id);
        state.repos[0].history_state.history_scope = scope;

        reduce(
            &mut repos,
            &id_alloc,
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::HeadBranchLoaded {
                repo_id,
                result: Ok("HEAD".to_string()),
            }),
        );

        let seq = expect_log_reply(&mut state.repos[0], scope, None, None);
        reduce(
            &mut repos,
            &id_alloc,
            &mut state,
            Msg::Internal(crate::msg::InternalMsg::LogLoaded {
                repo_id,
                seq,
                scope,
                cursor: None,
                result: Ok(LogPage {
                    commits,
                    next_cursor: None,
                }),
            }),
        );

        assert!(
            state.repos[0].detached_head_commit.is_none(),
            "{scope:?} should not infer detached HEAD from first visible commit {expected_first_visible}"
        );
    }
}

#[test]
fn set_history_scope_bumps_log_rev() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(2);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);

    let log_before = (state.repos[0].log_rev, state.repos[0].history_state.log_rev);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryScope {
            repo_id,
            scope: LogScope::AllBranches,
        },
    );

    assert!(
        state.repos[0].log_rev > log_before.0,
        "repo log_rev should bump after SetHistoryScope"
    );
    assert!(
        state.repos[0].history_state.log_rev > log_before.1,
        "log_rev should bump after SetHistoryScope"
    );
}

#[test]
fn status_loaded_bumps_status_rev() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(2);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);

    let status_before = state.repos[0].status_rev;

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::StatusLoaded {
            repo_id,
            result: Ok(RepoStatus::default()),
        }),
    );

    assert!(
        state.repos[0].status_rev > status_before,
        "status_rev should bump after StatusLoaded"
    );
}

#[test]
fn external_tags_change_reloads_tags() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].set_tags(Loadable::Ready(vec![worktree_core::domain::Tag {
        name: "v1.0.0".to_string(),
        target: CommitId("abc123".into()),
        created_at: None,
    }]));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange {
                git_state: true,
                tags: true,
                ..Default::default()
            },
            worktree_paths: None,
        },
    );

    assert!(
        matches!(state.repos[0].tags, Loadable::NotLoaded),
        "tags should be reset to NotLoaded on external tags change"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadTags { repo_id: id } if *id == repo_id)),
        "expected LoadTags effect on external tags change"
    );
}

#[test]
fn external_git_state_change_without_tags_flag_does_not_reload_tags() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].set_tags(Loadable::Ready(vec![worktree_core::domain::Tag {
        name: "v1.0.0".to_string(),
        target: CommitId("abc123".into()),
        created_at: None,
    }]));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );

    assert!(
        !effects.iter().any(|e| matches!(e, Effect::LoadTags { .. })),
        "LoadTags should not fire for a git_state change without tags flag"
    );
    assert!(
        !matches!(state.repos[0].tags, Loadable::NotLoaded),
        "tags should remain Ready when tags flag is not set"
    );
}

#[test]
fn external_tags_change_without_git_state_flag_reloads_tags() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.repos[0].set_tags(Loadable::Ready(vec![worktree_core::domain::Tag {
        name: "v1.0.0".to_string(),
        target: CommitId("abc123".into()),
        created_at: None,
    }]));

    // The `tags` flag must drive a tag reload independently of `git_state`, so a
    // change that only sets `tags` still refreshes them.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange {
                git_state: false,
                tags: true,
                ..Default::default()
            },
            worktree_paths: None,
        },
    );

    assert!(
        matches!(state.repos[0].tags, Loadable::NotLoaded),
        "tags should be reset to NotLoaded when only the tags flag is set"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadTags { repo_id: id } if *id == repo_id)),
        "expected LoadTags effect when only the tags flag is set"
    );
}

/// Picking an author while a walk is already running must dispatch the new load
/// at once. Queueing it behind the old walk is what made filtering feel frozen
/// on a large repository, where a walk runs for tens of seconds and the
/// repo-load pool has one or two threads.
#[test]
fn author_filter_change_starts_its_load_while_a_walk_is_in_flight() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    let unfiltered = expect_log_reply(&mut state.repos[0], scope, None, None);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryAuthorFilter {
            repo_id: RepoId(1),
            author: Some("alice".to_string()),
        },
    );

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadLog { author: Some(author), cursor: None, .. } if author == "alice"
        )),
        "expected the filter change to start its load immediately, got {effects:?}"
    );
    assert!(
        !state.repos[0]
            .loads_in_flight
            .is_active_log_reply(unfiltered),
        "the unfiltered walk it replaced is no longer the active one"
    );
}

/// Same as the author-filter case: restricting history to a set of refs has to
/// dispatch its walk at once, not queue behind the in-flight one — the walk it
/// replaces is exactly the "whole repository" walk a filter exists to shorten.
#[test]
fn ref_filter_change_starts_its_load_while_a_walk_is_in_flight() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    state.repos[0].loads_in_flight.clear();
    let unfiltered = state.repos[0]
        .loads_in_flight
        .request_log(crate::model::PendingLogLoad {
            scope,
            author: None,
            refs: Vec::new(),
            limit: 200,
            cursor: None,
        })
        .expect("a declared log walk starts immediately");

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryRefFilters {
            repo_id: RepoId(1),
            refs: vec!["refs/tags/v2".to_string(), "refs/heads/dev".to_string()],
        },
    );

    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadLog { refs, cursor: None, .. }
                if refs == &["refs/heads/dev".to_string(), "refs/tags/v2".to_string()]
        )),
        "expected the ref filter change to start its load immediately with the sorted refs, got {effects:?}"
    );
    assert!(
        !state.repos[0]
            .loads_in_flight
            .is_active_log_reply(unfiltered),
        "the unfiltered walk it replaced is no longer the active one"
    );
}

/// The stored set is normalized (sorted, deduplicated) before it reaches the
/// state, the walk and the session file — three derivations that must agree —
/// and setting the same set again, however spelled, is a no-op: no reload, no
/// persist.
#[test]
fn ref_filter_change_normalizes_and_noops_when_unchanged() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryRefFilters {
            repo_id: RepoId(1),
            refs: vec![
                "refs/tags/b".to_string(),
                "refs/heads/a".to_string(),
                "refs/tags/b".to_string(),
            ],
        },
    );

    assert_eq!(
        state.repos[0].history_state.history_ref_filters,
        vec!["refs/heads/a".to_string(), "refs/tags/b".to_string()],
        "the stored set is sorted and deduplicated"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::PersistRepoHistoryRefFilters { refs, .. }
                if refs == &["refs/heads/a".to_string(), "refs/tags/b".to_string()]
        )),
        "the persist effect carries the normalized set, got {effects:?}"
    );

    let before = state.repos[0].history_state.log_rev;
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetHistoryRefFilters {
            repo_id: RepoId(1),
            // Same membership, different spelling: nothing may restart.
            refs: vec!["refs/tags/b".to_string(), "refs/heads/a".to_string()],
        },
    );

    assert_eq!(
        state.repos[0].history_state.history_ref_filters,
        vec!["refs/heads/a".to_string(), "refs/tags/b".to_string()]
    );
    assert!(
        effects.is_empty(),
        "re-setting the same filter set must be a no-op, got {effects:?}"
    );
    assert_eq!(
        state.repos[0].history_state.log_rev, before,
        "a no-op must not bump the log revision the repaints key on"
    );
}

/// A walk cancelled because a newer filter replaced it is routine, not a
/// failure: it must not raise a diagnostic or blank the history out.
#[test]
fn cancelled_log_reply_is_not_reported_as_an_error() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    let seq = expect_log_reply(&mut state.repos[0], scope, Some("alice"), None);
    state.repos[0].history_state.history_author_filter = Some("alice".to_string());
    state.repos[0].set_log(Loadable::Loading);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope,
            cursor: None,
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Cancelled,
            )),
        }),
    );

    assert!(
        state.repos[0].diagnostics.is_empty(),
        "a cancelled walk must not raise a diagnostic, got {:?}",
        state.repos[0].diagnostics
    );
    assert!(
        !matches!(state.repos[0].log, Loadable::Error(_)),
        "a cancelled walk must not blank the history out"
    );
    assert!(effects.is_empty(), "nothing to schedule, got {effects:?}");
}

/// Partial pages land as they are found, so a filter that has to walk a large
/// history shows what it has instead of the previous filter's rows.
#[test]
fn log_chunks_replace_the_page_progressively() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    let seq = expect_log_reply(&mut state.repos[0], scope, Some("alice"), None);
    state.repos[0].history_state.history_author_filter = Some("alice".to_string());
    state.repos[0].set_log(Loadable::Loading);

    let commit = |id: &str| Commit {
        signed: false,
        id: CommitId(id.into()),
        parent_ids: worktree_core::domain::CommitParentIds::new(),
        summary: id.into(),
        author: "alice".into(),
        time: SystemTime::UNIX_EPOCH,
    };
    let mut chunk = |commits: Vec<Commit>, scanned: u64, state: &mut AppState| {
        reduce(
            &mut repos,
            &id_alloc,
            state,
            Msg::Internal(crate::msg::InternalMsg::LogChunkLoaded {
                repo_id: RepoId(1),
                seq,
                commits,
                scanned,
            }),
        );
    };

    // A partial page shows through the retained slot while the walk runs. It is
    // deliberately not `Ready`: the walk has not answered "is there more?" yet,
    // and a `Ready` page with no cursor would answer "no" on its behalf.
    let partial = |state: &AppState| {
        state.repos[0]
            .history_state
            .retained_log_while_loading
            .clone()
    };

    // Nothing found yet: only the progress readout moves.
    chunk(Vec::new(), 50_000, &mut state);
    assert!(state.repos[0].log.is_loading());
    assert!(partial(&state).is_none());
    assert_eq!(state.repos[0].history_state.log_scan_progress, Some(50_000));

    chunk(vec![commit("c1")], 90_000, &mut state);
    assert!(state.repos[0].log.is_loading());
    assert_eq!(
        partial(&state)
            .expect("the first chunk shows its commits")
            .commits
            .len(),
        1
    );

    // Chunks are prefixes of one another, so a later one simply replaces.
    chunk(vec![commit("c1"), commit("c2")], 140_000, &mut state);
    assert_eq!(
        partial(&state)
            .expect("the second chunk extends the page")
            .commits
            .len(),
        2
    );
    assert_eq!(
        state.repos[0].history_state.log_scan_progress,
        Some(140_000)
    );

    // The finished page clears the progress.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: RepoId(1),
            seq,
            scope,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![commit("c1"), commit("c2"), commit("c3")],
                next_cursor: None,
            }),
        }),
    );
    let Loadable::Ready(page) = &state.repos[0].log else {
        panic!("expected the finished page");
    };
    assert_eq!(page.commits.len(), 3);
    assert_eq!(state.repos[0].history_state.log_scan_progress, None);
}

/// Chunks from a walk that a newer filter superseded must not paint rows for
/// the filter the user has already moved off.
#[test]
fn superseded_log_chunks_are_ignored() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    let superseded = expect_log_reply(&mut state.repos[0], scope, Some("alice"), None);
    // A newer filter takes over; the walk above is cancelled but still running.
    state.repos[0]
        .loads_in_flight
        .request_log(crate::model::PendingLogLoad {
            scope,
            author: Some("bob".to_string()),
            refs: Vec::new(),
            limit: 200,
            cursor: None,
        })
        .expect("the newer filter starts at once");
    state.repos[0].set_log(Loadable::Loading);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogChunkLoaded {
            repo_id: RepoId(1),
            seq: superseded,
            commits: vec![Commit {
                signed: false,
                id: CommitId("stale".into()),
                parent_ids: worktree_core::domain::CommitParentIds::new(),
                summary: "stale".into(),
                author: "alice".into(),
                time: SystemTime::UNIX_EPOCH,
            }],
            scanned: 10,
        }),
    );

    assert!(
        state.repos[0].log.is_loading(),
        "a superseded chunk must not paint rows"
    );
    assert!(
        state.repos[0]
            .history_state
            .retained_log_while_loading
            .is_none(),
        "a superseded chunk must not paint rows"
    );
    assert_eq!(state.repos[0].history_state.log_scan_progress, None);
}

/// A cancelled walk's reply never reaches the reducer — the repo-load guard
/// drops it — so whoever cancels has to take the progress readout down, or the
/// "Scanning history…" banner sits there with a frozen count.
#[test]
fn cancelling_repo_loads_clears_the_scan_progress() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    let scope = state.repos[0].history_state.history_scope;
    let seq = expect_log_reply(&mut state.repos[0], scope, Some("alice"), None);
    state.repos[0].set_log(Loadable::Loading);
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogChunkLoaded {
            repo_id: RepoId(1),
            seq,
            commits: Vec::new(),
            scanned: 400_000,
        }),
    );
    assert_eq!(
        state.repos[0].history_state.log_scan_progress,
        Some(400_000)
    );

    // A completed action invalidates every load in flight, the walk included.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id: RepoId(1),
            action: RepoActionKind::CheckoutBranch,
            result: Ok(()),
            paths: None,
        }),
    );

    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CancelRepoLoads { .. })),
        "the completed action must cancel the loads in flight, got {effects:?}"
    );
    assert_eq!(
        state.repos[0].history_state.log_scan_progress, None,
        "the banner must not outlive the walk it was counting for"
    );
}

/// Staging knows exactly which paths moved between the lanes, so its
/// completion refresh answers through the path-targeted status scan: one
/// pathspec `git status` merges in milliseconds, where the primary refresh's
/// full walk — which the gix staged cache can no longer serve, the action
/// having just rewritten the index — re-scans the whole worktree before the
/// staged panel moves.
#[test]
fn stage_finish_refreshes_the_status_lanes_through_the_targeted_scan() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    // The targeted lane merges onto a settled snapshot.
    state.repos[0].set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus::default())));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id,
            action: RepoActionKind::StagePaths,
            result: Ok(()),
            paths: Some(crate::msg::RepoPathList::new(vec![PathBuf::from(
                "src/lib.rs",
            )])),
        }),
    );

    let expected: std::sync::Arc<[PathBuf]> = vec![PathBuf::from("src/lib.rs")].into();
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadStatusForPaths { repo_id: candidate, paths: p }
                if *candidate == repo_id && **p == *expected
        )),
        "the staged paths answer through the targeted scan, got {effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadStatus { .. })),
        "the full worktree scan is coalesced away, not stacked on top"
    );
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadWorktreeStatus { .. } | Effect::LoadStagedStatus { .. }
        )),
        "the coarse per-lane scans are coalesced away too"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadHeadBranch { repo_id: candidate } if *candidate == repo_id
        )),
        "the primary refresh still covers the head-anchored panes"
    );
}

/// The targeted lane is an accelerator for a settled snapshot, not a
/// replacement for the first load: with nothing to merge onto (and after a
/// failed action, whose paths may not have moved at all) the full scan
/// answers as before.
#[test]
fn stage_finish_keeps_the_full_scan_without_a_settled_snapshot_or_on_failure() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    let paths = || {
        Some(crate::msg::RepoPathList::new(vec![PathBuf::from(
            "src/lib.rs",
        )]))
    };

    // No snapshot has settled yet: the merge has nothing to merge onto.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id,
            action: RepoActionKind::StagePaths,
            result: Ok(()),
            paths: paths(),
        }),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadStatus { repo_id: candidate } if *candidate == repo_id)),
        "nothing settled to merge onto — the full scan answers"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadStatusForPaths { .. })),
        "no targeted scan without a settled snapshot"
    );

    // A settled snapshot but a failed action: the paths may not have moved.
    state.repos[0].set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus::default())));
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id,
            action: RepoActionKind::StagePaths,
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Backend("boom".to_string()),
            )),
            paths: paths(),
        }),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadStatus { repo_id: candidate } if *candidate == repo_id)),
        "a failed action refreshes through the full scan as before"
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadStatusForPaths { .. })),
        "a failed action claims no targeted scan"
    );
}

/// The assume-unchanged manager's list: the loaded reply paints the state, and
/// a finished toggle reloads it only when a list was loaded at all — reopening
/// the dialog is what fetches the first one.
#[test]
fn assume_unchanged_list_loads_and_reloads_after_toggle() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));

    // A finished toggle with no list loaded must not fetch one behind the
    // user's back: the context-menu entry marks a file without ever opening
    // the manager.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id: RepoId(1),
            action: RepoActionKind::SetAssumeUnchanged,
            result: Ok(()),
            paths: None,
        }),
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadAssumeUnchanged { .. })),
        "no list was loaded, so no reload may be scheduled"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::AssumeUnchangedListLoaded {
            repo_id: RepoId(1),
            result: Ok(vec![PathBuf::from("src/big.bin")]),
        }),
    );
    assert!(effects.is_empty());
    let Loadable::Ready(list) = &state.repos[0].assume_unchanged else {
        panic!("expected the list to be Ready");
    };
    assert_eq!(list.as_slice(), [PathBuf::from("src/big.bin")]);
    assert!(state.repos[0].assume_unchanged_rev > 0);

    let rev_before = state.repos[0].assume_unchanged_rev;
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
            repo_id: RepoId(1),
            action: RepoActionKind::SetAssumeUnchanged,
            result: Ok(()),
            paths: None,
        }),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadAssumeUnchanged { repo_id } if *repo_id == RepoId(1))),
        "a finished toggle must reload the loaded list"
    );

    // A failed reload surfaces as an error loadable, not a stale list.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::AssumeUnchangedListLoaded {
            repo_id: RepoId(1),
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Backend("boom".to_string()),
            )),
        }),
    );
    assert!(matches!(
        state.repos[0].assume_unchanged,
        Loadable::Error(_)
    ));
    assert!(state.repos[0].assume_unchanged_rev > rev_before);
}

/// An open repository with nothing loaded, for the hover-message reducer tests
/// below.
fn hover_message_state(repo_id: RepoId) -> AppState {
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].set_open(Loadable::Ready(()));
    state.active_repo = Some(repo_id);
    state
}

fn hover_message_slot(state: &AppState) -> Option<(CommitId, Loadable<Arc<str>>)> {
    state.repos[0].hover_commit_message.clone()
}

#[test]
fn hovering_a_commit_reads_its_message_once_however_often_the_row_is_re_entered() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = hover_message_state(repo_id);
    let commit_id = CommitId("abc".into());

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadHoverCommitMessage { commit_id: c, .. } if *c == commit_id
        )),
        "the first hover has to issue the read, got {effects:?}"
    );
    assert!(matches!(
        hover_message_slot(&state),
        Some((id, Loadable::Loading)) if id == commit_id
    ));

    // The pointer wandering within the same row re-dispatches; the read is
    // already in flight, so it must not be issued a second time.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );
    assert!(effects.is_empty(), "a read in flight is not re-issued");

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HoverCommitMessageLoaded {
            repo_id,
            commit_id: commit_id.clone(),
            result: Ok("Fix the thing\n\nWhy it broke.".to_string()),
        }),
    );
    assert!(matches!(
        hover_message_slot(&state),
        Some((id, Loadable::Ready(message)))
            if id == commit_id && message.as_ref() == "Fix the thing\n\nWhy it broke."
    ));

    // And once it has arrived, re-hovering re-reads nothing at all.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );
    assert!(effects.is_empty(), "a message already held is not re-read");
}

#[test]
fn a_hover_message_that_failed_is_retried_on_the_next_hover() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = hover_message_state(repo_id);
    let commit_id = CommitId("abc".into());

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HoverCommitMessageLoaded {
            repo_id,
            commit_id: commit_id.clone(),
            result: Err(Error::new(ErrorKind::Backend("boom".to_string()))),
        }),
    );
    assert!(matches!(
        hover_message_slot(&state),
        Some((id, Loadable::Error(_))) if id == commit_id
    ));
    assert!(
        state.notifications.is_empty(),
        "a hover losing its race is not worth telling the user about, got {:?}",
        state.notifications
    );

    // Unlike the loading and loaded cases, a failure does not suppress the next
    // attempt -- otherwise one transient error kills the card for that commit
    // for the rest of the session.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadHoverCommitMessage { commit_id: c, .. } if *c == commit_id
        )),
        "a failed read is retried, got {effects:?}"
    );
}

#[test]
fn a_hover_message_for_a_commit_the_pointer_has_left_is_dropped() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = hover_message_state(repo_id);
    let first = CommitId("aaa".into());
    let second = CommitId("bbb".into());

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: first.clone(),
        },
    );
    // The pointer moves to another row before the first read comes back.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: second.clone(),
        },
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::HoverCommitMessageLoaded {
            repo_id,
            commit_id: first,
            result: Ok("the row the pointer already left".to_string()),
        }),
    );

    assert!(
        matches!(
            hover_message_slot(&state),
            Some((id, Loadable::Loading)) if id == second
        ),
        "the late result must not overwrite the row now under the pointer"
    );
}

#[test]
fn hovering_a_repository_that_is_not_open_yet_reads_nothing() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let repo_id = RepoId(1);
    let mut state = hover_message_state(repo_id);
    state.repos[0].set_open(Loadable::Loading);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadHoverCommitMessage {
            repo_id,
            commit_id: CommitId("abc".into()),
        },
    );

    assert!(effects.is_empty(), "got {effects:?}");
    assert!(
        hover_message_slot(&state).is_none(),
        "and nothing is recorded as being fetched"
    );
}

/// The multi-worktree scan is the most expensive thing the watcher can queue: a
/// full `status` walk of every *other* linked worktree, which (see the known gix
/// behaviour) does not honour the global gitignore and so traverses an un-ignored
/// `target/` or `node_modules/` in full. What keeps that off the hot path is that
/// the index is classified apart from the rest of the git dir: staging or
/// unstaging writes `.git/index` and nothing else, which cannot change any other
/// worktree's status and must not cost a scan. A linked worktree's own index
/// lives at `.git/worktrees/<name>/index`, which is *not* `is_git_index_path`, so
/// it still arrives as a git-state change and still earns one.
#[test]
fn an_index_only_change_does_not_rescan_the_other_worktrees() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    let repo_id = RepoId(1);

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::OpenRepo(PathBuf::from("/tmp/repo")),
    );
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id,
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
            repo: Arc::new(DummyRepo::new("/tmp/repo")),
        }),
    );

    let scans = |effects: &[Effect]| {
        effects
            .iter()
            .filter(
                |e| matches!(e, Effect::LoadWorktreeDirty { repo_id: id, .. } if *id == repo_id),
            )
            .count()
    };

    // The open-repo refresh already queued one; let it finish so the in-flight
    // guard is not what makes the next assertion pass.
    let settle = |repos: &mut FxHashMap<RepoId, Arc<dyn GitRepository>>,
                  state: &mut AppState,
                  id_alloc: &AtomicU64| {
        reduce(
            repos,
            id_alloc,
            state,
            Msg::Internal(crate::msg::InternalMsg::WorktreeDirtyLoaded {
                repo_id,
                result: Ok(Vec::new()),
            }),
        );
    };
    settle(&mut repos, &mut state, &id_alloc);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Index,
            worktree_paths: None,
        },
    );
    assert_eq!(
        scans(&effects),
        0,
        "an index write cannot change another worktree's status, so it must not \
         queue a walk of every one of them"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::GitState,
            worktree_paths: None,
        },
    );
    assert_eq!(
        scans(&effects),
        1,
        "a git-state change can move another worktree's HEAD or index, so it still \
         earns exactly one scan"
    );
    settle(&mut repos, &mut state, &id_alloc);
}

use crate::model::SidebarMode;
use worktree_core::domain::{ContributorCommit, FileEntry, FileSource};

fn state_with_loaded_file_browser(sidebar_mode: SidebarMode) -> (AppState, RepoId) {
    let mut state = AppState::default();
    let repo_id = RepoId(1);
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(repo_id);
    state.sidebar_mode = sidebar_mode;
    state.repos[0].set_open(Loadable::Ready(()));
    state.repos[0].set_status(Loadable::Ready(Arc::new(RepoStatus::default())));
    state.repos[0].file_browser.entries = Loadable::Ready(Arc::new(vec![FileEntry {
        name: "src".to_string(),
        path: Arc::new(PathBuf::from("src")),
        kind: worktree_core::domain::FileEntryKind::Directory,
        depth: 0,
    }]));
    state.repos[0]
        .file_browser
        .expanded_dirs
        .insert(Arc::new(PathBuf::from("src")));
    (state, repo_id)
}

fn file_browser_loads(effects: &[Effect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, Effect::LoadFileBrowser { .. }))
        .count()
}

#[test]
fn worktree_change_refreshes_the_visible_file_browser_without_blanking_it() {
    // The reported bug's second half: a new folder has to reach the tree already
    // on screen, without discarding the rows or their expansion on the way.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_loaded_file_browser(SidebarMode::Files);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );

    assert_eq!(file_browser_loads(&effects), 1);
    assert!(
        matches!(state.repos[0].file_browser.entries, Loadable::Ready(_)),
        "the rows must stay on screen until the new listing arrives"
    );
    assert!(
        state.repos[0]
            .file_browser
            .expanded_dirs
            .contains(&Arc::new(PathBuf::from("src"))),
        "a refresh must not collapse the tree"
    );
    assert!(!state.repos[0].file_browser.stale);
}

#[test]
fn worktree_change_only_marks_the_hidden_file_browser_stale() {
    // Walking the working directory is far costlier than the other loads, so it
    // must not run for every disk event while the sidebar shows branches.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_loaded_file_browser(SidebarMode::Branches);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );

    assert_eq!(file_browser_loads(&effects), 0);
    assert!(state.repos[0].file_browser.stale);

    // ...and the deferred work happens on the way back in.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetSidebarMode {
            mode: SidebarMode::Files,
        },
    );
    assert_eq!(file_browser_loads(&effects), 1);
}

#[test]
fn commit_browsing_ignores_worktree_changes() {
    // A commit's tree is immutable, so a disk event says nothing about it.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_loaded_file_browser(SidebarMode::Files);
    state.repos[0].file_browser.source =
        FileSource::Commit(CommitId("1111111111111111111111111111111111111111".into()));

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::all(),
            worktree_paths: None,
        },
    );

    assert_eq!(file_browser_loads(&effects), 0);
    assert!(!state.repos[0].file_browser.stale);
}

#[test]
fn a_burst_of_worktree_changes_coalesces_into_one_walk_at_a_time() {
    // Events arrive back to back; none may stack a second walk on the first.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_loaded_file_browser(SidebarMode::Files);

    let mut change = || {
        reduce(
            &mut repos,
            &id_alloc,
            &mut state,
            Msg::RepoExternallyChanged {
                repo_id,
                change: crate::msg::RepoExternalChange::Worktree,
                worktree_paths: None,
            },
        )
    };

    assert_eq!(file_browser_loads(&change()), 1);
    assert_eq!(file_browser_loads(&change()), 0);
    assert_eq!(file_browser_loads(&change()), 0);

    // The reply releases the lane and dispatches the request queued behind it, so
    // the changes that arrived mid-walk are not lost.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::FileBrowserLoaded {
            repo_id,
            source: FileSource::WorkingDirectory,
            result: Ok(Vec::new()),
        }),
    );
    assert_eq!(file_browser_loads(&effects), 1);
}

#[test]
fn a_reply_for_an_abandoned_source_still_releases_the_lane() {
    // Browsing to a commit mid-walk means the reply is for a source nobody wants
    // any more. It still has to end that walk, or the new listing never runs.
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = state_with_loaded_file_browser(SidebarMode::Files);

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: None,
        },
    );
    assert_eq!(file_browser_loads(&effects), 1);

    let commit_id = CommitId("1111111111111111111111111111111111111111".into());
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SetFileBrowserSource {
            repo_id,
            source: FileSource::Commit(commit_id.clone()),
        },
    );
    assert_eq!(
        file_browser_loads(&effects),
        0,
        "the live walk still holds the lane"
    );

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::FileBrowserLoaded {
            repo_id,
            source: FileSource::WorkingDirectory,
            result: Ok(Vec::new()),
        }),
    );
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::LoadFileBrowser {
                source: FileSource::Commit(_),
                ..
            }
        )),
        "the queued commit listing must dispatch once the live walk ends"
    );
    assert!(
        matches!(state.repos[0].file_browser.entries, Loadable::NotLoaded),
        "the stale live rows must not be adopted as the commit's tree"
    );
}

fn file_status(
    path: &str,
    kind: worktree_core::domain::FileStatusKind,
) -> Loadable<Arc<Vec<worktree_core::domain::FileStatus>>> {
    Loadable::Ready(Arc::new(vec![worktree_core::domain::FileStatus {
        path: PathBuf::from(path),
        kind,
        conflict: None,
    }]))
}

fn working_tree_test_state() -> (AppState, RepoId) {
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.active_repo = Some(RepoId(1));
    (state, RepoId(1))
}

#[test]
fn select_working_tree_summary_displaces_other_selections_and_synthesizes_details() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = working_tree_test_state();

    {
        let repo = &mut state.repos[0];
        repo.detached_head_commit = Some(CommitId("abc123".into()));
        repo.staged_status =
            file_status("staged.txt", worktree_core::domain::FileStatusKind::Added);
        repo.worktree_status =
            file_status("b.txt", worktree_core::domain::FileStatusKind::Modified);
        repo.history_state.selected_commit = Some(CommitId("f00d".into()));
        repo.history_state.multi_selection = crate::model::CommitMultiSelection {
            commits: vec![CommitId("f00d".into()), CommitId("abc123".into())],
            anchor: Some(CommitId("f00d".into())),
            anchor_index: Some(0),
            anchor_log_rev: Some(0),
        };
        repo.history_state.worktree_selection = Some(PathBuf::from("/tmp/other-wt"));
    }

    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectWorkingTreeSummary { repo_id },
    );
    assert!(
        effects.is_empty(),
        "the file list comes from status state; no backend work is needed"
    );

    let repo = &state.repos[0];
    assert_eq!(
        repo.history_state.selected_commit,
        Some(CommitId::uncommitted()),
        "the working tree row selects the not-committed-yet sentinel"
    );
    assert!(
        repo.history_state.multi_selection.commits.is_empty(),
        "the sentinel never enters multi_selection"
    );
    assert_eq!(
        repo.history_state.worktree_selection, None,
        "the main checkout's row displaces a linked-worktree selection"
    );
    let Loadable::Ready(details) = &repo.history_state.commit_details else {
        panic!("the details must be synthesized from status state");
    };
    assert_eq!(details.id, CommitId::uncommitted());
    assert_eq!(details.parent_ids, vec![CommitId("abc123".into())]);
    assert_eq!(
        details
            .files
            .iter()
            .map(|f| (f.path.clone(), f.kind))
            .collect::<Vec<_>>(),
        vec![
            (
                PathBuf::from("staged.txt"),
                worktree_core::domain::FileStatusKind::Added
            ),
            (
                PathBuf::from("b.txt"),
                worktree_core::domain::FileStatusKind::Modified
            ),
        ],
        "staged entries first, then unstaged"
    );

    // Re-selecting is a no-op: no rev churn for the fingerprint the details
    // pane hashes.
    let rev_before = repo.history_state.selected_commit_rev;
    let details_rev_before = repo.history_state.commit_details_rev;
    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectWorkingTreeSummary { repo_id },
    );
    assert_eq!(state.repos[0].history_state.selected_commit_rev, rev_before);
    assert_eq!(
        state.repos[0].history_state.commit_details_rev,
        details_rev_before
    );
}

#[test]
fn working_tree_details_resync_after_status_replies() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = working_tree_test_state();
    state.repos[0].worktree_status =
        file_status("b.txt", worktree_core::domain::FileStatusKind::Modified);

    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectWorkingTreeSummary { repo_id },
    );

    // A status reply with a different file list re-derives the details.
    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(vec![worktree_core::domain::FileStatus {
                path: PathBuf::from("c.txt"),
                kind: worktree_core::domain::FileStatusKind::Deleted,
                conflict: None,
            }]),
        }),
    );
    let Loadable::Ready(details) = &state.repos[0].history_state.commit_details else {
        panic!("details stay synthesized");
    };
    assert_eq!(
        details
            .files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>(),
        vec![PathBuf::from("c.txt")],
        "the review follows the status reply"
    );

    // An identical reply bumps nothing.
    let details_rev_before = state.repos[0].history_state.commit_details_rev;
    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::WorktreeStatusLoaded {
            repo_id,
            result: Ok(vec![worktree_core::domain::FileStatus {
                path: PathBuf::from("c.txt"),
                kind: worktree_core::domain::FileStatusKind::Deleted,
                conflict: None,
            }]),
        }),
    );
    assert_eq!(
        state.repos[0].history_state.commit_details_rev, details_rev_before,
        "an unchanged status must not churn the details fingerprint"
    );
}

#[test]
fn log_replacement_keeps_the_working_tree_selection() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = working_tree_test_state();

    let _ = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectWorkingTreeSummary { repo_id },
    );

    let seq = expect_log_reply(&mut state.repos[0], LogScope::FirstParent, None, None);
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id,
            seq,
            scope: LogScope::FirstParent,
            cursor: None,
            result: Ok(LogPage {
                commits: vec![Commit {
                    id: CommitId("abc123".into()),
                    parent_ids: Default::default(),
                    summary: "one".into(),
                    author: "a".into(),
                    time: std::time::SystemTime::UNIX_EPOCH,
                    signed: false,
                }],
                next_cursor: None,
            }),
        }),
    );
    assert!(
        effects.is_empty()
            || effects
                .iter()
                .all(|e| !matches!(e, Effect::LoadCommitDetails { .. }))
    );
    assert_eq!(
        state.repos[0].history_state.selected_commit,
        Some(CommitId::uncommitted()),
        "the sentinel is not a commit that vanished, so a replaced page keeps it"
    );
}

#[test]
fn select_commit_with_the_sentinel_routes_to_the_working_tree_row() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let (mut state, repo_id) = working_tree_test_state();

    // A nav-history replay restores the sentinel through SelectCommit; it must
    // land on the working-tree row, never on a backend details load.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::SelectCommit {
            repo_id,
            commit_id: CommitId::uncommitted(),
        },
    );
    assert!(
        effects.is_empty(),
        "no LoadCommitDetails may be requested for the sentinel: {effects:?}"
    );
    assert_eq!(
        state.repos[0].history_state.selected_commit,
        Some(CommitId::uncommitted())
    );
    assert!(
        state.repos[0]
            .history_state
            .multi_selection
            .commits
            .is_empty()
    );
}

/// The statistics window's commits: the dialog's open dispatches the load
/// (which parks the state in Loading), and the reply paints it — errors as an
/// error loadable, not a stale silent empty.
#[test]
fn repo_statistics_load_and_reply() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    state.repos[0].open = Loadable::Ready(());
    state.active_repo = Some(RepoId(1));

    // A repo still opening must not start a statistics walk.
    state.repos[0].open = Loadable::NotLoaded;
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadRepoStatistics { repo_id: RepoId(1) },
    );
    assert!(
        effects.is_empty(),
        "no statistics load before the repo is open"
    );

    state.repos[0].open = Loadable::Ready(());
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadRepoStatistics { repo_id: RepoId(1) },
    );
    assert!(matches!(
        &effects[..],
        [Effect::LoadRepoStatistics { repo_id }] if *repo_id == RepoId(1)
    ));
    assert!(matches!(state.repos[0].statistics, Loadable::Loading));

    let commit = |author: &str, seconds: i64| ContributorCommit {
        author: Arc::from(author),
        time: std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds as u64),
    };
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoStatisticsLoaded {
            repo_id: RepoId(1),
            result: Ok(vec![commit("Alice", 100), commit("Bob", 200)]),
        }),
    );
    assert!(effects.is_empty());
    let Loadable::Ready(statistics) = &state.repos[0].statistics else {
        panic!("expected the statistics window to be Ready");
    };
    assert_eq!(statistics.len(), 2);
    assert_eq!(&*statistics[0].author, "Alice");
    assert!(state.repos[0].statistics_rev > 0);

    // A failed walk surfaces as an error loadable, not a stale list.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::RepoStatisticsLoaded {
            repo_id: RepoId(1),
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Backend("boom".to_string()),
            )),
        }),
    );
    assert!(matches!(state.repos[0].statistics, Loadable::Error(_)));
}

#[test]
fn bisect_state_loaded_stores_snapshot_and_replays_re_requests() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        RepoId(1),
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));

    let snapshot = worktree_core::services::BisectState {
        original_branch: Some("main".to_string()),
        bad: Some(CommitId("bad000000000000000000000000000000000000".into())),
        good: vec![CommitId("good00000000000000000000000000000000000".into())],
        skipped: vec![CommitId("skip0000000000000000000000000000000000".into())],
        current: Some(CommitId("cur00000000000000000000000000000000000".into())),
    };
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::BisectStateLoaded {
            repo_id: RepoId(1),
            result: Ok(Some(snapshot.clone())),
        }),
    );
    assert!(effects.is_empty());
    assert_eq!(state.repos[0].bisect, Loadable::Ready(Some(snapshot)));

    // Not bisecting is a value, not an error.
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::BisectStateLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    assert_eq!(state.repos[0].bisect, Loadable::Ready(None));

    // A refresh that arrived while the load was in flight is replayed once
    // the reply lands.
    state.repos[0]
        .loads_in_flight
        .request(crate::model::RepoLoadsInFlight::BISECT_STATE);
    state.repos[0]
        .loads_in_flight
        .request(crate::model::RepoLoadsInFlight::BISECT_STATE);
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::BisectStateLoaded {
            repo_id: RepoId(1),
            result: Ok(None),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadBisectState { repo_id: RepoId(1) }]
    ));

    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(crate::msg::InternalMsg::BisectStateLoaded {
            repo_id: RepoId(1),
            result: Err(worktree_core::error::Error::new(
                worktree_core::error::ErrorKind::Backend("boom".to_string()),
            )),
        }),
    );
    assert!(matches!(state.repos[0].bisect, Loadable::Error(_)));
}

#[test]
fn worktree_external_change_with_paths_uses_the_targeted_lane() {
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let id_alloc = AtomicU64::new(1);
    let mut state = AppState::default();

    let repo_id = RepoId(1);
    repos.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    let mut repo = RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    );
    // The targeted lane only merges onto a settled snapshot.
    repo.set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus::default())));
    state.repos.push(repo);

    let paths: std::sync::Arc<[PathBuf]> = vec![PathBuf::from("src/lib.rs")].into();
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: Some(std::sync::Arc::clone(&paths)),
        },
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            Effect::LoadStatusForPaths { repo_id: candidate, paths: p }
                if *candidate == repo_id && **p == *paths
        )),
        "a small purely-worktree burst with a known path set rescans just those paths"
    );
    assert!(
        !has_worktree_status_effect(&effects, repo_id),
        "the coarse worktree rescan is not stacked on top"
    );

    // Without paths (or without a settled snapshot) the coarse lane answers.
    let mut repos2: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos2.insert(repo_id, Arc::new(DummyRepo::new("/tmp/repo")));
    let mut state2 = AppState::default();
    state2.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
    ));
    let effects = reduce(
        &mut repos2,
        &id_alloc,
        &mut state2,
        Msg::RepoExternallyChanged {
            repo_id,
            change: crate::msg::RepoExternalChange::Worktree,
            worktree_paths: Some(paths),
        },
    );
    assert!(
        has_worktree_status_effect(&effects, repo_id),
        "no settled snapshot to merge onto — the coarse worktree lane answers"
    );
}
