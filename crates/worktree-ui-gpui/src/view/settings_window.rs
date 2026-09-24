//! Settings window facade: the view state, constructor, shared persistence
//! plumbing and the `Render` dispatch. Domain logic lives in child modules:
//! `window_chrome` (sizing/chrome), `categories` (nav model), `widgets`
//! (shared row builders), `option_rows` (dropdown row sources),
//! `git_runtime`, `external_editor`, `ai_commit` and `domain/` (per-category
//! settings).

use self::ai_commit::AiCommitModels;
use self::categories::{SettingsCategory, SettingsSection, SettingsView};
use self::domain::diff::{
    CHANGE_TRACKING_OPTIONS, DIFF_CONTENT_MODE_OPTIONS, DIFF_SCROLL_SYNC_OPTIONS,
    DIFF_VIEW_MODE_OPTIONS,
};
use self::domain::general::settings_theme_mode_options;
use self::domain::gpg_signing::GpgConfig;
use self::domain::links::THEMES_GUIDE_URL;
use self::domain::merge_tool::{MergeToolAvailability, MergeToolOption, merge_tool_options};
use self::domain::tags::git_log_tag_fetch_mode_label;
use self::domain::terminal::TerminalSettingsStatus;
use self::external_editor::{
    custom_external_editor_path_prompt_options, initial_external_editor_setting,
};
use self::git_runtime::{GitExecutableMode, SettingsRuntimeInfo, applied_git_executable_path};
use self::widgets::{
    SETTINGS_DROPDOWN_COMPACT_LIST_EXTRA_HEIGHT_PX, SETTINGS_DROPDOWN_COMPACT_ROW_HEIGHT_PX,
    SETTINGS_DROPDOWN_DENSE_DETAIL_ROW_HEIGHT_PX, SETTINGS_DROPDOWN_DETAIL_LIST_EXTRA_HEIGHT_PX,
    SETTINGS_DROPDOWN_DETAIL_ROW_HEIGHT_PX, SETTINGS_DROPDOWN_LIST_MAX_HEIGHT_PX,
    SETTINGS_THEME_DROPDOWN_LIST_MAX_HEIGHT_PX, settings_row_separator_color,
    stop_dropdown_wheel_chaining,
};
use self::window_chrome::{
    settings_window_client_inset_for_scale, settings_window_default_size_for_percent,
    settings_window_frame, settings_window_options_for_scale,
    settings_window_traffic_lights_safe_inset,
};
#[cfg(test)]
use super::scroll_geometry::absolute_scroll_y;
use super::*;
use crate::i18n::{t, tr, tr_str};
use crate::ui_scale;
use std::sync::Arc;
use worktree_core::domain::HistoryMode;
use worktree_core::external_merge_tool::ExternalMergeToolSelection;
use worktree_core::process::GitExecutablePreference;
use worktree_state::model::{DefaultTagType, GitLogTagFetchMode};
use worktree_state::session::ExternalCodeEditorSetting;
// Consumed only by the tests subtree: imported under cfg(test) so the
// plain library build carries no unused imports.
#[cfg(test)]
use self::domain::general::settings_theme_modes;
#[cfg(test)]
use self::external_editor::ExternalEditorPreferencePersistQueue;
#[cfg(test)]
use self::git_runtime::git_executable_scope_note;
#[cfg(test)]
use self::git_runtime::{
    GitCompatibility, GitVersion, MIN_GIT_MAJOR, MIN_GIT_MINOR, git_runtime_info_from_state,
    is_supported_git_version, parse_git_version, parse_git_version_token, parse_u32_prefix,
};
#[cfg(test)]
use self::widgets::{settings_dropdown_background, uniform_list_vertical_scroll_metrics};
#[cfg(test)]
use self::window_chrome::{
    SETTINGS_WINDOW_DEFAULT_HEIGHT_PX, SETTINGS_WINDOW_DEFAULT_WIDTH_PX,
    SETTINGS_WINDOW_MIN_HEIGHT_PX, SETTINGS_WINDOW_MIN_WIDTH_PX, SETTINGS_WINDOW_TITLE,
    settings_window_client_inset, settings_window_options, settings_window_titlebar_options,
};
#[cfg(test)]
use gpui::{WindowBounds, WindowDecorations};

mod ai_commit;
mod categories;
mod domain;
mod external_editor;
mod git_runtime;
mod option_rows;
mod widgets;
mod window_chrome;

