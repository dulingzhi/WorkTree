use super::*;

fn short_sha(id: &str) -> String {
    id.get(0..8).unwrap_or(id).to_string()
}

/// Confirmation popover for an autosquash: lists every `fixup!`/`squash!`
/// commit that will fold into its target, and on confirm dispatches
/// `Msg::ConfirmAutosquash` (a non-interactive rebase of `base..HEAD`). The
/// fold is computed asynchronously on open (`Msg::Autosquash`); this panel
/// only renders `repo.history_state.autosquash_preview`.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    _base: String,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let preview = this
        .state
        .repos
        .iter()
        .find(|r| r.id == repo_id)
        .map(|repo| repo.history_state.autosquash_preview.clone())
        .unwrap_or(Loadable::NotLoaded);

    // Only a plan with at least one fold is actionable; a `Loading` preview or
    // an empty `NotLoaded` (nothing to fold / head drifted) keeps Confirm off.
    let plan = match &preview {
        Loadable::Ready(plan) if plan.folded_count() > 0 => Some(plan),
        _ => None,
    };

    let title: SharedString = match plan {
        Some(plan) => crate::i18n::t!(
            "prompts.autosquash.title_count",
            count = plan.folded_count()
        )
        .into_owned()
        .into(),
        None => crate::i18n::tr("prompts.autosquash.title"),
    };

    let mut dialog = ConfirmDialog::new(title, DIALOG_420_WIDTH);

    match plan {
        Some(plan) => {
            let rows: Vec<gpui::Div> = plan
                .folds
                .iter()
                .flat_map(|fold| {
                    let sha = short_sha(&fold.target_commit_id);
                    let target = fold.target_summary.clone();
                    fold.fixups.iter().map(move |fixup| {
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(theme.colors.foreground.primary)
                            .child(
                                crate::i18n::t!(
                                    "prompts.autosquash.fold_row",
                                    fixup = fixup.summary.clone(),
                                    sha = sha.clone(),
                                    summary = target.clone(),
                                )
                                .into_owned(),
                            )
                    })
                })
                .collect();
            dialog = dialog.section(div().flex().flex_col().gap_1().children(rows));
            dialog = dialog.note(
                theme,
                crate::i18n::t!("prompts.autosquash.count_line", count = plan.folded_count())
                    .into_owned(),
            );
        }
        None => {
            // `Loading` flashes only before the first model update (the open
            // path sets Loading synchronously); `NotLoaded` here means the fold
            // resolved to nothing — the reducer already toasted why.
            let message = match &preview {
                Loadable::Loading => crate::i18n::tr("prompts.autosquash.loading"),
                _ => crate::i18n::tr("prompts.autosquash.empty"),
            };
            dialog = dialog.text(theme, message);
        }
    }

    let confirm = components::Button::new(
        "autosquash_confirm",
        crate::i18n::tr("prompts.autosquash.confirm"),
    )
    .style(components::ButtonStyle::Filled)
    .disabled(plan.is_none())
    .on_click(theme, cx, move |this, _e, _w, cx| {
        this.store.dispatch(Msg::ConfirmAutosquash { repo_id });
        this.close_popover(cx);
    });

    let cancel = super::cancel_button("autosquash_cancel", "autosquash_cancel_hint", theme)
        .on_click(theme, cx, move |this, _e, _w, cx| {
            this.store.dispatch(Msg::CancelAutosquash { repo_id });
            this.close_popover(cx);
        });

    dialog.render(theme, cancel, confirm, cx)
}
