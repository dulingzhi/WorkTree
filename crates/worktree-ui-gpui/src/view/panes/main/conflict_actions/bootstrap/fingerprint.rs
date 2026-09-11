//! Source fingerprinting: cheap content samples that decide whether a conflict
//! source has actually changed.

use rustc_hash::FxHasher;
const CONFLICT_SOURCE_FINGERPRINT_SAMPLE_COUNT: usize = 8;
const CONFLICT_SOURCE_FINGERPRINT_WINDOW_BYTES: usize = 256;

// This is a lightweight UI cache key, not a cryptographic hash. Domain labels
// keep the text/bytes/none cases distinct without opaque numeric seeds.
fn sampled_content_fingerprint(bytes: &[u8], domain: &str) -> u64 {
    use std::hash::Hasher;

    let mut hasher = FxHasher::default();
    hasher.write_usize(domain.len());
    hasher.write(domain.as_bytes());
    hasher.write_usize(bytes.len());
    if bytes.is_empty() {
        return hasher.finish();
    }

    let window_len = CONFLICT_SOURCE_FINGERPRINT_WINDOW_BYTES.min(bytes.len());
    let sample_count = if bytes.len() <= window_len {
        1
    } else {
        CONFLICT_SOURCE_FINGERPRINT_SAMPLE_COUNT
    };
    let max_start = bytes.len().saturating_sub(window_len);
    let denominator = sample_count.saturating_sub(1).max(1);
    for sample_ix in 0..sample_count {
        let start = if sample_count == 1 {
            0
        } else {
            sample_ix.saturating_mul(max_start) / denominator
        };
        hasher.write_usize(start);
        hasher.write(&bytes[start..start.saturating_add(window_len)]);
    }
    hasher.finish()
}

pub(super) fn shared_text_fingerprint(text: &Option<std::sync::Arc<str>>) -> u64 {
    let Some(text) = text.as_ref() else {
        return sampled_content_fingerprint(&[], "conflict-source:text:none");
    };
    sampled_content_fingerprint(text.as_bytes(), "conflict-source:text")
}

pub(super) fn shared_bytes_fingerprint(bytes: &Option<std::sync::Arc<[u8]>>) -> u64 {
    let Some(bytes) = bytes.as_ref() else {
        return sampled_content_fingerprint(&[], "conflict-source:bytes:none");
    };
    sampled_content_fingerprint(bytes.as_ref(), "conflict-source:bytes")
}

pub(super) fn conflict_file_source_fingerprint(file: &worktree_state::model::ConflictFile) -> u64 {
    let side_fingerprint = |text: &Option<std::sync::Arc<str>>,
                            bytes: &Option<std::sync::Arc<[u8]>>,
                            side_domain: &str| {
        let value = if text.is_some() {
            shared_text_fingerprint(text)
        } else {
            shared_bytes_fingerprint(bytes)
        };
        sampled_content_fingerprint(&value.to_le_bytes(), side_domain)
    };

    let mut acc = sampled_content_fingerprint(&[], "conflict-source:file");
    for (side_domain, text, bytes) in [
        ("conflict-source:side:base", &file.base, &file.base_bytes),
        ("conflict-source:side:ours", &file.ours, &file.ours_bytes),
        (
            "conflict-source:side:theirs",
            &file.theirs,
            &file.theirs_bytes,
        ),
        (
            "conflict-source:side:current",
            &file.current,
            &file.current_bytes,
        ),
    ] {
        acc = acc.rotate_left(13) ^ side_fingerprint(text, bytes, side_domain);
    }
    acc
}
