use super::helpers::{
    BlameTimeRangeCache, CachedUnresolvedRows, CollapsedDiffProjectionIdentity,
    CollapsedDiffReveal, DiffHorizontalScrollState, DiffTextAutoscrollTarget,
    FileDiffSplitWordHighlights, FileDiffStyleCacheEpochs, IRebaseViewState,
    PreparedSyntaxDocumentKey, ResolvedOutputSourceRevision, StashedResolvedOutlineState,
};
use super::*;
use crate::kit::text_model::TextModelSnapshot;

pub(crate) struct MainPaneView {
    pub(in crate::view) store: Arc<AppStore>,
    pub(super) state: Arc<AppState>,
    pub(in crate::view) view_mode: WorkTreeViewMode,
    pub(in crate::view) focused_mergetool_labels: Option<FocusedMergetoolLabels>,
    pub(in crate::view) focused_mergetool_exit_code: Option<Arc<AtomicI32>>,
    pub(in crate::view) theme: AppTheme,
    pub(in crate::view) date_time_format: DateTimeFormat,
    pub(super) _ui_model_subscription: gpui::Subscription,
    pub(in crate::view) root_view: WeakEntity<WorkTreeView>,
    pub(in crate::view) tooltip_host: WeakEntity<TooltipHost>,
    pub(super) notify_fingerprint: u64,
    pub(in crate::view) active_context_menu_invoker: Option<SharedString>,

    pub(in crate::view) last_window_size: Size<Pixels>,
    pub(in crate::view) layout_sidebar_render_width: Pixels,
    pub(in crate::view) layout_details_render_width: Pixels,
    pub(in crate::view) layout_sidebar_collapsed: bool,
    pub(in crate::view) layout_details_collapsed: bool,

