//! VCS-neutral read model — the one read surface every backend implements.
//!
//! GitComet is git-native, and [`crate::services::GitRepository`] grew around
//! git's shape: one trait where history walks, diffs, refs, and status sit
//! next to staging, stashing, reflogs, worktrees, and interactive rebase.
//! Jujutsu shares the browsing half of that surface but almost none of the
//! mutating half, so this module carves out the neutral **read** core that
//! both VCSes can serve, leaving every backend-specific concern behind.
//!
//! # The boundary: what is shared (≈70%) and what is not (≈30%)
//!
//! **Shared — belongs on [`VcsReadModel`]:**
//!
//! * History graph queries: paged walks with an optional author filter,
//!   reported incrementally through [`LogChunk`] and cancellable mid-walk.
//! * Per-node details (message, author, dates, changed files).
//! * Diffs in all three shapes — against the working copy, for one commit,
//!   and between two commits — expressed through [`DiffTarget`].
//! * Refs/bookmarks with their remote counterpart state: a branch carries an
//!   optional upstream and an ahead/behind [`UpstreamDivergence`]. That pair
//!   is the *divergent bookmark* notion under both systems — git's local vs
//!   remote-tracking branch, jj's bookmark target vs its remote target. A
//!   bookmark that moved on both sides is `ahead > 0 && behind > 0`.
//! * Working-copy changes, with an **optional selection set** (see
//!   [`VcsReadModel::status`]).
//! * Remotes and tags, which both systems read the same way.
//!
//! **Not shared — stays on backend-specific traits:**
//!
//! * *Change identity* (deliberately excluded, the hard gap): git commits
//!   have no change id; jj changes are identified by a `ChangeId` that
//!   survives amendment while the commit id changes. This trait identifies
//!   graph nodes by [`CommitId`] — a content hash both systems can produce —
//!   and stays silent about evolution: amending, abandoning, or rebasing
//!   simply yields ids this trait has never seen before. jj's describe /
//!   restore / op-log workflows stay on the jj side; git's rebase tooling
//!   stays on [`crate::services::GitRepository`].
//! * *Selection-set semantics*: git's index is a real, user-curated set;
//!   jj has none (the working copy is the change). [`RepoStatus::staged`]
//!   models the lane without mandating it — see [`VcsReadModel::status`].
//! * Git-shaped reads with no jj counterpart (stash, reflog, linked
//!   worktrees, submodules, blame, interactive-rebase todo listing) and
//!   jj-shaped reads with no git counterpart (revsets, op log) stay on
//!   their own traits.
//! * *All writes.* During compatibility the gix jj adapter routes the
//!   supported writes itself; the native jj flavor (P3) defines its own
//!   write trait. A read trait that cannot corrupt a repository is also the
//!   safest common denominator to build shared panels on.
//!
//! # Migration shape
//!
//! A blanket implementation gives every existing
//! [`crate::services::GitRepository`] (the gix backend, the jj adapter, and
//! every test double) [`VcsReadModel`] for free, so callers can move to the
//! neutral surface one site at a time while both traits stay live
//! ("双轨"). Backends that were never git-shaped implement this trait
//! directly and keep their own write surface.

use crate::domain::{
    Branch, CommitDetails, CommitFileChange, CommitId, Diff, DiffTarget, HistoryMode, LogCursor,
    LogPage, Remote, RemoteBranch, RepoSpec, RepoStatus, Tag, UpstreamDivergence,
};
use crate::services::{CancellationToken, GitRepository, LogChunk, RepoCapabilities};
use rustc_hash::FxHashMap;

/// One page request against the history graph.
///
/// Bundled into a struct (rather than positional parameters) so filters can
/// be added without breaking implementations: a revset-style text filter, a
/// path filter, or date bounds all fit here later.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LogQuery {
    /// Which slice of the graph to walk.
    pub mode: HistoryMode,
    /// Case-insensitive substring match against the author name shown in the
    /// UI. `None` walks unfiltered.
    pub author: Option<String>,
    /// Page size. `0` is the caller's mistake, not "everything": backends
    /// pass it straight through to their walk.
    pub limit: usize,
    /// Where the previous page ended; `None` starts from the tip.
    pub cursor: Option<LogCursor>,
}

/// The VCS-neutral read surface. See the [module documentation](self) for the
/// shared/backend-specific boundary; the short version: everything here is a
/// query that cannot mutate the repository.
pub trait VcsReadModel: Send + Sync {
    /// Location/identity of the open repository.
    fn spec(&self) -> &RepoSpec;

