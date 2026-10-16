//! The per-repository settings prompt: local git-config overrides for the
//! identity and commit signing, mirroring the C# client's `Config(repo)`
//! semantics — empty means "inherit the global value" (the local override is
//! unset), never an empty string in the config.

use super::*;
use worktree_state::identity_profiles::IdentityProfile;

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
    let field = |draft_value: &str,
                 current: &Option<String>,
                 key: &'static str,
                 plan: &mut Vec<(&'static str, Option<String>)>| {
        let next = (!draft_value.trim().is_empty()).then(|| draft_value.trim().to_string());
        if next != *current {
            plan.push((key, next));
        }
    };
    field(&draft.user_name, &current.user_name, "user.name", &mut plan);
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
            worktree_core::process::git_config_local_get(workdir, key).map(|value| value == "true")
        };
        Self {
            user_name: worktree_core::process::git_config_local_get(workdir, "user.name"),
            user_email: worktree_core::process::git_config_local_get(workdir, "user.email"),
            sign_commits: read_bool("commit.gpgsign"),
            global_user_name: worktree_core::process::git_config_global_get("user.name"),
            global_user_email: worktree_core::process::git_config_global_get("user.email"),
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
        worktree_core::process::git_config_local_set(workdir, key, value.as_deref())
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

impl PopoverHost {
    /// Read the live locals and globals for the repo-settings prompt. One
    /// central seam so tests can count reads: an open reads once and an
    /// apply re-reads once, but a render must never read at all.
    pub(super) fn load_repo_settings_current(
        &mut self,
        workdir: &std::path::Path,
    ) -> RepoSettingsCurrent {
        #[cfg(test)]
        {
            self.repo_settings.repo_settings_test_loads += 1;
        }
        RepoSettingsCurrent::load(workdir)
    }

    /// How many times the config snapshot was read this session, so a
    /// regression that re-reads per render fails a test instead of shipping
    /// as input lag.
    #[cfg(test)]
    pub(in super::super) fn repo_settings_test_loads_for_tests(&self) -> usize {
        self.repo_settings.repo_settings_test_loads
    }

    /// Enter on either input applies, resolving the repo from the open kind.
    pub(super) fn submit_repo_settings_open(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
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
                .repo_settings
                .repo_settings_user_input
                .read_with(cx, |input, _| input.text().to_string()),
            user_email: self
                .repo_settings
                .repo_settings_email_input
                .read_with(cx, |input, _| input.text().to_string()),
            sign_commits: self.repo_settings.repo_settings_sign_commits,
        };
        // The panel takes the open snapshot; applying re-reads the live
        // locals so a write plan never goes stale against an outside edit.
        let current = self
            .repo_settings
            .repo_settings_current
            .take()
            .unwrap_or_else(|| self.load_repo_settings_current(&workdir));
        let plan = repo_settings_apply_plan(&draft, &current);
        // The fetch-prune preference is not a git-config key, so it rides
        // outside the write plan: dispatch it, and the reducer persists to
        // the session file and no-ops when the value is unchanged.
        let prune = self.repo_settings.repo_settings_fetch_prune;
        if self
            .state
            .repos
            .iter()
            .find(|repo| repo.id == repo_id)
            .is_some_and(|repo| repo.fetch_prune_deleted_remote_tracking_branches != prune)
        {
            self.store
                .dispatch(Msg::SetFetchPruneDeletedRemoteTrackingBranches {
                    repo_id,
                    enabled: prune,
                });
        }
        if plan.is_empty() {
            self.dismiss_prompt_popover_window(cx);
            return;
        }
        match apply_repo_settings(&workdir, &plan) {
            Ok(()) => {
                self.repo_settings.repo_settings_error = None;
                self.dismiss_prompt_popover_window(cx);
            }
            Err(message) => {
                self.repo_settings.repo_settings_error = Some(message.into());
                cx.notify();
            }
        }
        let _ = repo_id;
    }

    fn dismiss_prompt_popover_window(&mut self, cx: &mut gpui::Context<Self>) {
        self.close_popover(cx);
    }

    /// Fill the draft from a saved profile. Only touches the in-memory draft
    /// (inputs + tri-state signing); the existing Apply button is what commits
    /// it to the repo's local git config, keeping one write path.
    pub(super) fn apply_identity_profile_to_draft(
        &mut self,
        profile: &IdentityProfile,
        cx: &mut gpui::Context<Self>,
    ) {
        self.repo_settings
            .repo_settings_user_input
            .update(cx, |input, cx| {
                input.set_text(profile.user_name.clone(), cx);
                cx.notify();
            });
        self.repo_settings
            .repo_settings_email_input
            .update(cx, |input, cx| {
                input.set_text(profile.user_email.clone(), cx);
                cx.notify();
            });
        self.repo_settings.repo_settings_sign_commits = profile.sign_commits;
        cx.notify();
    }

    /// Persist the current draft as a named profile. Empty names are rejected
    /// into the profile-error row rather than writing an unnamed entry; a name
    /// that already exists overwrites that profile (upsert by name).
    pub(super) fn save_identity_profile(&mut self, cx: &mut gpui::Context<Self>) {
        let name = self
            .repo_settings
            .repo_settings_profile_name_input
            .read_with(cx, |input, _| input.text().trim().to_string());
        if name.is_empty() {
            self.repo_settings.repo_settings_profile_error =
                Some(crate::i18n::tr_str("input.repo_settings.profile_name_required").into());
            cx.notify();
            return;
        }
        let profile = IdentityProfile {
            name: name.clone(),
            user_name: self
                .repo_settings
                .repo_settings_user_input
                .read_with(cx, |input, _| input.text().to_string()),
            user_email: self
                .repo_settings
                .repo_settings_email_input
                .read_with(cx, |input, _| input.text().to_string()),
            sign_commits: self.repo_settings.repo_settings_sign_commits,
        };
        let mut profiles = worktree_state::identity_profiles::load_identity_profiles();
        profiles.upsert(profile);
        match worktree_state::identity_profiles::persist_identity_profiles(&profiles) {
            Ok(()) => {
                self.repo_settings.repo_settings_identity_profiles = profiles.profiles;
                self.repo_settings.repo_settings_profile_error = None;
                self.repo_settings
                    .repo_settings_profile_name_input
                    .update(cx, |input, cx| {
                        input.set_text(String::new(), cx);
                        cx.notify();
                    });
            }
            Err(message) => {
                self.repo_settings.repo_settings_profile_error = Some(message.to_string().into());
            }
        }
        cx.notify();
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

    // The snapshot is read on open and consumed by this first render only:
    // the placeholders quote the globals once, the inputs own their drafts
    // afterwards, and a read is five git process spawns — every keystroke
    // re-renders this host, so a re-render must never re-read.
    if let Some(current) = this.repo_settings.repo_settings_current.take() {
        let placeholder = |global: &Option<String>, inherit: &str| -> SharedString {
            match global {
                Some(value) => format!("{inherit}: {value}").into(),
                None => inherit.to_string().into(),
            }
        };
        this.repo_settings
            .repo_settings_user_input
            .update(cx, |input, cx| {
                input.set_placeholder(
                    placeholder(
                        &current.global_user_name,
                        crate::i18n::tr_str("input.repo_settings.inherit"),
                    ),
                    cx,
                );
            });
        this.repo_settings
            .repo_settings_email_input
            .update(cx, |input, cx| {
                input.set_placeholder(
                    placeholder(
                        &current.global_user_email,
                        crate::i18n::tr_str("input.repo_settings.inherit"),
                    ),
                    cx,
                );
            });
    }

    let sign_row = |state: Option<bool>| -> &'static str {
        match state {
            Some(true) => "input.repo_settings.sign_on",
            Some(false) => "input.repo_settings.sign_off",
            None => "input.repo_settings.sign_inherit",
        }
    };

    let mut body = div().flex().flex_col().gap(scaled_px(6.0)).px_2().py_1();

    body = body
        .child(input_label(
            theme,
            crate::i18n::tr_str("input.repo_settings.user_label"),
        ))
        .child(
            div()
                .id("repo_settings_user_row")
                .debug_selector(|| "repo_settings_user_input".to_string())
                .w_full()
                .min_w(px(0.0))
                .child(this.repo_settings.repo_settings_user_input.clone()),
        )
        .child(input_label(
            theme,
            crate::i18n::tr_str("input.repo_settings.email_label"),
        ))
        .child(
            div()
                .id("repo_settings_email_row")
                .debug_selector(|| "repo_settings_email_input".to_string())
                .w_full()
                .min_w(px(0.0))
                .child(this.repo_settings.repo_settings_email_input.clone()),
        );

    // The signing override cycles inherit → on → off, so all three states
    // are reachable without a third widget.
    let sign_state = this.repo_settings.repo_settings_sign_commits;
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
                this.repo_settings.repo_settings_sign_commits =
                    match this.repo_settings.repo_settings_sign_commits {
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

    // Fetch-time pruning is a WorkTree preference (persisted in the session
    // file), not a local git-config key, so it is a plain on/off checkbox
    // rather than the tri-state cycle the config overrides use.
    let prune_row = |enabled: bool| -> &'static str {
        match enabled {
            true => "input.repo_settings.fetch_prune_on",
            false => "input.repo_settings.fetch_prune_off",
        }
    };
    let prune_state = this.repo_settings.repo_settings_fetch_prune;
    body = body.child(
        div()
            .id("repo_settings_fetch_prune_row")
            .debug_selector(|| "repo_settings_fetch_prune_toggle".to_string())
            .flex()
            .items_center()
            .gap(scaled_px(6.0))
            .px_1()
            .cursor(gpui::CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                this.repo_settings.repo_settings_fetch_prune =
                    !this.repo_settings.repo_settings_fetch_prune;
                cx.notify();
            }))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.foreground.primary)
                    .child(crate::i18n::tr("input.repo_settings.fetch_prune_label")),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(crate::i18n::tr(prune_row(prune_state))),
            ),
    );

    // Identity profiles: a click fills the draft (user/email/sign), reusing
    // the existing Apply path to commit. The list is empty until the user
    // saves one via the row below.
    let profiles = &this.repo_settings.repo_settings_identity_profiles;
    if !profiles.is_empty() {
        let mut profile_rows = div().flex().flex_col().gap(scaled_px(2.0)).px_1();
        for profile in profiles {
            let name = profile.name.clone();
            let summary = format!("{} <{}>", profile.user_name, profile.user_email);
            let debug_label = format!("repo_settings_profile_{}", profile.name);
            let name_for_click = name.clone();
            profile_rows = profile_rows.child(
                div()
                    .id(SharedString::from(name.clone()))
                    .debug_selector(move || debug_label.clone())
                    .flex()
                    .items_center()
                    .gap(scaled_px(6.0))
                    .px_1()
                    .rounded(scaled_px(4.0))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .on_click(cx.listener(move |this, _e: &ClickEvent, _w, cx| {
                        let profiles = this.repo_settings.repo_settings_identity_profiles.clone();
                        if let Some(profile) = profiles.iter().find(|p| p.name == name_for_click) {
                            this.apply_identity_profile_to_draft(profile, cx);
                        }
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.colors.foreground.primary)
                            .child(name),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(summary),
                    ),
            );
        }
        body = body.child(
            div()
                .id("repo_settings_profiles_section")
                .debug_selector(|| "repo_settings_profiles_section".to_string())
                .flex()
                .flex_col()
                .gap(scaled_px(4.0))
                .px_1()
                .pt(scaled_px(4.0))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(crate::i18n::tr_str("input.repo_settings.profiles_title")),
                )
                .child(profile_rows),
        );
    }

    // Save-as-profile row: name the current draft and persist it.
    let profile_error = this
        .repo_settings
        .repo_settings_profile_error
        .clone()
        .map(|message| {
            div()
                .id("repo_settings_profile_error")
                .debug_selector(|| "repo_settings_profile_error".to_string())
                .px_1()
                .text_xs()
                .text_color(theme.colors.status.danger.foreground)
                .line_clamp(2)
                .child(message)
        });

    body = body
        .child(
            div()
                .id("repo_settings_save_profile_row")
                .debug_selector(|| "repo_settings_save_profile_row".to_string())
                .flex()
                .items_center()
                .gap(scaled_px(6.0))
                .px_1()
                .pt(scaled_px(4.0))
                .child(
                    div()
                        .w(scaled_px(150.0))
                        .min_w(px(0.0))
                        .child(this.repo_settings.repo_settings_profile_name_input.clone()),
                )
                .child(
                    components::Button::new(
                        "repo_settings_save_profile_btn",
                        crate::i18n::tr_str("input.repo_settings.profile_save"),
                    )
                    .style(components::ButtonStyle::Subtle)
                    .on_click(theme, cx, |this, _e, _w, cx| {
                        this.save_identity_profile(cx);
                    }),
                ),
        )
        .children(profile_error);

    let error_row = this
        .repo_settings
        .repo_settings_error
        .clone()
        .map(|message| {
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
                        div()
                            .debug_selector(|| "repo_settings_cancel".to_string())
                            .child(
                                cancel_button(
                                    "repo_settings_cancel",
                                    "repo_settings_cancel_hint",
                                    theme,
                                )
                                .focus_handle(this.repo_settings.repo_settings_focus.cancel.clone())
                                .on_click(
                                    theme,
                                    cx,
                                    |this, _e, window, cx| {
                                        this.dismiss_prompt_popover(window, cx);
                                    },
                                ),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "repo_settings_apply".to_string())
                            .child(
                                components::Button::new(
                                    "repo_settings_apply_btn",
                                    crate::i18n::tr("input.repo_settings.apply"),
                                )
                                .focus_handle(this.repo_settings.repo_settings_focus.submit.clone())
                                .style(components::ButtonStyle::Filled)
                                .on_click(
                                    theme,
                                    cx,
                                    move |this, _e, _w, cx| {
                                        this.submit_repo_settings(repo_id, workdir.clone(), cx);
                                    },
                                ),
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
        assert!(repo_settings_apply_plan(&draft("Old", "", None), &current).is_empty());

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