    pub(in crate::view) reveal_whitespace_chars: bool,
    /// section 30 merge tool: auto-advance to the next unresolved conflict after a
    /// source pick. Persisted UI setting (cog menu).
    pub(in crate::view) mergetool_auto_advance: bool,
    /// section 30 merge tool: default for the collapse-unchanged-context mode when a
    /// conflicted file opens. Persisted UI setting (cog menu).
    pub(in crate::view) mergetool_collapse_unchanged: bool,
    /// section 30 merge tool: sync the resolved output pane's scroll with the source
    /// columns (in modes where they share a row space). Persisted UI setting
    /// (cog menu). Merge-tool-specific rather than a general diff setting
    /// because the resolver ships as a standalone tool.
    pub(in crate::view) mergetool_output_scroll_sync: bool,
    /// section 30 merge tool: show per-column and resolved-output line number
    /// gutters. Persisted UI setting (cog menu).
    pub(in crate::view) mergetool_show_line_numbers: bool,
    /// section 30 merge tool: last-used view mode (true = 3-way). Fresh opens of
    /// base-present conflicts default to this; toolbar toggle persists it.
    pub(in crate::view) mergetool_view_three_way: bool,
    pub(in crate::view) diff_view: DiffViewMode,
    pub(in crate::view) annotate_enabled: bool,
    /// Width (design px) of the annotate column; user-resizable, session-local.
    pub(in crate::view) annotate_column_width: f32,
    /// Active annotate-column resize drag, if any.
    pub(in crate::view) annotate_resize: Option<AnnotateResizeState>,
    /// Blame annotation sub-area currently hovered (row index + area). Drives the
    /// accent highlight and tooltip for the annotation column on the next paint.
    pub(in crate::view) blame_annot_hover: Option<(usize, crate::view::rows::AnnotArea)>,
    /// Diff row whose stage/unstage gutter button is currently hovered, as the
    /// row index plus which column's gutter it sits in. Drives painting the
    /// button and its tooltip on the next paint; `None` means none is showing.
    pub(in crate::view) diff_stage_gutter_hover: Option<crate::view::rows::DiffStageHover>,
    /// Painted bounds of each row's stage-gutter cell, recorded during paint so
    /// tests can drive the button without duplicating its geometry.
    pub(in crate::view) diff_stage_gutter_cells:
        FxHashMap<(usize, crate::view::rows::DiffStageSlot), gpui::Bounds<Pixels>>,
    /// Memoized `(min, max)` author-time range for the currently loaded blame,
    /// keyed by a clone of the blame `Arc`. The range never changes after load,
    /// so this avoids rescanning all blame lines on every render frame. Holding
    /// the `Arc` (rather than a bare pointer) keeps the allocation alive while
    /// cached, so a reloaded blame can never alias the same address and return a
    /// stale range.
    pub(in crate::view) blame_time_range_cache: BlameTimeRangeCache,
    pub(in crate::view) rendered_preview_modes: RenderedPreviewModes,
    pub(in crate::view) diff_word_wrap: bool,
    pub(in crate::view) diff_show_line_numbers: bool,
    pub(in crate::view) diff_scroll_sync: DiffScrollSync,
    pub(in crate::view) diff_content_mode: DiffContentMode,
    pub(in crate::view) diff_whitespace_mode: DiffWhitespaceMode,
    pub(in crate::view) diff_split_ratio: f32,
    pub(in crate::view) diff_split_resize: Option<DiffSplitResizeState>,
    pub(in crate::view) diff_split_last_synced_x: [Pixels; 2],
    pub(in crate::view) diff_split_last_synced_y: [Pixels; 2],
    pub(in crate::view) diff_horizontal_scroll: DiffHorizontalScrollState,
    pub(in crate::view) diff_cache_repo_id: Option<RepoId>,
    pub(in crate::view) diff_cache_rev: u64,
    pub(in crate::view) diff_cache_content_signature: Option<u64>,
    pub(in crate::view) diff_cache_target: Option<DiffTarget>,
    pub(in crate::view) diff_cache: Vec<AnnotatedDiffLine>,
    pub(in crate::view) diff_row_provider: Option<Arc<super::diff_cache::PagedPatchDiffRows>>,
    pub(in crate::view) diff_split_row_provider:
        Option<Arc<super::diff_cache::PagedPatchSplitRows>>,
    pub(in crate::view) diff_file_for_src_ix: Vec<Option<Arc<str>>>,
    pub(in crate::view) diff_language_for_src_ix: Vec<Option<rows::DiffSyntaxLanguage>>,
    pub(in crate::view) diff_yaml_block_scalar_for_src_ix: Vec<bool>,
    pub(in crate::view) diff_click_kinds: Vec<DiffClickKind>,
    pub(in crate::view) diff_line_kind_for_src_ix: Vec<worktree_core::domain::DiffLineKind>,
    pub(in crate::view) diff_visual_line_kind_for_src_ix: Vec<worktree_core::domain::DiffLineKind>,
    pub(in crate::view) diff_hide_unified_header_for_src_ix: Vec<bool>,
    pub(in crate::view) diff_header_display_cache: FxHashMap<usize, SharedString>,
    pub(in crate::view) diff_split_cache: Vec<PatchSplitRow>,
    pub(in crate::view) diff_split_cache_len: usize,
    pub(in crate::view) diff_panel_focus_handle: FocusHandle,
    pub(in crate::view) diff_autoscroll_pending: bool,
    pub(in crate::view) diff_raw_input: Entity<components::TextInput>,
    pub(in crate::view) submodule_hash_inputs: Vec<Entity<components::TextInput>>,
    pub(in crate::view) diff_visible_indices: Vec<usize>,
    pub(in crate::view) diff_visible_inline_map: Option<super::diff_cache::PatchInlineVisibleMap>,
    pub(in crate::view) diff_wrap_visible_rows: Vec<DiffWrapVisualRow>,
    pub(in crate::view) diff_wrap_visible_cache_key: Option<DiffWrapVisibleCacheKey>,
    pub(in crate::view) collapsed_diff_hunks: Vec<CollapsedDiffHunk>,
    pub(in crate::view) collapsed_diff_hunk_ix_by_src_ix: FxHashMap<usize, usize>,
    pub(in crate::view) collapsed_diff_reveals: FxHashMap<usize, CollapsedDiffReveal>,
    pub(in crate::view) collapsed_diff_visible_rows: Vec<CollapsedDiffVisibleRow>,
    pub(in crate::view) collapsed_diff_hunk_visible_indices: Vec<usize>,
    pub(in crate::view) collapsed_diff_header_display_cache: FxHashMap<usize, SharedString>,
    pub(in crate::view) collapsed_diff_projection_identity: Option<CollapsedDiffProjectionIdentity>,
    pub(in crate::view) diff_visible_cache_len: usize,
    pub(in crate::view) diff_visible_view: DiffViewMode,
    pub(in crate::view) diff_visible_is_file_view: bool,
    pub(in crate::view) diff_visible_projection_rev: u64,
    pub(in crate::view) diff_visible_cache_projection_rev: u64,
    pub(in crate::view) diff_scrollbar_markers_cache: Vec<components::ScrollbarMarker>,
    pub(in crate::view) diff_word_highlights: Vec<Option<Vec<Range<usize>>>>,
    pub(in crate::view) diff_word_highlights_inflight: Option<u64>,
    pub(in crate::view) diff_file_stats: Vec<Option<(usize, usize)>>,
    pub(in crate::view) diff_text_segments_cache: Vec<Option<VersionedCachedDiffStyledText>>,
    pub(in crate::view) diff_text_query_segments_cache: Vec<Option<VersionedCachedDiffStyledText>>,
    pub(in crate::view) diff_text_query_cache_query: SharedString,
    pub(in crate::view) diff_text_query_cache_options: super::diff_search::DiffSearchOptions,
    pub(in crate::view) diff_text_query_cache_matcher:
        Option<super::diff_search::DiffSearchMatcher>,
    pub(in crate::view) diff_text_query_cache_generation: u64,
    pub(in crate::view) diff_selection_anchor: Option<usize>,
    pub(in crate::view) diff_selection_range: Option<(usize, usize)>,
    pub(in crate::view) diff_text_selecting: bool,
    pub(in crate::view) diff_text_anchor: Option<DiffTextPos>,
    pub(in crate::view) diff_text_head: Option<DiffTextPos>,
    pub(super) diff_text_autoscroll_seq: u64,
    pub(super) diff_text_autoscroll_target: Option<DiffTextAutoscrollTarget>,
    pub(super) diff_text_last_mouse_pos: Point<Pixels>,
    pub(in crate::view) diff_suppress_clicks_remaining: u8,
    pub(in crate::view) diff_text_hitboxes: FxHashMap<(usize, DiffTextRegion), DiffTextHitbox>,
    /// A search match whose row still has to be brought into view sideways, and
    /// how many more frames to keep trying for.
    ///
    /// The vertical jump is deferred to the list's own prepaint and the row is
    /// only measurable once it paints at its new position, which is not always
    /// the very next frame — the frame that applies the scroll can still be
    /// painting the rows it was showing before. The budget is what stops a row
    /// that never paints from leaving the request live for good.
    pub(in crate::view) diff_search_horizontal_reveal: Option<(usize, u8)>,
    /// Where the merge tool's column rows painted their text this frame, for the
    /// sideways half of a search reveal. Rebuilt every frame like
    /// [`Self::diff_text_hitboxes`].
    pub(in crate::view) conflict_text_hitboxes:
        FxHashMap<(usize, ThreeWayColumn), ConflictTextHitbox>,
    pub(in crate::view) diff_text_layout_cache_epoch: u64,
    pub(in crate::view) diff_text_layout_cache: FxHashMap<u64, DiffTextLayoutCacheEntry>,
    pub(in crate::view) diff_search_active: bool,
    pub(in crate::view) diff_search_query: SharedString,
    pub(in crate::view) diff_search_options: super::diff_search::DiffSearchOptions,
    pub(in crate::view) diff_search_regex_error: Option<SharedString>,
    pub(in crate::view) diff_search_matches: Vec<usize>,
    pub(in crate::view) diff_search_inline_patch_trigram_index:
        Option<super::diff_search::DiffSearchVisibleTrigramIndex>,
    pub(in crate::view) diff_search_match_ix: Option<usize>,
    pub(in crate::view) diff_search_debounce_seq: u64,
    pub(in crate::view) diff_search_pending_previous_query: Option<SharedString>,
    pub(in crate::view) diff_search_scroll: ScrollHandle,
    pub(in crate::view) diff_search_input: Entity<components::TextInput>,
    pub(super) _diff_search_subscription: gpui::Subscription,

