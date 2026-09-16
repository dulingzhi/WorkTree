use super::*;

static MERGETOOL_TRACE_TEST_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

fn write_deterministic_blob(path: &Path, total_bytes: usize) {
    use std::io::Write as _;

    let mut file = std::fs::File::create(path).expect("blob file should be creatable");
    let mut remaining = total_bytes;
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut buf = [0u8; 8192];

    while remaining > 0 {
        for byte in &mut buf {
            state ^= state << 7;
            state ^= state >> 9;
            state = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
            *byte = (state >> 24) as u8;
        }

        let chunk_len = remaining.min(buf.len());
        file.write_all(&buf[..chunk_len])
            .expect("blob chunk should be writable");
        remaining -= chunk_len;
    }
}

fn local_file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn schedule_effect_with_state_for_test(
    executor: &super::executor::TaskExecutor,
    session_persist_executor: &super::executor::TaskExecutor,
    backend: &Arc<dyn GitBackend>,
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    state: AppState,
    msg_tx: std::sync::mpsc::Sender<Msg>,
    effect: Effect,
) {
    let thread_state = Arc::new(std::sync::RwLock::new(Arc::new(state)));
    let msg_tx = super::worker_channel::StoreWorkerSender::for_test_msg_sender(msg_tx);
    let mut repo_task_tokens = FxHashMap::default();
    let repo_load_executor = super::executor::TaskExecutor::new(1);
    let metadata_executor = super::executor::TaskExecutor::new(1);
    super::effects::schedule_effect(
        super::effects::EffectExecutors {
            executor,
            repo_load_executor: &repo_load_executor,
            session_persist_executor,
            metadata_executor: &metadata_executor,
        },
        &thread_state,
        backend,
        repos,
        &mut repo_task_tokens,
        msg_tx,
        effect,
    );
}

fn schedule_effect_for_test(
    executor: &super::executor::TaskExecutor,
    session_persist_executor: &super::executor::TaskExecutor,
    backend: &Arc<dyn GitBackend>,
    repos: &FxHashMap<RepoId, Arc<dyn GitRepository>>,
    msg_tx: std::sync::mpsc::Sender<Msg>,
    effect: Effect,
) {
    schedule_effect_with_state_for_test(
        executor,
        session_persist_executor,
        backend,
        repos,
        AppState::default(),
        msg_tx,
        effect,
    );
}

#[test]
fn session_update_effects_persist_on_session_executor() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            panic!("session persistence effects should not open repositories")
        }
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let session_file = dir.path().join("session.json");
    let repo_a = dir.path().join("repo-a");
    let repo_b = dir.path().join("repo-b");
    let _session_file_override =
        crate::session::push_test_session_file_path_override(Some(session_file.clone()));

    let executor = super::executor::TaskExecutor::new(1);
    let session_executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &session_executor,
        &backend,
        &repos,
        msg_tx.clone(),
        Effect::PersistRecentRepo {
            repo_id: Some(RepoId(1)),
            workdir: repo_a.clone(),
            action: "test recent",
        },
    );
    schedule_effect_for_test(
        &executor,
        &session_executor,
        &backend,
        &repos,
        msg_tx.clone(),
        Effect::PersistRepoHistoryMode {
            repo_id: Some(RepoId(1)),
            workdir: repo_a.clone(),
            mode: LogScope::NoMerges,
            action: "test history mode",
        },
    );
    schedule_effect_for_test(
        &executor,
        &session_executor,
        &backend,
        &repos,
        msg_tx,
        Effect::PersistRepoHistoryModesBatch {
            repo_id: Some(RepoId(1)),
            updates: vec![(repo_b.clone(), LogScope::FirstParent)],
            action: "test history batch",
        },
    );

    let (completed_tx, completed_rx) = std::sync::mpsc::channel();
    session_executor.spawn(move || {
        completed_tx
            .send(())
            .expect("session persistence completion receiver should remain open");
    });
    completed_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("session persistence effects did not complete before timeout");

    let persistence_failures = msg_rx.try_iter().collect::<Vec<_>>();
    assert!(
        persistence_failures.is_empty(),
        "session persistence effects reported failures: {persistence_failures:?}"
    );

    let session = crate::session::load_from_path(&session_file);
    assert_eq!(session.recent_repos.first(), Some(&repo_a));
    assert_eq!(
        crate::session::load_repo_history_mode_from_path(&repo_a, &session_file),
        Some(LogScope::NoMerges)
    );
    assert_eq!(
        crate::session::load_repo_history_mode_from_path(&repo_b, &session_file),
        Some(LogScope::FirstParent)
    );
}

#[test]
fn unavailable_git_effect_emits_synthetic_repo_command_error() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    let state = AppState {
        git_runtime: worktree_core::process::GitRuntimeState {
            preference: worktree_core::process::GitExecutablePreference::Custom(PathBuf::new()),
            availability: worktree_core::process::GitExecutableAvailability::Unavailable {
                detail: "Custom Git executable is not configured. Choose an executable or switch back to System PATH.".to_string(),
            },
        },
        ..AppState::default()
    };

    schedule_effect_with_state_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        state,
        msg_tx,
        Effect::FetchAll {
            repo_id: RepoId(7),
            prune: true,
            auth: None,
        },
    );

    let msg = msg_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("expected synthetic unavailable-git message");
    match msg {
        Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
            repo_id,
            command,
            result,
        }) => {
            assert_eq!(repo_id, RepoId(7));
            assert_eq!(command, RepoCommandKind::FetchAll);
            let err = result.expect_err("expected unavailable-git failure");
            assert!(
                err.to_string()
                    .contains("Custom Git executable is not configured"),
                "unexpected error: {err}"
            );
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn safe_push_after_commit_effect_carries_auth_to_finished_message() {
    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo_id = RepoId(3);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(
        repo_id,
        Arc::new(UnsupportedRepo {
            spec: RepoSpec {
                workdir: PathBuf::from("/tmp/repo"),
            },
        }),
    );
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();
    let context = worktree_core::services::SafePushAfterCommitContext {
        amend: false,
        local_branch: Some("main".to_string()),
        pre_head: None,
        post_head: Some(CommitId("2222222222222222222222222222222222222222".into())),
    };
    let auth = worktree_core::auth::StagedGitAuth {
        kind: worktree_core::auth::GitAuthKind::UsernamePassword,
        username: Some("alice".to_string()),
        secret: "token".to_string(),
    };

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::SafePushAfterCommit {
            repo_id,
            context: context.clone(),
            auth: Some(auth.clone()),
        },
    );

    let msg = msg_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("expected safe-push completion message");
    match msg {
        Msg::Internal(crate::msg::InternalMsg::SafePushAfterCommitFinished {
            repo_id: emitted_repo_id,
            context: emitted_context,
            auth: emitted_auth,
            result,
        }) => {
            assert_eq!(emitted_repo_id, repo_id);
            assert_eq!(emitted_context, context);
            assert_eq!(emitted_auth, Some(auth));
            let err = result.expect_err("unsupported test repo should fail safe push");
            assert!(err.to_string().contains("safe push after commit"));
        }
        other => panic!("unexpected message: {other:?}"),
    }
}

#[test]
fn clone_repo_effect_clones_local_repo_and_emits_finished_and_open_repo() {
    if !super::require_git_shell_for_store_tests() {
        return;
    }
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let base = std::env::temp_dir().join(format!(
        "worktree-clone-effect-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let src = base.join("src");
    let dest = base.join("dest");
    let _ = std::fs::create_dir_all(&src);

    run_git(&src, &["init"]);
    run_git(&src, &["config", "user.email", "you@example.com"]);
    run_git(&src, &["config", "user.name", "You"]);
    run_git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\n").unwrap();
    run_git(&src, &["add", "a.txt"]);
    run_git(
        &src,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CloneRepo {
            url: src.display().to_string(),
            dest: dest.clone(),
            auth: None,
            ssh_key: None,
        },
    );

    let start = Instant::now();
    let mut saw_finished_ok = false;
    let mut saw_open_repo = false;
    while start.elapsed() < Duration::from_secs(15) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::Internal(crate::msg::InternalMsg::CloneRepoFinished {
                dest: finished_dest,
                result,
                ..
            }) if finished_dest == dest => {
                assert!(result.is_ok(), "clone failed: {result:?}");
                saw_finished_ok = true;
            }
            Msg::OpenRepo(path) if path == dest => {
                saw_open_repo = true;
            }
            _ => {}
        }

        if saw_finished_ok && saw_open_repo {
            break;
        }
    }

    assert!(saw_finished_ok, "did not observe CloneRepoFinished");
    assert!(saw_open_repo, "did not observe OpenRepo after clone");
    assert!(dest.join(".git").exists(), "expected .git at cloned dest");
}

#[test]
fn clone_repo_effect_abort_removes_partially_created_destination() {
    if !super::require_git_shell_for_store_tests() {
        return;
    }
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    const LARGE_BLOB_BYTES: usize = 64 * 1024 * 1024;

    let temp = tempfile::tempdir().expect("tempdir");
    let src = temp.path().join("src");
    let dest = temp.path().join("dest");
    std::fs::create_dir_all(&src).expect("source dir");

    run_git(&src, &["init"]);
    run_git(&src, &["config", "user.email", "you@example.com"]);
    run_git(&src, &["config", "user.name", "You"]);
    run_git(&src, &["config", "commit.gpgsign", "false"]);
    write_deterministic_blob(&src.join("payload.bin"), LARGE_BLOB_BYTES);
    run_git(&src, &["add", "payload.bin"]);
    run_git(
        &src,
        &["-c", "commit.gpgsign=false", "commit", "-m", "init"],
    );

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx.clone(),
        Effect::CloneRepo {
            url: local_file_url(&src),
            dest: dest.clone(),
            auth: None,
            ssh_key: None,
        },
    );

    let start = Instant::now();
    let mut abort_sent = false;
    let mut saw_finished_err = false;
    let mut saw_open_repo = false;

    while start.elapsed() < Duration::from_secs(30) {
        if !abort_sent && dest.exists() {
            schedule_effect_for_test(
                &executor,
                &executor,
                &backend,
                &repos,
                msg_tx.clone(),
                Effect::AbortCloneRepo { dest: dest.clone() },
            );
            abort_sent = true;
        }

        let msg = match msg_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::Internal(crate::msg::InternalMsg::CloneRepoFinished {
                dest: finished_dest,
                result,
                ..
            }) if finished_dest == dest => {
                assert!(abort_sent, "clone finished before abort could be sent");
                let err = result.expect_err("aborted clone should not succeed");
                let err_text = err.to_string();
                assert!(
                    err_text.contains("clone aborted"),
                    "unexpected abort error: {err_text}"
                );
                saw_finished_err = true;
                break;
            }
            Msg::OpenRepo(path) if path == dest => {
                saw_open_repo = true;
            }
            _ => {}
        }
    }

    assert!(abort_sent, "did not send abort request");
    assert!(saw_finished_err, "did not observe CloneRepoFinished error");
    assert!(
        !saw_open_repo,
        "aborted clone should not open the repository"
    );
    assert!(
        !dest.exists(),
        "aborted clone should clean up the destination directory"
    );
}

