//! The per-row styled-text cache: identity-keyed diff text the conflict rows render
//! through.

use super::super::conflict_resolver;
use super::super::diff_text::*;
use super::super::perf::{self, ViewPerfSpan};
use super::super::*;
use crate::kit::text_search::{DiffSearchMatcher, DiffSearchOptions};

pub(super) const CONFLICT_ROW_FONT_SCALE: f32 = 0.80;
pub(super) const CONFLICT_ROW_TEXT_TRAILING_PADDING_PX: f32 = 16.0;

/// Resolved-output gutter geometry, in design units. `resolved_output_gutter_width`
/// sums these to size the gutter container, and the row renderer lays the same
/// values out inside it, so the two must be scaled together or the gutter clips
/// its own marker and badge.
pub(super) const RESOLVED_OUTPUT_MARKER_PX: f32 = 12.0;
/// `mr_1` after the marker lane. Rem-derived in the row (so already UI-scaled
/// there); named here so the width sum agrees with it.
pub(super) const RESOLVED_OUTPUT_MARKER_GAP_PX: f32 = 4.0;
pub(super) const RESOLVED_OUTPUT_MARKER_LANE_PX: f32 =
    RESOLVED_OUTPUT_MARKER_PX + RESOLVED_OUTPUT_MARKER_GAP_PX;
/// Width of the A/B/C origin badge cell.
pub(super) const RESOLVED_OUTPUT_BADGE_PX: f32 = 24.0;
/// `mr_1` after the line-number cell.
pub(super) const RESOLVED_OUTPUT_LINE_NO_GAP_PX: f32 = 4.0;
/// The fold row's "reveal 20 more lines" buttons and their glyphs.
pub(super) const CONFLICT_FOLD_REVEAL_BUTTON_PX: f32 = 18.0;
pub(super) const CONFLICT_FOLD_REVEAL_ICON_PX: f32 = 10.0;

/// The A/B/C glyph box inside the badge cell, and the auto-resolve confidence dot
/// pinned to the badge's top-right corner.
pub(super) const RESOLVED_OUTPUT_BADGE_GLYPH_W_PX: f32 = 18.0;
pub(super) const RESOLVED_OUTPUT_BADGE_GLYPH_H_PX: f32 = 14.0;
pub(super) const RESOLVED_OUTPUT_CONFIDENCE_DOT_PX: f32 = 5.0;
/// The marker bar inside the lane, and the caps that flag a region's first and
/// last row.
pub(super) const RESOLVED_OUTPUT_MARKER_BAR_PX: f32 = 2.0;
pub(super) const RESOLVED_OUTPUT_MARKER_CAP_W_PX: f32 = 8.0;
pub(super) const RESOLVED_OUTPUT_MARKER_CAP_INSET_PX: f32 = 3.0;

pub(super) fn build_conflict_cached_diff_styled_text(
    theme: AppTheme,
    text: &str,
    word_ranges: &[Range<usize>],
    query: &str,
    language: Option<DiffSyntaxLanguage>,
    syntax_mode: DiffSyntaxMode,
    word_kind: Option<crate::theme::DiffColorKind>,
) -> CachedDiffStyledText {
    build_conflict_cached_diff_styled_text_with_source_identity(
        theme,
        text,
        None,
        word_ranges,
        query,
        language,
        syntax_mode,
        word_kind,
    )
}

pub(super) fn build_conflict_cached_diff_styled_text_with_source_identity(
    theme: AppTheme,
    text: &str,
    source_identity: Option<DiffTextSourceIdentity>,
    word_ranges: &[Range<usize>],
    query: &str,
    language: Option<DiffSyntaxLanguage>,
    syntax_mode: DiffSyntaxMode,
    word_kind: Option<crate::theme::DiffColorKind>,
) -> CachedDiffStyledText {
    let _perf_scope = perf::span(ViewPerfSpan::StyledTextBuild);
    build_cached_diff_styled_text_with_source_identity(
        theme,
        text,
        source_identity,
        word_ranges,
        query,
        language,
        syntax_mode,
        word_kind,
    )
}

pub(super) enum ConflictRowStyledTextValue {
    StableCached,
    QueryCached,
    Owned(CachedDiffStyledText),
}