    pub(in crate::view) file_diff_cache_repo_id: Option<RepoId>,
    pub(in crate::view) file_diff_cache_rev: u64,
    pub(in crate::view) file_diff_cache_content_signature: Option<u64>,
    pub(in crate::view) file_diff_cache_whitespace_mode: DiffWhitespaceMode,
    pub(in crate::view) file_diff_cache_target: Option<DiffTarget>,
    pub(in crate::view) file_diff_cache_error: Option<String>,
    pub(in crate::view) file_diff_cache_path: Option<std::path::PathBuf>,
    pub(in crate::view) file_diff_cache_language: Option<rows::DiffSyntaxLanguage>,
    pub(in crate::view) file_diff_cache_rows: Vec<FileDiffRow>,
    pub(in crate::view) file_diff_row_provider: Option<Arc<super::diff_cache::PagedFileDiffRows>>,
    /// Real old-side file text used for split and inline syntax projection.
    pub(in crate::view) file_diff_old_text: SharedString,
    pub(in crate::view) file_diff_old_line_starts: Arc<[usize]>,
    pub(in crate::view) file_diff_old_line_to_row: Arc<[Option<usize>]>,
    pub(in crate::view) file_diff_old_line_to_inline_row: Arc<[Option<usize>]>,
    /// Real new-side file text used for split and inline syntax projection.
    pub(in crate::view) file_diff_new_text: SharedString,
    pub(in crate::view) file_diff_new_line_starts: Arc<[usize]>,
    pub(in crate::view) file_diff_new_line_to_row: Arc<[Option<usize>]>,
    pub(in crate::view) file_diff_new_line_to_inline_row: Arc<[Option<usize>]>,
    pub(in crate::view) file_diff_inline_cache: Vec<AnnotatedDiffLine>,
    pub(in crate::view) file_diff_inline_row_provider:
        Option<Arc<super::diff_cache::PagedFileDiffInlineRows>>,
    pub(in crate::view) file_diff_inline_text: SharedString,
    pub(in crate::view) file_diff_inline_word_highlights: rows::LruCache<usize, Vec<Range<usize>>>,
    pub(in crate::view) file_diff_split_word_highlights:
        rows::LruCache<usize, FileDiffSplitWordHighlights>,
    pub(in crate::view) file_diff_cache_seq: u64,
    pub(in crate::view) file_diff_cache_inflight: Option<u64>,
    pub(in crate::view) file_diff_syntax_generation: u64,
    pub(in crate::view) file_diff_style_cache_epochs: FileDiffStyleCacheEpochs,
    pub(in crate::view) syntax_chunk_poll_task: Option<gpui::Task<()>>,
    pub(in crate::view) prepared_syntax_documents:
        FxHashMap<PreparedSyntaxDocumentKey, rows::PreparedDiffSyntaxDocument>,
    #[cfg(test)]
    pub(in crate::view) diff_syntax_budget_override: Option<rows::DiffSyntaxBudget>,