pub(crate) struct SettingsWindowView {
    theme_mode: ThemeMode,
    theme: AppTheme,
    language: crate::i18n::Language,
    avatar_source: crate::avatar_source::AvatarSource,
    ui_scale_percent: u32,
    /// Row-rhythm tier shown in the Density row; mirrors the app-wide global.
    ui_density: crate::density::Density,
    ui_font_family: String,
    editor_font_family: String,
    use_font_ligatures: bool,
    ui_font_options: Arc<[String]>,
    editor_font_options: Arc<[String]>,
    external_editor_options: Arc<[crate::external_editor::ExternalEditorOption]>,
    settings_window_scroll: ScrollHandle,
    theme_scroll: UniformListScrollHandle,
    language_scroll: UniformListScrollHandle,
    avatar_source_scroll: UniformListScrollHandle,
    ui_font_scroll: UniformListScrollHandle,
    editor_font_scroll: UniformListScrollHandle,
    external_editor_scroll: UniformListScrollHandle,
    date_format_scroll: UniformListScrollHandle,
    timezone_scroll: UniformListScrollHandle,
    change_tracking_scroll: UniformListScrollHandle,
    diff_content_mode_scroll: UniformListScrollHandle,
    diff_scroll_sync_scroll: UniformListScrollHandle,
    diff_view_mode_scroll: UniformListScrollHandle,
    date_time_format: DateTimeFormat,
    timezone: Timezone,
    show_timezone: bool,
    change_tracking_view: ChangeTrackingView,
    terminal_preferences: TerminalPreferences,
    terminal_external_program_input: Entity<components::TextInput>,
    terminal_external_args_input: Entity<components::TextInput>,
    terminal_status: Option<TerminalSettingsStatus>,
    diff_content_mode: DiffContentMode,
    diff_whitespace_mode: DiffWhitespaceMode,
    diff_view_mode: DiffViewMode,
    diff_reveal_whitespace_chars: bool,
    diff_word_wrap: bool,
    diff_show_line_numbers: bool,
    auto_save_file_edits: bool,
    diff_scroll_sync: DiffScrollSync,
    history_show_graph: bool,
    history_show_author: bool,
    history_show_date: bool,
    history_show_sha: bool,
    history_relative_dates: bool,
    history_highlight_commit_chain: bool,
    history_show_tags: bool,
    history_tag_fetch_mode: GitLogTagFetchMode,
    default_history_mode: HistoryMode,
    default_tag_type: DefaultTagType,
    current_view: SettingsView,
    selected_category: SettingsCategory,
    search_query: String,
    search_input: Entity<components::TextInput>,
    nav_scroll: ScrollHandle,
    open_source_licenses_scroll: UniformListScrollHandle,
    runtime_info: SettingsRuntimeInfo,
    git_executable_mode: GitExecutableMode,
    git_custom_path_draft: String,
    git_executable_input: Entity<components::TextInput>,
    gpg_config: GpgConfig,
    gpg_signing_key_draft: String,
    gpg_program_draft: String,
    gpg_signing_key_input: Entity<components::TextInput>,
    gpg_program_input: Entity<components::TextInput>,
    gpg_save_error: Option<String>,
    external_editor_setting: Option<ExternalCodeEditorSetting>,
    external_editor_custom_path_draft: String,
    external_editor_custom_arguments_draft: String,
    external_editor_custom_path_input: Entity<components::TextInput>,
    external_editor_custom_arguments_input: Entity<components::TextInput>,
    ai_commit_source: crate::ai_commit_sources::AiSource,
    ai_commit_source_scroll: UniformListScrollHandle,
    ai_commit_availability: Option<crate::ai_commit_sources::SourceAvailability>,
    ai_commit_custom_command_draft: String,
    ai_commit_custom_command_input: Entity<components::TextInput>,
    ai_commit_provider: crate::ai_commit::AiProvider,
    ai_commit_provider_scroll: UniformListScrollHandle,
    ai_commit_models: AiCommitModels,
    ai_commit_models_scroll: UniformListScrollHandle,
    ai_commit_model_draft: String,
    ai_commit_api_key_draft: String,
    ai_commit_endpoint_draft: String,
    ai_commit_model_input: Entity<components::TextInput>,
    ai_commit_api_key_input: Entity<components::TextInput>,
    ai_commit_endpoint_input: Entity<components::TextInput>,
    merge_tool_selection: ExternalMergeToolSelection,
    merge_tool_scroll: UniformListScrollHandle,
    /// Mirrors the custom-command input even when the selection is not
    /// `Custom`, so text typed before switching is not lost.
    merge_tool_custom_command_draft: String,
    merge_tool_custom_command_input: Entity<components::TextInput>,
    /// Mirrors the executable-path input, like the custom-command draft. When
    /// no manual path is set this holds the informational PATH echo, so a
    /// programmatic `set_text` is never mistaken for user input.
    merge_tool_executable_path_draft: String,
    merge_tool_executable_path_input: Entity<components::TextInput>,
    /// Bumped on every executable-path change; an availability result only
    /// echoes into the input while its generation is still current, so a
    /// background PATH hit can never clobber mid-typing text.
    merge_tool_path_generation: u64,
    merge_tool_availability: Option<MergeToolAvailability>,
    expanded_section: Option<SettingsSection>,
    hover_resize_edge: Option<ResizeEdge>,
    title_drag_state: chrome::TitleBarDragState,
    _git_executable_input_subscription: gpui::Subscription,
    _gpg_signing_key_input_subscription: gpui::Subscription,
    _gpg_program_input_subscription: gpui::Subscription,
    _external_editor_custom_path_input_subscription: gpui::Subscription,
    _external_editor_custom_arguments_input_subscription: gpui::Subscription,
    _ai_commit_model_input_subscription: gpui::Subscription,
    _ai_commit_custom_command_input_subscription: gpui::Subscription,
    _ai_commit_api_key_input_subscription: gpui::Subscription,
    _ai_commit_endpoint_input_subscription: gpui::Subscription,
    _merge_tool_custom_command_input_subscription: gpui::Subscription,
    _merge_tool_executable_path_input_subscription: gpui::Subscription,
    _appearance_subscription: gpui::Subscription,
    _search_input_subscription: gpui::Subscription,
    #[cfg(test)]
    overflow_probe: bool,
    #[cfg(test)]
    external_editor_browse_notify_count: usize,
    #[cfg(test)]
    ai_commit_models_test_fetches: usize,
    #[cfg(test)]
    gpg_config_test_writes: Vec<(String, Option<String>)>,
}

pub(crate) fn open_settings_window(cx: &mut App) {
    if let Some(window) = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsWindowView>())
    {
        let _ = window.update(cx, |_view, window, _cx| {
            window.activate_window();
        });
        cx.activate(true);
        return;
    }

    let ui_session = session::load();
    let ui_scale = ui_scale::current_or_initialize_from_session(&ui_session, cx);
    crate::density::current_or_initialize_from_session(&ui_session, cx);
    let bounds = Bounds::centered(
        None,
        settings_window_default_size_for_percent(ui_scale.percent),
        cx,
    );
    let ui_scale_percent = ui_scale.percent;
    cx.open_window(
        settings_window_options_for_scale(bounds, ui_scale_percent),
        move |window, cx| {
            ui_scale::apply_to_window(window, ui_scale_percent);
            window.on_window_should_close(cx, |window, cx| {
                crate::app::mark_clean_shutdown_if_last_window(cx);
                window.remove_window();
                false
            });
            cx.new(|cx| SettingsWindowView::new(window, cx))
        },
    )
    .expect("failed to open settings window");

    cx.activate(true);
}

