//! Search tuning constants.

pub(super) const FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES: usize = 32 * 1024;
pub(super) const FILE_PREVIEW_REGEX_SEARCH_WINDOW_BYTES: usize = 256 * 1024;
pub(super) const FILE_PREVIEW_REGEX_SEARCH_KEEP_BYTES: usize = 64 * 1024;
pub(super) const DIFF_SEARCH_QUERY_DEBOUNCE_MS: u64 = 150;
pub(super) const MAX_UTF8_CHAR_BYTES: usize = 4;
pub(super) const DIFF_SEARCH_TRIGRAM_MIN_QUERY_BYTES: usize = 3;
/// Ceiling on the editor's match list. The other views are bounded by their row
/// count; this one stores a range per *occurrence*, so a query like the regex `.`
/// would otherwise grow one per byte.
pub(super) const FILE_EDITOR_SEARCH_MAX_MATCHES: usize = 20_000;