#[test]
fn load_conflict_file_effect_reads_worktree_and_emits_loaded() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        diff: worktree_core::domain::FileDiffText,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn diff_file_text(
            &self,
            _target: &DiffTarget,
        ) -> Result<Option<worktree_core::domain::FileDiffText>> {
            Ok(Some(self.diff.clone()))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let base = std::env::temp_dir().join(format!(
        "worktree-conflict-load-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("conflict.txt");
    let current = "a\n<<<<<<<\nours\n=======\ntheirs\n>>>>>>>\nb\n";
    std::fs::write(base.join(&rel), current.as_bytes()).unwrap();

    let repo_id = RepoId(1);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        diff: worktree_core::domain::FileDiffText::new(
            rel.clone(),
            Some("ours\n".to_string()),
            Some("theirs\n".to_string()),
        ),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadConflictFile {
            repo_id,
            path: rel.clone(),
            mode: crate::model::ConflictFileLoadMode::CurrentOnly,
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
                repo_id: rid,
                path,
                result,
                conflict_session,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert_eq!(path, rel);
            assert!(conflict_session.is_none());
            let file = result.unwrap().unwrap();
            assert_eq!(file.path, PathBuf::from("conflict.txt"));
            assert_eq!(file.base_bytes, None);
            assert_eq!(file.ours_bytes, None);
            assert_eq!(file.theirs_bytes, None);
            assert_eq!(file.current_bytes, None);
            assert_eq!(file.base, None);
            assert_eq!(file.ours, None);
            assert_eq!(file.theirs, None);
            assert_eq!(file.current.as_deref(), Some(current));
            return;
        };
    }
    panic!("timed out waiting for ConflictFileLoaded");
}

#[test]
fn load_conflict_file_effect_reuses_conflict_session_payloads_without_stage_fetch() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;
    use worktree_core::services::ConflictFileStages;

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        session: ConflictSession,
        stage_calls: Arc<AtomicUsize>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn conflict_file_stages(&self, _path: &Path) -> Result<Option<ConflictFileStages>> {
            self.stage_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        fn conflict_session(&self, _path: &Path) -> Result<Option<ConflictSession>> {
            Ok(Some(self.session.clone()))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let base = std::env::temp_dir().join(format!(
        "worktree-conflict-load-session-reuse-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("session_reuse.txt");
    let base_text = "base\n";
    let ours_text = "ours\n";
    let theirs_text = "theirs\n";
    let current_text = "<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\n";
    let stage_calls = Arc::new(AtomicUsize::new(0));
    let repo_id = RepoId(8);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        session: ConflictSession::from_merged_text(
            rel.clone(),
            FileConflictKind::BothModified,
            ConflictPayload::Text(base_text.to_string().into()),
            ConflictPayload::Text(ours_text.to_string().into()),
            ConflictPayload::Text(theirs_text.to_string().into()),
            current_text,
        ),
        stage_calls: stage_calls.clone(),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadConflictFile {
            repo_id,
            path: rel.clone(),
            mode: crate::model::ConflictFileLoadMode::Full,
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
                repo_id: rid,
                path,
                result,
                conflict_session,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert_eq!(path, rel);
            let session = conflict_session.expect("session should be forwarded from backend");
            let file = result.unwrap().unwrap();
            assert_eq!(file.path, rel);
            assert_eq!(file.base.as_deref(), Some(base_text));
            assert_eq!(file.ours.as_deref(), Some(ours_text));
            assert_eq!(file.theirs.as_deref(), Some(theirs_text));
            assert_eq!(file.current.as_deref(), Some(current_text));
            assert_eq!(file.base_bytes, None);
            assert_eq!(file.ours_bytes, None);
            assert_eq!(file.theirs_bytes, None);
            assert_eq!(file.current_bytes, None);
            assert_eq!(stage_calls.load(Ordering::SeqCst), 0);
            assert_eq!(session.current_text(), Some(current_text));
            assert!(
                matches!(&session.base, ConflictPayload::Text(text) if std::sync::Arc::ptr_eq(file.base.as_ref().expect("base text"), text))
            );
            assert!(
                matches!(&session.ours, ConflictPayload::Text(text) if std::sync::Arc::ptr_eq(file.ours.as_ref().expect("ours text"), text))
            );
            assert!(
                matches!(&session.theirs, ConflictPayload::Text(text) if std::sync::Arc::ptr_eq(file.theirs.as_ref().expect("theirs text"), text))
            );
            assert!(
                matches!(
                    session.current.as_ref(),
                    Some(ConflictPayload::Text(text))
                        if std::sync::Arc::ptr_eq(file.current.as_ref().expect("current text"), text)
                ),
                "current text should be forwarded from the session without rereading the worktree"
            );
            return;
        }
    }

    panic!("timed out waiting for ConflictFileLoaded");
}

#[test]
fn load_conflict_file_effect_preserves_binary_payloads_when_reusing_session() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;
    use worktree_core::services::ConflictFileStages;

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        session: ConflictSession,
        stage_calls: Arc<AtomicUsize>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn conflict_file_stages(&self, _path: &Path) -> Result<Option<ConflictFileStages>> {
            self.stage_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        fn conflict_session(&self, _path: &Path) -> Result<Option<ConflictSession>> {
            Ok(Some(self.session.clone()))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let base = std::env::temp_dir().join(format!(
        "worktree-conflict-load-session-reuse-binary-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("session_reuse.bin");
    let base_bytes = vec![0xff, 0x00, 0x01];
    let ours_bytes = vec![0xfe, 0x10, 0x11];
    let theirs_bytes = vec![0xfd, 0x20, 0x21];
    let current_bytes = vec![0xfc, 0x30, 0x31];
    let base_payload: Arc<[u8]> = base_bytes.clone().into();
    let ours_payload: Arc<[u8]> = ours_bytes.clone().into();
    let theirs_payload: Arc<[u8]> = theirs_bytes.clone().into();
    let stage_calls = Arc::new(AtomicUsize::new(0));
    let repo_id = RepoId(9);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        session: ConflictSession::new_with_current(
            rel.clone(),
            FileConflictKind::BothModified,
            ConflictPayload::Binary(base_payload.clone()),
            ConflictPayload::Binary(ours_payload.clone()),
            ConflictPayload::Binary(theirs_payload.clone()),
            ConflictPayload::Binary(current_bytes.clone().into()),
        ),
        stage_calls: stage_calls.clone(),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadConflictFile {
            repo_id,
            path: rel.clone(),
            mode: crate::model::ConflictFileLoadMode::Full,
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
                repo_id: rid,
                path,
                result,
                conflict_session,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert_eq!(path, rel);
            let session = conflict_session.expect("session should be forwarded from backend");
            let file = result.unwrap().unwrap();
            assert_eq!(file.path, rel);
            assert_eq!(file.base_bytes.as_deref(), Some(base_bytes.as_slice()));
            assert_eq!(file.ours_bytes.as_deref(), Some(ours_bytes.as_slice()));
            assert_eq!(file.theirs_bytes.as_deref(), Some(theirs_bytes.as_slice()));
            assert_eq!(
                file.current_bytes.as_deref(),
                Some(current_bytes.as_slice())
            );
            assert_eq!(file.base, None);
            assert_eq!(file.ours, None);
            assert_eq!(file.theirs, None);
            assert_eq!(file.current, None);
            assert!(
                Arc::ptr_eq(file.base_bytes.as_ref().expect("base bytes"), &base_payload,),
                "base binary bytes should be forwarded from the session without cloning",
            );
            assert!(
                Arc::ptr_eq(file.ours_bytes.as_ref().expect("ours bytes"), &ours_payload,),
                "ours binary bytes should be forwarded from the session without cloning",
            );
            assert!(
                Arc::ptr_eq(
                    file.theirs_bytes.as_ref().expect("theirs bytes"),
                    &theirs_payload,
                ),
                "theirs binary bytes should be forwarded from the session without cloning",
            );
            assert!(
                matches!(
                    session.current.as_ref(),
                    Some(ConflictPayload::Binary(bytes))
                        if Arc::ptr_eq(file.current_bytes.as_ref().expect("current bytes"), bytes)
                ),
                "current binary bytes should be forwarded from the session without rereading the worktree",
            );
            assert_eq!(stage_calls.load(Ordering::SeqCst), 0);
            return;
        }
    }

    panic!("timed out waiting for ConflictFileLoaded");
}

#[test]
fn load_conflict_file_effect_reuses_absent_current_payload_without_rereading_worktree() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;
    use worktree_core::mergetool_trace::{self, MergetoolTraceStage};
    use worktree_core::services::ConflictFileStages;

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        session: ConflictSession,
        stage_calls: Arc<AtomicUsize>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn conflict_file_stages(&self, _path: &Path) -> Result<Option<ConflictFileStages>> {
            self.stage_calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        fn conflict_session(&self, _path: &Path) -> Result<Option<ConflictSession>> {
            Ok(Some(self.session.clone()))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let _trace_lock = MERGETOOL_TRACE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _trace = mergetool_trace::capture();

    let base = std::env::temp_dir().join(format!(
        "worktree-conflict-load-session-reuse-absent-current-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("removed.txt");
    let base_text = "base\n";
    let stage_calls = Arc::new(AtomicUsize::new(0));
    let repo_id = RepoId(10);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        session: ConflictSession::new_with_current(
            rel.clone(),
            FileConflictKind::BothDeleted,
            ConflictPayload::Text(base_text.into()),
            ConflictPayload::Absent,
            ConflictPayload::Absent,
            ConflictPayload::Absent,
        ),
        stage_calls: stage_calls.clone(),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadConflictFile {
            repo_id,
            path: rel.clone(),
            mode: crate::model::ConflictFileLoadMode::Full,
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
                repo_id: rid,
                path,
                result,
                conflict_session,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert_eq!(path, rel);
            let session = conflict_session.expect("session should be forwarded from backend");
            let file = result.unwrap().unwrap();
            assert_eq!(file.path, rel);
            assert_eq!(file.base.as_deref(), Some(base_text));
            assert_eq!(file.ours, None);
            assert_eq!(file.theirs, None);
            assert_eq!(file.current, None);
            assert_eq!(file.base_bytes, None);
            assert_eq!(file.ours_bytes, None);
            assert_eq!(file.theirs_bytes, None);
            assert_eq!(file.current_bytes, None);
            assert_eq!(stage_calls.load(Ordering::SeqCst), 0);
            assert!(matches!(
                session.current.as_ref(),
                Some(ConflictPayload::Absent)
            ));

            let trace = mergetool_trace::snapshot();
            let path_events: Vec<_> = trace
                .events
                .iter()
                .filter(|event| event.path.as_deref() == Some(rel.as_path()))
                .collect();
            assert!(
                path_events
                    .iter()
                    .any(|event| event.stage == MergetoolTraceStage::LoadCurrentReuse),
                "known-absent current payload should reuse the session value instead of rereading the worktree",
            );
            assert!(
                !path_events
                    .iter()
                    .any(|event| event.stage == MergetoolTraceStage::LoadCurrentRead),
                "known-absent current payload should not fall back to a worktree read",
            );
            return;
        }
    }

    panic!("timed out waiting for ConflictFileLoaded");
}

#[test]
fn load_conflict_file_effect_records_trace_stages_and_sizes() {
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;
    use worktree_core::mergetool_trace::{self, MergetoolTraceStage};
    use worktree_core::services::ConflictFileStages;

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        stages: ConflictFileStages,
        session: ConflictSession,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn conflict_file_stages(&self, _path: &Path) -> Result<Option<ConflictFileStages>> {
            Ok(Some(self.stages.clone()))
        }

        fn conflict_session(&self, _path: &Path) -> Result<Option<ConflictSession>> {
            Ok(Some(self.session.clone()))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    fn trace_line_count(text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.as_bytes()
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count()
                + 1
        }
    }

    let _trace_lock = MERGETOOL_TRACE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _trace = mergetool_trace::capture();
    let base = std::env::temp_dir().join(format!(
        "worktree-conflict-load-trace-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("trace_conflict.html");
    let base_text = "<div>base</div>\n<section>common</section>\n<footer>end</footer>\n";
    let ours_text = "<div>ours</div>\n<section>common</section>\n<footer>end</footer>\n";
    let theirs_text = "<div>theirs</div>\n<section>common</section>\n<footer>end</footer>\n";
    let current_text = [
        "<<<<<<< ours",
        "<div>ours</div>",
        "=======",
        "<div>theirs</div>",
        ">>>>>>> theirs",
        "<section>common</section>",
        "<footer>end</footer>",
        "",
    ]
    .join("\n");
    let repo_id = RepoId(7);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        stages: ConflictFileStages {
            path: rel.clone(),
            base_bytes: Some(base_text.as_bytes().to_vec().into()),
            ours_bytes: Some(ours_text.as_bytes().to_vec().into()),
            theirs_bytes: Some(theirs_text.as_bytes().to_vec().into()),
            base: Some(base_text.to_string().into()),
            ours: Some(ours_text.to_string().into()),
            theirs: Some(theirs_text.to_string().into()),
        },
        session: ConflictSession::from_merged_text(
            rel.clone(),
            FileConflictKind::BothModified,
            ConflictPayload::Text(base_text.to_string().into()),
            ConflictPayload::Text(ours_text.to_string().into()),
            ConflictPayload::Text(theirs_text.to_string().into()),
            &current_text,
        ),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadConflictFile {
            repo_id,
            path: rel.clone(),
            mode: crate::model::ConflictFileLoadMode::Full,
        },
    );

    let loaded_file = {
        let start = Instant::now();
        loop {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out waiting for ConflictFileLoaded"
            );
            match msg_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Msg::Internal(crate::msg::InternalMsg::ConflictFileLoaded {
                    repo_id: rid,
                    path,
                    result,
                    conflict_session,
                })) if rid == repo_id && path == rel => {
                    let session = conflict_session.expect("trace test should receive a session");
                    assert_eq!(session.regions.len(), 1);
                    break result.unwrap().unwrap();
                }
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(err) => panic!("channel closed while waiting for conflict load: {err:?}"),
            }
        }
    };

    assert_eq!(loaded_file.path, rel);
    assert_eq!(loaded_file.base.as_deref(), Some(base_text));
    assert_eq!(loaded_file.ours.as_deref(), Some(ours_text));
    assert_eq!(loaded_file.theirs.as_deref(), Some(theirs_text));
    assert_eq!(loaded_file.current.as_deref(), Some(current_text.as_str()));

    let trace = mergetool_trace::snapshot();
    let path_events: Vec<_> = trace
        .events
        .iter()
        .filter(|event| event.path.as_deref() == Some(rel.as_path()))
        .collect();
    assert_eq!(
        path_events.len(),
        3,
        "expected exactly the three load-stage trace events for the synthetic conflict path"
    );

    let session_event = path_events
        .iter()
        .find(|event| event.stage == MergetoolTraceStage::LoadConflictSession)
        .copied()
        .expect("missing conflict-session trace event");
    assert_eq!(session_event.base.bytes, Some(base_text.len()));
    assert_eq!(session_event.ours.lines, Some(trace_line_count(ours_text)));
    assert_eq!(
        session_event.conflict_block_count,
        Some(1),
        "session trace should report the parsed conflict block count"
    );

    let stages_event = path_events
        .iter()
        .find(|event| event.stage == MergetoolTraceStage::LoadConflictFileStages)
        .copied()
        .expect("missing conflict-file-stages trace event");
    assert_eq!(stages_event.base.lines, Some(trace_line_count(base_text)));
    assert_eq!(stages_event.ours.bytes, Some(ours_text.len()));
    assert_eq!(stages_event.theirs.bytes, Some(theirs_text.len()));

    let current_event = path_events
        .iter()
        .find(|event| event.stage == MergetoolTraceStage::LoadCurrentReuse)
        .copied()
        .expect("missing current-reuse trace event");
    assert_eq!(current_event.current.bytes, Some(current_text.len()));
    assert_eq!(
        current_event.current.lines,
        Some(trace_line_count(&current_text))
    );
}

#[test]
fn save_worktree_file_effect_writes_and_can_stage() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        staged: std::sync::Mutex<Vec<PathBuf>>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, paths: &[&Path]) -> Result<()> {
            let mut staged = self.staged.lock().unwrap();
            for p in paths {
                staged.push(p.to_path_buf());
            }
            Ok(())
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let base = std::env::temp_dir().join(format!(
        "worktree-save-worktree-file-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let rel = PathBuf::from("dir/out.txt");
    let contents = "hello\nworld\n";

    let repo_id = RepoId(1);
    let repo: Arc<Repo> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        staged: std::sync::Mutex::new(Vec::new()),
    });
    let repo_trait: Arc<dyn GitRepository> = repo.clone();
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo_trait);
        repos
    };

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx.clone(),
        Effect::SaveWorktreeFile {
            repo_id,
            path: rel.clone(),
            contents: contents.to_string(),
            stage: true,
        },
    );

    let mut saw_write_and_stage = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
                repo_id: rid,
                command,
                result,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert!(matches!(
                command,
                crate::msg::RepoCommandKind::SaveWorktreeFile { .. }
            ));
            assert!(result.is_ok());
            let on_disk = std::fs::read_to_string(base.join(&rel)).unwrap();
            assert_eq!(on_disk, contents);
            let staged = repo.staged.lock().unwrap().clone();
            assert_eq!(staged, vec![rel.clone()]);
            saw_write_and_stage = true;
            break;
        };
    }
    assert!(
        saw_write_and_stage,
        "timed out waiting for RepoCommandFinished"
    );

    let escaped_name = format!(
        "worktree-save-worktree-file-escape-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let escaped_path = PathBuf::from("..").join(&escaped_name);
    let escaped_dest = base
        .parent()
        .expect("temp dir should have a parent")
        .join(&escaped_name);
    let _ = std::fs::remove_file(&escaped_dest);

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::SaveWorktreeFile {
            repo_id,
            path: escaped_path,
            contents: "escape".to_string(),
            stage: false,
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
                repo_id: rid,
                command,
                result,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert!(matches!(
                command,
                crate::msg::RepoCommandKind::SaveWorktreeFile { .. }
            ));
            let err = result.expect_err("expected traversal write to fail");
            match err.kind() {
                ErrorKind::Backend(message) => {
                    assert!(
                        message.contains("outside repository workdir"),
                        "unexpected error message: {message}"
                    );
                }
                other => panic!("unexpected error kind: {other:?}"),
            }
            assert!(
                !escaped_dest.exists(),
                "unexpected file written outside workdir: {}",
                escaped_dest.display()
            );
            return;
        };
    }
    panic!("timed out waiting for RepoCommandFinished");
}

