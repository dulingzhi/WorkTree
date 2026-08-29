#[cfg(feature = "benchmarks")]
pub mod benchmarks;
pub mod model;
pub mod msg;
pub mod name_summary;
pub mod session;
pub mod store;

// Embeds every `locales/*.en.yml` / `locales/*.zh-CN.yml` catalog into the
// binary and generates the crate-root lookup functions that `t!` calls. Must
// stay at the crate root: `t!` expands to `crate::_rust_i18n_try_translate`.
//
// The locale itself is process-global (shared with worktree-ui-gpui, which
// owns the language setting and calls `rust_i18n::set_locale`); this crate
// only carries the catalogs for the messages its store produces. Until the UI
// seeds a locale the global defaults to empty, which misses every lookup and
// falls back to English — so tests that assert message text stay green.
rust_i18n::i18n!("locales", fallback = ["en"]);
