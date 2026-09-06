//! Per-category settings domains.

use super::*;

// `pub(super)` = visible to the settings_window facade, which imports the
// per-domain items the shared chrome and `Render` dispatch still consume
// (compiler evidence: E0603 at the facade's `use self::domain::<name>::…`
// lines when these were private).
pub(super) mod diff;
pub(super) mod general;
pub(super) mod git_log;
pub(super) mod gpg_signing;
pub(super) mod links;
pub(super) mod merge_tool;
pub(super) mod tags;
pub(super) mod terminal;
