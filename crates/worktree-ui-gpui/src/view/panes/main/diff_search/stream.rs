//! Streamed line-text search backends (literal and regex) and UTF-8 decoding.

use super::consts::{
    FILE_PREVIEW_REGEX_SEARCH_KEEP_BYTES, FILE_PREVIEW_REGEX_SEARCH_WINDOW_BYTES,
    FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES, MAX_UTF8_CHAR_BYTES,
};
use super::row_text::{
    StreamMatchCollectionMode, build_literal_search_prefix_table, folded_search_byte,
    next_row_start_after_stream_abs, normalized_file_diff_line_text_len,
    normalized_stream_row_text, visible_ix_for_stream_abs,
};

use crate::kit::text_search::DiffSearchMatcher;
use crate::kit::text_search::is_word_char;
use crate::kit::text_search::next_char_boundary_after;
use std::borrow::Cow;
use std::collections::VecDeque;
struct LiteralFileDiffLineTextStreamSearch {
    needle: Vec<u8>,
    prefix: Vec<usize>,
    match_case: bool,
    whole_word: bool,
    matched: usize,
    stream_abs: usize,
    recent_bytes: VecDeque<u8>,
    pending_whole_word_start: Option<usize>,
    pending_whole_word_after: Vec<u8>,
    row_starts: Vec<(usize, usize)>,
    last_reported_visible_ix: Option<usize>,
}

impl LiteralFileDiffLineTextStreamSearch {
    fn new(matcher: &DiffSearchMatcher) -> Option<Self> {
        if matcher.options().regex || matcher.query().is_empty() {
            return None;
        }

        let needle = matcher
            .query()
            .as_bytes()
            .iter()
            .copied()
            .map(|byte| folded_search_byte(byte, matcher.options().match_case))
            .collect::<Vec<_>>();
        let prefix = build_literal_search_prefix_table(needle.as_slice());

        Some(Self {
            needle,
            prefix,
            match_case: matcher.options().match_case,
            whole_word: matcher.options().whole_word,
            matched: 0,
            stream_abs: 0,
            recent_bytes: VecDeque::with_capacity(
                matcher.query().len().saturating_add(MAX_UTF8_CHAR_BYTES),
            ),
            pending_whole_word_start: None,
            pending_whole_word_after: Vec::with_capacity(MAX_UTF8_CHAR_BYTES),
            row_starts: Vec::new(),
            last_reported_visible_ix: None,
        })
    }

    fn has_rows(&self) -> bool {
        !self.row_starts.is_empty()
    }

    fn push_row_start(&mut self, visible_ix: usize) {
        self.row_starts.push((self.stream_abs, visible_ix));
    }

    fn row_reported(&self, visible_ix: usize) -> bool {
        self.last_reported_visible_ix == Some(visible_ix)
    }

    fn report_match_start(&mut self, match_start: usize, out: &mut Vec<usize>) {
        let Some(visible_ix) = visible_ix_for_stream_abs(self.row_starts.as_slice(), match_start)
        else {
            return;
        };
        if self.last_reported_visible_ix == Some(visible_ix) {
            return;
        }
        out.push(visible_ix);
        self.last_reported_visible_ix = Some(visible_ix);
    }

    fn finish_pending_whole_word(&mut self, out: &mut Vec<usize>) {
        let Some(match_start) = self.pending_whole_word_start else {
            return;
        };
        match decode_first_utf8_char(self.pending_whole_word_after.as_slice()) {
            Utf8CharDecode::Complete(ch) => {
                self.pending_whole_word_start = None;
                self.pending_whole_word_after.clear();
                if !is_word_char(ch) {
                    self.report_match_start(match_start, out);
                }
            }
            Utf8CharDecode::Invalid => {
                self.pending_whole_word_start = None;
                self.pending_whole_word_after.clear();
                self.report_match_start(match_start, out);
            }
            Utf8CharDecode::Incomplete => {}
        }
    }

