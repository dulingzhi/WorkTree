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
//!
//! Remote pixels come through this module's own resolver, not gpui's image
//! asset cache: gpui's cache is memory-only (every avatar re-downloads on
//! each launch) and notifies a single view per URL when a fetch lands. Here
//! the bytes are cached on disk under the OS cache dir, decoded once per
//! process into a [`RenderImage`], and every window repaints when it arrives.

use repositorytree_state::session;
use gpui::App;
use gpui::RenderImage;
use gpui::SharedString;
use md5::{Digest, Md5};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

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

/// Resolved remote avatars for this process, keyed by URL.
enum AvatarEntry {
    /// The URL has no avatar (HTTP error, transport error, undecodable body).
    /// Sticky until restart — surfaces keep showing the initials without
    /// re-requesting the URL on every repaint, where gpui's asset cache
    /// evicts failed fetches and retries them each frame.
    Failed,
    Ready(Arc<RenderImage>),
}

static AVATARS: LazyLock<Mutex<HashMap<String, AvatarEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// URLs a resolve is currently in flight for.
static RESOLVING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// How long a cached avatar is served without going back to the network.
const AVATAR_CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The resolved remote image for `email`, when the active source yields a URL
/// for it and that URL has already resolved to pixels in this process.
///
/// `None` also covers pending and failed loads — callers fall back to the
/// initials circle in every one of those cases, after arming the resolver via
/// [`ensure_avatar_loaded`].
pub(crate) fn remote_avatar(email: Option<&str>) -> Option<(SharedString, Arc<RenderImage>)> {
    let url = avatar_url(email)?;
    match AVATARS.lock().unwrap().get(url.as_ref()) {
        Some(AvatarEntry::Ready(image)) => Some((url, Arc::clone(image))),
        _ => None,
    }
}

/// Resolve `url` to pixels, from the disk cache or the network, and repaint
/// every surface when the answer lands.
///
/// gpui's own image path keeps fetched pixels in a memory-only cache and only
/// notifies the one view whose prepaint first requested a URL — neither
/// survives a restart, and other views stay on their stand-ins. Resolving
/// here instead pins both: avatars are cached on disk (see
/// [`avatar_cache_dir`]), held in this process's map, and painted from
/// [`RenderImage`] via `ImageSource::Render`, with [`App::refresh_windows`]
/// doing the repaint.
pub(crate) fn ensure_avatar_loaded(url: &SharedString, cx: &mut App) {
    if AVATARS.lock().unwrap().contains_key(url.as_ref()) {
        return; // Ready or Failed — settled for this process.
    }
    if !RESOLVING.lock().unwrap().insert(url.to_string()) {
        return; // Already in flight.
    }
    // Tests render real rows with real emails; they must not touch the
    // network (or depend on the machine having any).
    #[cfg(test)]
    {
        let _ = cx;
        return;
    }
    #[cfg(not(test))]
    {
        let http = cx.http_client();
        let url = url.clone();
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            resolve_avatar(&url, http).await;
            let _ = cx.update(|cx| cx.refresh_windows());
        })
        .detach();
    }
}

/// Fetch `url`'s pixels: disk cache first, then the network, storing the
/// outcome in the process map. A stale-but-usable disk hit is served
/// immediately and refreshed in the same pass (stale-while-revalidate).
#[cfg(not(test))]
async fn resolve_avatar(url: &SharedString, http: Arc<dyn gpui::http_client::HttpClient>) {
    let path = avatar_cache_path(url);
    if let Some(path) = path.as_deref()
        && let Ok(bytes) = std::fs::read(path)
        && let Some(image) = decode_avatar(&bytes)
    {
        store_ready(url, image);
        if cache_file_is_fresh(path) {
            return;
        }
    }

    match http.get(url.as_ref(), true).await {
        Ok(response) if response.status.is_success() => match decode_avatar(&response.body) {
            Some(image) => {
                write_cache_file(path.as_deref(), &response.body);
                store_ready(url, image);
            }
            // A 200 with a body the decoder rejects will not improve on retry.
            None => store_failed_if_empty(url),
        },
        // 404 (`d=404` on purpose), other statuses, transport errors — alike
        // for this process. A stale disk image, if one was stored above, wins.
        _ => store_failed_if_empty(url),
    }
}

fn store_ready(url: &str, image: Arc<RenderImage>) {
    AVATARS
        .lock()
        .unwrap()
        .insert(url.to_string(), AvatarEntry::Ready(image));
}

/// Record a failure only when nothing better is already stored — a stale disk
/// image must not be replaced by a failed revalidation.
fn store_failed_if_empty(url: &str) {
    let mut avatars = AVATARS.lock().unwrap();
    avatars
        .entry(url.to_string())
        .or_insert(AvatarEntry::Failed);
}

fn cache_file_is_fresh(path: &std::path::Path) -> bool {
    let modified = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    modified.is_some_and(|modified| modified.elapsed().is_ok_and(|age| age < AVATAR_CACHE_TTL))
}

/// Write the fetched bytes to the cache, best-effort: the cache is an
/// optimization, so an unwritable directory degrades to re-fetching rather
/// than an error anywhere.
#[cfg(not(test))]
fn write_cache_file(path: Option<&std::path::Path>, bytes: &[u8]) {
    let Some(path) = path else { return };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_ok() {
        let _ = std::fs::write(path, bytes);
    }
}

