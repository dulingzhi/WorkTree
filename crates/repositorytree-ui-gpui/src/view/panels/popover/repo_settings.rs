//! The per-repository settings prompt: local git-config overrides for the
//! identity and commit signing, mirroring the C# client's `Config(repo)`
//! semantics — empty means "inherit the global value" (the local override is
//! unset), never an empty string in the config.

use super::*;

/// One field's draft state: what the input holds, and the placeholder that
/// says what inheriting would mean.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RepoSettingsDraft {
    pub user_name: String,
    pub user_email: String,
    /// Tri-state signing: `None` inherits the global setting.
    pub sign_commits: Option<bool>,
}

/// The write plan for one Apply: the keys whose effective value differs from
/// their draft, mapped to `Some(value)` to set or `None` to unset (return
/// the key to inheriting). Pure so the diff-based "only write what changed"
/// contract is unit-testable without a repository.
pub(super) fn repo_settings_apply_plan(
    draft: &RepoSettingsDraft,
    current: &RepoSettingsCurrent,
) -> Vec<(&'static str, Option<String>)> {
    let mut plan = Vec::new();
    // An empty draft unsets the local override (inherit); a filled one equal
    // to the current override is already in place — nothing to write.
    let field = |draft_value: &str, current: &Option<String>, key: &'static str, plan: &mut Vec<(&'static str, Option<String>)>| {
        let next = (!draft_value.trim().is_empty()).then(|| draft_value.trim().to_string());
        if next != *current {
            plan.push((key, next));
        }
    };
    field(
        &draft.user_name,
        &current.user_name,
        "user.name",
        &mut plan,
    );
    field(
        &draft.user_email,
        &current.user_email,
        "user.email",
        &mut plan,
    );
    if draft.sign_commits != current.sign_commits {
        plan.push(("commit.gpgsign", draft.sign_commits.map(|v| v.to_string())));
    }
    plan
}

/// The local overrides the popover opened against, plus the global values
/// the placeholders quote.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct RepoSettingsCurrent {
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub sign_commits: Option<bool>,
    pub global_user_name: Option<String>,
    pub global_user_email: Option<String>,
}

impl RepoSettingsCurrent {
    /// Read the live values for one workdir. The locals decide what the
    /// inputs start from; the globals decide what an empty input means.
    pub(super) fn load(workdir: &std::path::Path) -> Self {
        let read_bool = |key: &str| {
            repositorytree_core::process::git_config_local_get(workdir, key)
                .map(|value| value == "true")
        };
        Self {
            user_name: repositorytree_core::process::git_config_local_get(workdir, "user.name"),
            user_email: repositorytree_core::process::git_config_local_get(workdir, "user.email"),
            sign_commits: read_bool("commit.gpgsign"),
            global_user_name: repositorytree_core::process::git_config_global_get("user.name"),
            global_user_email: repositorytree_core::process::git_config_global_get("user.email"),
        }
    }
}

