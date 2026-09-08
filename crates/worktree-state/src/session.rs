use crate::model::{AppState, DefaultTagType, GitLogTagFetchMode, RepoId};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::{env, fs, io};
use worktree_core::domain::{HistoryMode, LogScope};
use worktree_core::external_merge_tool::ExternalMergeToolSelection;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiSession {
    pub open_repos: Vec<PathBuf>,
    pub active_repo: Option<PathBuf>,
    pub recent_repos: Vec<PathBuf>,
    /// Repositories the user pinned in the repository picker, in the order they
    /// were pinned. Independent of `recent_repos`, so a pin outlives the
    /// recents cap.
    pub pinned_repos: Vec<PathBuf>,
    pub repo_picker_sort: Option<String>,
    /// Storage keys of the repository picker sections the user folded away.
    /// Every section defaults to expanded, so this only ever holds deviations.
    pub repo_picker_collapsed_sections: BTreeSet<String>,
    pub repo_sidebar_collapsed_items: BTreeMap<PathBuf, BTreeSet<String>>,
    pub repo_sidebar_pinned_branches: BTreeMap<PathBuf, BTreeSet<String>>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub sidebar_width: Option<u32>,
    pub details_width: Option<u32>,
    pub sidebar_collapsed: Option<bool>,
    pub theme_mode: Option<String>,
    /// UI language key (`i18n::Language::key`); `None` follows the system
    /// locale.
    pub language: Option<String>,
    /// Commit-author avatar source key; `None` means the built-in initials
    /// circles (no network).
    pub avatar_source: Option<String>,
    /// AI commit-message generation: configuration-source key
    /// (`ai_commit_sources::AiSource`). `None` means manual. External-tool
    /// sources resolve credentials live and never persist them — only the
    /// manual provider fields below are stored.
    pub ai_commit_source: Option<String>,
    /// Template for the custom-command source; only read when the source is
    /// `custom`.
    pub ai_commit_custom_command: Option<String>,
    /// AI commit-message generation: provider key (`ai_commit::AiProvider`),
    /// then its credentials. `None`/empty keeps the feature unconfigured.
    pub ai_commit_provider: Option<String>,
    pub ai_commit_api_key: Option<String>,
    pub ai_commit_model: Option<String>,
    pub ai_commit_endpoint: Option<String>,
    pub ui_scale_percent: Option<u32>,
    /// UI density key (`comfortable`/`compact`); `None` follows the
    /// comfortable default. Stored as a string so future tiers load from old
    /// session files without a migration.
    pub ui_density: Option<String>,
    pub ui_font_family: Option<String>,
    pub editor_font_family: Option<String>,
    pub use_font_ligatures: Option<bool>,
    pub date_time_format: Option<String>,
    pub timezone: Option<String>,
    pub show_timezone: Option<bool>,
    pub change_tracking_view: Option<String>,
    pub diff_scroll_sync: Option<String>,
    pub diff_content_mode: Option<String>,
    pub diff_whitespace_mode: Option<String>,
    pub diff_view_mode: Option<String>,
    pub annotate_enabled: Option<bool>,
    pub diff_reveal_whitespace_chars: Option<bool>,
    pub diff_word_wrap: Option<bool>,
    pub diff_show_line_numbers: Option<bool>,
    pub auto_save_file_edits: Option<bool>,
    pub mergetool_auto_advance: Option<bool>,
    pub mergetool_collapse_unchanged: Option<bool>,
    pub mergetool_output_scroll_sync: Option<bool>,
    pub mergetool_show_line_numbers: Option<bool>,
    pub mergetool_view_three_way: Option<bool>,
    pub change_tracking_height: Option<u32>,
    pub untracked_height: Option<u32>,
    pub history_show_graph: Option<bool>,
    pub history_show_author: Option<bool>,
    pub history_show_date: Option<bool>,
    pub history_show_sha: Option<bool>,
    pub terminal_external_mode: Option<String>,
    pub terminal_external_program: Option<String>,
    pub terminal_external_args: Option<Vec<String>>,
    pub terminal_action_bar_target: Option<String>,
    pub history_show_tags: Option<bool>,
    pub history_relative_dates: Option<bool>,
    pub history_highlight_commit_chain: Option<bool>,
    pub history_tag_fetch_mode: Option<GitLogTagFetchMode>,
    pub default_history_mode: Option<HistoryMode>,
    pub commit_push_after_enabled: Option<bool>,
    pub push_pull_retry_enabled: Option<bool>,
    pub default_tag_type: Option<DefaultTagType>,
    pub git_executable_path: Option<PathBuf>,
    pub external_code_editor: Option<ExternalCodeEditorSetting>,
    /// External merge tool for the conflicted-file context menu; `None`/`FromGitConfig`
    /// resolves the tool from git config.
    pub external_merge_tool: Option<ExternalMergeToolSelection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalCodeEditorSetting {
    Detected {
        id: String,
        path: PathBuf,
    },
    Custom {
        executable: PathBuf,
        arguments: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryScopeSetting {
    CurrentBranch,
    AllBranches,
}

impl From<LogScope> for HistoryScopeSetting {
    fn from(value: LogScope) -> Self {
        match value {
            HistoryMode::AllBranches => Self::AllBranches,
            HistoryMode::FullReachable
            | HistoryMode::FirstParent
            | HistoryMode::NoMerges
            | HistoryMode::MergesOnly => Self::CurrentBranch,
        }
    }
}

impl From<HistoryScopeSetting> for LogScope {
    fn from(value: HistoryScopeSetting) -> Self {
        match value {
            HistoryScopeSetting::CurrentBranch => Self::CurrentBranch,
            HistoryScopeSetting::AllBranches => Self::AllBranches,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryModeSetting {
    FullReachable,
    FirstParent,
    NoMerges,
    MergesOnly,
    AllBranches,
}

impl From<HistoryMode> for HistoryModeSetting {
    fn from(value: HistoryMode) -> Self {
        match value {
            HistoryMode::FullReachable => Self::FullReachable,
            HistoryMode::FirstParent => Self::FirstParent,
            HistoryMode::NoMerges => Self::NoMerges,
            HistoryMode::MergesOnly => Self::MergesOnly,
            HistoryMode::AllBranches => Self::AllBranches,
        }
    }
}

impl From<HistoryModeSetting> for HistoryMode {
    fn from(value: HistoryModeSetting) -> Self {
        match value {
            HistoryModeSetting::FullReachable => Self::FullReachable,
            HistoryModeSetting::FirstParent => Self::FirstParent,
            HistoryModeSetting::NoMerges => Self::NoMerges,
            HistoryModeSetting::MergesOnly => Self::MergesOnly,
            HistoryModeSetting::AllBranches => Self::AllBranches,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UiSessionFileV1 {
    pub version: u32,
    pub open_repos: Vec<String>,
    pub active_repo: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UiSessionFile {
    pub version: u32,
    pub open_repos: Vec<String>,
    pub active_repo: Option<String>,
    pub recent_repos: Option<Vec<String>>,
    pub pinned_repos: Option<Vec<String>>,
    pub repo_picker_sort: Option<String>,
    pub repo_picker_collapsed_sections: Option<BTreeSet<String>>,
    pub repo_sidebar_collapsed_items: Option<BTreeMap<String, BTreeSet<String>>>,
    pub repo_sidebar_pinned_branches: Option<BTreeMap<String, BTreeSet<String>>>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub sidebar_width: Option<u32>,
    pub details_width: Option<u32>,
    pub sidebar_collapsed: Option<bool>,
    pub theme_mode: Option<String>,
    pub language: Option<String>,
    pub avatar_source: Option<String>,
    pub ai_commit_source: Option<String>,
    pub ai_commit_custom_command: Option<String>,
    pub ai_commit_provider: Option<String>,
    pub ai_commit_api_key: Option<String>,
    pub ai_commit_model: Option<String>,
    pub ai_commit_endpoint: Option<String>,
    pub ui_scale_percent: Option<u32>,
    pub ui_density: Option<String>,
    pub ui_font_family: Option<String>,
    pub editor_font_family: Option<String>,
    pub use_font_ligatures: Option<bool>,
    pub date_time_format: Option<String>,
    pub timezone: Option<String>,
    pub show_timezone: Option<bool>,
    pub change_tracking_view: Option<String>,
    pub diff_scroll_sync: Option<String>,
    pub diff_content_mode: Option<String>,
    pub diff_whitespace_mode: Option<String>,
    pub diff_view_mode: Option<String>,
    pub annotate_enabled: Option<bool>,
    pub diff_reveal_whitespace_chars: Option<bool>,
    pub diff_word_wrap: Option<bool>,
    pub diff_show_line_numbers: Option<bool>,
    pub auto_save_file_edits: Option<bool>,
    pub mergetool_auto_advance: Option<bool>,
    pub mergetool_collapse_unchanged: Option<bool>,
    pub mergetool_output_scroll_sync: Option<bool>,
    pub mergetool_show_line_numbers: Option<bool>,
    pub mergetool_view_three_way: Option<bool>,
    pub change_tracking_height: Option<u32>,
    pub untracked_height: Option<u32>,
    pub history_show_graph: Option<bool>,
    pub history_show_author: Option<bool>,
    pub history_show_date: Option<bool>,
    pub history_show_sha: Option<bool>,
    pub terminal_external_mode: Option<String>,
    pub terminal_external_program: Option<String>,
    pub terminal_external_args: Option<Vec<String>>,
    pub terminal_action_bar_target: Option<String>,
    pub history_show_tags: Option<bool>,
    pub history_relative_dates: Option<bool>,
    pub history_highlight_commit_chain: Option<bool>,
    pub history_tag_fetch_mode: Option<GitLogTagFetchMode>,
    pub default_history_mode: Option<HistoryModeSetting>,
    pub commit_push_after_enabled: Option<bool>,
    pub push_pull_retry_enabled: Option<bool>,
    pub default_tag_type: Option<DefaultTagType>,
    pub git_executable_path: Option<String>,
    pub external_code_editor: Option<ExternalCodeEditorSettingFile>,
    pub external_merge_tool: Option<ExternalMergeToolSelection>,
    pub repo_history_modes: Option<BTreeMap<String, HistoryModeSetting>>,
    pub repo_history_scopes: Option<BTreeMap<String, HistoryScopeSetting>>,
    pub repo_history_author_filters: Option<BTreeMap<String, Option<String>>>,
    pub repo_history_ref_filters: Option<BTreeMap<String, Vec<String>>>,
    pub repo_fetch_prune_deleted_remote_tracking_branches: Option<BTreeMap<String, bool>>,
    pub survey_prompt: Option<SurveyPromptSession>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExternalCodeEditorSettingFile {
    Detected {
        id: String,
        path: String,
    },
    Custom {
        executable: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SurveyPromptSession {
    pub survey_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_at_unix_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postponed_until_unix_seconds: Option<u64>,
}

pub const SESSION_FILE_VERSION_V1: u32 = 1;
pub const SESSION_FILE_VERSION_V2: u32 = 2;
const SESSION_FILE_VERSION_V3: u32 = 3;
pub const CURRENT_SESSION_FILE_VERSION: u32 = SESSION_FILE_VERSION_V3;
pub const MAX_RECENT_REPOS: usize = 15;
const DEFAULT_UI_SCALE_PERCENT: u32 = 100;
const MIN_UI_SCALE_PERCENT: u32 = 80;
const MAX_UI_SCALE_PERCENT: u32 = 200;
#[cfg(unix)]
pub const SESSION_PATH_BYTES_PREFIX: &str = "worktree-path-bytes:";
#[cfg(windows)]
const SESSION_PATH_WIDE_PREFIX: &str = "worktree-path-utf16le:";

const SESSION_FILE_ENV: &str = "WORKTREE_SESSION_FILE";
const DISABLE_SESSION_PERSIST_ENV: &str = "WORKTREE_DISABLE_SESSION_PERSIST";

pub fn load() -> UiSession {
    let Some(path) = default_session_file_path() else {
        return UiSession::default();
    };

    load_from_path(&path)
}

pub fn load_from_path(path: &Path) -> UiSession {
    let Some(file) = load_file(path) else {
        return UiSession::default();
    };

    let (open_repos, active_repo) = parse_repos(file.open_repos, file.active_repo);
    let recent_repos = parse_path_list(file.recent_repos.unwrap_or_default());
    let pinned_repos = parse_path_list(file.pinned_repos.unwrap_or_default());
    let repo_sidebar_collapsed_items =
        parse_path_keyed_string_sets(file.repo_sidebar_collapsed_items.unwrap_or_default());
    let repo_sidebar_pinned_branches =
        parse_path_keyed_string_sets(file.repo_sidebar_pinned_branches.unwrap_or_default());
    UiSession {
        open_repos,
        active_repo,
        recent_repos,
        pinned_repos,
        repo_picker_sort: file.repo_picker_sort,
        repo_picker_collapsed_sections: file.repo_picker_collapsed_sections.unwrap_or_default(),
        repo_sidebar_collapsed_items,
        repo_sidebar_pinned_branches,
        window_width: file.window_width,
        window_height: file.window_height,
        sidebar_width: file.sidebar_width,
        details_width: file.details_width,
        sidebar_collapsed: file.sidebar_collapsed,
        theme_mode: file.theme_mode,
        language: file.language,
        avatar_source: file.avatar_source,
        ai_commit_source: file.ai_commit_source,
        ai_commit_custom_command: file.ai_commit_custom_command,
        ai_commit_provider: file.ai_commit_provider,
        ai_commit_api_key: file.ai_commit_api_key,
        ai_commit_model: file.ai_commit_model,
        ai_commit_endpoint: file.ai_commit_endpoint,
        ui_scale_percent: file.ui_scale_percent,
        ui_density: file.ui_density,
        ui_font_family: file.ui_font_family,
        editor_font_family: file.editor_font_family,
        use_font_ligatures: file.use_font_ligatures,
        date_time_format: file.date_time_format,
        timezone: file.timezone,
        show_timezone: file.show_timezone,
        change_tracking_view: file.change_tracking_view,
        diff_scroll_sync: file.diff_scroll_sync,
        diff_content_mode: file.diff_content_mode,
        diff_whitespace_mode: file.diff_whitespace_mode,
        diff_view_mode: file.diff_view_mode,
        annotate_enabled: file.annotate_enabled,
        diff_reveal_whitespace_chars: file.diff_reveal_whitespace_chars,
        diff_word_wrap: file.diff_word_wrap,
        diff_show_line_numbers: file.diff_show_line_numbers,
        auto_save_file_edits: file.auto_save_file_edits,
        mergetool_auto_advance: file.mergetool_auto_advance,
        mergetool_collapse_unchanged: file.mergetool_collapse_unchanged,
        mergetool_output_scroll_sync: file.mergetool_output_scroll_sync,
        mergetool_show_line_numbers: file.mergetool_show_line_numbers,
        mergetool_view_three_way: file.mergetool_view_three_way,
        change_tracking_height: file.change_tracking_height,
        untracked_height: file.untracked_height,
        history_show_graph: file.history_show_graph,
        history_show_author: file.history_show_author,
        history_show_date: file.history_show_date,
        history_show_sha: file.history_show_sha,
        terminal_external_mode: file.terminal_external_mode,
        terminal_external_program: file.terminal_external_program,
        terminal_external_args: file.terminal_external_args,
        terminal_action_bar_target: file.terminal_action_bar_target,
        history_show_tags: file.history_show_tags,
        history_relative_dates: file.history_relative_dates,
        history_highlight_commit_chain: file.history_highlight_commit_chain,
        history_tag_fetch_mode: file.history_tag_fetch_mode,
        default_history_mode: file.default_history_mode.map(Into::into),
        commit_push_after_enabled: file.commit_push_after_enabled,
        push_pull_retry_enabled: file.push_pull_retry_enabled,
        default_tag_type: file.default_tag_type,
        git_executable_path: file
            .git_executable_path
            .as_deref()
            .map(path_from_storage_key),
        external_code_editor: external_code_editor_from_file(file.external_code_editor),
        external_merge_tool: file.external_merge_tool,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepoSessionPreferences {
    pub default_history_mode: Option<HistoryMode>,
    pub repo_history_modes: BTreeMap<String, HistoryMode>,
    pub repo_history_scopes: BTreeMap<String, LogScope>,
    pub repo_history_author_filters: BTreeMap<String, Option<String>>,
    pub repo_history_ref_filters: BTreeMap<String, Vec<String>>,
    pub repo_fetch_prune_deleted_remote_tracking_branches: BTreeMap<String, bool>,
}

pub(crate) fn load_repo_session_preferences() -> RepoSessionPreferences {
    let Some(session_file_path) = default_session_file_path() else {
        return RepoSessionPreferences::default();
    };
    load_repo_session_preferences_from_path(&session_file_path)
}

pub fn load_repo_session_preferences_from_path(session_file_path: &Path) -> RepoSessionPreferences {
    let Some(file) = load_file(session_file_path) else {
        return RepoSessionPreferences::default();
    };

    RepoSessionPreferences {
        default_history_mode: file.default_history_mode.map(Into::into),
        repo_history_modes: file
            .repo_history_modes
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect(),
        repo_history_scopes: file
            .repo_history_scopes
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect(),
        repo_history_author_filters: file.repo_history_author_filters.unwrap_or_default(),
        repo_history_ref_filters: file.repo_history_ref_filters.unwrap_or_default(),
        repo_fetch_prune_deleted_remote_tracking_branches: file
            .repo_fetch_prune_deleted_remote_tracking_branches
            .unwrap_or_default(),
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionReposSnapshot {
    pub open_repos: Arc<[Arc<str>]>,
    pub active_repo_index: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct CachedSessionReposSnapshot {
    repo_ids: SmallVec<[RepoId; 24]>,
    repo_keys: SmallVec<[Arc<str>; 24]>,
    dedup_indexes_by_repo: SmallVec<[usize; 24]>,
    open_repos: Arc<[Arc<str>]>,
}

thread_local! {
    pub static SESSION_REPOS_SNAPSHOT_CACHE: RefCell<Option<CachedSessionReposSnapshot>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    static TEST_SESSION_FILE_PATH_OVERRIDE: RefCell<Vec<Option<PathBuf>>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) struct TestSessionFilePathGuard;

#[cfg(test)]
pub(crate) fn push_test_session_file_path_override(
    path: impl Into<Option<PathBuf>>,
) -> TestSessionFilePathGuard {
    TEST_SESSION_FILE_PATH_OVERRIDE.with(|stack| stack.borrow_mut().push(path.into()));
    TestSessionFilePathGuard
}

#[cfg(test)]
impl Drop for TestSessionFilePathGuard {
    fn drop(&mut self) {
        TEST_SESSION_FILE_PATH_OVERRIDE.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert!(popped.is_some(), "session path override stack underflow");
        });
    }
}

#[cfg(test)]
fn test_session_file_path_override() -> Option<Option<PathBuf>> {
    TEST_SESSION_FILE_PATH_OVERRIDE.with(|stack| stack.borrow().last().cloned())
}

fn snapshot_repos_from_cache(state: &AppState) -> Option<SessionReposSnapshot> {
    SESSION_REPOS_SNAPSHOT_CACHE.with(|cache| {
        let cache = cache.borrow();
        let cached = cache.as_ref()?;
        if cached.repo_ids.len() != state.repos.len() {
            return None;
        }

        let mut active_repo_index = None;
        for (repo_ix, repo) in state.repos.iter().enumerate() {
            if cached.repo_ids[repo_ix] != repo.id
                || !Arc::ptr_eq(&cached.repo_keys[repo_ix], repo.session_workdir_key())
            {
                return None;
            }
            if active_repo_index.is_none() && Some(repo.id) == state.active_repo {
                active_repo_index = Some(cached.dedup_indexes_by_repo[repo_ix]);
            }
        }

        Some(SessionReposSnapshot {
            open_repos: Arc::clone(&cached.open_repos),
            active_repo_index,
        })
    })
}

pub fn snapshot_repos_from_state(state: &AppState) -> SessionReposSnapshot {
    if let Some(snapshot) = snapshot_repos_from_cache(state) {
        return snapshot;
    }

    // Repo switches rarely change the open-tab order, so cache the last exact repo sequence and
    // reuse its dedup map on steady-state switches. When the sequence changes, rebuild once with
    // a linear scan over the small user-scale repo list.
    let mut repo_ids = SmallVec::<[RepoId; 24]>::with_capacity(state.repos.len());
    let mut repo_keys = SmallVec::<[Arc<str>; 24]>::with_capacity(state.repos.len());
    let mut unique_keys = SmallVec::<[Arc<str>; 24]>::new();
    let mut dedup_indexes_by_repo = SmallVec::<[usize; 24]>::with_capacity(state.repos.len());
    let active_repo_id = state.active_repo;
    let mut active_repo_index = None;

    for repo in &state.repos {
        repo_ids.push(repo.id);
        let key = repo.session_workdir_key();
        repo_keys.push(Arc::clone(key));

        let unique_ix = if let Some(ix) = unique_keys
            .iter()
            .position(|seen| seen.as_ref() == key.as_ref())
        {
            ix
        } else {
            unique_keys.push(Arc::clone(key));
            unique_keys.len() - 1
        };
        dedup_indexes_by_repo.push(unique_ix);
        if active_repo_index.is_none() && Some(repo.id) == active_repo_id {
            active_repo_index = Some(unique_ix);
        }
    }

    let open_repos: Arc<[Arc<str>]> = unique_keys.into_vec().into();
    SESSION_REPOS_SNAPSHOT_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedSessionReposSnapshot {
            repo_ids,
            repo_keys,
            dedup_indexes_by_repo,
            open_repos: Arc::clone(&open_repos),
        });
    });

    SessionReposSnapshot {
        open_repos,
        active_repo_index,
    }
}

pub fn persist_from_state(state: &AppState) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };

    persist_from_state_to_path(state, &path)
}

pub fn persist_from_state_to_path(state: &AppState, path: &Path) -> io::Result<()> {
    persist_from_state_impl(state, path)
}

fn persist_from_state_impl(state: &AppState, path: &Path) -> io::Result<()> {
    let snapshot = snapshot_repos_from_state(state);
    persist_repos_snapshot_to_path(&snapshot, path)
}

pub fn persist_repos_snapshot(snapshot: &SessionReposSnapshot) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    persist_repos_snapshot_to_path(snapshot, &path)
}

pub fn persist_repos_snapshot_to_path(
    snapshot: &SessionReposSnapshot,
    path: &Path,
) -> io::Result<()> {
    persist_repos_snapshot_impl(snapshot, path)
}

fn persist_repos_snapshot_impl(snapshot: &SessionReposSnapshot, path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;
        file.open_repos = snapshot
            .open_repos
            .iter()
            .map(|path| path.to_string())
            .collect();
        file.active_repo = snapshot
            .active_repo_index
            .and_then(|ix| snapshot.open_repos.get(ix))
            .map(|path| path.to_string());

        persist_to_path(path, &file)
    })
}

/// Moves `value` to the front of an MRU list, dropping any earlier copy of it
/// and holding the list to [`MAX_RECENT_REPOS`]. The cap lives here alone so
/// the session file and the in-memory caches the UI shows can never disagree
/// about how long the list is.
fn promote_within_recents_cap<T: PartialEq>(list: &mut Vec<T>, value: T) {
    list.retain(|existing| existing != &value);
    list.insert(0, value);
    list.truncate(MAX_RECENT_REPOS);
}

/// [`promote_within_recents_cap`] for a caller holding its own copy of what
/// [`UiSession::recent_repos`] last returned: applies one recents bump to that
/// copy so it still matches the file after [`persist_recent_repo`] writes it.
pub fn promote_recent_repo(recents: &mut Vec<PathBuf>, workdir: &Path) {
    promote_within_recents_cap(recents, workdir.to_path_buf());
}

pub fn persist_recent_repo(workdir: &Path) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    persist_recent_repo_to_path(workdir, &path)
}

/// Storage key for a repository path in the recents list.
///
/// Canonicalized so the key matches the workdir the store holds for an open
/// repository, which is canonicalized on open (see
/// `worktree_state::store::canonicalize_path`). The repo picker relies on plain
/// equality between the two to keep a still-open repository out of the
/// "recently closed" section; on macOS, where the temp and home directories are
/// reached through symlinks, an uncanonicalized key would compare unequal to the
/// very same directory and the repository would be listed twice.
///
/// Falls back to the path as given when it cannot be canonicalized, so a
/// repository that has since been deleted or unmounted still round-trips.
///
/// That fallback is one-way: once the directory is gone the canonical form it
/// was stored under can no longer be reconstructed from the path alone. Removal
/// therefore normalizes the *stored* side too rather than relying on this key
/// alone -- see [`remove_recent_repo_to_path`].
fn recent_repo_storage_key(workdir: &Path) -> String {
    path_storage_key(&worktree_core::path_utils::canonicalize_or_original(
        workdir.to_path_buf(),
    ))
}

pub fn persist_recent_repo_to_path(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    persist_recent_repo_impl(workdir, session_file_path)
}

fn persist_recent_repo_impl(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;

        let workdir_key = recent_repo_storage_key(workdir);
        let raw_key = path_storage_key(workdir);
        let recent_repos = file.recent_repos.get_or_insert_with(Vec::new);
        // Blanks go, and a key a hand-edited file padded is normalized in place
        // so the promotion below still recognizes it as the same repository.
        // The uncanonicalized form an older build wrote goes too, so re-opening
        // a repository heals the list instead of duplicating it.
        recent_repos.retain_mut(|path| {
            let trimmed = path.trim();
            if trimmed.is_empty() || trimmed == raw_key {
                return false;
            }
            if trimmed.len() != path.len() {
                *path = trimmed.to_owned();
            }
            true
        });
        promote_within_recents_cap(recent_repos, workdir_key);

        persist_to_path(session_file_path, &file)
    })
}

pub fn remove_recent_repo(workdir: &Path) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    remove_recent_repo_to_path(workdir, &path)
}

pub fn remove_recent_repo_to_path(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;

        // Must key exactly as `persist_recent_repo_to_path` does, or removal
        // silently misses entries written in the other form.
        let workdir_key = recent_repo_storage_key(workdir);
        let raw_key = path_storage_key(workdir);
        let Some(recent_repos) = file.recent_repos.as_mut() else {
            return Ok(());
        };
        // `raw_key` also clears entries left by older builds, which stored the
        // path uncanonicalized. Entries are normalized on their own side as
        // well, so an entry and a caller that spell the same directory
        // differently -- one through a symlink, one not -- still match: keying
        // off `workdir` alone cannot bridge that once the directory is gone,
        // because `canonicalize` no longer resolves it.
        recent_repos.retain(|path| {
            let path = path.trim();
            if path == workdir_key || path == raw_key {
                return false;
            }
            // Through the storage-key decoder, not `Path::new`: a non-UTF-8
            // workdir is stored hex-encoded, and canonicalizing that encoding
            // as a literal path would quietly never match.
            let decoded = path_from_storage_key(path);
            // Only absolute entries are resolved. A relative one -- which only
            // a hand-edited file can produce -- would canonicalize against the
            // process working directory and could match a repository the user
            // never asked to forget.
            if !decoded.is_absolute() {
                return true;
            }
            let normalized = recent_repo_storage_key(&decoded);
            normalized != workdir_key && normalized != raw_key
        });

        persist_to_path(session_file_path, &file)
    })
}

pub fn persist_pinned_repo(workdir: &Path) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    persist_pinned_repo_to_path(workdir, &path)
}

/// Appends a repository to the pin list. Unlike the recents, pins keep the
/// order the user created them in and are never capped — they leave the list
/// only when the user unpins them. Pinning something already pinned therefore
/// leaves it where it is rather than moving it to the end.
pub fn persist_pinned_repo_to_path(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    persist_pinned_repo_impl(workdir, session_file_path)
}

fn persist_pinned_repo_impl(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;

        let workdir_key = path_storage_key(workdir);
        let pinned_repos = file.pinned_repos.get_or_insert_with(Vec::new);
        pinned_repos.retain(|path| !path.trim().is_empty());
        if !pinned_repos.iter().any(|path| path.trim() == workdir_key) {
            pinned_repos.push(workdir_key);
        }

        persist_to_path(session_file_path, &file)
    })
}

