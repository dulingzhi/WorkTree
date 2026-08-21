//! The local branch menu's "Change tracking upstream…" picker.
//!
//! One plain list of the repository's remote branches; picking a row runs the
//! same `set-upstream-to` the remote branch menu's entry runs, and the unlink
//! row at the top clears the pairing again. Deliberately plain rows — the
//! picker answers "which remote branch does this branch follow", and any extra
//! column would only compete with that answer.

use super::*;
use std::rc::Rc;

/// What activating a row in the upstream picker does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum UpstreamRow {
    /// Track this remote branch — the full short ref (`origin/main`) that
    /// `git branch --set-upstream-to` takes.
    RemoteBranch(String),
    /// Clear the branch's tracking config.
    Unlink,
}

/// Rows the picker displays, in render order, paired with what each one does
/// and which row is the branch's current upstream.
///
/// Both the panel and keyboard navigation go through this, so the list the user
/// sees and the list Enter walks can never disagree.
pub(super) struct UpstreamRows {
    pub(super) items: Vec<components::PickerPromptItem>,
    pub(super) rows: Vec<UpstreamRow>,
    /// Index of the tracked remote branch **before filtering** —
    /// `PickerPrompt` compares `marked_index` against the pre-filter index.
    pub(super) marked_index: Option<usize>,
}

/// The branch's current upstream as `remote/branch`, read from the branch list
/// — the same data the sidebar's tracking rows show.
fn branch_upstream(repo: &RepoState, branch: &str) -> Option<String> {
    let branches = repo.branches.ready()?;
    branches
        .iter()
        .find(|b| b.name == branch)
        .and_then(|b| b.upstream.as_ref())
        .map(|upstream| format!("{}/{}", upstream.remote, upstream.branch))
}

/// Takes the repository rather than the host so the result is a pure function
/// of its inputs, which is what lets [`rows_cache`](super::rows_cache) memoise
/// it across frames.
pub(super) fn rows(repo: &RepoState, branch: &str, _query: &str) -> UpstreamRows {
    let current = branch_upstream(repo, branch);
    let remote_count = repo
        .remote_branches
        .ready()
        .map_or(0usize, |values| values.len());
    let mut items = Vec::with_capacity(remote_count.saturating_add(1));
    let mut rows = Vec::with_capacity(remote_count.saturating_add(1));
    let mut marked_index = None;

    // Unlink first, so "stop tracking" never scrolls off. Offered only while a
    // pairing exists — with none, the row would announce clearing what is
    // already clear.
    if let Some(upstream) = current.as_deref() {
        items.push(
            components::PickerPromptItem::from_parts([
                components::PickerPromptItemPart::new(crate::i18n::tr("pick.upstream.unlink"))
                    .searchable(false)
                    .tooltip(false),
                components::PickerPromptItemPart::separator("  "),
                // The ref stays searchable, so typing the tracked name keeps
                // the clear action in reach.
                components::PickerPromptItemPart::new(upstream.to_string())
                    .profile(components::TextTruncationProfile::End),
            ])
            .icon("icons/unlink.svg"),
        );
        rows.push(UpstreamRow::Unlink);
    }

    if let Loadable::Ready(remote_branches) = &repo.remote_branches {
        for remote_branch in remote_branches.iter() {
            let full = format!("{}/{}", remote_branch.remote, remote_branch.name);
            if current.as_deref() == Some(full.as_str()) {
                marked_index = Some(items.len());
            }
            items.push(
                components::PickerPromptItem::plain(full.clone()).icon("icons/git_branch.svg"),
            );
            rows.push(UpstreamRow::RemoteBranch(full));
        }
    }

    UpstreamRows {
        items,
        rows,
        marked_index,
    }
}

/// Everything [`rows`] reads, digested for the cache. Mirrors the
/// `PopoverKind::UpstreamPicker` arm of [`super::fingerprint`]: the branch list
/// decides whether the unlink row exists and which row carries the mark, the
/// remote list is the rows themselves.
pub(super) fn rows_signature(repo: &RepoState, branch: &str) -> u64 {
    use std::hash::Hash;

    super::rows_cache::signature(|hasher| {
        repo.id.hash(hasher);
        repo.branches_rev.hash(hasher);
        repo.remote_branches_rev.hash(hasher);
        branch.hash(hasher);
    })
}

