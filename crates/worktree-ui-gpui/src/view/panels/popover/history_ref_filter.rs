use super::*;
use worktree_core::domain::{Branch, RemoteBranch, Tag};

/// Height the ref list caps itself at, matching the assume-unchanged manager:
/// a handful of refs reads at a glance, and a long remote list stays
/// scrollable without pushing the popover off screen.
const REF_FILTER_LIST_MAX_HEIGHT_PX: f32 = 240.0;

/// One selectable row: the full ref name the walk resolves, and the shorter
/// name the list shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RefRow {
    pub(super) full_name: String,
    pub(super) label: String,
}

/// Every row the popover offers, grouped the way it renders.
///
/// A filter can reference a ref that no longer exists (a branch deleted after
/// the filter was saved to the session). The walk errors on unresolvable refs
/// rather than silently dropping them — `git log <gone-branch>` does the same —
/// so the filter is listed as `missing` instead of vanishing: the checkmark
/// shows what the walk is being asked for, and one click clears it.
#[derive(Clone)]
pub(super) struct RefRows {
    pub(super) local: Vec<RefRow>,
    pub(super) remote: Vec<RefRow>,
    pub(super) tags: Vec<RefRow>,
    pub(super) missing: Vec<String>,
}

pub(super) fn rows(
    branches: &[Branch],
    remote_branches: &[RemoteBranch],
    tags: &[Tag],
    filters: &[String],
) -> RefRows {
    let mut local: Vec<RefRow> = branches
        .iter()
        .map(|branch| RefRow {
            full_name: format!("refs/heads/{}", branch.name),
            label: branch.name.clone(),
        })
        .collect();
    local.sort_by(|a, b| a.label.cmp(&b.label));

    let mut remote: Vec<RefRow> = remote_branches
        .iter()
        .map(|branch| RefRow {
            full_name: format!("refs/remotes/{}/{}", branch.remote, branch.name),
            label: format!("{}/{}", branch.remote, branch.name),
        })
        .collect();
    remote.sort_by(|a, b| a.label.cmp(&b.label));

    let mut tag_rows: Vec<RefRow> = tags
        .iter()
        .map(|tag| RefRow {
            full_name: format!("refs/tags/{}", tag.name),
            label: tag.name.clone(),
        })
        .collect();
    tag_rows.sort_by(|a, b| a.label.cmp(&b.label));

    // `filters` arrives sorted and deduplicated from the state setter; the
    // leftovers keep that order because it is also the display order.
    let listed: std::collections::HashSet<&str> = local
        .iter()
        .chain(remote.iter())
        .chain(tag_rows.iter())
        .map(|row| row.full_name.as_str())
        .collect();
    let missing = filters
        .iter()
        .filter(|full_name| !listed.contains(full_name.as_str()))
        .cloned()
        .collect();

    RefRows {
        local,
        remote,
        tags: tag_rows,
        missing,
    }
}

/// Narrow the rows to those whose label (or, for the missing leftovers,
/// full name) contains the query, case-insensitively. An empty query keeps
/// everything — pure, so the shape is unit-testable without a popover.
pub(super) fn filter_rows_by_query(rows: RefRows, query: &str) -> RefRows {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return rows;
    }
    let matches = |label: &str| label.to_ascii_lowercase().contains(&query);
    RefRows {
        local: rows
            .local
            .into_iter()
            .filter(|row| matches(&row.label))
            .collect(),
        remote: rows
            .remote
            .into_iter()
            .filter(|row| matches(&row.label))
            .collect(),
        tags: rows
            .tags
            .into_iter()
            .filter(|row| matches(&row.label))
            .collect(),
        missing: rows
            .missing
            .into_iter()
            .filter(|name| matches(name))
            .collect(),
    }
}

/// Sets `full_name`'s membership in `filters`, returning the new set for
/// dispatch. The state setter sorts and deduplicates anyway, so the value the
/// popover computes and the value the walk keys on cannot disagree.
pub(super) fn toggled(filters: &[String], full_name: &str) -> Vec<String> {
    let mut next = filters.to_vec();
    if let Some(position) = next.iter().position(|name| name == full_name) {
        next.remove(position);
    } else {
        next.push(full_name.to_owned());
    }
    next
}

/// Applies a toggle (or a clear, with an empty set) and deliberately keeps the
/// popover open: each click refilters immediately, the way the sidebar toggles
/// in the C# original do, and the rows repaint from the store.
fn apply(this: &mut PopoverHost, repo_id: RepoId, refs: Vec<String>) {
    this.store
        .dispatch(Msg::SetHistoryRefFilters { repo_id, refs });
}

