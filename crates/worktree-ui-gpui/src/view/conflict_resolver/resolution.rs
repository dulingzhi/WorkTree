//! Resolution: pick choices, autosolve summaries, and applying session
//! region resolutions to parsed marker segments.

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutosolveTraceMode {
    /// The safe rules and the subchunk split, applied automatically when the
    /// file opened. Whitespace-only, regex and history merges are never
    /// automatic — kdiff3 parity, see the section 30 auto-solve policy.
    OnOpen,
    #[cfg(test)]
    History,
}

/// Ordered pick choices for a view mode. Both the letter (`a/b/c/d`) and the
/// `Ctrl+1/2/3` shortcuts index into this list, so the key→choice mapping lives
/// in one place per mode. Note `Both` sits last, so `Ctrl+1/2/3` reaches it only
/// in two-way mode (three-way exposes it via the `d` letter pick).
fn conflict_pick_choices(view_mode: ConflictResolverViewMode) -> &'static [ConflictChoice] {
    match view_mode {
        ConflictResolverViewMode::ThreeWay => &[
            ConflictChoice::Base,
            ConflictChoice::Ours,
            ConflictChoice::Theirs,
            ConflictChoice::Both,
        ],
        ConflictResolverViewMode::TwoWayDiff => &[
            ConflictChoice::Ours,
            ConflictChoice::Theirs,
            ConflictChoice::Both,
        ],
    }
}

/// Resolve conflict quick-pick keyboard shortcuts to a concrete choice.
pub fn conflict_quick_pick_choice_for_key(
    key: &str,
    view_mode: ConflictResolverViewMode,
) -> Option<ConflictChoice> {
    let index = match key {
        "a" => 0,
        "b" => 1,
        "c" => 2,
        "d" => 3,
        _ => return None,
    };
    conflict_pick_choices(view_mode).get(index).copied()
}

/// Resolve kdiff3-compatible `Ctrl+1/2/3` pick aliases (section 30 keyboard model).
///
/// Unlike the single-letter picks these also work while the output editor is
/// focused, since they cannot collide with text input.
pub fn conflict_ctrl_pick_choice_for_key(
    key: &str,
    view_mode: ConflictResolverViewMode,
) -> Option<ConflictChoice> {
    let index = match key {
        "1" => 0,
        "2" => 1,
        "3" => 2,
        _ => return None,
    };
    conflict_pick_choices(view_mode).get(index).copied()
}

/// Build a user-facing summary for the most recent autosolve run.
///
/// The summary is shown in the resolver UI so autosolve behavior remains
/// auditable without opening command logs.
pub fn format_autosolve_trace_summary(
    mode: AutosolveTraceMode,
    unresolved_before: usize,
    unresolved_after: usize,
    stats: &worktree_state::msg::ConflictAutosolveStats,
) -> String {
    let resolved = unresolved_before.saturating_sub(unresolved_after);
    let blocks_word = if resolved == 1 { "block" } else { "blocks" };
    match mode {
        AutosolveTraceMode::OnOpen => format!(
            "Auto-solved on open: resolved {resolved} {blocks_word}, unresolved {} -> {} (pass1 {}, split {}, regex {}).",
            unresolved_before, unresolved_after, stats.pass1, stats.pass2_split, stats.regex
        ),
        #[cfg(test)]
        AutosolveTraceMode::History => format!(
            "Last autosolve (history): resolved {resolved} {blocks_word}, unresolved {} -> {} (history {}).",
            unresolved_before, unresolved_after, stats.history
        ),
    }
}

/// KDiff3-style accounting for a merge session.
///
/// Plan-backed sessions count every block classified as a conflict or delta,
/// not just the subset that still needs a user decision. The whitespace count
/// is optional because marker-only fallback sessions do not have KDiff3's
/// exact aligned-row classification available.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConflictSummaryCounts {
    pub total: usize,
    pub auto_solved: usize,
    pub unsolved: usize,
    pub whitespace_conflicts: Option<usize>,
}

impl ConflictSummaryCounts {
    fn normalized(self) -> Self {
        let unsolved = self.unsolved.min(self.total);
        Self {
            auto_solved: self.auto_solved.min(self.total.saturating_sub(unsolved)),
            unsolved,
            whitespace_conflicts: self.whitespace_conflicts.map(|count| count.min(self.total)),
            ..self
        }
    }
}

/// Format the shared total / auto-solved / unsolved report used by the toast
/// and resolver status bar.
pub fn format_conflict_summary(counts: ConflictSummaryCounts) -> String {
    let counts = counts.normalized();
    format!(
        "Total {} / auto-solved {} / unsolved {}",
        counts.total, counts.auto_solved, counts.unsolved
    )
}

