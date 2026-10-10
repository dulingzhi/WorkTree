//! UI language support (i18n).
//!
//! Translations live in `locales/*.en.yml` / `locales/*.zh-CN.yml` next to the
//! crate manifest and are embedded into the binary at compile time by
//! `rust_i18n::i18n!` (invoked in `lib.rs`, which is where the generated
//! lookup functions must land). Call sites translate through the re-exported
//! [`t!`] macro or the [`tr`] helper, both of which follow the active
//! [`Language`] held in the [`AppLanguage`] gpui global — the same shape as
//! `ui_scale`'s global.

use gpui::{BorrowAppContext, SharedString};
use std::borrow::Cow;
use std::sync::OnceLock;
use worktree_state::session;

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

/// Catalogue-coverage guards.
///
/// The English catalogue is the source of truth: every key it defines must be
/// translated in the other catalogues, and every semantic key a call site asks
/// for must exist in it. Both failures are silent at runtime — a missing key
/// renders as the raw key — so they are asserted here instead.
///
/// Gettext-style keys (English source text used as its own key, e.g.
/// `tr("Checkout")`) have no English entry by design: they fall back to
/// themselves. Only dotted lowercase identifiers are therefore checked.
#[cfg(test)]
mod catalog_tests {
    use rust_i18n::Backend as _;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    /// Every `key -> value` pair the loader embedded for `locale`.
    fn catalog(locale: &str) -> BTreeMap<String, String> {
        let entries = crate::_rust_i18n_backend()
            .messages_for_locale(locale)
            .unwrap_or_else(|| panic!("no `{locale}` catalogue was embedded"));
        entries
            .into_iter()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect()
    }

    fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_source_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Every `"…"` literal handed to `tr(` / `tr_str(` / `t!(`, with the byte
    /// offset it starts at.
    fn key_literals(text: &str) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for call in ["tr(", "tr_str(", "t!("] {
            let mut from = 0;
            while let Some(rel) = text[from..].find(call) {
                let after_call = from + rel + call.len();
                let rest = &text[after_call..];
                let trimmed = rest.trim_start();
                // Anything but a `"` (an interpolated key, a call that takes
                // something else): resume just past the call so a nested hit
                // on the same line still matches.
                let Some(after_quote) = trimmed
                    .strip_prefix('"')
                    .and_then(|key| key.find('"').map(|end| (key, end)))
                else {
                    from = after_call;
                    continue;
                };
                let (key, end) = after_quote;
                let literal = key[..end].to_string();
                from = after_call + (rest.len() - trimmed.len()) + 1 + end + 1;
                out.push((literal, after_call));
            }
        }
        out
    }

    /// Semantic keys are dotted lowercase identifiers such as
    /// `ui.placeholder.mr_push_target`; English source text used as its own
    /// key is gettext style and has no English entry by design.
    fn is_semantic_key(key: &str) -> bool {
        key.contains('.')
            && !key.contains(char::is_whitespace)
            && key.split('.').all(|segment| {
                !segment.is_empty()
                    && segment
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
    }

    fn line_of(text: &str, offset: usize) -> usize {
        text[..offset].matches('\n').count() + 1
    }

    /// `key -> first place it is asked for`, for failure messages.
    fn referenced_keys() -> BTreeMap<String, String> {
        let mut files = Vec::new();
        collect_source_files(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut out = BTreeMap::new();
        for path in files {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (key, offset) in key_literals(&text) {
                if is_semantic_key(&key) {
                    out.entry(key).or_insert_with(|| {
                        format!("{}:{}", path.display(), line_of(&text, offset))
                    });
                }
            }
        }
        out
    }

    #[test]
    fn both_catalogues_are_embedded() {
        let locales = crate::_rust_i18n_backend().available_locales();
        let locales: Vec<&str> = locales.iter().map(|locale| locale.as_ref()).collect();
        assert!(locales.contains(&"en"), "loaded locales: {locales:?}");
        assert!(locales.contains(&"zh-CN"), "loaded locales: {locales:?}");
    }

    #[test]
    fn every_english_key_is_translated() {
        let english = catalog("en");
        let chinese = catalog("zh-CN");
        assert!(!english.is_empty(), "the English catalogue is empty");
        let missing: Vec<&String> = english
            .keys()
            .filter(|key| !chinese.contains_key(*key))
            .collect();
        assert!(
            missing.is_empty(),
            "zh-CN is missing {} translation(s): {missing:#?}",
            missing.len()
        );
    }

    #[test]
    fn every_semantic_key_a_call_site_asks_for_exists() {
        let english = catalog("en");
        let referenced = referenced_keys();
        // A scanner that quietly stops seeing the tree is worse than no
        // scanner at all: fail on a suspiciously small result.
        assert!(
            referenced.len() > 1000,
            "the key scanner only found {} key(s) under src/",
            referenced.len()
        );
        let missing: Vec<(&String, &String)> = referenced
            .iter()
            .filter(|(key, _)| !english.contains_key(*key))
            .collect();
        assert!(
            missing.is_empty(),
            "{} call site(s) ask for a key the English catalogue does not define: {missing:#?}",
            missing.len()
        );
    }

    #[test]
    fn english_catalogue_has_no_empty_values() {
        let english = catalog("en");
        let empty: Vec<&String> = english
            .iter()
            .filter(|(_, value)| value.trim().is_empty())
            .map(|(key, _)| key)
            .collect();
        assert!(
            empty.is_empty(),
            "{} English value(s) are empty: {empty:#?}",
            empty.len()
        );
    }
}
