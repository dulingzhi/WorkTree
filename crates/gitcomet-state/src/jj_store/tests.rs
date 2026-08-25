//! Store-level tests for the jj store, driven through a fake backend so
//! they run without jj installed. The reducer's pure transitions have
//! their own tests in `reducer.rs`; these cover the wiring: the worker
//! loop, effect scheduling, and result routing.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitcomet_core::domain::RepoSpec;
use gitcomet_core::error::{Error, ErrorKind};
use gitcomet_core::services::{CommandOutput, Result};
use gitcomet_jj_core::{
    ChangeId, JjBookmark, JjChange, JjCommitId, JjConflict, JjFileStat, JjFileStatus, JjLogPage,
    JjLogQuery, JjOp, JjRepository,
};

use super::backend::JjBackend;
use super::model::JjAppState;
use super::{JjMsg, JjStore};

/// An in-memory jj repository that records every call and answers from
/// fixed fixtures.
struct FakeJjRepository {
    spec: RepoSpec,
    calls: Mutex<Vec<String>>,
}

impl FakeJjRepository {
    fn new(workdir: &str) -> Arc<Self> {
        Arc::new(Self {
            spec: RepoSpec {
                workdir: PathBuf::from(workdir),
            },
            calls: Mutex::new(Vec::new()),
        })
    }

