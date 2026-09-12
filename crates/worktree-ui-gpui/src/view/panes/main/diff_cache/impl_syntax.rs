//! `MainPaneView` prepared syntax documents and their cache keys.

use super::super::helpers::PreparedSyntaxDocumentKey;
use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
use super::helpers::{
    FULL_DOCUMENT_SYNTAX_MODE, FileDiffBackgroundPreparedSyntaxDocuments,
    FileDiffPreparedSyntaxApplyResult, PREPARED_SYNTAX_DOCUMENT_CACHE_MAX_ENTRIES,
    SyncFileDiffPreparedSyntaxApplyResult, prepared_syntax_document_key,
};
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use crate::view::rows;

// @split-module: impl_syntax
impl MainPaneView {
    fn prepared_syntax_document(
        &self,
        key: &PreparedSyntaxDocumentKey,
    ) -> Option<rows::PreparedDiffSyntaxDocument> {
        self.prepared_syntax_documents.get(key).copied()
    }

    fn prepared_syntax_reparse_seed_document(
        &self,
        key: &PreparedSyntaxDocumentKey,
    ) -> Option<rows::PreparedDiffSyntaxDocument> {
        self.prepared_syntax_documents
            .iter()
            .filter(|(candidate_key, _)| {
                candidate_key.repo_id == key.repo_id
                    && candidate_key.file_path == key.file_path
                    && candidate_key.view_mode == key.view_mode
                    && candidate_key.target_rev != key.target_rev
            })
            .max_by_key(|(candidate_key, _)| candidate_key.target_rev)
            .map(|(_, document)| *document)
    }

    fn insert_prepared_syntax_document(
        &mut self,
        key: PreparedSyntaxDocumentKey,
        document: rows::PreparedDiffSyntaxDocument,
    ) -> bool {
        if self.prepared_syntax_documents.contains_key(&key) {
            return false;
        }
        if self.prepared_syntax_documents.len() >= PREPARED_SYNTAX_DOCUMENT_CACHE_MAX_ENTRIES
            && let Some(evict_key) = self.prepared_syntax_documents.keys().next().cloned()
        {
            self.prepared_syntax_documents.remove(&evict_key);
        }
        self.prepared_syntax_documents.insert(key, document);
        true
    }

    fn rekey_prepared_syntax_document(
        &mut self,
        old_key: PreparedSyntaxDocumentKey,
        new_key: PreparedSyntaxDocumentKey,
    ) {
        if old_key == new_key {
            return;
        }
        let Some(document) = self.prepared_syntax_documents.remove(&old_key) else {
            return;
        };
        self.prepared_syntax_documents
            .entry(new_key)
            .or_insert(document);
    }

    pub(super) fn rekey_file_diff_prepared_syntax_documents_for_rev(&mut self, new_rev: u64) {
        let Some(repo_id) = self.file_diff_cache_repo_id else {
            return;
        };
        let Some(path) = self.file_diff_cache_path.clone() else {
            return;
        };
        let old_rev = self.file_diff_cache_rev;
        if old_rev == new_rev {
            return;
        }

        for view_mode in [
            PreparedSyntaxViewMode::FileDiffSplitLeft,
            PreparedSyntaxViewMode::FileDiffSplitRight,
        ] {
            let old_key = prepared_syntax_document_key(repo_id, old_rev, &path, view_mode);
            let new_key = prepared_syntax_document_key(repo_id, new_rev, &path, view_mode);
            self.rekey_prepared_syntax_document(old_key, new_key);
        }
    }

    pub(in crate::view::panes::main) fn full_document_syntax_budget(
        &self,
    ) -> rows::DiffSyntaxBudget {
        #[cfg(test)]
        if let Some(budget) = self.diff_syntax_budget_override {
            return budget;
        }

        rows::DiffSyntaxBudget::default()
    }

    #[cfg(test)]
    pub(in crate::view) fn set_full_document_syntax_budget_override_for_tests(
        &mut self,
        budget: rows::DiffSyntaxBudget,
    ) {
        self.diff_syntax_budget_override = Some(budget);
    }

