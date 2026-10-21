//! Iteration 09-A Slice 2/3: per-repository extension manager. Lists the active
//! (bundled ∪ global ∪ repo-local) extension commands and hooks, tagging each
//! with its source (`Built-in` / `Global` / `Repo`) and offering runtime
//! enable/disable toggles.
//!
//! Toggling writes only to [`WorkTreeView::disabled_extension_ids`]; it never
//! edits a toml file, so a disabled extension drops out of the command palette,
//! stops firing as a lifecycle hook, and cannot be executed until re-enabled.

use super::*;
use worktree_core::extensions::{
    ExtensionCommand, ExtensionHook, ExtensionSource, merge_extensions_with_source,
};

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);

    let workdir = this
        .state
        .repos
        .iter()
        .find(|repo| repo.id == repo_id)
        .map(|repo| repo.spec.workdir.clone());
    let Some(workdir) = workdir else {
        return components::context_menu(
            theme,
            div().w(scaled_px(440.0)).child(
                div()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr_str("ui.common.no_repository")),
            ),
        );
    };

    // The manager resolves the merged set itself (bundled builtin + global
    // app-data config + the repository-local overlay) so it never depends on
    // view construction order. Precedence repo > global > builtin.
    let builtin_ext = worktree_core::extensions::builtin_extensions();
    let global_ext = session::app_data_dir()
        .map(|dir| worktree_core::extensions::load_extensions(&dir.join("extensions.toml")))
        .unwrap_or_default();
    let repo_ext = worktree_core::extensions::load_extensions(&workdir.join("extensions.toml"));
    let (commands, hooks) = merge_extensions_with_source(&builtin_ext, &global_ext, &repo_ext);

    let disabled = this
        .root_view
        .upgrade()
        .map(|root| root.read(cx).disabled_extension_ids.clone())
        .unwrap_or_default();

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    body = body.child(section_title(
        theme,
        crate::i18n::tr_str("input.extensions_manager.commands_title"),
        &scaled_px,
    ));
    if commands.is_empty() {
        body = body.child(empty_hint(
            theme,
            crate::i18n::tr_str("input.extensions_manager.no_commands"),
            &scaled_px,
        ));
    } else {
        for annotated in &commands {
            let id = annotated.command.id.clone();
            let enabled = !disabled.contains(&id);
            let source = source_label(annotated.source);
            body = body.child(command_row(
                theme,
                cx,
                &annotated.command,
                source,
                enabled,
                id,
                &scaled_px,
            ));
        }
    }

    body = body.child(section_title(
        theme,
        crate::i18n::tr_str("input.extensions_manager.hooks_title"),
        &scaled_px,
    ));
    if hooks.is_empty() {
        body = body.child(empty_hint(
            theme,
            crate::i18n::tr_str("input.extensions_manager.no_hooks"),
            &scaled_px,
        ));
    } else {
        for annotated in &hooks {
            let id = annotated.hook.id.clone();
            let enabled = !disabled.contains(&id);
            let source = source_label(annotated.source);
            body = body.child(hook_row(
                theme,
                cx,
                &annotated.hook,
                source,
                id,
                enabled,
                &scaled_px,
            ));
        }
    }

    components::context_menu(
        theme,
        div()
            .id("extensions_manager_popover")
            .debug_selector(|| "extensions_manager_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(440.0))
            .child(popover_title(crate::i18n::tr(
                "input.extensions_manager.title",
            )))
            .child(dialog_divider(theme))
            .child(body)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(dialog_cancel_button(
                        "extensions_manager_done",
                        "extensions_manager_done_hint",
                        theme,
                        cx,
                    )),
            ),
    )
}

fn source_label(source: ExtensionSource) -> &'static str {
    match source {
        ExtensionSource::Builtin => crate::i18n::tr_str("input.extensions_manager.source_builtin"),
        ExtensionSource::Global => crate::i18n::tr_str("input.extensions_manager.source_global"),
        ExtensionSource::Repo => crate::i18n::tr_str("input.extensions_manager.source_repo"),
    }
}

fn section_title(
    theme: AppTheme,
    label: &'static str,
    scaled_px: &dyn Fn(f32) -> Pixels,
) -> gpui::Div {
    div()
        .px_1()
        .pt(scaled_px(4.0))
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(label)
}

fn empty_hint(
    theme: AppTheme,
    label: &'static str,
    scaled_px: &dyn Fn(f32) -> Pixels,
) -> gpui::Div {
    div()
        .px_1()
        .py(scaled_px(2.0))
        .text_sm()
        .text_color(theme.colors.foreground.secondary)
        .child(label)
}

