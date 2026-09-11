//! The visible-row trigram index and its query predicates.

use super::consts::DIFF_SEARCH_TRIGRAM_MIN_QUERY_BYTES;
use super::row_text::DiffSearchVisibleCandidates;

use crate::kit::text_search::AsciiCaseInsensitiveNeedle;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
#[derive(Clone, Debug, Default)]
pub(in crate::view) struct DiffSearchVisibleTrigramIndex {
    postings: FxHashMap<u32, Vec<u32>>,
}

impl DiffSearchVisibleTrigramIndex {
    pub(in crate::view) fn insert_text(
        &mut self,
        visible_ix: u32,
        text: &str,
        trigrams: &mut SmallVec<[u32; 64]>,
    ) {
        collect_unique_ascii_folded_byte_trigrams(text.as_bytes(), trigrams);
        for trigram in trigrams.iter().copied() {
            self.postings.entry(trigram).or_default().push(visible_ix);
        }
    }

    pub(in crate::view) fn finish(mut self) -> Self {
        for indices in self.postings.values_mut() {
            indices.shrink_to_fit();
        }
        self
    }

    pub(in crate::view) fn candidates<'a>(
        &'a self,
        needle: &[u8],
    ) -> DiffSearchVisibleCandidates<'a> {
        if needle.len() < 3 {
            return DiffSearchVisibleCandidates::All;
        }

        let mut trigrams = SmallVec::<[u32; 64]>::new();
        collect_unique_ascii_folded_byte_trigrams(needle, &mut trigrams);

        let mut best: Option<&[u32]> = None;
        for trigram in trigrams.iter() {
            let Some(postings) = self.postings.get(trigram).map(Vec::as_slice) else {
                return DiffSearchVisibleCandidates::None;
            };
            if best.is_none_or(|current| postings.len() < current.len()) {
                best = Some(postings);
            }
        }

        match best {
            Some(postings) => DiffSearchVisibleCandidates::Indexed(postings),
            None => DiffSearchVisibleCandidates::All,
        }
    }
}

pub(super) fn diff_search_inline_patch_query_uses_trigram_index(
    query: AsciiCaseInsensitiveNeedle<'_>,
) -> bool {
    query.as_bytes().len() >= DIFF_SEARCH_TRIGRAM_MIN_QUERY_BYTES
}

fn collect_unique_ascii_folded_byte_trigrams(bytes: &[u8], trigrams: &mut SmallVec<[u32; 64]>) {
    trigrams.clear();
    if bytes.len() < 3 {
        return;
    }

    trigrams.extend(bytes.windows(3).map(encode_ascii_folded_byte_trigram));
    trigrams.sort_unstable();
    trigrams.dedup();
}

fn encode_ascii_folded_byte_trigram(window: &[u8]) -> u32 {
    debug_assert_eq!(window.len(), 3);
    (u32::from(window[0].to_ascii_lowercase()) << 16)
        | (u32::from(window[1].to_ascii_lowercase()) << 8)
        | u32::from(window[2].to_ascii_lowercase())
}