    pub(in crate::view) fn file_diff_prepared_syntax_key(
        &self,
        view_mode: PreparedSyntaxViewMode,
    ) -> Option<PreparedSyntaxDocumentKey> {
        let repo_id = self.file_diff_cache_repo_id?;
        let path = self.file_diff_cache_path.as_ref()?;
        Some(prepared_syntax_document_key(
            repo_id,
            self.file_diff_cache_rev,
            path,
            view_mode,
        ))
    }

    pub(super) fn file_diff_prepared_syntax_document(
        &self,
        view_mode: PreparedSyntaxViewMode,
    ) -> Option<rows::PreparedDiffSyntaxDocument> {
        let key = self.file_diff_prepared_syntax_key(view_mode)?;
        self.prepared_syntax_document(&key)
    }

    pub(in crate::view) fn worktree_preview_prepared_syntax_key(
        &self,
    ) -> Option<PreparedSyntaxDocumentKey> {
        let repo_id = self.active_repo_id()?;
        let path = self.worktree_preview_path.as_ref()?;
        Some(prepared_syntax_document_key(
            repo_id,
            self.worktree_preview_content_rev,
            path,
            PreparedSyntaxViewMode::WorktreePreview,
        ))
    }

    pub(in crate::view) fn worktree_preview_prepared_syntax_document(
        &self,
    ) -> Option<rows::PreparedDiffSyntaxDocument> {
        let key = self.worktree_preview_prepared_syntax_key()?;
        self.prepared_syntax_document(&key)
    }