/// Build the one-shot toast message pushed when a conflict file's resolver
/// opens fresh.
pub fn format_open_summary_toast(counts: ConflictSummaryCounts) -> Option<String> {
    if counts.total == 0 {
        return None;
    }
    Some(format_conflict_summary(counts))
}

/// Count a session using KDiff3's conflict-reporting convention.
///
/// With a merge plan, `total` is every stable conflict-or-delta block,
/// `unsolved` is the currently unresolved subset, and `auto_solved` is their
/// difference. Marker-only sessions retain region-based fallback accounting.
pub fn conflict_session_summary_counts(
    session: &worktree_core::conflict_session::ConflictSession,
) -> ConflictSummaryCounts {
    use worktree_core::conflict_session::ConflictRegionResolution;

    if let Some(plan) = session.merge_plan.as_ref() {
        let mut total = 0usize;
        let mut unsolved = 0usize;
        let mut whitespace_conflicts = 0usize;
        for block in plan
            .blocks
            .iter()
            .filter(|block| block.original_conflict || block.is_delta)
        {
            total += 1;
            unsolved += usize::from(!block.is_resolved());
            // KDiff3's status line reports the *unsolved* whitespace subset
            // (`getNumberOfUnsolvedConflicts(&wsc)`), so the count falls as the
            // user picks sides for them.
            whitespace_conflicts += usize::from(block.whitespace_conflict && !block.is_resolved());
        }
        return ConflictSummaryCounts {
            total,
            auto_solved: total.saturating_sub(unsolved),
            unsolved,
            whitespace_conflicts: Some(whitespace_conflicts),
        };
    }

    let auto_solved = session
        .regions
        .iter()
        .filter(|region| {
            matches!(
                &region.resolution,
                ConflictRegionResolution::AutoResolved { .. }
            )
        })
        .count();
    ConflictSummaryCounts {
        total: session.total_regions(),
        auto_solved,
        unsolved: session.unsolved_count(),
        whitespace_conflicts: None,
    }
}

/// Summarize the on-open autosolve pass from session region resolutions.
///
/// On a fresh open every resolved region is an [`AutoResolved`] one (user
/// picks cannot exist yet), so the confidence-tier breakdown can be
/// reconstructed from the applied rules. Returns `None` when nothing was
/// auto-resolved.
///
/// [`AutoResolved`]: worktree_core::conflict_session::ConflictRegionResolution::AutoResolved
pub fn on_open_autosolve_summary(
    session: &worktree_core::conflict_session::ConflictSession,
) -> Option<String> {
    use worktree_core::conflict_session::{AutosolveRule, ConflictRegionResolution};

    let mut stats = worktree_state::msg::ConflictAutosolveStats::default();
    for region in &session.regions {
        let ConflictRegionResolution::AutoResolved { rule, .. } = &region.resolution else {
            continue;
        };
        match rule {
            AutosolveRule::IdenticalSides
            | AutosolveRule::OnlyOursChanged
            | AutosolveRule::OnlyTheirsChanged
            | AutosolveRule::WhitespaceOnly => stats.pass1 += 1,
            AutosolveRule::SubchunkFullyMerged => stats.pass2_split += 1,
            AutosolveRule::RegexEquivalentSides
            | AutosolveRule::RegexOnlyTheirsChanged
            | AutosolveRule::RegexOnlyOursChanged => stats.regex += 1,
            AutosolveRule::HistoryMerged => stats.history += 1,
        }
    }

    let resolved = stats.total_resolved();
    if resolved == 0 {
        return None;
    }
    let unresolved_after = session.unsolved_count();
    Some(format_autosolve_trace_summary(
        AutosolveTraceMode::OnOpen,
        unresolved_after + resolved,
        unresolved_after,
        &stats,
    ))
}

/// Count conflict blocks whose backing session regions were auto-resolved when
/// the resolver opened.
/// Build a per-conflict autosolve trace label for the active conflict.
///
/// Returns `None` when the active conflict does not map to an auto-resolved
/// session region.
pub fn active_conflict_autosolve_trace_label(
    session: &worktree_core::conflict_session::ConflictSession,
    conflict_region_indices: &[usize],
    active_conflict: usize,
) -> Option<String> {
    use worktree_core::conflict_session::ConflictRegionResolution;

    let region_index = *conflict_region_indices.get(active_conflict)?;
    let region = session.regions.get(region_index)?;
    if let ConflictRegionResolution::AutoResolved {
        rule, confidence, ..
    } = &region.resolution
    {
        Some(format!(
            "Auto: {} ({})",
            rule.description(),
            confidence.label()
        ))
    } else {
        None
    }
}