#[derive(Default)]
pub(super) struct ConflictRowStyledText {
    pub(super) styled: Option<ConflictRowStyledTextValue>,
    pub(super) pending: bool,
}

impl ConflictRowStyledText {
    pub(super) fn resolve<'a>(
        &'a self,
        stable_cache: &'a conflict_resolver::ConflictSplitStyledTextCache,
        query_cache: &'a conflict_resolver::ConflictSplitStyledTextCache,
        key: (usize, ConflictPickSide),
    ) -> Option<&'a CachedDiffStyledText> {
        match self.styled.as_ref()? {
            ConflictRowStyledTextValue::StableCached => stable_cache.get(&key),
            ConflictRowStyledTextValue::QueryCached => query_cache.get(&key),
            ConflictRowStyledTextValue::Owned(styled) => Some(styled),
        }
    }
}

/// Which search wash a row wears: the current match stands out from the rest.
pub(super) fn row_emphasis(
    current_match_row: Option<usize>,
    visible_row_ix: usize,
) -> DiffSearchMatchEmphasis {
    if current_match_row == Some(visible_row_ix) {
        DiffSearchMatchEmphasis::Current
    } else {
        DiffSearchMatchEmphasis::Other
    }
}

pub(super) fn conflict_diff_query_matcher(
    query: &str,
    query_options: DiffSearchOptions,
) -> Option<DiffSearchMatcher> {
    (!query.is_empty()).then(|| DiffSearchMatcher::new(query, query_options))
}

pub(super) fn build_conflict_row_base_styled(
    theme: AppTheme,
    text: &str,
    source_identity: Option<DiffTextSourceIdentity>,
    word_ranges: &[Range<usize>],
    syntax_lang: Option<DiffSyntaxLanguage>,
    syntax_mode: DiffSyntaxMode,
    prepared_line: PreparedDiffSyntaxLine,
) -> PreparedDocumentLineStyledText {
    if prepared_line.document.is_some() {
        return build_cached_diff_styled_text_for_prepared_document_line_nonblocking(
            theme,
            text,
            word_ranges,
            "",
            DiffSyntaxConfig {
                language: syntax_lang,
                mode: syntax_mode,
            },
            None,
            prepared_line,
        );
    }

    PreparedDocumentLineStyledText::Cacheable(
        build_conflict_cached_diff_styled_text_with_source_identity(
            theme,
            text,
            source_identity,
            word_ranges,
            "",
            syntax_lang,
            syntax_mode,
            None,
        ),
    )
}

pub(super) fn conflict_display_text(
    text: &SharedString,
    styled: Option<&CachedDiffStyledText>,
    reveal_whitespace_chars: bool,
) -> SharedString {
    match styled {
        Some(_styled) if reveal_whitespace_chars => whitespace_visible_line_text(text.as_ref()),
        Some(styled) => styled.text.clone(),
        None if reveal_whitespace_chars => whitespace_visible_line_text(text.as_ref()),
        None => text.clone(),
    }
}

fn conflict_row_text_width(
    window: &mut Window,
    text: &SharedString,
    font_family: Option<&str>,
) -> Pixels {
    if text.is_empty() {
        return px(0.0);
    }

    let mut style = window.text_style();
    style.font_weight = FontWeight::NORMAL;
    if let Some(font_family) = font_family {
        style.font_family = font_family.to_string().into();
    }

    let font_size = style.font_size.to_pixels(window.rem_size()) * CONFLICT_ROW_FONT_SCALE;
    if !text.as_ref().contains(['\n', '\r']) {
        return window
            .text_system()
            .shape_line(text.clone(), font_size, &[style.to_run(text.len())], None)
            .width;
    }

    text.as_ref()
        .split(['\n', '\r'])
        .filter(|line| !line.is_empty())
        .map(|line| {
            window
                .text_system()
                .shape_line(
                    line.to_string().into(),
                    font_size,
                    &[style.to_run(line.len())],
                    None,
                )
                .width
        })
        .max_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(px(0.0))
}

pub(super) fn conflict_input_row_min_width(
    window: &mut Window,
    text: &SharedString,
    editor_font_family: &str,
    show_line_numbers: bool,
    ui_scale_percent: u32,
) -> Pixels {
    let pad = window.rem_size() * 0.5;
    let gap = pad;
    let line_no_width = if show_line_numbers {
        conflict_line_no_width(ui_scale_percent) + gap
    } else {
        px(0.0)
    };
    let row_extra = pad * 2.0 + line_no_width;
    (row_extra
        + conflict_row_text_width(window, text, Some(editor_font_family))
        + conflict_scaled_px(CONFLICT_ROW_TEXT_TRAILING_PADDING_PX, ui_scale_percent))
    .round()
}