impl SettingsWindowView {
    fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let ui_session = session::load();
        let ui_scale = ui_scale::current_or_initialize_from_session(&ui_session, cx);
        crate::density::current_or_initialize_from_session(&ui_session, cx);
        let font_preferences =
            crate::font_preferences::current_or_initialize_from_session(&ui_session, cx);
        let theme_mode = ui_session
            .theme_mode
            .as_deref()
            .and_then(ThemeMode::from_key)
            .unwrap_or_default();
        crate::i18n::current_or_initialize_from_session(&ui_session, cx);
        let language = ui_session
            .language
            .as_deref()
            .and_then(crate::i18n::Language::from_key)
            .unwrap_or_default();
        crate::avatar_source::init_from_session(&ui_session);
        let avatar_source = ui_session
            .avatar_source
            .as_deref()
            .and_then(crate::avatar_source::AvatarSource::from_key)
            .unwrap_or_default();
        // Set after the i18n global is seeded so the native title follows the
        // active language.
        window.set_window_title(tr_str("settings.window.title"));
        let date_time_format = ui_session
            .date_time_format
            .as_deref()
            .and_then(DateTimeFormat::from_key)
            .unwrap_or(DateTimeFormat::YmdHm);
        let timezone = ui_session
            .timezone
            .as_deref()
            .and_then(Timezone::from_key)
            .unwrap_or_default();
        let show_timezone = ui_session.show_timezone.unwrap_or(true);
        let change_tracking_view = ui_session
            .change_tracking_view
            .as_deref()
            .and_then(ChangeTrackingView::from_key)
            .unwrap_or_default();
        let terminal_preferences = TerminalPreferences::from_ui_session(&ui_session);
        let diff_scroll_sync = ui_session
            .diff_scroll_sync
            .as_deref()
            .and_then(DiffScrollSync::from_key)
            .unwrap_or_default();
        let diff_content_mode = ui_session
            .diff_content_mode
            .as_deref()
            .and_then(DiffContentMode::from_key)
            .unwrap_or_default();
        let diff_whitespace_mode = ui_session
            .diff_whitespace_mode
            .as_deref()
            .and_then(DiffWhitespaceMode::from_key)
            .unwrap_or_default();
        let diff_view_mode = ui_session
            .diff_view_mode
            .as_deref()
            .and_then(DiffViewMode::from_key)
            .unwrap_or(DiffViewMode::Split);
        let diff_reveal_whitespace_chars = ui_session.diff_reveal_whitespace_chars.unwrap_or(false);
        let diff_word_wrap = ui_session.diff_word_wrap.unwrap_or(false);
        let diff_show_line_numbers = ui_session.diff_show_line_numbers.unwrap_or(true);
        let auto_save_file_edits = ui_session.auto_save_file_edits.unwrap_or(false);
        let history_show_graph = ui_session.history_show_graph.unwrap_or(true);
        let history_show_author = ui_session.history_show_author.unwrap_or(true);
        let history_show_date = ui_session.history_show_date.unwrap_or(true);
        let history_show_sha = ui_session.history_show_sha.unwrap_or(false);
        let history_relative_dates = ui_session.history_relative_dates.unwrap_or(true);
        let history_highlight_commit_chain =
            ui_session.history_highlight_commit_chain.unwrap_or(true);
        let history_show_tags = ui_session.history_show_tags.unwrap_or(true);
        let history_tag_fetch_mode = ui_session.history_tag_fetch_mode.unwrap_or_default();
        let default_history_mode = ui_session.default_history_mode.unwrap_or_default();
        let default_tag_type = ui_session.default_tag_type.unwrap_or_default();
        let external_editor_setting = initial_external_editor_setting(&ui_session);
        let external_editor_options: Arc<[crate::external_editor::ExternalEditorOption]> =
            crate::external_editor::external_editor_options(external_editor_setting.as_ref())
                .into();
        let (external_editor_custom_path_draft, external_editor_custom_arguments_draft) =
            match &external_editor_setting {
                Some(ExternalCodeEditorSetting::Custom {
                    executable,
                    arguments,
                }) => (
                    executable.display().to_string(),
                    arguments.clone().unwrap_or_default(),
                ),
                _ => (String::new(), String::new()),
            };
        // The AI settings are a process global the ✨ button reads; the window
        // drafts from that current value. Construction never installs the
        // global — app startup and the settings save own installation — so
        // every test that opens this window stays side-effect free instead of
        // clobbering a parallel generation test's arranged settings.
        let ai_commit_current = crate::ai_commit::current();
        let ai_commit_source = ai_commit_current.source;
        let ai_commit_provider = ai_commit_current.provider;
        let ai_commit_model_draft = ai_commit_current.model;
        let ai_commit_api_key_draft = ai_commit_current.api_key;
        let ai_commit_endpoint_draft = ai_commit_current.endpoint;
        let ai_commit_custom_command_draft = ai_commit_current.custom_command;
        // Seeded from the session file only — construction never installs the
        // process global so tests stay side-effect free; the setter and app
        // startup own installation.
        let merge_tool_selection = ui_session.external_merge_tool.clone().unwrap_or_default();
        // The executable-path input starts from the persisted manual path; the
        // informational PATH echo fills in when the availability check lands.
        let merge_tool_executable_path_draft = match &merge_tool_selection {
            ExternalMergeToolSelection::Builtin { path: Some(p), .. } => p.clone(),
            _ => String::new(),
        };
        let merge_tool_custom_command_draft = match &merge_tool_selection {
            ExternalMergeToolSelection::Custom { command, .. } => command.clone(),
            _ => String::new(),
        };
        let theme = theme_mode.resolve_theme(window.appearance());
        let runtime_info = SettingsRuntimeInfo::detect();
        let git_executable_mode =
            GitExecutableMode::from_preference(&runtime_info.git.runtime.preference);
        let git_custom_path_draft = match &runtime_info.git.runtime.preference {
            GitExecutablePreference::Custom(path) if !path.as_os_str().is_empty() => {
                path.display().to_string()
            }
            _ => String::new(),
        };

        let appearance_subscription = {
            let view = cx.weak_entity();
            let mut first = true;
            window.observe_window_appearance(move |window, app| {
                if first {
                    first = false;
                    return;
                }

                let _ = view.update(app, |this, cx| {
                    if !this.theme_mode.is_automatic() {
                        return;
                    }
                    this.theme = this.theme_mode.resolve_theme(window.appearance());
                    cx.notify();
                });
            })
        };