fn choice_for_resolved_content(block: &ConflictBlock, content: &str) -> Option<ConflictChoice> {
    if !block.choice.is_empty() && content_matches_block_choice(block, content) {
        return Some(block.choice);
    }
    if content == block.ours {
        return Some(ConflictChoice::Ours);
    }
    if content == block.theirs {
        return Some(ConflictChoice::Theirs);
    }
    if block.base.as_deref().is_some_and(|base| content == base) {
        return Some(ConflictChoice::Base);
    }
    content
        .strip_prefix(block.ours.as_str())
        .is_some_and(|rest| rest == block.theirs)
        .then_some(ConflictChoice::Both)
}

fn content_matches_block_choice(block: &ConflictBlock, content: &str) -> bool {
    use worktree_core::conflict_output::ConflictOutputSource;

    let mut selected = String::new();
    for source in block.choice.iter() {
        match source {
            ConflictOutputSource::Base => {
                let Some(base) = block.base.as_deref() else {
                    return false;
                };
                selected.push_str(base);
            }
            ConflictOutputSource::Ours => selected.push_str(&block.ours),
            ConflictOutputSource::Theirs => selected.push_str(&block.theirs),
        }
    }
    selected == content
}

fn resolution_for_choice(
    choice: ConflictChoice,
    has_base: bool,
) -> worktree_core::conflict_session::ConflictRegionResolution {
    use worktree_core::conflict_output::ConflictOutputSource;
    use worktree_core::conflict_session::ConflictRegionResolution;
    use worktree_core::merge::{MergeSource, OrderedSelection};

    let selection = OrderedSelection::from_sources(choice.iter().filter_map(|source| {
        match (has_base, source) {
            (true, ConflictOutputSource::Base) => Some(MergeSource::A),
            (true, ConflictOutputSource::Ours) => Some(MergeSource::B),
            (true, ConflictOutputSource::Theirs) => Some(MergeSource::C),
            (false, ConflictOutputSource::Base) => None,
            (false, ConflictOutputSource::Ours) => Some(MergeSource::A),
            (false, ConflictOutputSource::Theirs) => Some(MergeSource::B),
        }
    }));
    if selection.is_empty() {
        ConflictRegionResolution::Unresolved
    } else {
        ConflictRegionResolution::Sources(selection)
    }
}

pub(in crate::view) fn choice_for_selection(
    selection: &worktree_core::merge::OrderedSelection,
    has_base: bool,
) -> Option<ConflictChoice> {
    use worktree_core::conflict_output::{ConflictOutputChoice, ConflictOutputSource};
    use worktree_core::merge::MergeSource;

    let mut choice = ConflictOutputChoice::empty();
    for source in selection.iter() {
        let output_source = match (has_base, source) {
            (true, MergeSource::A) => ConflictOutputSource::Base,
            (true, MergeSource::B) => ConflictOutputSource::Ours,
            (true, MergeSource::C) => ConflictOutputSource::Theirs,
            (false, MergeSource::A) => ConflictOutputSource::Ours,
            (false, MergeSource::B) => ConflictOutputSource::Theirs,
            (false, MergeSource::C) => return None,
        };
        choice.append(output_source);
    }
    Some(choice)
}

/// Derive per-region session resolution updates from the current resolved output.
///
/// This is used to persist manual resolver edits back into state without
/// requiring marker reparse in the reducer.
pub fn derive_region_resolution_updates_from_output(
    segments: &[ConflictSegment],
    block_region_indices: &[usize],
    block_map: &ResolvedOutputBlockMap,
    output_text: &str,
) -> Option<
    Vec<(
        usize,
        worktree_core::conflict_session::ConflictRegionResolution,
    )>,
> {
    use worktree_core::conflict_session::ConflictRegionResolution as R;

    if !block_map.is_valid_for(segments, output_text) {
        return None;
    }
    let mut updates = Vec::with_capacity(block_map.ranges().len());

    let mut block_ix = 0usize;
    for seg in segments {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        let content = block_map.block_slice(segments, output_text, block_ix)?;
        let region_ix = block_region_indices
            .get(block_ix)
            .copied()
            .unwrap_or(block_ix);

        let resolution = if !block.resolved
            && (content.contains(UNRESOLVED_MERGE_CONFLICT_PLACEHOLDER)
                || (!block.choice.is_empty() && content_matches_block_choice(block, content)))
        {
            R::Unresolved
        } else if content.is_empty() && block.choice.is_empty() {
            // Empty bytes alone cannot prove that an empty source was chosen.
            // Only the explicit block choice below carries that intent.
            R::ManualEdit(String::new())
        } else if let Some(choice) = choice_for_resolved_content(block, content) {
            resolution_for_choice(choice, block.base.is_some())
        } else {
            R::ManualEdit(content.to_string())
        };
        updates.push((region_ix, resolution));
        block_ix += 1;
    }

    Some(updates)
}