    fn push_byte(&mut self, byte: u8, out: &mut Vec<usize>) {
        if self.pending_whole_word_start.is_some() {
            self.pending_whole_word_after.push(byte);
            self.finish_pending_whole_word(out);
        }

        let folded = folded_search_byte(byte, self.match_case);
        while self.matched > 0 && folded != self.needle[self.matched] {
            self.matched = self.prefix[self.matched - 1];
        }
        if folded == self.needle[self.matched] {
            self.matched += 1;
        }

        self.recent_bytes.push_back(byte);
        while self.recent_bytes.len() > self.needle.len().saturating_add(MAX_UTF8_CHAR_BYTES) {
            self.recent_bytes.pop_front();
        }

        if self.matched == self.needle.len() {
            let match_end = self.stream_abs.saturating_add(1);
            let match_start = match_end.saturating_sub(self.needle.len());
            let before_is_word = if match_start == 0 {
                false
            } else {
                let recent_bytes = self.recent_bytes.make_contiguous();
                let before_end = recent_bytes.len().saturating_sub(self.needle.len());
                trailing_utf8_char_is_word(&recent_bytes[..before_end])
            };

            if !self.whole_word || !before_is_word {
                if self.whole_word {
                    self.pending_whole_word_start = Some(match_start);
                    self.pending_whole_word_after.clear();
                } else {
                    self.report_match_start(match_start, out);
                }
            }

            self.matched = self.prefix[self.needle.len() - 1];
        }

        self.stream_abs = self.stream_abs.saturating_add(1);
    }

    fn push_bytes(&mut self, bytes: &[u8], out: &mut Vec<usize>) {
        for &byte in bytes {
            self.push_byte(byte, out);
        }
    }

    fn skip_bytes_until_next_row(&mut self, byte_count: usize) {
        self.stream_abs = self.stream_abs.saturating_add(byte_count);
        self.matched = 0;
        self.pending_whole_word_start = None;
        self.pending_whole_word_after.clear();
        self.recent_bytes.clear();
    }

    fn finish(&mut self, out: &mut Vec<usize>) {
        if let Some(match_start) = self.pending_whole_word_start.take() {
            self.pending_whole_word_after.clear();
            self.report_match_start(match_start, out);
        }
    }
}

enum Utf8CharDecode {
    Complete(char),
    Incomplete,
    Invalid,
}

fn decode_first_utf8_char(bytes: &[u8]) -> Utf8CharDecode {
    let Some(&first) = bytes.first() else {
        return Utf8CharDecode::Incomplete;
    };

    if first.is_ascii() {
        return Utf8CharDecode::Complete(char::from(first));
    }

    let expected_len = match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return Utf8CharDecode::Invalid,
    };
    if bytes.len() < expected_len {
        return Utf8CharDecode::Incomplete;
    }

    match std::str::from_utf8(&bytes[..expected_len]) {
        Ok(text) => match text.chars().next() {
            Some(ch) if ch.len_utf8() == expected_len => Utf8CharDecode::Complete(ch),
            _ => Utf8CharDecode::Invalid,
        },
        Err(_) => Utf8CharDecode::Invalid,
    }
}

fn trailing_utf8_char_is_word(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    let start = bytes.len().saturating_sub(MAX_UTF8_CHAR_BYTES);
    let tail = &bytes[start..];
    for offset in 0..tail.len() {
        if let Utf8CharDecode::Complete(ch) = decode_first_utf8_char(&tail[offset..])
            && ch.len_utf8() == tail.len() - offset
        {
            return is_word_char(ch);
        }
    }

    false
}

fn collect_file_diff_line_text_literal_stream_match_visible_rows(
    rows: impl IntoIterator<Item = (usize, worktree_core::file_diff::FileDiffLineText)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    let Some(mut search) = LiteralFileDiffLineTextStreamSearch::new(matcher) else {
        return;
    };

    for (visible_ix, raw_text) in rows {
        if search.has_rows() {
            search.push_byte(b'\n', out);
        }
        search.push_row_start(visible_ix);

        let row_len = normalized_file_diff_line_text_len(&raw_text);
        let mut chunk_start = 0usize;
        while chunk_start < row_len {
            let chunk_end = chunk_start
                .saturating_add(FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES)
                .min(row_len);
            if let Some(bytes) = raw_text.slice_bytes(chunk_start..chunk_end) {
                search.push_bytes(bytes.as_ref(), out);
            } else {
                search.skip_bytes_until_next_row(chunk_end.saturating_sub(chunk_start));
            }
            chunk_start = chunk_end;

            if search.row_reported(visible_ix) {
                search.skip_bytes_until_next_row(row_len.saturating_sub(chunk_start));
                break;
            }
        }
    }

    search.finish(out);
}

struct RegexFileDiffLineTextStreamSearch<'a> {
    matcher: &'a DiffSearchMatcher,
    window: String,
    window_start_abs: usize,
    stream_abs: usize,
    row_starts: Vec<(usize, usize)>,
    last_reported_visible_ix: Option<usize>,
}

