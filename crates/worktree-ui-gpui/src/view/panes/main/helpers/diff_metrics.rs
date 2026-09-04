use super::super::*;
use rustc_hash::FxHashMap;

#[derive(Clone, Debug)]
pub(in crate::view) struct VersionedCachedDiffStyledText {
    pub(in crate::view) syntax_epoch: u64,
    pub(in crate::view) query_generation: u64,
    pub(in crate::view) styled: CachedDiffStyledText,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct FileDiffStyleCacheEpochs {
    pub(in crate::view) split_left: u64,
    pub(in crate::view) split_right: u64,
}

impl FileDiffStyleCacheEpochs {
    pub(in crate::view) fn bump_left(&mut self) {
        self.split_left = self.split_left.wrapping_add(1);
    }

    pub(in crate::view) fn bump_right(&mut self) {
        self.split_right = self.split_right.wrapping_add(1);
    }

    pub(in crate::view) fn bump_both(&mut self) {
        self.bump_left();
        self.bump_right();
    }

    pub(in crate::view) fn split_epoch(self, region: crate::view::DiffTextRegion) -> u64 {
        match region {
            crate::view::DiffTextRegion::SplitLeft => self.split_left,
            crate::view::DiffTextRegion::SplitRight => self.split_right,
            crate::view::DiffTextRegion::Inline => 0,
        }
    }

