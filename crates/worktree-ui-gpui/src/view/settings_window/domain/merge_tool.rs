//! Merge tool settings: preset selection, custom command/path drafts and
//! background availability probing.

use super::*;
use std::path::PathBuf;
use worktree_core::external_merge_tool::MERGE_TOOL_PRESETS as MERGE_TOOL_PRESET_TABLE;

/// The merge tool wants a single executable file, not a directory.
fn merge_tool_executable_path_prompt_options() -> gpui::PathPromptOptions {
    gpui::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(tr("settings.merge_tool.executable_path_prompt")),
    }
}

/// What the merge tool page shows for the selected built-in preset: the
/// effective executable (manual path if one is set, otherwise the first
/// program found on `PATH`), or why neither applies. Computed in the
/// background so a render never touches the filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view::settings_window) enum MergeToolAvailability {
    /// The effective executable exists. `resolved` is the full path, echoed
    /// into the executable-path input; `via_override` distinguishes a manual
    /// path from a PATH hit for the status message.
    Available {
        resolved: String,
        via_override: bool,
    },
    /// A manual path is configured but neither exists as a file nor resolves
    /// on PATH.
    OverrideMissing(String),
    /// No manual path, and none of these candidate names were on PATH.
    Missing(Vec<String>),
}

/// One selectable row of the merge tool dropdown: the git-config default, a
/// built-in preset, or the custom command.
pub(in crate::view::settings_window) enum MergeToolOption {
    FromGitConfig,
    Preset(&'static worktree_core::external_merge_tool::MergeToolPreset),
    Custom,
}

/// The dropdown rows, in display order.
pub(in crate::view::settings_window) fn merge_tool_options() -> Vec<MergeToolOption> {
    let mut options = Vec::with_capacity(2 + MERGE_TOOL_PRESET_TABLE.len());
    options.push(MergeToolOption::FromGitConfig);
    options.extend(MERGE_TOOL_PRESET_TABLE.iter().map(MergeToolOption::Preset));
    options.push(MergeToolOption::Custom);
    options
}

/// Label shown for the current selection on the collapsed summary row.
pub(in crate::view::settings_window) fn merge_tool_selection_summary(
    selection: &ExternalMergeToolSelection,
) -> SharedString {
    match selection {
        ExternalMergeToolSelection::FromGitConfig => {
            tr_str("settings.merge_tool.from_git_config").into()
        }
        // A preset id the table no longer knows (hand-edited session, older
        // build) still deserves an honest label rather than a silent reset.
        ExternalMergeToolSelection::Builtin { id, .. } => {
            worktree_core::external_merge_tool::merge_tool_preset(id)
                .map(|preset| SharedString::from(tr_str(preset.label_key)))
                .unwrap_or_else(|| SharedString::from(id.clone()))
        }
        ExternalMergeToolSelection::Custom { .. } => tr_str("settings.merge_tool.custom").into(),
    }
}

impl SettingsWindowView {
    /// Install the selection into the process global (so the next conflicted
    /// right-click already uses it) and persist it to the session file.
    pub(in crate::view::settings_window) fn persist_merge_tool_preference(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        worktree_core::external_merge_tool::install_external_merge_tool(
            self.merge_tool_selection.clone(),
        );
        self.persist_preferences(cx);
    }

    /// Switch the external merge tool. Switching to Custom adopts the current
    /// command draft; switching away keeps the draft for a later switch back.
    pub(in crate::view::settings_window) fn set_merge_tool_selection(
        &mut self,
        selection: ExternalMergeToolSelection,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.merge_tool_selection == selection {
            return;
        }
        self.merge_tool_selection = selection;
        self.reset_merge_tool_executable_path_input(cx);
        self.persist_merge_tool_preference(cx);
        self.refresh_merge_tool_availability(cx);
        cx.notify();
    }

    /// Point the executable-path input at the current selection's manual path
    /// (empty for a fresh preset); the PATH echo fills in once the
    /// availability check lands.
    fn reset_merge_tool_executable_path_input(&mut self, cx: &mut gpui::Context<Self>) {
        let text = match &self.merge_tool_selection {
            ExternalMergeToolSelection::Builtin {
                path: Some(path), ..
            } => path.clone(),
            _ => String::new(),
        };
        if self.merge_tool_executable_path_draft == text {
            return;
        }
        self.merge_tool_executable_path_draft = text.clone();
        self.merge_tool_executable_path_input
            .update(cx, |input, cx| input.set_text(text, cx));
    }

