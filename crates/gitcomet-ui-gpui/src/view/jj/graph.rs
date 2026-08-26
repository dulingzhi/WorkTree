//! The change list's lane graph (#89): the same lane layout and painter
//! as the git history list, driven by jj change parents instead of commit
//! parents. The model side is pure (`change_graph_rows`,
//! `graph_column_width_px`) so topology is testable without a window; the
//! one interactive piece (`render_graph_cell`) wraps a row's `GraphRow` in
//! a `canvas` element whose paint closure reuses `paint_history_graph`
//! verbatim — same lane pitch, elbows, node glyphs and selected-lane wash
//! as the git graph.

use super::*;

use crate::view::history_graph::{self, GraphRow};
use crate::view::rows::history_graph_paint::{SelectedLane, paint_history_graph, selected_lane_at};
use gitcomet_jj_core::JjChange;
use gitcomet_state::jj_store::JjRepoState;

/// The changes the list draws: everything loaded except @, which the
/// working-copy card pins above the list. Kept as the single ordering
/// shared by the row view-models and the graph rows — index `ix` of one is
/// index `ix` of the other.
pub(super) fn listed_changes(repo: &JjRepoState) -> Vec<&JjChange> {
    repo.changes
        .iter()
        .filter(|change| !change.is_working_copy)
        .collect()
}

/// Whether a change carries a local bookmark (jj renders remote halves as
/// `name@remote`, so a bare name is the local one). Local bookmarks are
/// the jj equivalent of the git graph's branch heads: they get their own
/// lane colour rather than inheriting the descendant lane's.
fn has_local_bookmark(change: &JjChange) -> bool {
    change
        .bookmarks
        .iter()
        .any(|bookmark| !bookmark.contains('@'))
}

/// The lane layout for the listed changes. `@` is not in the list, so no
/// active-head target is passed — the seeded lane defaults to the newest
/// row, which reads as "the line @ sits on continues down through the
/// list". Parents outside the loaded page have no row, so their lanes end
/// (see `history_graph::compute_graph_rows`).
pub(super) fn change_graph_rows(repo: &JjRepoState, theme: AppTheme) -> Vec<GraphRow> {
    let changes = listed_changes(repo);
    let heads: Vec<&str> = changes
        .iter()
        .filter(|change| has_local_bookmark(change))
        .map(|change| change.change_id.0.as_str())
        .collect();
    history_graph::compute_graph_rows(&changes, theme, heads, None)
}

/// The graph cell's width: one lane pitch per drawn column plus the
/// painter's side margins, clamped the way the git history clamps its
/// auto width. Past the clamp the cell clips (`overflow_hidden`), which
/// drops far-right lanes the same way a narrowed git graph column does.
pub(super) fn graph_column_width_px(graph: &[GraphRow]) -> f32 {
    let columns = graph
        .iter()
        .map(|row| row.lanes_now.len().max(row.lanes_next.len()))
        .max()
        .unwrap_or(0) as f32;
    (HISTORY_GRAPH_MARGIN_X_PX * 2.0 + HISTORY_GRAPH_COL_GAP_PX * columns)
        .min(crate::view::HISTORY_COL_GRAPH_MAX_PX)
        .max(HISTORY_GRAPH_MARGIN_X_PX * 2.0 + HISTORY_GRAPH_COL_GAP_PX)
}

/// The lane the selection sits on, if the selected change is in the list —
/// the one lane that keeps full colour while every other lane washes out.
pub(super) fn selected_lane_for(
    graph: &[GraphRow],
    repo: &JjRepoState,
    selected: Option<&gitcomet_jj_core::ChangeId>,
) -> Option<SelectedLane> {
    let selected = selected?;
    let ix = listed_changes(repo)
        .iter()
        .position(|change| change.change_id == *selected)?;
    let row = graph.get(ix)?;
    selected_lane_at(graph, ix, row.node_color_ix)
}

/// What the row is painted over, tints included. Only the merge glyph's
/// knockout actually reads this; a hover tint is not trackable from a
/// canvas child, so on a hovered merge row the knockout can lag one tint
/// behind — the same compromise the worktree bands make for their hover.
pub(super) fn graph_row_background(selected: bool, theme: AppTheme) -> gpui::Rgba {
    if selected {
        theme.colors.interaction.selected_background
    } else {
        theme.colors.surface.canvas
    }
}