impl<'a> RegexFileDiffLineTextStreamSearch<'a> {
    fn new(matcher: &'a DiffSearchMatcher) -> Self {
        Self {
            matcher,
            window: String::with_capacity(FILE_PREVIEW_REGEX_SEARCH_KEEP_BYTES),
            window_start_abs: 0,
            stream_abs: 0,
            row_starts: Vec::new(),
            last_reported_visible_ix: None,
        }
    }

    fn has_rows(&self) -> bool {
        !self.row_starts.is_empty()
    }

    fn push_row_start(&mut self, visible_ix: usize) {
        self.row_starts.push((self.stream_abs, visible_ix));
    }

    fn row_reported(&self, visible_ix: usize) -> bool {
        self.last_reported_visible_ix == Some(visible_ix)
    }

    fn report_match_start(&mut self, match_start: usize, out: &mut Vec<usize>) {
        let Some(visible_ix) = visible_ix_for_stream_abs(self.row_starts.as_slice(), match_start)
        else {
            return;
        };
        if self.last_reported_visible_ix == Some(visible_ix) {
            return;
        }
        out.push(visible_ix);
        self.last_reported_visible_ix = Some(visible_ix);
    }

    fn scan_window(&mut self, stream_finished: bool, out: &mut Vec<usize>) {
        let mut search_start = 0usize;
        while search_start < self.window.len() {
            let Some(range) = self
                .matcher
                .find_range_at_or_after(self.window.as_str(), search_start)
            else {
                break;
            };

            let has_real_before = range.start > 0 || self.window_start_abs == 0;
            let has_real_after = range.end < self.window.len() || stream_finished;
            let match_start_abs = self.window_start_abs.saturating_add(range.start);
            if has_real_before && has_real_after {
                self.report_match_start(match_start_abs, out);
            }

            let next_row_start =
                next_row_start_after_stream_abs(self.row_starts.as_slice(), match_start_abs)
                    .unwrap_or_else(|| self.window_start_abs.saturating_add(self.window.len()));
            let next_search_start = next_row_start
                .saturating_sub(self.window_start_abs)
                .min(self.window.len());
            if next_search_start > range.start {
                search_start = next_search_start;
            } else {
                search_start = next_char_boundary_after(self.window.as_str(), range.start)
                    .unwrap_or(self.window.len());
            }
        }
    }

    fn trim_window(&mut self) {
        if self.window.len() <= FILE_PREVIEW_REGEX_SEARCH_KEEP_BYTES {
            return;
        }

        let mut drop_len = self
            .window
            .len()
            .saturating_sub(FILE_PREVIEW_REGEX_SEARCH_KEEP_BYTES);
        while drop_len > 0 && !self.window.is_char_boundary(drop_len) {
            drop_len -= 1;
        }
        if drop_len == 0 {
            return;
        }

        self.window.drain(..drop_len);
        self.window_start_abs = self.window_start_abs.saturating_add(drop_len);
    }

    fn push_str(&mut self, text: &str, out: &mut Vec<usize>) {
        self.window.push_str(text);
        self.stream_abs = self.stream_abs.saturating_add(text.len());
        if self.window.len() >= FILE_PREVIEW_REGEX_SEARCH_WINDOW_BYTES {
            self.scan_window(false, out);
            self.trim_window();
        }
    }

    fn skip_bytes_until_next_row(&mut self, byte_count: usize) {
        self.stream_abs = self.stream_abs.saturating_add(byte_count);
        self.window.clear();
        self.window_start_abs = self.stream_abs;
    }

    fn finish(&mut self, out: &mut Vec<usize>) {
        self.scan_window(true, out);
    }
}

fn collect_file_diff_line_text_regex_stream_match_visible_rows(
    rows: impl IntoIterator<Item = (usize, worktree_core::file_diff::FileDiffLineText)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    let mut search = RegexFileDiffLineTextStreamSearch::new(matcher);

    for (visible_ix, raw_text) in rows {
        if search.has_rows() {
            search.push_str("\n", out);
        }
        search.push_row_start(visible_ix);

        let row_len = normalized_file_diff_line_text_len(&raw_text);
        let mut chunk_start = 0usize;
        while chunk_start < row_len {
            let requested_end = chunk_start
                .saturating_add(FILE_PREVIEW_SEARCH_SCAN_CHUNK_BYTES)
                .min(row_len);
            let Some((text, resolved_range)) =
                raw_text.slice_text_resolved(chunk_start..requested_end)
            else {
                break;
            };
            if !text.is_empty() {
                search.push_str(text.as_ref(), out);
            }

            if resolved_range.end > chunk_start {
                chunk_start = resolved_range.end;
            } else {
                chunk_start = requested_end;
            }

            if search.row_reported(visible_ix) {
                search.skip_bytes_until_next_row(row_len.saturating_sub(chunk_start));
                break;
            }
        }
    }

    search.finish(out);
}

