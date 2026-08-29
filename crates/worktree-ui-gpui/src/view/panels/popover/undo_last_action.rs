use super::*;
use worktree_core::services::SequencerState;
use worktree_core::undo::{UndoAction, UndoPlan, classify_undo};

/// What "undo the last action" means for a repository right now. An
/// in-progress operation is aborted rather than reversed; a completed one is
/// undone by resetting back to the reflog-recorded position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UndoResolution {
    /// A merge is waiting to be concluded — `git merge --abort`.
    AbortMerge,
    /// A rebase/apply/cherry-pick sequencer is mid-flight — the abort
    /// command the sequencer confirm already dispatches.
    AbortRebase,
    /// A completed operation whose reverse is a reset to `target`; `mode`
    /// is the current selection (the plan's suggestion until changed).
    ResetBack { plan: UndoPlan },
    /// Fewer than two reflog entries, or an operation this doesn't model.
    Nothing,
}

/// Resolves the undo for a loaded [`RepoState`]. `None` repo → `Nothing`.
pub(crate) fn resolve_undo(repo: Option<&RepoState>) -> UndoResolution {
    let Some(repo) = repo else {
        return UndoResolution::Nothing;
    };
    if matches!(&repo.merge_commit_message, Loadable::Ready(Some(_))) {
        return UndoResolution::AbortMerge;
    }
    if matches!(&repo.sequencer_state, Loadable::Ready(state) if *state != SequencerState::None)
        || matches!(&repo.rebase_in_progress, Loadable::Ready(true))
    {
        return UndoResolution::AbortRebase;
    }
    match &repo.reflog {
        Loadable::Ready(entries) => match classify_undo(entries) {
            Some(plan) => UndoResolution::ResetBack { plan },
            None => UndoResolution::Nothing,
        },
        _ => UndoResolution::Nothing,
    }
}

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let resolution = resolve_undo(this.state.repos.iter().find(|repo| repo.id == repo_id));

    match resolution {
        UndoResolution::AbortMerge => {
            ConfirmDialog::new(crate::i18n::tr("panels.undo.title"), DIALOG_380_WIDTH)
                .text(theme, crate::i18n::tr("confirm.merge_abort.body_merge"))
                .command(theme, "git merge --abort")
                .render(
                    theme,
                    dialog_cancel_button("undo_cancel", "undo_cancel_hint", theme, cx),
                    components::Button::new(
                        "undo_abort_merge_go",
                        crate::i18n::tr("panels.action_bar.abort_merge"),
                    )
                    .style(components::ButtonStyle::Danger)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        this.store.dispatch(Msg::MergeAbort { repo_id });
                        this.close_popover(cx);
                    })
                    .debug_selector(|| "undo_abort_merge_go".to_string()),
                    cx,
                )
        }
        UndoResolution::AbortRebase => {
            ConfirmDialog::new(crate::i18n::tr("panels.undo.title"), DIALOG_380_WIDTH)
                .text(
                    theme,
                    crate::i18n::tr("confirm.merge_abort.body_rebase_or_apply"),
                )
                .command(theme, "git rebase --abort")
                .render(
                    theme,
                    dialog_cancel_button("undo_cancel", "undo_cancel_hint", theme, cx),
                    components::Button::new(
                        "undo_abort_rebase_go",
                        crate::i18n::tr("panels.action_bar.abort"),
                    )
                    .style(components::ButtonStyle::Danger)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        this.store.dispatch(Msg::RebaseAbort { repo_id });
                        this.close_popover(cx);
                    })
                    .debug_selector(|| "undo_abort_rebase_go".to_string()),
                    cx,
                )
        }
        UndoResolution::ResetBack { plan } => {
            let UndoAction::ResetBack {
                target,
                default_mode,
            } = &plan.action;
            let mode = this.undo_reset_mode.unwrap_or(*default_mode);
            let target = target.as_ref().to_string();
            let short_target: SharedString = target.get(0..8).unwrap_or(&target).to_owned().into();
            let mode_note = match mode {
                ResetMode::Hard => crate::i18n::tr("confirm.reset.note_hard"),
                ResetMode::Mixed => crate::i18n::tr("confirm.reset.note_mixed"),
                ResetMode::Soft => crate::i18n::tr("confirm.reset.note_soft"),
            };
            let command_preview: SharedString = format!(
                "git reset {} {}",
                match mode {
                    ResetMode::Soft => "--soft",
                    ResetMode::Mixed => "--mixed",
                    ResetMode::Hard => "--hard",
                },
                target,
            )
            .into();

            ConfirmDialog::new(crate::i18n::tr("panels.undo.title"), DIALOG_440_WIDTH)
                .text(
                    theme,
                    crate::i18n::t!("panels.undo.operation", op = plan.operation.as_ref())
                        .to_string(),
                )
                .mono_value(
                    theme,
                    crate::i18n::t!("panels.undo.return_to", sha = short_target).to_string(),
                )
                .section(mode_chips(theme, mode, cx))
                .note(theme, mode_note)
                .command(theme, command_preview)
                .render(
                    theme,
                    dialog_cancel_button("undo_cancel", "undo_cancel_hint", theme, cx),
                    components::Button::new("undo_go", crate::i18n::tr("panels.undo.go"))
                        .style(if mode == ResetMode::Hard {
                            components::ButtonStyle::Danger
                        } else {
                            components::ButtonStyle::Filled
                        })
                        .on_click(theme, cx, move |this, _e, _w, cx| {
                            this.store.dispatch(Msg::Reset {
                                repo_id,
                                target: target.clone(),
                                mode,
                            });
                            this.close_popover(cx);
                        })
                        .debug_selector(|| "undo_go".to_string()),
                    cx,
                )
        }
        UndoResolution::Nothing => {
            ConfirmDialog::new(crate::i18n::tr("panels.undo.title"), DIALOG_380_WIDTH)
                .text(theme, crate::i18n::tr("panels.undo.nothing_body"))
                .render(
                    theme,
                    dialog_cancel_button("undo_cancel", "undo_cancel_hint", theme, cx),
                    components::Button::new(
                        "undo_nothing_ok",
                        crate::i18n::tr("panels.undo.close"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.close_popover(cx);
                    })
                    .debug_selector(|| "undo_nothing_ok".to_string()),
                    cx,
                )
        }
    }
}

