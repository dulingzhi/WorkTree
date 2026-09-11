//! `GitRepository` log-domain trait, split out of the former
//! monolithic `services::GitRepository`.

use super::{CancellationToken, LogChunk, Result};
use crate::domain::{
    Commit, CommitDetails, CommitFileChange, CommitId, ContributorCommit, HistoryMode, LogCursor,
    LogPage, RecentCommitMessage, ReflogEntry,
};
use crate::error::{Error, ErrorKind};
use rustc_hash::FxHashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

pub trait GitRepositoryLog {
    fn log_history_mode_page(
        &self,
        mode: HistoryMode,
        limit: usize,
        cursor: Option<&LogCursor>,
    ) -> Result<LogPage> {
        match mode {
            HistoryMode::AllBranches => self.log_all_branches_page(limit, cursor),
            HistoryMode::FullReachable
            | HistoryMode::FirstParent
            | HistoryMode::NoMerges
            | HistoryMode::MergesOnly => self.log_head_page(limit, cursor),
        }
    }

    fn log_history_mode_page_cancellable(
        &self,
        mode: HistoryMode,
        limit: usize,
        cursor: Option<&LogCursor>,
        cancellation: &CancellationToken,
    ) -> Result<LogPage> {
        cancellation.check_cancelled()?;
        let page = self.log_history_mode_page(mode, limit, cursor)?;
        cancellation.check_cancelled()?;
        Ok(page)
    }

    /// Like [`Self::log_history_mode_page`], but restricted to commits whose
    /// author matches `author` (case-insensitive substring match against the
    /// author name shown in the UI), cancellable, and reporting the page as it
    /// is built.
    ///
    /// An author filter has to walk history until it has found `limit` matching
    /// commits, which for a rare author means walking all of it — over ten
    /// seconds on a repository with a million commits. `on_chunk` lets the
    /// caller show what has been found so far instead of nothing at all, and
    /// `cancellation` lets a filter the user has moved on from be dropped
    /// rather than waited out.
    ///
    /// Each chunk carries the whole page built up to that point, so chunks are
    /// prefixes of each other and of the returned page, and applying one is
    /// idempotent. The default implementation ignores the filter and reports
    /// nothing; backends that support filtering override this method.
    fn log_history_mode_page_streaming(
        &self,
        mode: HistoryMode,
        author: Option<&str>,
        limit: usize,
        cursor: Option<&LogCursor>,
        cancellation: &CancellationToken,
        on_chunk: &mut dyn FnMut(LogChunk),
    ) -> Result<LogPage> {
        let _ = (author, on_chunk);
        cancellation.check_cancelled()?;
        let page = self.log_history_mode_page(mode, limit, cursor)?;
        cancellation.check_cancelled()?;
        Ok(page)
    }

    /// [`Self::log_history_mode_page_streaming`] for callers with nothing to
    /// cancel and no use for the intermediate pages.
    fn log_history_mode_page_filtered(
        &self,
        mode: HistoryMode,
        author: Option<&str>,
        limit: usize,
        cursor: Option<&LogCursor>,
    ) -> Result<LogPage> {
        self.log_history_mode_page_streaming(
            mode,
            author,
            limit,
            cursor,
            &CancellationToken::new(),
            &mut |_| {},
        )
    }

    /// Like [`Self::log_history_mode_page_streaming`], but the walk is seeded
    /// from `refs` — full ref names such as `refs/heads/topic`,
    /// `refs/remotes/origin/topic`, or `refs/tags/v1.0` — instead of HEAD (or
    /// every ref, under [`HistoryMode::AllBranches`]). The mode still shapes
    /// the walk: first-parent follows each tip's mainline, and no-merges /
    /// merges-only keep filtering the commits that make the page. An empty
    /// `refs` is not supported here — callers treat it as "no filter" and use
    /// the HEAD/all-refs entry points instead.
    ///
    /// The default fails loudly rather than falling back to unfiltered
    /// history: a backend that cannot resolve refs must not silently pretend
    /// the filter was applied.
    fn log_history_mode_refs_page_streaming(
        &self,
        _mode: HistoryMode,
        _refs: &[String],
        _author: Option<&str>,
        _limit: usize,
        _cursor: Option<&LogCursor>,
        _cancellation: &CancellationToken,
        _on_chunk: &mut dyn FnMut(LogChunk),
    ) -> Result<LogPage> {
        Err(Error::new(ErrorKind::Unsupported(
            "ref-filtered history is not implemented for this backend",
        )))
    }

    fn log_head_page(&self, limit: usize, cursor: Option<&LogCursor>) -> Result<LogPage>;

    fn log_head_page_cancellable(
        &self,
        limit: usize,
        cursor: Option<&LogCursor>,
        cancellation: &CancellationToken,
    ) -> Result<LogPage> {
        cancellation.check_cancelled()?;
        let page = self.log_head_page(limit, cursor)?;
        cancellation.check_cancelled()?;
        Ok(page)
    }

