//! Text-search kernel shared by the pane search state and the row painters.
//!
//! `DiffSearchMatcher` is the stateful query configuration (case/whole-word/regex);
//! `AsciiCaseInsensitiveNeedle` is the borrowed zero-allocation fast path. Both used to
//! live in `view::panes::main::diff_search`, which forced `view::rows` to import the
//! pane layer; they sink here so both sides depend on `kit` instead.

use memchr::{memchr_iter, memchr2_iter};
use regex::{Regex, RegexBuilder};
use std::borrow::Cow;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct DiffSearchOptions {
    pub(crate) match_case: bool,
    pub(crate) whole_word: bool,
    pub(crate) regex: bool,
}

pub(crate) fn normalize_diff_search_query(query: &str) -> Cow<'_, str> {
    if !query.contains('\r') {
        return Cow::Borrowed(query);
    }
    Cow::Owned(query.replace("\r\n", "\n").replace('\r', "\n"))
}

pub(crate) struct DiffSearchMatcher {
    query: String,
    options: DiffSearchOptions,
    regex: Option<Regex>,
    regex_error: Option<String>,
}

impl DiffSearchMatcher {
    pub(crate) fn new(query: &str, options: DiffSearchOptions) -> Self {
        let query = normalize_diff_search_query(query).into_owned();
        let (regex, regex_error) = if options.regex && !query.is_empty() {
            match RegexBuilder::new(&query)
                .case_insensitive(!options.match_case)
                .multi_line(true)
                .build()
            {
                Ok(regex) => (Some(regex), None),
                Err(err) => (None, Some(err.to_string())),
            }
        } else {
            (None, None)
        };

        Self {
            query,
            options,
            regex,
            regex_error,
        }
    }

    pub(crate) fn query(&self) -> &str {
        self.query.as_str()
    }

    pub(crate) fn options(&self) -> DiffSearchOptions {
        self.options
    }

    pub(crate) fn regex_error(&self) -> Option<&str> {
        self.regex_error.as_deref()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub(crate) fn can_use_ascii_case_insensitive_fast_path(&self) -> bool {
        !self.options.match_case
            && !self.options.whole_word
            && !self.options.regex
            && !self.query.contains('\n')
    }

    pub(crate) fn can_use_single_row_literal_path(&self) -> bool {
        !self.options.regex && !self.query.contains('\n')
    }

    pub(crate) fn is_match(&self, haystack: &str) -> bool {
        self.find_range_at_or_after(haystack, 0).is_some()
    }

    pub(crate) fn find_ranges_into(
        &self,
        haystack: &str,
        out: &mut Vec<Range<usize>>,
        max_matches: usize,
    ) {
        out.clear();
        if max_matches == 0 || self.is_empty() || self.regex_error.is_some() {
            return;
        }

        let mut search_start = 0usize;
        while out.len() < max_matches {
            let Some(range) = self.find_range_at_or_after(haystack, search_start) else {
                break;
            };
            search_start = range.end;
            out.push(range);
        }
    }

    fn find_literal_case_sensitive_from(
        &self,
        haystack: &str,
        start_at: usize,
    ) -> Option<Range<usize>> {
        let needle = self.query.as_bytes();
        let haystack_bytes = haystack.as_bytes();
        let (&first, _) = needle.first().zip(needle.last())?;
        let last_start = haystack_bytes.len().checked_sub(needle.len())?;
        let start_at = start_at.min(haystack_bytes.len());
        if start_at > last_start {
            return None;
        }

        for offset in memchr_iter(first, &haystack_bytes[start_at..=last_start]) {
            let start = start_at + offset;
            let range = start..(start + needle.len());
            if haystack_bytes.get(range.clone()) == Some(needle)
                && self.range_has_requested_boundaries(haystack, range.clone())
            {
                return Some(range);
            }
        }
        None
    }

    fn find_literal_ascii_case_insensitive_from(
        &self,
        haystack: &str,
        start_at: usize,
    ) -> Option<Range<usize>> {
        let needle = AsciiCaseInsensitiveNeedle::new(&self.query)?;
        let mut search_start = start_at;
        loop {
            let range = needle.find_range_from(haystack, search_start)?;
            if self.range_has_requested_boundaries(haystack, range.clone()) {
                return Some(range);
            }
            // A boundary-rejected candidate does not consume the match: the
            // next candidate may overlap it, so resume one byte past its start.
            search_start = range.start + 1;
        }
    }

    pub(crate) fn find_row_overlay_ranges_into(
        &self,
        haystack: &str,
        out: &mut Vec<Range<usize>>,
        max_matches: usize,
    ) {
        self.find_ranges_into(haystack, out, max_matches);
    }

    pub(crate) fn find_range_at_or_after(
        &self,
        haystack: &str,
        start_at: usize,
    ) -> Option<Range<usize>> {
        if self.is_empty() || self.regex_error.is_some() {
            return None;
        }

        if let Some(regex) = self.regex.as_ref() {
            let mut search_start = start_at.min(haystack.len());
            loop {
                let m = regex.find_at(haystack, search_start)?;
                let range = m.start()..m.end();
                if !range.is_empty() && self.range_has_requested_boundaries(haystack, range.clone())
                {
                    return Some(range);
                }
                search_start = next_char_boundary_after(haystack, m.start())?;
            }
        }

        if self.options.match_case {
            self.find_literal_case_sensitive_from(haystack, start_at)
        } else {
            self.find_literal_ascii_case_insensitive_from(haystack, start_at)
        }
    }

    fn range_has_requested_boundaries(&self, haystack: &str, range: Range<usize>) -> bool {
        if !self.options.whole_word {
            return true;
        }

        !haystack[..range.start]
            .chars()
            .next_back()
            .is_some_and(is_word_char)
            && !haystack[range.end..]
                .chars()
                .next()
                .is_some_and(is_word_char)
    }
}

#[inline]
pub(crate) fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub(crate) fn next_char_boundary_after(s: &str, ix: usize) -> Option<usize> {
    if ix >= s.len() {
        return None;
    }

    Some(ix + s[ix..].chars().next()?.len_utf8())
}

#[derive(Clone, Copy)]
pub(crate) struct AsciiCaseInsensitiveNeedle<'a> {
    bytes: &'a [u8],
    first_lower: u8,
    first_upper: u8,
    last_lower: u8,
    last_upper: u8,
}