pub fn remove_pinned_repo(workdir: &Path) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    remove_pinned_repo_to_path(workdir, &path)
}

pub fn remove_pinned_repo_to_path(workdir: &Path, session_file_path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;

        let workdir_key = path_storage_key(workdir);
        let Some(pinned_repos) = file.pinned_repos.as_mut() else {
            return Ok(());
        };
        pinned_repos.retain(|path| path.trim() != workdir_key);

        persist_to_path(session_file_path, &file)
    })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiSettings {
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub sidebar_width: Option<u32>,
    pub details_width: Option<u32>,
    pub sidebar_collapsed: Option<bool>,
    pub repo_sidebar_collapsed_items: Option<BTreeMap<PathBuf, BTreeSet<String>>>,
    pub repo_sidebar_pinned_branches: Option<BTreeMap<PathBuf, BTreeSet<String>>>,
    pub theme_mode: Option<String>,
    pub language: Option<String>,
    pub avatar_source: Option<String>,
    pub ai_commit_source: Option<String>,
    pub ai_commit_custom_command: Option<String>,
    pub ai_commit_provider: Option<String>,
    pub ai_commit_api_key: Option<String>,
    pub ai_commit_model: Option<String>,
    pub ai_commit_endpoint: Option<String>,
    pub ui_scale_percent: Option<u32>,
    pub ui_density: Option<String>,
    pub ui_font_family: Option<String>,
    pub editor_font_family: Option<String>,
    pub use_font_ligatures: Option<bool>,
    pub date_time_format: Option<String>,
    pub timezone: Option<String>,
    pub show_timezone: Option<bool>,
    pub change_tracking_view: Option<String>,
    pub repo_picker_sort: Option<String>,
    /// Whole replacement set — the repository picker owns it and always writes
    /// every collapsed section it knows about.
    pub repo_picker_collapsed_sections: Option<BTreeSet<String>>,
    pub diff_scroll_sync: Option<String>,
    pub diff_content_mode: Option<String>,
    pub diff_whitespace_mode: Option<String>,
    pub diff_view_mode: Option<String>,
    pub annotate_enabled: Option<bool>,
    pub diff_reveal_whitespace_chars: Option<bool>,
    pub diff_word_wrap: Option<bool>,
    pub diff_show_line_numbers: Option<bool>,
    pub auto_save_file_edits: Option<bool>,
    pub mergetool_auto_advance: Option<bool>,
    pub mergetool_collapse_unchanged: Option<bool>,
    pub mergetool_output_scroll_sync: Option<bool>,
    pub mergetool_show_line_numbers: Option<bool>,
    pub mergetool_view_three_way: Option<bool>,
    pub change_tracking_height: Option<u32>,
    pub untracked_height: Option<u32>,
    pub history_show_graph: Option<bool>,
    pub history_show_author: Option<bool>,
    pub history_show_date: Option<bool>,
    pub history_show_sha: Option<bool>,
    pub terminal_external_mode: Option<String>,
    pub terminal_external_program: Option<String>,
    pub terminal_external_args: Option<Vec<String>>,
    pub terminal_action_bar_target: Option<String>,
    pub history_show_tags: Option<bool>,
    pub history_relative_dates: Option<bool>,
    pub history_highlight_commit_chain: Option<bool>,
    pub history_tag_fetch_mode: Option<GitLogTagFetchMode>,
    pub default_history_mode: Option<HistoryMode>,
    pub commit_push_after_enabled: Option<bool>,
    pub push_pull_retry_enabled: Option<bool>,
    pub default_tag_type: Option<DefaultTagType>,
    pub git_executable_path: Option<Option<PathBuf>>,
    pub external_code_editor: Option<Option<ExternalCodeEditorSetting>>,
    /// `FromGitConfig` is the "unconfigured" state, so a plain `Some` write
    /// covers every reachable value.
    pub external_merge_tool: Option<ExternalMergeToolSelection>,
}