/// One checkable row. Same visual language as `checkable_option_row` (a 16px
/// box that fills with a check when enabled), minus the per-row focus handle:
/// that helper takes a dedicated `FocusHandle` per static option, and this
/// list is dynamic.
fn toggle_row(
    ix: usize,
    row: RefRow,
    missing: bool,
    repo_id: RepoId,
    theme: AppTheme,
    tooltip_host: &WeakEntity<TooltipHost>,
    cx: &mut gpui::Context<PopoverHost>,
) -> impl IntoElement {
    let scaled_px = super::popover_scaled_px_fn(cx);
    let mark_color = if missing {
        theme.colors.status.warning.foreground
    } else {
        theme.colors.status.success.foreground
    };
    let box_border = if missing {
        theme.colors.status.warning.foreground
    } else {
        theme.colors.stroke.default
    };

    let full_name = row.full_name.clone();
    let full_name_for_click = full_name.clone();
    let checkbox = div()
        .size(scaled_px(16.0))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(box_border)
        .rounded(scaled_px(theme.radii.control * 0.5))
        .when(missing, |this| {
            this.bg(with_alpha(
                theme.colors.status.warning.foreground,
                if theme.is_dark { 0.18 } else { 0.12 },
            ))
        })
        .child(crate::view::icons::svg_icon(
            "icons/check.svg",
            mark_color,
            scaled_px(10.0),
        ));

    div()
        .id(("history_ref_filter_row", ix))
        .debug_selector(move || format!("history_ref_filter_row_{full_name}"))
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py(scaled_px(2.0))
        .rounded(px(theme.radii.row))
        .cursor(CursorStyle::PointingHand)
        .hover(move |s| s.bg(theme.hover_overlay()))
        .active(move |s| s.bg(theme.active_overlay()))
        .child(checkbox)
        .child(
            components::TruncatedText::path(row.label)
                .id(("history_ref_filter_row_label", ix))
                .text_sm()
                .text_color(if missing {
                    theme.colors.status.warning.foreground
                } else {
                    theme.colors.foreground.secondary
                })
                .full_text_tooltip(tooltip_host.clone())
                .render(cx),
        )
        .when(missing, |this| {
            this.child(
                div()
                    .ml_auto()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.colors.status.warning.foreground)
                    .child(crate::i18n::tr("panels.ref_filter.missing_hint")),
            )
        })
        .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
            // Read the set at click time rather than capturing it at render:
            // the popover stays open across toggles, so this handler can serve
            // several clicks and must always branch off the latest state.
            let Some(repo) = this.state.repos.iter().find(|repo| repo.id == repo_id) else {
                return;
            };
            let refs = toggled(
                &repo.history_state.history_ref_filters,
                &full_name_for_click,
            );
            apply(this, repo_id, refs);
            cx.notify();
        }))
}