impl<'a> AsciiCaseInsensitiveNeedle<'a> {
    #[inline]
    pub(crate) fn new(needle: &'a str) -> Option<Self> {
        let bytes = needle.as_bytes();
        let (&first, &last) = bytes.first().zip(bytes.last())?;

        Some(Self {
            bytes,
            first_lower: first.to_ascii_lowercase(),
            first_upper: first.to_ascii_uppercase(),
            last_lower: last.to_ascii_lowercase(),
            last_upper: last.to_ascii_uppercase(),
        })
    }

    #[inline]
    pub(crate) fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    /// First match at or after `start_at`. This is the single copy of the
    /// memchr2 scan: `is_match` answers with it from 0, and
    /// `DiffSearchMatcher`'s case-insensitive literal path loops over it while
    /// rejecting candidates that fail the whole-word boundary check.
    pub(crate) fn find_range_from(self, haystack: &str, start_at: usize) -> Option<Range<usize>> {
        let haystack_bytes = haystack.as_bytes();
        let needle_len = self.bytes.len();
        let last_start = haystack_bytes.len().checked_sub(needle_len)?;
        let start_at = start_at.min(haystack_bytes.len());
        if start_at > last_start {
            return None;
        }

        if needle_len == 1 {
            let offset = memchr2_iter(
                self.first_lower,
                self.first_upper,
                &haystack_bytes[start_at..],
            )
            .next()?;
            let start = start_at + offset;
            return Some(start..start + 1);
        }

        let middle = &self.bytes[1..needle_len - 1];
        for offset in memchr2_iter(
            self.first_lower,
            self.first_upper,
            &haystack_bytes[start_at..=last_start],
        ) {
            let start = start_at + offset;
            let last = haystack_bytes[start + needle_len - 1];
            if last != self.last_lower && last != self.last_upper {
                continue;
            }

            if haystack_bytes[start + 1..start + needle_len - 1].eq_ignore_ascii_case(middle) {
                return Some(start..start + needle_len);
            }
        }

        None
    }

    #[inline]
    pub(crate) fn is_match(self, haystack: &str) -> bool {
        self.find_range_from(haystack, 0).is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiffSearchQueryReuse {
    None,
    SameSemantics,
    Refinement,
}

#[inline]
pub(crate) fn diff_search_query_reuse(
    previous_query: &str,
    next_query: &str,
) -> DiffSearchQueryReuse {
    let previous_query = normalize_diff_search_query(previous_query);
    let next_query = normalize_diff_search_query(next_query);
    if next_query
        .as_bytes()
        .eq_ignore_ascii_case(previous_query.as_bytes())
    {
        return DiffSearchQueryReuse::SameSemantics;
    }

    if !previous_query.is_empty()
        && next_query.len() > previous_query.len()
        && next_query
            .as_bytes()
            .get(..previous_query.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(previous_query.as_bytes()))
    {
        return DiffSearchQueryReuse::Refinement;
    }

    DiffSearchQueryReuse::None
}