    fn record(&self, label: &str) {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(label.to_string());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn change(name: &str, working_copy: bool) -> JjChange {
    JjChange {
        change_id: ChangeId(name.to_string()),
        commit_id: JjCommitId(format!("c{name}")),
        divergent: false,
        conflicted: false,
        is_working_copy: working_copy,
        bookmarks: Vec::new(),
        author_name: "A".to_string(),
        author_email: "a@a".to_string(),
        committed_at_unix: 1,
        description: name.to_string(),
    }
}

fn log_page() -> JjLogPage {
    JjLogPage {
        changes: vec![change("at", true), change("base", false)],
        next_cursor: None,
    }
}

impl JjRepository for FakeJjRepository {
    fn spec(&self) -> &RepoSpec {
        &self.spec
    }

    fn snapshot(&self) -> Result<()> {
        self.record("snapshot");
        Ok(())
    }

    fn log(&self, query: &JjLogQuery) -> Result<JjLogPage> {
        self.record(&format!("log:{}", query.revset));
        Ok(log_page())
    }

    fn change_files(&self, change: &ChangeId) -> Result<Vec<JjFileStat>> {
        self.record(&format!("change_files:{}", change.0));
        Ok(vec![
            JjFileStat {
                path: "modified.txt".to_string(),
                status: JjFileStatus::Modified,
            },
            JjFileStat {
                path: "renamed.txt".to_string(),
                status: JjFileStatus::Renamed {
                    from: "old.txt".to_string(),
                },
            },
        ])
    }

    fn file_diff_text(&self, change: &ChangeId, path: &str) -> Result<String> {
        self.record(&format!("file_diff:{}:{}", change.0, path));
        Ok(format!("--- a/{path}\n+++ b/{path}\n+{path} line\n"))
    }

    fn describe(&self, change: &ChangeId, message: &str) -> Result<()> {
        self.record(&format!("describe:{}:{}", change.0, message));
        Ok(())
    }

    fn new_change(&self, message: Option<&str>) -> Result<JjChange> {
        self.record(&format!("new:{message:?}"));
        Ok(change("new-at", true))
    }

    fn abandon(&self, change: &ChangeId) -> Result<()> {
        self.record(&format!("abandon:{}", change.0));
        Ok(())
    }

    fn squash(&self, from: &ChangeId, into: Option<&ChangeId>) -> Result<()> {
        self.record(&format!("squash:{}:{:?}", from.0, into.map(|c| &c.0)));
        Ok(())
    }

    fn split(&self, _change: &ChangeId) -> Result<()> {
        Err(Error::new(ErrorKind::Unsupported("fake split")))
    }

    fn bookmarks(&self) -> Result<Vec<JjBookmark>> {
        self.record("bookmarks");
        Ok(vec![JjBookmark {
            name: "main".to_string(),
            remote: None,
            target_commit_id: JjCommitId("cbase".to_string()),
            conflicted: false,
        }])
    }

    fn bookmark_create(&self, name: &str, _target: &ChangeId) -> Result<()> {
        self.record(&format!("bookmark_create:{name}"));
        Ok(())
    }

    fn bookmark_delete(&self, name: &str) -> Result<()> {
        self.record(&format!("bookmark_delete:{name}"));
        Ok(())
    }

    fn bookmark_rename(&self, old_name: &str, new_name: &str) -> Result<()> {
        self.record(&format!("bookmark_rename:{old_name}:{new_name}"));
        Ok(())
    }

    fn bookmark_track(&self, name: &str, remote: Option<&str>) -> Result<()> {
        self.record(&format!("bookmark_track:{name}:{remote:?}"));
        Ok(())
    }

    fn op_log(&self, _limit: usize) -> Result<Vec<JjOp>> {
        self.record("op_log");
        Ok(vec![JjOp {
            op_id: "op1".to_string(),
            description: "add workspace".to_string(),
            user: "A <a@a>".to_string(),
            started_at_unix: 1,
        }])
    }

    fn op_undo(&self) -> Result<()> {
        self.record("op_undo");
        Ok(())
    }

    fn op_restore(&self, op_id: &str) -> Result<()> {
        self.record(&format!("op_restore:{op_id}"));
        Ok(())
    }

    fn conflicts(&self) -> Result<Vec<JjConflict>> {
        self.record("conflicts");
        Ok(Vec::new())
    }

    fn fetch_all_with_output(&self) -> Result<CommandOutput> {
        self.record("fetch_all");
        Ok(CommandOutput::empty_success("jj git fetch --all-remotes"))
    }

    fn push_tracked_with_output(&self) -> Result<CommandOutput> {
        self.record("push");
        Ok(CommandOutput::empty_success("jj git push"))
    }
}

struct FakeJjBackend {
    repo: Mutex<Option<Arc<FakeJjRepository>>>,
}

impl FakeJjBackend {
    fn with(repo: Arc<FakeJjRepository>) -> Arc<Self> {
        Arc::new(Self {
            repo: Mutex::new(Some(repo)),
        })
    }
}

impl JjBackend for FakeJjBackend {
    fn open(&self, _workdir: &std::path::Path) -> Result<Arc<dyn JjRepository>> {
        self.repo
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|repo| repo as Arc<dyn JjRepository>)
            .ok_or_else(|| Error::new(ErrorKind::Backend("fake repo taken".to_string())))
    }
}

fn wait_until(store: &JjStore, predicate: impl Fn(&JjAppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let snapshot = store.snapshot();
        if predicate(&snapshot) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for jj store state; calls so far satisfy nothing"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn test_error(message: &str) -> Error {
    Error::new(ErrorKind::Backend(message.to_string()))
}

#[test]
fn opening_a_repo_loads_everything() {
    let repo = FakeJjRepository::new("/tmp/fake-jj");
    let (store, _event_rx) = JjStore::new(FakeJjBackend::with(Arc::clone(&repo)));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/fake-jj"),
    });

    wait_until(&store, |state| {
        state.repos.first().is_some_and(|repo| {
            !repo.changes.is_empty()
                && !repo.bookmarks.is_empty()
                && !repo.ops.is_empty()
                && repo.open_error.is_none()
        })
    });
    let snapshot = store.snapshot();
    let repo_state = &snapshot.repos[0];
    assert_eq!(repo_state.changes.len(), 2);
    assert_eq!(
        repo_state
            .working_copy
            .as_ref()
            .map(|c| c.change_id.0.clone()),
        Some("at".to_string())
    );
    assert_eq!(repo_state.bookmarks[0].name, "main");
    assert_eq!(repo_state.ops[0].op_id, "op1");
    assert!(repo_state.conflicts.is_empty());
    assert_eq!(snapshot.active_repo, Some(repo_state.id));

