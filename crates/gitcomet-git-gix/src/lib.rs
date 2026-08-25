mod backend;
mod jj;
mod open;
mod repo;
mod util;

pub use backend::GixBackend;

// Embeds every `locales/*.en.yml` / `locales/*.zh-CN.yml` catalog into the
// binary and generates the crate-root lookup functions that `t!` calls. Must
// stay at the crate root: `t!` expands to `crate::_rust_i18n_try_translate`.
// The locale is process-global (set by the UI crate); unset it falls back to
// English, matching the historical strings tests assert.
rust_i18n::i18n!("locales", fallback = ["en"]);

#[doc(hidden)]
pub fn install_test_git_command_environment(
    global_config: std::path::PathBuf,
    home_dir: std::path::PathBuf,
    xdg_config_home: std::path::PathBuf,
    gnupg_home: std::path::PathBuf,
) {
    util::install_test_git_command_environment(util::TestGitCommandEnvironment {
        global_config,
        home_dir,
        xdg_config_home,
        gnupg_home,
    });
}

#[doc(hidden)]
pub fn allow_test_repo_local_mergetool_command(repo: &std::path::Path, tool_name: &str) {
    repo::allow_test_repo_local_mergetool_command(repo, tool_name);
}