/// The three reset modes as toggle chips, shaped after the statistics
/// popover's period tabs: selected chip carries the accent border.
fn mode_chips(
    theme: AppTheme,
    selected: ResetMode,
    cx: &mut gpui::Context<PopoverHost>,
) -> impl IntoElement {
    let scaled_px = super::popover_scaled_px_fn(cx);
    let mut row = div()
        .px_2()
        .py_1()
        .flex()
        .items_center()
        .gap(scaled_px(6.0))
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground.secondary)
                .child(crate::i18n::tr("panels.undo.mode_label")),
        );

    for (mode, key) in [
        (ResetMode::Soft, "soft"),
        (ResetMode::Mixed, "mixed"),
        (ResetMode::Hard, "hard"),
    ] {
        let is_selected = mode == selected;
        let border = if is_selected {
            theme.colors.accent.foreground
        } else {
            theme.colors.stroke.default
        };
        let text = if is_selected {
            theme.colors.accent.foreground
        } else {
            theme.colors.foreground.secondary
        };
        let label = match mode {
            ResetMode::Soft => crate::i18n::tr("panels.undo.mode_soft"),
            ResetMode::Mixed => crate::i18n::tr("panels.undo.mode_mixed"),
            ResetMode::Hard => crate::i18n::tr("panels.undo.mode_hard"),
        };
        row = row.child(
            div()
                .id(SharedString::from(format!("undo_mode_{key}")))
                .debug_selector(move || format!("undo_mode_chip_{key}"))
                .flex()
                .items_center()
                .px(scaled_px(10.0))
                .py(scaled_px(3.0))
                .rounded(scaled_px(theme.radii.row))
                .border_1()
                .border_color(border)
                .text_size(scaled_px(12.0))
                .text_color(text)
                .cursor(CursorStyle::PointingHand)
                .hover(move |s| s.bg(theme.hover_overlay()))
                .child(label)
                .on_click(cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                    this.undo_reset_mode = Some(mode);
                    cx.notify();
                })),
        );
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_core::domain::{CommitId, ReflogEntry};
    use worktree_state::model::RepoState;

    fn entry(sha: &str, message: &str) -> ReflogEntry {
        ReflogEntry {
            index: 0,
            new_id: CommitId(sha.into()),
            message: message.into(),
            time: None,
            selector: "HEAD@{0}".into(),
            author: "Alice".into(),
        }
    }

    fn repo_with(entries: Vec<ReflogEntry>) -> RepoState {
        let mut repo = RepoState::new_opening(
            RepoId(1),
            worktree_core::domain::RepoSpec {
                workdir: std::path::PathBuf::from("/tmp/undo-test"),
            },
        );
        repo.reflog = Loadable::Ready(Arc::new(entries));
        repo
    }

    fn in_progress_repo() -> RepoState {
        let mut repo = repo_with(vec![
            entry("b", "commit: mid-merge work"),
            entry("a", "commit: base"),
        ]);
        repo.merge_commit_message = Loadable::Ready(Some("merge msg".to_string()));
        repo
    }

    #[test]
    fn missing_repo_has_nothing_to_undo() {
        assert_eq!(resolve_undo(None), UndoResolution::Nothing);
    }

    #[test]
    fn a_merge_waiting_to_conclude_is_aborted_not_reset() {
        // The reflog alone would classify the newest commit as undoable; the
        // in-progress merge must win, because resetting mid-merge strands the
        // MERGE_HEAD state.
        assert_eq!(
            resolve_undo(Some(&in_progress_repo())),
            UndoResolution::AbortMerge
        );
    }

    #[test]
    fn a_rebase_or_cherry_pick_in_flight_is_aborted() {
        let mut repo = repo_with(vec![
            entry("b", "rebase (pick): second"),
            entry("a", "commit: base"),
        ]);
        repo.rebase_in_progress = Loadable::Ready(true);
        assert_eq!(resolve_undo(Some(&repo)), UndoResolution::AbortRebase);

        let mut repo = repo_with(vec![entry("b", "commit: pick"), entry("a", "commit: base")]);
        repo.sequencer_state = Loadable::Ready(SequencerState::CherryPick);
        assert_eq!(resolve_undo(Some(&repo)), UndoResolution::AbortRebase);
    }

    #[test]
    fn a_completed_operation_resolves_to_its_plan() {
        let repo = repo_with(vec![
            entry("b", "merge main: Fast-forward"),
            entry("a", "commit: base"),
        ]);
        let UndoResolution::ResetBack { plan } = resolve_undo(Some(&repo)) else {
            panic!("expected a reset-back plan");
        };
        assert_eq!(plan.kind, worktree_core::undo::UndoKind::Merge);
        let UndoAction::ResetBack {
            target,
            default_mode,
        } = plan.action;
        assert_eq!(target.as_ref(), "a");
        assert_eq!(default_mode, ResetMode::Mixed);
    }

    #[test]
    fn an_empty_or_unloaded_reflog_resolves_to_nothing() {
        assert_eq!(
            resolve_undo(Some(&repo_with(Vec::new()))),
            UndoResolution::Nothing
        );
        let mut repo = repo_with(Vec::new());
        repo.reflog = Loadable::NotLoaded;
        assert_eq!(resolve_undo(Some(&repo)), UndoResolution::Nothing);
    }
}