    let calls = repo.calls();
    assert!(calls.contains(&"log:".to_string()));
    assert!(calls.contains(&"bookmarks".to_string()));
    assert!(calls.contains(&"op_log".to_string()));
    assert!(calls.contains(&"conflicts".to_string()));
}

#[test]
fn a_failed_open_reports_the_error() {
    struct MissingBackend;
    impl JjBackend for MissingBackend {
        fn open(&self, _workdir: &std::path::Path) -> Result<Arc<dyn JjRepository>> {
            Err(test_error("no jj here"))
        }
    }
    let (store, _event_rx) = JjStore::new(Arc::new(MissingBackend));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/missing-jj"),
    });

    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|repo| repo.open_error.is_some())
    });
    let snapshot = store.snapshot();
    assert_eq!(snapshot.repos[0].open_error.as_deref(), Some("no jj here"));
}

#[test]
fn mutations_run_and_refresh_the_state() {
    let repo = FakeJjRepository::new("/tmp/fake-jj");
    let (store, _event_rx) = JjStore::new(FakeJjBackend::with(Arc::clone(&repo)));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/fake-jj"),
    });
    wait_until(&store, |state| {
        state.repos.first().is_some_and(|r| !r.changes.is_empty())
    });
    let repo_id = store.snapshot().repos[0].id;

    store.dispatch(JjMsg::DescribeChange {
        repo_id,
        change: ChangeId("at".to_string()),
        message: "describe the working copy".to_string(),
    });

    // The mutation ran against the repository, and its completion cleared
    // the pending marker and refreshed under a bumped epoch (the open loads
    // ran at epoch 0; only a finished command bumps it to 1).
    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|repo| repo.refresh_epoch >= 1 && repo.pending_command.is_none())
    });
    let snapshot = store.snapshot();
    assert_eq!(snapshot.repos[0].refresh_epoch, 1);
    assert!(
        repo.calls()
            .contains(&"describe:at:describe the working copy".to_string())
    );
    // The refresh re-ran the loads.
    assert!(
        repo.calls()
            .iter()
            .filter(|c| c.as_str() == "bookmarks")
            .count()
            >= 2
    );
}

#[test]
fn closing_a_repo_removes_it() {
    let repo = FakeJjRepository::new("/tmp/fake-jj");
    let (store, _event_rx) = JjStore::new(FakeJjBackend::with(Arc::clone(&repo)));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/fake-jj"),
    });
    wait_until(&store, |state| !state.repos.is_empty());
    let repo_id = store.snapshot().repos[0].id;

    store.dispatch(JjMsg::CloseRepo { repo_id });
    wait_until(&store, |state| state.repos.is_empty());
    assert!(store.snapshot().active_repo.is_none());
}

