//! UI language support (i18n).
//!
//! Translations live in `locales/*.en.yml` / `locales/*.zh-CN.yml` next to the
//! crate manifest and are embedded into the binary at compile time by
//! `rust_i18n::i18n!` (invoked in `lib.rs`, which is where the generated
//! lookup functions must land). Call sites translate through the re-exported
//! [`t!`] macro or the [`tr`] helper, both of which follow the active
//! [`Language`] held in the [`AppLanguage`] gpui global — the same shape as
//! `ui_scale`'s global.

use gitcomet_state::session;
use gpui::{BorrowAppContext, SharedString};
use std::borrow::Cow;
use std::sync::OnceLock;

pub(crate) use rust_i18n::t;

/// A UI language the user can pick. `System` defers to the operating-system
/// locale, resolved once per process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Language {
    #[default]
    System,
    English,
    ChineseSimplified,
}

impl Language {
    pub(crate) const ALL: &'static [Language] = &[
        Language::System,
        Language::English,
        Language::ChineseSimplified,
    ];

    /// Stable key persisted in `session.json`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::ChineseSimplified => "zh-CN",
        }
    }

    pub(crate) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "system" => Some(Self::System),
            "en" => Some(Self::English),
            "zh-CN" | "zh-Hans" | "zh" => Some(Self::ChineseSimplified),
            _ => None,
        }
    }

    /// Label shown in the settings dropdown, written in the language itself
    /// so it stays readable no matter which language is active.
    pub(crate) fn native_label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::English => "English",
            Self::ChineseSimplified => "简体中文",
        }
    }

    /// The rust-i18n locale `self` resolves to.
    pub(crate) fn resolved_locale(self) -> &'static str {
        match self {
            Self::System => system_locale(),
            Self::English => "en",
            Self::ChineseSimplified => "zh-CN",
        }
    }
}

/// Any Chinese locale (`zh`, `zh-CN`, `zh-Hans`, `zh_TW`, …) resolves to the
/// Simplified Chinese catalog; everything else falls back to English.
fn system_locale() -> &'static str {
    static DETECTED: OnceLock<&'static str> = OnceLock::new();
    DETECTED.get_or_init(|| match sys_locale::get_locale() {
        Some(locale) if locale.to_ascii_lowercase().starts_with("zh") => "zh-CN",
        _ => "en",
    })
}

/// Global holder for the active language, mirroring `ui_scale`'s global.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppLanguage {
    pub(crate) language: Language,
    initialized: bool,
}

impl Default for AppLanguage {
    fn default() -> Self {
        Self {
            language: Language::System,
            initialized: false,
        }
    }
}

impl gpui::Global for AppLanguage {}

pub(crate) fn current<C>(cx: &mut C) -> AppLanguage
where
    C: BorrowAppContext,
{
    cx.update_default_global::<AppLanguage, _>(|language, _cx| *language)
}

/// Seed the global from the persisted session. Safe to call from every
/// window — only the first call applies.
pub(crate) fn current_or_initialize_from_session<C>(
    ui_session: &session::UiSession,
    cx: &mut C,
) -> AppLanguage
where
    C: BorrowAppContext,
{
    let current = current(cx);
    if current.initialized {
        return current;
    }

    let language = ui_session
        .language
        .as_deref()
        .and_then(Language::from_key)
        .unwrap_or_default();
    apply(language, cx)
}

pub(crate) fn set_current<C>(cx: &mut C, language: Language) -> AppLanguage
where
    C: BorrowAppContext,
{
    apply(language, cx)
}

fn apply<C>(language: Language, cx: &mut C) -> AppLanguage
where
    C: BorrowAppContext,
{
    // rust-i18n's locale is process-global, so one call retargets every `t!`
    // lookup on every thread. Tests pin English so string assertions stay
    // deterministic on machines running any UI language.
    let locale = if cfg!(test) {
        "en"
    } else {
        language.resolved_locale()
    };
    rust_i18n::set_locale(locale);
    let next = AppLanguage {
        language,
        initialized: true,
    };
    cx.set_global(next);
    next
}

/// Translate `key` into the active language as a cheap-clone `SharedString`.
pub(crate) fn tr(key: &'static str) -> SharedString {
    t!(key).into()
}

/// Gettext-style lookup for surfaces keyed by their English source text
/// (context-menu labels): translate `text` when the active locale has an
/// entry for it, return it unchanged otherwise. English displays resolve to
/// the input because the catalogs only carry non-English locales.
pub(crate) fn tr_en(text: &str) -> SharedString {
    t!(text).into()
}

/// Same as [`tr`], for APIs that require `&'static str` (row labels, const
/// tables). Without interpolation arguments `t!` always borrows from the
/// compile-time-embedded catalogs, so the returned reference is `'static`.
pub(crate) fn tr_str(key: &'static str) -> &'static str {
    match t!(key) {
        Cow::Borrowed(text) => text,
        // Unreachable for no-argument lookups, which never allocate; kept so
        // the function stays total.
        Cow::Owned(text) => Box::leak(text.into_boxed_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_keys_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_key(language.key()), Some(*language));
        }
        assert_eq!(
            Language::from_key("zh-Hans"),
            Some(Language::ChineseSimplified)
        );
        assert_eq!(Language::from_key("zh"), Some(Language::ChineseSimplified));
        assert_eq!(Language::from_key("fr"), None);
    }

    #[test]
    fn explicit_language_resolves_to_its_own_locale() {
        assert_eq!(Language::English.resolved_locale(), "en");
        assert_eq!(Language::ChineseSimplified.resolved_locale(), "zh-CN");
    }

    #[test]
    fn system_locale_resolves_to_a_known_catalog() {
        let locale = Language::System.resolved_locale();
        assert!(locale == "en" || locale == "zh-CN");
    }

    #[test]
    fn catalogs_translate_semantic_keys() {
        assert_eq!(t!("app.language.title"), "Language");
        assert_eq!(t!("app.language.title", locale = "zh-CN"), "语言");
    }

    #[test]
    fn catalogs_translate_english_source_keys() {
        assert_eq!(t!("Checkout", locale = "zh-CN"), "检出");
        assert_eq!(t!("Delete branch", locale = "zh-CN"), "删除分支");
        // Unknown English source text falls back to itself.
        assert_eq!(
            t!("Some label never shipped", locale = "zh-CN"),
            "Some label never shipped"
        );
    }

    #[test]
    fn interpolation_uses_named_placeholders() {
        assert_eq!(
            t!(
                "app.language.system_with_resolved",
                locale = "zh-CN",
                language = "简体中文"
            ),
            "跟随系统（简体中文）"
        );
    }
}