        let terminal_external_program_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "wezterm".into(),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(terminal_preferences.external_terminal_program.clone(), cx);
            input
        });

        let terminal_external_args_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.terminal.args_placeholder"),
                    multiline: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_line_height(Some(px(20.0)), cx);
            input.set_text(terminal_preferences.external_args_multiline(), cx);
            input
        });

        let git_executable_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "/path/to/git".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        git_executable_input.update(cx, |input, cx| {
            input.set_text(git_custom_path_draft.clone(), cx);
        });
        let git_executable_input_subscription =
            cx.observe(&git_executable_input, |this, input, cx| {
                let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
                let next = input.read(cx).text().to_string();
                if this.git_custom_path_draft != next {
                    this.git_custom_path_draft = next;
                    cx.notify();
                }
                if enter_pressed && this.git_executable_mode == GitExecutableMode::Custom {
                    this.apply_git_executable_settings(cx);
                }
            });

        let gpg_config = GpgConfig::read_from_git();
        let gpg_signing_key_draft = gpg_config.user_signing_key.clone();
        let gpg_program_draft = gpg_config.gpg_program.clone();

        let gpg_signing_key_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.gpg_signing.signing_key_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        gpg_signing_key_input.update(cx, |input, cx| {
            input.set_text(gpg_signing_key_draft.clone(), cx);
        });
        let gpg_signing_key_input_subscription =
            cx.observe(&gpg_signing_key_input, |this, input, cx| {
                let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
                let next = input.read(cx).text().to_string();
                if this.gpg_signing_key_draft != next {
                    this.gpg_signing_key_draft = next;
                    cx.notify();
                }
                if enter_pressed {
                    this.apply_gpg_signing_key(cx);
                }
            });

        let gpg_program_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.gpg_signing.program_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        gpg_program_input.update(cx, |input, cx| {
            input.set_text(gpg_program_draft.clone(), cx);
        });
        let gpg_program_input_subscription = cx.observe(&gpg_program_input, |this, input, cx| {
            let enter_pressed = input.update(cx, |input, _| input.take_enter_pressed());
            let next = input.read(cx).text().to_string();
            if this.gpg_program_draft != next {
                this.gpg_program_draft = next;
                cx.notify();
            }
            if enter_pressed {
                this.apply_gpg_program(cx);
            }
        });

        let external_editor_custom_path_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "/path/to/editor".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        external_editor_custom_path_input.update(cx, |input, cx| {
            input.set_text(external_editor_custom_path_draft.clone(), cx);
        });
        let external_editor_custom_arguments_input = cx.new(|cx| {
            components::TextInput::new(
                components::TextInputOptions {
                    placeholder: "--reuse-window {path}".into(),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        external_editor_custom_arguments_input.update(cx, |input, cx| {
            input.set_text(external_editor_custom_arguments_draft.clone(), cx);
        });
        let external_editor_custom_path_input_subscription =
            cx.observe(&external_editor_custom_path_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.external_editor_custom_path_draft == next {
                    return;
                }
                this.external_editor_custom_path_draft = next;
                if this.external_editor_is_custom() {
                    this.persist_external_editor_from_custom_drafts(cx);
                }
                cx.notify();
            });
        let search_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.search.placeholder"),
                    leading_icon: Some("icons/zoom.svg"),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input
        });
        let search_input_subscription = cx.observe(&search_input, |this, input, cx| {
            let next = input.read(cx).text().to_string();
            if this.search_query == next {
                return;
            }
            this.search_query = next;
            // Keep the visible page in the filtered set: if the current
            // category no longer matches, jump to the first one that does.
            if !this.selected_category.matches_query(&this.search_query)
                && let Some(first) = SettingsCategory::ALL
                    .iter()
                    .copied()
                    .find(|category| category.matches_query(&this.search_query))
            {
                this.selected_category = first;
                this.expanded_section = None;
            }
            cx.notify();
        });

        let external_editor_custom_arguments_input_subscription = cx.observe(
            &external_editor_custom_arguments_input,
            |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.external_editor_custom_arguments_draft == next {
                    return;
                }
                this.external_editor_custom_arguments_draft = next;
                if this.external_editor_is_custom() {
                    this.persist_external_editor_from_custom_drafts(cx);
                }
                cx.notify();
            },
        );

        let ai_commit_custom_command_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.ai_commit.custom_command_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(ai_commit_custom_command_draft.clone(), cx);
            input
        });
        let ai_commit_custom_command_input_subscription =
            cx.observe(&ai_commit_custom_command_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.ai_commit_custom_command_draft == next {
                    return;
                }
                this.ai_commit_custom_command_draft = next;
                this.persist_ai_commit_settings(cx);
                this.refresh_ai_commit_availability(cx);
                cx.notify();
            });

        let merge_tool_custom_command_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.merge_tool.custom_command_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(merge_tool_custom_command_draft.clone(), cx);
            input
        });
        let merge_tool_custom_command_input_subscription =
            cx.observe(&merge_tool_custom_command_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.merge_tool_custom_command_draft == next {
                    return;
                }
                this.merge_tool_custom_command_draft = next.clone();
                // The typed text only becomes live once Custom is selected;
                // otherwise it is just a draft remembered for the switch.
                let mut selection_changed = false;
                if let ExternalMergeToolSelection::Custom { command, .. } =
                    &mut this.merge_tool_selection
                {
                    *command = next;
                    selection_changed = true;
                }
                if selection_changed {
                    this.persist_merge_tool_preference(cx);
                }
                cx.notify();
            });

        let merge_tool_executable_path_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.merge_tool.executable_path_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(merge_tool_executable_path_draft.clone(), cx);
            input
        });
        let merge_tool_executable_path_input_subscription =
            cx.observe(&merge_tool_executable_path_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.merge_tool_executable_path_draft == next {
                    return;
                }
                this.merge_tool_executable_path_draft = next.clone();
                // The typed text is the manual executable path of the selected
                // built-in tool; empty text clears it back to PATH resolution.
                let trimmed = next.trim().to_string();
                this.set_merge_tool_manual_path((!trimmed.is_empty()).then_some(trimmed), cx);
            });

        let ai_commit_model_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: ai_commit_provider.default_model().into(),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(ai_commit_model_draft.clone(), cx);
            input
        });
        let ai_commit_model_input_subscription =
            cx.observe(&ai_commit_model_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.ai_commit_model_draft == next {
                    return;
                }
                this.ai_commit_model_draft = next;
                this.persist_ai_commit_settings(cx);
                cx.notify();
            });
        let ai_commit_api_key_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: tr("settings.ai_commit.api_key_placeholder"),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(ai_commit_api_key_draft.clone(), cx);
            input
        });
        let ai_commit_api_key_input_subscription =
            cx.observe(&ai_commit_api_key_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.ai_commit_api_key_draft == next {
                    return;
                }
                this.ai_commit_api_key_draft = next;
                this.persist_ai_commit_settings(cx);
                cx.notify();
            });
        let ai_commit_endpoint_input = cx.new(|cx| {
            let mut input = components::TextInput::new(
                components::TextInputOptions {
                    placeholder: ai_commit_provider.default_endpoint().into(),
                    ..Default::default()
                },
                window,
                cx,
            );
            input.set_theme(theme, cx);
            input.set_text(ai_commit_endpoint_draft.clone(), cx);
            input
        });
        let ai_commit_endpoint_input_subscription =
            cx.observe(&ai_commit_endpoint_input, |this, input, cx| {
                let next = input.read(cx).text().to_string();
                if this.ai_commit_endpoint_draft == next {
                    return;
                }
                this.ai_commit_endpoint_draft = next;
                this.persist_ai_commit_settings(cx);
                cx.notify();
            });

        // The system font scan may still be running; the dropdowns start on
        // the bundled fallback and refill themselves once the scan publishes
        // the real catalog.
        if !crate::font_preferences::system_font_catalog_ready() {
            cx.spawn(async move |this, cx| {
                let ready = cx
                    .background_spawn(async move {
                        crate::font_preferences::wait_for_system_font_catalog(
                            std::time::Duration::from_secs(10),
                        )
                    })
                    .await;
                if ready {
                    let _ = this.update(cx, |this, cx| {
                        this.refresh_font_options(cx);
                    });
                }
            })
            .detach();
        }

        // Editor detection sweeps every PATH directory — seconds on Windows
        // machines under antivirus scanners — so construction only reads the
        // cache and a background pass refills the list when it is stale.
        if !crate::external_editor::detection_cache_fresh() {
            cx.spawn(async move |this, cx| {
                let detected = cx
                    .background_spawn(async move {
                        crate::external_editor::ensure_external_editors_detected()
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.refresh_external_editor_options(detected, cx);
                });
            })
            .detach();
        }

        Self {
            theme_mode,
            theme,
            language,
            avatar_source,
            ui_scale_percent: ui_scale.percent,
            ui_density: crate::density::current(cx).density,
            ui_font_family: font_preferences.ui_font_family,
            editor_font_family: font_preferences.editor_font_family,
            use_font_ligatures: font_preferences.use_font_ligatures,
            ui_font_options: crate::font_preferences::ui_font_options(),
            editor_font_options: crate::font_preferences::editor_font_options(),
            external_editor_options,
            settings_window_scroll: ScrollHandle::default(),
            theme_scroll: UniformListScrollHandle::default(),
            language_scroll: UniformListScrollHandle::default(),
            avatar_source_scroll: UniformListScrollHandle::default(),
            ui_font_scroll: UniformListScrollHandle::default(),
            editor_font_scroll: UniformListScrollHandle::default(),
            external_editor_scroll: UniformListScrollHandle::default(),
            date_format_scroll: UniformListScrollHandle::default(),
            timezone_scroll: UniformListScrollHandle::default(),
            change_tracking_scroll: UniformListScrollHandle::default(),
            diff_content_mode_scroll: UniformListScrollHandle::default(),
            diff_scroll_sync_scroll: UniformListScrollHandle::default(),
            diff_view_mode_scroll: UniformListScrollHandle::default(),
            date_time_format,
            timezone,
            show_timezone,
            change_tracking_view,
            terminal_preferences,
            terminal_external_program_input,
            terminal_external_args_input,
            terminal_status: None,
            diff_content_mode,
            diff_whitespace_mode,
            diff_view_mode,
            diff_reveal_whitespace_chars,
            diff_word_wrap,
            diff_show_line_numbers,
            auto_save_file_edits,
            diff_scroll_sync,
            history_show_graph,
            history_show_author,
            history_show_date,
            history_show_sha,
            history_relative_dates,
            history_highlight_commit_chain,
            history_show_tags,
            history_tag_fetch_mode,
            default_history_mode,
            default_tag_type,
            current_view: SettingsView::Root,
            selected_category: SettingsCategory::General,
            search_query: String::new(),
            search_input,
            nav_scroll: ScrollHandle::default(),
            open_source_licenses_scroll: UniformListScrollHandle::default(),
            runtime_info,
            git_executable_mode,
            git_custom_path_draft,
            git_executable_input,
            gpg_config,
            gpg_signing_key_draft,
            gpg_program_draft,
            gpg_signing_key_input,
            gpg_program_input,
            gpg_save_error: None,
            external_editor_setting,
            external_editor_custom_path_draft,
            external_editor_custom_arguments_draft,
            external_editor_custom_path_input,
            external_editor_custom_arguments_input,
            ai_commit_source,
            ai_commit_source_scroll: UniformListScrollHandle::default(),
            ai_commit_availability: None,
            ai_commit_custom_command_draft,
            ai_commit_custom_command_input,
            ai_commit_provider,
            ai_commit_provider_scroll: UniformListScrollHandle::default(),
            ai_commit_models: AiCommitModels::NotFetched,
            ai_commit_models_scroll: UniformListScrollHandle::default(),
            ai_commit_model_draft,
            ai_commit_api_key_draft,
            ai_commit_endpoint_draft,
            ai_commit_model_input,
            ai_commit_api_key_input,
            ai_commit_endpoint_input,
            merge_tool_selection,
            merge_tool_scroll: UniformListScrollHandle::default(),
            merge_tool_custom_command_draft,
            merge_tool_custom_command_input,
            merge_tool_executable_path_draft,
            merge_tool_executable_path_input,
            merge_tool_path_generation: 0,
            merge_tool_availability: None,
            expanded_section: None,
            hover_resize_edge: None,
            title_drag_state: chrome::TitleBarDragState::default(),
            _git_executable_input_subscription: git_executable_input_subscription,
            _gpg_signing_key_input_subscription: gpg_signing_key_input_subscription,
            _gpg_program_input_subscription: gpg_program_input_subscription,
            _external_editor_custom_path_input_subscription:
                external_editor_custom_path_input_subscription,
            _external_editor_custom_arguments_input_subscription:
                external_editor_custom_arguments_input_subscription,
            _ai_commit_model_input_subscription: ai_commit_model_input_subscription,
            _ai_commit_custom_command_input_subscription:
                ai_commit_custom_command_input_subscription,
            _ai_commit_api_key_input_subscription: ai_commit_api_key_input_subscription,
            _ai_commit_endpoint_input_subscription: ai_commit_endpoint_input_subscription,
            _merge_tool_custom_command_input_subscription:
                merge_tool_custom_command_input_subscription,
            _merge_tool_executable_path_input_subscription:
                merge_tool_executable_path_input_subscription,
            _appearance_subscription: appearance_subscription,
            _search_input_subscription: search_input_subscription,
            #[cfg(test)]
            overflow_probe: false,
            #[cfg(test)]
            external_editor_browse_notify_count: 0,
            #[cfg(test)]
            ai_commit_models_test_fetches: 0,
            #[cfg(test)]
            gpg_config_test_writes: Vec::new(),
        }
    }

    fn persist_preferences(&self, cx: &mut gpui::Context<Self>) {
        let settings = self.preference_settings();

        cx.background_spawn(async move {
            let _ = session::persist_ui_settings(settings);
        })
        .detach();
    }

    fn preference_settings(&self) -> session::UiSettings {
        let ai_commit = crate::ai_commit::current();
        let mut settings = session::UiSettings {
            repo_picker_sort: None,
            repo_picker_collapsed_sections: None,
            window_width: None,
            window_height: None,
            sidebar_width: None,
            details_width: None,
            sidebar_collapsed: None,
            repo_sidebar_collapsed_items: None,
            repo_sidebar_pinned_branches: None,
            theme_mode: Some(self.theme_mode.key().to_string()),
            language: Some(self.language.key().to_string()),
            avatar_source: Some(crate::avatar_source::current().key().to_string()),
            ai_commit_source: Some(ai_commit.source.key().to_string()),
            ai_commit_custom_command: Some(ai_commit.custom_command),
            ai_commit_provider: Some(ai_commit.provider.key().to_string()),
            ai_commit_api_key: Some(ai_commit.api_key),
            ai_commit_model: Some(ai_commit.model),
            ai_commit_endpoint: Some(ai_commit.endpoint),
            ui_scale_percent: Some(self.ui_scale_percent),
            ui_density: Some(self.ui_density.key().to_string()),
            ui_font_family: Some(self.ui_font_family.clone()),
            editor_font_family: Some(self.editor_font_family.clone()),
            use_font_ligatures: Some(self.use_font_ligatures),
            date_time_format: Some(self.date_time_format.key().to_string()),
            timezone: Some(self.timezone.key()),
            show_timezone: Some(self.show_timezone),
            change_tracking_view: Some(self.change_tracking_view.key().to_string()),
            diff_scroll_sync: Some(self.diff_scroll_sync.key().to_string()),
            diff_content_mode: Some(self.diff_content_mode.key().to_string()),
            diff_whitespace_mode: Some(self.diff_whitespace_mode.key().to_string()),
            diff_view_mode: Some(self.diff_view_mode.key().to_string()),
            // Annotate is toggled from the diff toolbar, not the settings window,
            // so leave it untouched here (None never overwrites the stored value).
            annotate_enabled: None,
            diff_reveal_whitespace_chars: Some(self.diff_reveal_whitespace_chars),
            diff_word_wrap: Some(self.diff_word_wrap),
            diff_show_line_numbers: Some(self.diff_show_line_numbers),
            auto_save_file_edits: Some(self.auto_save_file_edits),
            // Merge tool settings are managed from the resolver's cog menu;
            // None never overwrites the stored values.
            mergetool_auto_advance: None,
            mergetool_collapse_unchanged: None,
            mergetool_output_scroll_sync: None,
            mergetool_show_line_numbers: None,
            mergetool_view_three_way: None,
            change_tracking_height: None,
            untracked_height: None,
            history_show_graph: Some(self.history_show_graph),
            history_show_author: Some(self.history_show_author),
            history_show_date: Some(self.history_show_date),
            history_show_sha: Some(self.history_show_sha),
            history_relative_dates: Some(self.history_relative_dates),
            history_highlight_commit_chain: Some(self.history_highlight_commit_chain),
            history_show_tags: Some(self.history_show_tags),
            history_tag_fetch_mode: Some(self.history_tag_fetch_mode),
            default_history_mode: Some(self.default_history_mode),
            default_tag_type: Some(self.default_tag_type),
            commit_push_after_enabled: None,
            push_pull_retry_enabled: None,
            git_executable_path: Some(applied_git_executable_path(&self.runtime_info.git.runtime)),
            terminal_external_mode: None,
            terminal_external_program: None,
            terminal_external_args: None,
            terminal_action_bar_target: None,
            external_code_editor: None,
            external_merge_tool: Some(self.merge_tool_selection.clone()),
        };
        self.terminal_preferences
            .apply_to_ui_settings(&mut settings);
        settings
    }

    fn update_main_windows(
        &self,
        cx: &mut gpui::Context<Self>,
        f: impl FnMut(&mut WorkTreeView, &mut Window, &mut gpui::Context<WorkTreeView>) + 'static,
    ) {
        let handles: Vec<_> = cx
            .windows()
            .into_iter()
            .filter_map(|window| window.downcast::<WorkTreeView>())
            .collect();
        cx.spawn(
            async move |_view: WeakEntity<Self>, cx: &mut gpui::AsyncApp| {
                cx.update(move |cx| {
                    let mut f = f;
                    for handle in handles {
                        let _ = handle.update(cx, |view, window, cx| f(view, window, cx));
                    }
                });
            },
        )
        .detach();
    }
}

