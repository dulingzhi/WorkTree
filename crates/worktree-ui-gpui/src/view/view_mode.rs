use super::*;
use worktree_core::path_utils::canonicalize_or_original;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WorkTreeViewMode {
    #[default]
    Normal,
    FocusedMergetool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InitialRepositoryLaunchMode {
    #[default]
    RestoreSession,
    OpenExplicitly,
}

#[derive(Clone, Debug, Default)]
pub struct WorkTreeViewConfig {
    pub initial_path: Option<std::path::PathBuf>,
    pub initial_repository_launch_mode: InitialRepositoryLaunchMode,
    pub view_mode: WorkTreeViewMode,
    pub focused_mergetool: Option<FocusedMergetoolViewConfig>,
    pub focused_mergetool_exit_code: Option<Arc<AtomicI32>>,
    pub startup_crash_report: Option<StartupCrashReport>,
}

impl WorkTreeViewConfig {
    pub fn normal(startup_crash_report: Option<StartupCrashReport>) -> Self {
        Self {
            initial_path: None,
            initial_repository_launch_mode: InitialRepositoryLaunchMode::RestoreSession,
            view_mode: WorkTreeViewMode::Normal,
            focused_mergetool: None,
            focused_mergetool_exit_code: None,
            startup_crash_report,
        }
    }

    pub fn normal_with_initial_repository(
        initial_path: std::path::PathBuf,
        startup_crash_report: Option<StartupCrashReport>,
    ) -> Self {
        Self {
            initial_path: Some(initial_path),
            initial_repository_launch_mode: InitialRepositoryLaunchMode::OpenExplicitly,
            view_mode: WorkTreeViewMode::Normal,
            focused_mergetool: None,
            focused_mergetool_exit_code: None,
            startup_crash_report,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupCrashReport {
    pub issue_url: String,
    pub summary: String,
    pub crash_log_path: std::path::PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusedMergetoolLabels {
    pub local: String,
    pub remote: String,
    pub base: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusedMergetoolViewConfig {
    pub repo_path: std::path::PathBuf,
    pub conflicted_file_path: std::path::PathBuf,
    pub labels: FocusedMergetoolLabels,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FocusedMergetoolBootstrap {
    pub(super) repo_path: std::path::PathBuf,
    pub(super) target_path: std::path::PathBuf,
}

impl FocusedMergetoolBootstrap {
    pub(super) fn from_view_config(config: FocusedMergetoolViewConfig) -> Self {
        let repo_path = normalize_bootstrap_repo_path(config.repo_path);
        let target_path = focused_mergetool_target_path(&repo_path, &config.conflicted_file_path);
        Self {
            repo_path,
            target_path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FocusedMergetoolBootstrapAction {
    OpenRepo(std::path::PathBuf),
    SetActiveRepo(RepoId),
    SelectConflictDiff {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    LoadConflictFile {
        repo_id: RepoId,
        path: std::path::PathBuf,
    },
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DeferredRepoBootstrap {
    RestoreSession {
        open_repos: Vec<std::path::PathBuf>,
        active_repo: Option<std::path::PathBuf>,
    },
    OpenRepo(std::path::PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SubmoduleDiffBootstrap {
    pub(super) repo_path: std::path::PathBuf,
    pub(super) target: DiffTarget,
}

impl SubmoduleDiffBootstrap {
    pub(super) fn new(repo_path: std::path::PathBuf, target: DiffTarget) -> Self {
        let repo_path = normalize_bootstrap_repo_path(repo_path);
        let target = normalize_bootstrap_diff_target(&repo_path, target);
        Self { repo_path, target }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SubmoduleDiffBootstrapAction {
    OpenRepo(std::path::PathBuf),
    SetActiveRepo(RepoId),
    SelectDiff { repo_id: RepoId, target: DiffTarget },
    Complete,
}

pub(super) fn normalize_bootstrap_repo_path(path: std::path::PathBuf) -> std::path::PathBuf {
    let path = if path.is_relative() {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    } else {
        path
    };
    canonicalize_path(path)
}

fn normalize_bootstrap_target_path(
    repo_path: &std::path::Path,
    target_path: std::path::PathBuf,
) -> std::path::PathBuf {
    if target_path.is_relative() {
        return target_path;
    }

    if let Ok(relative) = target_path.strip_prefix(repo_path) {
        return relative.to_path_buf();
    }

    canonicalize_path(target_path.clone())
        .strip_prefix(repo_path)
        .map(std::path::Path::to_path_buf)
        .unwrap_or(target_path)
}

fn normalize_bootstrap_diff_target(repo_path: &std::path::Path, target: DiffTarget) -> DiffTarget {
    match target {
        DiffTarget::WorkingTree { path, area } => DiffTarget::WorkingTree {
            path: normalize_bootstrap_target_path(repo_path, path),
            area,
        },
        DiffTarget::Commit { commit_id, path } => DiffTarget::Commit {
            commit_id,
            path: path.map(|path| normalize_bootstrap_target_path(repo_path, path)),
        },
        DiffTarget::CommitRange {
            from_commit_id,
            to_commit_id,
            path,
        } => DiffTarget::CommitRange {
            from_commit_id,
            to_commit_id,
            path: path.map(|path| normalize_bootstrap_target_path(repo_path, path)),
        },
    }
}

pub(super) fn focused_mergetool_target_path(
    repo_path: &std::path::Path,
    conflicted_file_path: &std::path::Path,
) -> std::path::PathBuf {
    if conflicted_file_path.is_relative() {
        return conflicted_file_path.to_path_buf();
    }

    if let Ok(relative) = conflicted_file_path.strip_prefix(repo_path) {
        return relative.to_path_buf();
    }

    let normalized_conflicted = canonicalize_path(conflicted_file_path.to_path_buf());
    if let Ok(relative) = normalized_conflicted.strip_prefix(repo_path) {
        return relative.to_path_buf();
    }

    conflicted_file_path.to_path_buf()
}

fn canonicalize_path(path: std::path::PathBuf) -> std::path::PathBuf {
    canonicalize_or_original(path)
}

pub(super) fn focused_mergetool_bootstrap_action(
    state: &AppState,
    bootstrap: &FocusedMergetoolBootstrap,
) -> Option<FocusedMergetoolBootstrapAction> {
    let Some(repo) = state
        .repos
        .iter()
        .find(|r| r.spec.workdir == bootstrap.repo_path)
    else {
        return Some(FocusedMergetoolBootstrapAction::OpenRepo(
            bootstrap.repo_path.clone(),
        ));
    };

    if state.active_repo != Some(repo.id) {
        return Some(FocusedMergetoolBootstrapAction::SetActiveRepo(repo.id));
    }

    if !matches!(repo.open, Loadable::Ready(())) {
        return None;
    }

    let target = DiffTarget::WorkingTree {
        area: DiffArea::Unstaged,
        path: bootstrap.target_path.clone(),
    };
    if repo.diff_state.diff_target.as_ref() != Some(&target) {
        return Some(FocusedMergetoolBootstrapAction::SelectConflictDiff {
            repo_id: repo.id,
            path: bootstrap.target_path.clone(),
        });
    }

    let has_conflict_file_target =
        repo.conflict_state.conflict_file_path.as_ref() == Some(&bootstrap.target_path);
    if !has_conflict_file_target || matches!(repo.conflict_state.conflict_file, Loadable::NotLoaded)
    {
        return Some(FocusedMergetoolBootstrapAction::LoadConflictFile {
            repo_id: repo.id,
            path: bootstrap.target_path.clone(),
        });
    }

    Some(FocusedMergetoolBootstrapAction::Complete)
}

pub(super) fn submodule_diff_bootstrap_action(
    state: &AppState,
    bootstrap: &SubmoduleDiffBootstrap,
) -> Option<SubmoduleDiffBootstrapAction> {
    let Some(repo) = state
        .repos
        .iter()
        .find(|r| r.spec.workdir == bootstrap.repo_path)
    else {
        return Some(SubmoduleDiffBootstrapAction::OpenRepo(
            bootstrap.repo_path.clone(),
        ));
    };

    if state.active_repo != Some(repo.id) {
        return Some(SubmoduleDiffBootstrapAction::SetActiveRepo(repo.id));
    }

    if !matches!(repo.open, Loadable::Ready(())) {
        return None;
    }

    if repo.diff_state.diff_target.as_ref() != Some(&bootstrap.target) {
        return Some(SubmoduleDiffBootstrapAction::SelectDiff {
            repo_id: repo.id,
            target: bootstrap.target.clone(),
        });
    }

    Some(SubmoduleDiffBootstrapAction::Complete)
}

pub(super) fn renders_full_chrome(view_mode: WorkTreeViewMode) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal)
}

pub(super) fn show_diff_file_navigation(view_mode: WorkTreeViewMode) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal)
}

pub(super) fn show_titlebar_repo_tabs(view_mode: WorkTreeViewMode) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal)
}

pub(super) fn command_palette_available(view_mode: WorkTreeViewMode) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal)
}

pub(super) fn should_seed_initial_repository_from_session(
    view_mode: WorkTreeViewMode,
    initial_path: Option<&std::path::Path>,
    initial_repository_launch_mode: InitialRepositoryLaunchMode,
    has_saved_open_repos: bool,
) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal)
        && initial_path.is_some()
        && (matches!(
            initial_repository_launch_mode,
            InitialRepositoryLaunchMode::OpenExplicitly
        ) || has_saved_open_repos)
}

pub(super) fn repository_entry_interstitial_active(
    view_mode: WorkTreeViewMode,
    has_repo_tabs: bool,
) -> bool {
    matches!(view_mode, WorkTreeViewMode::Normal) && !has_repo_tabs
}

pub(super) fn should_show_startup_repository_loading_screen(
    view_mode: WorkTreeViewMode,
    has_repo_tabs: bool,
    startup_repo_bootstrap_pending: bool,
) -> bool {
    repository_entry_interstitial_active(view_mode, has_repo_tabs) && startup_repo_bootstrap_pending
}

pub(super) fn should_show_splash_screen(
    view_mode: WorkTreeViewMode,
    has_repo_tabs: bool,
    startup_repo_bootstrap_pending: bool,
) -> bool {
    repository_entry_interstitial_active(view_mode, has_repo_tabs)
        && !startup_repo_bootstrap_pending
}

pub(super) fn titlebar_workspace_actions_enabled(
    view_mode: WorkTreeViewMode,
    has_repo_tabs: bool,
) -> bool {
    !repository_entry_interstitial_active(view_mode, has_repo_tabs)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum ThemeMode {
    #[default]
    Automatic,
    Named(String),
}

impl ThemeMode {
    pub(super) fn key(&self) -> &str {
        match self {
            Self::Automatic => "automatic",
            Self::Named(key) => key,
        }
    }

    pub(super) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "automatic" => Some(Self::Automatic),
            "light" => Some(Self::Named(
                crate::theme::DEFAULT_LIGHT_THEME_KEY.to_string(),
            )),
            "dark" => Some(Self::Named(
                crate::theme::DEFAULT_DARK_THEME_KEY.to_string(),
            )),
            _ if crate::theme::has_theme_key(raw) => Some(Self::Named(raw.to_string())),
            _ => None,
        }
    }

    pub(super) fn label(&self) -> String {
        match self {
            Self::Automatic => crate::i18n::tr_str("ui.label.theme.automatic").to_string(),
            // Theme names come from the theme files themselves and stay as-is.
            Self::Named(key) => crate::theme::theme_label(key).unwrap_or_else(|| key.clone()),
        }
    }

    pub(super) fn resolve_theme(&self, appearance: gpui::WindowAppearance) -> AppTheme {
        match self {
            Self::Automatic => AppTheme::default_for_window_appearance(appearance),
            Self::Named(key) => crate::theme::AppTheme::from_key(key)
                .unwrap_or_else(|| AppTheme::default_for_window_appearance(appearance)),
        }
    }

    pub(super) const fn is_automatic(&self) -> bool {
        matches!(self, Self::Automatic)
    }
}