pub(super) fn conflict_resolved_output_row_min_width(
    window: &mut Window,
    text: &SharedString,
    editor_font_family: &str,
    ui_scale_percent: u32,
) -> Pixels {
    let pad = window.rem_size() * 0.5;
    let row_extra = pad * 2.0;
    (row_extra
        + conflict_row_text_width(window, text, Some(editor_font_family))
        + conflict_scaled_px(CONFLICT_ROW_TEXT_TRAILING_PADDING_PX, ui_scale_percent))
    .round()
}

/// Width of the resolved-output line-number cell, sized to the file's digit
/// count so a short number sits snug against the marker lane instead of floating
/// across a cell wide enough for the largest line. The gutter container tracks
/// this width, so the marker stays pinned at the far-left edge and only where the
/// code column begins shifts a few px between files of very different line counts
/// (the same way any editor's line-number gutter widens with the line total).
pub(in crate::view) fn resolved_output_line_no_width(
    line_count: usize,
    ui_scale_percent: u32,
) -> Pixels {
    /// Design width of one line-number digit at the resolver's row font size.
    const DIGIT_WIDTH_PX: f32 = 8.0;
    let digits = line_count.max(1).to_string().len().max(2);
    conflict_scaled_px(digits as f32 * DIGIT_WIDTH_PX, ui_scale_percent)
}

/// Total width of the resolved-output gutter (marker lane + optional line-number
/// cell + origin badge, plus the row's horizontal padding), so the container
/// hugs its content and the badge/border sit right against the code.
pub(in crate::view) fn resolved_output_gutter_width(
    line_count: usize,
    show_line_numbers: bool,
    ui_scale_percent: u32,
) -> Pixels {
    let marker_and_badge = conflict_scaled_px(
        RESOLVED_OUTPUT_MARKER_LANE_PX + RESOLVED_OUTPUT_BADGE_PX + CONFLICT_ROW_PADDING_X_PX * 2.0,
        ui_scale_percent,
    );
    if show_line_numbers {
        marker_and_badge
            + resolved_output_line_no_width(line_count, ui_scale_percent)
            + conflict_scaled_px(RESOLVED_OUTPUT_LINE_NO_GAP_PX, ui_scale_percent)
    } else {
        marker_and_badge
    }
}

pub(super) fn render_conflict_markdown_preview_rows(
    this: &mut MainPaneView,
    range: Range<usize>,
    side: ThreeWayColumn,
    window: &mut Window,
    cx: &mut gpui::Context<MainPaneView>,
) -> Vec<AnyElement> {
    let theme = this.theme;
    let editor_font_family = crate::font_preferences::current_editor_font_family(cx);
    let Loadable::Ready(document) = this.conflict_resolver.markdown_preview.document(side) else {
        return Vec::new();
    };
    let document = Arc::clone(document);
    let viewport_width = match side {
        ThreeWayColumn::Base => {
            this.conflict_resolver_diff_scroll
                .0
                .borrow()
                .base_handle
                .bounds()
                .size
                .width
        }
        ThreeWayColumn::Ours => {
            this.conflict_preview_ours_scroll
                .0
                .borrow()
                .base_handle
                .bounds()
                .size
                .width
        }
        ThreeWayColumn::Theirs => {
            this.conflict_preview_theirs_scroll
                .0
                .borrow()
                .base_handle
                .bounds()
                .size
                .width
        }
    }
    .max(px(0.0));
    this.update_markdown_preview_horizontal_min_width(
        document.as_ref(),
        range.clone(),
        editor_font_family.as_str(),
        window,
        cx,
    );
    super::markdown_preview::render_markdown_preview_document_rows(
        document.as_ref(),
        range,
        &super::markdown_preview::MarkdownPreviewRenderContext {
            theme,
            min_width: this.diff_horizontal_content_width().max(viewport_width),
            editor_font_family: editor_font_family.into(),
            ui_scale_percent: crate::ui_scale::current(cx).percent,
            view: None,
            text_region: DiffTextRegion::Inline,
            wrap_plan: None,
            image_base_dir: None,
            query: this.markdown_preview_search_query(),
        },
    )
}

