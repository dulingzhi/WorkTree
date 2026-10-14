//! 07-4: the cross-session aggregated agent-change view. Lists every agent
//! session's changed paths in one panel, with per-path reject and per-session
//! / global batch accept (snapshot) and reject (restore from baseline).

use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);

    let aggregate = this
        .root_view
        .upgrade()
        .and_then(|root| root.read(cx).agent_aggregate.get(&repo_id).cloned());

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    match aggregate {
        None => {
            body = body.child(placeholder_row(
                &theme,
                crate::i18n::tr("chrome.agent.aggregate_empty"),
            ))
        }
        Some(state) if state.loading => {
            body = body.child(placeholder_row(
                &theme,
                crate::i18n::tr("chrome.agent.aggregate_scanning"),
            ))
        }
        Some(state)
            if state
                .sessions
                .iter()
                .all(|s| s.paths.is_empty() && s.error.is_none()) =>
        {
            body = body.child(placeholder_row(
                &theme,
                crate::i18n::tr("chrome.agent.aggregate_empty"),
            ))
        }
        Some(state) => {
            for session in &state.sessions {
                if session.paths.is_empty() && session.error.is_none() {
                    continue;
                }
                let baseline = session.baseline.0.as_ref();
                let short = &baseline[..baseline.len().min(8)];
                let mut card = div()
                    .id(format!("agent_agg_card_{}", session.id.0))
                    .debug_selector(|| format!("agent_agg_card_{}", session.id.0))
                    .flex()
                    .flex_col()
                    .gap(scaled_px(2.0))
                    .p_2()
                    .rounded(px(theme.radii.control))
                    .border_1()
                    .border_color(theme.colors.stroke.subtle)
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.primary)
                            .child(session.kind.display_label()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .line_clamp(1)
                            .child(session.worktree_path.display().to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(format!("baseline {short}")),
                    );

                if let Some(error) = &session.error {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(format!("scan failed: {error}")),
                    );
                }

                let mut paths_div = div()
                    .flex()
                    .flex_col()
                    .gap(scaled_px(2.0))
                    .max_h(scaled_px(180.0));
                for path in &session.paths {
                    let path_str = path.display().to_string();
                    let path_buf = path.clone();
                    let id = session.id;
                    paths_div = paths_div.child(
                        div()
                            .id(format!("agent_agg_path_{}_{}", id.0, path_str))
                            .debug_selector(|| format!("agent_agg_path_{}_{}", id.0, path_str))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(scaled_px(6.0))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.foreground.secondary)
                                    .line_clamp(1)
                                    .child(path_str.clone()),
                            )
                            .child(
                                div()
                                    .debug_selector(|| {
                                        format!("agent_agg_reject_{}_{}", id.0, path_str)
                                    })
                                    .child(
                                        components::Button::new(
                                            format!("agent_agg_reject_{}_{}", id.0, path_str),
                                            crate::i18n::tr("chrome.agent.aggregate_reject"),
                                        )
                                        .style(components::ButtonStyle::Outlined)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, _e, _w, cx| {
                                                let _ = this.root_view.update(cx, |root, cx| {
                                                    root.reject_agent_path(
                                                        repo_id,
                                                        id,
                                                        path_buf.clone(),
                                                        cx,
                                                    );
                                                });
                                            },
                                        ),
                                    ),
                            ),
                    );
                }
                card = card.child(paths_div);

                let id = session.id;
                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(scaled_px(6.0))
                        .child(
                            div()
                                .debug_selector(|| format!("agent_agg_view_{}", id.0))
                                .child(
                                    components::Button::new(
                                        format!("agent_agg_view_{}", id.0),
                                        crate::i18n::tr("chrome.agent.view_changes"),
                                    )
                                    .style(components::ButtonStyle::Filled)
                                    .on_click(
                                        theme,
                                        cx,
                                        move |this, _e, _w, cx| {
                                            let _ = this.root_view.update(cx, |root, cx| {
                                                root.view_agent_changes_for_session(
                                                    repo_id, id, cx,
                                                );
                                            });
                                            this.close_popover(cx);
                                        },
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| format!("agent_agg_reject_session_{}", id.0))
                                .child(
                                    components::Button::new(
                                        format!("agent_agg_reject_session_{}", id.0),
                                        crate::i18n::tr("chrome.agent.aggregate_reject_session"),
                                    )
                                    .style(components::ButtonStyle::Outlined)
                                    .on_click(
                                        theme,
                                        cx,
                                        move |this, _e, _w, cx| {
                                            let _ = this.root_view.update(cx, |root, cx| {
                                                root.reject_agent_session_all(repo_id, id, cx);
                                            });
                                        },
                                    ),
                                ),
                        ),
                );

                body = body.child(card);
            }
        }
    }

    let global = div()
        .flex()
        .items_center()
        .gap(scaled_px(6.0))
        .child(
            div()
                .debug_selector(|| "agent_agg_accept_all".to_string())
                .child(
                    components::Button::new(
                        "agent_agg_accept_all",
                        crate::i18n::tr("chrome.agent.aggregate_accept_all"),
                    )
                    .style(components::ButtonStyle::Filled)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        let _ = this.root_view.update(cx, |root, cx| {
                            root.accept_agent_all_sessions(repo_id, cx);
                        });
                    }),
                ),
        )
        .child(
            div()
                .debug_selector(|| "agent_agg_reject_all".to_string())
                .child(
                    components::Button::new(
                        "agent_agg_reject_all",
                        crate::i18n::tr("chrome.agent.aggregate_reject_all"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, _w, cx| {
                        let _ = this.root_view.update(cx, |root, cx| {
                            root.reject_agent_all_sessions(repo_id, cx);
                        });
                    }),
                ),
        )
        .child(
            div()
                .debug_selector(|| "agent_agg_pr_description".to_string())
                .child(
                    components::Button::new(
                        "agent_agg_pr_description",
                        crate::i18n::tr("chrome.agent.pr_description"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, move |this, _e, window, cx| {
                        // 07-5: route every session's changes into the PR/MR
                        // description generator via the push prompt.
                        if let Some(anchor) = this.popover_anchor.clone() {
                            this.open_popover(
                                PopoverKind::MergeRequestPushPrompt { repo_id },
                                anchor,
                                window,
                                cx,
                            );
                        }
                        this.start_agent_description_generation(
                            repo_id,
                            super::submit::AgentDescriptionScope::All,
                            cx,
                        );
                    }),
                ),
        );

    components::context_menu(
        theme,
        div()
            .id("agent_aggregate_popover")
            .debug_selector(|| "agent_aggregate_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(460.0))
            .child(popover_title(crate::i18n::tr(
                "chrome.agent.aggregate_title",
            )))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(global)
            .child(body),
    )
}

/// Empty / scanning placeholder line for the aggregate panel.
fn placeholder_row(theme: &AppTheme, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(label.into())
}
