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
}