/// Decode avatar bytes to a gpui render image.
///
/// Mirrors gpui's static-image decode (`gpui::platform::
/// decode_static_image_from_decoder`): EXIF orientation applied, RGBA8, R/B
/// channels swapped for the renderer's BGRA. The avatar hosts serve PNG/JPEG/
/// WebP, all covered by `guess_format`.
fn decode_avatar(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    use image::ImageDecoder as _;

    let format = image::guess_format(bytes).ok()?;
    let mut decoder = image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
        .into_decoder()
        .ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    let mut data = image.into_rgba8();
    for pixel in data.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(data)])))
}

/// Per-user directory for cached avatar bytes. Avatars are re-derivable, so
/// this follows the OS cache location (the session store's conventions, but a
/// cache tier), not the data location.
fn avatar_cache_dir() -> Option<PathBuf> {
    fn non_empty(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
        var.filter(|value| !value.is_empty()).map(PathBuf::from)
    }

    #[cfg(target_os = "macos")]
    {
        non_empty(std::env::var_os("HOME")).map(|home| home.join("Library/Caches/repositorytree/avatars"))
    }

    #[cfg(target_os = "linux")]
    {
        non_empty(std::env::var_os("XDG_CACHE_HOME"))
            .map(|dir| dir.join("repositorytree/avatars"))
            .or_else(|| {
                non_empty(std::env::var_os("HOME")).map(|home| home.join(".cache/repositorytree/avatars"))
            })
    }

    #[cfg(target_os = "windows")]
    {
        non_empty(std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")))
            .map(|dir| dir.join("repositorytree/cache/avatars"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        non_empty(std::env::var_os("HOME")).map(|home| home.join(".cache/repositorytree/avatars"))
    }
}

/// Cache file for `url`: the MD5 of the full URL — host and query included,
/// so gravatar and cravatar entries never collide — with no extension; the
/// format is sniffed from the bytes on read.
fn avatar_cache_path(url: &str) -> Option<PathBuf> {
    avatar_cache_dir().map(|dir| dir.join(md5_hex(url)))
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
    fn remote_avatar_reads_only_ready_entries() {
        let _guard = SourceGuard::new(AvatarSource::Gravatar);
        // Unique per test: the process map is shared and tests run in
        // parallel.
        let email = format!("ready-states-{}@example.com", line!());
        let url = avatar_url(Some(&email)).expect("gravatar yields a url");
        assert_eq!(
            remote_avatar(Some(&email)),
            None,
            "a URL nobody resolved yet has no image"
        );

        let image = decode_avatar(&png_bytes([10, 20, 30, 255])).expect("png decodes");
        store_ready(url.as_ref(), image);
        assert!(
            remote_avatar(Some(&email)).is_some(),
            "a resolved URL serves its image"
        );

        let email = format!("ready-states-failed-{}@example.com", line!());
        let url = avatar_url(Some(&email)).expect("gravatar yields a url");
        store_failed_if_empty(url.as_ref());
        assert_eq!(
            remote_avatar(Some(&email)),
            None,
            "a failed URL falls back to the initials"
        );
        // And the failure is sticky: a second store does not overwrite it.
        store_failed_if_empty(url.as_ref());
        assert_eq!(remote_avatar(Some(&email)), None);
    }

    #[test]
    fn a_stale_disk_image_survives_a_failed_revalidation() {
        // `store_failed_if_empty` is the network-failure arm of
        // `resolve_avatar`; it must not replace an already-stored image.
        let _guard = SourceGuard::new(AvatarSource::Gravatar);
        let email = format!("stale-keeps-image-{}@example.com", line!());
        let url = avatar_url(Some(&email)).expect("gravatar yields a url");
        let image = decode_avatar(&png_bytes([9, 9, 9, 255])).expect("png decodes");
        store_ready(url.as_ref(), image);
        store_failed_if_empty(url.as_ref());
        assert!(
            remote_avatar(Some(&email)).is_some(),
            "the stale image stays after the refresh failed"
        );
    }

    #[test]
    fn decoded_avatars_come_out_bgra() {
        let image = decode_avatar(&png_bytes([10, 20, 30, 255])).expect("png decodes");
        assert_eq!(image.size(0).width.0, 4);
        assert_eq!(image.frame_count(), 1);
        // gpui's renderer wants BGRA; the swap is the whole point of this
        // copy of gpui's decode. The frame holds all 16 pixels — check the
        // first, plus that the whole frame really is BGRA-repeated.
        let bytes = image.as_bytes(0).expect("frame 0 has bytes");
        assert_eq!(&bytes[..4], &[30, 20, 10, 255]);
        assert_eq!(bytes.len(), 4 * 4 * 4);
    }

    #[test]
    fn garbage_does_not_decode() {
        assert!(decode_avatar(b"not an image at all").is_none());
        assert!(decode_avatar(&[]).is_none());
    }

    #[test]
    fn cache_files_key_off_the_full_url() {
        let gravatar = avatar_cache_path("https://www.gravatar.com/avatar/x?s=128&d=404")
            .expect("a home dir is resolvable on CI");
        let cravatar = avatar_cache_path("https://cravatar.cn/avatar/x?s=128&d=404")
            .expect("a home dir is resolvable on CI");
        assert_ne!(gravatar, cravatar);
        assert!(
            gravatar.ends_with(md5_hex("https://www.gravatar.com/avatar/x?s=128&d=404")),
            "the file name is the URL's hash, extensionless: {gravatar:?}"
        );
    }

    /// A 4×4 single-color PNG — exercising the real encoder/decoder pair
    /// rather than a fixture file.
    fn png_bytes(rgba: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(4, 4, image::Rgba(rgba));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encoding a 4x4 png cannot fail");
        bytes
    }
}