impl Render for SettingsWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        self.terminal_external_program_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.terminal_external_args_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        let decorations = window.window_decorations();
        let show_custom_window_chrome =
            crate::linux_gui_env::LinuxGuiEnvironment::should_render_custom_window_chrome(
                decorations,
            );
        let (tiling, client_inset) = match decorations {
            Decorations::Client { tiling } => (
                Some(tiling),
                settings_window_client_inset_for_scale(self.ui_scale_percent),
            ),
            Decorations::Server => (None, px(0.0)),
        };
        window.set_client_inset(client_inset);

        let cursor = self
            .hover_resize_edge
            .map(chrome::cursor_style_for_resize_edge)
            .unwrap_or(CursorStyle::Arrow);
        let is_macos = cfg!(target_os = "macos");
        let header_bg = if window.is_window_active() {
            with_alpha(
                theme.colors.surface.panel,
                if theme.is_dark { 0.98 } else { 0.94 },
            )
        } else {
            theme.colors.surface.panel
        };
        let header_border = if window.is_window_active() {
            theme.colors.stroke.default
        } else {
            with_alpha(theme.colors.stroke.default, 0.7)
        };

        let drag_region = div()
            .id("settings_window_header_drag")
            .debug_selector(|| "settings_window_header_drag".to_string())
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .min_w(px(0.0))
            .px(px(12.0))
            .window_control_area(WindowControlArea::Drag)
            .when(is_macos, |this| {
                this.pl(settings_window_traffic_lights_safe_inset(
                    self.ui_scale_percent,
                ))
            })
            .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                if !chrome::should_handle_titlebar_double_click(e.click_count(), e.standard_click())
                {
                    return;
                }

                this.title_drag_state.clear();
                cx.stop_propagation();
                chrome::handle_titlebar_double_click(window);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|_this, e: &MouseUpEvent, window, cx| {
                    chrome::show_titlebar_secondary_menu(e.position, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _window, cx| {
                    this.title_drag_state.on_left_mouse_down(e.click_count);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e, _window, cx| {
                    this.title_drag_state.clear();
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _e, _window, cx| {
                    this.title_drag_state.clear();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, _e, window, _cx| {
                if this.title_drag_state.take_move_request() {
                    crate::app::begin_window_move(window);
                }
            }))
            .child(
                div()
                    .overflow_hidden()
                    .text_size(px(13.0))
                    .line_height(px(16.0))
                    .font_weight(FontWeight::BOLD)
                    .whitespace_nowrap()
                    .child(tr_str("settings.window.title")),
            );

        let min = chrome::titlebar_control_button(
            self.ui_scale_percent,
            "settings_window_min_btn",
            "icons/generic_minimize.svg",
            theme.colors.foreground.secondary,
            theme.colors.foreground.primary,
        )
        .id("settings_window_min")
        .debug_selector(|| "settings_window_min".to_string())
        .window_control_area(WindowControlArea::Min)
        .on_click(cx.listener(|_this, _e: &ClickEvent, window, cx| {
            cx.stop_propagation();
            window.minimize_window();
        }));

        let max_icon = if window.is_maximized() {
            "icons/generic_restore.svg"
        } else {
            "icons/generic_maximize.svg"
        };
        let max = chrome::titlebar_control_button(
            self.ui_scale_percent,
            "settings_window_max_btn",
            max_icon,
            theme.colors.foreground.secondary,
            theme.colors.foreground.primary,
        )
        .id("settings_window_max")
        .debug_selector(|| "settings_window_max".to_string())
        .window_control_area(WindowControlArea::Max)
        .on_click(cx.listener(|_this, _e: &ClickEvent, window, cx| {
            cx.stop_propagation();
            crate::app::toggle_window_zoom(window);
            cx.notify();
        }));

        let close = chrome::titlebar_control_button(
            self.ui_scale_percent,
            "settings_window_close_btn",
            "icons/generic_close.svg",
            theme.colors.foreground.secondary,
            theme.colors.status.danger.foreground,
        )
        .id("settings_window_close_btn")
        .debug_selector(|| "settings_window_close".to_string())
        .window_control_area(WindowControlArea::Close)
        .on_click(cx.listener(|_this, _e: &ClickEvent, window, cx| {
            cx.stop_propagation();
            crate::app::mark_clean_shutdown_if_last_window_from_view(cx);
            window.remove_window();
        }));

        let frame_rounding = chrome::client_frame_corner_rounding(theme, window);
        let header = div()
            .id("settings_window_header")
            .h(chrome::title_bar_height(self.ui_scale_percent))
            .w_full()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(header_border)
            .bg(header_bg)
            .when_some(
                chrome::client_frame_corner_rounding(theme, window),
                |d, rounding| {
                    d.when(rounding.top_left, |d| d.rounded_tl(rounding.radius))
                        .when(rounding.top_right, |d| d.rounded_tr(rounding.radius))
                },
            )
            .child(drag_region)
            .when(!is_macos, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pr_2()
                        .child(min)
                        .child(max)
                        .child(close),
                )
            });

        self.git_executable_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.external_editor_custom_path_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.external_editor_custom_arguments_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.ai_commit_model_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.ai_commit_api_key_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.ai_commit_endpoint_input
            .update(cx, |input, cx| input.set_theme(theme, cx));
        self.search_input
            .update(cx, |input, cx| input.set_theme(theme, cx));

        #[cfg(test)]
        let show_overflow_probe =
            self.overflow_probe && matches!(self.current_view, SettingsView::Root);
        #[cfg(not(test))]
        let show_overflow_probe = false;

        let content = if show_overflow_probe {
            self.overflow_probe_content(theme).into_any_element()
        } else {
            match self.current_view {
                SettingsView::Root => {
                    let no_separator = gpui::rgba(0x00000000);
                    let general_card = self.general_card(theme, no_separator, cx);
                    let terminal_card = self.terminal_card(theme, no_separator, cx);
                    let change_tracking_card = self.change_tracking_card(theme, no_separator, cx);
                    let diff_card = self.diff_card(theme, no_separator, cx);
                    let file_editing_card = self.file_editing_card(theme, no_separator, cx);
                    let git_log_card = self.git_log_card(theme, no_separator, cx);
                    let tags_card = self.tags_card(theme, no_separator, cx);
                    let git_executable_card = self.git_executable_card(theme, cx);
                    let gpg_signing_card = self.gpg_signing_card(theme, cx);
                    let merge_tool_card = self.merge_tool_card(theme, cx);
                    let environment_card = self.environment_card(theme, no_separator, cx);
                    let links_card = self.links_card(theme, no_separator, cx);
                    let storage_card = self.storage_card(theme, no_separator, cx);
                    // The visible page follows the selected nav category.
                    // Expanding a row can only happen from within its owning
                    // category, so deriving from an expanded section keeps the
                    // page and the expanded row consistent.
                    let active_category = self
                        .expanded_section
                        .map(SettingsSection::category)
                        .unwrap_or(self.selected_category);

                    let active_card = match active_category {
                        SettingsCategory::General => general_card,
                        SettingsCategory::Terminal => terminal_card,
                        SettingsCategory::ChangeTracking => change_tracking_card,
                        SettingsCategory::Diff => diff_card,
                        SettingsCategory::FileEditing => file_editing_card,
                        SettingsCategory::GitLog => git_log_card,
                        SettingsCategory::Tags => tags_card,
                        SettingsCategory::GitExecutable => git_executable_card,
                        SettingsCategory::GpgSigning => gpg_signing_card,
                        SettingsCategory::MergeTool => merge_tool_card,
                        SettingsCategory::Environment => environment_card,
                        SettingsCategory::Links => links_card,
                        SettingsCategory::Storage => storage_card,
                    };

                    let scroll_surface = restrict_scroll_to_vertical_axis(
                        div()
                            .id("settings_window_scroll")
                            .debug_selector(|| "settings_window_scroll".to_string())
                            .w_full()
                            .h_full()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.settings_window_scroll),
                    )
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .child(active_card);

                    let content_pane = div()
                        .id("settings_window_content_pane")
                        .debug_selector(|| "settings_window_content_pane".to_string())
                        .relative()
                        .flex_1()
                        .h_full()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .bg(theme.colors.surface.canvas)
                        .child(
                            div()
                                .w_full()
                                .flex_1()
                                .h_full()
                                .min_w(px(0.0))
                                .min_h(px(0.0))
                                .pr(components::Scrollbar::visible_gutter(
                                    self.settings_window_scroll.clone(),
                                    components::ScrollbarAxis::Vertical,
                                ))
                                .child(scroll_surface),
                        )
                        .child(
                            {
                                let scrollbar = components::Scrollbar::new(
                                    "settings_window_scrollbar",
                                    self.settings_window_scroll.clone(),
                                )
                                .always_visible();
                                #[cfg(test)]
                                let scrollbar =
                                    scrollbar.debug_selector("settings_window_scrollbar");
                                scrollbar
                            }
                            .render(theme),
                        );

                    div()
                        .id("settings_window_root_view")
                        .debug_selector(|| "settings_window_root_view".to_string())
                        .w_full()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .flex()
                        .flex_row()
                        .child(self.render_settings_nav(active_category, theme, cx))
                        .child(content_pane)
                }
                SettingsView::OpenSourceLicenses => {
                    let breadcrumb = div()
                        .id("settings_window_breadcrumb")
                        .w_full()
                        .px_2()
                        .py_1()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("settings_window_breadcrumb_settings")
                                .debug_selector(|| {
                                    "settings_window_breadcrumb_settings".to_string()
                                })
                                .px_2()
                                .py_1()
                                .rounded(px(theme.radii.row))
                                .cursor(CursorStyle::PointingHand)
                                .hover(move |s| s.bg(theme.colors.interaction.hover_background))
                                .active(move |s| s.bg(theme.colors.interaction.pressed_background))
                                .text_sm()
                                .text_color(theme.colors.accent.foreground)
                                .child(tr_str("settings.licenses.back"))
                                .on_click(cx.listener(|this, _e: &ClickEvent, _window, cx| {
                                    this.show_root(cx);
                                })),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.colors.foreground.secondary)
                                .child("/"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .child(tr_str("settings.links.open_source_licenses")),
                        );

                    let licenses_card = self.licenses_card(theme, cx);

                    div()
                        .id("settings_window_open_source_licenses_view")
                        .w_full()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_3()
                        .child(breadcrumb)
                        .child(licenses_card)
                }
            }
            .into_any_element()
        };

        let body = div()
            .id("settings_window_content")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.surface.canvas)
            .when_some(frame_rounding, |d, rounding| {
                d.when(rounding.bottom_left, |d| d.rounded_bl(rounding.radius))
                    .when(rounding.bottom_right, |d| d.rounded_br(rounding.radius))
            })
            .font(gpui::Font {
                family: crate::font_preferences::applied_ui_font_family(&self.ui_font_family)
                    .into(),
                features: crate::font_preferences::applied_font_features(self.use_font_ligatures),
                fallbacks: None,
                weight: gpui::FontWeight::default(),
                style: gpui::FontStyle::default(),
            })
            .text_color(theme.colors.foreground.primary);

        let body = if show_custom_window_chrome {
            body.child(header).child(content)
        } else {
            body.child(content)
        };

        let mut root = div()
            .size_full()
            .cursor(cursor)
            .text_color(theme.colors.foreground.primary)
            .relative()
            // Any click anywhere hides visible tooltips.
            .capture_any_mouse_down(cx.listener(|_this, _e: &MouseDownEvent, _window, cx| {
                crate::view::tooltip::dismiss_tooltips_on_mouse_down(cx);
            }));

        root = root.on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
            let Decorations::Client { tiling } = window.window_decorations() else {
                if this.hover_resize_edge.is_some() {
                    this.hover_resize_edge = None;
                    cx.notify();
                }
                return;
            };

            let size = window.viewport_size();
            let next = chrome::resize_edge(
                e.position,
                settings_window_client_inset_for_scale(this.ui_scale_percent),
                size,
                tiling,
            );
            if next != this.hover_resize_edge {
                this.hover_resize_edge = next;
                cx.notify();
            }
        }));

        if tiling.is_some() {
            root = root.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, window, cx| {
                    let Decorations::Client { tiling } = window.window_decorations() else {
                        return;
                    };

                    let size = window.viewport_size();
                    let edge = chrome::resize_edge(
                        e.position,
                        settings_window_client_inset_for_scale(this.ui_scale_percent),
                        size,
                        tiling,
                    );
                    let Some(edge) = edge else {
                        return;
                    };

                    cx.stop_propagation();
                    crate::app::begin_window_resize(window, edge);
                }),
            );
        } else {
            self.hover_resize_edge = None;
        }

        root.child(settings_window_frame(
            theme,
            decorations,
            body.into_any_element(),
            self.ui_scale_percent,
        ))
    }
}

#[cfg(test)]
mod tests;
