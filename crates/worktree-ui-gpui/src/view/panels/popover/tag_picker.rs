//! The palette's delete-tag picker.
//!
//! Activation deletes without a confirm — the tag context menu does the same,
//! and a deleted tag is recoverable from the reflog until the referenced
//! commits go away.

use super::*;
use std::rc::Rc;

/// Height the row list caps at. Shared with the keyboard navigation that
/// scrolls it, for the same windowing reason as the remote picker's.
pub(super) const TAG_PICKER_LIST_MAX_HEIGHT_PX: f32 = 240.0;

fn repo_for(this: &PopoverHost, repo_id: RepoId) -> Option<&RepoState> {
    this.state.repos.iter().find(|repo| repo.id == repo_id)
}

/// Everything the rows below read.
fn rows_signature(this: &PopoverHost, repo_id: RepoId) -> u64 {
    use std::hash::Hash;

    super::rows_cache::signature(|hasher| {
        let Some(repo) = repo_for(this, repo_id) else {
            return;
        };
        repo.id.hash(hasher);
        repo.tags_rev.hash(hasher);
        super::rows_cache::loadable_kind(&repo.tags).hash(hasher);
    })
}

/// The rows for `query`, built once per change to the tag list. Payloads are
/// tag names — the exact `name` `Msg::DeleteTag` takes.
pub(super) fn cached(
    this: &PopoverHost,
    repo_id: RepoId,
    query: &str,
) -> Rc<super::rows_cache::CachedRows<String>> {
    let key = super::rows_cache::RowsCacheKey::new(
        super::rows_cache::RowsCacheOwner::Tag,
        rows_signature(this, repo_id),
        query,
    );
    super::rows_cache::get_or_build(&this.tag_picker_rows_cache, key, |_now| {
        let Some(Loadable::Ready(tags)) = repo_for(this, repo_id).map(|repo| &repo.tags) else {
            return (Vec::new(), Vec::new(), None);
        };
        let (items, payloads) = tags
            .iter()
            .map(|tag| {
                (
                    components::PickerPromptItem::single(
                        tag.name.clone(),
                        components::TextTruncationProfile::Path,
                    ),
                    tag.name.clone(),
                )
            })
            .unzip();
        (items, payloads, None)
    })
}

pub(super) fn nav_targets(this: &PopoverHost, repo_id: RepoId, query: &str) -> Vec<String> {
    cached(this, repo_id, query).filtered_payloads()
}

/// Deletes the tag. Shared by the click handler and Enter.
pub(super) fn activate(
    this: &mut PopoverHost,
    repo_id: RepoId,
    name: String,
    cx: &mut gpui::Context<PopoverHost>,
) {
    this.store.dispatch(Msg::DeleteTag { repo_id, name });
    this.close_popover(cx);
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

    let Some(repo) = repo_for(this, repo_id) else {
        return label(this, crate::i18n::tr("ui.common.no_repository"), cx);
    };
    match &repo.tags {
        Loadable::Loading => return label(this, crate::i18n::tr("ui.common.loading"), cx),
        Loadable::NotLoaded => return label(this, crate::i18n::tr("ui.common.not_loaded"), cx),
        Loadable::Error(e) => {
            let e = e.clone();
            return label(this, e.into(), cx);
        }
        Loadable::Ready(_) => {}
    }

    let Some(search) = this.tag_picker_search_input.clone() else {
        return label(
            this,
            crate::i18n::tr("ui.common.search_input_not_initialized"),
            cx,
        );
    };
    let query = search.read(cx).text().trim().to_string();
    let built = cached(this, repo_id, &query);
    let names = Rc::clone(&built.payloads);

    search.update(cx, |input, cx| {
        input.set_chromeless(true, cx);
        input.set_leading_icon(Some("icons/tag.svg"), cx);
    });

    let menu = div()
        .flex()
        .flex_col()
        .min_w(width.min_px(ui_scale))
        .max_w(width.max_px(ui_scale))
        .child(super::popover_title(crate::i18n::tr(
            "palette.cmd.delete-tag",
        )))
        .child(div().border_t_1().border_color(theme.colors.stroke.default))
        .child(
            components::PickerPrompt::new(search, this.picker_prompt_scroll.clone())
                .prebuilt_items(Rc::clone(&built.items), Rc::clone(&built.layout))
                .tooltip_host(this.tooltip_host.clone())
                .empty_text(crate::i18n::tr("ui.picker.tag.empty"))
                .max_height(scaled_px(TAG_PICKER_LIST_MAX_HEIGHT_PX))
                .selected_index(this.tag_picker_selected_index)
                .render(theme, ui_scale_percent, cx, move |this, ix, _e, _window, cx| {
                    let Some(name) = names.get(ix).cloned() else {
                        return;
                    };
                    activate(this, repo_id, name, cx);
                }),
        );

    components::context_menu(theme, menu).w(width.preferred_px(ui_scale))
}