    /// What this backend supports. Shared panels use this to hide affordances
    /// whose underlying concept does not exist (e.g. the staged lane on a
    /// backend without a selection set).
    fn capabilities(&self) -> RepoCapabilities;

    /// Walk the history graph one page at a time.
    ///
    /// An author filter may have to scan past many non-matching commits, so
    /// the walk reports each prefix of the page through `on_chunk` (every
    /// chunk is a prefix of the next and of the returned page) and checks
    /// `cancellation` as it goes. Backends that cannot push the filter down
    /// must still honor it client-side or say so in their own docs —
    /// returning unfiltered pages silently is a bug.
    fn log_page(
        &self,
        query: &LogQuery,
        cancellation: &CancellationToken,
        on_chunk: &mut dyn FnMut(LogChunk),
    ) -> Result<LogPage, crate::error::Error>;

    /// [`Self::log_page`] for callers with nothing to cancel and no use for
    /// partial pages.
    fn log_page_once(&self, query: &LogQuery) -> Result<LogPage, crate::error::Error> {
        self.log_page(query, &CancellationToken::new(), &mut |_| {})
    }

    /// Full details for one graph node identified by [`CommitId`].
    fn commit_details(&self, id: &CommitId) -> Result<CommitDetails, crate::error::Error>;

    /// Structured diff for any of the three shapes [`DiffTarget`] expresses:
    /// working copy (selection set vs working tree — see [`Self::status`]),
    /// one commit, or a range. `to = None` in a range means "up to the live
    /// working tree".
    fn diff(&self, target: &DiffTarget) -> Result<Diff, crate::error::Error>;

    /// Files that differ between two points (`from` is the base/older side),
    /// without the line-level payload. The compare-selected-nodes feature
    /// lists ranges this way.
    fn diff_range_files(
        &self,
        from: &CommitId,
        to: Option<&CommitId>,
    ) -> Result<Vec<CommitFileChange>, crate::error::Error>;

    /// The ref/bookmark the working copy sits on. Empty or a backend-defined
    /// placeholder when the working copy is detached from any named ref.
    fn current_branch(&self) -> Result<String, crate::error::Error>;

    /// The graph node the working copy is based on, if the backend exposes
    /// one. On git this is `HEAD`'s commit; on jj the working-copy commit's
    /// parent chain tip as the backend chooses to surface it.
    fn head_commit_id(&self) -> Result<Option<CommitId>, crate::error::Error>;

    /// Named refs/bookmarks with their remote counterpart state. See the
    /// module docs for how ahead/behind maps to divergent bookmarks.
    fn list_branches(&self) -> Result<Vec<Branch>, crate::error::Error>;

    /// Refs as they exist on the remote side (`remote/name` in git terms).
    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>, crate::error::Error>;

    /// Tags, newest-first or backend order.
    fn list_tags(&self) -> Result<Vec<Tag>, crate::error::Error>;

    /// Remotes the backend knows about.
    fn list_remotes(&self) -> Result<Vec<Remote>, crate::error::Error>;

    /// Ahead/behind of the current ref against its remote counterpart, when
    /// the backend can compute it cheaply.
    fn upstream_divergence(&self) -> Result<Option<UpstreamDivergence>, crate::error::Error>;

    /// Working-copy changes split into an **optional selection set** and the
    /// rest.
    ///
    /// `staged` is git's index — a user-curated subset of changes picked for
    /// the next commit. A backend without that concept (jj snapshots the
    /// whole working copy into the current change) returns every change in
    /// `unstaged`, leaves `staged` empty, and reports
    /// [`RepoCapabilities::staging`] = false so the UI hides the lane. The
    /// two-lane shape is deliberate: one `RepoStatus` serves every backend,
    /// and the capability bit — not the data shape — decides what renders.
    fn status(&self) -> Result<RepoStatus, crate::error::Error>;

    /// Author name → email over a bounded recent slice of history, for
    /// per-email avatars on name-only surfaces. Names absent from the map
    /// keep their initials. The default reports no emails.
    fn author_email_map(&self) -> Result<FxHashMap<String, String>, crate::error::Error> {
        Ok(FxHashMap::default())
    }
}

/// Every [`GitRepository`] already is a [`VcsReadModel`]: reads delegate to
/// the corresponding trait methods, so the gix backend, the jj adapter, and
/// all existing test doubles implement the neutral surface for free while
/// both traits stay live.
///
/// Calls are fully qualified (`GitRepository::…`) — inside this impl both
/// traits are candidates for `self.` resolution and the qualified form pins
/// the delegation to the legacy trait, keeping it recursion-proof.
impl<T: GitRepository + ?Sized> VcsReadModel for T {
    fn spec(&self) -> &RepoSpec {
        GitRepository::spec(self)
    }

