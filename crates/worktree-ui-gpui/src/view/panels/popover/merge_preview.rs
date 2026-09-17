use super::*;
use worktree_core::services::MergeChangedFile;

fn short_sha(id: &str) -> String {
    id.get(0..8).unwrap_or(id).to_string()
}

/// One changed-file line: its path, then `+added −removed`. A binary file has
/// no line counts, so git reports `-` and the row says so instead.
fn changed_file_row(file: &MergeChangedFile) -> String {
    match (file.additions, file.deletions) {
        (Some(additions), Some(deletions)) => crate::i18n::t!(
            "prompts.merge_preview.file_row",
            path = file.path.clone(),
            added = additions,
            removed = deletions
        )
        .into_owned(),
        _ => crate::i18n::t!(
            "prompts.merge_preview.file_row_binary",
            path = file.path.clone()
        )
        .into_owned(),
    }
}

/// Read-only preview of merging `other` into the current HEAD.
///
/// The preview comes from `git merge-tree --write-tree`, which computes the
/// merged tree entirely in memory — the worktree, index and refs are never
/// touched. The panel therefore carries no confirm action, only a Close
/// button: it exists to answer "would this merge conflict?" before the user
/// commits to the real merge. The result is requested on open
/// (`Msg::PreviewMerge`) and rendered from `repo.history_state.merge_preview`.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    other: CommitId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let preview = this
        .state
        .repos
        .iter()
        .find(|r| r.id == repo_id)
        .map(|repo| repo.history_state.merge_preview.clone())
        .unwrap_or(Loadable::NotLoaded);

    let mut dialog = ConfirmDialog::new(
        crate::i18n::tr("prompts.merge_preview.title"),
        DIALOG_420_WIDTH,
    );
    dialog = dialog.mono_value(
        theme,
        crate::i18n::t!(
            "prompts.merge_preview.target",
            other = short_sha(other.as_ref())
        )
        .into_owned(),
    );

    match &preview {
        Loadable::Ready(result) if result.has_conflict => {
            let rows: Vec<gpui::Div> = result
                .conflicts
                .iter()
                .map(|conflict| {
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(theme.colors.status.danger.foreground)
                        .child(
                            crate::i18n::t!(
                                "prompts.merge_preview.conflict_row",
                                path = conflict.path.clone(),
                                kind = conflict.conflict_type.clone(),
                            )
                            .into_owned(),
                        )
                })
                .collect();
            dialog = dialog.section(
                div()
                    .id("merge_preview_conflicts")
                    .debug_selector(|| "merge_preview_conflicts".to_string())
                    .flex()
                    .flex_col()
                    .children(rows),
            );
            dialog = dialog.note(
                theme,
                crate::i18n::t!(
                    "prompts.merge_preview.conflict_count",
                    count = result.conflicts.len()
                )
                .into_owned(),
            );
        }
        Loadable::Ready(result) => {
            dialog = dialog.section(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.colors.foreground.primary)
                    .debug_selector(|| "merge_preview_clean".to_string())
                    .child(crate::i18n::tr("prompts.merge_preview.clean")),
            );
            if result.files.is_empty() {
                dialog = dialog.note(theme, crate::i18n::tr("prompts.merge_preview.no_changes"));
            } else {
                let rows: Vec<gpui::Div> = result
                    .files
                    .iter()
                    .map(|file| {
                        div()
                            .px_2()
                            .text_sm()
                            .text_color(theme.colors.foreground.secondary)
                            .child(changed_file_row(file))
                    })
                    .collect();
                dialog = dialog.section(
                    div()
                        .id("merge_preview_files")
                        .debug_selector(|| "merge_preview_files".to_string())
                        .flex()
                        .flex_col()
                        .children(rows),
                );
                dialog = dialog.note(
                    theme,
                    crate::i18n::t!(
                        "prompts.merge_preview.changed_count",
                        count = result.files.len()
                    )
                    .into_owned(),
                );
            }
        }
        Loadable::Loading => {
            dialog = dialog.text(theme, crate::i18n::tr("prompts.merge_preview.loading"));
        }
        // `NotLoaded` here means the preview resolved to nothing — either HEAD
        // moved between the request and its answer, or the open path never
        // fired (the reducer already toasted why). `Error` carries git's own
        // rejection, e.g. unrelated histories.
        Loadable::NotLoaded | Loadable::Error(_) => {
            let message: SharedString = match &preview {
                Loadable::Error(message) => message.as_str().into(),
                _ => crate::i18n::tr("prompts.merge_preview.empty"),
            };
            dialog = dialog.section(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.colors.foreground.secondary)
                    .debug_selector(|| "merge_preview_unavailable".to_string())
                    .child(message),
            );
        }
    }

    let close = components::Button::new(
        "merge_preview_close",
        crate::i18n::tr("panels.merge_preview.close"),
    )
    .style(components::ButtonStyle::Outlined)
    .on_click(theme, cx, move |this, _e, _w, cx| {
        this.store.dispatch(Msg::CancelMergePreview { repo_id });
        this.close_popover(cx);
    })
    .debug_selector(|| "merge_preview_close".to_string());

    dialog.render(theme, close, div(), cx)
}