/// A diff-column line-number cell: fixed width, vertically centered, with a
/// full-height right divider separating the number gutter from the code —
/// matching the resolved-output gutter's separator.
pub(super) fn conflict_diff_line_number_cell(
    theme: AppTheme,
    line_no: SharedString,
    ui_scale_percent: u32,
) -> gpui::Div {
    div()
        .w(conflict_line_no_width(ui_scale_percent))
        .h_full()
        .flex()
        .items_center()
        .border_r_1()
        .border_color(theme.colors.stroke.default)
        .text_color(theme.colors.foreground.secondary)
        .child(line_no)
}

pub(super) fn conflict_diff_text_cell(
    text: SharedString,
    styled: Option<&CachedDiffStyledText>,
    reveal_whitespace_chars: bool,
) -> AnyElement {
    let Some(styled) = styled else {
        let display = if reveal_whitespace_chars {
            whitespace_visible_line_text(text.as_ref())
        } else {
            text
        };
        return div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(display)
            .into_any_element();
    };

    if styled.highlights.is_empty() {
        let display = if reveal_whitespace_chars {
            whitespace_visible_line_text(text.as_ref())
        } else {
            styled.text.clone()
        };
        return div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(display)
            .into_any_element();
    }

    if reveal_whitespace_chars {
        let visible = whitespace_visible_line_styled_text_for_raw(styled, text.as_ref());
        if visible.highlights.is_empty() {
            return div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .child(visible.text)
                .into_any_element();
        }
        let visible_text = visible.text;
        let visible_highlights = visible.highlights;
        return div()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .child(
                gpui::StyledText::new(visible_text)
                    .with_highlights(visible_highlights.iter().cloned()),
            )
            .into_any_element();
    }

    div()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .child(
            gpui::StyledText::new(styled.text.clone())
                .with_highlights(styled.highlights.iter().cloned()),
        )
        .into_any_element()
}

#[cfg(test)]
pub(super) fn whitespace_visible_text(text: &str) -> SharedString {
    whitespace_visible_text_and_highlights(text, &[]).0
}

#[cfg(test)]
pub(super) fn whitespace_visible_text_and_highlights(
    text: &str,
    highlights: &[(Range<usize>, gpui::HighlightStyle)],
) -> (SharedString, Vec<(Range<usize>, gpui::HighlightStyle)>) {
    super::diff_text::whitespace_visible_text_and_highlights_impl(text, highlights, false)
}

pub(super) fn resolved_output_source_badge_colors(
    theme: AppTheme,
    source: conflict_resolver::ResolvedLineSource,
) -> (gpui::Rgba, gpui::Rgba) {
    match source {
        conflict_resolver::ResolvedLineSource::A => (
            with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.68 } else { 0.56 },
            ),
            theme.colors.accent.foreground,
        ),
        conflict_resolver::ResolvedLineSource::B => (
            with_alpha(
                theme.colors.status.success.foreground,
                if theme.is_dark { 0.68 } else { 0.56 },
            ),
            theme.colors.status.success.foreground,
        ),
        conflict_resolver::ResolvedLineSource::C => (
            with_alpha(
                theme.colors.status.warning.foreground,
                if theme.is_dark { 0.68 } else { 0.56 },
            ),
            theme.colors.status.warning.foreground,
        ),
        conflict_resolver::ResolvedLineSource::Manual => (
            with_alpha(
                theme.colors.foreground.secondary,
                if theme.is_dark { 0.48 } else { 0.42 },
            ),
            theme.colors.foreground.secondary,
        ),
    }
}

fn three_way_choice_short_label(choice: conflict_resolver::ConflictChoice) -> &'static str {
    match choice {
        conflict_resolver::ConflictChoice::Base => "A",
        conflict_resolver::ConflictChoice::Ours => "B",
        conflict_resolver::ConflictChoice::Theirs => "C",
        conflict_resolver::ConflictChoice::Both => "B+C",
        _ => "ordered",
    }
}

fn two_way_side_label(side: ConflictPickSide) -> &'static str {
    match side {
        ConflictPickSide::Ours => "A",
        ConflictPickSide::Theirs => "B",
    }
}

