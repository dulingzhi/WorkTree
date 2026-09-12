//! `MainPaneView` worktree preview cache state.

use super::super::helpers::{indexed_line_count_from_len, preview_line_flags_from_source};
use super::super::*;
#[cfg(any(test, feature = "benchmarks"))]
#[allow(unused_imports)]
pub(in crate::view) use super::file_diff::build_file_diff_cache_rebuild;
#[cfg(feature = "benchmarks")]
pub(in crate::view) use super::image_cache::render_svg_image_diff_preview;
use crate::view::rows;

// @split-module: impl_preview
impl MainPaneView {
    fn apply_worktree_preview_ready_state(
        &mut self,
        display_path: std::path::PathBuf,
        source_path: std::path::PathBuf,
        source_len: usize,
        source_text: SharedString,
        line_starts: Arc<[usize]>,
        line_flags: Arc<[u8]>,
        cx: &mut gpui::Context<Self>,
    ) {
        let line_count = indexed_line_count_from_len(source_len, line_starts.as_ref());
        let source_changed = self.worktree_preview_path.as_ref() != Some(&display_path)
            || self.worktree_preview_source_path.as_ref() != Some(&source_path)
            || self.worktree_preview_line_count() != Some(line_count)
            || self.worktree_preview_source_len != source_len
            || self.worktree_preview_text.as_ref() != source_text.as_ref()
            || self.worktree_preview_line_starts.as_ref() != line_starts.as_ref()
            || self.worktree_preview_line_flags.as_ref() != line_flags.as_ref();
        let cache_binding_changed =
            self.worktree_preview_segments_cache_path.as_ref() != Some(&display_path);
        let same_path_source_refresh = source_changed && !cache_binding_changed;

        self.worktree_preview_path = Some(display_path.clone());
        self.worktree_preview_source_path = Some(source_path);
        self.worktree_preview = Loadable::Ready(line_count);
        self.worktree_preview_source_len = source_len;
        self.worktree_preview_text = source_text;
        self.worktree_preview_line_starts = line_starts;
        self.worktree_preview_line_flags = line_flags;
        self.worktree_preview_search_trigram_index = None;
        self.worktree_preview_syntax_language = rows::diff_syntax_language_for_path(&display_path);
        self.worktree_preview_segments_cache_path = Some(display_path);
        self.worktree_preview_cache_write_blocked_until_rev = None;
        if source_changed || cache_binding_changed {
            self.worktree_preview_segments_cache.clear();
        }

        if source_changed {
            self.worktree_preview_content_rev = self.worktree_preview_content_rev.wrapping_add(1);
            self.worktree_preview_style_cache_epoch =
                self.worktree_preview_style_cache_epoch.wrapping_add(1);
            self.worktree_markdown_preview_path = None;
            self.worktree_markdown_preview_source_rev = 0;
            self.worktree_markdown_preview = Loadable::NotLoaded;
            self.worktree_markdown_preview_inflight = None;
        }

        if same_path_source_refresh {
            let blocked_rev = self.worktree_preview_content_rev;
            self.worktree_preview_cache_write_blocked_until_rev = Some(blocked_rev);
            if !crate::ui_runtime::current().uses_background_compute() {
                if self.worktree_preview_cache_write_blocked_until_rev == Some(blocked_rev) {
                    self.worktree_preview_cache_write_blocked_until_rev = None;
                }
            } else {
                cx.spawn(
                    async move |view: WeakEntity<MainPaneView>, cx: &mut gpui::AsyncApp| {
                        smol::Timer::after(std::time::Duration::from_millis(1)).await;
                        let _ = view.update(cx, |this, _cx| {
                            if this.worktree_preview_cache_write_blocked_until_rev
                                == Some(blocked_rev)
                            {
                                this.worktree_preview_cache_write_blocked_until_rev = None;
                            }
                        });
                    },
                )
                .detach();
            }
        }

        self.refresh_worktree_preview_syntax_document(cx);
    }

    pub(in crate::view) fn set_worktree_preview_ready_source(
        &mut self,
        path: std::path::PathBuf,
        source_text: SharedString,
        line_starts: Arc<[usize]>,
        cx: &mut gpui::Context<Self>,
    ) {
        let line_flags = preview_line_flags_from_source(source_text.as_ref(), line_starts.as_ref());
        self.apply_worktree_preview_ready_state(
            path.clone(),
            path,
            source_text.len(),
            source_text,
            line_starts,
            line_flags,
            cx,
        );
    }

    pub(in crate::view) fn set_worktree_preview_ready_materialized_source(
        &mut self,
        display_path: std::path::PathBuf,
        source_path: std::path::PathBuf,
        source_text: SharedString,
        line_starts: Arc<[usize]>,
        line_flags: Arc<[u8]>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.apply_worktree_preview_ready_state(
            display_path,
            source_path,
            source_text.len(),
            source_text,
            line_starts,
            line_flags,
            cx,
        );
    }

    pub(in crate::view) fn set_worktree_preview_ready_indexed_source(
        &mut self,
        display_path: std::path::PathBuf,
        source_path: std::path::PathBuf,
        source_len: usize,
        line_starts: Arc<[usize]>,
        line_flags: Arc<[u8]>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.apply_worktree_preview_ready_state(
            display_path,
            source_path,
            source_len,
            SharedString::default(),
            line_starts,
            line_flags,
            cx,
        );
    }

    pub(in crate::view) fn set_worktree_preview_ready_rows(
        &mut self,
        path: std::path::PathBuf,
        lines: &[String],
        source_len: usize,
        cx: &mut gpui::Context<Self>,
    ) {
        let (source_text, line_starts) =
            preview_source_text_and_line_starts_from_lines(lines, source_len);
        self.set_worktree_preview_ready_source(path, source_text, line_starts, cx);
    }
}