    /// Set or clear the selected built-in tool's manual executable path. The
    /// input's text is kept in sync by the caller (observer, browse or clear).
    pub(in crate::view::settings_window) fn set_merge_tool_manual_path(
        &mut self,
        path: Option<String>,
        cx: &mut gpui::Context<Self>,
    ) {
        let ExternalMergeToolSelection::Builtin { path: current, .. } =
            &mut self.merge_tool_selection
        else {
            return;
        };
        if *current == path {
            return;
        }
        *current = path;
        self.merge_tool_path_generation += 1;
        self.persist_merge_tool_preference(cx);
        self.refresh_merge_tool_availability(cx);
        cx.notify();
    }

    /// Adopt a file picked in the browse dialog as the manual executable path.
    pub(in crate::view::settings_window) fn apply_browsed_merge_tool_path(
        &mut self,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        let rendered = path.display().to_string();
        self.merge_tool_executable_path_draft = rendered.clone();
        self.merge_tool_executable_path_input
            .update(cx, |input, cx| input.set_text(rendered.clone(), cx));
        self.set_merge_tool_manual_path(Some(rendered), cx);
    }

    /// Drop the manual executable path; the field falls back to the PATH echo
    /// once the availability check lands, empty when nothing was found.
    pub(in crate::view::settings_window) fn clear_merge_tool_manual_path(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        self.merge_tool_executable_path_draft = String::new();
        self.merge_tool_executable_path_input
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.set_merge_tool_manual_path(None, cx);
    }