/// Derive per-region session resolution updates directly from marker segments.
///
/// Streamed resolved-output mode is read-only until explicit materialization,
/// so the block choice state is the source of truth and no full output string
/// needs to be assembled.
pub fn derive_region_resolution_updates_from_segments(
    segments: &[ConflictSegment],
    block_region_indices: &[usize],
) -> Vec<(
    usize,
    worktree_core::conflict_session::ConflictRegionResolution,
)> {
    use worktree_core::conflict_session::ConflictRegionResolution as R;

    let mut updates = Vec::with_capacity(conflict_count(segments));
    let mut block_ix = 0usize;
    for seg in segments {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        let region_ix = block_region_indices
            .get(block_ix)
            .copied()
            .unwrap_or(block_ix);
        let resolution = if !block.resolved {
            R::Unresolved
        } else {
            resolution_for_choice(block.choice, block.base.is_some())
        };
        updates.push((region_ix, resolution));
        block_ix += 1;
    }
    updates
}

/// Result of applying state-layer region resolutions to UI marker segments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionRegionApplyResult {
    /// Number of source regions visited/applied.
    pub applied_regions: usize,
    /// Mapping from visible block index -> source `ConflictSession` region index.
    pub block_region_indices: Vec<usize>,
}

/// Result of applying ConflictRegion choices to a plan projection while
/// retaining exact semantic block identities for every visible marker.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlanSessionRegionApplyResult {
    pub block_region_indices: Vec<usize>,
    pub block_plan_indices: Vec<usize>,
}

/// Apply marker-backed region choices to a projection whose marker blocks are
/// identified by merge-plan block index.
///
/// A plan-only automatic delta can become unresolved after a source toggle.
/// Such a block has no ConflictRegion, so it is retained untouched and given a
/// synthetic, out-of-range grouping key while its plan identity remains exact.
pub fn apply_plan_session_region_resolutions_with_index_map(
    segments: &mut Vec<ConflictSegment>,
    session: &worktree_core::conflict_session::ConflictSession,
    projected_plan_blocks: &[usize],
) -> Option<PlanSessionRegionApplyResult> {
    if conflict_count(segments) != projected_plan_blocks.len() {
        return None;
    }

    let mut marker_index = 0usize;
    let mut block_region_indices = Vec::new();
    let mut block_plan_indices = Vec::new();
    let mut synced = Vec::with_capacity(segments.len());
    for segment in segments.drain(..) {
        match segment {
            ConflictSegment::Text(text) => append_text_segment(&mut synced, text),
            ConflictSegment::Block(mut block) => {
                let block_index = projected_plan_blocks[marker_index];
                marker_index += 1;
                // Only the plan applies kdiff3's per-row whitespace rule, so
                // carry its verdict onto the display block here rather than
                // re-deriving a weaker one from the block text.
                block.whitespace_only = session
                    .merge_plan
                    .as_ref()
                    .and_then(|plan| plan.blocks.get(block_index))
                    .is_some_and(|plan_block| plan_block.whitespace_conflict);
                let region_index = session
                    .region_plan_blocks
                    .iter()
                    .position(|candidate| *candidate == block_index);
                if let Some(region_index) = region_index
                    && let Some(region) = session.regions.get(region_index)
                    && let Some(materialized) =
                        apply_region_resolution_to_block(&mut block, &region.resolution)
                {
                    append_text_segment(&mut synced, materialized);
                    continue;
                }

                synced.push(ConflictSegment::Block(block));
                block_plan_indices.push(block_index);
                block_region_indices.push(region_index.unwrap_or_else(|| {
                    // Real regions occupy 0..len. Plan-only keys start at len
                    // and stay unique by semantic block index.
                    session.regions.len().saturating_add(block_index)
                }));
            }
        }
    }
    *segments = synced;
    Some(PlanSessionRegionApplyResult {
        block_region_indices,
        block_plan_indices,
    })
}

/// Build a default visible block -> region index mapping by position.
pub fn sequential_conflict_region_indices(segments: &[ConflictSegment]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut conflict_ix = 0usize;
    for seg in segments {
        if matches!(seg, ConflictSegment::Block(_)) {
            out.push(conflict_ix);
            conflict_ix += 1;
        }
    }
    out
}