pub fn persist_ui_settings(settings: UiSettings) -> io::Result<()> {
    let Some(path) = default_session_file_path() else {
        return Ok(());
    };
    persist_ui_settings_to_path(settings, &path)
}

pub fn persist_ui_settings_to_path(settings: UiSettings, path: &Path) -> io::Result<()> {
    persist_ui_settings_impl(settings, path)
}

fn persist_ui_settings_impl(settings: UiSettings, path: &Path) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;
        if settings.window_width.is_some() && settings.window_height.is_some() {
            file.window_width = settings.window_width;
            file.window_height = settings.window_height;
        }
        if let Some(w) = settings.sidebar_width {
            file.sidebar_width = Some(w);
        }
        if let Some(w) = settings.details_width {
            file.details_width = Some(w);
        }
        if let Some(collapsed) = settings.sidebar_collapsed {
            file.sidebar_collapsed = Some(collapsed);
        }
        if let Some(items) = settings.repo_sidebar_collapsed_items {
            let items = path_keyed_string_sets_to_storage(items);
            file.repo_sidebar_collapsed_items = (!items.is_empty()).then_some(items);
        }
        if let Some(items) = settings.repo_sidebar_pinned_branches {
            let items = path_keyed_string_sets_to_storage(items);
            file.repo_sidebar_pinned_branches = (!items.is_empty()).then_some(items);
        }
        if let Some(theme_mode) = settings.theme_mode {
            file.theme_mode = Some(theme_mode);
        }
        if let Some(language) = settings.language {
            file.language = Some(language);
        }
        if let Some(avatar_source) = settings.avatar_source {
            file.avatar_source = Some(avatar_source);
        }
        if let Some(source) = settings.ai_commit_source {
            file.ai_commit_source = Some(source);
        }
        if let Some(command) = settings.ai_commit_custom_command {
            file.ai_commit_custom_command = Some(command);
        }
        if let Some(provider) = settings.ai_commit_provider {
            file.ai_commit_provider = Some(provider);
        }
        if let Some(api_key) = settings.ai_commit_api_key {
            file.ai_commit_api_key = Some(api_key);
        }
        if let Some(model) = settings.ai_commit_model {
            file.ai_commit_model = Some(model);
        }
        if let Some(endpoint) = settings.ai_commit_endpoint {
            file.ai_commit_endpoint = Some(endpoint);
        }
        if let Some(percent) = settings.ui_scale_percent {
            file.ui_scale_percent = Some(percent);
        }
        if let Some(density) = settings.ui_density {
            file.ui_density = Some(density);
        }
        if let Some(font_family) = settings.ui_font_family {
            file.ui_font_family = Some(font_family);
        }
        if let Some(font_family) = settings.editor_font_family {
            file.editor_font_family = Some(font_family);
        }
        if let Some(value) = settings.use_font_ligatures {
            file.use_font_ligatures = Some(value);
        }
        if let Some(fmt) = settings.date_time_format {
            file.date_time_format = Some(fmt);
        }
        if let Some(tz) = settings.timezone {
            file.timezone = Some(tz);
        }
        if let Some(value) = settings.show_timezone {
            file.show_timezone = Some(value);
        }
        if let Some(value) = settings.change_tracking_view {
            file.change_tracking_view = Some(value);
        }
        if let Some(value) = settings.repo_picker_sort {
            file.repo_picker_sort = Some(value);
        }
        // Owned by the repository picker (`repo_picker::persist_collapsed_sections`).
        if let Some(value) = settings.repo_picker_collapsed_sections {
            file.repo_picker_collapsed_sections = Some(value);
        }
        if let Some(value) = settings.diff_scroll_sync {
            file.diff_scroll_sync = Some(value);
        }
        if let Some(value) = settings.diff_content_mode {
            file.diff_content_mode = Some(value);
        }
        if let Some(value) = settings.diff_whitespace_mode {
            file.diff_whitespace_mode = Some(value);
        }
        if let Some(value) = settings.diff_view_mode {
            file.diff_view_mode = Some(value);
        }
        if let Some(value) = settings.annotate_enabled {
            file.annotate_enabled = Some(value);
        }
        if let Some(value) = settings.diff_reveal_whitespace_chars {
            file.diff_reveal_whitespace_chars = Some(value);
        }
        if let Some(value) = settings.mergetool_auto_advance {
            file.mergetool_auto_advance = Some(value);
        }
        if let Some(value) = settings.mergetool_collapse_unchanged {
            file.mergetool_collapse_unchanged = Some(value);
        }
        if let Some(value) = settings.mergetool_output_scroll_sync {
            file.mergetool_output_scroll_sync = Some(value);
        }
        if let Some(value) = settings.mergetool_show_line_numbers {
            file.mergetool_show_line_numbers = Some(value);
        }
        if let Some(value) = settings.mergetool_view_three_way {
            file.mergetool_view_three_way = Some(value);
        }
        if let Some(value) = settings.diff_word_wrap {
            file.diff_word_wrap = Some(value);
        }
        if let Some(value) = settings.auto_save_file_edits {
            file.auto_save_file_edits = Some(value);
        }
        if let Some(value) = settings.diff_show_line_numbers {
            file.diff_show_line_numbers = Some(value);
        }
        if let Some(value) = settings.change_tracking_height {
            file.change_tracking_height = Some(value);
        }
        if let Some(value) = settings.untracked_height {
            file.untracked_height = Some(value);
        }
        if let Some(value) = settings.history_show_graph {
            file.history_show_graph = Some(value);
        }
        if let Some(value) = settings.history_show_author {
            file.history_show_author = Some(value);
        }
        if let Some(value) = settings.history_show_date {
            file.history_show_date = Some(value);
        }
        if let Some(value) = settings.history_show_sha {
            file.history_show_sha = Some(value);
        }
        if let Some(value) = settings.terminal_external_mode {
            file.terminal_external_mode = Some(value);
        }
        if let Some(value) = settings.terminal_external_program {
            file.terminal_external_program = Some(value);
        }
        if let Some(value) = settings.terminal_external_args {
            let values = value
                .into_iter()
                .map(|arg| arg.trim().to_string())
                .filter(|arg| !arg.is_empty())
                .collect::<Vec<_>>();
            file.terminal_external_args = Some(values);
        }
        if let Some(value) = settings.terminal_action_bar_target {
            file.terminal_action_bar_target = Some(value);
        }
        if let Some(value) = settings.history_show_tags {
            file.history_show_tags = Some(value);
        }
        if let Some(value) = settings.history_highlight_commit_chain {
            file.history_highlight_commit_chain = Some(value);
        }
        if let Some(value) = settings.history_relative_dates {
            file.history_relative_dates = Some(value);
        }
        if let Some(value) = settings.history_tag_fetch_mode {
            file.history_tag_fetch_mode = Some(value);
        }
        if let Some(value) = settings.default_history_mode {
            file.default_history_mode = Some(value.into());
        }
        if let Some(value) = settings.commit_push_after_enabled {
            file.commit_push_after_enabled = Some(value);
        }
        if let Some(value) = settings.push_pull_retry_enabled {
            file.push_pull_retry_enabled = Some(value);
        }
        if let Some(value) = settings.default_tag_type {
            file.default_tag_type = Some(value);
        }
        if let Some(path) = settings.git_executable_path {
            file.git_executable_path = path.map(|path| path_storage_key(&path));
        }
        if let Some(editor) = settings.external_code_editor {
            file.external_code_editor = editor.map(external_code_editor_to_file);
        }
        if let Some(merge_tool) = settings.external_merge_tool {
            file.external_merge_tool = Some(merge_tool);
        }

        persist_to_path(path, &file)
    })
}