/// One row's graph cell: a fixed-width, full-height strip whose canvas
/// paints the row's `GraphRow` halves (`lanes_now` above the node,
/// `lanes_next` below), so consecutive rows' cells line up into one
/// continuous graph.
pub(super) fn render_graph_cell(
    graph: std::sync::Arc<Vec<GraphRow>>,
    row_ix: usize,
    selected_lane: Option<SelectedLane>,
    theme: AppTheme,
    row_background: gpui::Rgba,
    width_px: f32,
) -> impl IntoElement {
    let cell_width = px(width_px);
    div()
        .flex_none()
        .w(cell_width)
        .h_full()
        .overflow_hidden()
        .child(
            gpui::canvas(
                move |_bounds, _window, _cx| {},
                move |bounds, _pre, window, cx| {
                    if let Some(row) = graph.get(row_ix) {
                        paint_history_graph(
                            theme,
                            row,
                            row_ix,
                            None,
                            false,
                            selected_lane,
                            row_background,
                            bounds,
                            window,
                            cx,
                        );
                    }
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcomet_jj_core::{ChangeId, JjCommitId};

    fn change(id: &str, parents: &[&str]) -> JjChange {
        JjChange {
            change_id: ChangeId(id.to_string()),
            commit_id: JjCommitId(format!("c{id}")),
            parent_ids: parents
                .iter()
                .map(|parent| ChangeId(parent.to_string()))
                .collect(),
            divergent: false,
            conflicted: false,
            is_working_copy: false,
            bookmarks: Vec::new(),
            author_name: String::new(),
            author_email: String::new(),
            committed_at_unix: 0,
            description: String::new(),
        }
    }

    fn repo_with(changes: Vec<JjChange>) -> gitcomet_state::jj_store::JjRepoState {
        let mut repo = super::super::test_jj_repo_state(1, "/tmp/jj-graph");
        repo.changes = changes;
        repo
    }

    /// @ is pinned above the list, so the graph is computed over the rows
    /// the list actually draws — otherwise row indices and graph rows would
    /// disagree and every row would paint its neighbour's lanes.
    #[test]
    fn listed_changes_drop_the_working_copy() {
        let repo = repo_with(vec![
            JjChange {
                is_working_copy: true,
                ..change("at", &["tip"])
            },
            change("tip", &["base"]),
            change("base", &[]),
        ]);

        let listed = listed_changes(&repo);
        assert_eq!(
            listed
                .iter()
                .map(|change| change.change_id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["tip", "base"]
        );
    }

    #[test]
    fn a_local_bookmark_splits_its_own_lane() {
        let theme = AppTheme::gitcomet_dark();
        let tip = JjChange {
            bookmarks: vec!["main".to_string()],
            ..change("tip", &["base"])
        };
        let side = JjChange {
            bookmarks: vec!["feature".to_string(), "feature@upstream".to_string()],
            ..change("side", &["base"])
        };
        let repo = repo_with(vec![tip, side, change("base", &[])]);

        let graph = change_graph_rows(&repo, theme);

        // Both heads sit above the shared base, so the list carries two
        // lanes before converging on `base`.
        let widest = graph
            .iter()
            .map(|row| row.lanes_now.len().max(row.lanes_next.len()))
            .max()
            .unwrap();
        assert_eq!(widest, 2, "two bookmarked heads fork two lanes");
    }

    #[test]
    fn the_width_follows_the_lane_count_and_clamps() {
        let one_lane = vec![GraphRow {
            lanes_now: vec![history_graph::LanePaint::lane(0, false, false)].into(),
            lanes_next: vec![history_graph::LanePaint::lane(0, false, false)].into(),
            joins_in: Default::default(),
            edges_out: Default::default(),
            node_col: 0,
            node_color_ix: 0,
            is_merge: false,
        }];

        let expected_one = HISTORY_GRAPH_MARGIN_X_PX * 2.0 + HISTORY_GRAPH_COL_GAP_PX;
        assert_eq!(graph_column_width_px(&one_lane), expected_one);

        // More lanes widen the cell one lane pitch per column...
        let five_lanes = vec![GraphRow {
            lanes_now: vec![history_graph::LanePaint::lane(0, false, false); 5].into(),
            lanes_next: vec![history_graph::LanePaint::lane(0, false, false)].into(),
            joins_in: Default::default(),
            edges_out: Default::default(),
            node_col: 0,
            node_color_ix: 0,
            is_merge: false,
        }];
        assert_eq!(
            graph_column_width_px(&five_lanes),
            HISTORY_GRAPH_MARGIN_X_PX * 2.0 + HISTORY_GRAPH_COL_GAP_PX * 5.0
        );

        // ...until the clamp, which caps runaway topologies.
        let mut huge = one_lane;
        huge[0].lanes_now = vec![history_graph::LanePaint::lane(0, false, false); 500].into();
        assert_eq!(
            graph_column_width_px(&huge),
            crate::view::HISTORY_COL_GRAPH_MAX_PX
        );
    }

    #[test]
    fn the_selected_change_resolves_to_its_lane() {
        let theme = AppTheme::gitcomet_dark();
        let repo = repo_with(vec![
            change("tip", &["base"]),
            JjChange {
                bookmarks: vec!["feature".to_string()],
                ..change("side", &["base"])
            },
            change("base", &[]),
        ]);

        let graph = change_graph_rows(&repo, theme);

        let selected = selected_lane_for(&graph, &repo, Some(&ChangeId("tip".to_string())));
        assert!(selected.is_some(), "a listed change resolves to its lane");
        // @ and unknown ids resolve to nothing — the whole graph stays at
        // full colour instead of washing against a lane that isn't there.
        assert_eq!(
            selected_lane_for(&graph, &repo, Some(&ChangeId("nope".to_string()))),
            None
        );
    }
}
