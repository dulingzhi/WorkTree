//! The conflict text model: shared-storage `ConflictText`, conflict blocks
//! and segments, plus the line-scanning helpers every domain reuses.

use super::*;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConflictTextStorage {
    Owned(String),
    SharedSlice { text: Arc<str>, range: Range<usize> },
}

#[derive(Clone, Debug)]
pub struct ConflictText {
    pub(super) storage: ConflictTextStorage,
}

impl ConflictText {
    pub fn shared(text: Arc<str>) -> Self {
        let len = text.len();
        Self {
            storage: ConflictTextStorage::SharedSlice {
                text,
                range: 0..len,
            },
        }
    }

    pub fn shared_slice(text: Arc<str>, range: Range<usize>) -> Self {
        debug_assert!(
            text.get(range.clone()).is_some(),
            "shared conflict text range should stay within bounds"
        );
        Self {
            storage: ConflictTextStorage::SharedSlice { text, range },
        }
    }

    pub fn as_str(&self) -> &str {
        match &self.storage {
            ConflictTextStorage::Owned(text) => text.as_str(),
            ConflictTextStorage::SharedSlice { text, range } => text
                .get(range.clone())
                .expect("shared conflict text range should stay valid"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    pub fn len(&self) -> usize {
        self.as_str().len()
    }

    pub fn push_str(&mut self, suffix: &str) {
        if suffix.is_empty() {
            return;
        }

        match &mut self.storage {
            ConflictTextStorage::Owned(text) => text.push_str(suffix),
            ConflictTextStorage::SharedSlice { .. } => {
                let mut owned = self.as_str().to_string();
                owned.push_str(suffix);
                self.storage = ConflictTextStorage::Owned(owned);
            }
        }
    }

    pub fn into_owned_string(self) -> String {
        match self.storage {
            ConflictTextStorage::Owned(text) => text,
            ConflictTextStorage::SharedSlice { text, range } => text
                .get(range)
                .expect("shared conflict text range should stay valid")
                .to_string(),
        }
    }

    #[cfg(test)]
    pub(in crate::view) fn shares_backing_with(&self, other: &Arc<str>) -> bool {
        match &self.storage {
            ConflictTextStorage::Owned(_) => false,
            ConflictTextStorage::SharedSlice { text, .. } => Arc::ptr_eq(text, other),
        }
    }
}

impl Default for ConflictText {
    fn default() -> Self {
        String::new().into()
    }
}

impl std::fmt::Display for ConflictText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::ops::Deref for ConflictText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for ConflictText {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for ConflictText {
    fn from(value: String) -> Self {
        Self::shared(Arc::from(value))
    }
}

impl From<&str> for ConflictText {
    fn from(value: &str) -> Self {
        Self::shared(Arc::from(value))
    }
}

impl PartialEq for ConflictText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ConflictText {}

impl PartialEq<&str> for ConflictText {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<ConflictText> for &str {
    fn eq(&self, other: &ConflictText) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<String> for ConflictText {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<ConflictText> for String {
    fn eq(&self, other: &ConflictText) -> bool {
        self.as_str() == other.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictBlock {
    pub base: Option<ConflictText>,
    pub ours: ConflictText,
    pub theirs: ConflictText,
    pub choice: ConflictChoice,
    /// Whether this block has been explicitly resolved (by user pick or auto-resolve).
    /// Blocks start unresolved; becomes `true` when the user picks a side or auto-resolve runs.
    pub resolved: bool,
    /// Whether every aligned row in this block differs only in whitespace
    /// (kdiff3 `MergeBlock::bWhiteSpaceConflict`).
    ///
    /// Set from the merge plan's classification, which applies kdiff3's
    /// per-row rule; marker-only sessions have no aligned rows to classify and
    /// leave this `false`. Drives the `(Whitespace only)` placeholder variant.
    pub whitespace_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConflictSegment {
    Text(ConflictText),
    Block(ConflictBlock),
}

#[cfg(any(test, feature = "benchmarks"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConflictInlineRow {
    pub side: ConflictPickSide,
    pub kind: worktree_core::domain::DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
}

#[cfg(any(test, feature = "benchmarks"))]
pub(super) fn text_line_count(text: &str) -> u32 {
    u32::try_from(text_line_count_usize(text)).unwrap_or(u32::MAX)
}

pub(super) fn text_line_count_usize(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let bytes = text.as_bytes();
    let newline_count = memchr::memchr_iter(b'\n', bytes).count();
    if bytes.last() == Some(&b'\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

pub(super) fn indexed_line_count(text: &str, line_starts: &[usize]) -> usize {
    if text.is_empty() {
        0
    } else {
        line_starts.len()
    }
}

pub(super) fn indexed_line_text<'a>(
    text: &'a str,
    line_starts: &[usize],
    line_ix: usize,
) -> Option<&'a str> {
    if text.is_empty() {
        return None;
    }
    let text_len = text.len();
    let start = line_starts.get(line_ix).copied().unwrap_or(text_len);
    if start >= text_len {
        return None;
    }
    let mut end = line_starts
        .get(line_ix.saturating_add(1))
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    if end > start && text.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n') {
        end = end.saturating_sub(1);
    }
    Some(text.get(start..end).unwrap_or(""))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TextLineStats {
    pub(super) line_count: usize,
    pub(super) widest_line_ix: usize,
    pub(super) widest_line_len: usize,
}

impl TextLineStats {
    pub(super) fn widest_line(self) -> Option<(usize, usize)> {
        (self.line_count > 0).then_some((self.widest_line_ix, self.widest_line_len))
    }
}

pub(super) fn scan_text_line_stats(text: &str) -> TextLineStats {
    if text.is_empty() {
        return TextLineStats::default();
    }

    let bytes = text.as_bytes();
    let mut line_count = 0usize;
    let mut prev_pos = 0usize;
    let mut widest_line_ix = 0usize;
    let mut widest_line_len = 0usize;

    for pos in memchr::memchr_iter(b'\n', bytes) {
        let line_len = pos - prev_pos;
        if line_len > widest_line_len {
            widest_line_len = line_len;
            widest_line_ix = line_count;
        }
        line_count += 1;
        prev_pos = pos + 1;
    }

    // Handle last line (no trailing newline).
    if prev_pos < bytes.len() {
        let line_len = bytes.len() - prev_pos;
        if line_len > widest_line_len {
            widest_line_len = line_len;
            widest_line_ix = line_count;
        }
        line_count += 1;
    }

    TextLineStats {
        line_count,
        widest_line_ix,
        widest_line_len,
    }
}

/// Extract a single line from text using pre-computed line starts.
pub(super) fn line_text_from_starts<'a>(
    text: &'a str,
    line_starts: &[usize],
    line_ix: usize,
) -> &'a str {
    let text_len = text.len();
    let start = line_starts
        .get(line_ix)
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    let end = line_starts
        .get(line_ix + 1)
        .copied()
        .unwrap_or(text_len)
        .min(text_len);
    if start >= end {
        return "";
    }
    let slice = text.get(start..end).unwrap_or("");
    slice.strip_suffix('\n').unwrap_or(slice)
}