    pub(in crate::view) file_markdown_preview_cache_repo_id: Option<RepoId>,
    pub(in crate::view) file_markdown_preview_cache_rev: u64,
    pub(in crate::view) file_markdown_preview_cache_content_signature: Option<u64>,
    pub(in crate::view) file_markdown_preview_cache_target: Option<DiffTarget>,
    pub(in crate::view) file_markdown_preview: LoadableMarkdownDiff,
    pub(in crate::view) file_markdown_preview_seq: u64,
    pub(in crate::view) file_markdown_preview_inflight: Option<u64>,
    pub(in crate::view) markdown_preview_wrap: MarkdownPreviewWrapCache,
    /// Row the quick-search cursor wants revealed in the flowing markdown
    /// preview, shared with the renderer that measures it. See
    /// [`rows::MarkdownPreviewRevealRequest`].
    pub(in crate::view) markdown_preview_reveal: rows::MarkdownPreviewRevealRequest,

    pub(in crate::view) file_image_diff_cache_repo_id: Option<RepoId>,
    pub(in crate::view) file_image_diff_cache_rev: u64,
    pub(in crate::view) file_image_diff_cache_content_signature: Option<u64>,
    pub(in crate::view) file_image_diff_cache_target: Option<DiffTarget>,
    pub(in crate::view) file_image_diff_cache_seq: u64,
    pub(in crate::view) file_image_diff_cache_inflight: Option<u64>,
    pub(in crate::view) file_image_diff_cache_path: Option<std::path::PathBuf>,
    pub(in crate::view) file_image_diff_cache_old: Option<Arc<gpui::RenderImage>>,
    pub(in crate::view) file_image_diff_cache_new: Option<Arc<gpui::RenderImage>>,
    pub(in crate::view) file_image_diff_cache_old_svg_path: Option<std::path::PathBuf>,
    pub(in crate::view) file_image_diff_cache_new_svg_path: Option<std::path::PathBuf>,