pub(super) fn collect_file_diff_line_text_stream_match_visible_rows(
    rows: impl IntoIterator<Item = (usize, worktree_core::file_diff::FileDiffLineText)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    if matcher.options().regex {
        collect_file_diff_line_text_regex_stream_match_visible_rows(rows, matcher, out);
    } else {
        collect_file_diff_line_text_literal_stream_match_visible_rows(rows, matcher, out);
    }
}

pub(super) fn collect_stream_match_row_offsets<T: AsRef<str>>(
    rows: impl IntoIterator<Item = (usize, T)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<(usize, usize)>,
) {
    if matcher.can_use_single_row_literal_path() {
        for (visible_ix, row_text) in rows {
            if let Some(range) =
                matcher.find_range_at_or_after(normalized_stream_row_text(row_text.as_ref()), 0)
            {
                out.push((visible_ix, range.start));
            }
        }
        return;
    }

    let mut text = String::new();
    let mut line_starts = Vec::new();
    let mut visible_indices = Vec::new();

    for (visible_ix, row_text) in rows {
        if !visible_indices.is_empty() {
            text.push('\n');
        }
        line_starts.push(text.len());
        visible_indices.push(visible_ix);
        text.push_str(normalized_stream_row_text(row_text.as_ref()));
    }

    if visible_indices.is_empty() {
        return;
    }

    let mut search_start = 0usize;
    while let Some(range) = matcher.find_range_at_or_after(&text, search_start) {
        let start = range.start.min(text.len());
        let line_ix = match line_starts.binary_search(&start) {
            Ok(ix) => ix,
            Err(ix) => ix.saturating_sub(1),
        };
        if let Some(visible_ix) = visible_indices.get(line_ix).copied() {
            let line_start = line_starts.get(line_ix).copied().unwrap_or(start);
            out.push((visible_ix, start.saturating_sub(line_start)));
        }

        search_start = line_starts.get(line_ix + 1).copied().unwrap_or(text.len());
        if search_start <= start {
            break;
        }
    }
}

pub(super) fn collect_stream_match_visible_rows_with_mode<T: AsRef<str>>(
    rows: impl IntoIterator<Item = (usize, T)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) -> StreamMatchCollectionMode {
    if matcher.can_use_single_row_literal_path() {
        for (visible_ix, row_text) in rows {
            if matcher
                .find_range_at_or_after(normalized_stream_row_text(row_text.as_ref()), 0)
                .is_some()
            {
                out.push(visible_ix);
            }
        }
        return StreamMatchCollectionMode::SingleRowLiteral;
    }

    let mut row_offsets = Vec::new();
    collect_stream_match_row_offsets(rows, matcher, &mut row_offsets);
    out.extend(row_offsets.into_iter().map(|(visible_ix, _)| visible_ix));
    StreamMatchCollectionMode::MaterializedRows
}

pub(super) fn collect_stream_match_visible_rows<T: AsRef<str>>(
    rows: impl IntoIterator<Item = (usize, T)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    let _ = collect_stream_match_visible_rows_with_mode(rows, matcher, out);
}

pub(super) fn collect_split_stream_match_visible_rows<'a>(
    rows: impl IntoIterator<Item = (usize, Option<Cow<'a, str>>, Option<Cow<'a, str>>)>,
    matcher: &DiffSearchMatcher,
    out: &mut Vec<usize>,
) {
    if matcher.can_use_single_row_literal_path() {
        for (visible_ix, left, right) in rows {
            let left_matches = left.as_ref().is_some_and(|left| {
                matcher
                    .find_range_at_or_after(normalized_stream_row_text(left.as_ref()), 0)
                    .is_some()
            });
            let right_matches = right.as_ref().is_some_and(|right| {
                matcher
                    .find_range_at_or_after(normalized_stream_row_text(right.as_ref()), 0)
                    .is_some()
            });
            if left_matches || right_matches {
                out.push(visible_ix);
            }
        }
        return;
    }

    let mut left_rows = Vec::new();
    let mut right_rows = Vec::new();

    for (visible_ix, left, right) in rows {
        left_rows.push((visible_ix, left.unwrap_or(Cow::Borrowed(""))));
        right_rows.push((visible_ix, right.unwrap_or(Cow::Borrowed(""))));
    }

    collect_stream_match_visible_rows(left_rows, matcher, out);
    collect_stream_match_visible_rows(right_rows, matcher, out);
}