static SESSION_FILE_PERSIST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn session_file_persist_lock() -> &'static Mutex<()> {
    SESSION_FILE_PERSIST_LOCK.get_or_init(|| Mutex::new(()))
}

fn with_session_file_persist_lock<T>(persist: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _guard = session_file_persist_lock()
        .lock()
        .unwrap_or_else(|err| err.into_inner());
    persist()
}

pub fn load_default_history_mode() -> Option<HistoryMode> {
    let session_file_path = default_session_file_path()?;
    load_default_history_mode_from_path(&session_file_path)
}

pub fn load_default_history_mode_from_path(session_file_path: &Path) -> Option<HistoryMode> {
    let file = load_file(session_file_path)?;
    file.default_history_mode.map(Into::into)
}

pub fn load_repo_history_mode(workdir: &Path) -> Option<HistoryMode> {
    let session_file_path = default_session_file_path()?;
    load_repo_history_mode_from_path(workdir, &session_file_path)
}

pub fn load_repo_history_mode_from_path(
    workdir: &Path,
    session_file_path: &Path,
) -> Option<HistoryMode> {
    let workdir_key = path_storage_key(workdir);
    let file = load_file(session_file_path)?;
    let modes = file.repo_history_modes?;
    modes.get(&workdir_key).copied().map(Into::into)
}