    pub(in crate::view) fn inline_epoch(self, kind: worktree_core::domain::DiffLineKind) -> u64 {
        match kind {
            worktree_core::domain::DiffLineKind::Remove => self.split_left,
            worktree_core::domain::DiffLineKind::Add
            | worktree_core::domain::DiffLineKind::Context => self.split_right,
            worktree_core::domain::DiffLineKind::Header
            | worktree_core::domain::DiffLineKind::Hunk => 0,
        }
    }
}

pub(in crate::view) const FILE_DIFF_WORD_HIGHLIGHT_CACHE_MAX_ENTRIES: usize = 4_096;

#[derive(Clone, Debug, Default)]
pub(in crate::view) struct FileDiffSplitWordHighlights {
    pub(in crate::view) old: Vec<Range<usize>>,
    pub(in crate::view) new: Vec<Range<usize>>,
}

pub(in crate::view) fn versioned_cached_diff_styled_text_is_current(
    entry: Option<&VersionedCachedDiffStyledText>,
    syntax_epoch: u64,
) -> Option<&CachedDiffStyledText> {
    let entry = entry?;
    (entry.syntax_epoch == syntax_epoch).then_some(&entry.styled)
}

pub(in crate::view) fn versioned_query_cached_diff_styled_text_is_current(
    entry: Option<&VersionedCachedDiffStyledText>,
    syntax_epoch: u64,
    query_generation: u64,
) -> Option<&CachedDiffStyledText> {
    let entry = entry?;
    (entry.syntax_epoch == syntax_epoch && entry.query_generation == query_generation)
        .then_some(&entry.styled)
}

/// Whether the content pane is showing a file's full content *at the commit the
/// file browser is pinned to* — the state the historical browse tint marks.
///
/// Content-preview mode alone is not enough: a file's content can be opened from
/// some other commit while a browse point is active, and that content is not
/// what the browse point describes. The commit ids have to match.
pub(in crate::view) fn historical_browse_content(
    repo: &RepoState,
    rendered_target: Option<&DiffTarget>,
) -> bool {
    if !repo.diff_state.content_preview {
        return false;
    }
    let Some(browsing) = repo.browsing_commit() else {
        return false;
    };
    matches!(
        rendered_target,
        Some(DiffTarget::Commit { commit_id, .. }) if commit_id == browsing
    )
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum PreparedSyntaxViewMode {
    FileDiffSplitLeft,
    FileDiffSplitRight,
    WorktreePreview,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) struct PreparedSyntaxDocumentKey {
    pub(in crate::view) repo_id: RepoId,
    pub(in crate::view) target_rev: u64,
    pub(in crate::view) file_path: std::path::PathBuf,
    pub(in crate::view) view_mode: PreparedSyntaxViewMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) enum CollapsedDiffExpansionKind {
    #[default]
    None,
    Up,
    Down,
    Both,
    Short,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct CollapsedDiffHunk {
    pub(in crate::view) src_ix: usize,
    pub(in crate::view) base_row_start: usize,
    pub(in crate::view) base_row_end_exclusive: usize,
    pub(in crate::view) has_additions: bool,
    pub(in crate::view) has_removals: bool,
    pub(in crate::view) reveal_up_lines: usize,
    pub(in crate::view) reveal_down_lines: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) struct CollapsedDiffReveal {
    pub(in crate::view) up_lines: usize,
    pub(in crate::view) down_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct CollapsedDiffProjectionIdentity {
    pub(in crate::view) repo_id: RepoId,
    pub(in crate::view) diff_target: DiffTarget,
    pub(in crate::view) file_path: std::path::PathBuf,
    pub(in crate::view) diff_whitespace_mode: DiffWhitespaceMode,
    pub(in crate::view) patch_content_signature: Option<u64>,
    pub(in crate::view) file_content_signature: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum CollapsedDiffVisibleRow {
    HunkHeader {
        src_ix: usize,
        expansion_kind: CollapsedDiffExpansionKind,
        display_src_ix: Option<usize>,
        hidden_rows: usize,
    },
    FileRow {
        row_ix: usize,
    },
}

impl CollapsedDiffVisibleRow {
    pub(in crate::view) const fn row_ix(self) -> Option<usize> {
        match self {
            Self::FileRow { row_ix } => Some(row_ix),
            Self::HunkHeader { .. } => None,
        }
    }

    pub(in crate::view) const fn header_display_src_ix(self) -> Option<usize> {
        match self {
            Self::HunkHeader { display_src_ix, .. } => display_src_ix,
            Self::FileRow { .. } => None,
        }
    }

    pub(in crate::view) const fn header_action_src_ix(self) -> Option<usize> {
        match self {
            Self::HunkHeader { src_ix, .. } => Some(src_ix),
            Self::FileRow { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum DiffHorizontalScrollColumn {
    Primary,
    SplitRight,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct DiffWrapVisualRow {
    pub(in crate::view) source_visible_ix: usize,
    pub(in crate::view) wrap_ix: usize,
    pub(in crate::view) primary_range: rows::DiffWrapByteRange,
    pub(in crate::view) secondary_range: rows::DiffWrapByteRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct DiffWrapVisibleCacheKey {
    pub(in crate::view) source_len: usize,
    pub(in crate::view) diff_view: DiffViewMode,
    pub(in crate::view) is_file_view: bool,
    pub(in crate::view) collapsed_projection_active: bool,
    pub(in crate::view) projection_rev: u64,
    pub(in crate::view) diff_cache_rev: u64,
    pub(in crate::view) file_diff_cache_seq: u64,
    pub(in crate::view) inline_columns: usize,
    pub(in crate::view) split_columns: usize,
    /// Columns a file preview row wraps at. The preview is a single column
    /// with its own gutter, so neither of the diff's two widths describes it.
    pub(in crate::view) preview_columns: usize,
    /// Bumped when the previewed file's content changes, so the rows are
    /// rebuilt for the new text rather than kept from the old.
    pub(in crate::view) preview_content_rev: u64,
    pub(in crate::view) reveal_whitespace_chars: bool,
}

impl DiffHorizontalScrollColumn {
    pub(in crate::view) const fn index(self) -> usize {
        match self {
            Self::Primary => 0,
            Self::SplitRight => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::view) struct DiffHorizontalScrollState {
    pub(in crate::view) content_widths: [Pixels; 2],
}

/// Memoized blame author-time range, keyed by a clone of the blame `Arc`. See
/// [`MainPaneView::blame_time_range_cache`].
pub(in crate::view) type BlameTimeRangeCache = Option<(
    std::sync::Arc<Vec<worktree_core::services::BlameLine>>,
    Option<(i64, i64)>,
)>;

impl DiffHorizontalScrollState {
    pub(in crate::view) fn new() -> Self {
        Self {
            content_widths: [px(0.0); 2],
        }
    }

    pub(in crate::view) fn reset(&mut self) {
        self.content_widths = [px(0.0); 2];
    }

    pub(in crate::view) fn record_content_width(
        &mut self,
        column: DiffHorizontalScrollColumn,
        width: Pixels,
    ) -> bool {
        let ix = column.index();
        if width > self.content_widths[ix] {
            self.content_widths[ix] = width;
            true
        } else {
            false
        }
    }
}

/// View-local editing state for one repo's interactive rebase setup.
#[derive(Default)]
pub(in crate::view) struct IRebaseViewState {
    pub(in crate::view) mode: ICommitEditorMode,
    pub(in crate::view) entries: Vec<worktree_core::services::InteractiveRebaseEntry>,
    pub(in crate::view) original_entries: Vec<worktree_core::services::InteractiveRebaseEntry>,
    pub(in crate::view) source_colors: FxHashMap<String, u8>,
    /// Active auto-squash strategy, or None when auto-squash is off.
    pub(in crate::view) autosquash_mode: Option<AutosquashMode>,
    /// Commits folded away by auto-squash, keyed by the surviving commit id.
    /// Each survivor's `entries` row displays these ids; they are re-expanded
    /// into `fixup` todo entries when the rebase starts.
    pub(in crate::view) folded:
        FxHashMap<String, Vec<worktree_core::services::InteractiveRebaseEntry>>,
    pub(in crate::view) drag_state: Option<IRebaseDragState>,
    /// Variable-height virtualized list state, lazily created on first render
    /// (`ListState` has no `Default`). Kept in sync with `entries`/`folded` via
    /// `list_sig` (remeasure on same-count content change, reset on count change).
    pub(in crate::view) scroll: Option<gpui::ListState>,
    /// (content-hash, item-count) the `scroll` ListState was last synced to.
    pub(in crate::view) list_sig: (u64, usize),
    /// (ix_a, ix_b, version) — the two data-indices swapped by ▲/▼; drives fade-in animation.
    pub(in crate::view) reorder_anim: Option<(usize, usize, u32)>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) enum ICommitEditorMode {
    #[default]
    Rebase,
    CherryPick,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::view) struct IRebaseDragState {
    pub(in crate::view) from_ix: usize,
    pub(in crate::view) to_ix: usize,
    /// Drop-target position in display order (0..=entry_count).
    pub(in crate::view) display_pos: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum DiffTextAutoscrollTarget {
    DiffLeftOrInline,
    DiffSplitRight,
    WorktreePreview,
    ConflictResolvedPreview,
}

pub(in crate::view) fn parse_conflict_canvas_rows_env(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "off" | "no"
    )
}

pub(in crate::view) fn conflict_canvas_rows_enabled_from_env() -> bool {
    std::env::var("WORKTREE_CONFLICT_CANVAS_ROWS")
        .ok()
        .is_none_or(|value| parse_conflict_canvas_rows_env(&value))
}
