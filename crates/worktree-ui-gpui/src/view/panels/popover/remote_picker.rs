//! The palette's remote picker — one list, three destinations.
//!
//! `DeleteBranch` lists `remote/branch` refs; `RemoveRemote` and `EditUrl`
//! list remotes. Activation never acts directly: it opens the confirm (or the
//! remote's context menu, for `EditUrl`), so every flow ends in the same UI
//! the sidebar reaches.

use super::*;
use std::rc::Rc;

/// Height the row list caps at. Shared with the keyboard navigation that
/// scrolls it — the list is windowed, so a navigation assuming another
/// viewport would scroll to the wrong place.
pub(super) const REMOTE_PICKER_LIST_MAX_HEIGHT_PX: f32 = 240.0;

/// Which row a payload came from — the activation arms read exactly one of the
/// two variants per purpose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RemotePickerRow {
    Remote { name: String },
    RemoteBranch { remote: String, branch: String },
}

fn repo_for(this: &PopoverHost, repo_id: RepoId) -> Option<&RepoState> {
    this.state.repos.iter().find(|repo| repo.id == repo_id)
}

/// The status-line text for a `Loadable` that has no rows to show yet. `None`
/// means ready — build the rows. Generic because the two row sources carry
/// different payloads.
fn not_ready_text<T>(loadable: &Loadable<T>) -> Option<gpui::SharedString> {
    match loadable {
        Loadable::Ready(_) => None,
        Loadable::Loading => Some(crate::i18n::tr("ui.common.loading")),
        Loadable::NotLoaded => Some(crate::i18n::tr("ui.common.not_loaded")),
        Loadable::Error(e) => Some(e.clone().into()),
    }
}

/// Everything the rows below read. The purpose is part of the key because it
/// selects the row source — the remotes list and the remote-branch list move
/// independently.
fn rows_signature(this: &PopoverHost, repo_id: RepoId, purpose: RemotePickerPurpose) -> u64 {
    use std::hash::Hash;

    super::rows_cache::signature(|hasher| {
        let Some(repo) = repo_for(this, repo_id) else {
            return;
        };
        repo.id.hash(hasher);
        purpose.hash(hasher);
        match purpose {
            RemotePickerPurpose::DeleteBranch => {
                repo.remote_branches_rev.hash(hasher);
                super::rows_cache::loadable_kind(&repo.remote_branches).hash(hasher);
            }
            RemotePickerPurpose::RemoveRemote | RemotePickerPurpose::EditUrl => {
                repo.remotes_rev.hash(hasher);
                super::rows_cache::loadable_kind(&repo.remotes).hash(hasher);
            }
        }
    })
}

/// The rows for `query`, built once per change to the underlying list.
pub(super) fn cached(
    this: &PopoverHost,
    repo_id: RepoId,
    purpose: RemotePickerPurpose,
    query: &str,
) -> Rc<super::rows_cache::CachedRows<RemotePickerRow>> {
    let key = super::rows_cache::RowsCacheKey::new(
        super::rows_cache::RowsCacheOwner::Remote,
        rows_signature(this, repo_id, purpose),
        query,
    );
    super::rows_cache::get_or_build(&this.remote_picker_rows_cache, key, |_now| {
        let Some(repo) = repo_for(this, repo_id) else {
            return (Vec::new(), Vec::new(), None);
        };
        let rows: Vec<(components::PickerPromptItem, RemotePickerRow)> = match purpose {
            RemotePickerPurpose::DeleteBranch => {
                let Loadable::Ready(branches) = &repo.remote_branches else {
                    return (Vec::new(), Vec::new(), None);
                };
                branches
                    .iter()
                    .map(|branch| {
                        (
                            components::PickerPromptItem::single(
                                format!("{}/{}", branch.remote, branch.name),
                                components::TextTruncationProfile::Path,
                            ),
                            RemotePickerRow::RemoteBranch {
                                remote: branch.remote.clone(),
                                branch: branch.name.clone(),
                            },
                        )
                    })
                    .collect()
            }
            RemotePickerPurpose::RemoveRemote | RemotePickerPurpose::EditUrl => {
                let Loadable::Ready(remotes) = &repo.remotes else {
                    return (Vec::new(), Vec::new(), None);
                };
                remotes
                    .iter()
                    .map(|remote| {
                        (
                            components::PickerPromptItem::single(
                                remote.name.clone(),
                                components::TextTruncationProfile::Path,
                            ),
                            RemotePickerRow::Remote {
                                name: remote.name.clone(),
                            },
                        )
                    })
                    .collect()
            }
        };
        let (items, payloads) = rows.into_iter().unzip();
        (items, payloads, None)
    })
}

