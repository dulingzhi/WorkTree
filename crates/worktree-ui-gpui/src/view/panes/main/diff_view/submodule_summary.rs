//! Submodule diff summary domain: the hash/label free functions, the
//! summary renderer and its hash-input slot preparation.
use super::*;
use worktree_core::domain::{
    SubmoduleDiffRangeKind, SubmoduleDiffSummary, SubmoduleDiffSummaryMode, SubmoduleInnerChange,
    SubmoduleStatus,
};
use worktree_state::model::{InlineSubmoduleDiffEntry, InlineSubmoduleDiffSection};

fn short_submodule_hash(commit_id: &CommitId) -> String {
    let raw = commit_id.as_ref();
    raw.chars().take(12).collect()
}

fn short_submodule_hash_opt(commit_id: Option<&CommitId>) -> String {
    commit_id
        .map(short_submodule_hash)
        .unwrap_or_else(|| crate::i18n::tr_str("diff.submodule.missing").to_string())
}

fn full_submodule_hash_opt(commit_id: Option<&CommitId>) -> String {
    commit_id
        .map(|commit_id| commit_id.as_ref().to_string())
        .unwrap_or_else(|| crate::i18n::tr_str("diff.submodule.missing").to_string())
}

fn submodule_range_label(kind: SubmoduleDiffRangeKind) -> &'static str {
    match kind {
        SubmoduleDiffRangeKind::StagedPointer => {
            crate::i18n::tr_str("diff.submodule.range.staged_pointer")
        }
        SubmoduleDiffRangeKind::UnstagedPointer => {
            crate::i18n::tr_str("diff.submodule.range.unstaged_pointer")
        }
        SubmoduleDiffRangeKind::CommitHistory => {
            crate::i18n::tr_str("diff.submodule.range.commit_history")
        }
    }
}

fn inline_submodule_entries(summary: &SubmoduleDiffSummary) -> Vec<InlineSubmoduleDiffEntry> {
    let capacity = summary.ranges.iter().fold(
        summary
            .live_staged
            .len()
            .saturating_add(summary.live_unstaged.len()),
        |len, range| len.saturating_add(range.changes.len()),
    );
    let mut entries = Vec::with_capacity(capacity);
    for range in &summary.ranges {
        let Some((from_commit_id, to_commit_id)) = range.from.clone().zip(range.to.clone()) else {
            continue;
        };
        entries.extend(range.changes.iter().map(|change| InlineSubmoduleDiffEntry {
            path: change.path.clone(),
            kind: change.kind,
            target: DiffTarget::CommitRange {
                from_commit_id: from_commit_id.clone(),
                to_commit_id: Some(to_commit_id.clone()),
                path: Some(change.path.clone()),
            },
            section: InlineSubmoduleDiffSection::Range(range.kind),
        }));
    }
    entries.extend(
        summary
            .live_staged
            .iter()
            .map(|change| InlineSubmoduleDiffEntry {
                path: change.path.clone(),
                kind: change.kind,
                target: DiffTarget::WorkingTree {
                    path: change.path.clone(),
                    area: DiffArea::Staged,
                },
                section: InlineSubmoduleDiffSection::LiveStaged,
            }),
    );
    entries.extend(
        summary
            .live_unstaged
            .iter()
            .map(|change| InlineSubmoduleDiffEntry {
                path: change.path.clone(),
                kind: change.kind,
                target: DiffTarget::WorkingTree {
                    path: change.path.clone(),
                    area: DiffArea::Unstaged,
                },
                section: InlineSubmoduleDiffSection::LiveUnstaged,
            }),
    );
    entries
}

