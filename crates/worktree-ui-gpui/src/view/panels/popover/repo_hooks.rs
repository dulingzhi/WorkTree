use super::*;

/// Height the hook list caps itself at: the standard set is 19 names, so the
/// rows scroll rather than pushing the dialog off screen.
const REPO_HOOKS_LIST_MAX_HEIGHT_PX: f32 = 320.0;

/// Native manager for the repository's `.git/hooks` directory.
///
/// Lists every standard hook plus any user-defined hook in `repo.repo_hooks`
/// (requested on open via `Msg::RequestRepoHooks`), with per-row actions:
/// toggle the executable bit, edit the script in the external editor, or
/// delete it. Undefined hooks offer a single "Create" action that starts from
/// the `.sample` template when one exists, else a `#!/bin/sh` skeleton.
///
/// Every mutation round-trips through `InternalMsg::RepoHooksLoaded`, which
/// re-lists the directory — so the dialog stays open and only the touched row
/// changes face.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale_percent = super::popover_ui_scale_percent(cx);
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);
    let repo = this.state.repos.iter().find(|r| r.id == repo_id);
    let workdir = repo.map(|r| r.spec.workdir.clone());

    let header = div()
        .px(scaled_px(8.0))
        .py(scaled_px(4.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::BOLD)
                .child(crate::i18n::tr("prompts.repo_hooks.title")),
        )
        .child(
            components::Button::new(
                "repo_hooks_close",
                crate::i18n::tr("panels.repo_hooks.close"),
            )
            .style(components::ButtonStyle::Outlined)
            .on_click(theme, cx, move |this, _e, _w, cx| {
                this.store.dispatch(Msg::CancelRepoHooks { repo_id });
                this.close_popover(cx);
            })
            .debug_selector(|| "repo_hooks_close".to_string()),
        );

    let body: AnyElement = match repo.map(|r| &r.repo_hooks) {
        None => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("ui.common.no_repository"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        // The open path dispatches the load, so NotLoaded is the brief moment
        // before the Loading transition lands — same face for both.
        Some(Loadable::NotLoaded) | Some(Loadable::Loading) => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("prompts.repo_hooks.loading"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Error(e)) => div()
            .id("repo_hooks_error")
            .debug_selector(|| "repo_hooks_error".to_string())
            .px_2()
            .py_1()
            .text_sm()
            .text_color(theme.colors.status.danger.foreground)
            .child(e.clone())
            .into_any_element(),
        Some(Loadable::Ready(list)) if list.0.is_empty() => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("prompts.repo_hooks.empty"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Ready(list)) => {
            let list = list.clone();
            div()
                .id("repo_hooks_rows")
                .debug_selector(|| "repo_hooks_rows".to_string())
                .max_h(scaled_px(REPO_HOOKS_LIST_MAX_HEIGHT_PX))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(rows(theme, repo_id, &list, workdir, cx))
                .into_any_element()
        }
    };

    components::context_menu(
        theme,
        div()
            .id("repo_hooks_manager")
            .debug_selector(|| "repo_hooks_manager".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(540.0))
            .child(header)
            .child(super::dialog_divider(theme))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr("prompts.repo_hooks.hint")),
            )
            .child(body),
    )
}

/// One row per hook: the name and its state on the left, the actions the row
/// currently supports on the right.
fn rows(
    theme: AppTheme,
    repo_id: RepoId,
    list: &std::sync::Arc<worktree_core::domain::RepoHookList>,
    workdir: Option<std::path::PathBuf>,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let mut row_list = div().flex().flex_col();
    for (ix, hook) in list.0.iter().enumerate() {
        let name = hook.name.0.clone();
        let hook_name = hook.name.clone();
        let state_label: gpui::SharedString = if !hook.defined {
            crate::i18n::tr("prompts.repo_hooks.not_defined")
        } else if hook.enabled {
            crate::i18n::tr("prompts.repo_hooks.enabled")
        } else {
            crate::i18n::tr("prompts.repo_hooks.disabled")
        };

        let mut actions = div().flex().items_center().gap_1();
        if !hook.defined {
            let create_name = hook_name.clone();
            let from_sample = hook.has_sample;
            actions = actions.child(
                components::Button::new(
                    format!("repo_hooks_create_{ix}"),
                    crate::i18n::tr("prompts.repo_hooks.create"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, _cx| {
                    // Stays open: the re-list that follows swaps this row's
                    // face to "enabled + edit/delete".
                    this.store.dispatch(Msg::CreateRepoHook {
                        repo_id,
                        name: create_name.clone(),
                        from_sample,
                    });
                })
                .debug_selector(move || format!("repo_hooks_create_{ix}")),
            );
        } else {
            let toggle_name = hook_name.clone();
            let toggle_label = if hook.enabled {
                crate::i18n::tr("prompts.repo_hooks.disable")
            } else {
                crate::i18n::tr("prompts.repo_hooks.enable")
            };
            let enabled = !hook.enabled;
            actions = actions.child(
                components::Button::new(format!("repo_hooks_toggle_{ix}"), toggle_label)
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, _w, _cx| {
                        this.store.dispatch(Msg::SetRepoHookEnabled {
                            repo_id,
                            name: toggle_name.clone(),
                            enabled,
                        });
                    })
                    .debug_selector(move || format!("repo_hooks_toggle_{ix}")),
            );

            // Editing needs the hook's absolute path; without the spec there is
            // nothing to open, so the button is simply not offered.
            if let Some(ref workdir) = workdir {
                let path = worktree_core::hooks::hook_path(workdir, &hook_name);
                actions = actions.child(
                    components::Button::new(
                        format!("repo_hooks_edit_{ix}"),
                        crate::i18n::tr("prompts.repo_hooks.edit"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, _w, _cx| {
                        this.store.dispatch(Msg::OpenFileEditor {
                            repo_id,
                            path: path.clone(),
                        });
                    })
                    .debug_selector(move || format!("repo_hooks_edit_{ix}")),
                );
            }

            let delete_name = hook_name.clone();
            actions = actions.child(
                components::Button::new(
                    format!("repo_hooks_delete_{ix}"),
                    crate::i18n::tr("prompts.repo_hooks.delete"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _w, _cx| {
                    this.store.dispatch(Msg::DeleteRepoHook {
                        repo_id,
                        name: delete_name.clone(),
                    });
                })
                .debug_selector(move || format!("repo_hooks_delete_{ix}")),
            );
        }

        row_list = row_list.child(
            div()
                .id(("repo_hooks_row", ix))
                .debug_selector(move || format!("repo_hooks_row_{ix}"))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_sm()
                                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                                .text_color(theme.colors.foreground.primary)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(state_label),
                        ),
                )
                .child(actions),
        );
    }
    row_list
}