/// The cached rows for `query`: the items, the filtered layout, and the payload
/// behind each row. Every caller goes through this, so a frame that only
/// repaints reuses the rows instead of rebuilding them.
pub(super) fn cached(
    this: &PopoverHost,
    repo_id: RepoId,
    branch: &str,
    query: &str,
) -> Rc<super::rows_cache::CachedRows<UpstreamRow>> {
    let Some(repo) = this.state.repos.iter().find(|r| r.id == repo_id) else {
        return super::rows_cache::CachedRows::empty();
    };
    let key = super::rows_cache::RowsCacheKey::new(
        super::rows_cache::RowsCacheOwner::UpstreamPicker,
        rows_signature(repo, branch),
        query,
    );
    super::rows_cache::get_or_build(&this.upstream_picker_rows_cache, key, |_now| {
        let built = rows(repo, branch, query);
        (built.items, built.rows, built.marked_index)
    })
}

/// Payloads for the rows surviving `query`, in the order the picker renders
/// them — the list keyboard navigation walks.
pub(super) fn nav_targets(
    this: &PopoverHost,
    repo_id: RepoId,
    branch: &str,
    query: &str,
) -> Vec<UpstreamRow> {
    cached(this, repo_id, branch, query).filtered_payloads()
}

/// Activates a row: re-points (or clears) the branch's tracking config through
/// the same messages the remote branch menu already dispatches.
pub(super) fn activate(
    this: &mut PopoverHost,
    repo_id: RepoId,
    branch: String,
    row: UpstreamRow,
    cx: &mut gpui::Context<PopoverHost>,
) {
    match row {
        UpstreamRow::RemoteBranch(upstream) => this.store.dispatch(Msg::SetUpstreamBranch {
            repo_id,
            branch,
            upstream,
        }),
        UpstreamRow::Unlink => this
            .store
            .dispatch(Msg::UnsetUpstreamBranch { repo_id, branch }),
    }
    this.close_popover(cx);
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    branch: String,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale = super::popover_ui_scale(cx);
    let ui_scale_percent = ui_scale.percent();
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);
    let width = super::PICKER_WIDTH;
    // Only for the load-state arms below; the rows themselves come from the
    // cache, which resolves the repository itself.
    let repo = this.state.repos.iter().find(|r| r.id == repo_id);

    let header = div()
        .px(scaled_px(8.0))
        .py(scaled_px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_col()
                .min_w(px(0.0))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .child(crate::i18n::tr("pick.upstream.title")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .line_height(scaled_px(14.0))
                        .child(
                            components::TruncatedText::new(branch.clone())
                                .id(("upstream_picker_title_branch", repo_id.0))
                                .text_color(theme.colors.foreground.secondary)
                                .full_text_tooltip(this.tooltip_host.clone())
                                .render(cx),
                        ),
                ),
        )
        .child(
            components::Button::new(
                "upstream_picker_close",
                crate::i18n::tr("prompts.file_history.close"),
            )
            .style(components::ButtonStyle::Outlined)
            .on_click(theme, cx, |this, _e, _w, cx| this.close_popover(cx)),
        );

    let body: AnyElement = match repo.map(|r| &r.remote_branches) {
        None => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.no_repository"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Loading) => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.loading"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Error(e)) => components::context_menu_label(
            theme,
            ui_scale_percent,
            e.clone(),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::NotLoaded) => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.not_loaded"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Ready(_)) => {
            if let Some(search) = this.upstream_picker_search_input.clone() {
                let query = search.read(cx).text().trim().to_string();
                let built = cached(this, repo_id, &branch, &query);
                let row_payloads = std::rc::Rc::clone(&built.payloads);
                components::PickerPrompt::new(search, this.picker_prompt_scroll.clone())
                    .prebuilt_items(
                        std::rc::Rc::clone(&built.items),
                        std::rc::Rc::clone(&built.layout),
                    )
                    .tooltip_host(this.tooltip_host.clone())
                    .empty_text(crate::i18n::tr("ui.picker.upstream.no_remote_branches"))
                    .max_height(scaled_px(components::PICKER_LIST_MAX_HEIGHT_PX))
                    .selected_index(this.upstream_picker_selected_index)
                    .marked_index(built.marked_index)
                    .render(theme, ui_scale_percent, cx, move |this, ix, _e, _w, cx| {
                        let Some(row) = row_payloads.get(ix).cloned() else {
                            return;
                        };
                        activate(this, repo_id, branch.clone(), row, cx);
                    })
                    .into_any_element()
            } else {
                components::context_menu_label(
                    theme,
                    ui_scale_percent,
                    crate::i18n::tr("ui.common.search_input_not_initialized"),
                    Some(this.tooltip_host.clone()),
                    cx,
                )
                .into_any_element()
            }
        }
    };

    components::context_menu(
        theme,
        div()
            .flex()
            .flex_col()
            .w(width.preferred_px(ui_scale))
            .child(header)
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A named revision bump, and the label the assertion reports it under —
    /// the same shape the rows_cache tests use.
    type RevisionBump = (&'static str, fn(&mut RepoState));

    fn repo() -> RepoState {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            gitcomet_core::domain::RepoSpec {
                workdir: std::path::PathBuf::from("/tmp/upstream_picker"),
            },
        );
        repo.open = Loadable::Ready(());
        repo.head_branch = Loadable::Ready("feature".to_string());
        repo.branches = Loadable::Ready(std::sync::Arc::new(vec![
            gitcomet_core::domain::Branch {
                name: "feature".to_string(),
                target: CommitId("aaaa".into()),
                upstream: Some(gitcomet_core::domain::Upstream {
                    remote: "origin".to_string(),
                    branch: "main".to_string(),
                }),
                divergence: None,
            },
            gitcomet_core::domain::Branch {
                name: "spare".to_string(),
                target: CommitId("bbbb".into()),
                upstream: None,
                divergence: None,
            },
        ]));
        repo.remote_branches = Loadable::Ready(std::sync::Arc::new(vec![
            gitcomet_core::domain::RemoteBranch {
                remote: "origin".to_string(),
                name: "main".to_string(),
                target: CommitId("aaaa".into()),
            },
            gitcomet_core::domain::RemoteBranch {
                remote: "origin".to_string(),
                name: "dev".to_string(),
                target: CommitId("cccc".into()),
            },
        ]));
        repo
    }

    #[test]
    fn rows_list_remote_branches_and_mark_the_tracked_one() {
        let repo = repo();
        let built = rows(&repo, "feature", "");

        // The unlink action leads, then every remote branch in order.
        assert_eq!(
            built.rows,
            vec![
                UpstreamRow::Unlink,
                UpstreamRow::RemoteBranch("origin/main".to_string()),
                UpstreamRow::RemoteBranch("origin/dev".to_string()),
            ]
        );
        // The tracked branch is the marked row.
        assert_eq!(built.marked_index, Some(1));
    }

    #[test]
    fn rows_without_upstream_offer_no_unlink_row() {
        let repo = repo();
        let built = rows(&repo, "spare", "");

        assert_eq!(
            built.rows,
            vec![
                UpstreamRow::RemoteBranch("origin/main".to_string()),
                UpstreamRow::RemoteBranch("origin/dev".to_string()),
            ]
        );
        assert_eq!(built.marked_index, None);
    }

    #[test]
    fn every_input_the_upstream_rows_read_invalidates_them() {
        let bumps: Vec<RevisionBump> = vec![
            ("branches_rev", |repo: &mut RepoState| {
                repo.branches_rev = repo.branches_rev.wrapping_add(1)
            }),
            ("remote_branches_rev", |repo: &mut RepoState| {
                repo.remote_branches_rev = repo.remote_branches_rev.wrapping_add(1)
            }),
        ];

        for (label, bump) in bumps {
            let mut repo = repo();
            let before = rows_signature(&repo, "feature");
            bump(&mut repo);
            assert_ne!(
                before,
                rows_signature(&repo, "feature"),
                "{label} must invalidate the upstream rows"
            );
        }
    }
}
