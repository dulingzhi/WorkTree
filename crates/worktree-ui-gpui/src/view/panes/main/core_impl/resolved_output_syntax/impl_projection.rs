//! `MainPaneView` output projection: streaming, materialization and the edit deltas.

use super::outline::compute_resolved_outline_computation_from_projection;

use crate::kit::text_model::TextModelSnapshot;
use crate::view::conflict_resolver;
use crate::view::panes::main::helpers::ResolvedOutputKey;
use crate::view::panes::main::helpers::ResolvedOutputSourceRevision;
use crate::view::panes::main::helpers::UnresolvedRows;
use crate::view::panes::main::helpers::resolved_output_unresolved_rows;
use crate::view::panes::main::helpers::should_skip_resolved_outline_provenance;
use crate::view::panes::main::state::MainPaneView;
use crate::view::rows;
use gpui::SharedString;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;
// @split-module: impl_projection
impl MainPaneView {
    pub(in crate::view) fn conflict_resolved_output_is_streamed(&self) -> bool {
        self.conflict_resolved_output_projection.is_some()
    }

    pub(in crate::view) fn rebuild_conflict_resolved_output_block_map(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolver.output_is_protected {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
            return;
        }
        let map = conflict_resolver::ResolvedOutputBlockMap::from_segments(
            &self.conflict_resolver.marker_segments,
        );
        if self.conflict_resolved_output_is_streamed()
            || self.conflict_resolver_input.read_with(cx, |input, _| {
                map.is_valid_for(&self.conflict_resolver.marker_segments, input.text())
            })
        {
            self.conflict_resolved_output_block_map = map;
        } else {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
        }
    }

    pub(in crate::view::panes::main) fn apply_conflict_resolved_output_edit_deltas(
        &mut self,
        edit_deltas: Vec<(Range<usize>, Range<usize>)>,
        output_text: &(impl conflict_resolver::ResolvedOutputSource + ?Sized),
    ) {
        if edit_deltas.is_empty() {
            return;
        }
        if !self
            .conflict_resolved_output_block_map
            .apply_edit_deltas(edit_deltas)
            || !self
                .conflict_resolved_output_block_map
                .is_valid_for(&self.conflict_resolver.marker_segments, output_text)
        {
            self.conflict_resolved_output_block_map =
                conflict_resolver::ResolvedOutputBlockMap::default();
        }
    }

    pub(in crate::view) fn conflict_resolved_output_is_modified(&self) -> bool {
        self.conflict_resolved_output_modified
    }

    pub(in crate::view) fn mark_conflict_resolved_output_saved(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.conflict_resolved_output_saved_snapshot =
            (!self.conflict_resolved_output_is_streamed()).then(|| {
                self.conflict_resolver_input
                    .read_with(cx, |input, _| input.text_snapshot())
            });
        self.conflict_resolved_output_modified = false;
    }

    fn sync_conflict_resolved_preview_projection(
        &mut self,
        projection: conflict_resolver::ResolvedOutputProjection,
        path: Option<&std::path::PathBuf>,
    ) {
        self.conflict_resolved_output_block_map =
            conflict_resolver::ResolvedOutputBlockMap::from_segments(
                &self.conflict_resolver.marker_segments,
            );
        self.conflict_resolved_output_projection = Some(projection.clone());
        self.conflict_resolved_preview_path = path.cloned();
        self.conflict_resolved_preview_source_revision = None;
        self.conflict_resolved_preview_text = TextModelSnapshot::default();
        self.conflict_resolved_preview_syntax_language =
            path.and_then(rows::diff_syntax_language_for_path);
        self.conflict_resolved_preview_line_count = projection.len();
        self.conflict_resolved_preview_line_starts = Arc::default();
        self.conflict_resolved_output_measure_row = projection.widest_line_ix();
        self.conflict_resolved_outline_stash = None;
        self.conflict_resolver.resolved_output_visible_dirty = true;
    }

    pub(in crate::view) fn refresh_streamed_resolved_output_preview_from_projection(
        &mut self,
        projection: conflict_resolver::ResolvedOutputProjection,
        path: Option<&std::path::PathBuf>,
    ) {
        let trace_started = Instant::now();
        let output_line_count = projection.len();
        let view_mode = self.conflict_resolver.view_mode;
        let computed = compute_resolved_outline_computation_from_projection(
            &projection,
            &self.conflict_resolver.marker_segments,
            view_mode,
            (!should_skip_resolved_outline_provenance(view_mode, output_line_count))
                .then(|| self.resolved_outline_source_view()),
        );
        self.sync_conflict_resolved_preview_projection(projection, path);
        self.apply_resolved_outline_computation(path, trace_started, computed);
    }

    pub(in crate::view) fn refresh_streamed_resolved_output_preview_from_markers(
        &mut self,
        path: Option<&std::path::PathBuf>,
    ) {
        let projection = conflict_resolver::ResolvedOutputProjection::from_segments(
            &self.conflict_resolver.marker_segments,
        );
        self.refresh_streamed_resolved_output_preview_from_projection(projection, path);
    }

    pub(in crate::view) fn ensure_conflict_resolved_output_materialized(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.conflict_resolved_output_is_streamed() {
            return;
        }

        // No size ceiling here. The output pane is editable by definition, and a
        // read-only fallback above some line count is a worse answer than a
        // slower one: the user opened a merge to resolve it. The buffer is
        // rope-backed and every hot path (syntax refresh, unresolved-row scan,
        // shaping) reads windows rather than the whole document, so cost scales
        // with the visible region plus the conflict count, not the file.
        let resolved =
            conflict_resolver::generate_resolved_text(&self.conflict_resolver.marker_segments);
        let path = self.conflict_resolver.path.clone();
        self.conflict_resolved_output_projection = None;
        self.conflict_resolved_preview_path = path.clone();
        self.fill_conflict_resolved_output_buffer(resolved, cx);
        self.conflict_resolved_preview_source_revision =
            Some(self.conflict_resolver_input.read_with(cx, |input, _| {
                ResolvedOutputSourceRevision::from_snapshot(&input.text_snapshot())
            }));
        self.rebuild_conflict_resolved_output_block_map(cx);
        self.recompute_conflict_resolved_outline_and_provenance(path.as_ref(), cx);
    }