pub fn load_repo_history_modes() -> BTreeMap<String, HistoryMode> {
    let Some(session_file_path) = default_session_file_path() else {
        return BTreeMap::new();
    };
    load_repo_history_modes_from_path(&session_file_path)
}

pub fn load_repo_history_modes_from_path(
    session_file_path: &Path,
) -> BTreeMap<String, HistoryMode> {
    let Some(file) = load_file(session_file_path) else {
        return BTreeMap::new();
    };
    file.repo_history_modes
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect()
}

pub fn persist_repo_history_mode(workdir: &Path, mode: HistoryMode) -> io::Result<()> {
    let Some(session_file_path) = default_session_file_path() else {
        return Ok(());
    };
    persist_repo_history_mode_to_path(workdir, mode, &session_file_path)
}

fn repo_history_mode_setting_from_file(
    file: &UiSessionFile,
    workdir: &Path,
) -> Option<HistoryModeSetting> {
    file.repo_history_modes.as_ref().and_then(|modes| {
        workdir
            .to_str()
            .and_then(|path| modes.get(path).copied())
            .or_else(|| {
                let workdir_key = path_storage_key(workdir);
                modes.get(&workdir_key).copied()
            })
    })
}

pub fn persist_repo_history_mode_to_path(
    workdir: &Path,
    mode: HistoryMode,
    session_file_path: &Path,
) -> io::Result<()> {
    persist_repo_history_mode_impl(workdir, mode, session_file_path)
}