#[test]
fn change_files_and_file_diff_load_into_the_detail_panels() {
    let repo = FakeJjRepository::new("/tmp/fake-jj");
    let (store, _event_rx) = JjStore::new(FakeJjBackend::with(Arc::clone(&repo)));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/fake-jj"),
    });
    wait_until(&store, |state| {
        state.repos.first().is_some_and(|r| !r.changes.is_empty())
    });
    let repo_id = store.snapshot().repos[0].id;
    let epoch = store.snapshot().repos[0].refresh_epoch;

    store.dispatch(JjMsg::LoadChangeFiles {
        repo_id,
        epoch,
        change: ChangeId("at".to_string()),
    });
    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|r| !r.details.files.is_empty())
    });
    let snapshot = store.snapshot();
    let repo_state = &snapshot.repos[0];
    assert_eq!(
        repo_state.details.change.as_ref().map(|c| c.0.clone()),
        Some("at".to_string())
    );
    assert!(!repo_state.details.loading);
    assert_eq!(repo_state.details.files.len(), 2);

    store.dispatch(JjMsg::LoadFileDiff {
        repo_id,
        epoch,
        change: ChangeId("at".to_string()),
        path: "modified.txt".to_string(),
    });
    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|r| r.file_diff.text.is_some())
    });
    let snapshot = store.snapshot();
    let repo_state = &snapshot.repos[0];
    let text = repo_state.file_diff.text.as_deref().expect("diff text");
    assert!(text.contains("+++ b/modified.txt"));
    assert_eq!(repo_state.file_diff.path.as_deref(), Some("modified.txt"));
    assert!(repo.calls().contains(&"change_files:at".to_string()));
    assert!(
        repo.calls()
            .contains(&"file_diff:at:modified.txt".to_string())
    );
}

#[test]
fn a_refresh_clears_the_detail_panels_and_drops_stale_results() {
    let repo = FakeJjRepository::new("/tmp/fake-jj");
    let (store, _event_rx) = JjStore::new(FakeJjBackend::with(Arc::clone(&repo)));
    store.dispatch(JjMsg::OpenRepo {
        workdir: PathBuf::from("/tmp/fake-jj"),
    });
    wait_until(&store, |state| {
        state.repos.first().is_some_and(|r| !r.changes.is_empty())
    });
    let repo_id = store.snapshot().repos[0].id;
    let epoch = store.snapshot().repos[0].refresh_epoch;

    store.dispatch(JjMsg::LoadChangeFiles {
        repo_id,
        epoch,
        change: ChangeId("at".to_string()),
    });
    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|r| !r.details.files.is_empty())
    });

    // A refresh (any mutation or external touch) clears the panel so the
    // view re-requests it under the bumped epoch; a result from the old
    // epoch or for a moved selection cannot land afterwards.
    store.dispatch(JjMsg::RefreshRepo { repo_id });
    wait_until(&store, |state| {
        state
            .repos
            .first()
            .is_some_and(|r| r.refresh_epoch == epoch + 1 && r.details.change.is_none())
    });
    let snapshot = store.snapshot();
    let repo_state = &snapshot.repos[0];
    assert!(repo_state.details.files.is_empty());

    let stale_epoch = epoch;
    store.dispatch(JjMsg::ChangeFilesLoaded {
        repo_id,
        epoch: stale_epoch,
        change: ChangeId("at".to_string()),
        files: vec![JjFileStat {
            path: "stale.txt".to_string(),
            status: JjFileStatus::Added,
        }],
    });
    let snapshot = store.snapshot();
    assert!(snapshot.repos[0].details.files.is_empty());

    // A result for a change the user moved away from within the same
    // epoch is dropped too. The moved-to change's own load is queued
    // before the stale result, and waiting for its files to land proves
    // the worker drained past the stale message.
    let fresh_epoch = store.snapshot().repos[0].refresh_epoch;
    store.dispatch(JjMsg::LoadChangeFiles {
        repo_id,
        epoch: fresh_epoch,
        change: ChangeId("base".to_string()),
    });
    store.dispatch(JjMsg::ChangeFilesLoaded {
        repo_id,
        epoch: fresh_epoch,
        change: ChangeId("at".to_string()),
        files: vec![JjFileStat {
            path: "stale-selection.txt".to_string(),
            status: JjFileStatus::Added,
        }],
    });
    wait_until(&store, |state| {
        state.repos.first().is_some_and(|r| {
            r.details.change.as_ref().map(|c| c.0.as_str()) == Some("base")
                && !r.details.files.is_empty()
        })
    });
    let snapshot = store.snapshot();
    assert!(
        snapshot.repos[0]
            .details
            .files
            .iter()
            .all(|file| file.path != "stale-selection.txt")
    );
}
