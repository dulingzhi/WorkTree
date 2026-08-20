//! Commit-author avatar sourcing.
//!
//! Author avatars are either the built-in initials circles or remote images
//! from Gravatar (or its Chinese mirror, Cravatar), addressed by the MD5 of
//! the author's email. Which one renders is a user setting held in a
//! process-global here — unlike `i18n`/`ui_scale` it is read from plain
//! render helpers and canvas closures that have no `cx` handy, so it follows
//! rust-i18n's locale shape instead of a gpui global.
//!
//! Privacy: `Initials` (the default) never touches the network. Switching to
//! `Gravatar`/`Cravatar` sends the MD5-hashed author email of every rendered
//! commit to the chosen service; `d=404` keeps missing avatars a clean
//! fallback to the initials instead of a placeholder image.

use gitcomet_state::session;
use gpui::App;
use gpui::SharedString;
use md5::{Digest, Md5};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

/// Requested pixel size. Avatars render at 16–32 px, so one generous size
/// serves every site at 2x sharpness while staying a tiny download.
const AVATAR_REQUEST_PX: u32 = 128;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum AvatarSource {
    /// Built-in initials circle; no network.
    #[default]
    Initials,
    /// `www.gravatar.com`.
    Gravatar,
    /// `cravatar.cn` (Gravatar mirror).
    Cravatar,
}

impl AvatarSource {
    pub(crate) const ALL: &'static [AvatarSource] = &[
        AvatarSource::Initials,
        AvatarSource::Gravatar,
        AvatarSource::Cravatar,
    ];

    /// Stable key persisted in `session.json`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Initials => "initials",
            Self::Gravatar => "gravatar",
            Self::Cravatar => "cravatar",
        }
    }

    pub(crate) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "initials" => Some(Self::Initials),
            "gravatar" => Some(Self::Gravatar),
            "cravatar" => Some(Self::Cravatar),
            _ => None,
        }
    }

    /// Label shown in the settings dropdown.
    pub(crate) fn label(self) -> SharedString {
        match self {
            Self::Initials => crate::i18n::tr("settings.avatar_source.option_initials"),
            Self::Gravatar => crate::i18n::tr("settings.avatar_source.option_gravatar"),
            Self::Cravatar => crate::i18n::tr("settings.avatar_source.option_cravatar"),
        }
    }

    fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::Gravatar,
            2 => Self::Cravatar,
            _ => Self::Initials,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// The active source. Safe to call from any thread, including canvas paint
/// closures.
pub(crate) fn current() -> AvatarSource {
    AvatarSource::from_u8(CURRENT.load(Ordering::Relaxed))
}

pub(crate) fn set_current(source: AvatarSource) {
    CURRENT.store(source as u8, Ordering::Relaxed);
    INITIALIZED.store(true, Ordering::Relaxed);
}

/// Seed the global from the persisted session. Safe to call from every
/// window — only the first call applies.
pub(crate) fn init_from_session(ui_session: &session::UiSession) {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    let source = ui_session
        .avatar_source
        .as_deref()
        .and_then(AvatarSource::from_key)
        .unwrap_or_default();
    CURRENT.store(source as u8, Ordering::Relaxed);
}

/// Gravatar hashes the email trimmed and lowercased. Borrows when the input
/// already matches (the common case), copies only for mixed-case emails.
fn normalize_email_for_hash(email: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = email.trim();
    if trimmed.chars().any(|c| c.is_ascii_uppercase()) {
        trimmed.to_ascii_lowercase().into()
    } else {
        trimmed.into()
    }
}

/// Remote avatar URL for `email` under the active source, or `None` when the
/// source is `Initials` or the email is empty/blank — callers fall back to
/// the initials circle in that case.
pub(crate) fn avatar_url(email: Option<&str>) -> Option<SharedString> {
    let email = normalize_email_for_hash(email?);
    if email.is_empty() {
        return None;
    }
    let host = match current() {
        AvatarSource::Initials => return None,
        AvatarSource::Gravatar => "https://www.gravatar.com/avatar",
        AvatarSource::Cravatar => "https://cravatar.cn/avatar",
    };
    Some(
        format!(
            "{host}/{}?s={AVATAR_REQUEST_PX}&d=404",
            md5_hex(email.as_ref())
        )
        .into(),
    )
}

fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let mut hex = String::with_capacity(32);
    for byte in hasher.finalize() {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Avatar URLs a load-watcher has been attached to, settled or in flight.
static WATCHED_AVATAR_URLS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Claim `url` for watching; `false` when a watcher already exists.
fn claim_watch(url: &str) -> bool {
    WATCHED_AVATAR_URLS.lock().unwrap().insert(url.to_string())
}

/// Make sure `url`'s arrival repaints every surface that shows it.
///
/// gpui redraws only the single view whose prepaint first requested a given
/// remote image once its fetch completes — `Window::use_asset` captures that
/// one view and `cx.notify`s it when the task lands. Every other view showing
/// the same URL keeps its stale frame, which is why the history list stayed on
/// the loading initials after the avatar downloaded. Watching each URL once
/// here and refreshing all windows when the fetch settles repaints every
/// surface; the claimed set keeps it to one watcher per URL for the process.
pub(crate) fn ensure_avatar_loaded(url: &SharedString, cx: &mut App) {
    if !claim_watch(url) {
        return;
    }
    // Routed through gpui's asset cache, so the img elements' own `use_asset`
    // calls join this exact task instead of starting parallel fetches.
    let (load, _is_first) = cx.fetch_asset::<gpui::ImgResourceLoader>(&gpui::Resource::Uri(
        gpui::SharedUri::from(url.to_string()),
    ));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        // Success and 404 alike: either way the surfaces have their final
        // state, and this URL never needs a watcher again.
        let _ = load.await;
        let _ = cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restore the process-global after a test mutates it — parallel tests
    /// in this module are the only writers.
    struct SourceGuard(AvatarSource);

    impl SourceGuard {
        fn new(source: AvatarSource) -> Self {
            set_current(source);
            Self(current())
        }
    }

    impl Drop for SourceGuard {
        fn drop(&mut self) {
            set_current(self.0);
        }
    }

    #[test]
    fn avatar_source_keys_round_trip() {
        for source in AvatarSource::ALL {
            assert_eq!(AvatarSource::from_key(source.key()), Some(*source));
        }
        assert_eq!(AvatarSource::from_key("github"), None);
    }

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(md5_hex("test"), "098f6bcd4621d373cade4e832627b4f6");
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn email_normalizes_to_trimmed_lowercase() {
        assert_eq!(
            normalize_email_for_hash(" User@Example.COM ").as_ref(),
            "user@example.com"
        );
        assert_eq!(normalize_email_for_hash("a@b.c").as_ref(), "a@b.c");
    }

    #[test]
    fn initials_source_and_missing_email_have_no_url() {
        let _guard = SourceGuard::new(AvatarSource::Gravatar);
        assert_eq!(avatar_url(None), None);
        assert_eq!(avatar_url(Some("")), None);
        assert_eq!(avatar_url(Some("   ")), None);
        let _guard = SourceGuard::new(AvatarSource::Initials);
        assert_eq!(avatar_url(Some("user@example.com")), None);
    }

    #[test]
    fn hosts_build_gravatar_style_urls() {
        // The mixed-case input proves normalization feeds the hash; md5_hex
        // itself is pinned by `md5_matches_known_vector` above.
        let expected_hash = md5_hex("user@example.com");
        let _guard = SourceGuard::new(AvatarSource::Gravatar);
        assert_eq!(
            avatar_url(Some("User@Example.COM ")).as_deref(),
            Some(format!("https://www.gravatar.com/avatar/{expected_hash}?s=128&d=404").as_str())
        );
        let _guard = SourceGuard::new(AvatarSource::Cravatar);
        let expected_hash = md5_hex("a@b.c");
        assert_eq!(
            avatar_url(Some("a@b.c")).as_deref(),
            Some(format!("https://cravatar.cn/avatar/{expected_hash}?s=128&d=404").as_str())
        );
    }

    #[test]
    fn a_url_is_claimed_for_watching_only_once() {
        // Unique per test: the claimed set is process-global and tests in
        // this module run in parallel.
        let first = format!(
            "https://www.gravatar.com/avatar/watched-once-{}.md5",
            line!()
        );
        assert!(claim_watch(&first));
        assert!(!claim_watch(&first));
        let second = format!(
            "https://www.gravatar.com/avatar/watched-once-{}.md5",
            line!()
        );
        assert!(claim_watch(&second));
    }
}