/// Run the plan against the repository's local config. The bool field needs
/// the tri-state shape `--unset` provides, so it routes through the same
/// Option writer as the text fields.
pub(super) fn apply_repo_settings(
    workdir: &std::path::Path,
    plan: &[(&'static str, Option<String>)],
) -> Result<(), String> {
    for (key, value) in plan {
        repositorytree_core::process::git_config_local_set(workdir, key, value.as_deref())
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

impl PopoverHost {
    /// Enter on either input applies, resolving the repo from the open kind.
    pub(super) fn submit_repo_settings_open(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let Some(PopoverKind::RepoSettingsPrompt { repo_id }) = self.popover else {
            return;
        };
        let Some(workdir) = self
            .state
            .repos
            .iter()
            .find(|repo| repo.id == repo_id)
            .map(|repo| repo.spec.workdir.clone())
        else {
            return;
        };
        self.submit_repo_settings(repo_id, workdir, cx);
        let _ = window;
    }

    /// Apply the drafts: build the write plan against the snapshot the
    /// popover opened with (re-read if the panel already consumed it), run
    /// it, and close on success — failures stay in the prompt as an error
    /// row so the drafts survive for a retry.
    pub(super) fn submit_repo_settings(
        &mut self,
        repo_id: RepoId,
        workdir: std::path::PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        let draft = RepoSettingsDraft {
            user_name: self
                .repo_settings_user_input
                .read_with(cx, |input, _| input.text().to_string()),
            user_email: self
                .repo_settings_email_input
                .read_with(cx, |input, _| input.text().to_string()),
            sign_commits: self.repo_settings_sign_commits,
        };
        // The panel takes the open snapshot; applying re-reads the live
        // locals so a write plan never goes stale against an outside edit.
        let current = self
            .repo_settings_current
            .take()
            .unwrap_or_else(|| RepoSettingsCurrent::load(&workdir));
        let plan = repo_settings_apply_plan(&draft, &current);
        if plan.is_empty() {
            self.dismiss_prompt_popover_window(cx);
            return;
        }
        match apply_repo_settings(&workdir, &plan) {
            Ok(()) => {
                self.repo_settings_error = None;
                self.dismiss_prompt_popover_window(cx);
            }
            Err(message) => {
                self.repo_settings_error = Some(message.into());
                cx.notify();
            }
        }
        let _ = repo_id;
    }

    fn dismiss_prompt_popover_window(&mut self, cx: &mut gpui::Context<Self>) {
        self.close_popover(cx);
    }
}

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
                components::context_menu_label(
                    theme,
                    crate::ui_scale::current(cx).percent,
                    crate::i18n::tr("ui.common.no_repository"),
                    Some(this.tooltip_host.clone()),
                    cx,
                )
                .into_any_element(),
            ),
        );
    };

    // The snapshot is read on open; the inputs own their drafts afterwards.
    let current = this
        .repo_settings_current
        .take()
        .unwrap_or_else(|| RepoSettingsCurrent::load(&workdir));
    let placeholder = |global: &Option<String>, inherit: &str| -> SharedString {
        match global {
            Some(value) => format!("{inherit}: {value}").into(),
            None => inherit.to_string().into(),
        }
    };
    this.repo_settings_user_input.update(cx, |input, cx| {
        input.set_placeholder(
            placeholder(&current.global_user_name, &crate::i18n::tr_str("input.repo_settings.inherit")),
            cx,
        );
    });
    this.repo_settings_email_input.update(cx, |input, cx| {
        input.set_placeholder(
            placeholder(&current.global_user_email, &crate::i18n::tr_str("input.repo_settings.inherit")),
            cx,
        );
    });

    let sign_row = |state: Option<bool>| -> &'static str {
        match state {
            Some(true) => "input.repo_settings.sign_on",
            Some(false) => "input.repo_settings.sign_off",
            None => "input.repo_settings.sign_inherit",
        }
    };

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    body = body
        .child(input_label(theme, crate::i18n::tr_str("input.repo_settings.user_label")))
        .child(
            div()
                .id("repo_settings_user_row")
                .debug_selector(|| "repo_settings_user_input".to_string())
                .w_full()
                .min_w(px(0.0))
                .child(this.repo_settings_user_input.clone()),
        )
        .child(input_label(theme, crate::i18n::tr_str("input.repo_settings.email_label")))
        .child(
            div()
                .id("repo_settings_email_row")
                .debug_selector(|| "repo_settings_email_input".to_string())
                .w_full()
                .min_w(px(0.0))
                .child(this.repo_settings_email_input.clone()),
        );

    // The signing override cycles inherit → on → off, so all three states
    // are reachable without a third widget.
    let sign_state = this.repo_settings_sign_commits;
    body = body.child(
        div()
            .id("repo_settings_sign_row")
            .debug_selector(|| "repo_settings_sign_toggle".to_string())
            .flex()
            .items_center()
            .gap(scaled_px(6.0))
            .px_1()
            .cursor(gpui::CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                this.repo_settings_sign_commits = match this.repo_settings_sign_commits {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) => None,
                };
                cx.notify();
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.foreground.primary)
                    .child(crate::i18n::tr("input.repo_settings.sign_label")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr(sign_row(sign_state))),
            ),
    );

    let error_row = this.repo_settings_error.clone().map(|message| {
        div()
            .id("repo_settings_error")
            .debug_selector(|| "repo_settings_error".to_string())
            .px_1()
            .text_xs()
            .text_color(theme.colors.status.danger.foreground)
            .line_clamp(2)
            .child(message)
    });

    components::context_menu(
        theme,
        div()
            .id("repo_settings_popover")
            .debug_selector(|| "repo_settings_popover".to_string())
            .flex()
            .flex_col()
            .w(scaled_px(440.0))
            .child(popover_title(crate::i18n::tr("input.repo_settings.title")))
            .child(div().border_t_1().border_color(theme.colors.stroke.default))
            .child(body)
            .children(error_row)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        cancel_button("repo_settings_cancel", "repo_settings_cancel_hint", theme)
                            .on_click(theme, cx, |this, _e, window, cx| {
                                this.dismiss_prompt_popover(window, cx);
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "repo_settings_apply".to_string())
                            .child(
                                components::Button::new(
                                    "repo_settings_apply_btn",
                                    crate::i18n::tr("input.repo_settings.apply"),
                                )
                                .style(components::ButtonStyle::Filled)
                                .on_click(theme, cx, move |this, _e, _w, cx| {
                                    this.submit_repo_settings(repo_id, workdir.clone(), cx);
                                }),
                            ),
                    ),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(user: &str, email: &str, sign: Option<bool>) -> RepoSettingsDraft {
        RepoSettingsDraft {
            user_name: user.to_string(),
            user_email: email.to_string(),
            sign_commits: sign,
        }
    }

    #[test]
    fn apply_plan_writes_only_what_changed() {
        let current = RepoSettingsCurrent {
            user_name: Some("Old".to_string()),
            user_email: None,
            sign_commits: None,
            ..Default::default()
        };

        // Nothing differs from the overrides already in place.
        assert!(repo_settings_apply_plan(
            &draft("Old", "", None),
            &current
        )
        .is_empty());

        // A filled field sets, an emptied field unsets (inherits), and the
        // untouched signing override is not written at all.
        let plan = repo_settings_apply_plan(&draft("New", "me@example.com", None), &current);
        assert_eq!(
            plan,
            vec![
                ("user.name", Some("New".to_string())),
                ("user.email", Some("me@example.com".to_string())),
            ]
        );

        // Clearing back to inherit unsets; signing flips write explicit
        // values; inherit-after-off writes an unset.
        let current = RepoSettingsCurrent {
            user_name: Some("New".to_string()),
            user_email: Some("me@example.com".to_string()),
            sign_commits: Some(false),
            ..Default::default()
        };
        let plan = repo_settings_apply_plan(&draft("", "", Some(true)), &current);
        assert_eq!(
            plan,
            vec![
                ("user.name", None),
                ("user.email", None),
                ("commit.gpgsign", Some("true".to_string())),
            ]
        );
        let plan = repo_settings_apply_plan(&draft("", "", None), &current);
        assert_eq!(
            plan,
            vec![
                ("user.name", None),
                ("user.email", None),
                ("commit.gpgsign", None),
            ]
        );
    }

    #[test]
    fn apply_plan_trims_before_comparing() {
        let current = RepoSettingsCurrent::default();
        let plan = repo_settings_apply_plan(&draft("  Name  ", "", None), &current);
        assert_eq!(plan, vec![("user.name", Some("Name".to_string()))]);
    }
}