#[test]
fn append_gitignore_patterns_effect_creates_appends_and_dedupes() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    // `tempfile` rather than a hand-rolled directory: its `Drop` runs on unwind,
    // so a failing assertion below does not leave a stray repo in /tmp.
    let dir = tempfile::tempdir().expect("create tempdir");
    let base = dir.path().to_path_buf();
    let gitignore = base.join(".gitignore");

    let repo_id = RepoId(1);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let repo: Arc<dyn GitRepository> = Arc::new(Repo {
            spec: RepoSpec {
                workdir: base.clone(),
            },
        });
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);

    // Runs one append to completion and hands back the command's stdout, which
    // is how the worker reports the "nothing to add" short-circuit.
    let append = |patterns: Vec<String>| -> String {
        let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();
        schedule_effect_for_test(
            &executor,
            &executor,
            &backend,
            &repos,
            msg_tx,
            Effect::AppendGitignorePatterns { repo_id, patterns },
        );
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
                && let Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
                    command,
                    result,
                    ..
                }) = msg
            {
                assert!(matches!(
                    command,
                    crate::msg::RepoCommandKind::AppendGitignorePatterns { .. }
                ));
                return result.expect("append should succeed").stdout;
            }
        }
        panic!("timed out waiting for RepoCommandFinished");
    };

    append(vec!["/a.log".to_string()]);
    assert_eq!(
        std::fs::read_to_string(&gitignore).unwrap(),
        "/a.log\n",
        "the file is created when absent"
    );

    append(vec!["/a.log".to_string(), "/b.log".to_string()]);
    assert_eq!(
        std::fs::read_to_string(&gitignore).unwrap(),
        "/a.log\n/b.log\n",
        "the already-present pattern is skipped, the new one appended"
    );

    let before = std::fs::read_to_string(&gitignore).unwrap();
    let stdout = append(vec!["/a.log".to_string()]);
    assert_eq!(
        std::fs::read_to_string(&gitignore).unwrap(),
        before,
        "re-running must not duplicate the line"
    );
    assert_eq!(
        stdout.trim(),
        worktree_core::gitignore::NOTHING_TO_ADD,
        "a fully redundant append must short-circuit before the write so it does \
         not bump the mtime and rebuild the filesystem watcher for nothing — and \
         it must say so with the marker `summarize_command` keys off, or the user \
         is told a write happened"
    );

    std::fs::write(&gitignore, "/target").unwrap();
    append(vec!["/c.log".to_string()]);
    assert_eq!(
        std::fs::read_to_string(&gitignore).unwrap(),
        "/target\n/c.log\n",
        "an unterminated last line must not fuse with the new pattern"
    );

    std::fs::write(&gitignore, "/target\r\n").unwrap();
    append(vec!["/d.log".to_string()]);
    assert_eq!(
        std::fs::read_to_string(&gitignore).unwrap(),
        "/target\r\n/d.log\r\n",
        "a CRLF file stays CRLF"
    );
}

#[test]
fn checkout_conflict_base_effect_calls_repo_and_emits_finished() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        checkout_base_calls: std::sync::Mutex<Vec<PathBuf>>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn checkout_conflict_base(&self, path: &Path) -> Result<CommandOutput> {
            self.checkout_base_calls
                .lock()
                .unwrap()
                .push(path.to_path_buf());
            Ok(CommandOutput::empty_success(format!(
                "git checkout :1:{}",
                path.display()
            )))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let repo_id = RepoId(1);
    let rel = PathBuf::from("conflicted.txt");
    let repo: Arc<Repo> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: std::env::temp_dir(),
        },
        checkout_base_calls: std::sync::Mutex::new(Vec::new()),
    });
    let repo_trait: Arc<dyn GitRepository> = repo.clone();
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo_trait);
        repos
    };

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CheckoutConflictBase {
            repo_id,
            path: rel.clone(),
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
                repo_id: rid,
                command,
                result,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert!(matches!(
                command,
                crate::msg::RepoCommandKind::CheckoutConflictBase { path } if path == rel
            ));
            assert!(result.is_ok());
            assert_eq!(repo.checkout_base_calls.lock().unwrap().as_slice(), [rel]);
            return;
        };
    }
    panic!("timed out waiting for RepoCommandFinished");
}

#[test]
fn accept_conflict_deletion_effect_calls_repo_and_emits_finished() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        accepted_deletion_calls: std::sync::Mutex<Vec<PathBuf>>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }

        fn accept_conflict_deletion(&self, path: &Path) -> Result<CommandOutput> {
            self.accepted_deletion_calls
                .lock()
                .unwrap()
                .push(path.to_path_buf());
            Ok(CommandOutput::empty_success(format!(
                "git rm -- {}",
                path.display()
            )))
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let repo_id = RepoId(1);
    let rel = PathBuf::from("conflicted.txt");
    let repo: Arc<Repo> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: std::env::temp_dir(),
        },
        accepted_deletion_calls: std::sync::Mutex::new(Vec::new()),
    });
    let repo_trait: Arc<dyn GitRepository> = repo.clone();
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo_trait);
        repos
    };

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::AcceptConflictDeletion {
            repo_id,
            path: rel.clone(),
        },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if let Ok(msg) = msg_rx.recv_timeout(Duration::from_millis(50))
            && let Msg::Internal(crate::msg::InternalMsg::RepoCommandFinished {
                repo_id: rid,
                command,
                result,
            }) = msg
        {
            assert_eq!(rid, repo_id);
            assert!(matches!(
                command,
                crate::msg::RepoCommandKind::AcceptConflictDeletion { path } if path == rel
            ));
            assert!(result.is_ok());
            assert_eq!(
                repo.accepted_deletion_calls.lock().unwrap().as_slice(),
                [rel]
            );
            return;
        };
    }
    panic!("timed out waiting for RepoCommandFinished");
}

#[test]
fn load_stashes_effect_truncates_results_to_limit() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    struct Repo {
        spec: RepoSpec,
        stashes: Vec<StashEntry>,
    }

    impl GitRepository for Repo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for Repo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for Repo {}
    impl GitRepositoryRemotes for Repo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for Repo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for Repo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for Repo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            Ok(self.stashes.clone())
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for Repo {}

    let base = std::env::temp_dir().join(format!(
        "worktree-stash-load-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&base);

    let stashes = (0..5)
        .map(|i| StashEntry {
            index: i,
            id: CommitId(format!("stash-{i}").into()),
            message: format!("stash message {i}").into(),
            created_at: None,
        })
        .collect::<Vec<_>>();

    let repo_id = RepoId(1);
    let repo: Arc<dyn GitRepository> = Arc::new(Repo {
        spec: RepoSpec {
            workdir: base.clone(),
        },
        stashes,
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadStashes { repo_id, limit: 2 },
    );

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(m) => m,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::Internal(crate::msg::InternalMsg::StashesLoaded {
                repo_id: got_repo_id,
                result,
            }) if got_repo_id == repo_id => {
                let entries = result.expect("expected stash list Ok");
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].index, 0);
                assert_eq!(entries[1].index, 1);
                return;
            }
            _ => {}
        }
    }

    panic!("did not observe StashesLoaded");
}

#[test]
fn stash_effect_requests_stash_reload_on_success() {
    use std::sync::Mutex;

    struct RecordingRepo {
        spec: RepoSpec,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl GitRepository for RecordingRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for RecordingRepo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for RecordingRepo {}
    impl GitRepositoryRemotes for RecordingRepo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for RecordingRepo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for RecordingRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for RecordingRepo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            message: &str,
            include_untracked: bool,
            keep_index: bool,
            paths: &[PathBuf],
        ) -> Result<()> {
            self.calls.lock().unwrap().push(format!(
                "stash {message} include_untracked={include_untracked} keep_index={keep_index} paths={}",
                paths.len()
            ));
            Ok(())
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for RecordingRepo {}

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _workdir: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let repo: Arc<RecordingRepo> = Arc::new(RecordingRepo {
        spec: RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
        calls: Arc::clone(&calls),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(RepoId(1), repo);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::Stash {
            repo_id: RepoId(1),
            message: "wip".to_string(),
            include_untracked: true,
            keep_index: false,
            paths: Vec::new().into(),
        },
    );

    let start = Instant::now();
    let mut saw_load_stashes = false;
    let mut saw_finished = false;
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::LoadStashes { repo_id: RepoId(1) } => saw_load_stashes = true,
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: RepoId(1),
                action: RepoActionKind::Stash,
                result: Ok(()),
                paths: None,
            }) => saw_finished = true,
            _ => {}
        }

        if saw_load_stashes && saw_finished {
            break;
        }
    }

    assert!(
        saw_load_stashes,
        "expected stash effect to request stash reload"
    );
    assert!(saw_finished, "expected stash effect to complete");
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["stash wip include_untracked=true keep_index=false paths=0".to_string()]
    );
}

#[test]
fn pop_stash_effect_applies_and_drops_then_requests_stash_reload() {
    use std::sync::Mutex;

    struct RecordingRepo {
        spec: RepoSpec,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl GitRepository for RecordingRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for RecordingRepo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for RecordingRepo {}
    impl GitRepositoryRemotes for RecordingRepo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for RecordingRepo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for RecordingRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for RecordingRepo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("apply {index}"));
            Ok(())
        }

        fn stash_drop(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("drop {index}"));
            Ok(())
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for RecordingRepo {}

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _workdir: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let repo: Arc<RecordingRepo> = Arc::new(RecordingRepo {
        spec: RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
        calls: Arc::clone(&calls),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(RepoId(1), repo);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::PopStash {
            repo_id: RepoId(1),
            index: 3,
        },
    );

    let start = Instant::now();
    let mut saw_load_stashes = false;
    let mut saw_finished = false;
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::LoadStashes { repo_id: RepoId(1) } => saw_load_stashes = true,
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: RepoId(1),
                action: RepoActionKind::PopStash,
                result: Ok(()),
                paths: None,
            }) => saw_finished = true,
            _ => {}
        }

        if saw_load_stashes && saw_finished {
            break;
        }
    }

    assert!(
        saw_load_stashes,
        "expected pop stash effect to request stash reload"
    );
    assert!(saw_finished, "expected pop stash effect to complete");
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["apply 3".to_string(), "drop 3".to_string()]
    );
}