fn section_label(
    theme: AppTheme,
    scaled_px: impl Fn(f32) -> Pixels,
    key: &'static str,
) -> gpui::Div {
    div()
        .px_2()
        .pt(scaled_px(4.0))
        .pb(scaled_px(2.0))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.colors.foreground.secondary)
        .child(crate::i18n::tr(key))
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale_percent = super::popover_ui_scale_percent(cx);
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);
    let tooltip_host = this.tooltip_host.clone();
    // The list rebuild below needs `this` mutably (row listeners do not, but
    // the TruncatedText renders want a Context), so read the repo first.
    let repo = this.state.repos.iter().find(|repo| repo.id == repo_id);

    let filters: Vec<String> = repo
        .map(|repo| repo.history_state.history_ref_filters.clone())
        .unwrap_or_default();
    let filter_active = !filters.is_empty();
    let query = this
        .history_ref_filter_search_input
        .as_ref()
        .map(|input| input.read(cx).text().trim().to_string())
        .unwrap_or_default();

    let header = div()
        .px(scaled_px(8.0))
        .py(scaled_px(4.0))
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(crate::i18n::tr("panels.ref_filter.title")),
        )
        .child(div().flex_1())
        .when(filter_active, |header| {
            header.child(
                components::Button::new(
                    "history_ref_filter_clear",
                    crate::i18n::tr("panels.ref_filter.clear"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, _cx| {
                    apply(this, repo_id, Vec::new());
                })
                .debug_selector(|| "history_ref_filter_clear".to_string()),
            )
        })
        .child(
            components::Button::new(
                "history_ref_filter_close",
                crate::i18n::tr("panels.ref_filter.close"),
            )
            .style(components::ButtonStyle::Outlined)
            .on_click(theme, cx, |this, _e, _w, cx| this.close_popover(cx))
            .debug_selector(|| "history_ref_filter_close".to_string()),
        );

    // Sections in display order; empty ones are skipped below so a checkout
    // with no tags never shows a "TAGS" headline over nothing.
    let list_body: AnyElement =
        match repo.map(|repo| (&repo.branches, &repo.remote_branches, &repo.tags)) {
            None => components::context_menu_label(
                theme,
                ui_scale_percent,
                crate::i18n::tr("ui.common.no_repository"),
                Some(tooltip_host.clone()),
                cx,
            )
            .into_any_element(),
            // The open path dispatches the loads, so NotLoaded is the brief moment
            // before the Loading transition lands — same face for both.
            Some(
                (Loadable::NotLoaded, ..)
                | (Loadable::Loading, ..)
                | (_, Loadable::NotLoaded, _)
                | (_, Loadable::Loading, _)
                | (.., Loadable::NotLoaded)
                | (.., Loadable::Loading),
            ) => components::context_menu_label(
                theme,
                ui_scale_percent,
                crate::i18n::tr("ui.common.loading_ellipsis"),
                Some(tooltip_host.clone()),
                cx,
            )
            .into_any_element(),
            Some(
                (Loadable::Error(e), ..) | (_, Loadable::Error(e), _) | (.., Loadable::Error(e)),
            ) => components::context_menu_label(
                theme,
                ui_scale_percent,
                e.clone(),
                Some(tooltip_host.clone()),
                cx,
            )
            .into_any_element(),
            Some((
                Loadable::Ready(branches),
                Loadable::Ready(remote_branches),
                Loadable::Ready(tags),
            )) => {
                let rows =
                    filter_rows_by_query(rows(branches, remote_branches, tags, &filters), &query);
                let mut list = div().flex().flex_col();
                let mut row_ix = 0usize;

                if !rows.local.is_empty() {
                    list = list.child(section_label(
                        theme,
                        scaled_px,
                        "panels.ref_filter.section_local",
                    ));
                    for row in rows.local {
                        list = list.child(toggle_row(
                            row_ix,
                            row,
                            false,
                            repo_id,
                            theme,
                            &tooltip_host,
                            cx,
                        ));
                        row_ix += 1;
                    }
                }
                if !rows.remote.is_empty() {
                    list = list.child(section_label(
                        theme,
                        scaled_px,
                        "panels.ref_filter.section_remote",
                    ));
                    for row in rows.remote {
                        list = list.child(toggle_row(
                            row_ix,
                            row,
                            false,
                            repo_id,
                            theme,
                            &tooltip_host,
                            cx,
                        ));
                        row_ix += 1;
                    }
                }
                if !rows.tags.is_empty() {
                    list = list.child(section_label(
                        theme,
                        scaled_px,
                        "panels.ref_filter.section_tags",
                    ));
                    for row in rows.tags {
                        list = list.child(toggle_row(
                            row_ix,
                            row,
                            false,
                            repo_id,
                            theme,
                            &tooltip_host,
                            cx,
                        ));
                        row_ix += 1;
                    }
                }
                for full_name in rows.missing {
                    list = list.child(toggle_row(
                        row_ix,
                        RefRow {
                            full_name: full_name.clone(),
                            label: full_name,
                        },
                        true,
                        repo_id,
                        theme,
                        &tooltip_host,
                        cx,
                    ));
                    row_ix += 1;
                }

                if row_ix == 0 {
                    components::context_menu_label(
                        theme,
                        ui_scale_percent,
                        if query.is_empty() {
                            crate::i18n::tr("panels.ref_filter.empty")
                        } else {
                            crate::i18n::tr("panels.ref_filter.no_match")
                        },
                        Some(tooltip_host.clone()),
                        cx,
                    )
                    .into_any_element()
                } else {
                    div()
                        .id("history_ref_filter_rows")
                        .debug_selector(|| "history_ref_filter_rows".to_string())
                        .max_h(scaled_px(REF_FILTER_LIST_MAX_HEIGHT_PX))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .child(list)
                        .into_any_element()
                }
            }
        };

    components::context_menu(
        theme,
        div()
            .id("history_ref_filter")
            .debug_selector(|| "history_ref_filter".to_string())
            .flex()
            .flex_col()
            .w(super::HISTORY_REF_FILTER_WIDTH.preferred_px(super::popover_ui_scale(cx)))
            .child(header)
            .child(super::dialog_divider(theme))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr("panels.ref_filter.hint")),
            )
            .when_some(
                this.history_ref_filter_search_input.clone(),
                |popover, search| {
                    popover.child(
                        div()
                            .id("history_ref_filter_search_row")
                            .debug_selector(|| "history_ref_filter_search".to_string())
                            .px_2()
                            .pb(scaled_px(4.0))
                            .child(search),
                    )
                },
            )
            .child(list_body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_core::domain::CommitId;

    fn branch(name: &str) -> Branch {
        Branch {
            name: name.to_owned(),
            target: CommitId("target".into()),
            upstream: None,
            divergence: None,
        }
    }

    fn remote_branch(remote: &str, name: &str) -> RemoteBranch {
        RemoteBranch {
            remote: remote.to_owned(),
            name: name.to_owned(),
            target: CommitId("target".into()),
        }
    }

    fn tag(name: &str) -> Tag {
        Tag {
            name: name.to_owned(),
            target: CommitId("target".into()),
            created_at: None,
        }
    }

    fn full_names(rows: &[RefRow]) -> Vec<&str> {
        rows.iter().map(|row| row.full_name.as_str()).collect()
    }

    #[test]
    fn rows_compose_full_names_and_sort_within_sections() {
        let rows = rows(
            &[branch("dev"), branch("main")],
            &[
                remote_branch("upstream", "release"),
                remote_branch("origin", "main"),
            ],
            &[tag("v2"), tag("v1")],
            &[],
        );

        // The walk resolves full ref names, so that is what every row carries;
        // the label stays the short form the sidebar shows.
        assert_eq!(
            full_names(&rows.local),
            ["refs/heads/dev", "refs/heads/main"]
        );
        assert_eq!(rows.local[0].label, "dev");
        assert_eq!(
            full_names(&rows.remote),
            ["refs/remotes/origin/main", "refs/remotes/upstream/release"]
        );
        assert_eq!(rows.remote[1].label, "upstream/release");
        assert_eq!(full_names(&rows.tags), ["refs/tags/v1", "refs/tags/v2"]);
        assert!(rows.missing.is_empty());
    }

    #[test]
    fn filter_rows_by_query_narrows_labels_case_insensitively() {
        let rows = RefRows {
            local: vec![
                RefRow {
                    full_name: "refs/heads/main".into(),
                    label: "main".into(),
                },
                RefRow {
                    full_name: "refs/heads/feat/Widget".into(),
                    label: "feat/Widget".into(),
                },
            ],
            remote: vec![RefRow {
                full_name: "refs/remotes/origin/MAIN".into(),
                label: "origin/MAIN".into(),
            }],
            tags: vec![RefRow {
                full_name: "refs/tags/v1".into(),
                label: "v1".into(),
            }],
            missing: vec!["refs/heads/gone-Main".into()],
        };

        let filtered = filter_rows_by_query(rows.clone(), "main");
        assert_eq!(
            filtered
                .local
                .iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            vec!["main"],
            "the label match is case-insensitive"
        );
        assert_eq!(filtered.remote.len(), 1);
        assert_eq!(filtered.missing, vec!["refs/heads/gone-Main"]);
        assert!(
            filtered.tags.is_empty(),
            "an unmatched section simply empties"
        );

        // An empty (or whitespace) query is a no-op, not a wipe.
        let untouched = filter_rows_by_query(rows, "  ");
        assert_eq!(untouched.local.len(), 2);
        assert_eq!(untouched.tags.len(), 1);
    }

    #[test]
    fn rows_flag_filters_that_match_no_listed_ref_as_missing() {
        let rows = rows(
            &[branch("main")],
            &[remote_branch("origin", "main")],
            &[tag("v1")],
            &[
                // A local branch deleted after the filter was saved.
                "refs/heads/gone".to_string(),
                // Still listed: not missing.
                "refs/heads/main".to_string(),
                // A remote-tracking ref whose remote was removed.
                "refs/remotes/old/main".to_string(),
                "refs/tags/never-existed".to_string(),
            ],
        );

        assert_eq!(
            rows.missing,
            vec![
                "refs/heads/gone".to_string(),
                "refs/remotes/old/main".to_string(),
                "refs/tags/never-existed".to_string(),
            ],
            "only filters no listed ref explains are missing, in filter order"
        );
    }

    #[test]
    fn toggling_adds_and_removes_symmetrically() {
        let filters = vec!["refs/heads/dev".to_string()];

        assert_eq!(
            toggled(&filters, "refs/tags/v1"),
            vec!["refs/heads/dev".to_string(), "refs/tags/v1".to_string()]
        );
        assert_eq!(
            toggled(&filters, "refs/heads/dev"),
            Vec::<String>::new(),
            "toggling the only member off clears the set"
        );
    }
}