fn apply_region_resolution_to_block(
    block: &mut ConflictBlock,
    resolution: &worktree_core::conflict_session::ConflictRegionResolution,
) -> Option<String> {
    use worktree_core::conflict_session::ConflictRegionResolution as R;

    match resolution {
        R::Unresolved => {
            block.choice = ConflictChoice::empty();
            block.resolved = false;
            None
        }
        R::PickBase => {
            if block.base.is_some() {
                block.choice = ConflictChoice::Base;
                block.resolved = true;
            } else {
                block.choice = ConflictChoice::empty();
                block.resolved = false;
            }
            None
        }
        R::PickOurs => {
            block.choice = ConflictChoice::Ours;
            block.resolved = true;
            None
        }
        R::PickTheirs => {
            block.choice = ConflictChoice::Theirs;
            block.resolved = true;
            None
        }
        R::PickBoth => {
            block.choice = ConflictChoice::Both;
            block.resolved = true;
            None
        }
        R::Sources(selection) => {
            if let Some(choice) = choice_for_selection(selection, block.base.is_some()) {
                block.choice = choice;
                block.resolved = !choice.is_empty();
            } else {
                block.choice = ConflictChoice::empty();
                block.resolved = false;
            }
            None
        }
        R::ManualEdit(text) => {
            if let Some(choice) = choice_for_resolved_content(block, text) {
                block.choice = choice;
                block.resolved = true;
                return None;
            }
            Some(text.clone())
        }
        R::AutoResolved { content, .. } => {
            if let Some(choice) = choice_for_resolved_content(block, content) {
                block.choice = choice;
                block.resolved = true;
                return None;
            }
            Some(content.clone())
        }
    }
}

/// Apply ordered per-block resolutions to parsed UI marker segments.
///
/// This is used by save/export paths that derive resolutions from the current
/// resolved-output buffer and need to keep manual edits as plain text while
/// preserving untouched unresolved blocks.
pub(in crate::view) fn apply_ordered_region_resolutions(
    segments: &mut Vec<ConflictSegment>,
    resolutions: &[worktree_core::conflict_session::ConflictRegionResolution],
) -> usize {
    if segments.is_empty() || resolutions.is_empty() {
        return 0;
    }

    let mut applied = 0usize;
    let mut block_ix = 0usize;
    let mut synced: Vec<ConflictSegment> = Vec::with_capacity(segments.len());

    for seg in segments.drain(..) {
        match seg {
            ConflictSegment::Text(text) => append_text_segment(&mut synced, text),
            ConflictSegment::Block(mut block) => {
                if let Some(resolution) = resolutions.get(block_ix) {
                    if let Some(materialized_text) =
                        apply_region_resolution_to_block(&mut block, resolution)
                    {
                        append_text_segment(&mut synced, materialized_text);
                    } else {
                        synced.push(ConflictSegment::Block(block));
                    }
                    applied += 1;
                } else {
                    synced.push(ConflictSegment::Block(block));
                }
                block_ix += 1;
            }
        }
    }

    *segments = synced;
    applied
}

/// Apply state-layer region resolutions to parsed UI marker segments.
///
/// This allows resolver rebuilds to preserve choices tracked in
/// `RepoState.conflict_state.conflict_session`, and materializes manual/auto-resolved
/// non-side-pick text into plain `Text` segments when needed.
///
/// Returns how many conflict regions were applied.
#[cfg(test)]
pub fn apply_session_region_resolutions(
    segments: &mut Vec<ConflictSegment>,
    regions: &[worktree_core::conflict_session::ConflictRegion],
) -> usize {
    apply_session_region_resolutions_with_index_map(segments, regions).applied_regions
}

/// Like [`apply_session_region_resolutions`] but also returns a visible block
/// index map back to the original `ConflictSession` region indices.
pub fn apply_session_region_resolutions_with_index_map(
    segments: &mut Vec<ConflictSegment>,
    regions: &[worktree_core::conflict_session::ConflictRegion],
) -> SessionRegionApplyResult {
    if segments.is_empty() {
        return SessionRegionApplyResult::default();
    }
    if regions.is_empty() {
        return SessionRegionApplyResult {
            applied_regions: 0,
            block_region_indices: sequential_conflict_region_indices(segments),
        };
    }

    let mut applied = 0usize;
    let mut conflict_ix = 0usize;
    let mut block_region_indices = Vec::new();
    let mut synced: Vec<ConflictSegment> = Vec::with_capacity(segments.len());

    for seg in segments.drain(..) {
        match seg {
            ConflictSegment::Text(text) => append_text_segment(&mut synced, text),
            ConflictSegment::Block(mut block) => {
                if let Some(region) = regions.get(conflict_ix) {
                    if let Some(materialized_text) =
                        apply_region_resolution_to_block(&mut block, &region.resolution)
                    {
                        append_text_segment(&mut synced, materialized_text);
                    } else {
                        synced.push(ConflictSegment::Block(block));
                        block_region_indices.push(conflict_ix);
                    }
                    applied += 1;
                } else {
                    synced.push(ConflictSegment::Block(block));
                    block_region_indices.push(conflict_ix);
                }
                conflict_ix += 1;
            }
        }
    }

    *segments = synced;
    SessionRegionApplyResult {
        applied_regions: applied,
        block_region_indices,
    }
}

