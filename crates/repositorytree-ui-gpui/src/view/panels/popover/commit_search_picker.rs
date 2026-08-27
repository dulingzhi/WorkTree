//! The palette's commit search — two tiers in one list.
//!
//! Tier one is instant: rows from the log page the history view already
//! loaded, filtered by the prompt as usual. Tier two is the trailing action
//! row, which runs the cross-history `git log --all` search; its results join
//! the list under their own section once they land, ranked after the local
//! matches and safe from the prompt's filter (a match can live in a commit
//! body the row never shows). Activation reveals the commit in the history
//! view.

use super::*;
use repositorytree_core::domain::{Commit, CommitId};
use std::collections::HashSet;
use std::rc::Rc;

/// Height the row list caps at. Shared with the keyboard navigation that
/// scrolls it, for the same windowing reason as the remote picker's.
pub(super) const COMMIT_SEARCH_PICKER_LIST_MAX_HEIGHT_PX: f32 = 240.0;

/// What activating a row does: reveal a commit, or run the cross-history
/// search for the typed query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CommitSearchPickerRow {
    Commit(CommitId),
    SearchAll,
}

fn repo_for(this: &PopoverHost, repo_id: RepoId) -> Option<&RepoState> {
    this.state.repos.iter().find(|repo| repo.id == repo_id)
}

/// One commit row, shared by both tiers. The summary leads; author, relative
/// time and short sha sit on the detail line. Author and sha stay searchable
/// so typing a name or a prefix finds the row — the local tier's whole match
/// semantics.
fn commit_row(commit: &Commit, now: std::time::SystemTime) -> components::PickerPromptItem {
    let id = commit.id.as_ref();
    let short = &id[..id.len().min(7)];
    let unix_secs = commit
        .time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    components::PickerPromptItem::from_parts([components::PickerPromptItemPart::new(
        commit.summary.to_string(),
    )
    .profile(components::TextTruncationProfile::End)])
    .secondary_parts([
        components::PickerPromptItemPart::new(commit.author.to_string()),
        components::PickerPromptItemPart::separator("  •  "),
        components::PickerPromptItemPart::new(crate::view::date_time::format_relative_time(
            unix_secs, now,
        ))
        .searchable(false)
        .flexible(false)
        .tooltip(false),
        components::PickerPromptItemPart::separator("  •  "),
        components::PickerPromptItemPart::new(short.to_string())
            .flexible(false)
            .tooltip(false),
    ])
}

/// The trailing action row: prefix plus the query, like the checkout picker's
/// create row, so the typed text stays visible in the row itself. Its detail
/// line says what a deep search covers — or that one is running.
fn search_all_row(query: &str, repo: &RepoState) -> components::PickerPromptItem {
    let hint = if repo.commit_search_query.as_deref() == Some(query) {
        match &repo.commit_search {
            Loadable::Loading => crate::i18n::tr("ui.common.loading"),
            Loadable::Error(e) => e.clone().into(),
            _ => crate::i18n::tr("ui.picker.commit_search.hint"),
        }
    } else {
        crate::i18n::tr("ui.picker.commit_search.hint")
    };
    components::PickerPromptItem::from_parts([
        components::PickerPromptItemPart::new(crate::i18n::tr(
            "ui.picker.commit_search.search_all_prefix",
        ))
        .flexible(false)
        .searchable(false)
        .tooltip(false),
        components::PickerPromptItemPart::new(query.to_string()).flexible(false),
    ])
    .secondary_parts([components::PickerPromptItemPart::new(hint).searchable(false)])
    .icon("icons/zoom.svg")
}

/// Everything the rows below read: the loaded log page (tier one) and the
/// search results with their revision (tier two, including the Loading bump
/// that swaps the action row's hint to "loading").
fn rows_signature(this: &PopoverHost, repo_id: RepoId) -> u64 {
    use std::hash::Hash;

    super::rows_cache::signature(|hasher| {
        let Some(repo) = repo_for(this, repo_id) else {
            return;
        };
        repo.id.hash(hasher);
        repo.log_rev.hash(hasher);
        super::rows_cache::loadable_kind(&repo.log).hash(hasher);
        repo.commit_search_rev.hash(hasher);
        repo.commit_search_query.hash(hasher);
    })
}