    pub(in crate::view) worktree_preview_path: Option<std::path::PathBuf>,
    pub(in crate::view) worktree_preview_source_path: Option<std::path::PathBuf>,
    pub(in crate::view) worktree_preview: Loadable<usize>,
    pub(in crate::view) worktree_preview_source_len: usize,
    pub(in crate::view) worktree_preview_text: SharedString,
    pub(in crate::view) worktree_preview_line_starts: Arc<[usize]>,
    pub(in crate::view) worktree_preview_line_flags: Arc<[u8]>,
    pub(in crate::view) worktree_preview_search_trigram_index:
        Option<super::diff_search::DiffSearchVisibleTrigramIndex>,
    pub(in crate::view) worktree_preview_content_rev: u64,
    pub(in crate::view) worktree_markdown_preview_path: Option<std::path::PathBuf>,
    pub(in crate::view) worktree_markdown_preview_source_rev: u64,
    pub(in crate::view) worktree_markdown_preview: LoadableMarkdownDoc,
    /// Sizes read from the headers of the pictures the rendered preview draws,
    /// so a picture that has not decoded yet can still hold its box open.
    pub(in crate::view) worktree_markdown_preview_picture_sizes: rows::MarkdownPreviewPictureSizes,
    /// Where each sideways-scrolling block of the rendered preview is scrolled
    /// to, so its scrollbar has something to read.
    pub(in crate::view) worktree_markdown_preview_block_scrolls: rows::MarkdownDocumentBlockScrolls,
    /// Block grouping of the document the rendered preview last drew, so it is
    /// not re-derived on every frame.
    pub(in crate::view) worktree_markdown_preview_blocks: rows::MarkdownDocumentBlockCache,
    /// Pictures in the rendered preview that are still decoding and already
    /// have someone waiting to repaint the pane when they finish.
    pub(in crate::view) worktree_markdown_preview_image_waits: FxHashSet<gpui::Resource>,
    pub(in crate::view) worktree_markdown_preview_seq: u64,
    pub(in crate::view) worktree_markdown_preview_inflight: Option<u64>,
    pub(in crate::view) worktree_preview_segments_cache_path: Option<std::path::PathBuf>,
    pub(in crate::view) worktree_preview_syntax_language: Option<rows::DiffSyntaxLanguage>,
    pub(in crate::view) worktree_preview_style_cache_epoch: u64,
    pub(in crate::view) worktree_preview_cache_write_blocked_until_rev: Option<u64>,
    pub(in crate::view) worktree_preview_segments_cache:
        FxHashMap<usize, VersionedCachedDiffStyledText>,
    pub(in crate::view) diff_preview_is_new_file: bool,

    /// The editable working-tree buffer. See `super::file_editor`.
    pub(in crate::view) file_editor_input: Entity<components::TextInput>,
    pub(super) _file_editor_input_subscription: gpui::Subscription,
    /// Which repo/path the input currently holds, so a target change is one
    /// comparison rather than a reload every frame.
    pub(in crate::view) file_editor_key: Option<(RepoId, std::path::PathBuf)>,
    pub(in crate::view) file_editor_language: Option<rows::DiffSyntaxLanguage>,
    pub(in crate::view) file_editor_loading: bool,
    /// Repo status revision the buffer was last read at. A clean buffer re-reads
    /// when this moves, so an external write to the open file is picked up
    /// rather than silently overwritten by the next save.
    pub(in crate::view) file_editor_loaded_status_rev: u64,
    pub(in crate::view) file_editor_error: Option<SharedString>,
    pub(in crate::view) file_editor_dirty: bool,
    /// The topmost 0-based line an unsaved edit has touched, or `None` while the
    /// buffer matches disk.
    ///
    /// Blame is indexed by committed line number, so an insertion or deletion
    /// shifts the attribution of everything under it — but only under it. This
    /// watermark is what lets the gutter keep showing blame for the untouched
    /// head of the file instead of blanking the whole column on the first
    /// keystroke. Deliberately pessimistic: an edit that changed no line count
    /// still moves it, because tracking that precisely costs more than the
    /// attribution below it is worth.
    pub(in crate::view) file_editor_first_dirty_line: Option<u32>,
    /// Fingerprint of the text last known to be on disk. `None` before the
    /// first read lands, which reads as "everything is unsaved".
    pub(in crate::view) file_editor_saved_fingerprint: Option<u64>,
    /// Unsaved buffers the user navigated away from, keyed by path. This is what
    /// makes leaving a file and coming back non-destructive with auto-save off.
    /// Keyed by repo *and* path: two repo tabs can hold the same relative path,
    /// and one must not restore over the other's buffer.
    pub(in crate::view) file_editor_stash:
        FxHashMap<(RepoId, std::path::PathBuf), super::file_editor::StashedFileEdit>,
    /// Bumped whenever the set of files with unsaved edits changes.
    ///
    /// That set lives here rather than in the store, so nothing outside this
    /// pane can notice it moving on its own — the sidebar keys its file-row
    /// cache off this counter and repaints on the notify that bumps it.
    pub(in crate::view) unsaved_file_edits_rev: u64,
    /// The pending quiet-period timer for auto-save. Dropping it cancels it, so
    /// every keystroke simply replaces it.
    pub(in crate::view) file_editor_autosave: Option<gpui::Task<()>>,
    /// The editor's tree-sitter document. Owned here for the same reason the
    /// resolved output's is: it must survive every keystroke, which is exactly
    /// what a content-hash-keyed cache cannot do.
    pub(in crate::view) file_editor_live_syntax: Option<rows::LiveSyntaxDocument>,
    /// `(model_id, revision)` the live tree was last built or synced for.
    pub(in crate::view) file_editor_live_syntax_source: Option<(u64, u64)>,
    pub(in crate::view) file_editor_live_syntax_building: Option<(u64, u64)>,
    /// In-flight *first* parse. Kept apart from the reparse slot, which is
    /// cleared whenever there is no document to reparse — the state a first
    /// parse runs in.
    pub(in crate::view) file_editor_live_syntax_build: Option<gpui::Task<()>>,
    pub(in crate::view) file_editor_live_syntax_reparse: Option<gpui::Task<()>>,
    /// The delimiters currently washed as the caret's bracket pair.
    pub(in crate::view) file_editor_bracket_match: Option<(Range<usize>, Range<usize>)>,
    /// Byte ranges of every search match in the editor buffer, one per
    /// occurrence and parallel to `diff_search_matches`, which carries the line
    /// each of them sits on. Keeping the two parallel is what lets the shared
    /// `n/N` label and match cursor work over the editor unchanged.
    pub(in crate::view) file_editor_search_matches: Vec<Range<usize>>,
    /// The buffer the scan reads. It runs without a `cx` and so cannot reach the
    /// input; a snapshot is an `Arc` bump and immutable under later edits, which
    /// makes caching one here the cheap way to hand it the live text.
    pub(in crate::view) file_editor_search_source: Option<TextModelSnapshot>,
    /// Bumped whenever the *painted* match set moves — a rescan, a cursor step,
    /// the search closing. `render_file_editor` rebinds the highlight provider
    /// when it differs from `file_editor_search_applied_rev`.
    pub(in crate::view) file_editor_search_rev: u64,
    pub(in crate::view) file_editor_search_applied_rev: u64,
    /// Bumped only when the match *cursor* moves. Separate from the rev above
    /// because it drives the selection, and a rescan alone must not re-select:
    /// the buffer is rescanned on every keystroke while the search box is open,
    /// which would drag the caret off what the user is typing.
    pub(in crate::view) file_editor_search_reveal_rev: u64,
    pub(in crate::view) file_editor_search_reveal_applied_rev: u64,
    /// Set once a search reveal has moved the caret, cleared once the editor
    /// has been scrolled sideways to it.
    ///
    /// The caret's x can only be read from the layout of a frame that already
    /// painted it, so the horizontal half of the reveal lands one frame after
    /// the selection does.
    pub(in crate::view) file_editor_search_reveal_x_pending: bool,
    /// Bumped on every theme change: the syntax palette is baked into the
    /// snapshot the provider closes over, so a new theme needs a new binding key.
    pub(in crate::view) file_editor_provider_theme_epoch: u64,
    /// Mirrors the settings window's toggle; the pane never writes it back.
    pub(in crate::view) auto_save_file_edits: bool,