    pub(in crate::view) fn refresh_worktree_preview_syntax_document(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(language) = self.worktree_preview_syntax_language else {
            return;
        };
        let Some(key) = self.worktree_preview_prepared_syntax_key() else {
            return;
        };
        if !matches!(self.worktree_preview, Loadable::Ready(_)) {
            return;
        }
        if self.worktree_preview_text.is_empty() {
            return;
        }
        let source_text = self.worktree_preview_text.clone();
        let line_starts = Arc::clone(&self.worktree_preview_line_starts);

        if self.prepared_syntax_document(&key).is_some() {
            return;
        }
        let reparse_seed = self.prepared_syntax_reparse_seed_document(&key);
        let background_reparse_seed: Option<rows::PreparedDiffSyntaxReparseSeed> =
            reparse_seed.and_then(rows::prepared_diff_syntax_reparse_seed);

        let budget = self.full_document_syntax_budget();
        match rows::prepare_diff_syntax_document_with_budget_reuse_text(
            language,
            FULL_DOCUMENT_SYNTAX_MODE,
            source_text.clone(),
            Arc::clone(&line_starts),
            budget,
            reparse_seed,
            None,
        ) {
            rows::PrepareDiffSyntaxDocumentResult::Ready(document) => {
                self.insert_prepared_syntax_document(key, document);
            }
            rows::PrepareDiffSyntaxDocumentResult::TimedOut => {
                cx.spawn(
                    async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                        let prepare_document = move || {
                            rows::prepare_diff_syntax_document_in_background_text_with_reuse(
                                language,
                                FULL_DOCUMENT_SYNTAX_MODE,
                                source_text,
                                line_starts,
                                background_reparse_seed,
                                None,
                            )
                        };
                        let parsed_document =
                            if crate::ui_runtime::current().uses_background_compute() {
                                smol::unblock(prepare_document).await
                            } else {
                                prepare_document()
                            };

                        let _ = view.update(cx, |this, cx| {
                            let Some(parsed_document) = parsed_document else {
                                return;
                            };

                            let inserted = this.insert_prepared_syntax_document(
                                key.clone(),
                                rows::inject_background_prepared_diff_syntax_document(
                                    parsed_document,
                                ),
                            );
                            if inserted
                                && this.worktree_preview_prepared_syntax_key().as_ref()
                                    == Some(&key)
                            {
                                this.worktree_preview_style_cache_epoch =
                                    this.worktree_preview_style_cache_epoch.wrapping_add(1);
                                cx.notify();
                            }
                        });
                    },
                )
                .detach();
            }
            rows::PrepareDiffSyntaxDocumentResult::Unsupported => {}
        }
    }

    /// Applies a foreground sync prepare result for one side. Returns `true` if
    /// the side needs a background async parse instead.
    fn apply_sync_syntax_result(
        &mut self,
        attempt: Option<rows::PrepareDiffSyntaxDocumentResult>,
        key: &Option<PreparedSyntaxDocumentKey>,
    ) -> SyncFileDiffPreparedSyntaxApplyResult {
        match attempt {
            Some(rows::PrepareDiffSyntaxDocumentResult::Ready(document)) => {
                SyncFileDiffPreparedSyntaxApplyResult {
                    inserted: key.as_ref().is_some_and(|key| {
                        self.insert_prepared_syntax_document(key.clone(), document)
                    }),
                    needs_background_prepare: false,
                }
            }
            Some(rows::PrepareDiffSyntaxDocumentResult::TimedOut) => {
                SyncFileDiffPreparedSyntaxApplyResult {
                    inserted: false,
                    needs_background_prepare: true,
                }
            }
            _ => SyncFileDiffPreparedSyntaxApplyResult::default(),
        }
    }

    /// Applies background-parsed documents for both sides and reports which
    /// side became newly cacheable.
    fn apply_background_syntax_documents(
        &mut self,
        left_key: &Option<PreparedSyntaxDocumentKey>,
        left_doc: Option<rows::BackgroundPreparedDiffSyntaxDocument>,
        right_key: &Option<PreparedSyntaxDocumentKey>,
        right_doc: Option<rows::BackgroundPreparedDiffSyntaxDocument>,
    ) -> FileDiffPreparedSyntaxApplyResult {
        let mut applied = FileDiffPreparedSyntaxApplyResult::default();
        if let (Some(key), Some(document)) = (left_key.as_ref(), left_doc) {
            applied.split_left = self.insert_prepared_syntax_document(
                key.clone(),
                rows::inject_background_prepared_diff_syntax_document(document),
            );
        }
        if let (Some(key), Some(document)) = (right_key.as_ref(), right_doc) {
            applied.split_right = self.insert_prepared_syntax_document(
                key.clone(),
                rows::inject_background_prepared_diff_syntax_document(document),
            );
        }
        applied
    }

    pub(super) fn refresh_file_diff_syntax_documents(
        &mut self,
        cx: &mut gpui::Context<Self>,
        split_left_reparse_seed_override: Option<rows::PreparedDiffSyntaxDocument>,
        split_right_reparse_seed_override: Option<rows::PreparedDiffSyntaxDocument>,
        split_left_edit_hint: Option<rows::DiffSyntaxEdit>,
        split_right_edit_hint: Option<rows::DiffSyntaxEdit>,
    ) {
        if self.file_diff_old_text.is_empty() && self.file_diff_new_text.is_empty() {
            return;
        }

        let Some(language) = self.file_diff_cache_language else {
            return;
        };

        // Split and inline syntax both project from the real old/new documents.
        // Only those real side documents are parsed here; inline rows later map
        // through old_line/new_line instead of parsing any synthetic diff stream.
        let split_left_key =
            self.file_diff_prepared_syntax_key(PreparedSyntaxViewMode::FileDiffSplitLeft);
        let split_right_key =
            self.file_diff_prepared_syntax_key(PreparedSyntaxViewMode::FileDiffSplitRight);
        let split_left_reparse_seed = split_left_reparse_seed_override.or_else(|| {
            split_left_key
                .as_ref()
                .and_then(|key| self.prepared_syntax_reparse_seed_document(key))
        });
        let split_right_reparse_seed = split_right_reparse_seed_override.or_else(|| {
            split_right_key
                .as_ref()
                .and_then(|key| self.prepared_syntax_reparse_seed_document(key))
        });

        let needs_split_left_prepare = split_left_key
            .as_ref()
            .is_some_and(|key| self.prepared_syntax_document(key).is_none());
        let needs_split_right_prepare = split_right_key
            .as_ref()
            .is_some_and(|key| self.prepared_syntax_document(key).is_none());
        if !needs_split_left_prepare && !needs_split_right_prepare {
            return;
        }

        let budget = self.full_document_syntax_budget();

        let split_left_attempt = needs_split_left_prepare.then(|| {
            rows::prepare_diff_syntax_document_with_budget_reuse_text(
                language,
                FULL_DOCUMENT_SYNTAX_MODE,
                self.file_diff_old_text.clone(),
                Arc::clone(&self.file_diff_old_line_starts),
                budget,
                split_left_reparse_seed,
                split_left_edit_hint.clone(),
            )
        });
        let split_right_attempt = needs_split_right_prepare.then(|| {
            rows::prepare_diff_syntax_document_with_budget_reuse_text(
                language,
                FULL_DOCUMENT_SYNTAX_MODE,
                self.file_diff_new_text.clone(),
                Arc::clone(&self.file_diff_new_line_starts),
                budget,
                split_right_reparse_seed,
                split_right_edit_hint.clone(),
            )
        });

        let split_left_sync = self.apply_sync_syntax_result(split_left_attempt, &split_left_key);
        let split_right_sync = self.apply_sync_syntax_result(split_right_attempt, &split_right_key);
        let needs_split_left_async = split_left_sync.needs_background_prepare;
        let needs_split_right_async = split_right_sync.needs_background_prepare;

        if split_left_sync.inserted {
            self.file_diff_style_cache_epochs.bump_left();
        }
        if split_right_sync.inserted {
            self.file_diff_style_cache_epochs.bump_right();
        }
        if split_left_sync.inserted || split_right_sync.inserted {
            cx.notify();
        }

        if !needs_split_left_async && !needs_split_right_async {
            return;
        }

        let syntax_generation = self.file_diff_syntax_generation;
        let repo_id = self.file_diff_cache_repo_id;
        let diff_file_rev = self.file_diff_cache_rev;
        let diff_target = self.file_diff_cache_target.clone();

        let split_left_source = needs_split_left_async.then(|| {
            (
                self.file_diff_old_text.clone(),
                Arc::clone(&self.file_diff_old_line_starts),
            )
        });
        let split_left_background_reparse_seed = split_left_reparse_seed
            .filter(|_| needs_split_left_async)
            .and_then(rows::prepared_diff_syntax_reparse_seed);
        let split_left_edit_hint = split_left_edit_hint.filter(|_| needs_split_left_async);
        let split_right_source = needs_split_right_async.then(|| {
            (
                self.file_diff_new_text.clone(),
                Arc::clone(&self.file_diff_new_line_starts),
            )
        });
        let split_right_background_reparse_seed = split_right_reparse_seed
            .filter(|_| needs_split_right_async)
            .and_then(rows::prepared_diff_syntax_reparse_seed);
        let split_right_edit_hint = split_right_edit_hint.filter(|_| needs_split_right_async);

        cx.spawn(
            async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                let prepare_documents = move || FileDiffBackgroundPreparedSyntaxDocuments {
                    split_left: split_left_source.and_then(|(text, line_starts)| {
                        rows::prepare_diff_syntax_document_in_background_text_with_reuse(
                            language,
                            FULL_DOCUMENT_SYNTAX_MODE,
                            text,
                            line_starts,
                            split_left_background_reparse_seed,
                            split_left_edit_hint,
                        )
                    }),
                    split_right: split_right_source.and_then(|(text, line_starts)| {
                        rows::prepare_diff_syntax_document_in_background_text_with_reuse(
                            language,
                            FULL_DOCUMENT_SYNTAX_MODE,
                            text,
                            line_starts,
                            split_right_background_reparse_seed,
                            split_right_edit_hint,
                        )
                    }),
                };
                let parsed_documents = if crate::ui_runtime::current().uses_background_compute() {
                    smol::unblock(prepare_documents).await
                } else {
                    prepare_documents()
                };

                let _ = view.update(cx, |this, cx| {
                    if this.file_diff_syntax_generation != syntax_generation {
                        return;
                    }
                    if this.file_diff_cache_repo_id != repo_id
                        || this.file_diff_cache_rev != diff_file_rev
                        || this.file_diff_cache_target != diff_target
                    {
                        return;
                    }

                    let applied = this.apply_background_syntax_documents(
                        &split_left_key,
                        parsed_documents.split_left,
                        &split_right_key,
                        parsed_documents.split_right,
                    );

                    if applied.any() {
                        if applied.split_left {
                            this.file_diff_style_cache_epochs.bump_left();
                        }
                        if applied.split_right {
                            this.file_diff_style_cache_epochs.bump_right();
                        }
                        cx.notify();
                    }
                });
            },
        )
        .detach();
    }
}