fn persist_repo_history_mode_impl(
    workdir: &Path,
    mode: HistoryMode,
    session_file_path: &Path,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        let mode = HistoryModeSetting::from(mode);

        if repo_history_mode_setting_from_file(&file, workdir)
            .is_some_and(|existing| existing == mode)
        {
            return Ok(());
        }

        file.version = CURRENT_SESSION_FILE_VERSION;
        let workdir_key = path_storage_key(workdir);
        file.repo_history_modes
            .get_or_insert_with(BTreeMap::new)
            .insert(workdir_key, mode);

        persist_to_path(session_file_path, &file)
    })
}

pub fn persist_repo_history_modes_batch_to_path(
    updates: &[(PathBuf, HistoryMode)],
    session_file_path: &Path,
) -> io::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }

    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        let mut changed = false;

        for (workdir, mode) in updates {
            let mode = HistoryModeSetting::from(*mode);
            if repo_history_mode_setting_from_file(&file, workdir)
                .is_some_and(|existing| existing == mode)
            {
                continue;
            }

            let workdir_key = path_storage_key(workdir);
            file.repo_history_modes
                .get_or_insert_with(BTreeMap::new)
                .insert(workdir_key, mode);
            changed = true;
        }

        if !changed {
            return Ok(());
        }

        file.version = CURRENT_SESSION_FILE_VERSION;
        persist_to_path(session_file_path, &file)
    })
}

pub fn load_repo_history_scope(workdir: &Path) -> Option<LogScope> {
    let session_file_path = default_session_file_path()?;
    load_repo_history_scope_from_path(workdir, &session_file_path)
}

pub fn load_repo_history_scope_from_path(
    workdir: &Path,
    session_file_path: &Path,
) -> Option<LogScope> {
    let workdir_key = path_storage_key(workdir);
    let file = load_file(session_file_path)?;
    let scopes = file.repo_history_scopes?;
    scopes.get(&workdir_key).copied().map(Into::into)
}

pub fn load_repo_history_scopes() -> BTreeMap<String, LogScope> {
    let Some(session_file_path) = default_session_file_path() else {
        return BTreeMap::new();
    };
    load_repo_history_scopes_from_path(&session_file_path)
}

pub fn load_repo_history_scopes_from_path(session_file_path: &Path) -> BTreeMap<String, LogScope> {
    let Some(file) = load_file(session_file_path) else {
        return BTreeMap::new();
    };
    file.repo_history_scopes
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect()
}

pub fn persist_repo_history_scope(workdir: &Path, scope: LogScope) -> io::Result<()> {
    let Some(session_file_path) = default_session_file_path() else {
        return Ok(());
    };
    persist_repo_history_scope_to_path(workdir, scope, &session_file_path)
}

pub fn persist_repo_history_scope_to_path(
    workdir: &Path,
    scope: LogScope,
    session_file_path: &Path,
) -> io::Result<()> {
    persist_repo_history_scope_impl(workdir, scope, session_file_path)
}

fn persist_repo_history_scope_impl(
    workdir: &Path,
    scope: LogScope,
    session_file_path: &Path,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        let scope = HistoryScopeSetting::from(scope);

        if let Some(existing_scope) = file.repo_history_scopes.as_ref().and_then(|scopes| {
            workdir
                .to_str()
                .and_then(|path| scopes.get(path).copied())
                .or_else(|| {
                    let workdir_key = path_storage_key(workdir);
                    scopes.get(&workdir_key).copied()
                })
        }) && existing_scope == scope
        {
            return Ok(());
        }

        file.version = CURRENT_SESSION_FILE_VERSION;
        let workdir_key = path_storage_key(workdir);
        file.repo_history_scopes
            .get_or_insert_with(BTreeMap::new)
            .insert(workdir_key, scope);

        persist_to_path(session_file_path, &file)
    })
}

/// Persists the history author filter for `workdir`. `None` clears the stored
/// filter; a `Some(Some(_))` stores the active author.
pub fn persist_repo_history_author_filter_to_path(
    workdir: &Path,
    author: Option<&str>,
    session_file_path: &Path,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        let stored = file
            .repo_history_author_filters
            .get_or_insert_with(BTreeMap::new);
        let workdir_key = path_storage_key(workdir);
        let existing = stored.get(&workdir_key).cloned().flatten();
        if existing == author.map(ToOwned::to_owned) {
            return Ok(());
        }
        if let Some(author) = author {
            stored.insert(workdir_key, Some(author.to_owned()));
        } else {
            stored.remove(&workdir_key);
        }
        file.version = CURRENT_SESSION_FILE_VERSION;
        persist_to_path(session_file_path, &file)
    })
}

/// Persists the history ref filters for `workdir`. An empty list clears the
/// stored entry; otherwise the sorted, deduplicated ref names are stored.
pub fn persist_repo_history_ref_filters_to_path(
    workdir: &Path,
    refs: &[String],
    session_file_path: &Path,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        let stored = file
            .repo_history_ref_filters
            .get_or_insert_with(BTreeMap::new);
        let workdir_key = path_storage_key(workdir);
        let existing = stored.get(&workdir_key).cloned().unwrap_or_default();
        if existing == refs {
            return Ok(());
        }
        if refs.is_empty() {
            stored.remove(&workdir_key);
        } else {
            stored.insert(workdir_key, refs.to_vec());
        }
        file.version = CURRENT_SESSION_FILE_VERSION;
        persist_to_path(session_file_path, &file)
    })
}

pub fn load_repo_fetch_prune_deleted_remote_tracking_branches(workdir: &Path) -> Option<bool> {
    let session_file_path = default_session_file_path()?;
    load_repo_fetch_prune_deleted_remote_tracking_branches_from_path(workdir, &session_file_path)
}

pub fn load_repo_fetch_prune_deleted_remote_tracking_branches_from_path(
    workdir: &Path,
    session_file_path: &Path,
) -> Option<bool> {
    let workdir_key = path_storage_key(workdir);
    let file = load_file(session_file_path)?;
    let settings = file.repo_fetch_prune_deleted_remote_tracking_branches?;
    settings.get(&workdir_key).copied()
}

pub fn load_repo_fetch_prune_deleted_remote_tracking_branches_by_repo() -> BTreeMap<String, bool> {
    let Some(session_file_path) = default_session_file_path() else {
        return BTreeMap::new();
    };
    load_repo_fetch_prune_deleted_remote_tracking_branches_by_repo_from_path(&session_file_path)
}

pub fn load_repo_fetch_prune_deleted_remote_tracking_branches_by_repo_from_path(
    session_file_path: &Path,
) -> BTreeMap<String, bool> {
    let Some(file) = load_file(session_file_path) else {
        return BTreeMap::new();
    };
    file.repo_fetch_prune_deleted_remote_tracking_branches
        .unwrap_or_default()
}

