//! `MainPaneView` context the row builders read: coverage, blame and the
//! per-query text-segment cache.

use super::super::*;

// @split-module: impl_context
impl MainPaneView {
    /// Test-only reader for the blame lines behind that context.
    ///
    /// `blame_render_ctx` needs `&mut self` to memoize its time range, which a
    /// `read`-borrowed pane cannot give it.
    #[cfg(test)]
    pub(in crate::view) fn blame_render_ctx_for_test(
        &self,
    ) -> Option<&std::sync::Arc<Vec<worktree_core::services::BlameLine>>> {
        if !self.annotation_active() || !self.blame_matches_rendered_target() {
            return None;
        }
        match &self.active_repo()?.history_state.blame {
            worktree_state::model::Loadable::Ready(lines) => Some(lines),
            _ => None,
        }
    }

    /// Build a blame render context when annotate is enabled and blame for the
    /// current target is loaded; otherwise `None`.
    ///
    /// While the same target reloads, falls back to the annotations retained by
    /// the store (`retained_blame_while_loading`) so the column keeps its
    /// contents instead of blanking on every refresh. The retained value is
    /// dropped when blame re-targets, so it always describes `blame_path`.
    pub(in crate::view) fn blame_render_ctx(&mut self) -> Option<BlameRenderCtx> {
        if !self.annotation_active() || !self.blame_matches_rendered_target() {
            return None;
        }
        let repo = self.active_repo()?;
        let lines = match &repo.history_state.blame {
            worktree_state::model::Loadable::Ready(lines) => lines,
            worktree_state::model::Loadable::NotLoaded
            | worktree_state::model::Loadable::Loading => {
                repo.history_state.retained_blame_while_loading.as_ref()?
            }
            worktree_state::model::Loadable::Error(_) => return None,
        };
        let path: std::sync::Arc<std::path::Path> =
            std::sync::Arc::from(repo.history_state.blame_path.as_deref()?);
        // When blaming a specific commit, that commit is the one currently being
        // viewed; "view file at this commit" on its own lines would be a no-op.
        let viewed_commit = match &repo.history_state.blame_source {
            Some(worktree_core::domain::BlameSource::Revision(Some(rev))) => {
                Some(std::sync::Arc::<str>::from(rev.as_str()))
            }
            _ => None,
        };
        // The blamed working-tree area, used to classify uncommitted lines as
        // staged vs unstaged. `None` for revision blame (no such distinction).
        let area = match &repo.history_state.blame_source {
            Some(worktree_core::domain::BlameSource::WorkingTree(area)) => Some(*area),
            _ => None,
        };
        let lines = std::sync::Arc::clone(lines);
        // The time range never changes for a given loaded blame, so memoize it by
        // the blame Arc's identity instead of rescanning every frame. Compare by
        // `ptr_eq` against a held Arc clone: keeping the cached allocation alive
        // means a reloaded blame can't reuse the same address and alias a stale
        // range (an ABA hazard a bare pointer key would have).
        let range = match &self.blame_time_range_cache {
            Some((cached, range)) if std::sync::Arc::ptr_eq(cached, &lines) => *range,
            _ => {
                let range = crate::view::rows::blame::blame_time_range(&lines);
                self.blame_time_range_cache = Some((std::sync::Arc::clone(&lines), range));
                range
            }
        };
        Some(BlameRenderCtx {
            lines,
            range,
            now: std::time::SystemTime::now(),
            path,
            viewed_commit,
            area,
        })
    }

    pub(super) fn diff_text_segments_cache_get_for_query(
        &mut self,
        key: usize,
        query: &str,
        options: DiffSearchOptions,
        syntax_epoch: u64,
    ) -> Option<&CachedDiffStyledText> {
        if query.is_empty() {
            return self.diff_text_segments_cache_get(key, syntax_epoch);
        }

        self.sync_diff_text_query_overlay_cache(query, options);
        let query_generation = self.diff_text_query_cache_generation;
        if self.diff_text_query_segments_cache.len() <= key {
            self.diff_text_query_segments_cache
                .resize_with(key + 1, || None);
        }

        if versioned_query_cached_diff_styled_text_is_current(
            self.diff_text_query_segments_cache
                .get(key)
                .and_then(Option::as_ref),
            syntax_epoch,
            query_generation,
        )
        .is_none()
        {
            let base = self
                .diff_text_segments_cache_get(key, syntax_epoch)?
                .clone();
            // The diff view marks its current match by selecting the row, so
            // every match here wears the same wash.
            let overlaid = build_cached_diff_query_overlay_styled_text(
                self.theme,
                &base,
                self.diff_text_query_cache_matcher.as_ref()?,
                DiffSearchMatchEmphasis::Other,
            );
            self.diff_text_query_segments_cache[key] = Some(VersionedCachedDiffStyledText {
                syntax_epoch,
                query_generation,
                styled: overlaid,
            });
        }

        versioned_query_cached_diff_styled_text_is_current(
            self.diff_text_query_segments_cache
                .get(key)
                .and_then(Option::as_ref),
            syntax_epoch,
            query_generation,
        )
    }

    /// The coverage overlay inputs for the file on screen: the imported
    /// report plus the diff target's concrete path. `None` when no report
    /// is imported or the target spans a whole commit with no single file
    /// selected — there is nothing honest to annotate then.
    pub(super) fn diff_coverage_context(
        &self,
    ) -> Option<(
        std::sync::Arc<worktree_core::coverage::CoverageReport>,
        String,
    )> {
        let repo = self.active_repo()?;
        let report = repo.coverage.clone()?;
        let path = match repo.diff_state.diff_target.as_ref()? {
            DiffTarget::WorkingTree { path, .. } => path,
            DiffTarget::Commit {
                path: Some(path), ..
            } => path,
            DiffTarget::CommitRange {
                path: Some(path), ..
            } => path,
            _ => return None,
        };
        let normalized = worktree_core::coverage::normalize_coverage_path(&path.to_string_lossy());
        (!normalized.is_empty()).then_some((report, normalized))
    }
}