pub fn conflict_count(segments: &[ConflictSegment]) -> usize {
    segments
        .iter()
        .filter(|s| matches!(s, ConflictSegment::Block(_)))
        .count()
}

/// Count how many conflict blocks have been explicitly resolved.
pub fn resolved_conflict_count(segments: &[ConflictSegment]) -> usize {
    segments
        .iter()
        .filter(|s| matches!(s, ConflictSegment::Block(b) if b.resolved))
        .count()
}

/// Compute effective conflict counters for resolver UI state.
///
/// Marker segments are authoritative for text-based conflict flows. For
/// non-marker strategies (binary side-pick / keep-delete / decision-only),
/// callers can pass state-layer session counters as a fallback.
pub fn effective_conflict_counts(
    segments: &[ConflictSegment],
    session_counts: Option<(usize, usize)>,
) -> (usize, usize) {
    let total = conflict_count(segments);
    if total > 0 {
        return (total, resolved_conflict_count(segments));
    }
    if let Some((session_total, session_resolved)) = session_counts {
        return (session_total, session_resolved.min(session_total));
    }
    (0, 0)
}

/// Return conflict indices for currently unresolved blocks in queue order.
#[cfg(test)]
pub fn unresolved_conflict_indices(segments: &[ConflictSegment]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut conflict_ix = 0usize;
    for seg in segments {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        if !block.resolved {
            out.push(conflict_ix);
        }
        conflict_ix += 1;
    }
    out
}

/// Apply a choice to all unresolved conflict blocks.
///
/// Already-resolved blocks are preserved. Choosing `Base` skips unresolved
/// 2-way blocks that don't have an ancestor section.
///
/// Returns the number of blocks updated.
#[cfg(test)]
pub fn apply_choice_to_unresolved_segments(
    segments: &mut [ConflictSegment],
    choice: ConflictChoice,
) -> usize {
    let mut updated = 0usize;
    for seg in segments {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        if block.resolved {
            continue;
        }
        if matches!(choice, ConflictChoice::Base) && block.base.is_none() {
            continue;
        }
        block.choice = choice;
        block.resolved = true;
        updated += 1;
    }
    updated
}

/// Find the next unresolved conflict index after `current`.
/// Wraps around to the first unresolved conflict.
#[cfg(test)]
pub fn next_unresolved_conflict_index(
    segments: &[ConflictSegment],
    current: usize,
) -> Option<usize> {
    let unresolved = unresolved_conflict_indices(segments);
    unresolved
        .iter()
        .copied()
        .find(|&ix| ix > current)
        .or_else(|| unresolved.first().copied())
}

/// Find the previous unresolved conflict index before `current`.
/// Wraps around to the last unresolved conflict.
#[cfg(test)]
pub fn prev_unresolved_conflict_index(
    segments: &[ConflictSegment],
    current: usize,
) -> Option<usize> {
    let unresolved = unresolved_conflict_indices(segments);
    unresolved
        .iter()
        .rev()
        .copied()
        .find(|&ix| ix < current)
        .or_else(|| unresolved.last().copied())
}

/// Apply safe auto-resolve rules (Pass 1) to all unresolved conflict blocks.
///
/// Safe rules:
/// 1. `ours == theirs` — both sides made the same change → pick ours.
/// 2. `ours == base` and `theirs != base` — only theirs changed → pick theirs.
/// 3. `theirs == base` and `ours != base` — only ours changed → pick ours.
/// 4. (if `whitespace_normalize`) whitespace-only difference → pick ours.
///
/// Returns the number of blocks auto-resolved.
#[cfg(test)]
pub fn auto_resolve_segments(segments: &mut [ConflictSegment]) -> usize {
    auto_resolve_segments_with_options(segments, false)
}