    fn log_all_branches_page(&self, _limit: usize, _cursor: Option<&LogCursor>) -> Result<LogPage> {
        Err(Error::new(ErrorKind::Unsupported(
            "all-branches history is not implemented for this backend",
        )))
    }

    fn log_all_branches_page_cancellable(
        &self,
        limit: usize,
        cursor: Option<&LogCursor>,
        cancellation: &CancellationToken,
    ) -> Result<LogPage> {
        cancellation.check_cancelled()?;
        let page = self.log_all_branches_page(limit, cursor)?;
        cancellation.check_cancelled()?;
        Ok(page)
    }

    fn log_file_page(
        &self,
        _path: &Path,
        _limit: usize,
        _cursor: Option<&LogCursor>,
    ) -> Result<LogPage> {
        Err(Error::new(ErrorKind::Unsupported(
            "file history is not implemented for this backend",
        )))
    }

    /// Author name → email for recent commits across all refs, gathered in
    /// one pass so UI surfaces that only carry the author name (history
    /// rows, hover cards) can still address per-email avatars
    /// (Gravatar-style hashing).
    ///
    /// The map covers a bounded, most-recent slice of history; names absent
    /// from it simply keep their initials avatar. First identity seen per
    /// name wins — history is walked newest-first, so that is the identity
    /// the user is looking at. The default reports no emails; backends that
    /// can gather them override this.
    fn author_email_map(&self) -> Result<FxHashMap<String, String>> {
        Ok(FxHashMap::default())
    }

    /// Every commit no older than `since`, across local branches and remotes,
    /// as bare (author, time) pairs for the statistics window. The backend
    /// bounds the walk itself; callers still re-filter by the exact cutoff.
    /// The default reports an empty window; backends that can gather them
    /// override this.
    fn contributor_commits_since(&self, since: SystemTime) -> Result<Vec<ContributorCommit>> {
        let _ = since;
        Ok(Vec::new())
    }

    fn commit_details(&self, id: &CommitId) -> Result<CommitDetails>;

    /// Files that differ between two points (`from` → `to`), for the
    /// compare-selected-commits feature. `from` is the base/older side, so the
    /// result reads as "what `to` adds/removes relative to `from`". `to = None`
    /// compares `from` against the live working tree. Branch and tag comparisons
    /// resolve their tips to commit ids before calling this.
    fn diff_range_files(
        &self,
        _from: &CommitId,
        _to: Option<&CommitId>,
    ) -> Result<Vec<CommitFileChange>> {
        Err(Error::new(ErrorKind::Unsupported(
            "range file listing is not implemented for this backend",
        )))
    }

    /// Full `%B` messages of the given commits, in input order. Message-only
    /// on purpose: callers like the cherry-pick editor need nothing else, and
    /// implementations should skip the per-commit tree diff `commit_details`
    /// pays for.
    fn commit_messages(&self, ids: &[CommitId]) -> Result<Vec<String>> {
        ids.iter()
            .map(|id| self.commit_details(id).map(|details| details.message))
            .collect()
    }

    /// Stable topological ordering of an arbitrary set of commits. Selected
    /// ancestors precede selected descendants even when unselected commits
    /// lie between them; unrelated commits retain their input order.
    fn topologically_order_commits(&self, _ids: &[CommitId]) -> Result<Vec<CommitId>> {
        Err(Error::new(ErrorKind::Unsupported(
            "topological commit ordering is not implemented for this backend",
        )))
    }

    fn recent_commit_messages(&self, _limit: usize) -> Result<Vec<RecentCommitMessage>> {
        Err(Error::new(ErrorKind::Unsupported(
            "recent commit messages are not implemented for this backend",
        )))
    }

    /// Cross-history commit search over message and author fields (both
    /// passes case-insensitive), newest-first, deduplicated and capped at
    /// `limit`. Backends that can't run it report `Unsupported`.
    fn search_commits(&self, _query: &str, _limit: usize) -> Result<Vec<Commit>> {
        Err(Error::new(ErrorKind::Unsupported(
            "commit search is not implemented for this backend",
        )))
    }

    fn reflog_head(&self, limit: usize) -> Result<Vec<ReflogEntry>>;

    /// Resolve the path of the file currently known as `path` (at some revision)
    /// to the name it has in `commit`'s tree, following renames. Returns `None`
    /// when it cannot be determined (the caller should then fall back to `path`).
    ///
    /// This lets "view file at this commit" navigate across renames: a file's
    /// name in an older/newer commit may differ from the path the caller holds,
    /// and opening the wrong name would fail because it is absent from that tree.
    fn resolve_file_path_at_commit(
        &self,
        _path: &Path,
        _commit: &CommitId,
    ) -> Result<Option<PathBuf>> {
        Ok(None)
    }
}