    pub(in crate::view::settings_window) fn browse_merge_tool_executable_path(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(merge_tool_executable_path_prompt_options());
        let view = cx.weak_entity();

        window
            .spawn(cx, async move |cx| {
                let paths = match rx.await {
                    Ok(Ok(Some(paths))) => paths,
                    Ok(Ok(None)) => return,
                    Ok(Err(_)) | Err(_) => return,
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                let _ = view.update(cx, |this, cx| {
                    this.apply_browsed_merge_tool_path(path, cx);
                });
            })
            .detach();
    }

    /// Echo the effective executable into the path input while the user has
    /// not taken manual control: with no manual path the field shows the
    /// PATH-resolved executable (informational — editing adopts it as the
    /// manual path) and clears when nothing was found.
    fn sync_merge_tool_path_echo(&mut self, generation: u64, cx: &mut gpui::Context<Self>) {
        if generation != self.merge_tool_path_generation {
            return;
        }
        if matches!(
            &self.merge_tool_selection,
            ExternalMergeToolSelection::Builtin { path: Some(path), .. }
                if !path.trim().is_empty()
        ) {
            return;
        }
        let echo = match &self.merge_tool_availability {
            Some(MergeToolAvailability::Available {
                resolved,
                via_override: false,
            }) => resolved.clone(),
            _ => String::new(),
        };
        if self.merge_tool_executable_path_draft == echo {
            return;
        }
        self.merge_tool_executable_path_draft = echo.clone();
        self.merge_tool_executable_path_input
            .update(cx, |input, cx| input.set_text(echo, cx));
    }

    pub(in crate::view::settings_window) fn set_merge_tool_trust_exit_code(
        &mut self,
        value: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let ExternalMergeToolSelection::Custom {
            trust_exit_code, ..
        } = &mut self.merge_tool_selection
        else {
            return;
        };
        if *trust_exit_code == value {
            return;
        }
        *trust_exit_code = value;
        self.persist_merge_tool_preference(cx);
        cx.notify();
    }

    /// Check the selected built-in preset's effective executable in the
    /// background — the lookup touches the filesystem and must not stall a
    /// render. `None` in `merge_tool_availability` means "in flight"; the row
    /// is only shown for built-in presets at all.
    pub(in crate::view::settings_window) fn refresh_merge_tool_availability(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        use crate::ai_commit_sources::{EnvAccess, find_executable};

        let ExternalMergeToolSelection::Builtin { id, path } = &self.merge_tool_selection else {
            self.merge_tool_availability = None;
            return;
        };
        let Some(preset) = worktree_core::external_merge_tool::merge_tool_preset(id) else {
            self.merge_tool_availability = None;
            return;
        };
        let candidates = preset.program_candidates.to_vec();
        let manual_path = path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string);
        let queried_id = id.to_string();
        let queried_manual_path = manual_path.clone();
        let generation = self.merge_tool_path_generation;
        self.merge_tool_availability = None;
        cx.spawn(async move |this, cx| {
            let availability = cx
                .background_spawn(async move {
                    let env = EnvAccess::real();
                    match manual_path {
                        // The backend uses the manual path verbatim; a bare
                        // name still resolves through PATH as a courtesy.
                        Some(manual) => {
                            let resolved = if std::path::Path::new(&manual).is_file() {
                                Some(manual.clone())
                            } else {
                                find_executable(&manual, &env)
                                    .map(|path| path.display().to_string())
                            };
                            match resolved {
                                Some(resolved) => MergeToolAvailability::Available {
                                    resolved,
                                    via_override: true,
                                },
                                None => MergeToolAvailability::OverrideMissing(manual),
                            }
                        }
                        None => {
                            match candidates
                                .iter()
                                .find_map(|name| find_executable(name, &env))
                            {
                                Some(path) => MergeToolAvailability::Available {
                                    resolved: path.display().to_string(),
                                    via_override: false,
                                },
                                None => MergeToolAvailability::Missing(
                                    candidates.iter().map(|name| name.to_string()).collect(),
                                ),
                            }
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // A newer edit or preset switch re-queried — drop the stale
                // result rather than flash it (or echo it into the input).
                let still_current = match &this.merge_tool_selection {
                    ExternalMergeToolSelection::Builtin { id, path } => {
                        *id == queried_id
                            && path
                                .as_deref()
                                .map(str::trim)
                                .filter(|path| !path.is_empty())
                                .map(str::to_string)
                                == queried_manual_path
                            && generation == this.merge_tool_path_generation
                    }
                    _ => false,
                };
                if !still_current {
                    return;
                }
                this.merge_tool_availability = Some(availability);
                this.sync_merge_tool_path_echo(generation, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::view::settings_window) fn merge_tool_card(
        &self,
        theme: AppTheme,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let merge_tool_row = self
            .summary_row(
                "settings_window_merge_tool_selection",
                tr_str("settings.merge_tool.row_label"),
                merge_tool_selection_summary(&self.merge_tool_selection),
                self.expanded_section == Some(SettingsSection::MergeTool),
                theme,
            )
            .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                this.toggle_section(SettingsSection::MergeTool, cx);
            }));

        let merge_tool_custom = matches!(
            &self.merge_tool_selection,
            ExternalMergeToolSelection::Custom { .. }
        );
        let merge_tool_trust_exit_code = match &self.merge_tool_selection {
            ExternalMergeToolSelection::Custom {
                trust_exit_code, ..
            } => *trust_exit_code,
            _ => false,
        };
        let merge_tool_trust_exit_code_row = self
            .toggle_row(
                "settings_window_merge_tool_trust_exit_code",
                tr_str("settings.merge_tool.trust_exit_code"),
                merge_tool_trust_exit_code,
                theme,
            )
            .on_click(cx.listener(move |this, _e: &ClickEvent, _window, cx| {
                this.set_merge_tool_trust_exit_code(
                    !matches!(
                        &this.merge_tool_selection,
                        ExternalMergeToolSelection::Custom {
                            trust_exit_code: true,
                            ..
                        }
                    ),
                    cx,
                );
            }));

        let mut merge_tool_card = self
            .card(
                "settings_window_merge_tool",
                tr_str("settings.nav.merge_tool"),
                theme,
            )
            .child(
                div()
                    .id("settings_window_merge_tool_scope_note")
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(tr("settings.merge_tool.scope_note")),
            )
            .child(merge_tool_row);

        if self.expanded_section == Some(SettingsSection::MergeTool) {
            let option_count = merge_tool_options().len();
            let list = uniform_list(
                "settings_window_merge_tool_list",
                option_count,
                cx.processor(Self::render_merge_tool_option_rows),
            )
            .w_full()
            .min_w(px(0.0))
            .h_full()
            .min_h(px(0.0))
            .track_scroll(&self.merge_tool_scroll)
            .on_scroll_wheel(stop_dropdown_wheel_chaining(self.merge_tool_scroll.clone()));
            let list = restrict_scroll_to_vertical_axis(list).into_any_element();
            merge_tool_card = merge_tool_card.child(self.dropdown_list_container(
                "settings_window_merge_tool_list_container",
                "settings_window_merge_tool_scrollbar",
                self.merge_tool_scroll.clone(),
                option_count,
                SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
                SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX,
                SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
                list,
                theme,
            ));

            let merge_tool_manual_path = match &self.merge_tool_selection {
                ExternalMergeToolSelection::Builtin {
                    path: Some(path), ..
                } => {
                    let trimmed = path.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                }
                _ => None,
            };
            if let ExternalMergeToolSelection::Builtin { id, .. } = &self.merge_tool_selection {
                let preset_missing =
                    worktree_core::external_merge_tool::merge_tool_preset(id).is_none();
                let resolved_program = |resolved: &str| {
                    std::path::Path::new(resolved)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| resolved.to_string())
                };
                let (status_text, status_color) = match &self.merge_tool_availability {
                    None if preset_missing => (
                        tr("settings.merge_tool.unknown_preset"),
                        theme.colors.status.warning.foreground,
                    ),
                    None => (
                        tr("settings.merge_tool.checking"),
                        theme.colors.foreground.secondary,
                    ),
                    Some(MergeToolAvailability::Available {
                        resolved,
                        via_override: false,
                    }) => (
                        crate::i18n::t!(
                            "settings.merge_tool.available",
                            program = resolved_program(resolved)
                        )
                        .into_owned()
                        .into(),
                        theme.colors.status.success.foreground,
                    ),
                    Some(MergeToolAvailability::Available {
                        resolved,
                        via_override: true,
                    }) => (
                        crate::i18n::t!("settings.merge_tool.available_override", path = resolved)
                            .into_owned()
                            .into(),
                        theme.colors.status.success.foreground,
                    ),
                    Some(MergeToolAvailability::OverrideMissing(path)) => (
                        crate::i18n::t!("settings.merge_tool.override_missing", path = path)
                            .into_owned()
                            .into(),
                        theme.colors.status.warning.foreground,
                    ),
                    Some(MergeToolAvailability::Missing(candidates)) => (
                        crate::i18n::t!(
                            "settings.merge_tool.missing",
                            programs = candidates.join(", ")
                        )
                        .into_owned()
                        .into(),
                        theme.colors.status.warning.foreground,
                    ),
                };
                merge_tool_card = merge_tool_card.child(
                    div()
                        .id("settings_window_merge_tool_availability")
                        .debug_selector(|| "settings_window_merge_tool_availability".to_string())
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_text),
                );

                // Manual executable path: empty means "resolve
                // from PATH", and the field echoes the PATH hit
                // so the effective executable is always visible.
                let browse_button = components::Button::new(
                    "settings_window_merge_tool_executable_path_browse",
                    tr("settings.action.browse"),
                )
                .style(components::ButtonStyle::Outlined)
                .on_click(theme, cx, |this, _e, window, cx| {
                    this.browse_merge_tool_executable_path(window, cx);
                });
                let path_row = div()
                    .px_2()
                    .pb_1()
                    .w_full()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(self.merge_tool_executable_path_input.clone()),
                    )
                    .child(browse_button);
                // Clearing hands the field back to the PATH echo.
                let path_row = if merge_tool_manual_path.is_some() {
                    let clear_button = components::Button::new(
                        "settings_window_merge_tool_executable_path_clear",
                        tr("settings.merge_tool.clear_path"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |this, _e, _window, cx| {
                        this.clear_merge_tool_manual_path(cx);
                    });
                    path_row.child(
                        div()
                            .id("settings_window_merge_tool_executable_path_clear")
                            .debug_selector(|| {
                                "settings_window_merge_tool_executable_path_clear".to_string()
                            })
                            .child(clear_button),
                    )
                } else {
                    path_row
                };
                merge_tool_card = merge_tool_card.child(
                    self.detail_container(
                        "settings_window_merge_tool_executable_path_container",
                        theme,
                    )
                    .child(
                        div()
                            .px_2()
                            .pt_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.merge_tool.executable_path")),
                    )
                    .child(path_row)
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .text_color(theme.colors.foreground.secondary)
                            .child(tr_str("settings.merge_tool.executable_path_hint")),
                    ),
                );
            }

            merge_tool_card = merge_tool_card.child(
                div()
                    .id("settings_window_merge_tool_hint")
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(tr_str("settings.merge_tool.hint")),
            );

            if merge_tool_custom {
                merge_tool_card = merge_tool_card.child(
                    self.detail_container("settings_window_merge_tool_custom_container", theme)
                        .child(
                            div()
                                .px_2()
                                .pt_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.merge_tool.custom_command")),
                        )
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .w_full()
                                .min_w(px(0.0))
                                .child(self.merge_tool_custom_command_input.clone()),
                        )
                        .child(
                            div()
                                .px_2()
                                .pb_1()
                                .text_xs()
                                .text_color(theme.colors.foreground.secondary)
                                .child(tr_str("settings.merge_tool.custom_hint")),
                        ),
                );
                merge_tool_card = merge_tool_card.child(merge_tool_trust_exit_code_row);
                merge_tool_card = merge_tool_card.child(
                    div()
                        .id("settings_window_merge_tool_trust_exit_code_hint")
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.colors.foreground.secondary)
                        .child(tr_str("settings.merge_tool.trust_exit_code_hint")),
                );
            }
        }
        merge_tool_card
    }
}