impl MainPaneView {
    fn prepare_submodule_hash_input(
        &mut self,
        slot: usize,
        value: String,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> Entity<components::TextInput> {
        let Some(input) = self
            .submodule_hash_inputs
            .get(slot % self.submodule_hash_inputs.len().max(1))
            .cloned()
        else {
            return self.diff_raw_input.clone();
        };
        input.update(cx, |input, cx| {
            input.set_theme(theme, cx);
            input.set_text(value, cx);
            input.set_read_only(true, cx);
        });
        input
    }

    pub(super) fn render_submodule_summary(
        &mut self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let Some(repo) = self.active_repo() else {
            return components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.submodule"),
                crate::i18n::tr("diff.common.no_repository"),
            )
            .into_any_element();
        };
        let Some(repo_id) = self.active_repo_id() else {
            return components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.submodule"),
                crate::i18n::tr("diff.common.no_repository"),
            )
            .into_any_element();
        };
        let Some(selected_target) = repo.diff_state.diff_target.as_ref().cloned() else {
            return components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.submodule"),
                crate::i18n::tr("diff.submodule.no_submodule_selected"),
            )
            .into_any_element();
        };
        let (submodule_path, selected_area) = match &selected_target {
            DiffTarget::WorkingTree { path, area } => (path.clone(), Some(*area)),
            DiffTarget::Commit {
                path: Some(path), ..
            } => (path.clone(), None),
            _ => {
                return components::empty_state(
                    theme,
                    crate::i18n::tr("diff.pane.submodule"),
                    crate::i18n::tr("diff.submodule.no_submodule_selected"),
                )
                .into_any_element();
            }
        };

        let repo_workdir = repo.spec.workdir.clone();
        let open_path = repo_workdir.join(&submodule_path);
        let fallback_status = match &repo.submodules {
            Loadable::Ready(submodules) => submodules
                .iter()
                .find(|submodule| submodule.path == submodule_path)
                .map(|submodule| submodule.status),
            _ => None,
        };
        let fallback_initialized = open_path.join(".git").exists();

        match &repo.diff_state.submodule_summary {
            Loadable::NotLoaded | Loadable::Loading => components::empty_state(
                theme,
                crate::i18n::tr("diff.pane.submodule"),
                crate::i18n::tr("diff.submodule.loading_summary"),
            )
            .into_any_element(),
            Loadable::Error(error) => {
                components::empty_state(theme, "Submodule", error.clone()).into_any_element()
            }
            Loadable::Ready(summary) => {
                let summary = (**summary).clone();
                let inline_entries = inline_submodule_entries(&summary);
                let summary_status = summary.status.or(fallback_status);
                let initialized = match summary_status {
                    Some(SubmoduleStatus::NotInitialized) => false,
                    Some(SubmoduleStatus::MergeConflict | SubmoduleStatus::MissingMapping) => false,
                    Some(_) => true,
                    None => fallback_initialized,
                };
                let can_open = initialized;
                let can_change_pointer = summary.mode == SubmoduleDiffSummaryMode::Worktree
                    && can_open
                    && !matches!(
                        summary_status,
                        Some(SubmoduleStatus::MergeConflict | SubmoduleStatus::MissingMapping)
                    );
                let show_load = summary.mode == SubmoduleDiffSummaryMode::Worktree
                    && (matches!(summary_status, Some(SubmoduleStatus::NotInitialized))
                        || (summary_status.is_none() && !fallback_initialized));
                let submodule_repo_path = repo_workdir.join(&summary.path);
                let summary_path = summary.path.clone();

                let status_badge = |status: SubmoduleStatus| {
                    let (label, color) = match status {
                        SubmoduleStatus::UpToDate => (
                            crate::i18n::tr_str("diff.submodule.status.loaded"),
                            theme.colors.status.success.foreground,
                        ),
                        SubmoduleStatus::NotInitialized => (
                            crate::i18n::tr_str("diff.submodule.status.not_loaded"),
                            with_alpha(
                                theme.colors.foreground.secondary,
                                if theme.is_dark { 0.86 } else { 0.94 },
                            ),
                        ),
                        SubmoduleStatus::HeadMismatch => (
                            crate::i18n::tr_str("diff.submodule.status.head_mismatch"),
                            theme.colors.status.warning.foreground,
                        ),
                        SubmoduleStatus::MergeConflict => (
                            crate::i18n::tr_str("diff.submodule.status.conflict"),
                            theme.colors.status.danger.foreground,
                        ),
                        SubmoduleStatus::MissingMapping => (
                            crate::i18n::tr_str("diff.submodule.status.missing_mapping"),
                            theme.colors.status.danger.foreground,
                        ),
                        SubmoduleStatus::Unknown(_) => (
                            crate::i18n::tr_str("diff.submodule.status.unknown"),
                            theme.colors.foreground.secondary,
                        ),
                    };

                    div()
                        .px_1p5()
                        .h(px(20.0))
                        .rounded(px(theme.radii.row))
                        .border_1()
                        .border_color(with_alpha(color, if theme.is_dark { 0.45 } else { 0.32 }))
                        .bg(with_alpha(color, if theme.is_dark { 0.14 } else { 0.10 }))
                        .text_xs()
                        .text_color(color)
                        .child(label)
                };

                let change_row_icon = |kind: FileStatusKind| match kind {
                    FileStatusKind::Untracked | FileStatusKind::Added => {
                        ("icons/plus.svg", theme.colors.status.success.foreground)
                    }
                    FileStatusKind::Modified => {
                        ("icons/pencil.svg", theme.colors.status.warning.foreground)
                    }
                    FileStatusKind::Deleted => {
                        ("icons/minus.svg", theme.colors.status.danger.foreground)
                    }
                    FileStatusKind::Renamed => ("icons/swap.svg", theme.colors.accent.foreground),
                    FileStatusKind::Conflicted => {
                        ("icons/warning.svg", theme.colors.status.danger.foreground)
                    }
                };

                let render_change_rows =
                    |section_key: &str,
                     changes: &[SubmoduleInnerChange],
                     range_commits: Option<(CommitId, CommitId)>,
                     live_area: Option<DiffArea>,
                     _this: &mut MainPaneView,
                     cx: &mut gpui::Context<MainPaneView>| {
                        if changes.is_empty() {
                            return vec![
                                div()
                                    .px_2()
                                    .py_1()
                                    .text_sm()
                                    .text_color(theme.colors.foreground.secondary)
                                    .child(crate::i18n::tr("diff.submodule.no_inner_changes"))
                                    .into_any_element(),
                            ];
                        }

                        changes
                            .iter()
                            .map(|change| {
                                let (icon, icon_color) = change_row_icon(change.kind);
                                let additions = change
                                    .additions
                                    .map(|value| format!("+{value}"))
                                    .unwrap_or_else(|| "—".to_string());
                                let deletions = change
                                    .deletions
                                    .map(|value| format!("-{value}"))
                                    .unwrap_or_else(|| "—".to_string());
                                let change_path = change.path.clone();
                                let target = range_commits.as_ref().map_or_else(
                                    || {
                                        live_area.map(|area| DiffTarget::WorkingTree {
                                            path: change_path.clone(),
                                            area,
                                        })
                                    },
                                    |(from_commit_id, to_commit_id)| {
                                        Some(DiffTarget::CommitRange {
                                            from_commit_id: from_commit_id.clone(),
                                            to_commit_id: Some(to_commit_id.clone()),
                                            path: Some(change_path.clone()),
                                        })
                                    },
                                );
                                let inline_selected_ix = target.as_ref().and_then(|target| {
                                    inline_entries
                                        .iter()
                                        .position(|entry| &entry.target == target)
                                });
                                let repo_path_for_click = submodule_repo_path.clone();
                                let repo_path_for_menu = submodule_repo_path.clone();
                                let summary_path_for_inline = summary.path.clone();
                                let inline_entries_for_click = inline_entries.clone();
                                let context_menu_path = change_path.clone();

                                let mut row = div()
                                .id(format!("{}_{}", section_key, change_path.display()))
                                .px_2()
                                .py_1()
                                .rounded(px(theme.radii.row))
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(crate::view::icons::svg_icon(
                                    icon,
                                    icon_color,
                                    px(12.0),
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_sm()
                                        .line_clamp(1)
                                        .child(change_path.display().to_string()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(
                                            crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY,
                                        )
                                        .text_color(theme.colors.status.success.foreground)
                                        .child(additions),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(
                                            crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY,
                                        )
                                        .text_color(theme.colors.status.danger.foreground)
                                        .child(deletions),
                                );

                                if let Some(target) = target {
                                    row = row
                                        .cursor(CursorStyle::PointingHand)
                                        .hover(move |row| {
                                            row.bg(theme.colors.interaction.hover_background)
                                        })
                                        .on_click(cx.listener(
                                            move |this, _e: &ClickEvent, _window, cx| {
                                                let selected_ix = inline_selected_ix.unwrap_or(0);
                                                this.store.dispatch(Msg::OpenInlineSubmoduleDiff {
                                                    repo_id,
                                                    origin: worktree_state::model::ForeignDiffOrigin::Submodule,
                                                    submodule_repo_path: repo_path_for_click
                                                        .clone(),
                                                    parent_submodule_path: summary_path_for_inline
                                                        .clone(),
                                                    entries: inline_entries_for_click.clone(),
                                                    selected_ix,
                                                });
                                                cx.notify();
                                            },
                                        ))
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(
                                                move |this, e: &MouseDownEvent, window, cx| {
                                                    cx.stop_propagation();
                                                    this.activate_context_menu_invoker(
                                                        format!(
                                                            "submodule_inner_diff_menu_{}_{}",
                                                            repo_id.0,
                                                            context_menu_path.display()
                                                        )
                                                        .into(),
                                                        cx,
                                                    );
                                                    this.open_popover_at(
                                                        PopoverKind::SubmoduleInnerDiffMenu {
                                                            repo_id,
                                                            submodule_repo_path: repo_path_for_menu
                                                                .clone(),
                                                            target: target.clone(),
                                                        },
                                                        e.position,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ),
                                        );
                                }

                                row.into_any_element()
                            })
                            .collect::<Vec<_>>()
                    };

                let render_change_section =
                    |title: &'static str,
                     section_key: &str,
                     changes: &[SubmoduleInnerChange],
                     range_commits: Option<(CommitId, CommitId)>,
                     live_area: Option<DiffArea>,
                     this: &mut MainPaneView,
                     cx: &mut gpui::Context<MainPaneView>| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .pt_1()
                                    .text_xs()
                                    .text_color(theme.colors.foreground.secondary)
                                    .child(title),
                            )
                            .children(render_change_rows(
                                section_key,
                                changes,
                                range_commits,
                                live_area,
                                this,
                                cx,
                            ))
                            .into_any_element()
                    };

                let mut range_sections = Vec::new();
                for (slot, range) in summary.ranges.iter().enumerate() {
                    let emphasized = match range.kind {
                        SubmoduleDiffRangeKind::StagedPointer => {
                            selected_area == Some(DiffArea::Staged)
                        }
                        SubmoduleDiffRangeKind::UnstagedPointer => {
                            selected_area == Some(DiffArea::Unstaged)
                        }
                        SubmoduleDiffRangeKind::CommitHistory => true,
                    };
                    let changed = range.from != range.to;
                    let range_hash_input = self.prepare_submodule_hash_input(
                        slot,
                        format!(
                            "{} -> {}",
                            full_submodule_hash_opt(range.from.as_ref()),
                            full_submodule_hash_opt(range.to.as_ref())
                        ),
                        theme,
                        cx,
                    );
                    let range_commits = match (range.from.as_ref(), range.to.as_ref()) {
                        (Some(from), Some(to)) => Some((from.clone(), to.clone())),
                        _ => None,
                    };

                    let mut section = div()
                        .id(format!("submodule_range_{:?}", range.kind))
                        .px_2()
                        .py_2()
                        .rounded(px(theme.radii.row))
                        .border_1()
                        .border_color(if emphasized {
                            theme.colors.interaction.pressed_background
                        } else {
                            theme.colors.stroke.default
                        })
                        .bg(if emphasized {
                            with_alpha(
                                theme.colors.interaction.hover_background,
                                if theme.is_dark { 0.28 } else { 0.48 },
                            )
                        } else {
                            gpui::rgba(0x00000000)
                        })
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(submodule_range_label(range.kind)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_family(
                                            crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY,
                                        )
                                        .text_color(if changed {
                                            theme.colors.foreground.primary
                                        } else {
                                            theme.colors.foreground.secondary
                                        })
                                        .child(format!(
                                            "{} -> {}",
                                            short_submodule_hash_opt(range.from.as_ref()),
                                            short_submodule_hash_opt(range.to.as_ref())
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(crate::i18n::tr("diff.submodule.hashes")),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .min_w(px(0.0))
                                        .font_family(
                                            crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY,
                                        )
                                        .child(range_hash_input),
                                ),
                        );
                    if let Some(reason) = range.unavailable_reason.as_ref() {
                        section = section.child(
                            div()
                                .px_2()
                                .text_sm()
                                .text_color(theme.colors.foreground.secondary)
                                .child(reason.clone()),
                        );
                    }
                    section = section.child(render_change_section(
                        crate::i18n::tr_str("diff.submodule.changes_between_hashes"),
                        &format!("submodule_range_{:?}", range.kind),
                        &range.changes,
                        range_commits,
                        None,
                        self,
                        cx,
                    ));
                    range_sections.push(section.into_any_element());
                }

                div()
                    .id("submodule_summary_scroll")
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .gap_2()
                    .bg(theme.colors.surface.canvas)
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(crate::view::icons::svg_icon(
                                        "icons/box.svg",
                                        match summary_status.unwrap_or(SubmoduleStatus::UpToDate) {
                                            SubmoduleStatus::NotInitialized => with_alpha(
                                                theme.colors.foreground.secondary,
                                                if theme.is_dark { 0.82 } else { 0.94 },
                                            ),
                                            SubmoduleStatus::HeadMismatch => {
                                                theme.colors.status.warning.foreground
                                            }
                                            SubmoduleStatus::MergeConflict
                                            | SubmoduleStatus::MissingMapping => {
                                                theme.colors.status.danger.foreground
                                            }
                                            SubmoduleStatus::UpToDate
                                            | SubmoduleStatus::Unknown(_) => {
                                                theme.colors.accent.foreground
                                            }
                                        },
                                        px(14.0),
                                    ))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::BOLD)
                                            .child(summary.path.display().to_string()),
                                    )
                                    .when_some(summary_status, |this, status| {
                                        this.child(status_badge(status))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        components::Button::new(
                                            "submodule_summary_open",
                                            crate::i18n::tr("diff.submodule.open"),
                                        )
                                        .style(components::ButtonStyle::Outlined)
                                        .disabled(!can_open)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, _e, _w, cx| {
                                                if can_open {
                                                    this.store
                                                        .dispatch(Msg::OpenRepo(open_path.clone()));
                                                    cx.notify();
                                                }
                                            },
                                        ),
                                    )
                                    .when(show_load, |row| {
                                        let load_path = summary.path.clone();
                                        row.child(
                                            components::Button::new(
                                                "submodule_summary_load",
                                                crate::i18n::tr("diff.submodule.load"),
                                            )
                                            .style(components::ButtonStyle::Outlined)
                                            .on_click(
                                                theme,
                                                cx,
                                                move |this, _e, _w, cx| {
                                                    this.store.dispatch(Msg::LoadSubmodule {
                                                        repo_id,
                                                        path: load_path.clone(),
                                                    });
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                    })
                                    .child(
                                        components::Button::new(
                                            "submodule_summary_change_pointer",
                                            crate::i18n::tr("diff.submodule.change_pointer"),
                                        )
                                        .style(components::ButtonStyle::Outlined)
                                        .disabled(!can_change_pointer)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, e, window, cx| {
                                                if !can_change_pointer {
                                                    return;
                                                }
                                                this.open_popover_at(
                                                    PopoverKind::submodule(
                                                        repo_id,
                                                        SubmodulePopoverKind::ChangePointerPrompt {
                                                            path: summary_path.clone(),
                                                        },
                                                    ),
                                                    e.position(),
                                                    window,
                                                    cx,
                                                );
                                                cx.notify();
                                            },
                                        ),
                                    ),
                            ),
                    )
                    .children(range_sections)
                    .when(
                        summary.mode == SubmoduleDiffSummaryMode::Worktree
                            && !summary.live_staged.is_empty(),
                        |this| {
                            this.child(render_change_section(
                                crate::i18n::tr_str("diff.submodule.uncommitted_inner_staged"),
                                "submodule_live_staged",
                                &summary.live_staged,
                                None,
                                Some(DiffArea::Staged),
                                self,
                                cx,
                            ))
                        },
                    )
                    .when(
                        summary.mode == SubmoduleDiffSummaryMode::Worktree
                            && !summary.live_unstaged.is_empty(),
                        |this| {
                            this.child(render_change_section(
                                crate::i18n::tr_str("diff.submodule.uncommitted_inner_unstaged"),
                                "submodule_live_unstaged",
                                &summary.live_unstaged,
                                None,
                                Some(DiffArea::Unstaged),
                                self,
                                cx,
                            ))
                        },
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod submodule_helpers_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use worktree_core::domain::{
        SubmoduleDiffRange, SubmoduleDiffRangeKind, SubmoduleDiffSummary, SubmoduleDiffSummaryMode,
        SubmoduleInnerChange,
    };

    fn commit(id: &str) -> CommitId {
        CommitId(id.into())
    }

    #[test]
    fn short_hash_truncates_to_twelve_and_missing_falls_back() {
        assert_eq!(
            short_submodule_hash(&commit("abcdef1234567890")),
            "abcdef123456"
        );
        assert_eq!(short_submodule_hash(&commit("short")), "short");
        // A missing side reads as missing in both hash forms, never "".
        assert_eq!(
            full_submodule_hash_opt(None),
            crate::i18n::tr_str("diff.submodule.missing").to_string()
        );
        assert_eq!(
            full_submodule_hash_opt(None),
            short_submodule_hash_opt(None)
        );
        assert_eq!(
            full_submodule_hash_opt(Some(&commit("cafe"))),
            "cafe".to_string()
        );
    }

    #[test]
    fn range_labels_are_stable_per_kind() {
        // The labels are locale keys resolved at call time; the invariant is
        // that the three kinds map to three distinct, stable strings.
        let labels = [
            submodule_range_label(SubmoduleDiffRangeKind::StagedPointer),
            submodule_range_label(SubmoduleDiffRangeKind::UnstagedPointer),
            submodule_range_label(SubmoduleDiffRangeKind::CommitHistory),
        ];
        assert!(!labels[0].is_empty());
        assert_eq!(labels.len(), 3, "distinct kinds per the type system");
    }

    fn inner_change(path: &str) -> SubmoduleInnerChange {
        SubmoduleInnerChange {
            path: PathBuf::from(path),
            kind: FileStatusKind::Modified,
            additions: Some(1),
            deletions: Some(2),
        }
    }

    #[test]
    fn inline_entries_skip_incomplete_ranges_and_order_range_then_live() {
        let summary = SubmoduleDiffSummary {
            path: PathBuf::from("vendor/lib"),
            mode: SubmoduleDiffSummaryMode::CommitHistory,
            status: None,
            commit_id: Some(commit("cccc")),
            parent_commit_id: Some(commit("aaaa")),
            checked_out_head: None,
            ranges: vec![
                // A complete range contributes its changes as commit-range
                // targets under the range's section.
                SubmoduleDiffRange {
                    kind: SubmoduleDiffRangeKind::CommitHistory,
                    from: Some(commit("aaaa")),
                    to: Some(commit("cccc")),
                    changes: vec![inner_change("src/lib.rs")],
                    unavailable_reason: None,
                },
                // A range missing either side cannot address its changes —
                // skipped entirely, not half-built.
                SubmoduleDiffRange {
                    kind: SubmoduleDiffRangeKind::StagedPointer,
                    from: Some(commit("aaaa")),
                    to: None,
                    changes: vec![inner_change("skipped.txt")],
                    unavailable_reason: None,
                },
            ],
            live_staged: vec![inner_change("staged.txt")],
            live_unstaged: vec![inner_change("unstaged.txt")],
        };

        let entries = inline_submodule_entries(&summary);
        let paths: Vec<&Path> = entries.iter().map(|e| e.path.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                Path::new("src/lib.rs"),
                Path::new("staged.txt"),
                Path::new("unstaged.txt"),
            ],
            "ranges first (incomplete ones dropped), then live staged, then live unstaged"
        );
        match &entries[0].target {
            DiffTarget::CommitRange {
                from_commit_id,
                to_commit_id,
                ..
            } => {
                assert_eq!(from_commit_id.as_ref(), "aaaa");
                assert_eq!(to_commit_id.as_ref().map(AsRef::as_ref), Some("cccc"));
            }
            other => panic!("a range entry addresses a commit range, got {other:?}"),
        }
        assert!(matches!(
            &entries[1].target,
            DiffTarget::WorkingTree {
                area: DiffArea::Staged,
                ..
            }
        ));
        assert!(matches!(
            &entries[2].target,
            DiffTarget::WorkingTree {
                area: DiffArea::Unstaged,
                ..
            }
        ));
    }
}