    fn capabilities(&self) -> RepoCapabilities {
        GitRepository::capabilities(self)
    }

    fn log_page(
        &self,
        query: &LogQuery,
        cancellation: &CancellationToken,
        on_chunk: &mut dyn FnMut(LogChunk),
    ) -> Result<LogPage, crate::error::Error> {
        GitRepository::log_history_mode_page_streaming(
            self,
            query.mode,
            query.author.as_deref(),
            query.limit,
            query.cursor.as_ref(),
            cancellation,
            on_chunk,
        )
    }

    fn commit_details(&self, id: &CommitId) -> Result<CommitDetails, crate::error::Error> {
        GitRepository::commit_details(self, id)
    }

    fn diff(&self, target: &DiffTarget) -> Result<Diff, crate::error::Error> {
        GitRepository::diff_parsed(self, target)
    }

    fn diff_range_files(
        &self,
        from: &CommitId,
        to: Option<&CommitId>,
    ) -> Result<Vec<CommitFileChange>, crate::error::Error> {
        GitRepository::diff_range_files(self, from, to)
    }

    fn current_branch(&self) -> Result<String, crate::error::Error> {
        GitRepository::current_branch(self)
    }

    fn head_commit_id(&self) -> Result<Option<CommitId>, crate::error::Error> {
        GitRepository::head_commit_id(self)
    }

    fn list_branches(&self) -> Result<Vec<Branch>, crate::error::Error> {
        GitRepository::list_branches(self)
    }

    fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>, crate::error::Error> {
        GitRepository::list_remote_branches(self)
    }

    fn list_tags(&self) -> Result<Vec<Tag>, crate::error::Error> {
        GitRepository::list_tags(self)
    }

    fn list_remotes(&self) -> Result<Vec<Remote>, crate::error::Error> {
        GitRepository::list_remotes(self)
    }

    fn upstream_divergence(&self) -> Result<Option<UpstreamDivergence>, crate::error::Error> {
        GitRepository::upstream_divergence(self)
    }

    fn status(&self) -> Result<RepoStatus, crate::error::Error> {
        GitRepository::status(self)
    }

