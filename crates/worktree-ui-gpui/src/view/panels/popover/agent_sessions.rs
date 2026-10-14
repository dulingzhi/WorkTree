//! The agent workbench's roster: one card per running session plus a
//! start entry per agent. Pure launcher — every action closes the popover,
//! so the content is computed fresh from the root view's session state on
//! each open and never needs a repo rehash to stay current.

use super::*;

pub(super) fn panel(
    this: &mut PopoverHost,
    repo_id: RepoId,
    cx: &mut gpui::Context<PopoverHost>,
) -> gpui::Div {
    let theme = this.theme;
    let scaled_px = super::popover_scaled_px_fn(cx);

    // 07-1: a repo may host several sessions; list each on its own card.
    let sessions = this.root_view.upgrade().map(|root| {
        let root_view = root.read(cx);
        root_view
            .agent_sessions
            .get(&repo_id)
            .map(|sessions| {
                sessions
                    .iter()
                    .map(|(id, session)| {
                        let baseline = session.baseline.0.as_ref();
                        let short = &baseline[..baseline.len().min(8)];
                        let live = root_view
                            .terminal_sessions
                            .get(&repo_id)
                            .map(|s| {
                                s.instances
                                    .iter()
                                    .any(|instance| instance.session_seq == id.0)
                            })
                            .unwrap_or(false);
                        (
                            *id,
                            session.kind,
                            session.worktree_path.display().to_string(),
                            short.to_string(),
                            live,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    match sessions {
        Some(sessions) if !sessions.is_empty() => {
            for (id, kind, worktree, baseline_short, live) in sessions {
                body = body
                    .child(
                        div()
                            .id(format!("agent_session_card_{}", id.0))
                            .debug_selector(|| format!("agent_session_card_{}", id.0))
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
                                    .child(kind.display_label()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.foreground.secondary)
                                    .line_clamp(1)
                                    .child(worktree),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.foreground.secondary)
                                    .child(format!("baseline {baseline_short}")),
                            ),
                    )
                    .child({
                        let mut actions = div()
                            .flex()
                            .items_center()
                            .gap(scaled_px(6.0))
                            .child(
                                div()
                                    .debug_selector(|| format!("agent_view_changes_{}", id.0))
                                    .child(
                                        components::Button::new(
                                            format!("agent_view_changes_{}", id.0),
                                            crate::i18n::tr("chrome.agent.view_changes"),
                                        )
                                        .style(components::ButtonStyle::Filled)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, _e, _window, cx| {
                                                let _ = this.root_view.update(cx, |root, cx| {
                                                    root.view_agent_changes_for_session(
                                                        repo_id, id, cx,
                                                    )
                                                });
                                                this.close_popover(cx);
                                            },
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .debug_selector(|| format!("agent_stop_session_{}", id.0))
                                    .child(
                                        components::Button::new(
                                            format!("agent_stop_session_{}", id.0),
                                            crate::i18n::tr("chrome.agent.stop_session"),
                                        )
                                        .style(components::ButtonStyle::Outlined)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, _e, _window, cx| {
                                                let _ = this.root_view.update(cx, |root, cx| {
                                                    root.close_agent_session(repo_id, id, cx)
                                                });
                                                this.close_popover(cx);
                                            },
                                        ),
                                    ),
                            );
                        // A rehydrated session has no live terminal until it is
                        // resumed, so only offer "Resume" then.
                        if !live {
                            actions = actions.child(
                                div()
                                    .debug_selector(|| format!("agent_resume_session_{}", id.0))
                                    .child(
                                        components::Button::new(
                                            format!("agent_resume_session_{}", id.0),
                                            crate::i18n::tr("chrome.agent.resume"),
                                        )
                                        .style(components::ButtonStyle::Filled)
                                        .on_click(
                                            theme,
                                            cx,
                                            move |this, _e, window, cx| {
                                                let _ = this.root_view.update(cx, |root, cx| {
                                                    root.resume_agent_session(
                                                        repo_id, id, window, cx,
                                                    )
                                                });
                                                this.close_popover(cx);
                                            },
                                        ),
                                    ),
                            );
                        }
                        actions
                    });
            }
        }
        _ => {
            body = body.child(
                div()
                    .id("agent_session_empty")
                    .debug_selector(|| "agent_session_empty".to_string())
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr("chrome.agent.roster_empty")),
            );
        }
    }

    // Start entries: one per agent, disabled when its executable is not on
    // PATH. Starting adds a fresh worktree + session alongside any existing
    // ones (it no longer replaces the running session).
    let search_paths = crate::view::agent_workbench::system_search_paths();
    for kind in crate::view::agent_workbench::AgentKind::all() {
        let available = crate::view::agent_workbench::find_executable_in_paths(
            kind.executable(),
            &search_paths,
        )
        .is_some();
        let selector: &'static str = match kind {
            crate::view::agent_workbench::AgentKind::ClaudeCode => "agent_start_claude",
            crate::view::agent_workbench::AgentKind::Codex => "agent_start_codex",
        };
        body = body.child(
            div().debug_selector(move || selector.to_string()).child(
                components::Button::new(
                    selector,
                    crate::i18n::t!("chrome.agent.start", agent = kind.display_label()),
                )
                .start_slot(crate::view::icons::svg_icon(
                    "icons/sparkle.svg",
                    theme.colors.accent.foreground,
                    px(13.0),
                ))
                .disabled(!available)
                .on_click(theme, cx, move |this, _e, window, cx| {
                    let _ = this
                        .root_view
                        .update(cx, |root, cx| root.start_agent_session(kind, window, cx));
                    this.close_popover(cx);
                }),
            ),
        );
    }

    components::context_menu(
        theme,
        div()
            .id("agent_sessions_popover")
            .debug_selector(|| "agent_sessions_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(440.0))
            .child(popover_title(crate::i18n::tr("chrome.agent.roster_title")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body),
    )
}