#[test]
fn pop_stash_effect_propagates_apply_error_without_drop_or_reload() {
    use std::sync::Mutex;

    struct FailingApplyRepo {
        spec: RepoSpec,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl GitRepository for FailingApplyRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for FailingApplyRepo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for FailingApplyRepo {}
    impl GitRepositoryRemotes for FailingApplyRepo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for FailingApplyRepo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for FailingApplyRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for FailingApplyRepo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("apply {index}"));
            Err(Error::new(ErrorKind::Backend("apply failed".to_string())))
        }

        fn stash_drop(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("drop {index}"));
            Ok(())
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for FailingApplyRepo {}

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _workdir: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let repo: Arc<FailingApplyRepo> = Arc::new(FailingApplyRepo {
        spec: RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
        calls: Arc::clone(&calls),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(RepoId(1), repo);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::PopStash {
            repo_id: RepoId(1),
            index: 7,
        },
    );

    let start = Instant::now();
    let mut saw_load_stashes = false;
    let mut saw_finished_err = false;
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::LoadStashes { repo_id: RepoId(1) } => saw_load_stashes = true,
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: RepoId(1),
                action: RepoActionKind::PopStash,
                result: Err(_),
                paths: None,
            }) => {
                saw_finished_err = true;
                break;
            }
            _ => {}
        }
    }

    assert!(
        !saw_load_stashes,
        "pop stash apply failure should not request stash reload"
    );
    assert!(
        saw_finished_err,
        "expected pop stash effect to emit apply error completion"
    );
    assert_eq!(*calls.lock().unwrap(), vec!["apply 7".to_string()]);
}

#[test]
fn drop_stash_effect_requests_stash_reload_on_success() {
    use std::sync::Mutex;

    struct RecordingRepo {
        spec: RepoSpec,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl GitRepository for RecordingRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for RecordingRepo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for RecordingRepo {}
    impl GitRepositoryRemotes for RecordingRepo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for RecordingRepo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for RecordingRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for RecordingRepo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("drop {index}"));
            Ok(())
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for RecordingRepo {}

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _workdir: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let repo: Arc<RecordingRepo> = Arc::new(RecordingRepo {
        spec: RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
        calls: Arc::clone(&calls),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(RepoId(1), repo);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::DropStash {
            repo_id: RepoId(1),
            index: 3,
        },
    );

    let start = Instant::now();
    let mut saw_load_stashes = false;
    let mut saw_finished = false;
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::LoadStashes { repo_id: RepoId(1) } => saw_load_stashes = true,
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: RepoId(1),
                action: RepoActionKind::DropStash,
                result: Ok(()),
                paths: None,
            }) => saw_finished = true,
            _ => {}
        }

        if saw_load_stashes && saw_finished {
            break;
        }
    }

    assert!(
        saw_load_stashes,
        "expected drop stash effect to request stash reload"
    );
    assert!(saw_finished, "expected drop stash effect to complete");
    assert_eq!(*calls.lock().unwrap(), vec!["drop 3".to_string()]);
}

#[test]
fn drop_stash_effect_requests_stash_reload_on_error() {
    use std::sync::Mutex;

    struct FailingRepo {
        spec: RepoSpec,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl GitRepository for FailingRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }
    }
    impl GitRepositoryLog for FailingRepo {
        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unimplemented!()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            unimplemented!()
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
            unimplemented!()
        }
    }
    impl GitRepositoryHistory for FailingRepo {}
    impl GitRepositoryRemotes for FailingRepo {
        fn list_remotes(&self) -> Result<Vec<Remote>> {
            unimplemented!()
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            unimplemented!()
        }

        fn fetch_all(&self) -> Result<()> {
            unimplemented!()
        }

        fn pull(&self, _mode: PullMode) -> Result<()> {
            unimplemented!()
        }

        fn push(&self) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryStatus for FailingRepo {
        fn status(&self) -> Result<RepoStatus> {
            unimplemented!()
        }
    }
    impl GitRepositoryDiff for FailingRepo {
        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unimplemented!()
        }
    }
    impl GitRepositoryPorcelain for FailingRepo {
        fn current_branch(&self) -> Result<String> {
            unimplemented!()
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            unimplemented!()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unimplemented!()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unimplemented!()
        }

        fn stash_create(
            &self,
            _message: &str,
            _include_untracked: bool,
            _keep_index: bool,
            _paths: &[PathBuf],
        ) -> Result<()> {
            unimplemented!()
        }

        fn stash_list(&self) -> Result<Vec<StashEntry>> {
            unimplemented!()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unimplemented!()
        }

        fn stash_drop(&self, index: usize) -> Result<()> {
            self.calls.lock().unwrap().push(format!("drop {index}"));
            Err(Error::new(ErrorKind::Backend("drop failed".to_string())))
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unimplemented!()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unimplemented!()
        }
    }
    impl GitRepositoryWorktree for FailingRepo {}

    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _workdir: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Unsupported("test backend")))
        }
    }

    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let repo: Arc<FailingRepo> = Arc::new(FailingRepo {
        spec: RepoSpec {
            workdir: PathBuf::from("/tmp/repo"),
        },
        calls: Arc::clone(&calls),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let mut repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    repos.insert(RepoId(1), repo);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::DropStash {
            repo_id: RepoId(1),
            index: 4,
        },
    );

    let start = Instant::now();
    let mut saw_load_stashes = false;
    let mut saw_finished_err = false;
    while start.elapsed() < Duration::from_secs(5) {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("channel closed: {e:?}"),
        };

        match msg {
            Msg::LoadStashes { repo_id: RepoId(1) } => saw_load_stashes = true,
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: RepoId(1),
                action: RepoActionKind::DropStash,
                result: Err(_),
                paths: None,
            }) => {
                saw_finished_err = true;
                break;
            }
            _ => {}
        }
    }

    assert!(
        saw_load_stashes,
        "drop stash failure should still request stash reload"
    );
    assert!(
        saw_finished_err,
        "expected drop stash effect to emit error completion"
    );
    assert_eq!(*calls.lock().unwrap(), vec!["drop 4".to_string()]);
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn unsupported_repo_result<T>() -> Result<T> {
    Err(Error::new(ErrorKind::Unsupported(
        "unsupported repo for effect scheduling coverage",
    )))
}

struct UnsupportedRepo {
    spec: RepoSpec,
}

impl GitRepository for UnsupportedRepo {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }
}
impl GitRepositoryLog for UnsupportedRepo {
    fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        unsupported_repo_result()
    }

    fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
        unsupported_repo_result()
    }

    fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
        unsupported_repo_result()
    }
}
impl GitRepositoryHistory for UnsupportedRepo {}
impl GitRepositoryRemotes for UnsupportedRepo {
    fn list_remotes(&self) -> Result<Vec<Remote>> {
        unsupported_repo_result()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        unsupported_repo_result()
    }

    fn fetch_all(&self) -> Result<()> {
        unsupported_repo_result()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        unsupported_repo_result()
    }

    fn push(&self) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryStatus for UnsupportedRepo {
    fn status(&self) -> Result<RepoStatus> {
        unsupported_repo_result()
    }
}
impl GitRepositoryDiff for UnsupportedRepo {
    fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
        unsupported_repo_result()
    }
}
impl GitRepositoryPorcelain for UnsupportedRepo {
    fn current_branch(&self) -> Result<String> {
        unsupported_repo_result()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        unsupported_repo_result()
    }

    fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn delete_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_create(
        &self,
        _message: &str,
        _include_untracked: bool,
        _keep_index: bool,
        _paths: &[PathBuf],
    ) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        unsupported_repo_result()
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn commit(&self, _message: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryWorktree for UnsupportedRepo {}

struct PanicOpenBackend;

impl GitBackend for PanicOpenBackend {
    fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
        panic!("open should not be called in effect scheduler tests")
    }
}

struct BlockingReleaseGuard {
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl Drop for BlockingReleaseGuard {
    fn drop(&mut self) {
        let (lock, condvar) = &*self.release;
        let mut released = lock.lock().expect("release mutex");
        *released = true;
        condvar.notify_all();
    }
}

fn wait_for_release_signal(release: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, condvar) = &**release;
    let mut released = lock.lock().expect("release mutex");
    while !*released {
        released = condvar.wait(released).expect("release wait");
    }
}

enum MetadataRepoMode {
    BlockingRemoteTags,
    ReadyTags,
}

struct MetadataSchedulingRepo {
    spec: RepoSpec,
    mode: MetadataRepoMode,
    started_tx: std::sync::mpsc::Sender<&'static str>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl GitRepository for MetadataSchedulingRepo {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }
}
impl GitRepositoryLog for MetadataSchedulingRepo {
    fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        unsupported_repo_result()
    }

    fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
        unsupported_repo_result()
    }

    fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
        unsupported_repo_result()
    }
}
impl GitRepositoryHistory for MetadataSchedulingRepo {}
impl GitRepositoryRemotes for MetadataSchedulingRepo {
    fn list_remotes(&self) -> Result<Vec<Remote>> {
        unsupported_repo_result()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        unsupported_repo_result()
    }

    fn fetch_all(&self) -> Result<()> {
        unsupported_repo_result()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        unsupported_repo_result()
    }

    fn push(&self) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryStatus for MetadataSchedulingRepo {
    fn status(&self) -> Result<RepoStatus> {
        unsupported_repo_result()
    }
}
impl GitRepositoryDiff for MetadataSchedulingRepo {
    fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
        unsupported_repo_result()
    }
}
impl GitRepositoryPorcelain for MetadataSchedulingRepo {
    fn current_branch(&self) -> Result<String> {
        unsupported_repo_result()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        unsupported_repo_result()
    }

    fn list_tags(&self) -> Result<Vec<worktree_core::domain::Tag>> {
        match self.mode {
            MetadataRepoMode::ReadyTags => {
                let _ = self.started_tx.send("tags");
                Ok(Vec::new())
            }
            MetadataRepoMode::BlockingRemoteTags => unsupported_repo_result(),
        }
    }

    fn list_remote_tags(&self) -> Result<Vec<worktree_core::domain::RemoteTag>> {
        match self.mode {
            MetadataRepoMode::BlockingRemoteTags => {
                let _ = self.started_tx.send("remote_tags");
                wait_for_release_signal(&self.release);
                Ok(Vec::new())
            }
            MetadataRepoMode::ReadyTags => unsupported_repo_result(),
        }
    }

    fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn delete_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_create(
        &self,
        _message: &str,
        _include_untracked: bool,
        _keep_index: bool,
        _paths: &[PathBuf],
    ) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        unsupported_repo_result()
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn commit(&self, _message: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryWorktree for MetadataSchedulingRepo {}

enum SelectedDiffRepoMode {
    BlockingDiff,
    ReadyDiff,
}

struct SelectedDiffSchedulingRepo {
    spec: RepoSpec,
    mode: SelectedDiffRepoMode,
    started_tx: std::sync::mpsc::Sender<RepoId>,
    started_repo_id: RepoId,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl GitRepository for SelectedDiffSchedulingRepo {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }
}
impl GitRepositoryLog for SelectedDiffSchedulingRepo {
    fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        unsupported_repo_result()
    }

    fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
        unsupported_repo_result()
    }

    fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
        unsupported_repo_result()
    }
}
impl GitRepositoryHistory for SelectedDiffSchedulingRepo {}
impl GitRepositoryRemotes for SelectedDiffSchedulingRepo {
    fn list_remotes(&self) -> Result<Vec<Remote>> {
        unsupported_repo_result()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        unsupported_repo_result()
    }

    fn fetch_all(&self) -> Result<()> {
        unsupported_repo_result()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        unsupported_repo_result()
    }

    fn push(&self) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryStatus for SelectedDiffSchedulingRepo {
    fn status(&self) -> Result<RepoStatus> {
        unsupported_repo_result()
    }
}
impl GitRepositoryDiff for SelectedDiffSchedulingRepo {
    fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
        unsupported_repo_result()
    }

    fn diff_parsed_cancellable(
        &self,
        target: &DiffTarget,
        cancellation: &CancellationToken,
    ) -> Result<worktree_core::domain::Diff> {
        let _ = self.started_tx.send(self.started_repo_id);
        if matches!(self.mode, SelectedDiffRepoMode::BlockingDiff) {
            while !cancellation.is_cancelled() {
                let (lock, condvar) = &*self.release;
                let released = lock.lock().expect("release mutex");
                let (released, _) = condvar
                    .wait_timeout(released, Duration::from_millis(10))
                    .expect("release wait");
                if *released {
                    break;
                }
            }
        }
        cancellation.check_cancelled()?;
        Ok(worktree_core::domain::Diff::from_unified(
            target.clone(),
            "diff --git a/tracked.txt b/tracked.txt\n",
        ))
    }
}
impl GitRepositoryPorcelain for SelectedDiffSchedulingRepo {
    fn current_branch(&self) -> Result<String> {
        unsupported_repo_result()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        unsupported_repo_result()
    }

    fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn delete_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_create(
        &self,
        _message: &str,
        _include_untracked: bool,
        _keep_index: bool,
        _paths: &[PathBuf],
    ) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        unsupported_repo_result()
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn commit(&self, _message: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryWorktree for SelectedDiffSchedulingRepo {}

struct RecordingLogRepo {
    spec: RepoSpec,
    calls: Arc<std::sync::Mutex<Vec<String>>>,
}

impl GitRepository for RecordingLogRepo {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }
}
impl GitRepositoryLog for RecordingLogRepo {
    fn log_history_mode_page(
        &self,
        mode: LogScope,
        limit: usize,
        cursor: Option<&LogCursor>,
    ) -> Result<LogPage> {
        self.calls
            .lock()
            .expect("log recording mutex")
            .push(format!(
                "history {mode:?} {limit} {}",
                cursor
                    .map(|cursor| cursor.last_seen.as_ref())
                    .unwrap_or("none")
            ));
        Ok(LogPage {
            commits: Vec::new(),
            next_cursor: None,
        })
    }

    fn log_head_page(&self, limit: usize, cursor: Option<&LogCursor>) -> Result<LogPage> {
        self.calls
            .lock()
            .expect("log recording mutex")
            .push(format!(
                "head {limit} {}",
                cursor
                    .map(|cursor| cursor.last_seen.as_ref())
                    .unwrap_or("none")
            ));
        Ok(LogPage {
            commits: Vec::new(),
            next_cursor: None,
        })
    }

    fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
        unsupported_repo_result()
    }

    fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
        unsupported_repo_result()
    }
}
impl GitRepositoryHistory for RecordingLogRepo {}
impl GitRepositoryRemotes for RecordingLogRepo {
    fn list_remotes(&self) -> Result<Vec<Remote>> {
        unsupported_repo_result()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        unsupported_repo_result()
    }

    fn fetch_all(&self) -> Result<()> {
        unsupported_repo_result()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        unsupported_repo_result()
    }

    fn push(&self) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryStatus for RecordingLogRepo {
    fn status(&self) -> Result<RepoStatus> {
        unsupported_repo_result()
    }
}
impl GitRepositoryDiff for RecordingLogRepo {
    fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
        unsupported_repo_result()
    }
}
impl GitRepositoryPorcelain for RecordingLogRepo {
    fn current_branch(&self) -> Result<String> {
        unsupported_repo_result()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        unsupported_repo_result()
    }

    fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn delete_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_create(
        &self,
        _message: &str,
        _include_untracked: bool,
        _keep_index: bool,
        _paths: &[PathBuf],
    ) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        unsupported_repo_result()
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn commit(&self, _message: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryWorktree for RecordingLogRepo {}

struct RecordingCheckoutRepo {
    spec: RepoSpec,
    calls: Arc<std::sync::Mutex<Vec<String>>>,
}

impl GitRepository for RecordingCheckoutRepo {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }
}
impl GitRepositoryLog for RecordingCheckoutRepo {
    fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        unsupported_repo_result()
    }

    fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
        unsupported_repo_result()
    }

    fn reflog_head(&self, _limit: usize) -> Result<Vec<ReflogEntry>> {
        unsupported_repo_result()
    }
}
impl GitRepositoryHistory for RecordingCheckoutRepo {}
impl GitRepositoryRemotes for RecordingCheckoutRepo {
    fn list_remotes(&self) -> Result<Vec<Remote>> {
        unsupported_repo_result()
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
        unsupported_repo_result()
    }

    fn fetch_all(&self) -> Result<()> {
        unsupported_repo_result()
    }

    fn pull(&self, _mode: PullMode) -> Result<()> {
        unsupported_repo_result()
    }

    fn push(&self) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryStatus for RecordingCheckoutRepo {
    fn status(&self) -> Result<RepoStatus> {
        unsupported_repo_result()
    }
}
impl GitRepositoryDiff for RecordingCheckoutRepo {
    fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
        unsupported_repo_result()
    }
}
impl GitRepositoryPorcelain for RecordingCheckoutRepo {
    fn current_branch(&self) -> Result<String> {
        unsupported_repo_result()
    }

    fn list_branches(&self) -> Result<Vec<Branch>> {
        unsupported_repo_result()
    }

    fn create_branch(&self, name: &str, target: &CommitId) -> Result<()> {
        self.calls
            .lock()
            .expect("checkout recording mutex")
            .push(format!("create {name} {}", target.as_ref()));
        Ok(())
    }

    fn delete_branch(&self, _name: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn checkout_branch(&self, name: &str) -> Result<()> {
        self.calls
            .lock()
            .expect("checkout recording mutex")
            .push(format!("checkout {name}"));
        Ok(())
    }

    fn checkout_remote_branch(&self, remote: &str, branch: &str, local_branch: &str) -> Result<()> {
        self.calls
            .lock()
            .expect("checkout recording mutex")
            .push(format!(
                "checkout_remote {remote}/{branch} -> {local_branch}"
            ));
        Ok(())
    }

    fn checkout_commit(&self, id: &CommitId) -> Result<()> {
        self.calls
            .lock()
            .expect("checkout recording mutex")
            .push(format!("checkout_commit {}", id.as_ref()));
        Ok(())
    }

    fn checkout_pull_request(&self, remote: &str, number: u64) -> Result<()> {
        self.calls
            .lock()
            .expect("checkout recording mutex")
            .push(format!("checkout_pull {remote} #{number}"));
        Ok(())
    }

    fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn revert(&self, _id: &CommitId) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_create(
        &self,
        _message: &str,
        _include_untracked: bool,
        _keep_index: bool,
        _paths: &[PathBuf],
    ) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_list(&self) -> Result<Vec<StashEntry>> {
        unsupported_repo_result()
    }

    fn stash_apply(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stash_drop(&self, _index: usize) -> Result<()> {
        unsupported_repo_result()
    }

    fn stage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn unstage(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }

    fn commit(&self, _message: &str) -> Result<()> {
        unsupported_repo_result()
    }

    fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
        unsupported_repo_result()
    }
}
impl GitRepositoryWorktree for RecordingCheckoutRepo {}

fn wait_for_checkout_refresh_messages(
    msg_rx: &std::sync::mpsc::Receiver<Msg>,
    repo_id: RepoId,
    expect_refresh_branches: bool,
    expect_load_worktrees: bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_refresh_branches = false;
    let mut saw_load_worktrees = false;
    let mut saw_finished = false;

    while Instant::now() < deadline {
        let msg = match msg_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(msg) => msg,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(err) => panic!("channel closed: {err:?}"),
        };

        match msg {
            Msg::RefreshBranches { repo_id: rid } if rid == repo_id => {
                saw_refresh_branches = true;
            }
            Msg::LoadWorktrees { repo_id: rid } if rid == repo_id => {
                saw_load_worktrees = true;
            }
            Msg::Internal(crate::msg::InternalMsg::RepoActionFinished {
                repo_id: rid,
                action: _,
                result: Ok(()),
                paths: None,
            }) if rid == repo_id => {
                saw_finished = true;
            }
            _ => {}
        }

        if saw_finished
            && saw_refresh_branches == expect_refresh_branches
            && saw_load_worktrees == expect_load_worktrees
        {
            return;
        }
    }

    assert_eq!(
        saw_refresh_branches, expect_refresh_branches,
        "unexpected RefreshBranches emission for repo {repo_id:?}"
    );
    assert_eq!(
        saw_load_worktrees, expect_load_worktrees,
        "unexpected LoadWorktrees emission for repo {repo_id:?}"
    );
    assert!(
        saw_finished,
        "expected RepoActionFinished for repo {repo_id:?}"
    );
}

#[test]
fn checkout_branch_effect_requests_branch_and_worktree_reload_on_success() {
    let repo_id = RepoId(700);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingCheckoutRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-checkout-branch-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CheckoutBranch {
            repo_id,
            name: "feature".to_string(),
        },
    );

    wait_for_checkout_refresh_messages(&msg_rx, repo_id, true, true);
    assert_eq!(
        *calls.lock().expect("checkout recording mutex"),
        vec!["checkout feature".to_string()]
    );
}

#[test]
fn checkout_remote_branch_effect_requests_branch_and_worktree_reload_on_success() {
    let repo_id = RepoId(701);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingCheckoutRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-checkout-remote-branch-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CheckoutRemoteBranch {
            repo_id,
            remote: "origin".to_string(),
            branch: "feature".to_string(),
            local_branch: "feature".to_string(),
        },
    );

    wait_for_checkout_refresh_messages(&msg_rx, repo_id, true, true);
    assert_eq!(
        *calls.lock().expect("checkout recording mutex"),
        vec!["checkout_remote origin/feature -> feature".to_string()]
    );
}

#[test]
fn checkout_pull_request_effect_requests_branch_and_worktree_reload_on_success() {
    let repo_id = RepoId(703);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingCheckoutRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-checkout-pull-request-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CheckoutPullRequest {
            repo_id,
            remote: "origin".to_string(),
            number: 42,
        },
    );

    wait_for_checkout_refresh_messages(&msg_rx, repo_id, true, true);
    assert_eq!(
        *calls.lock().expect("checkout recording mutex"),
        vec!["checkout_pull origin #42".to_string()]
    );
}

#[test]
fn checkout_commit_effect_requests_worktree_reload_on_success() {
    let repo_id = RepoId(702);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingCheckoutRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-checkout-commit-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();
    let commit_id = CommitId("deadbeef".into());

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CheckoutCommit {
            repo_id,
            commit_id: commit_id.clone(),
        },
    );

    wait_for_checkout_refresh_messages(&msg_rx, repo_id, false, true);
    assert_eq!(
        *calls.lock().expect("checkout recording mutex"),
        vec![format!("checkout_commit {}", commit_id.as_ref())]
    );
}

#[test]
fn create_branch_and_checkout_effect_requests_branch_and_worktree_reload_on_success() {
    let repo_id = RepoId(703);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingCheckoutRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-create-branch-and-checkout-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::CreateBranchAndCheckout {
            repo_id,
            name: "feature".to_string(),
            target: "HEAD".to_string(),
        },
    );

    wait_for_checkout_refresh_messages(&msg_rx, repo_id, true, true);
    assert_eq!(
        *calls.lock().expect("checkout recording mutex"),
        vec![
            "create feature HEAD".to_string(),
            "checkout feature".to_string()
        ]
    );
}

fn recv_n_msgs(msg_rx: &std::sync::mpsc::Receiver<Msg>, n: usize) {
    for _ in 0..n {
        msg_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("expected effect scheduler message");
    }
}

#[test]
fn open_repo_effect_emits_repo_opened_ok() {
    struct Backend {
        repo: Arc<dyn GitRepository>,
    }
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Ok(Arc::clone(&self.repo))
        }
    }

    let repo_id = RepoId(42);
    let workdir = unique_temp_path("worktree-open-repo-ok");
    let repo: Arc<dyn GitRepository> = Arc::new(UnsupportedRepo {
        spec: RepoSpec {
            workdir: workdir.clone(),
        },
    });
    let backend: Arc<dyn GitBackend> = Arc::new(Backend { repo });

    let executor = super::executor::TaskExecutor::new(1);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::OpenRepo {
            repo_id,
            path: workdir.clone(),
        },
    );

    let msg = msg_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("expected RepoOpenedOk");
    match msg {
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedOk {
            repo_id: got_repo_id,
            spec,
            repo,
        }) => {
            assert_eq!(got_repo_id, repo_id);
            assert_eq!(spec.workdir, workdir);
            assert_eq!(repo.spec().workdir, workdir);
        }
        _ => panic!("expected RepoOpenedOk"),
    }
}

#[test]
fn open_repo_effect_emits_repo_opened_err() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            Err(Error::new(ErrorKind::Backend(
                "backend open failed".to_string(),
            )))
        }
    }

    let repo_id = RepoId(43);
    let workdir = unique_temp_path("worktree-open-repo-err");
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let executor = super::executor::TaskExecutor::new(1);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::OpenRepo {
            repo_id,
            path: workdir.clone(),
        },
    );

    let msg = msg_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("expected RepoOpenedErr");
    match msg {
        Msg::Internal(crate::msg::InternalMsg::RepoOpenedErr {
            repo_id: got_repo_id,
            spec,
            error,
        }) => {
            assert_eq!(got_repo_id, repo_id);
            assert_eq!(spec.workdir, workdir);
            assert!(matches!(error.kind(), ErrorKind::Backend(_)));
        }
        _ => panic!("expected RepoOpenedErr"),
    }
}