    fn author_email_map(&self) -> Result<FxHashMap<String, String>, crate::error::Error> {
        GitRepository::author_email_map(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileStatus, FileStatusKind, Upstream};
    use crate::error::{Error, ErrorKind};
    use crate::services::Result;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    /// Minimal [`GitRepository`] double: required methods are `unsupported`
    /// stubs, and exactly the reads the blanket impl delegates to return
    /// fixed data or record their arguments.
    struct NeutralReadRepo {
        spec: RepoSpec,
        calls: Mutex<Vec<(&'static str, Option<String>, usize, Option<String>)>>,
    }

    impl NeutralReadRepo {
        fn new() -> Self {
            Self {
                spec: RepoSpec {
                    workdir: PathBuf::from("/tmp/neutral-read-repo"),
                },
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(&'static str, Option<String>, usize, Option<String>)> {
            self.calls.lock().expect("calls mutex").clone()
        }
    }

    fn unsupported<T>() -> Result<T> {
        Err(Error::new(ErrorKind::Unsupported(
            "not exercised by vcs_read delegation tests",
        )))
    }

    impl GitRepository for NeutralReadRepo {
        fn spec(&self) -> &RepoSpec {
            &self.spec
        }

        fn log_history_mode_page_streaming(
            &self,
            mode: HistoryMode,
            author: Option<&str>,
            limit: usize,
            cursor: Option<&LogCursor>,
            cancellation: &CancellationToken,
            on_chunk: &mut dyn FnMut(LogChunk),
        ) -> Result<LogPage> {
            cancellation.check_cancelled()?;
            self.calls.lock().expect("calls mutex").push((
                "log",
                mode.is_all_branches().then_some("all".to_string()),
                limit,
                author.map(str::to_string),
            ));
            let _ = cursor;
            on_chunk(LogChunk {
                commits: Vec::new(),
                scanned: 3,
            });
            Ok(LogPage {
                commits: Vec::new(),
                next_cursor: None,
            })
        }

        fn log_head_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
            unsupported()
        }

        fn commit_details(&self, _id: &CommitId) -> Result<CommitDetails> {
            Ok(CommitDetails {
                id: CommitId("c0ffee".into()),
                message: "details message".to_string(),
                author_name: "Author".to_string(),
                author_email: "author@example.com".to_string(),
                authored_at_unix: 1,
                committed_at: String::new(),
                committed_at_unix: 2,
                parent_ids: Vec::new(),
                files: Vec::new(),
            })
        }

        fn diff_parsed(&self, target: &DiffTarget) -> Result<Diff> {
            Ok(Diff {
                target: target.clone(),
                lines: Vec::new(),
            })
        }

        fn diff_unified(&self, _target: &DiffTarget) -> Result<String> {
            unsupported()
        }

        fn current_branch(&self) -> Result<String> {
            Ok("main".to_string())
        }

        fn head_commit_id(&self) -> Result<Option<CommitId>> {
            Ok(Some(CommitId("c0ffee".into())))
        }

        fn list_branches(&self) -> Result<Vec<Branch>> {
            Ok(vec![Branch {
                name: "main".to_string(),
                target: CommitId("c0ffee".into()),
                upstream: Some(Upstream {
                    remote: "origin".to_string(),
                    branch: "main".to_string(),
                }),
                divergence: Some(UpstreamDivergence {
                    ahead: 2,
                    behind: 1,
                }),
            }])
        }

        fn list_remote_branches(&self) -> Result<Vec<RemoteBranch>> {
            Ok(vec![RemoteBranch {
                remote: "origin".to_string(),
                name: "main".to_string(),
                target: CommitId("c0ffee".into()),
            }])
        }

        fn list_tags(&self) -> Result<Vec<Tag>> {
            Ok(vec![Tag {
                name: "v1".to_string(),
                target: CommitId("c0ffee".into()),
                created_at: Some(4),
            }])
        }

        fn list_remotes(&self) -> Result<Vec<Remote>> {
            Ok(vec![Remote {
                name: "origin".to_string(),
                url: Some("https://example.com/repo".to_string()),
            }])
        }

        fn upstream_divergence(&self) -> Result<Option<UpstreamDivergence>> {
            Ok(Some(UpstreamDivergence {
                ahead: 2,
                behind: 1,
            }))
        }

        fn status(&self) -> Result<RepoStatus> {
            Ok(RepoStatus {
                staged: Vec::new(),
                unstaged: vec![FileStatus {
                    path: PathBuf::from("README.md"),
                    kind: FileStatusKind::Modified,
                    conflict: None,
                }],
            })
        }

        fn author_email_map(&self) -> Result<FxHashMap<String, String>> {
            let mut map = FxHashMap::default();
            map.insert("Author".to_string(), "author@example.com".to_string());
            Ok(map)
        }

        fn reflog_head(&self, _limit: usize) -> Result<Vec<crate::domain::ReflogEntry>> {
            unsupported()
        }

        fn list_remote_tags(&self) -> Result<Vec<crate::domain::RemoteTag>> {
            unsupported()
        }

        fn create_branch(&self, _name: &str, _target: &CommitId) -> Result<()> {
            unsupported()
        }

        fn delete_branch(&self, _name: &str) -> Result<()> {
            unsupported()
        }

        fn checkout_branch(&self, _name: &str) -> Result<()> {
            unsupported()
        }

        fn checkout_commit(&self, _id: &CommitId) -> Result<()> {
            unsupported()
        }

        fn cherry_pick(&self, _id: &CommitId) -> Result<()> {
            unsupported()
        }

        fn revert(&self, _id: &CommitId) -> Result<()> {
            unsupported()
        }

        fn stash_create(&self, _message: &str, _include_untracked: bool) -> Result<()> {
            unsupported()
        }

        fn stash_list(&self) -> Result<Vec<crate::domain::StashEntry>> {
            unsupported()
        }

        fn stash_apply(&self, _index: usize) -> Result<()> {
            unsupported()
        }

        fn stash_drop(&self, _index: usize) -> Result<()> {
            unsupported()
        }

        fn stage(&self, _paths: &[&Path]) -> Result<()> {
            unsupported()
        }

        fn unstage(&self, _paths: &[&Path]) -> Result<()> {
            unsupported()
        }

        fn commit(&self, _message: &str) -> Result<()> {
            unsupported()
        }

        fn fetch_all(&self) -> Result<()> {
            unsupported()
        }

        fn pull(&self, _mode: crate::services::PullMode) -> Result<()> {
            unsupported()
        }

        fn push(&self) -> Result<()> {
            unsupported()
        }

        fn discard_worktree_changes(&self, _paths: &[&Path]) -> Result<()> {
            unsupported()
        }
    }

    #[test]
    fn log_page_delegates_query_fields_and_forwards_chunks() {
        let repo = NeutralReadRepo::new();
        let query = LogQuery {
            mode: HistoryMode::AllBranches,
            author: Some("Alice".to_string()),
            limit: 9,
            cursor: None,
        };
        let mut chunks = Vec::new();
        let page = repo
            .log_page(&query, &CancellationToken::new(), &mut |chunk| {
                chunks.push(chunk);
            })
            .expect("delegated log walk should succeed");

        assert!(page.commits.is_empty());
        assert_eq!(
            repo.calls(),
            vec![("log", Some("all".to_string()), 9, Some("Alice".to_string()))]
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].scanned, 3);
    }

    #[test]
    fn log_page_once_returns_the_page_without_reporting_chunks() {
        let repo = NeutralReadRepo::new();
        let query = LogQuery {
            limit: 5,
            ..LogQuery::default()
        };
        let page = repo
            .log_page_once(&query)
            .expect("convenience walk should succeed");
        assert_eq!(page.next_cursor, None);
        assert_eq!(repo.calls(), vec![("log", None, 5, None)]);
    }

    #[test]
    fn cancelled_log_page_returns_cancelled() {
        let repo = NeutralReadRepo::new();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = repo
            .log_page(
                &LogQuery {
                    limit: 1,
                    ..LogQuery::default()
                },
                &cancellation,
                &mut |_| {},
            )
            .expect_err("cancelled walks must fail");
        assert!(matches!(error.kind(), ErrorKind::Cancelled));
        assert!(repo.calls().is_empty(), "no walk should start");
    }

    #[test]
    fn blanket_impl_delegates_every_read() {
        // Every call is qualified: on the concrete type both traits are in
        // scope and carry the same method names, and qualification is what
        // pins the assertion to the neutral surface under test.
        let repo = NeutralReadRepo::new();

        assert_eq!(
            VcsReadModel::spec(&repo).workdir,
            PathBuf::from("/tmp/neutral-read-repo")
        );
        assert_eq!(
            VcsReadModel::capabilities(&repo),
            RepoCapabilities::default()
        );
        assert_eq!(
            VcsReadModel::commit_details(&repo, &CommitId("c0ffee".into()))
                .expect("details")
                .message,
            "details message"
        );
        assert_eq!(
            VcsReadModel::diff(
                &repo,
                &DiffTarget::Commit {
                    commit_id: CommitId("c0ffee".into()),
                    path: None,
                }
            )
            .expect("diff")
            .lines
            .len(),
            0
        );
        assert_eq!(VcsReadModel::current_branch(&repo).expect("branch"), "main");
        assert_eq!(
            VcsReadModel::head_commit_id(&repo).expect("head"),
            Some(CommitId("c0ffee".into()))
        );

        let branches = VcsReadModel::list_branches(&repo).expect("branches");
        assert_eq!(branches.len(), 1);
        assert_eq!(
            branches[0].divergence,
            Some(UpstreamDivergence {
                ahead: 2,
                behind: 1
            })
        );

        assert_eq!(
            VcsReadModel::list_remote_branches(&repo).expect("remote branches")[0].remote,
            "origin"
        );
        assert_eq!(VcsReadModel::list_tags(&repo).expect("tags")[0].name, "v1");
        assert_eq!(
            VcsReadModel::list_remotes(&repo).expect("remotes")[0].name,
            "origin"
        );
        assert_eq!(
            VcsReadModel::upstream_divergence(&repo).expect("divergence"),
            Some(UpstreamDivergence {
                ahead: 2,
                behind: 1
            })
        );

        let status = VcsReadModel::status(&repo).expect("status");
        assert!(status.staged.is_empty());
        assert_eq!(status.unstaged.len(), 1);

        let emails = VcsReadModel::author_email_map(&repo).expect("emails");
        assert_eq!(
            emails.get("Author").map(String::as_str),
            Some("author@example.com")
        );
    }

    #[test]
    fn object_safe_through_dyn() {
        // The store holds repositories as `Arc<dyn …>`; the trait must stay
        // object-safe, and the blanket impl must let a concrete backend
        // coerce into the neutral trait object without going through
        // GitRepository first.
        let read: Arc<dyn VcsReadModel> = Arc::new(NeutralReadRepo::new());
        assert_eq!(read.current_branch().expect("branch"), "main");
    }
}
