//! AI commit message settings: source/provider/model state, the model-list
//! fetch lifecycle and persistence.

use super::*;

/// The model list fetched from the provider's `/models` endpoint, for the AI
/// commit settings' model picker. The draft fields are plain `String`s that
/// sync straight to their inputs; this one has a request lifecycle instead.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum AiCommitModels {
    #[default]
    NotFetched,
    Loading,
    Ready(Arc<[String]>),
    Error(String),
}

impl SettingsWindowView {
    /// Check the selected non-manual source in the background — resolution
    /// touches the filesystem (config files, PATH) and must not stall a
    /// render. The result lands in `ai_commit_availability` for the status
    /// row; `None` while in flight.
    pub(super) fn refresh_ai_commit_availability(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::ai_commit_sources::{EnvAccess, check_availability};

        let source = self.ai_commit_source;
        let manual = crate::ai_commit::current();
        let custom_command = self.ai_commit_custom_command_draft.clone();
        self.ai_commit_availability = None;
        cx.spawn(async move |this, cx| {
            let availability = cx
                .background_spawn(async move {
                    check_availability(source, &manual, &custom_command, &EnvAccess::real())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.ai_commit_availability = Some(availability);
                cx.notify();
            });
        })
        .detach();
    }

    /// Switch the configuration source. Manual keeps its provider fields;
    /// every other source resolves credentials live, so the manual drafts
    /// stay as they are for a later switch back.
    pub(super) fn set_ai_commit_source(
        &mut self,
        source: crate::ai_commit_sources::AiSource,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ai_commit_source == source {
            return;
        }
        self.ai_commit_source = source;
        // The fetched model list belongs to the manual endpoint; it is stale
        // the moment another source is selected.
        self.ai_commit_models = AiCommitModels::NotFetched;
        self.persist_ai_commit_settings(cx);
        if source != crate::ai_commit_sources::AiSource::Manual {
            self.refresh_ai_commit_availability(cx);
        } else {
            self.ai_commit_availability = None;
        }
        cx.notify();
    }

    /// Summary value for the AI section row: the selected source's name —
    /// the manual provider, annotated when no API key is set yet.
    pub(super) fn ai_commit_summary(&self) -> gpui::SharedString {
        if self.ai_commit_source != crate::ai_commit_sources::AiSource::Manual {
            return self.ai_commit_source.label();
        }
        let label = self.ai_commit_provider.label();
        if self.ai_commit_api_key_draft.trim().is_empty() {
            crate::i18n::t!("settings.ai_commit.summary_unconfigured", provider = label).into()
        } else {
            label
        }
    }

    /// Push the AI section's drafts into the process-global settings and
    /// persist them. The ✨ button reads the global, so every edit keeps it
    /// in sync. External-source credentials are never part of this — they
    /// are resolved at generation time.
    pub(super) fn persist_ai_commit_settings(&mut self, cx: &mut gpui::Context<Self>) {
        crate::ai_commit::set_current(crate::ai_commit::AiCommitSettings {
            source: self.ai_commit_source,
            provider: self.ai_commit_provider,
            api_key: self.ai_commit_api_key_draft.clone(),
            // A key typed on this page is a console API key, never a token.
            bearer_auth: false,
            model: self.ai_commit_model_draft.clone(),
            endpoint: self.ai_commit_endpoint_draft.clone(),
            custom_command: self.ai_commit_custom_command_draft.clone(),
        });
        self.persist_preferences(cx);
    }

    /// Switch the AI provider, resetting model and endpoint to the new
    /// provider's defaults so a stray value from the other provider is never
    /// sent to the new endpoint.
    pub(super) fn set_ai_commit_provider(
        &mut self,
        provider: crate::ai_commit::AiProvider,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ai_commit_provider == provider {
            return;
        }

        self.ai_commit_provider = provider;
        self.ai_commit_model_draft = provider.default_model().to_string();
        self.ai_commit_endpoint_draft.clear();
        // The fetched model list belongs to the old provider's endpoint — a
        // stale list next to the new provider's defaults would invite picking
        // a model the new endpoint rejects.
        self.ai_commit_models = AiCommitModels::NotFetched;
        self.ai_commit_model_input.update(cx, |input, cx| {
            input.set_text(provider.default_model().to_string(), cx);
        });
        self.ai_commit_endpoint_input
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        self.persist_ai_commit_settings(cx);
        cx.notify();
    }

    /// Fetch the provider's model list for the model picker. The drafts —
    /// not the process global — are the source of truth here: the user may
    /// have edited the endpoint or key without blurring the input yet.
    pub(super) fn fetch_ai_commit_models(&mut self, cx: &mut gpui::Context<Self>) {
        if matches!(self.ai_commit_models, AiCommitModels::Loading) {
            return;
        }
        self.ai_commit_models = AiCommitModels::Loading;
        cx.notify();

        // The request needs the network; test builds exercise the state
        // machine up to this point and count what would be sent.
        #[cfg(not(test))]
        {
            let settings = crate::ai_commit::AiCommitSettings {
                source: self.ai_commit_source,
                provider: self.ai_commit_provider,
                api_key: self.ai_commit_api_key_draft.clone(),
                bearer_auth: false,
                model: self.ai_commit_model_draft.clone(),
                endpoint: self.ai_commit_endpoint_draft.clone(),
                custom_command: self.ai_commit_custom_command_draft.clone(),
            };
            cx.spawn(async move |pane, cx| {
                let result = crate::ai_commit::fetch_models(&settings).await;
                let _ = pane.update(cx, |pane, cx| {
                    pane.finish_ai_commit_models_fetch(result, cx);
                });
            })
            .detach();
        }
        #[cfg(test)]
        {
            self.ai_commit_models_test_fetches += 1;
        }
    }

    pub(super) fn finish_ai_commit_models_fetch(
        &mut self,
        result: Result<Vec<String>, String>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.ai_commit_models = match result {
            Ok(models) => AiCommitModels::Ready(models.into()),
            Err(error) => AiCommitModels::Error(error),
        };
        cx.notify();
    }

    pub(super) fn set_ai_commit_model(&mut self, model: String, cx: &mut gpui::Context<Self>) {
        if self.ai_commit_model_draft == model {
            return;
        }
        self.ai_commit_model_draft = model.clone();
        self.ai_commit_model_input
            .update(cx, |input, cx| input.set_text(model, cx));
        self.persist_ai_commit_settings(cx);
    }
}