pub fn persist_repo_fetch_prune_deleted_remote_tracking_branches(
    workdir: &Path,
    enabled: bool,
) -> io::Result<()> {
    let Some(session_file_path) = default_session_file_path() else {
        return Ok(());
    };
    persist_repo_fetch_prune_deleted_remote_tracking_branches_to_path(
        workdir,
        enabled,
        &session_file_path,
    )
}

pub fn persist_repo_fetch_prune_deleted_remote_tracking_branches_to_path(
    workdir: &Path,
    enabled: bool,
    session_file_path: &Path,
) -> io::Result<()> {
    persist_repo_fetch_prune_deleted_remote_tracking_branches_impl(
        workdir,
        enabled,
        session_file_path,
    )
}

fn persist_repo_fetch_prune_deleted_remote_tracking_branches_impl(
    workdir: &Path,
    enabled: bool,
    session_file_path: &Path,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;
        let workdir_key = path_storage_key(workdir);
        file.repo_fetch_prune_deleted_remote_tracking_branches
            .get_or_insert_with(BTreeMap::new)
            .insert(workdir_key, enabled);

        persist_to_path(session_file_path, &file)
    })
}

pub fn should_show_survey_prompt(survey_id: &str) -> bool {
    let Some(session_file_path) = default_session_file_path() else {
        return false;
    };
    should_show_survey_prompt_from_path(&session_file_path, survey_id, current_unix_seconds())
}

pub fn should_show_survey_prompt_from_path(
    session_file_path: &Path,
    survey_id: &str,
    now_unix_seconds: u64,
) -> bool {
    let Some(file) = load_file(session_file_path) else {
        return false;
    };
    if !has_recorded_session_repository(&file) {
        return false;
    }

    let Some(prompt) = file.survey_prompt else {
        return true;
    };
    if prompt.survey_id != survey_id {
        return true;
    }
    if prompt.opened_at_unix_seconds.is_some() {
        return false;
    }

    prompt
        .postponed_until_unix_seconds
        .is_none_or(|postponed_until| postponed_until <= now_unix_seconds)
}

pub fn persist_survey_prompt_opened(survey_id: &str) -> io::Result<()> {
    let Some(session_file_path) = default_session_file_path() else {
        return Ok(());
    };
    persist_survey_prompt_opened_to_path(&session_file_path, survey_id, current_unix_seconds())
}

pub fn persist_survey_prompt_opened_to_path(
    session_file_path: &Path,
    survey_id: &str,
    now_unix_seconds: u64,
) -> io::Result<()> {
    persist_survey_prompt_opened_impl(session_file_path, survey_id, now_unix_seconds)
}

fn persist_survey_prompt_opened_impl(
    session_file_path: &Path,
    survey_id: &str,
    now_unix_seconds: u64,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;
        file.survey_prompt = Some(SurveyPromptSession {
            survey_id: survey_id.to_string(),
            opened_at_unix_seconds: Some(now_unix_seconds),
            postponed_until_unix_seconds: None,
        });

        persist_to_path(session_file_path, &file)
    })
}

pub fn persist_survey_prompt_postponed(survey_id: &str, postpone_seconds: u64) -> io::Result<()> {
    let Some(session_file_path) = default_session_file_path() else {
        return Ok(());
    };
    persist_survey_prompt_postponed_to_path(
        &session_file_path,
        survey_id,
        postpone_seconds,
        current_unix_seconds(),
    )
}

pub fn persist_survey_prompt_postponed_to_path(
    session_file_path: &Path,
    survey_id: &str,
    postpone_seconds: u64,
    now_unix_seconds: u64,
) -> io::Result<()> {
    persist_survey_prompt_postponed_impl(
        session_file_path,
        survey_id,
        postpone_seconds,
        now_unix_seconds,
    )
}

