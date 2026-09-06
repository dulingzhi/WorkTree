//! Context-menu and popover entry points: opening the conflict-resolver menus
//! and forwarding popovers to the root view.
use super::super::helpers::{
    conflict_group_selected_choices_for_ix, conflict_resolver_output_context_line,
    line_start_offset_for_index, resolved_output_marker_for_line,
};
use super::*;
use crate::view::panes::PaneChromeExt;

impl PaneChromeExt for MainPaneView {
    fn root_view(&self) -> &WeakEntity<WorkTreeView> {
        &self.root_view
    }

    fn theme_slot(&mut self) -> &mut AppTheme {
        &mut self.theme
    }
}

impl MainPaneView {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::view) fn open_conflict_resolver_input_row_context_menu(
        &mut self,
        invoker: SharedString,
        line_label: SharedString,
        line_target: ResolverPickTarget,
        chunk_label: SharedString,
        chunk_target: ResolverPickTarget,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.activate_context_menu_invoker(invoker, cx);
        self.open_popover_at(
            PopoverKind::ConflictResolverInputRowMenu {
                line_label,
                line_target,
                chunk_label,
                chunk_target,
            },
            anchor,
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::view) fn open_conflict_resolver_chunk_context_menu(
        &mut self,
        invoker: SharedString,
        conflict_ix: usize,
        has_base: bool,
        is_three_way: bool,
        selected_choices: Vec<conflict_resolver::ConflictChoice>,
        output_line_ix: Option<usize>,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.activate_context_menu_invoker(invoker, cx);
        // Opening the chunk menu selects that conflict and brings the
        // *other* pane to it — the pane the user right-clicked is already in
        // view and must not jump under the open menu. Reveals are non-strict:
        // nothing scrolls when the target rows are already fully visible.
        self.conflict_resolver_select_conflict(conflict_ix, cx);
        if output_line_ix.is_some() {
            // Invoked from the resolved output: reveal the source columns.
            if let Some(vi) = self.conflict_resolver_visible_ix_for_conflict(conflict_ix) {
                self.conflict_resolver_reveal_all_columns(vi);
            }
        } else {
            // Invoked from a source column: reveal the resolved output chunk.
            let output_text = (!self.conflict_resolved_output_is_streamed()).then(|| {
                self.conflict_resolver_input
                    .read_with(cx, |input, _| input.text().to_string())
            });
            let line_count = output_text
                .as_ref()
                .map(|text| text.split('\n').count().max(1))
                .unwrap_or_else(|| self.conflict_resolved_preview_line_count.max(1));
            if let Some(line) = self.conflict_resolver_output_line_for_conflict(
                conflict_ix,
                output_text.as_deref().unwrap_or(""),
            ) {
                self.conflict_resolver_reveal_resolved_output_line(line, line_count);
            }
        }
        let split_selection_rows = self.conflict_resolver_split_selection_row_count(conflict_ix);
        let (join_previous_region, join_next_region) =
            self.conflict_resolver_join_region_targets(conflict_ix);
        self.open_popover_at(
            PopoverKind::ConflictResolverChunkMenu {
                conflict_ix,
                has_base,
                is_three_way,
                selected_choices,
                output_line_ix,
                split_selection_rows,
                join_previous_region,
                join_next_region,
                alignment_marked_columns: self.conflict_resolver_alignment_marked_columns(),
                has_manual_alignments: self.conflict_resolver_has_manual_alignments(),
                output_is_protected: self.conflict_resolver.output_is_protected,
            },
            anchor,
            window,
            cx,
        );
    }

    pub(in crate::view) fn conflict_resolver_selected_choices_for_conflict_ix(
        &self,
        conflict_ix: usize,
    ) -> Vec<conflict_resolver::ConflictChoice> {
        conflict_group_selected_choices_for_ix(
            &self.conflict_resolver.marker_segments,
            &self.conflict_resolver.conflict_region_indices,
            conflict_ix,
        )
    }

    pub(in crate::view) fn conflict_resolver_has_base_for_conflict_ix(
        &self,
        conflict_ix: usize,
    ) -> bool {
        self.conflict_resolver
            .marker_segments
            .iter()
            .filter_map(|seg| match seg {
                conflict_resolver::ConflictSegment::Block(block) => Some(block.base.is_some()),
                _ => None,
            })
            .nth(conflict_ix)
            .unwrap_or(false)
    }

    pub(in crate::view) fn conflict_resolver_split_selection_row_count(
        &self,
        conflict_ix: usize,
    ) -> Option<usize> {
        let selection = self.conflict_resolver.row_selection?;
        if selection.selecting || selection.conflict_ix != conflict_ix {
            return None;
        }
        self.conflict_resolver.split_boundaries_for_selection()?;
        Some(selection.row_range().count())
    }

    fn conflict_resolver_join_region_targets(
        &self,
        conflict_ix: usize,
    ) -> (
        Option<ConflictResolverJoinTarget>,
        Option<ConflictResolverJoinTarget>,
    ) {
        let Some(region_index) = self
            .conflict_resolver
            .conflict_region_indices
            .get(conflict_ix)
            .copied()
        else {
            return (None, None);
        };
        if self
            .conflict_resolver
            .conflict_region_indices
            .iter()
            .filter(|&&index| index == region_index)
            .take(2)
            .count()
            != 1
        {
            return (None, None);
        }
        let Some(repo_id) = self
            .conflict_resolver
            .repo_id
            .or_else(|| self.active_repo_id())
        else {
            return (None, None);
        };
        let Some(path) = self.conflict_resolver.dispatch_path() else {
            return (None, None);
        };
        let Some(repo) = self.state.repos.iter().find(|repo| repo.id == repo_id) else {
            return (None, None);
        };
        if repo.conflict_state.conflict_rev != self.conflict_resolver.conflict_rev {
            return (None, None);
        }
        let Some(session) = repo.conflict_state.conflict_session.as_ref() else {
            return (None, None);
        };
        if session.path != path.as_path()
            || session.strategy
                != worktree_core::conflict_session::ConflictResolverStrategy::FullTextResolver
            || region_index >= session.regions.len()
        {
            return (None, None);
        }

        let target = |first_region_index| ConflictResolverJoinTarget {
            repo_id,
            path: path.clone(),
            conflict_rev: repo.conflict_state.conflict_rev,
            first_region_index,
        };
        let visible_ix_for_unique_region = |wanted: usize| {
            let mut matches = self
                .conflict_resolver
                .conflict_region_indices
                .iter()
                .enumerate()
                .filter_map(|(ix, &region)| (region == wanted).then_some(ix));
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        };
        let previous = region_index.checked_sub(1).and_then(|previous_region| {
            let previous_ix = visible_ix_for_unique_region(previous_region)?;
            (previous_ix.checked_add(1) == Some(conflict_ix)
                && self
                    .conflict_resolver
                    .conflict_blocks_have_joinable_context(previous_ix, conflict_ix))
            .then(|| target(previous_region))
        });
        let next = region_index.checked_add(1).and_then(|next_region| {
            if next_region >= session.regions.len() {
                return None;
            }
            let next_ix = visible_ix_for_unique_region(next_region)?;
            (conflict_ix.checked_add(1) == Some(next_ix)
                && self
                    .conflict_resolver
                    .conflict_blocks_have_joinable_context(conflict_ix, next_ix))
            .then(|| target(region_index))
        });
        (previous, next)
    }

    pub(in crate::view) fn open_conflict_resolver_output_context_menu(
        &mut self,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let (selected_text, cursor_offset, clicked_offset, content) =
            self.conflict_resolver_input.read_with(cx, |i, _| {
                (
                    i.selected_text(),
                    i.cursor_offset(),
                    i.offset_for_position(anchor),
                    i.text().to_string(),
                )
            });
        let context_line =
            conflict_resolver_output_context_line(&content, cursor_offset, Some(clicked_offset));

        self.open_conflict_resolver_output_context_menu_at_line(
            context_line,
            selected_text,
            content,
            anchor,
            window,
            cx,
        );
    }

    pub(in crate::view) fn open_conflict_resolver_output_context_menu_for_line(
        &mut self,
        line_ix: usize,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.conflict_resolved_output_is_streamed() {
            let context_line =
                line_ix.min(self.conflict_resolved_preview_line_count.saturating_sub(1));
            self.open_conflict_resolver_output_context_menu_at_line(
                context_line,
                None,
                String::new(),
                anchor,
                window,
                cx,
            );
            return;
        }

        let content = self
            .conflict_resolver_input
            .read_with(cx, |i, _| i.text().to_string());
        let context_line = line_ix.min(self.conflict_resolved_preview_line_count.saturating_sub(1));
        let cursor_offset = line_start_offset_for_index(
            self.conflict_resolved_preview_line_starts.as_ref(),
            content.len(),
            context_line,
        );
        self.conflict_resolver_input.update(cx, |input, cx| {
            input.set_cursor_offset(cursor_offset, cx);
        });

        self.open_conflict_resolver_output_context_menu_at_line(
            context_line,
            None,
            content,
            anchor,
            window,
            cx,
        );
    }

    fn open_conflict_resolver_output_context_menu_at_line(
        &mut self,
        context_line: usize,
        selected_text: Option<String>,
        content: String,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let conflict_marker = if self.conflict_resolved_output_is_streamed() {
            self.conflict_resolver
                .resolved_outline
                .markers
                .get(context_line)
                .copied()
                .flatten()
        } else {
            resolved_output_marker_for_line(
                &self.conflict_resolver.marker_segments,
                &content,
                context_line,
                &self.conflict_resolved_output_block_map,
            )
        };
        if let Some(marker) = conflict_marker {
            let is_three_way = self.conflict_resolver.view_mode
                == conflict_resolver::ConflictResolverViewMode::ThreeWay;
            let selected_choices =
                self.conflict_resolver_selected_choices_for_conflict_ix(marker.conflict_ix);
            let has_base = self.conflict_resolver_has_base_for_conflict_ix(marker.conflict_ix);
            let invoker: SharedString = format!(
                "resolver_output_chunk_menu_{}_{}",
                marker.conflict_ix, context_line
            )
            .into();
            self.open_conflict_resolver_chunk_context_menu(
                invoker,
                marker.conflict_ix,
                has_base,
                is_three_way,
                selected_choices,
                Some(context_line),
                anchor,
                window,
                cx,
            );
            return;
        }

        let is_three_way = self.conflict_resolver.view_mode
            == conflict_resolver::ConflictResolverViewMode::ThreeWay;

        let (has_source_a, has_source_b, has_source_c) = if is_three_way {
            (
                self.conflict_resolver
                    .three_way_has_line(ThreeWayColumn::Base, context_line),
                self.conflict_resolver
                    .three_way_has_line(ThreeWayColumn::Ours, context_line),
                self.conflict_resolver
                    .three_way_has_line(ThreeWayColumn::Theirs, context_line),
            )
        } else {
            {
                let row = self
                    .conflict_resolver
                    .two_way_split_row_by_source(context_line);
                (
                    row.as_ref().and_then(|r| r.old.as_ref()).is_some(),
                    row.as_ref().and_then(|r| r.new.as_ref()).is_some(),
                    false,
                )
            }
        };

        self.open_popover_at(
            PopoverKind::ConflictResolverOutputMenu {
                cursor_line: context_line,
                selected_text,
                has_source_a,
                has_source_b,
                has_source_c,
                is_three_way,
            },
            anchor,
            window,
            cx,
        );
    }
}