pub(super) fn nav_targets(
    this: &PopoverHost,
    repo_id: RepoId,
    purpose: RemotePickerPurpose,
    query: &str,
) -> Vec<RemotePickerRow> {
    cached(this, repo_id, purpose, query).filtered_payloads()
}

/// Opens the destination popover for the picked row: the delete/remote confirm
/// for the acting purposes, or the remote's context menu for `EditUrl` (its
/// fetch- and push-URL entries are two distinct edits, so the menu is where
/// both stay reachable). Shared by the click handler and Enter.
pub(super) fn activate(
    this: &mut PopoverHost,
    repo_id: RepoId,
    purpose: RemotePickerPurpose,
    row: RemotePickerRow,
    position: Option<gpui::Point<gpui::Pixels>>,
    window: &mut Window,
    cx: &mut gpui::Context<PopoverHost>,
) {
    let kind = PopoverKind::remote(
        repo_id,
        match (purpose, row) {
            (
                RemotePickerPurpose::DeleteBranch,
                RemotePickerRow::RemoteBranch { remote, branch },
            ) => RemotePopoverKind::DeleteBranchConfirm { remote, branch },
            (RemotePickerPurpose::RemoveRemote, RemotePickerRow::Remote { name }) => {
                RemotePopoverKind::RemoveConfirm { name }
            }
            (RemotePickerPurpose::EditUrl, RemotePickerRow::Remote { name }) => {
                RemotePopoverKind::Menu { name }
            }
            // The purpose decides the row source, so the payload shape cannot
            // disagree with it; a mismatched pair is a wiring bug, not user
            // state, and the no-op keeps the picker open either way.
            _ => return,
        },
    );
    match position {
        Some(position) => this.open_popover_at(kind, position, window, cx),
        None => this.open_popover_centered(kind, window, cx),
    }
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    purpose: RemotePickerPurpose,
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

    let Some(repo) = repo_for(this, repo_id) else {
        return label(this, crate::i18n::tr("ui.common.no_repository"), cx);
    };
    // The two row sources are different `Loadable<T>`s, so the not-ready
    // check is generic over the payload.
    if let Some(text) = if purpose == RemotePickerPurpose::DeleteBranch {
        not_ready_text(&repo.remote_branches)
    } else {
        not_ready_text(&repo.remotes)
    } {
        return label(this, text, cx);
    }

    let Some(search) = this.remote_picker_search_input.clone() else {
        return label(
            this,
            crate::i18n::tr("ui.common.search_input_not_initialized"),
            cx,
        );
    };
    let query = search.read(cx).text().trim().to_string();
    let built = cached(this, repo_id, purpose, &query);
    let rows = Rc::clone(&built.payloads);

    // The title repeats the palette command so the picker names its own
    // destination, the way the branch picker's non-checkout purposes do.
    let title = match purpose {
        RemotePickerPurpose::DeleteBranch => crate::i18n::tr("palette.cmd.delete-remote-branch"),
        RemotePickerPurpose::RemoveRemote => crate::i18n::tr("palette.cmd.remove-remote"),
        RemotePickerPurpose::EditUrl => crate::i18n::tr("palette.cmd.edit-remote-url"),
    };
    let empty_text = if purpose == RemotePickerPurpose::DeleteBranch {
        crate::i18n::tr("ui.picker.remote_branch.empty")
    } else {
        crate::i18n::tr("ui.picker.remote.empty")
    };

    search.update(cx, |input, cx| {
        input.set_chromeless(true, cx);
        input.set_leading_icon(Some("icons/cloud.svg"), cx);
    });

    let menu = div()
        .flex()
        .flex_col()
        .min_w(width.min_px(ui_scale))
        .max_w(width.max_px(ui_scale))
        .child(super::popover_title(title))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            components::PickerPrompt::new(search, this.picker_prompt_scroll.clone())
                .prebuilt_items(Rc::clone(&built.items), Rc::clone(&built.layout))
                .tooltip_host(this.tooltip_host.clone())
                .empty_text(empty_text)
                .max_height(scaled_px(REMOTE_PICKER_LIST_MAX_HEIGHT_PX))
                .selected_index(this.remote_picker_selected_index)
                .render(
                    theme,
                    ui_scale_percent,
                    cx,
                    move |this, ix, e, window, cx| {
                        let Some(row) = rows.get(ix).cloned() else {
                            return;
                        };
                        activate(
                            this,
                            repo_id,
                            purpose,
                            row,
                            Some(e.position()),
                            window,
                            cx,
                        );
                    },
                ),
        );

    components::context_menu(theme, menu).w(width.preferred_px(ui_scale))
}