#[test]
fn open_repo_effect_suppresses_result_after_cancellation() {
    use std::sync::{Condvar, Mutex};

    struct Backend {
        started_tx: std::sync::mpsc::Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
        repo: Arc<dyn GitRepository>,
    }

    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            let _ = self.started_tx.send(());
            let (lock, condvar) = &*self.release;
            let mut released = lock.lock().expect("release mutex");
            while !*released {
                released = condvar.wait(released).expect("release condvar");
            }
            Ok(Arc::clone(&self.repo))
        }
    }

    let repo_id = RepoId(44);
    let workdir = unique_temp_path("worktree-open-repo-cancelled");
    let repo: Arc<dyn GitRepository> = Arc::new(UnsupportedRepo {
        spec: RepoSpec {
            workdir: workdir.clone(),
        },
    });
    let (started_tx, started_rx) = std::sync::mpsc::channel::<()>();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(Backend {
        started_tx,
        release: Arc::clone(&release),
        repo,
    });

    let executor = super::executor::TaskExecutor::new(1);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();
    let msg_tx = super::worker_channel::StoreWorkerSender::for_test_msg_sender(msg_tx);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: workdir.clone(),
        },
    ));
    let thread_state = Arc::new(std::sync::RwLock::new(Arc::new(state)));
    let mut repo_task_tokens = FxHashMap::default();
    let repo_load_executor = super::executor::TaskExecutor::new(1);
    let metadata_executor = super::executor::TaskExecutor::new(1);

    super::effects::schedule_effect(
        super::effects::EffectExecutors {
            executor: &executor,
            repo_load_executor: &repo_load_executor,
            session_persist_executor: &executor,
            metadata_executor: &metadata_executor,
        },
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx.clone(),
        Effect::OpenRepo {
            repo_id,
            path: workdir,
        },
    );
    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("open effect did not start");

    super::effects::schedule_effect(
        super::effects::EffectExecutors {
            executor: &executor,
            repo_load_executor: &repo_load_executor,
            session_persist_executor: &executor,
            metadata_executor: &metadata_executor,
        },
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx,
        Effect::CancelRepoLoads {
            repo_id,
            load_epoch: 0,
        },
    );
    {
        let (lock, condvar) = &*release;
        let mut released = lock.lock().expect("release mutex");
        *released = true;
        condvar.notify_all();
    }

    assert!(
        msg_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "cancelled open effect should not emit a result"
    );
}

#[test]
fn open_repo_effects_are_bounded_by_repo_load_executor() {
    struct Backend {
        started_tx: std::sync::mpsc::Sender<PathBuf>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl GitBackend for Backend {
        fn open(&self, path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            let workdir = path.to_path_buf();
            let _ = self.started_tx.send(workdir);
            wait_for_release_signal(&self.release);
            Err(Error::new(ErrorKind::Backend(
                "open released by test".to_string(),
            )))
        }
    }

    let repo_a = RepoId(45);
    let repo_b = RepoId(46);
    let workdir_a = unique_temp_path("worktree-open-repo-bounded-a");
    let workdir_b = unique_temp_path("worktree-open-repo-bounded-b");
    let (started_tx, started_rx) = std::sync::mpsc::channel::<PathBuf>();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let _release_guard = BlockingReleaseGuard {
        release: Arc::clone(&release),
    };
    let backend: Arc<dyn GitBackend> = Arc::new(Backend {
        started_tx,
        release: Arc::clone(&release),
    });

    let executor = super::executor::TaskExecutor::new(1);
    let repo_load_executor = super::executor::TaskExecutor::new(1);
    let metadata_executor = super::executor::TaskExecutor::new(1);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, _msg_rx) = std::sync::mpsc::channel::<Msg>();
    let msg_tx = super::worker_channel::StoreWorkerSender::for_test_msg_sender(msg_tx);
    let thread_state = Arc::new(std::sync::RwLock::new(Arc::new(AppState::default())));
    let mut repo_task_tokens = FxHashMap::default();
    let executors = super::effects::EffectExecutors {
        executor: &executor,
        repo_load_executor: &repo_load_executor,
        session_persist_executor: &executor,
        metadata_executor: &metadata_executor,
    };

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx.clone(),
        Effect::OpenRepo {
            repo_id: repo_a,
            path: workdir_a.clone(),
        },
    );
    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first open did not start"),
        workdir_a
    );

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx,
        Effect::OpenRepo {
            repo_id: repo_b,
            path: workdir_b.clone(),
        },
    );
    assert!(
        started_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "second open should wait for the single repo-load worker"
    );

    {
        let (lock, condvar) = &*release;
        let mut released = lock.lock().expect("release mutex");
        *released = true;
        condvar.notify_all();
    }
    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second open did not start after worker was released"),
        workdir_b
    );
}

#[test]
fn worktree_and_submodule_effects_report_missing_repo_handle() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            panic!("open should not be called in this test")
        }
    }

    let repo_id = RepoId(77);
    let executor = super::executor::TaskExecutor::new(1);
    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = FxHashMap::default();
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx.clone(),
        Effect::LoadWorktrees { repo_id },
    );
    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadSubmodules { repo_id },
    );

    let first = msg_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("expected WorktreesLoaded");
    let second = msg_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("expected SubmodulesLoaded");

    match first {
        Msg::Internal(crate::msg::InternalMsg::WorktreesLoaded {
            repo_id: got_repo_id,
            result: Err(error),
        }) => {
            assert_eq!(got_repo_id, repo_id);
            assert!(
                matches!(error.kind(), ErrorKind::Backend(message) if message.contains("Repository handle not found"))
            );
        }
        _ => panic!("expected WorktreesLoaded missing-handle error"),
    }

    match second {
        Msg::Internal(crate::msg::InternalMsg::SubmodulesLoaded {
            repo_id: got_repo_id,
            result: Err(error),
        }) => {
            assert_eq!(got_repo_id, repo_id);
            assert!(
                matches!(error.kind(), ErrorKind::Backend(message) if message.contains("Repository handle not found"))
            );
        }
        _ => panic!("expected SubmodulesLoaded missing-handle error"),
    }
}

#[test]
fn load_log_effect_uses_history_mode_api() {
    let repo_id = RepoId(498);
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let cursor = LogCursor {
        last_seen: CommitId("cursor".into()),
        resume_from: None,
        resume_token: None,
    };
    let repo: Arc<dyn GitRepository> = Arc::new(RecordingLogRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-load-log-history-mode-effect"),
        },
        calls: Arc::clone(&calls),
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadLog {
            repo_id,
            seq: 1,
            scope: LogScope::NoMerges,
            author: None,
            refs: Vec::new(),
            limit: 20,
            cursor: Some(cursor.clone()),
        },
    );

    let msg = msg_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("expected LogLoaded");
    match msg {
        Msg::Internal(crate::msg::InternalMsg::LogLoaded {
            repo_id: got_repo_id,
            seq,
            scope,
            cursor: got_cursor,
            result: Ok(page),
        }) => {
            assert_eq!(got_repo_id, repo_id);
            assert_eq!(seq, 1, "the reply carries the sequence of its request");
            assert_eq!(scope, LogScope::NoMerges);
            assert_eq!(got_cursor, Some(cursor));
            assert!(page.commits.is_empty());
            assert!(page.next_cursor.is_none());
        }
        _ => panic!("expected LogLoaded"),
    }

    assert_eq!(
        *calls.lock().expect("log recording mutex"),
        vec!["history NoMerges 20 cursor".to_string()]
    );
}

#[test]
fn activation_load_effect_is_not_blocked_by_main_executor_queue() {
    let repo_id = RepoId(499);
    let repo: Arc<dyn GitRepository> = Arc::new(UnsupportedRepo {
        spec: RepoSpec {
            workdir: unique_temp_path("worktree-foreground-load-effect"),
        },
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let executor = super::executor::TaskExecutor::new(1);
    let (block_started_tx, block_started_rx) = std::sync::mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let release_task = Arc::clone(&release);
    executor.spawn(move || {
        block_started_tx.send(()).expect("send block started");
        let (lock, condvar) = &*release_task;
        let mut released = lock.lock().expect("release mutex");
        while !*released {
            released = condvar.wait(released).expect("release wait");
        }
    });
    block_started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("executor blocker started");

    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();
    schedule_effect_for_test(
        &executor,
        &executor,
        &backend,
        &repos,
        msg_tx,
        Effect::LoadStatus { repo_id },
    );

    let msg = msg_rx.recv_timeout(Duration::from_secs(1));
    {
        let (lock, condvar) = &*release;
        let mut released = lock.lock().expect("release mutex");
        *released = true;
        condvar.notify_all();
    }

    match msg.expect("foreground load should not wait behind queued executor work") {
        Msg::Internal(crate::msg::InternalMsg::StatusLoaded {
            repo_id: got_repo_id,
            ..
        }) => assert_eq!(got_repo_id, repo_id),
        other => panic!("expected status load result, got {other:?}"),
    }
}

#[test]
fn remote_tag_load_for_one_repo_does_not_block_other_repo_metadata_refresh() {
    let repo_a = RepoId(510);
    let repo_b = RepoId(511);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let _release_guard = BlockingReleaseGuard {
        release: Arc::clone(&release),
    };
    let (started_tx, started_rx) = std::sync::mpsc::channel::<&'static str>();
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(
            repo_a,
            Arc::new(MetadataSchedulingRepo {
                spec: RepoSpec {
                    workdir: unique_temp_path("worktree-metadata-blocking-remote-tags"),
                },
                mode: MetadataRepoMode::BlockingRemoteTags,
                started_tx: started_tx.clone(),
                release: Arc::clone(&release),
            }) as Arc<dyn GitRepository>,
        );
        repos.insert(
            repo_b,
            Arc::new(MetadataSchedulingRepo {
                spec: RepoSpec {
                    workdir: unique_temp_path("worktree-metadata-ready-tags"),
                },
                mode: MetadataRepoMode::ReadyTags,
                started_tx,
                release: Arc::clone(&release),
            }) as Arc<dyn GitRepository>,
        );
        repos
    };
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let executor = super::executor::TaskExecutor::new(1);
    let repo_load_executor = super::executor::TaskExecutor::new(1);
    let metadata_executor =
        super::executor::TaskExecutor::new(super::executor::metadata_worker_threads());
    let (msg_tx, _msg_rx) = std::sync::mpsc::channel::<Msg>();
    let msg_tx = super::worker_channel::StoreWorkerSender::for_test_msg_sender(msg_tx);
    let mut state = AppState::default();
    state.repos.push(RepoState::new_opening(
        repo_a,
        RepoSpec {
            workdir: unique_temp_path("worktree-metadata-state-a"),
        },
    ));
    state.repos.push(RepoState::new_opening(
        repo_b,
        RepoSpec {
            workdir: unique_temp_path("worktree-metadata-state-b"),
        },
    ));
    let thread_state = Arc::new(std::sync::RwLock::new(Arc::new(state)));
    let mut repo_task_tokens = FxHashMap::default();
    let executors = super::effects::EffectExecutors {
        executor: &executor,
        repo_load_executor: &repo_load_executor,
        session_persist_executor: &executor,
        metadata_executor: &metadata_executor,
    };

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx.clone(),
        Effect::LoadRemoteTags { repo_id: repo_a },
    );
    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("remote tag task did not start"),
        "remote_tags"
    );

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx,
        Effect::LoadTags { repo_id: repo_b },
    );

    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("metadata refresh for repo B should not wait behind repo A remote tags"),
        "tags"
    );
}

#[test]
fn cancelled_selected_diff_does_not_keep_executor_busy_for_next_repo() {
    let repo_a = RepoId(520);
    let repo_b = RepoId(521);
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let _release_guard = BlockingReleaseGuard {
        release: Arc::clone(&release),
    };
    let (started_tx, started_rx) = std::sync::mpsc::channel::<RepoId>();
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(
            repo_a,
            Arc::new(SelectedDiffSchedulingRepo {
                spec: RepoSpec {
                    workdir: unique_temp_path("worktree-selected-diff-blocking-a"),
                },
                mode: SelectedDiffRepoMode::BlockingDiff,
                started_tx: started_tx.clone(),
                started_repo_id: repo_a,
                release: Arc::clone(&release),
            }) as Arc<dyn GitRepository>,
        );
        repos.insert(
            repo_b,
            Arc::new(SelectedDiffSchedulingRepo {
                spec: RepoSpec {
                    workdir: unique_temp_path("worktree-selected-diff-ready-b"),
                },
                mode: SelectedDiffRepoMode::ReadyDiff,
                started_tx,
                started_repo_id: repo_b,
                release: Arc::clone(&release),
            }) as Arc<dyn GitRepository>,
        );
        repos
    };
    let backend: Arc<dyn GitBackend> = Arc::new(PanicOpenBackend);
    let executor = super::executor::TaskExecutor::new(1);
    let repo_load_executor = super::executor::TaskExecutor::new(1);
    let metadata_executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, _msg_rx) = std::sync::mpsc::channel::<Msg>();
    let msg_tx = super::worker_channel::StoreWorkerSender::for_test_msg_sender(msg_tx);
    let target_a = DiffTarget::WorkingTree {
        path: PathBuf::from("repo-a.txt"),
        area: DiffArea::Unstaged,
    };
    let target_b = DiffTarget::WorkingTree {
        path: PathBuf::from("repo-b.txt"),
        area: DiffArea::Unstaged,
    };
    let mut state = AppState::default();
    let mut repo_state_a = RepoState::new_opening(
        repo_a,
        RepoSpec {
            workdir: unique_temp_path("worktree-selected-diff-state-a"),
        },
    );
    repo_state_a.diff_state.diff_target = Some(target_a.clone());
    let mut repo_state_b = RepoState::new_opening(
        repo_b,
        RepoSpec {
            workdir: unique_temp_path("worktree-selected-diff-state-b"),
        },
    );
    repo_state_b.diff_state.diff_target = Some(target_b.clone());
    state.repos.push(repo_state_a);
    state.repos.push(repo_state_b);
    let thread_state = Arc::new(std::sync::RwLock::new(Arc::new(state)));
    let mut repo_task_tokens = FxHashMap::default();
    let executors = super::effects::EffectExecutors {
        executor: &executor,
        repo_load_executor: &repo_load_executor,
        session_persist_executor: &executor,
        metadata_executor: &metadata_executor,
    };

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx.clone(),
        Effect::LoadSelectedDiff {
            repo_id: repo_a,
            load_patch_diff: true,
            load_file_text: false,
            preview_text_side: None,
            load_submodule_summary: false,
            load_file_image: false,
        },
    );
    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("repo A diff task did not start"),
        repo_a
    );

    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx.clone(),
        Effect::CancelRepoLoads {
            repo_id: repo_a,
            load_epoch: 0,
        },
    );
    super::effects::schedule_effect(
        executors,
        &thread_state,
        &backend,
        &repos,
        &mut repo_task_tokens,
        msg_tx,
        Effect::LoadSelectedDiff {
            repo_id: repo_b,
            load_patch_diff: true,
            load_file_text: false,
            preview_text_side: None,
            load_submodule_summary: false,
            load_file_image: false,
        },
    );

    assert_eq!(
        started_rx
            .recv_timeout(Duration::from_millis(200))
            .expect("repo B diff should start once repo A load epoch is cancelled"),
        repo_b
    );
}