    pub(in crate::view) conflict_resolver_input: Entity<components::TextInput>,
    pub(super) _conflict_resolver_input_subscription: gpui::Subscription,
    pub(in crate::view) conflict_resolver: ConflictResolverUiState,
    pub(in crate::view) conflict_open_summary_toasted_files:
        FxHashSet<(RepoId, std::path::PathBuf)>,
    pub(in crate::view) conflict_resolver_vsplit_ratio: f32,
    pub(in crate::view) conflict_resolver_vsplit_resize: Option<ConflictVSplitResizeState>,
    pub(in crate::view) conflict_three_way_col_ratios: [f32; 2],
    pub(in crate::view) conflict_three_way_col_widths: [Pixels; 3],
    pub(in crate::view) conflict_hsplit_resize: Option<ConflictHSplitResizeState>,
    pub(in crate::view) conflict_diff_split_ratio: f32,
    pub(in crate::view) conflict_diff_split_resize: Option<ConflictDiffSplitResizeState>,
    pub(in crate::view) conflict_diff_split_col_widths: [Pixels; 2],
    pub(in crate::view) conflict_canvas_rows_enabled: bool,
    pub(in crate::view) conflict_diff_segments_cache_split:
        crate::view::conflict_resolver::ConflictSplitStyledTextCache,
    pub(in crate::view) conflict_diff_query_segments_cache_split:
        crate::view::conflict_resolver::ConflictSplitStyledTextCache,
    pub(in crate::view) conflict_diff_query_cache_query: SharedString,
    pub(in crate::view) conflict_diff_query_cache_options: super::diff_search::DiffSearchOptions,
    pub(in crate::view) conflict_three_way_segments_cache:
        FxHashMap<(usize, ThreeWayColumn), CachedDiffStyledText>,
    /// Quick-search overlay layered on top of `conflict_three_way_segments_cache`.
    ///
    /// Separate so a query change throws away only the wash and leaves the
    /// syntax/word-highlight work standing, the way the two-way columns split
    /// `conflict_diff_segments_cache_split` from its query twin. Holds only
    /// non-current matches — the current one moves with the search cursor and
    /// is built per frame.
    pub(in crate::view) conflict_three_way_query_segments_cache:
        FxHashMap<(usize, ThreeWayColumn), CachedDiffStyledText>,
    /// Prepared full-document syntax trees for each merge-input side (base, ours, theirs).
    /// When present, three-way rendering uses document-based syntax instead of per-line heuristics.
    pub(in crate::view) conflict_three_way_prepared_syntax_documents:
        ThreeWaySides<Option<rows::PreparedDiffSyntaxDocument>>,
    /// Per-side flag tracking whether a background syntax parse is in-flight.
    pub(in crate::view) conflict_three_way_syntax_inflight: ThreeWaySides<bool>,
    pub(in crate::view) conflict_resolved_preview_path: Option<std::path::PathBuf>,
    /// Latest editable-output revision observed by the input subscription. This
    /// is intentionally independent of the content hash so a keypress can
    /// supersede debounce work without materializing and scanning the document.
    pub(in crate::view) conflict_resolved_preview_source_revision:
        Option<ResolvedOutputSourceRevision>,
    /// Editable-output snapshot at the last file load/save refresh. Snapshot
    /// equality is O(1), and undo restores the matching snapshot, so this can
    /// drive the user-facing Modified state without hashing the whole output.
    pub(in crate::view) conflict_resolved_output_saved_snapshot: Option<TextModelSnapshot>,
    pub(in crate::view) conflict_resolved_output_modified: bool,
    pub(in crate::view) conflict_resolved_output_projection:
        Option<conflict_resolver::ResolvedOutputProjection>,
    /// Byte ownership for displayed conflict blocks in the live output.
    pub(in crate::view) conflict_resolved_output_block_map:
        conflict_resolver::ResolvedOutputBlockMap,
    pub(in crate::view) conflict_resolved_preview_text: TextModelSnapshot,
    pub(in crate::view) conflict_resolved_preview_syntax_language: Option<rows::DiffSyntaxLanguage>,
    pub(in crate::view) conflict_resolved_preview_line_count: usize,
    pub(in crate::view) conflict_resolved_preview_line_starts: Arc<[usize]>,
    /// The editable resolved output's tree-sitter document. Owned here rather
    /// than in the shared thread-local cache because there is exactly one of
    /// them at a time and it must survive every keystroke — which is precisely
    /// what a content-hash-keyed cache cannot do.
    pub(in crate::view) conflict_resolved_output_live_syntax: Option<rows::LiveSyntaxDocument>,
    /// In-flight reparse for an edit that outran the foreground budget.
    pub(in crate::view) conflict_resolved_output_live_syntax_reparse: Option<gpui::Task<()>>,
    /// What the live tree was last built for: the buffer revision and the
    /// placeholder mask. Both must be unchanged for a refresh to be a no-op.
    ///
    /// Deliberately not pointer identity on the text. `SharedString` can be
    /// `Borrowed`, and `Arc<str>::from(&str)` then allocates afresh on every
    /// call, so a pointer check would never match — turning every refresh into a
    /// reparse and, because installing a provider notifies the input that
    /// triggered the refresh, into an unbreakable loop.
    pub(in crate::view) conflict_resolved_output_live_syntax_source:
        Option<(ResolvedOutputSourceRevision, Arc<[Range<usize>]>)>,
    /// Bumped on every theme change. The syntax palette is baked into
    /// `LiveSyntaxSnapshot`, so a new theme needs a new provider -- and the
    /// binding key is the only thing that makes `TextInput` adopt one.
    /// Hashing theme *colours* into the key instead is not enough: two dark
    /// themes can agree on the few colours sampled and still differ on the
    /// syntax palette, leaving stale colours installed.
    pub(in crate::view) conflict_resolved_output_provider_theme_epoch: u64,
    /// Which conflict the installed output highlights wash yellow. Conflict
    /// navigation moves no text and touches no tree, so none of the refresh
    /// paths fire on it; this is what tells the render pass the active row moved
    /// and the provider has to be rebuilt.
    pub(in crate::view) conflict_resolved_output_highlighted_conflict: Option<usize>,
    /// Unresolved output rows and their conflict, cached for the buffer
    /// revision they were computed from.
    ///
    /// Conflict navigation moves the yellow wash but changes no text, so
    /// recomputing these would rescan the whole document on every jump — which
    /// is exactly what made F3 cost tens of milliseconds on a large file.
    pub(in crate::view) conflict_resolved_output_unresolved_rows: Option<CachedUnresolvedRows>,
    /// How many times the resolved output's syntax refresh has gone past its
    /// early-out and rescanned the document. Only an edit should do that;
    /// navigation must not.
    #[cfg(test)]
    pub(in crate::view) conflict_resolved_output_full_scans: usize,
    /// Revision an off-thread first parse is currently running for, so repeated
    /// refreshes over the same text do not pile up duplicate builds.
    pub(in crate::view) conflict_resolved_output_live_syntax_building:
        Option<ResolvedOutputSourceRevision>,
    /// In-flight *first* parse. Kept apart from the reparse slot: that one is
    /// cleared whenever there is no document to reparse, which is exactly the
    /// state a first parse runs in -- sharing the slot would cancel it.
    pub(in crate::view) conflict_resolved_output_live_syntax_build: Option<gpui::Task<()>>,
    pub(in crate::view) conflict_resolved_output_measure_row: usize,
    pub(in crate::view) conflict_resolved_outline_stash: Option<StashedResolvedOutlineState>,
    #[cfg(test)]
    pub(in crate::view) conflict_resolved_outline_background_delay_override:
        Option<std::time::Duration>,

