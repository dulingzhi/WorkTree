//! Per-category settings domains.

use super::*;

// ---------------------------------------------------------------------------
// Setter convergence (P3/T13-13b): the macros below generate the isomorphic
// preference setters. Expansion-equivalence evidence: every generated setter
// is token-equal (modulo rustfmt) to the handwritten body it replaced; the
// per-setter proof table lives in the task evidence (D:\tmp\p3t13-13b1-*).
//
// Kept handwritten (divergence points):
//   set_ai_commit_source        - extra availability refresh + draft reset
//   set_terminal_status         - writes a status struct, no persist/forward
//   set_external_terminal_mode  - resets the draft before persisting
//   set_action_bar_terminal_target - same draft-reset shape, different field
//   set_gpg_commit_signing      - writes git config before persisting
//   set_git_executable_mode     - installs the runtime path, no prefs persist
//   set_external_editor_setting - seeds/clears the custom drafts on switch
//   set_ui_scale_percent        - rescales window chrome per main window
//   set_theme_mode              - resolves the theme + popover propagation
//   set_language                - updates the i18n global and macOS menus
//   set_avatar_source           - clears the resolved-avatar caches
//   set_merge_tool_selection    - availability-dependent persist path
//   set_merge_tool_manual_path  - Option<String> path handling + persist
//   set_merge_tool_trust_exit_code - nested selection rebuild
//   set_ai_commit_provider      - provider switch re-fetches the model list
//   set_ai_commit_model         - model-draft semantics, gated persist
//   set_history_column_preferences - two-arg update of a column set
//   set_history_show_tags / set_history_tag_fetch_mode - two-arg forward into
//                                 set_history_tag_preferences + section reset
//   set_default_history_mode    - no update_main_windows fan-out
// ---------------------------------------------------------------------------

/// Isomorphic preference setter: guard, assign, persist, forward to every
/// main window, notify. `reset_section` also collapses the expanded dropdown
/// section; `popover` forwards through the main window's popover host.
/// `$win` names the closure's window binding so expansions stay token-equal
/// to the handwritten originals (`_root_window` where the body rescales).
macro_rules! settings_setter {
    ($name:ident, $field:ident, $ty:ty, $next:ident, $win:ident, $target:ident) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, $win, cx| {
                view.$target($next, cx);
            });
            cx.notify();
        }
    };
    ($name:ident, $field:ident, $ty:ty, $next:ident, $win:ident, $target:ident, reset_section) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            self.expanded_section = None;
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, $win, cx| {
                view.$target($next, cx);
            });
            cx.notify();
        }
    };
    ($name:ident, $field:ident, $ty:ty, $next:ident, $win:ident, $target:ident, popover) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, $win, cx| {
                view.popover_host.update(cx, |host, cx| {
                    host.$target($next, cx);
                });
            });
            cx.notify();
        }
    };
    ($name:ident, $field:ident, $ty:ty, $next:ident, $win:ident, $target:ident, popover, reset_section) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            self.expanded_section = None;
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, $win, cx| {
                view.popover_host.update(cx, |host, cx| {
                    host.$target($next, cx);
                });
            });
            cx.notify();
        }
    };
}

/// Font-preference setter: the guard/assign pair plus a push of the whole
/// font triple into the process-global store before persisting. The triple's
/// field names are fixed, which keeps both `*_font_family` expansions
/// token-equal to their handwritten originals.
macro_rules! settings_font_setter {
    ($name:ident, $field:ident, $ty:ty, $next:ident, reset_section) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            self.expanded_section = None;
            crate::font_preferences::set_current(
                cx,
                self.ui_font_family.clone(),
                self.editor_font_family.clone(),
                self.use_font_ligatures,
            );
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, _window, cx| {
                view.notify_font_preferences_changed(cx);
            });
            cx.notify();
        }
    };
    ($name:ident, $field:ident, $ty:ty, $next:ident) => {
        pub(in crate::view::settings_window) fn $name(
            &mut self,
            $next: $ty,
            cx: &mut gpui::Context<Self>,
        ) {
            if self.$field == $next {
                return;
            }

            self.$field = $next;
            crate::font_preferences::set_current(
                cx,
                self.ui_font_family.clone(),
                self.editor_font_family.clone(),
                self.use_font_ligatures,
            );
            self.persist_preferences(cx);
            self.update_main_windows(cx, move |view, _window, cx| {
                view.notify_font_preferences_changed(cx);
            });
            cx.notify();
        }
    };
}

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