fn persist_survey_prompt_postponed_impl(
    session_file_path: &Path,
    survey_id: &str,
    postpone_seconds: u64,
    now_unix_seconds: u64,
) -> io::Result<()> {
    with_session_file_persist_lock(|| {
        let mut file = load_file(session_file_path).unwrap_or_default();
        file.version = CURRENT_SESSION_FILE_VERSION;
        file.survey_prompt = Some(SurveyPromptSession {
            survey_id: survey_id.to_string(),
            opened_at_unix_seconds: None,
            postponed_until_unix_seconds: Some(now_unix_seconds.saturating_add(postpone_seconds)),
        });

        persist_to_path(session_file_path, &file)
    })
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// Survey eligibility only needs a usage signal. A recorded repository means the user has used
// WorkTree before; it does not need to prove the repository still exists on disk.
fn has_recorded_session_repository(file: &UiSessionFile) -> bool {
    if file.open_repos.iter().any(|path| !path.trim().is_empty()) {
        return true;
    }
    if file
        .active_repo
        .as_deref()
        .is_some_and(|path| !path.trim().is_empty())
    {
        return true;
    }
    if file
        .recent_repos
        .as_ref()
        .is_some_and(|paths| paths.iter().any(|path| !path.trim().is_empty()))
    {
        return true;
    }
    false
}

fn parse_repos(
    open_repos_raw: Vec<String>,
    active_repo_raw: Option<String>,
) -> (Vec<PathBuf>, Option<PathBuf>) {
    let open_repos = parse_path_list(open_repos_raw);
    let seen: FxHashSet<PathBuf> = open_repos.iter().cloned().collect();

    let active_repo = active_repo_raw
        .as_deref()
        .and_then(|p| {
            let p = p.trim();
            if p.is_empty() {
                None
            } else {
                Some(path_from_storage_key(p))
            }
        })
        .filter(|active| seen.contains(active));

    (open_repos, active_repo)
}

fn parse_path_list(paths_raw: Vec<String>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::with_capacity(paths_raw.len());
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    for raw in paths_raw {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let path = path_from_storage_key(raw);
        if !seen.insert(path.clone()) {
            continue;
        }
        paths.push(path);
    }
    paths
}

fn parse_path_keyed_string_sets(
    paths_raw: BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut paths: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (raw_path, values) in paths_raw {
        let raw_path = raw_path.trim();
        if raw_path.is_empty() {
            continue;
        }
        let path = path_from_storage_key(raw_path);
        let entry = paths.entry(path).or_default();
        for value in values {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            entry.insert(value.to_string());
        }
    }
    paths.retain(|_, values| !values.is_empty());
    paths
}

fn path_keyed_string_sets_to_storage(
    paths: BTreeMap<PathBuf, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut stored = BTreeMap::new();
    for (path, values) in paths {
        let mut normalized = BTreeSet::new();
        for value in values {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            normalized.insert(value.to_string());
        }
        if normalized.is_empty() {
            continue;
        }
        stored.insert(path_storage_key(&path), normalized);
    }
    stored
}

fn non_empty_string(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn external_code_editor_from_file(
    setting: Option<ExternalCodeEditorSettingFile>,
) -> Option<ExternalCodeEditorSetting> {
    match setting? {
        ExternalCodeEditorSettingFile::Detected { id, path } => {
            let path = path.trim();
            if path.is_empty() {
                return None;
            }
            Some(ExternalCodeEditorSetting::Detected {
                id: non_empty_string(id)?,
                path: path_from_storage_key(path),
            })
        }
        ExternalCodeEditorSettingFile::Custom {
            executable,
            arguments,
        } => Some(ExternalCodeEditorSetting::Custom {
            executable: path_from_storage_key(executable.trim()),
            arguments: arguments.and_then(non_empty_string),
        }),
    }
}

fn external_code_editor_to_file(
    setting: ExternalCodeEditorSetting,
) -> ExternalCodeEditorSettingFile {
    match setting {
        ExternalCodeEditorSetting::Detected { id, path } => {
            ExternalCodeEditorSettingFile::Detected {
                id,
                path: path_storage_key(&path),
            }
        }
        ExternalCodeEditorSetting::Custom {
            executable,
            arguments,
        } => ExternalCodeEditorSettingFile::Custom {
            executable: path_storage_key(&executable),
            arguments: arguments.and_then(non_empty_string),
        },
    }
}

fn sanitize_ui_scale_percent(percent: Option<u32>) -> u32 {
    percent
        .unwrap_or(DEFAULT_UI_SCALE_PERCENT)
        .clamp(MIN_UI_SCALE_PERCENT, MAX_UI_SCALE_PERCENT)
}

fn migrate_scaled_dimension_to_design_units(
    value: Option<u32>,
    ui_scale_percent: Option<u32>,
) -> Option<u32> {
    let value = value? as f32;
    let factor =
        sanitize_ui_scale_percent(ui_scale_percent) as f32 / DEFAULT_UI_SCALE_PERCENT as f32;
    let design_units = (value / factor).round();
    (design_units.is_finite() && design_units >= 1.0).then_some(design_units as u32)
}

fn migrate_v2_file(mut file: UiSessionFile) -> UiSessionFile {
    let ui_scale_percent = file.ui_scale_percent;
    file.version = CURRENT_SESSION_FILE_VERSION;
    file.sidebar_width =
        migrate_scaled_dimension_to_design_units(file.sidebar_width, ui_scale_percent);
    file.details_width =
        migrate_scaled_dimension_to_design_units(file.details_width, ui_scale_percent);
    file.change_tracking_height =
        migrate_scaled_dimension_to_design_units(file.change_tracking_height, ui_scale_percent);
    file.untracked_height =
        migrate_scaled_dimension_to_design_units(file.untracked_height, ui_scale_percent);
    file
}

pub fn load_file(path: &Path) -> Option<UiSessionFile> {
    let Ok(contents) = fs::read_to_string(path) else {
        return None;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return None;
    };
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(SESSION_FILE_VERSION_V1 as u64) as u32;
    match version {
        SESSION_FILE_VERSION_V1 => {
            let file: UiSessionFileV1 = serde_json::from_value(value).ok()?;
            Some(UiSessionFile {
                version: CURRENT_SESSION_FILE_VERSION,
                open_repos: file.open_repos,
                active_repo: file.active_repo,
                ..UiSessionFile::default()
            })
        }
        SESSION_FILE_VERSION_V2 => {
            let file = serde_json::from_value::<UiSessionFile>(value).ok()?;
            Some(migrate_v2_file(file))
        }
        SESSION_FILE_VERSION_V3 => serde_json::from_value::<UiSessionFile>(value).ok(),
        _ => None,
    }
}

pub fn path_storage_key(path: &Path) -> String {
    if let Some(text) = path.to_str() {
        return text.to_string();
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;

        let bytes = path.as_os_str().as_bytes();
        let mut out = String::with_capacity(SESSION_PATH_BYTES_PREFIX.len() + bytes.len() * 2);
        out.push_str(SESSION_PATH_BYTES_PREFIX);
        out.push_str(&hex_encode(bytes));
        out
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;

        let mut raw = Vec::new();
        for unit in path.as_os_str().encode_wide() {
            raw.extend_from_slice(&unit.to_le_bytes());
        }
        let mut out = String::with_capacity(SESSION_PATH_WIDE_PREFIX.len() + raw.len() * 2);
        out.push_str(SESSION_PATH_WIDE_PREFIX);
        out.push_str(&hex_encode(&raw));
        out
    }

    #[cfg(not(any(unix, windows)))]
    {
        path.display().to_string()
    }
}

pub fn path_storage_key_shared(path: &Path) -> Arc<str> {
    if let Some(text) = path.to_str() {
        return Arc::from(text);
    }

    Arc::from(path_storage_key(path))
}

pub fn path_from_storage_key(raw: &str) -> PathBuf {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt as _;

        if let Some(hex) = raw.strip_prefix(SESSION_PATH_BYTES_PREFIX)
            && let Some(bytes) = hex_decode(hex)
        {
            return PathBuf::from(OsString::from_vec(bytes));
        }
    }

    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt as _;

        if let Some(hex) = raw.strip_prefix(SESSION_PATH_WIDE_PREFIX)
            && let Some(bytes) = hex_decode(hex)
            && bytes.len() % 2 == 0
        {
            let mut wide = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.as_chunks::<2>().0 {
                wide.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            return PathBuf::from(OsString::from_wide(&wide));
        }
    }

    PathBuf::from(raw)
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for pair in bytes.as_chunks::<2>().0 {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn persist_to_path(path: &Path, session: &impl Serialize) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let contents = serde_json::to_vec(session).expect("serializing session file should succeed");

    let mut tmp_file = tempfile::NamedTempFile::new_in(parent)?;
    tmp_file.write_all(&contents)?;
    tmp_file.flush()?;

    tmp_file.persist(path).map(|_| ()).map_err(|err| err.error)
}

fn default_session_file_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(path) = test_session_file_path_override() {
        return path;
    }

    if let Some(path) = env::var_os(SESSION_FILE_ENV)
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }

    if env::var_os(DISABLE_SESSION_PERSIST_ENV).is_some() {
        return None;
    }

    // Avoid reading/writing user state dir during test binaries (e.g. `cargo test`, `cargo nextest`).
    // `cfg!(test)` only applies to this crate's own unit tests; dependencies built for tests do not
    // have `cfg(test)` set, so we also use a runtime heuristic.
    if cfg!(test) || running_under_test_harness() {
        return None;
    }

    Some(app_state_dir()?.join("session.json"))
}

pub(crate) fn default_session_file_path_for_effect() -> Option<PathBuf> {
    default_session_file_path()
}

fn running_under_test_harness() -> bool {
    let Ok(exe) = env::current_exe() else {
        return false;
    };
    looks_like_test_binary(&exe)
}

pub fn looks_like_test_binary(exe: &Path) -> bool {
    if exe.components().any(|component| {
        component.as_os_str() == OsStr::new("deps")
            || component.as_os_str() == OsStr::new("nextest")
    }) {
        return true;
    }

    exe.file_stem()
        .is_some_and(looks_like_cargo_test_binary_name)
}

fn looks_like_cargo_test_binary_name(stem: &OsStr) -> bool {
    let Some(stem) = stem.to_str() else {
        return false;
    };
    let Some((_prefix, suffix)) = stem.rsplit_once('-') else {
        return false;
    };
    // Cargo test binaries typically end in a 16-hex-digit hash suffix, e.g. `mycrate-3ad1b0fd3f0c0d3e`.
    suffix.len() == 16 && suffix.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn user_themes_dir() -> Option<PathBuf> {
    if cfg!(test) || running_under_test_harness() {
        return None;
    }

    Some(app_data_dir()?.join("themes"))
}

fn non_empty_path(value: Option<&OsStr>) -> Option<PathBuf> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

fn app_data_dir() -> Option<PathBuf> {
    // Follow XDG on linux; otherwise fall back to platform conventions.
    #[cfg(target_os = "linux")]
    {
        app_data_dir_linux(
            env::var_os("XDG_DATA_HOME").as_deref(),
            env::var_os("HOME").as_deref(),
        )
    }

    #[cfg(target_os = "macos")]
    {
        let home = non_empty_path(env::var_os("HOME").as_deref())?;
        Some(home.join("Library/Application Support/worktree"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA"));
        Some(non_empty_path(appdata.as_deref())?.join("worktree"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        non_empty_path(env::var_os("HOME").as_deref()).map(|home| home.join(".worktree"))
    }
}

#[cfg(target_os = "linux")]
pub fn app_data_dir_linux(xdg_data_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(data_home) = non_empty_path(xdg_data_home) {
        return Some(data_home.join("worktree"));
    }
    let home = non_empty_path(home)?;
    Some(home.join(".local/share/worktree"))
}

fn app_state_dir() -> Option<PathBuf> {
    // Follow XDG on linux; otherwise fall back to platform conventions.
    #[cfg(target_os = "linux")]
    {
        if let Some(state_home) = non_empty_path(env::var_os("XDG_STATE_HOME").as_deref()) {
            return Some(state_home.join("worktree"));
        }
        let home = non_empty_path(env::var_os("HOME").as_deref())?;
        Some(home.join(".local/state/worktree"))
    }

    #[cfg(target_os = "macos")]
    {
        let home = non_empty_path(env::var_os("HOME").as_deref())?;
        Some(home.join("Library/Application Support/worktree"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = env::var_os("LOCALAPPDATA").or_else(|| env::var_os("APPDATA"));
        Some(non_empty_path(appdata.as_deref())?.join("worktree"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        non_empty_path(env::var_os("HOME").as_deref()).map(|home| home.join(".worktree"))
    }
}