    pub(in crate::view) history_view: Entity<super::HistoryView>,
    pub(in crate::view) diff_scroll: UniformListScrollHandle,
    pub(in crate::view) diff_split_right_scroll: UniformListScrollHandle,
    pub(in crate::view) conflict_resolver_diff_scroll: UniformListScrollHandle,
    pub(in crate::view) conflict_preview_ours_scroll: UniformListScrollHandle,
    pub(in crate::view) conflict_preview_theirs_scroll: UniformListScrollHandle,
    pub(in crate::view) conflict_preview_last_synced_x: [Pixels; 4],
    pub(in crate::view) conflict_preview_last_synced_y: [Pixels; 4],
    /// Source/output handle index that received the latest vertical wheel
    /// gesture: base/left=0, ours=1, theirs/right=2, output=3.
    pub(in crate::view) conflict_preview_vertical_wheel_master: Option<usize>,
    /// The next output/gutter sync belongs to that wheel gesture, so output
    /// must drive the pair instead of a stale gutter baseline.
    pub(in crate::view) conflict_output_gutter_wheel_sync_pending: bool,
    pub(in crate::view) conflict_resolved_preview_scroll: UniformListScrollHandle,
    /// Scroll handle for the editable resolved-output `TextInput`. The input lays
    /// out at full content height inside an `overflow_y_scroll` container that
    /// tracks this handle, and the input reads the same handle to window its line
    /// shaping. It is also the output member (index 3) of the conflict-preview
    /// scroll-sync group, so it stands in for `conflict_resolved_preview_scroll`
    /// (which now only backs the read-only projection paths).
    pub(in crate::view) conflict_resolved_output_editor_scroll: ScrollHandle,
    pub(in crate::view) conflict_resolved_preview_gutter_scroll: UniformListScrollHandle,
    pub(in crate::view) conflict_resolved_preview_gutter_last_synced_y: [Pixels; 2],
    pub(in crate::view) worktree_preview_scroll: UniformListScrollHandle,
    /// Scroll handle for the editor's `TextInput`: the input lays out at full
    /// content size inside an `overflow_scroll` container tracking this handle,
    /// and reads the same handle to window its line shaping.
    pub(in crate::view) file_editor_scroll: ScrollHandle,
    /// Gutter list, mirrored to `file_editor_scroll`'s vertical offset.
    pub(in crate::view) file_editor_gutter_scroll: UniformListScrollHandle,
    /// UI-scaled row height the gutter list paints at, computed by the render
    /// pass so the virtualized row processor can read it without a scale lookup.
    pub(in crate::view) file_editor_gutter_row_height: Pixels,
    /// The same, for the merge tool's resolved-output gutter. Navigation centres
    /// the editable output on a row from `&self`, where there is no `cx` to look
    /// the scale up through, so the render pass leaves it here.
    pub(in crate::view) conflict_resolved_gutter_row_height: Pixels,
    /// Blame for the edited file, resolved by the render pass so the virtualized
    /// gutter rows can read it without rebuilding the context per row.
    pub(in crate::view) file_editor_blame: Option<rows::BlameRenderCtx>,
    pub(in crate::view) file_editor_blame_width: Pixels,
    /// First gutter row owned by each logical line, so a wrapped line's number
    /// sits on the first of the rows it spans. Empty when the buffer is not
    /// wrapping. Retained to keep its allocation across frames.
    pub(in crate::view) file_editor_wrap_row_starts: Vec<usize>,

    pub(super) path_display_cache: std::cell::RefCell<path_display::PathDisplayCache>,

    /// Per-repo interactive rebase editing state, keyed by repo id so that
    /// setups open in several repo tabs at once stay independent. Entries are
    /// populated when a repo's setup becomes Ready and dropped when its setup
    /// goes away (see `apply_state`).
    pub(in crate::view) interactive_rebase_states: FxHashMap<RepoId, IRebaseViewState>,
}