/// The rows for `query`. Local matches first, deep results (only when they
/// belong to this exact query) after, action row last.
pub(super) fn cached(
    this: &PopoverHost,
    repo_id: RepoId,
    query: &str,
) -> Rc<super::rows_cache::CachedRows<CommitSearchPickerRow>> {
    let key = super::rows_cache::RowsCacheKey::new(
        super::rows_cache::RowsCacheOwner::CommitSearch,
        rows_signature(this, repo_id),
        query,
    );
    super::rows_cache::get_or_build(&this.commit_search_picker_rows_cache, key, |now| {
        let Some(repo) = repo_for(this, repo_id) else {
            return (Vec::new(), Vec::new(), None);
        };
        let query = query.trim();
        let mut items: Vec<components::PickerPromptItem> = Vec::new();
        let mut rows: Vec<CommitSearchPickerRow> = Vec::new();

        // Tier one: everything the loaded page holds. The prompt's filter is
        // the search — rows whose summary, author or sha don't contain the
        // query are dropped at layout time, not here.
        let mut loaded_ids: HashSet<&CommitId> = HashSet::new();
        if let Loadable::Ready(page) = &repo.log {
            for commit in &page.commits {
                loaded_ids.insert(&commit.id);
                items.push(
                    commit_row(commit, now).section(crate::i18n::tr(
                        "ui.picker.commit_search.section.loaded",
                    )),
                );
                rows.push(CommitSearchPickerRow::Commit(commit.id.clone()));
            }
        }

        // Tier two: only when the stored results answer this exact query — a
        // new query must not parade old results as its own. These rows were
        // vetted by the search itself, so they match any query and are never
        // re-filtered (a body-only match would otherwise vanish).
        if repo.commit_search_query.as_deref() == Some(query) && !query.is_empty() {
            if let Loadable::Ready(results) = &repo.commit_search {
                for commit in results.iter() {
                    // Already listed in the loaded section — a second row for
                    // it would only add noise.
                    if loaded_ids.contains(&commit.id) {
                        continue;
                    }
                    items.push(
                        commit_row(commit, now)
                            .section(crate::i18n::tr(
                                "ui.picker.commit_search.section.all_history",
                            ))
                            .match_any_query(),
                    );
                    rows.push(CommitSearchPickerRow::Commit(commit.id.clone()));
                }
            }
        }

        if !query.is_empty() {
            items.push(search_all_row(query, repo));
            rows.push(CommitSearchPickerRow::SearchAll);
        }

        (items, rows, None)
    })
}

pub(super) fn nav_targets(
    this: &PopoverHost,
    repo_id: RepoId,
    query: &str,
) -> Vec<CommitSearchPickerRow> {
    cached(this, repo_id, query).filtered_payloads()
}

/// Reveals the commit in the history view, or starts the cross-history
/// search. Shared by the click handler and Enter. A search already in flight
/// is a no-op — its results will arrive on their own.
pub(super) fn activate(
    this: &mut PopoverHost,
    repo_id: RepoId,
    row: CommitSearchPickerRow,
    cx: &mut gpui::Context<PopoverHost>,
) {
    match row {
        CommitSearchPickerRow::Commit(reference) => {
            this.store.dispatch(Msg::RevealCommit { repo_id, reference });
            this.close_popover(cx);
        }
        CommitSearchPickerRow::SearchAll => {
            let Some(search) = this.commit_search_picker_search_input.clone() else {
                return;
            };
            let query = search.read(cx).text().trim().to_string();
            if query.is_empty() || this.state.repos.iter().any(|repo| {
                repo.id == repo_id && matches!(repo.commit_search, Loadable::Loading)
            }) {
                return;
            }
            this.store.dispatch(Msg::SearchCommits { repo_id, query });
        }
    }
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale = super::popover_ui_scale(cx);
    let ui_scale_percent = ui_scale.percent();
    let width = super::PICKER_WIDTH;
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);

    let label =
        |this: &PopoverHost, text: gpui::SharedString, cx: &mut gpui::Context<PopoverHost>| {
            components::context_menu_label(
                theme,
                ui_scale_percent,
                text,
                Some(this.tooltip_host.clone()),
                cx,
            )
        };

    let Some(search) = this.commit_search_picker_search_input.clone() else {
        return label(
            this,
            crate::i18n::tr("ui.common.search_input_not_initialized"),
            cx,
        );
    };
    let query = search.read(cx).text().trim().to_string();
    let built = cached(this, repo_id, &query);
    let rows = Rc::clone(&built.payloads);

    search.update(cx, |input, cx| {
        input.set_chromeless(true, cx);
        input.set_leading_icon(Some("icons/zoom.svg"), cx);
    });

    let menu = div()
        .flex()
        .flex_col()
        .min_w(width.min_px(ui_scale))
        .max_w(width.max_px(ui_scale))
        .child(super::popover_title(crate::i18n::tr(
            "palette.cmd.search-commits",
        )))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            components::PickerPrompt::new(search, this.picker_prompt_scroll.clone())
                .prebuilt_items(Rc::clone(&built.items), Rc::clone(&built.layout))
                .tooltip_host(this.tooltip_host.clone())
                .empty_text(crate::i18n::tr("ui.picker.commit_search.empty"))
                .max_height(scaled_px(COMMIT_SEARCH_PICKER_LIST_MAX_HEIGHT_PX))
                .selected_index(this.commit_search_picker_selected_index)
                .render(theme, ui_scale_percent, cx, move |this, ix, _e, _window, cx| {
                    let Some(row) = rows.get(ix).cloned() else {
                        return;
                    };
                    activate(this, repo_id, row, cx);
                }),
        );

    components::context_menu(theme, menu).w(width.preferred_px(ui_scale))
}
