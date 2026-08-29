use super::*;

/// Height the path list caps itself at, matching the stash picker's list: a
/// handful of flagged paths reads at a glance, and dozens stay scrollable
/// without pushing the dialog off screen.
const ASSUME_UNCHANGED_LIST_MAX_HEIGHT_PX: f32 = 240.0;

/// Lists the paths marked assume-unchanged in the index, each with a row
/// action that clears the flag. The list is requested on open (see
/// `request_lazy_popover_repo_data`) and reloaded after every toggle, so the
/// dialog stays open and rows simply leave as they are restored.
pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let ui_scale_percent = super::popover_ui_scale_percent(cx);
    let scaled_px = |value: f32| super::popover_scaled_px_from_percent(value, ui_scale_percent);
    // Only the load-state arms below read it; the rows read the same Arc.
    let repo = this.state.repos.iter().find(|r| r.id == repo_id);

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
                .child(crate::i18n::tr("input.assume_unchanged.title")),
        )
        .child(
            components::Button::new(
                "assume_unchanged_close",
                crate::i18n::tr("input.assume_unchanged.close"),
            )
            .style(components::ButtonStyle::Outlined)
            .on_click(theme, cx, |this, _e, _w, cx| this.close_popover(cx)),
        );

    let body: AnyElement = match repo.map(|r| &r.assume_unchanged) {
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
            crate::i18n::tr("ui.common.loading_ellipsis"),
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
        Some(Loadable::Ready(paths)) if paths.is_empty() => components::context_menu_label(
            theme,
            ui_scale_percent,
            crate::i18n::tr("input.assume_unchanged.empty"),
            Some(this.tooltip_host.clone()),
            cx,
        )
        .into_any_element(),
        Some(Loadable::Ready(paths)) => {
            let paths = paths.clone();
            div()
                .id("assume_unchanged_rows")
                .debug_selector(|| "assume_unchanged_rows".to_string())
                .max_h(scaled_px(ASSUME_UNCHANGED_LIST_MAX_HEIGHT_PX))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(rows(theme, repo_id, &paths, this, cx))
                .into_any_element()
        }
    };

    components::context_menu(
        theme,
        div()
            .id("assume_unchanged_manager")
            .debug_selector(|| "assume_unchanged_manager".to_string())
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
                    .child(crate::i18n::tr("input.assume_unchanged.hint")),
            )
            .child(body),
    )
}

/// One row per flagged path: the path on the left (monospace, truncated with a
/// full-text tooltip — these are the same paths the status list shows), the
/// restore action on the right.
fn rows(
    theme: AppTheme,
    repo_id: RepoId,
    paths: &[std::path::PathBuf],
    this: &PopoverHost,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let mut list = div().flex().flex_col();
    for (ix, path) in paths.iter().enumerate() {
        let row_path = path.clone();
        list = list.child(
            div()
                .id(("assume_unchanged_row", ix))
                .debug_selector(move || format!("assume_unchanged_row_{ix}"))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .text_sm()
                .font_family(crate::font_preferences::EDITOR_MONOSPACE_FONT_FAMILY)
                .text_color(theme.colors.foreground.secondary)
                .child(
                    components::TruncatedText::path(path.display().to_string())
                        .id(("assume_unchanged_row_path", ix))
                        .full_text_tooltip(this.tooltip_host.clone())
                        .render(cx),
                )
                .child(
                    components::Button::new(
                        format!("assume_unchanged_remove_{ix}"),
                        crate::i18n::tr("input.assume_unchanged.remove_flag"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, _w, _cx| {
                        // Deliberately keeps the dialog open: the finished
                        // action reloads the list, and this row leaves it.
                        this.store.dispatch(Msg::SetAssumeUnchanged {
                            repo_id,
                            path: row_path.clone(),
                            enable: false,
                        });
                    }),
                ),
        );
    }
    list
}