#[test]
fn schedule_effect_dispatches_many_variants_with_repo_present() {
    struct Backend;
    impl GitBackend for Backend {
        fn open(&self, _path: &Path) -> std::result::Result<Arc<dyn GitRepository>, Error> {
            panic!("open should not be called in this test")
        }
    }

    let repo_id = RepoId(500);
    let workdir = unique_temp_path("worktree-effects-dispatch");
    std::fs::create_dir_all(&workdir).expect("create workdir");

    let repo: Arc<dyn GitRepository> = Arc::new(UnsupportedRepo {
        spec: RepoSpec {
            workdir: workdir.clone(),
        },
    });
    let repos: FxHashMap<RepoId, Arc<dyn GitRepository>> = {
        let mut repos = FxHashMap::default();
        repos.insert(repo_id, repo);
        repos
    };

    let backend: Arc<dyn GitBackend> = Arc::new(Backend);
    let executor = super::executor::TaskExecutor::new(1);
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<Msg>();

    let target = DiffTarget::WorkingTree {
        path: PathBuf::from("tracked.txt"),
        area: DiffArea::Unstaged,
    };
    let mut state = AppState::default();
    let mut repo_state = crate::model::RepoState::new_opening(
        repo_id,
        RepoSpec {
            workdir: workdir.clone(),
        },
    );
    repo_state.diff_state.diff_target = Some(target.clone());
    repo_state.conflict_state.conflict_file_path = Some(PathBuf::from("conflicted.txt"));
    state.active_repo = Some(repo_id);
    state.repos.push(repo_state);
    let commit_id = CommitId("deadbeef".into());
    let effect_specs: Vec<(Effect, usize)> = vec![
        (Effect::LoadBranches { repo_id }, 1),
        (Effect::LoadRemotes { repo_id }, 1),
        (Effect::LoadRemoteBranches { repo_id }, 1),
        (Effect::LoadStatus { repo_id }, 1),
        (Effect::LoadHeadBranch { repo_id }, 1),
        (Effect::LoadUpstreamDivergence { repo_id }, 1),
        (
            Effect::LoadLog {
                repo_id,
                seq: 1,
                scope: LogScope::CurrentBranch,
                author: None,
                refs: Vec::new(),
                limit: 20,
                cursor: None,
            },
            1,
        ),
        (
            Effect::LoadLog {
                repo_id,
                seq: 2,
                scope: LogScope::AllBranches,
                author: None,
                refs: Vec::new(),
                limit: 20,
                cursor: Some(LogCursor {
                    last_seen: CommitId("cursor".into()),
                    resume_from: None,
                    resume_token: None,
                }),
            },
            1,
        ),
        (Effect::LoadTags { repo_id }, 1),
        (Effect::LoadRemoteTags { repo_id }, 1),
        (Effect::LoadStashes { repo_id, limit: 3 }, 1),
        (Effect::LoadReflog { repo_id, limit: 5 }, 1),
        (
            Effect::LoadFileHistory {
                repo_id,
                path: PathBuf::from("tracked.txt"),
                limit: 10,
            },
            1,
        ),
        (
            Effect::LoadBlame {
                repo_id,
                path: PathBuf::from("tracked.txt"),
                source: worktree_core::domain::BlameSource::Revision(Some("HEAD".to_string())),
            },
            1,
        ),
        (Effect::LoadWorktrees { repo_id }, 1),
        (Effect::LoadSubmodules { repo_id }, 1),
        (Effect::LoadRebaseAndMergeState { repo_id }, 2),
        (Effect::LoadRebaseState { repo_id }, 1),
        (Effect::LoadMergeCommitMessage { repo_id }, 1),
        (
            Effect::LoadCommitDetails {
                repo_id,
                commit_id: commit_id.clone(),
            },
            1,
        ),
        (
            Effect::LoadDiff {
                repo_id,
                target: target.clone(),
            },
            1,
        ),
        (
            Effect::LoadDiffFile {
                repo_id,
                target: target.clone(),
            },
            1,
        ),
        (
            Effect::LoadDiffFileImage {
                repo_id,
                target: target.clone(),
            },
            1,
        ),
        (
            Effect::LoadSelectedDiff {
                repo_id,
                load_patch_diff: true,
                load_file_text: true,
                load_file_image: false,
                load_submodule_summary: false,
                preview_text_side: None,
            },
            2,
        ),
        (
            Effect::LoadConflictFile {
                repo_id,
                path: PathBuf::from("conflicted.txt"),
                mode: crate::model::ConflictFileLoadMode::CurrentOnly,
            },
            1,
        ),
        (
            Effect::LoadSelectedConflictFile {
                repo_id,
                mode: crate::model::ConflictFileLoadMode::CurrentOnly,
            },
            1,
        ),
        (
            Effect::SaveWorktreeFile {
                repo_id,
                path: PathBuf::from("nested/new.txt"),
                contents: "content".to_string(),
                stage: true,
            },
            1,
        ),
        (
            Effect::CheckoutBranch {
                repo_id,
                name: "main".to_string(),
            },
            1,
        ),
        (
            Effect::CheckoutRemoteBranch {
                repo_id,
                remote: "origin".to_string(),
                branch: "main".to_string(),
                local_branch: "main".to_string(),
            },
            1,
        ),
        (
            Effect::CheckoutCommit {
                repo_id,
                commit_id: commit_id.clone(),
            },
            1,
        ),
        (
            Effect::CherryPickCommit {
                repo_id,
                commit_id: commit_id.clone(),
                commit: true,
                mainline: None,
                summary: "pick me".into(),
            },
            1,
        ),
        (
            Effect::RevertCommit {
                repo_id,
                commit_id: commit_id.clone(),
            },
            1,
        ),
        (
            Effect::CreateBranch {
                repo_id,
                name: "topic".to_string(),
                target: "HEAD".to_string(),
            },
            1,
        ),
        (
            Effect::CreateBranchAndCheckout {
                repo_id,
                name: "topic2".to_string(),
                target: "HEAD".to_string(),
            },
            1,
        ),
        (
            Effect::RenameBranch {
                repo_id,
                old_name: "topic2".to_string(),
                new_name: "renamed-topic".to_string(),
            },
            1,
        ),
        (
            Effect::DeleteBranch {
                repo_id,
                name: "topic".to_string(),
            },
            1,
        ),
        (
            Effect::ForceDeleteBranch {
                repo_id,
                name: "topic".to_string(),
            },
            1,
        ),
        (
            Effect::ExportPatch {
                repo_id,
                commit_id: commit_id.clone(),
                dest: PathBuf::from("out.patch"),
            },
            1,
        ),
        (
            Effect::ArchiveZip {
                repo_id,
                revision: "v1.0".to_string(),
                dest: PathBuf::from("archive-v1.0.zip"),
            },
            1,
        ),
        ((Effect::CleanupRepo { repo_id }), 1),
        (
            Effect::SetAssumeUnchanged {
                repo_id,
                path: PathBuf::from("src/big.bin"),
                enable: false,
            },
            1,
        ),
        ((Effect::LoadAssumeUnchanged { repo_id }), 1),
        (
            Effect::ApplyPatch {
                repo_id,
                patch: PathBuf::from("change.patch"),
            },
            1,
        ),
        (
            Effect::AddWorktree {
                repo_id,
                path: PathBuf::from("wt"),
                reference: Some("main".to_string()),
            },
            1,
        ),
        (
            Effect::RemoveWorktree {
                repo_id,
                path: PathBuf::from("wt"),
            },
            1,
        ),
        (
            Effect::AddSubmodule {
                repo_id,
                url: "https://example.com/repo.git".to_string(),
                path: PathBuf::from("sub"),
                branch: None,
                name: None,
                force: false,
                approved_sources: Vec::new(),
                auth: None,
            },
            1,
        ),
        (
            Effect::UpdateSubmodules {
                approved_sources: Vec::new(),
                repo_id,
                auth: None,
            },
            1,
        ),
        (
            Effect::RemoveSubmodule {
                repo_id,
                path: PathBuf::from("sub"),
            },
            1,
        ),
        (
            Effect::StageHunk {
                repo_id,
                patch: "@@ -1 +1 @@".to_string(),
            },
            1,
        ),
        (
            Effect::UnstageHunk {
                repo_id,
                patch: "@@ -1 +1 @@".to_string(),
            },
            1,
        ),
        (
            Effect::ApplyWorktreePatch {
                repo_id,
                patch: "@@ -1 +1 @@".to_string(),
                reverse: true,
            },
            1,
        ),
        (
            Effect::StagePath {
                repo_id,
                path: PathBuf::from("tracked.txt"),
            },
            1,
        ),
        (
            Effect::StagePaths {
                repo_id,
                paths: vec![PathBuf::from("b.txt"), PathBuf::from("a.txt")].into(),
            },
            1,
        ),
        (
            Effect::UnstagePath {
                repo_id,
                path: PathBuf::from("tracked.txt"),
            },
            1,
        ),
        (
            Effect::UnstagePaths {
                repo_id,
                paths: vec![PathBuf::from("b.txt"), PathBuf::from("a.txt")].into(),
            },
            1,
        ),
        (
            Effect::DiscardWorktreeChangesPath {
                repo_id,
                path: PathBuf::from("tracked.txt"),
            },
            1,
        ),
        (
            Effect::DiscardWorktreeChangesPaths {
                repo_id,
                paths: vec![PathBuf::from("b.txt"), PathBuf::from("a.txt")],
            },
            1,
        ),
        (
            Effect::Commit {
                repo_id,
                message: "msg".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::CommitAmend {
                repo_id,
                message: "msg".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::SafePushAfterCommit {
                repo_id,
                context: worktree_core::services::SafePushAfterCommitContext {
                    amend: false,
                    local_branch: None,
                    pre_head: None,
                    post_head: None,
                },
                auth: None,
            },
            1,
        ),
        (
            Effect::FetchAll {
                repo_id,
                prune: true,
                auth: None,
            },
            1,
        ),
        (Effect::PruneMergedBranches { repo_id }, 1),
        (Effect::PruneLocalTags { repo_id }, 1),
        (
            Effect::Pull {
                repo_id,
                mode: PullMode::FastForwardOnly,
                auth: None,
            },
            1,
        ),
        (
            Effect::PullBranch {
                repo_id,
                remote: "origin".to_string(),
                branch: "main".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::MergeRef {
                repo_id,
                reference: "origin/main".to_string(),
            },
            1,
        ),
        (
            Effect::SquashRef {
                repo_id,
                reference: "origin/main".to_string(),
            },
            1,
        ),
        (
            Effect::Push {
                repo_id,
                auth: None,
            },
            1,
        ),
        (
            Effect::PushAfterCommit {
                repo_id,
                target: worktree_core::services::SafePushAfterCommitTarget {
                    remote: "origin".to_string(),
                    branch: "main".to_string(),
                    local_branch: "main".to_string(),
                    local_head: CommitId("2222222222222222222222222222222222222222".into()),
                },
                set_upstream: false,
                auth: None,
            },
            1,
        ),
        (
            Effect::ForcePush {
                repo_id,
                auth: None,
            },
            1,
        ),
        (
            Effect::ForcePushWithLease {
                repo_id,
                lease: worktree_core::services::ForcePushLease {
                    remote: "origin".to_string(),
                    branch: "main".to_string(),
                    expected: CommitId("1111111111111111111111111111111111111111".into()),
                    local_branch: "main".to_string(),
                    local_head: CommitId("2222222222222222222222222222222222222222".into()),
                },
                auth: None,
            },
            1,
        ),
        (
            Effect::PushSetUpstream {
                repo_id,
                remote: "origin".to_string(),
                branch: "main".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::SetUpstreamBranch {
                repo_id,
                branch: "main".to_string(),
                upstream: "origin/main".to_string(),
            },
            1,
        ),
        (
            Effect::UnsetUpstreamBranch {
                repo_id,
                branch: "main".to_string(),
            },
            1,
        ),
        (
            Effect::DeleteRemoteBranch {
                repo_id,
                remote: "origin".to_string(),
                branch: "main".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::Reset {
                repo_id,
                target: "HEAD~1".to_string(),
                mode: worktree_core::services::ResetMode::Mixed,
            },
            1,
        ),
        (
            Effect::Rebase {
                repo_id,
                onto: "main".to_string(),
            },
            1,
        ),
        (
            Effect::RebaseContinue {
                repo_id,
                auth: None,
            },
            1,
        ),
        (Effect::RebaseAbort { repo_id }, 1),
        (Effect::MergeAbort { repo_id }, 1),
        (
            Effect::CreateTag {
                repo_id,
                name: "v1.0.0".to_string(),
                target: "HEAD".to_string(),
                message: None,
                annotated: false,
            },
            1,
        ),
        (
            Effect::DeleteTag {
                repo_id,
                name: "v1.0.0".to_string(),
            },
            1,
        ),
        (
            Effect::PushTag {
                repo_id,
                remote: "origin".to_string(),
                name: "v1.0.0".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::DeleteRemoteTag {
                repo_id,
                remote: "origin".to_string(),
                name: "v1.0.0".to_string(),
                auth: None,
            },
            1,
        ),
        (
            Effect::AddRemote {
                repo_id,
                name: "origin".to_string(),
                url: "https://example.com/repo.git".to_string(),
            },
            1,
        ),
        (
            Effect::RemoveRemote {
                repo_id,
                name: "origin".to_string(),
            },
            1,
        ),
        (
            Effect::SetRemoteUrl {
                repo_id,
                name: "origin".to_string(),
                url: "https://example.com/repo.git".to_string(),
                kind: worktree_core::services::RemoteUrlKind::Fetch,
            },
            1,
        ),
        (
            Effect::SetRemoteSshKey {
                repo_id,
                remote: "origin".to_string(),
                key: Some("~/.ssh/id_ed25519".to_string()),
            },
            1,
        ),
        (
            Effect::CheckoutConflictSide {
                repo_id,
                path: PathBuf::from("conflicted.txt"),
                side: worktree_core::services::ConflictSide::Ours,
            },
            1,
        ),
        (
            Effect::AcceptConflictDeletion {
                repo_id,
                path: PathBuf::from("conflicted.txt"),
            },
            1,
        ),
        (
            Effect::CheckoutConflictBase {
                repo_id,
                path: PathBuf::from("conflicted.txt"),
            },
            1,
        ),
        (
            Effect::LaunchMergetool {
                repo_id,
                path: PathBuf::from("conflicted.txt"),
                preference:
                    worktree_core::external_merge_tool::ExternalMergeToolSelection::FromGitConfig,
            },
            1,
        ),
        (
            Effect::Stash {
                repo_id,
                message: "wip".to_string(),
                include_untracked: false,
                keep_index: false,
                paths: Vec::new().into(),
            },
            1,
        ),
        (Effect::ApplyStash { repo_id, index: 0 }, 1),
        (Effect::PopStash { repo_id, index: 0 }, 1),
        (Effect::DropStash { repo_id, index: 0 }, 2),
        // Success hook reloads the stash list, so a failing backend (the
        // default `stash_branch` is Unsupported) still reports exactly the
        // finished action.
        (
            Effect::StashBranch {
                repo_id,
                index: 0,
                branch: "recover".to_string(),
            },
            1,
        ),
    ];

    for (effect, expected_messages) in effect_specs {
        schedule_effect_with_state_for_test(
            &executor,
            &executor,
            &backend,
            &repos,
            state.clone(),
            msg_tx.clone(),
            effect,
        );
        recv_n_msgs(&msg_rx, expected_messages);
    }
}

#[test]
fn status_for_paths_patch_replaces_and_appends_covered_entries() {
    use crate::msg::InternalMsg;
    use worktree_core::domain::{FileStatus, FileStatusKind};
    use worktree_core::services::StatusForPaths;
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
    let entry = |path: &str, kind| FileStatus {
        path: PathBuf::from(path),
        kind,
        conflict: None,
    };
    repo.set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus {
        unstaged: vec![
            entry("a.txt", FileStatusKind::Modified),
            entry("b.txt", FileStatusKind::Modified),
            entry("c.txt", FileStatusKind::Untracked),
        ],
        staged: vec![entry("a.txt", FileStatusKind::Modified)],
    })));
    let status_rev_before = repo.status_rev;
    state.repos.push(repo);

    let paths: std::sync::Arc<[PathBuf]> =
        vec![PathBuf::from("a.txt"), PathBuf::from("d.txt")].into();
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths: paths.clone(),
            result: Ok(StatusForPaths::Lists {
                // a.txt changed shape (now deleted); d.txt is newly untracked.
                unstaged: vec![
                    entry("a.txt", FileStatusKind::Deleted),
                    entry("d.txt", FileStatusKind::Untracked),
                ],
                staged: vec![],
            }),
        }),
    );
    assert!(
        effects.is_empty(),
        "a mergeable patch runs no further effects"
    );
    let status = state.repos[0].status.ready().unwrap();
    assert_eq!(
        status
            .unstaged
            .iter()
            .map(|e| (e.path.display().to_string(), e.kind))
            .collect::<Vec<_>>(),
        vec![
            ("a.txt".to_string(), FileStatusKind::Deleted),
            ("b.txt".to_string(), FileStatusKind::Modified),
            ("c.txt".to_string(), FileStatusKind::Untracked),
            ("d.txt".to_string(), FileStatusKind::Untracked),
        ],
        "covered paths are replaced in place, uncovered ones survive, order matches a full scan"
    );
    assert!(
        status.staged.is_empty(),
        "a covered path's staged half is replaced too (here: removed)"
    );
    assert!(
        state.repos[0].status_rev > status_rev_before,
        "a content change bumps the rev the UI fingerprints"
    );

    // A no-op patch (same content) must not churn the rev.
    let rev_after_change = state.repos[0].status_rev;
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result: Ok(StatusForPaths::Lists {
                unstaged: vec![
                    entry("a.txt", FileStatusKind::Deleted),
                    entry("d.txt", FileStatusKind::Untracked),
                ],
                staged: vec![],
            }),
        }),
    );
    assert_eq!(
        state.repos[0].status_rev, rev_after_change,
        "re-applying the same patch leaves the snapshot untouched"
    );
}