/// Like [`auto_resolve_segments`] but with an optional whitespace-normalization toggle.
///
/// Segment-based autosolve is now test-only: the live on-open path uses the
/// session-based `apply_autosolve_to_session` in worktree-state, and the
/// manual re-trigger button was removed.
#[cfg(test)]
pub fn auto_resolve_segments_with_options(
    segments: &mut [ConflictSegment],
    whitespace_normalize: bool,
) -> usize {
    use worktree_core::conflict_session::{AutosolvePickSide, safe_auto_resolve_pick};

    let mut count = 0;
    for seg in segments.iter_mut() {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        if block.resolved {
            continue;
        }

        let Some((_, pick)) = safe_auto_resolve_pick(
            block.base.as_deref(),
            &block.ours,
            &block.theirs,
            whitespace_normalize,
        ) else {
            continue;
        };

        block.choice = match pick {
            AutosolvePickSide::Ours => ConflictChoice::Ours,
            AutosolvePickSide::Theirs => ConflictChoice::Theirs,
        };
        block.resolved = true;
        count += 1;
    }
    count
}

/// Apply Pass 3 regex-assisted auto-resolve rules (opt-in) to unresolved blocks.
///
/// This mode uses regex normalization rules from core and only performs
/// side-picks (`Ours` / `Theirs`), never synthetic text rewrites.
#[cfg(test)]
pub fn auto_resolve_segments_regex(
    segments: &mut [ConflictSegment],
    options: &worktree_core::conflict_session::RegexAutosolveOptions,
) -> usize {
    use worktree_core::conflict_session::{AutosolvePickSide, regex_assisted_auto_resolve_pick};

    let mut count = 0;
    for seg in segments.iter_mut() {
        let ConflictSegment::Block(block) = seg else {
            continue;
        };
        if block.resolved {
            continue;
        }

        let Some((_, pick)) = regex_assisted_auto_resolve_pick(
            block.base.as_deref(),
            &block.ours,
            &block.theirs,
            options,
        ) else {
            continue;
        };

        block.choice = match pick {
            AutosolvePickSide::Ours => ConflictChoice::Ours,
            AutosolvePickSide::Theirs => ConflictChoice::Theirs,
        };
        block.resolved = true;
        count += 1;
    }
    count
}

/// Apply history-aware auto-resolve to unresolved conflict blocks.
///
/// Detects history/changelog sections and merges entries by deduplication.
/// When a block is resolved by history merge, it is replaced with a `Text`
/// segment containing the merged content.
///
/// Returns the number of blocks resolved.
#[cfg(test)]
pub fn auto_resolve_segments_history(
    segments: &mut Vec<ConflictSegment>,
    options: &worktree_core::conflict_session::HistoryAutosolveOptions,
) -> usize {
    let mut block_region_indices = sequential_conflict_region_indices(segments);
    auto_resolve_segments_history_with_region_indices(segments, options, &mut block_region_indices)
}

/// Like [`auto_resolve_segments_history`] but keeps block->region mappings in sync.
#[cfg(test)]
pub fn auto_resolve_segments_history_with_region_indices(
    segments: &mut Vec<ConflictSegment>,
    options: &worktree_core::conflict_session::HistoryAutosolveOptions,
    block_region_indices: &mut Vec<usize>,
) -> usize {
    use worktree_core::conflict_session::history_merge_region;

    let mut new_segments = Vec::with_capacity(segments.len());
    let mut new_block_region_indices = Vec::with_capacity(block_region_indices.len());
    let mut block_ix = 0usize;
    let mut count = 0;

    for seg in segments.drain(..) {
        match seg {
            ConflictSegment::Block(block) => {
                let region_ix = block_region_indices
                    .get(block_ix)
                    .copied()
                    .unwrap_or(block_ix);
                block_ix += 1;
                if !block.resolved
                    && let Some(merged) = history_merge_region(
                        block.base.as_deref(),
                        &block.ours,
                        &block.theirs,
                        options,
                    )
                {
                    // Merge adjacent Text segments for cleanliness.
                    if let Some(ConflictSegment::Text(prev)) = new_segments.last_mut() {
                        prev.push_str(&merged);
                    } else {
                        new_segments.push(ConflictSegment::Text(merged.into()));
                    }
                    count += 1;
                    continue;
                }
                new_segments.push(ConflictSegment::Block(block));
                new_block_region_indices.push(region_ix);
            }
            other => new_segments.push(other),
        }
    }

    *segments = new_segments;
    *block_region_indices = new_block_region_indices;
    count
}

