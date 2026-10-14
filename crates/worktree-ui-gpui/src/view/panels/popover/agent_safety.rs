//! 07-6: the agent safety review. Lists each agent session's history-rewriting
//! git operations (scanned from its worktree reflog) and offers Stop / Snapshot
//! / Dismiss actions so the user can contain a runaway agent before it does
//! more damage.

use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);

    let review = this
        .root_view
        .upgrade()
        .and_then(|root| root.read(cx).agent_safety_review.get(&repo_id).cloned());

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    match review {
        // Rich case: a finished scan that found at least one session with
        // recorded activity. Render each session's findings and per-session
        // Stop / Snapshot actions.
        Some(state) if !state.loading && !state.sessions.is_empty() => {
            for session in &state.sessions {
                let baseline = session.baseline.0.as_ref();
                let short = &baseline[..baseline.len().min(8)];
                let id = session.id;
                let mut card = div()
                    .id(format!("agent_safety_card_{}", id.0))
                    .debug_selector(|| format!("agent_safety_card_{}", id.0))
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
                            .child(format!("reflog scan failed: {error}")),
                    );
                } else if session.findings.is_empty() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(crate::i18n::tr("chrome.agent.safety_clean")),
                    );
                } else {
                    let mut findings_div = div()
                        .flex()
                        .flex_col()
                        .gap(scaled_px(2.0))
                        .max_h(scaled_px(160.0));
                    for (line, risk) in &session.findings {
                        findings_div = findings_div.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.foreground.primary)
                                        .line_clamp(1)
                                        .child(line.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.foreground.secondary)
                                        .child(risk.to_string()),
                                ),
                        );
                    }
                    card = card.child(findings_div);
                }

                card = card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(scaled_px(6.0))
                        .child(
                            div()
                                .debug_selector(|| format!("agent_safety_stop_{}", id.0))
                                .child(
                                    components::Button::new(
                                        format!("agent_safety_stop_{}", id.0),
                                        crate::i18n::tr("chrome.agent.stop_session"),
                                    )
                                    .style(components::ButtonStyle::Filled)
                                    .on_click(
                                        theme,
                                        cx,
                                        move |this, _e, _window, cx| {
                                            let _ = this.root_view.update(cx, |root, cx| {
                                                root.close_agent_session(repo_id, id, cx);
                                            });
                                            this.close_popover(cx);
                                        },
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .debug_selector(|| format!("agent_safety_snapshot_{}", id.0))
                                .child(
                                    components::Button::new(
                                        format!("agent_safety_snapshot_{}", id.0),
                                        crate::i18n::tr("chrome.agent.snapshot_now"),
                                    )
                                    .style(components::ButtonStyle::Outlined)
                                    .on_click(
                                        theme,
                                        cx,
                                        move |this, _e, _window, cx| {
                                            let _ = this.root_view.update(cx, |root, cx| {
                                                root.snapshot_agent_session_now(repo_id, id, cx);
                                            });
                                            this.close_popover(cx);
                                        },
                                    ),
                                ),
                        ),
                );

                body = body.child(card);
            }
        }
        // Loaded but empty (no sessions), or still scanning (the load sets
        // `loading` synchronously, so this is the brief pre-land state).
        Some(state) => {
            let label = if state.loading {
                crate::i18n::tr("chrome.agent.safety_scanning")
            } else {
                crate::i18n::tr("chrome.agent.no_session")
            };
            body = body.child(placeholder_row(&theme, label));
        }
        None => {
            body = body.child(placeholder_row(
                &theme,
                crate::i18n::tr("chrome.agent.safety_scanning"),
            ));
        }
    }

    // Footer: a single dismiss action. Per-session Stop already closes the
    // popover; this is the no-op-besides-closing escape hatch.
    let footer = div().flex().items_center().gap(scaled_px(6.0)).child(
        div()
            .debug_selector(|| "agent_safety_dismiss".to_string())
            .child(
                components::Button::new(
                    "agent_safety_dismiss",
                    crate::i18n::tr("chrome.agent.safety_dismiss"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, move |this, _e, _window, cx| {
                    this.close_popover(cx);
                }),
            ),
    );

    components::context_menu(
        theme,
        div()
            .id("agent_safety_popover")
            .debug_selector(|| "agent_safety_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(460.0))
            .child(popover_title(crate::i18n::tr("chrome.agent.safety_title")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body)
            .child(footer),
    )
}

/// Empty / scanning placeholder line for the safety panel.
fn placeholder_row(theme: &AppTheme, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .text_xs()
        .text_color(theme.colors.foreground.secondary)
        .child(label.into())
}