/// A watcher can report a *directory*; git then expands it, so the response
/// carries files the request never named. The old list must still have those
/// files replaced, not appended-to — otherwise the same path shows up twice in
/// the staged/unstaged list.
#[test]
fn status_for_paths_patch_replaces_entries_git_expanded_from_a_directory() {
    use crate::msg::InternalMsg;
    use worktree_core::domain::{FileStatus, FileStatusKind};
    use worktree_core::services::StatusForPaths;
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
    let entry = |path: &str, kind| FileStatus {
        path: PathBuf::from(path),
        kind,
        conflict: None,
    };
    repo.set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus {
        unstaged: vec![
            entry("dir/a.txt", FileStatusKind::Modified),
            entry("dir/b.txt", FileStatusKind::Modified),
        ],
        staged: vec![],
    })));
    state.repos.push(repo);

    // The request names the directory; the response expands it to its files.
    let paths: std::sync::Arc<[PathBuf]> = vec![PathBuf::from("dir")].into();
    reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result: Ok(StatusForPaths::Lists {
                unstaged: vec![
                    entry("dir/a.txt", FileStatusKind::Deleted),
                    entry("dir/b.txt", FileStatusKind::Modified),
                ],
                staged: vec![],
            }),
        }),
    );

    let status = state.repos[0].status.ready().unwrap();
    let listed: Vec<String> = status
        .unstaged
        .iter()
        .map(|e| e.path.display().to_string())
        .collect();
    assert_eq!(
        listed,
        vec!["dir/a.txt".to_string(), "dir/b.txt".to_string()],
        "each expanded file appears exactly once — no duplicate rows"
    );
    assert_eq!(
        status.unstaged[0].kind,
        FileStatusKind::Deleted,
        "the freshly reported entry wins over the stale old one"
    );
}

/// The merge patches both lanes (a staged path's unstaged half is replaced
/// too), so it must finish both lanes' in-flight bits: a dispatch that holds
/// the staged flag — the stage/unstage/commit completion refresh — would
/// otherwise leave it stranded and coalesce every later staged refresh.
#[test]
fn status_for_paths_merge_finishes_both_status_lanes() {
    use crate::model::RepoLoadsInFlight;
    use crate::msg::InternalMsg;
    use worktree_core::services::StatusForPaths;
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
    state.repos[0].set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus::default())));

    // A dispatch that holds both lanes, as the action-completion refresh does.
    assert!(
        state.repos[0]
            .loads_in_flight
            .request(RepoLoadsInFlight::WORKTREE_STATUS)
    );
    assert!(
        state.repos[0]
            .loads_in_flight
            .request(RepoLoadsInFlight::STAGED_STATUS)
    );

    let paths: std::sync::Arc<[PathBuf]> = vec![PathBuf::from("a.txt")].into();
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result: Ok(StatusForPaths::Lists {
                unstaged: vec![],
                staged: vec![],
            }),
        }),
    );
    assert!(
        effects.is_empty(),
        "no coalesced refresh to replay, no fallback to run"
    );
    assert!(
        !state.repos[0]
            .loads_in_flight
            .is_in_flight(RepoLoadsInFlight::WORKTREE_STATUS)
            && !state.repos[0]
                .loads_in_flight
                .is_in_flight(RepoLoadsInFlight::STAGED_STATUS),
        "the merge finishes both lanes it patched"
    );
}

#[test]
fn status_for_paths_falls_back_to_a_full_scan_when_it_cannot_merge() {
    use crate::msg::InternalMsg;
    use worktree_core::services::StatusForPaths;
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
    let paths: std::sync::Arc<[PathBuf]> = vec![PathBuf::from("a.txt")].into();

    // Renames answer NeedsFullScan.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths: paths.clone(),
            result: Ok(StatusForPaths::NeedsFullScan),
        }),
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadStatus { repo_id: id }] if *id == repo_id
    ));

    // There is no settled snapshot to merge onto yet.
    state.repos[0].set_status(Loadable::Ready(std::sync::Arc::new(RepoStatus::default())));
    state.repos[0].status = Loadable::NotLoaded;
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::StatusForPathsLoaded {
            repo_id,
            paths,
            result: Ok(StatusForPaths::Lists {
                unstaged: vec![],
                staged: vec![],
            }),
        }),
    );
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::LoadStatus { repo_id: id }] if *id == repo_id
        ),
        "nothing settled to merge onto — the full scan answers"
    );
}

#[test]
fn lfs_image_preview_loads_on_request_and_lands_gated_by_target() {
    use crate::msg::InternalMsg;
    use worktree_core::domain::FileDiffImage;
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
    let target = DiffTarget::WorkingTree {
        path: PathBuf::from("art/logo.png"),
        area: DiffArea::Unstaged,
    };
    repo.diff_state.diff_target = Some(target.clone());
    state.repos.push(repo);

    // The request only runs while its target is the one on screen.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadLfsImagePreview {
            repo_id,
            target: target.clone(),
        },
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadLfsImagePreview { repo_id: id, target: t }] if *id == repo_id && *t == target
    ));
    assert!(matches!(
        state.repos[0].diff_state.lfs_image_preview,
        Loadable::Loading
    ));

    // A second request while loading does not stack.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::LoadLfsImagePreview {
            repo_id,
            target: target.clone(),
        },
    );
    assert!(effects.is_empty());

    // The reply lands as the smudged new side, keyed to the target's path.
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::LfsImagePreviewLoaded {
            repo_id,
            target: target.clone(),
            result: Ok(Some(FileDiffImage {
                path: PathBuf::from("art/logo.png"),
                old: None,
                new: Some(b"fake-smudged-image-bytes".to_vec()),
            })),
        }),
    );
    assert!(effects.is_empty());
    match &state.repos[0].diff_state.lfs_image_preview {
        Loadable::Ready(Some(image)) => {
            assert_eq!(image.path, PathBuf::from("art/logo.png"));
            assert!(image.old.is_none());
            assert_eq!(
                image.new.as_deref(),
                Some(b"fake-smudged-image-bytes".as_slice())
            );
        }
        other => panic!("expected a ready preview, got {other:?}"),
    }

    // A reply for a target that is no longer on screen is dropped.
    let moved = DiffTarget::WorkingTree {
        path: PathBuf::from("art/other.png"),
        area: DiffArea::Unstaged,
    };
    let effects = reduce(
        &mut repos,
        &id_alloc,
        &mut state,
        Msg::Internal(InternalMsg::LfsImagePreviewLoaded {
            repo_id,
            target: moved,
            result: Ok(None),
        }),
    );
    assert!(effects.is_empty());
    assert!(matches!(
        state.repos[0].diff_state.lfs_image_preview,
        Loadable::Ready(Some(_))
    ));
}