    /// Load merged text into the resolved-output editor.
    ///
    /// Every path that fills this buffer goes through here, because filling it
    /// has one non-obvious obligation: `set_text` leaves the caret at
    /// end-of-document, and the pane opens scrolled to the top, so a caret
    /// parked at the far end sends the first arrow key (or undo) autoscrolling
    /// to the bottom of a file the user was reading from the top. Park it where
    /// the view actually is; the user has not placed a caret yet.
    pub(in crate::view) fn fill_conflict_resolved_output_buffer(
        &mut self,
        text: impl Into<SharedString>,
        cx: &mut gpui::Context<Self>,
    ) {
        let text = text.into();
        let line_ending = crate::kit::TextInput::detect_line_ending(text.as_ref());
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_line_ending(line_ending);
            input.set_text(text, cx);
            input.set_selected_range(0..0, false, cx);
        });
    }

    /// Configure the resolved-output `TextInput` for rendering as the editable
    /// output pane. This is called from the render path, so it must stay cheap
    /// and side-effect free: the merged text is materialized into the buffer at
    /// bootstrap (see [`ensure_conflict_resolved_output_materialized`]), not here.
    /// It only points the editor at its shared scroll handle so the line-number
    /// gutter and the column scroll-sync group stay coupled to it.
    pub(in crate::view) fn prepare_conflict_resolved_output_editor(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.sync_conflict_resolved_output_active_conflict_highlight(cx);
        let scroll = self.conflict_resolved_output_editor_scroll.clone();
        let theme = self.theme;
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_read_only(false, cx);
            input.set_vertical_scroll_handle(Some(scroll));
            // Lay the editor out at content width so its `overflow_scroll`
            // container carries a horizontal `max_offset` on the shared handle,
            // letting the resolved output scroll-sync with the source columns
            // on the horizontal axis too.
            input.set_content_width_layout(true);
        });
    }

    /// Unresolved rows for `snapshot`, from the cache when it is still current.
    ///
    /// A miss only happens when navigation runs before any refresh has scanned
    /// this revision; the scan is then done once and cached like any other.
    pub(super) fn conflict_resolved_output_unresolved_rows_for(
        &mut self,
        snapshot: &TextModelSnapshot,
    ) -> UnresolvedRows {
        let key = ResolvedOutputKey::new(
            snapshot,
            &self.conflict_resolver.marker_segments,
            &self.conflict_resolved_output_block_map,
        );
        if let Some((cached_for, rows)) = self.conflict_resolved_output_unresolved_rows.as_ref()
            && *cached_for == key
        {
            return Arc::clone(rows);
        }

        #[cfg(test)]
        {
            self.conflict_resolved_output_full_scans += 1;
        }
        let rows = resolved_output_unresolved_rows(
            &self.conflict_resolver.marker_segments,
            &snapshot.rope(),
            &self.conflict_resolved_output_block_map,
        );
        self.conflict_resolved_output_unresolved_rows = Some((key, Arc::clone(&rows)));
        rows
    }

    /// Rebuild the output highlights when conflict navigation lands on another
    /// conflict, so the yellow wash follows the selection.
    ///
    /// Every other refresh path hangs off the text or the tree, and navigation
    /// moves neither — it only reassigns `active_conflict`, from a dozen call
    /// sites on a state struct that cannot reach the input. Comparing against the
    /// conflict the installed provider was built for catches all of them in one
    /// place, and makes the common render (nothing moved) a single comparison.
    fn sync_conflict_resolved_output_active_conflict_highlight(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_highlighted_conflict
            == self.conflict_resolver.active_conflict
        {
            return;
        }
        self.conflict_resolved_output_highlighted_conflict = self.conflict_resolver.active_conflict;
        // Streamed output is drawn row by row from the projection, which reads
        // the active conflict as it renders; only the editable buffer carries
        // highlights that have to be reinstalled.
        if self.conflict_resolved_output_is_streamed() {
            return;
        }
        // With a tree in hand, rebinding the provider is the whole job:
        // navigation moves no text, so the tree, the placeholder mask and the
        // protected spans all still stand. Going through the full syntax refresh
        // would redo them on every jump — and on the tree-less arm it would
        // re-tokenize the entire document, which is exactly the kind of
        // per-keypress cost the live engine exists to avoid.
        if self.conflict_resolved_output_live_syntax.is_some() {
            self.rebind_conflict_resolved_output_highlight_provider(cx);
            return;
        }
        let output_snapshot = self
            .conflict_resolver_input
            .read_with(cx, |input, _| input.text_snapshot());
        self.refresh_conflict_resolved_output_syntax(&output_snapshot, None, cx);
    }

    pub(in crate::view) fn current_conflict_resolved_output_text(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        if self.conflict_resolved_output_is_streamed() {
            conflict_resolver::generate_resolved_text(&self.conflict_resolver.marker_segments)
        } else {
            self.conflict_resolver_input
                .read_with(cx, |input, _| input.text().to_string())
        }
    }

    pub(in crate::view) fn conflict_resolver_save_contents_from_text(
        &mut self,
        text: String,
    ) -> String {
        self.conflict_resolver_sync_session_resolutions_from_output(&text);
        text
    }
}
