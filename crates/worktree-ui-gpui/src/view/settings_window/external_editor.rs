//! External editor: preference persistence queue, detection refresh and the
//! custom-editor draft plumbing.

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Default)]
pub(super) struct ExternalEditorPreferencePersistQueue {
    latest_sequence: Arc<AtomicU64>,
    write_lock: Arc<Mutex<()>>,
}

impl ExternalEditorPreferencePersistQueue {
    pub(super) fn next_sequence(&self) -> u64 {
        self.latest_sequence
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }

    fn persist_if_latest(
        &self,
        sequence: u64,
        setting: Option<ExternalCodeEditorSetting>,
    ) -> std::io::Result<bool> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if self.latest_sequence.load(Ordering::Acquire) != sequence {
            return Ok(false);
        }
        session::persist_ui_settings(external_editor_preference_settings(setting))?;
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn persist_to_path_if_latest(
        &self,
        sequence: u64,
        setting: Option<ExternalCodeEditorSetting>,
        path: &std::path::Path,
    ) -> std::io::Result<bool> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if self.latest_sequence.load(Ordering::Acquire) != sequence {
            return Ok(false);
        }
        session::persist_ui_settings_to_path(external_editor_preference_settings(setting), path)?;
        Ok(true)
    }
}

static EXTERNAL_EDITOR_PREFERENCE_PERSIST_QUEUE: OnceLock<ExternalEditorPreferencePersistQueue> =
    OnceLock::new();

fn external_editor_preference_persist_queue() -> &'static ExternalEditorPreferencePersistQueue {
    EXTERNAL_EDITOR_PREFERENCE_PERSIST_QUEUE.get_or_init(Default::default)
}

fn external_editor_preference_settings(
    setting: Option<ExternalCodeEditorSetting>,
) -> session::UiSettings {
    session::UiSettings {
        external_code_editor: Some(setting),
        ..session::UiSettings::default()
    }
}

pub(super) fn custom_external_editor_path_prompt_options() -> gpui::PathPromptOptions {
    gpui::PathPromptOptions {
        files: true,
        directories: true,
        multiple: false,
        prompt: Some(tr("settings.external_editor.prompt_select")),
    }
}

pub(super) fn initial_external_editor_setting(
    ui_session: &session::UiSession,
) -> Option<ExternalCodeEditorSetting> {
    crate::external_editor::configured_setting_preference_override()
        .unwrap_or_else(|| ui_session.external_code_editor.clone())
}

impl SettingsWindowView {
    /// Swaps in the editor list produced by the background detection pass
    /// that construction kicked off when the cache was stale.
    pub(super) fn refresh_external_editor_options(
        &mut self,
        detected: Vec<crate::external_editor::DetectedExternalEditor>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.external_editor_options =
            crate::external_editor::external_editor_options_from_detected(
                self.external_editor_setting.as_ref(),
                detected,
            )
            .into();
        cx.notify();
    }

    pub(super) fn external_editor_is_custom(&self) -> bool {
        matches!(
            self.external_editor_setting,
            Some(ExternalCodeEditorSetting::Custom { .. })
        )
    }

    fn custom_external_editor_setting_from_drafts(&self) -> ExternalCodeEditorSetting {
        let executable = self.external_editor_custom_path_draft.trim();
        let arguments = self.external_editor_custom_arguments_draft.trim();
        ExternalCodeEditorSetting::Custom {
            executable: if executable.is_empty() {
                PathBuf::new()
            } else {
                PathBuf::from(executable)
            },
            arguments: (!arguments.is_empty()).then(|| arguments.to_string()),
        }
    }

    fn persist_external_editor_preference(&self, cx: &mut gpui::Context<Self>) {
        let setting = self.external_editor_setting.clone();
        crate::external_editor::set_configured_setting_override(setting.clone());
        let persist_queue = external_editor_preference_persist_queue().clone();
        let sequence = persist_queue.next_sequence();
        let setting_for_persist = setting.clone();
        cx.background_spawn(async move {
            let _ = persist_queue.persist_if_latest(sequence, setting_for_persist);
        })
        .detach();
        cx.defer(move |cx| {
            crate::app::refresh_external_editor_app_surfaces_for_setting(setting.as_ref(), cx);
        });
    }

    pub(super) fn apply_browsed_external_editor_path(
        &mut self,
        path: PathBuf,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = path.display().to_string();
        self.external_editor_custom_path_draft = next.clone();
        self.external_editor_custom_path_input
            .update(cx, |input, cx| input.set_text(next, cx));
        self.persist_external_editor_from_custom_drafts(cx);
        self.notify_after_external_editor_browse(cx);
    }

    fn notify_after_external_editor_browse(&mut self, cx: &mut gpui::Context<Self>) {
        #[cfg(test)]
        {
            self.external_editor_browse_notify_count += 1;
        }
        cx.notify();
    }

    pub(super) fn persist_external_editor_from_custom_drafts(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) {
        let next = self.custom_external_editor_setting_from_drafts();
        if self.external_editor_setting.as_ref() == Some(&next) {
            return;
        }
        self.external_editor_setting = Some(next);
        self.persist_external_editor_preference(cx);
    }

    pub(super) fn set_external_editor_setting(
        &mut self,
        next: Option<ExternalCodeEditorSetting>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.external_editor_setting == next {
            self.expanded_section = None;
            cx.notify();
            return;
        }

        self.external_editor_setting = next;
        self.expanded_section = None;
        self.persist_external_editor_preference(cx);
        cx.notify();
    }

    pub(super) fn select_custom_external_editor(&mut self, cx: &mut gpui::Context<Self>) {
        self.set_external_editor_setting(
            Some(self.custom_external_editor_setting_from_drafts()),
            cx,
        );
    }
}