/// Apply Pass 2 (heuristic subchunk splitting) to unresolved conflict blocks.
///
/// For each unresolved block that has a base, attempts to split it into
/// line-level subchunks via 3-way diff/merge. Non-conflicting subchunks
/// become `Text` segments; remaining conflicts become smaller `Block` segments.
///
/// Returns the number of original blocks that were split.
#[cfg(test)]
pub fn auto_resolve_segments_pass2(segments: &mut Vec<ConflictSegment>) -> usize {
    let mut block_region_indices = sequential_conflict_region_indices(segments);
    auto_resolve_segments_pass2_with_region_indices(segments, &mut block_region_indices)
}

/// Like [`auto_resolve_segments_pass2`] but keeps block->region mappings in sync.
#[cfg(test)]
pub fn auto_resolve_segments_pass2_with_region_indices(
    segments: &mut Vec<ConflictSegment>,
    block_region_indices: &mut Vec<usize>,
) -> usize {
    use worktree_core::conflict_session::{Subchunk, split_conflict_into_subchunks};

    let mut new_segments = Vec::with_capacity(segments.len());
    let mut new_block_region_indices = Vec::with_capacity(block_region_indices.len());
    let mut block_ix = 0usize;
    let mut split_count = 0;

    for seg in segments.drain(..) {
        match seg {
            ConflictSegment::Block(block) => {
                let region_ix = block_region_indices
                    .get(block_ix)
                    .copied()
                    .unwrap_or(block_ix);
                block_ix += 1;
                if !block.resolved
                    && let Some(base) = block.base.as_deref()
                    && let Some(subchunks) =
                        split_conflict_into_subchunks(base, &block.ours, &block.theirs)
                {
                    split_count += 1;
                    for subchunk in subchunks {
                        match subchunk {
                            Subchunk::Resolved(text) => {
                                // Merge adjacent Text segments for cleanliness.
                                if let Some(ConflictSegment::Text(prev)) = new_segments.last_mut() {
                                    prev.push_str(&text);
                                } else {
                                    new_segments.push(ConflictSegment::Text(text.into()));
                                }
                            }
                            Subchunk::Conflict { base, ours, theirs } => {
                                new_segments.push(ConflictSegment::Block(ConflictBlock {
                                    base: Some(base.into()),
                                    ours: ours.into(),
                                    theirs: theirs.into(),
                                    choice: ConflictChoice::empty(),
                                    resolved: false,
                                    // Every row of a whitespace-only block is
                                    // whitespace-only, so each subchunk of it
                                    // is too. kdiff3 clears the flag the same
                                    // way when a split lands a real change in
                                    // a block (MergeEditLine.h join/append).
                                    whitespace_only: block.whitespace_only,
                                }));
                                new_block_region_indices.push(region_ix);
                            }
                        }
                    }
                    // If all subchunks resolved, no Block segments remain
                    // from this split (all became Text above).
                    continue;
                }
                new_segments.push(ConflictSegment::Block(block));
                new_block_region_indices.push(region_ix);
            }
            other => new_segments.push(other),
        }
    }

    *segments = new_segments;
    *block_region_indices = new_block_region_indices;
    split_count
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConflictStageSafetyCheck {
    pub has_conflict_markers: bool,
    pub unresolved_blocks: usize,
}

impl ConflictStageSafetyCheck {
    pub fn blocks_save(self) -> bool {
        self.has_conflict_markers || self.unresolved_blocks > 0
    }
}

/// Compute stage-safety status for the current conflict resolver output/state.
///
/// This gate is stricter than marker-only checks: unresolved conflict blocks
/// still block the save even if the current output text no longer contains
/// marker lines.
pub fn conflict_stage_safety_check(
    output_text: &str,
    segments: &[ConflictSegment],
    block_map: &ResolvedOutputBlockMap,
) -> ConflictStageSafetyCheck {
    use worktree_core::conflict_session::ConflictRegionResolution;

    // The editor is intentionally not synchronized into session state on
    // every keystroke. Derive the effective resolutions from its current
    // contents so a manual replacement can enable Save, which then performs
    // the actual synchronization.
    let unresolved_blocks =
        derive_region_resolution_updates_from_output(segments, &[], block_map, output_text)
            .map(|updates| {
                updates
                    .iter()
                    .filter(|(_, resolution)| {
                        matches!(resolution, ConflictRegionResolution::Unresolved)
                    })
                    .count()
            })
            .unwrap_or_else(|| {
                // Ownership validation failed. Treat every displayed block as
                // unresolved so Save fails closed instead of guessing from
                // repeated context anchors.
                conflict_count(segments)
            });
    ConflictStageSafetyCheck {
        has_conflict_markers: text_contains_conflict_markers(output_text),
        unresolved_blocks,
    }
}