fn command_row(
    theme: AppTheme,
    cx: &mut gpui::Context<PopoverHost>,
    command: &ExtensionCommand,
    source: &'static str,
    enabled: bool,
    id: String,
    scaled_px: &dyn Fn(f32) -> Pixels,
) -> gpui::Stateful<gpui::Div> {
    let title: String = if command.label.is_empty() {
        command.id.clone()
    } else {
        command.label.clone()
    };
    let toggle_label: &'static str = if enabled {
        crate::i18n::tr_str("input.extensions_manager.enabled")
    } else {
        crate::i18n::tr_str("input.extensions_manager.disabled")
    };
    let id_for_click = id.clone();
    let enabled_for_click = enabled;
    let debug_label = format!("ext_row_{}", command.id);
    div()
        .id(SharedString::from(debug_label.clone()))
        .debug_selector(move || debug_label.clone())
        .flex()
        .flex_col()
        .gap(scaled_px(2.0))
        .px_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap(scaled_px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_sm()
                        .text_color(theme.colors.foreground.primary)
                        .line_clamp(1)
                        .child(title),
                )
                .child(source_badge(theme, source, scaled_px))
                .child(
                    components::Button::new(
                        SharedString::from(format!("ext_toggle_{}", command.id)),
                        toggle_label,
                    )
                    .style(if enabled {
                        components::ButtonStyle::Outlined
                    } else {
                        components::ButtonStyle::Subtle
                    })
                    .on_click(
                        theme,
                        cx,
                        move |this, _e: &ClickEvent, _w, cx| {
                            let root_view = this.root_view.clone();
                            let ext_id = id_for_click.clone();
                            let turn_on = !enabled_for_click;
                            let _ = root_view.update(cx, |root, cx| {
                                root.set_extension_enabled(ext_id, turn_on, cx);
                            });
                            cx.notify();
                        },
                    ),
                ),
        )
        .when(!command.command.is_empty(), |row| {
            row.child(
                div()
                    .pl(scaled_px(2.0))
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .line_clamp(1)
                    .child(command.command.clone()),
            )
        })
}

fn hook_row(
    theme: AppTheme,
    cx: &mut gpui::Context<PopoverHost>,
    hook: &ExtensionHook,
    source: &'static str,
    id: String,
    enabled: bool,
    scaled_px: &dyn Fn(f32) -> Pixels,
) -> gpui::Stateful<gpui::Div> {
    let event_label = match hook.event {
        worktree_core::extensions::ExtensionHookEvent::RepositoryOpened => {
            crate::i18n::tr_str("input.extensions_manager.event_repository_opened")
        }
        worktree_core::extensions::ExtensionHookEvent::BeforeCommit => {
            crate::i18n::tr_str("input.extensions_manager.event_before_commit")
        }
        worktree_core::extensions::ExtensionHookEvent::AfterCommit => {
            crate::i18n::tr_str("input.extensions_manager.event_after_commit")
        }
    };
    let debug_label = format!("hook_row_{}", if id.is_empty() { event_label } else { &id });
    div()
        .id(SharedString::from(debug_label.clone()))
        .debug_selector(move || debug_label.clone())
        .flex()
        .flex_col()
        .gap(scaled_px(2.0))
        .px_1()
        .child(
            div()
                .flex()
                .items_center()
                .gap(scaled_px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_sm()
                        .text_color(theme.colors.foreground.primary)
                        .line_clamp(1)
                        .child(event_label),
                )
                .child(source_badge(theme, source, scaled_px))
                .child(
                    // Hooks without an id can never be toggled off, so they are
                    // always on; only id-bearing hooks get a working toggle.
                    if id.is_empty() {
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr_str("input.extensions_manager.always_on"))
                            .into_any_element()
                    } else {
                        let toggle_label: &'static str = if enabled {
                            crate::i18n::tr_str("input.extensions_manager.enabled")
                        } else {
                            crate::i18n::tr_str("input.extensions_manager.disabled")
                        };
                        let id_for_click = id.clone();
                        let enabled_for_click = enabled;
                        components::Button::new(
                            SharedString::from(format!("hook_toggle_{}", id)),
                            toggle_label,
                        )
                        .style(if enabled {
                            components::ButtonStyle::Outlined
                        } else {
                            components::ButtonStyle::Subtle
                        })
                        .on_click(theme, cx, move |this, _e: &ClickEvent, _w, cx| {
                            let root_view = this.root_view.clone();
                            let ext_id = id_for_click.clone();
                            let turn_on = !enabled_for_click;
                            let _ = root_view.update(cx, |root, cx| {
                                root.set_extension_enabled(ext_id, turn_on, cx);
                            });
                            cx.notify();
                        })
                        .into_any_element()
                    },
                ),
        )
        .when(!hook.command.is_empty(), |row| {
            row.child(
                div()
                    .pl(scaled_px(2.0))
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .line_clamp(1)
                    .child(hook.command.clone()),
            )
        })
}

fn source_badge(
    theme: AppTheme,
    source: &'static str,
    scaled_px: &dyn Fn(f32) -> Pixels,
) -> gpui::Div {
    div()
        .px(scaled_px(4.0))
        .py(scaled_px(1.0))
        .rounded(scaled_px(3.0))
        .bg(theme.colors.interaction.hover_background)
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .line_clamp(1)
        .child(source)
}