fn two_way_choice_for_side(side: ConflictPickSide) -> conflict_resolver::ConflictChoice {
    match side {
        ConflictPickSide::Ours => conflict_resolver::ConflictChoice::Ours,
        ConflictPickSide::Theirs => conflict_resolver::ConflictChoice::Theirs,
    }
}

pub(super) fn three_way_input_row_menu_targets(
    line_ix: usize,
    conflict_ix: usize,
    choice: conflict_resolver::ConflictChoice,
) -> (
    SharedString,
    ResolverPickTarget,
    SharedString,
    ResolverPickTarget,
) {
    let label = three_way_choice_short_label(choice);
    (
        format!("Pick this line ({label})").into(),
        ResolverPickTarget::ThreeWayLine { line_ix, choice },
        format!("Pick this chunk ({label})").into(),
        ResolverPickTarget::Chunk {
            conflict_ix,
            choice,
            output_line_ix: None,
        },
    )
}

pub(super) fn two_way_split_input_row_menu_targets(
    row_ix: usize,
    conflict_ix: usize,
    side: ConflictPickSide,
) -> (
    SharedString,
    ResolverPickTarget,
    SharedString,
    ResolverPickTarget,
) {
    let side_label = two_way_side_label(side);
    let choice = two_way_choice_for_side(side);
    (
        format!("Pick this line ({side_label})").into(),
        ResolverPickTarget::TwoWaySplitLine { row_ix, side },
        format!("Pick this chunk ({side_label})").into(),
        ResolverPickTarget::Chunk {
            conflict_ix,
            choice,
            output_line_ix: None,
        },
    )
}

/// Input-row menu targets for the section 30 aligned two-way view. `row_ix` is an
/// aligned visual row (shared by both columns), so the line pick reuses the
/// aligned-row-space `ThreeWayLine` target with this side's choice.
pub(super) fn two_way_aligned_input_row_menu_targets(
    row_ix: usize,
    conflict_ix: usize,
    side: ConflictPickSide,
) -> (
    SharedString,
    ResolverPickTarget,
    SharedString,
    ResolverPickTarget,
) {
    let side_label = two_way_side_label(side);
    let choice = two_way_choice_for_side(side);
    (
        format!("Pick this line ({side_label})").into(),
        ResolverPickTarget::ThreeWayLine {
            line_ix: row_ix,
            choice,
        },
        format!("Pick this chunk ({side_label})").into(),
        ResolverPickTarget::Chunk {
            conflict_ix,
            choice,
            output_line_ix: None,
        },
    )
}

/// Whether two lines are equal once all whitespace is removed. Matches the
/// block-local `append_conflict_row_without_whitespace` semantics used to
/// downgrade whitespace-only differences to context rows.
pub(super) fn texts_equal_ignoring_whitespace(a: &str, b: &str) -> bool {
    a.chars()
        .filter(|ch| !ch.is_whitespace())
        .eq(b.chars().filter(|ch| !ch.is_whitespace()))
}

pub(super) fn split_cell_bg(
    theme: AppTheme,
    kind: worktree_core::file_diff::FileDiffRowKind,
    side: ConflictPickSide,
) -> gpui::Rgba {
    // Side-identity colours, matching the three-way view: Ours = success
    // (green), Theirs = accent (blue). A cell is tinted only when that side
    // actually has changed content on the row (Ours: Remove/Modify, Theirs:
    // Add/Modify), so unchanged padding stays transparent.
    match (kind, side) {
        (worktree_core::file_diff::FileDiffRowKind::Add, ConflictPickSide::Theirs)
        | (worktree_core::file_diff::FileDiffRowKind::Modify, ConflictPickSide::Theirs) => {
            with_alpha(
                theme.colors.accent.foreground,
                if theme.is_dark { 0.14 } else { 0.10 },
            )
        }
        (worktree_core::file_diff::FileDiffRowKind::Remove, ConflictPickSide::Ours)
        | (worktree_core::file_diff::FileDiffRowKind::Modify, ConflictPickSide::Ours) => {
            with_alpha(
                theme.colors.status.success.foreground,
                if theme.is_dark { 0.10 } else { 0.08 },
            )
        }
        _ => with_alpha(theme.colors.surface.raised, 0.0),
    }
}
