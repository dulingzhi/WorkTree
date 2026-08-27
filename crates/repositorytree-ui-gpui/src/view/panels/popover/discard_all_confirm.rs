use super::*;

/// Palette "Discard All Changes". Unlike `discard_changes_confirm`, which
/// resolves its paths from the status pane's selection, this dialog targets
/// every path that has a staged or unstaged change — so it stays correct with
/// nothing selected at all.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;

    // Union of both areas' paths, deduped in order. A path present in both is
    // discarded once; the per-path discard semantics already restore the whole
    // worktree state of the path.
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(repo) = this.state.repos.iter().find(|r| r.id == repo_id) {
        let unstaged = repo
            .worktree_status_entries()
            .map(|entries| entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let staged = repo
            .staged_status_entries()
            .map(|entries| entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        paths.reserve(unstaged.len() + staged.len());
        for path in unstaged.into_iter().chain(staged) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    let can_discard = !paths.is_empty();
    let detail = if can_discard {
        crate::i18n::t!("panels.discard_confirm.files_count", count = paths.len()).into_owned()
    } else {
        crate::i18n::tr_str("panels.discard_confirm.no_changes").to_string()
    };

    ConfirmDialog::new(
        crate::i18n::tr("panels.discard_confirm.title"),
        DIALOG_420_WIDTH,
    )
    .text(
        theme,
        crate::i18n::t!("panels.discard_confirm.body", detail = detail).into_owned(),
    )
    .render(
        theme,
        dialog_cancel_button(
            "discard_changes_cancel",
            "discard_changes_cancel_hint",
            theme,
            cx,
        ),
        components::Button::new(
            "discard_all_go",
            crate::i18n::tr("panels.discard_confirm.discard"),
        )
        .style(components::ButtonStyle::Danger)
        .disabled(!can_discard)
        .on_click(theme, cx, move |this, _e, _w, cx| {
            // Same dispatch pair the multi-path branch of
            // `discard_worktree_changes_confirmed` issues.
            this.store.dispatch(Msg::ClearDiffSelection { repo_id });
            this.store.dispatch(Msg::DiscardWorktreeChangesPaths {
                repo_id,
                paths: paths.clone(),
            });
            this.close_popover(cx);
        }),
        cx,
    )
}
