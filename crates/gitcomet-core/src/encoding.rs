//! Best-effort conversion of non-UTF-8 text bytes into UTF-8.
//!
//! Repositories routinely hold files saved in legacy single/multi-byte
//! encodings — GBK is still the default for older toolchains on Chinese
//! Windows installs, so a source file can be perfectly valid text yet not
//! valid UTF-8. Rather than refusing to diff such files, we detect the most
//! likely encoding and transcode for display.

use std::borrow::Cow;

/// Detects and decodes non-UTF-8 text bytes.
///
/// Valid UTF-8 input is passed through untouched (borrowed, byte-identical).
/// Anything else is fed through a charset detector (GB18030, Windows-1252,
/// UTF-16, …) and re-encoded as UTF-8; undecodable byte sequences become
/// U+FFFD, so the result is always valid UTF-8.
pub fn text_bytes_to_utf8(bytes: &[u8]) -> Cow<'_, [u8]> {
    if std::str::from_utf8(bytes).is_ok() {
        return Cow::Borrowed(bytes);
    }

    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    // `false`: the input already failed strict UTF-8 validation, so guessing
    // UTF-8 would only produce U+FFFD soup — let the detector commit to a
    // legacy encoding instead.
    let encoding = detector.guess(None, false);
    let (text, _encoding, _had_errors) = encoding.decode(bytes);
    Cow::Owned(text.into_owned().into_bytes())
}

/// [`text_bytes_to_utf8`] for callers that want a `str` (e.g. decoding a blob
/// for a unified diff body).
pub fn text_bytes_to_utf8_string(bytes: Vec<u8>) -> String {
    match text_bytes_to_utf8(bytes.as_slice()) {
        Cow::Borrowed(_) => {
            // Reached only when the bytes are valid UTF-8, so the unwrap
            // cannot fail.
            String::from_utf8(bytes).expect("validated UTF-8 bytes")
        }
        Cow::Owned(utf8) => String::from_utf8(utf8).expect("decode output is UTF-8"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_utf8_passes_through_unchanged() {
        let bytes = b"hello \xe4\xb8\xad\xe6\x96\x87\n".as_slice();
        let converted = text_bytes_to_utf8(bytes);
        assert!(matches!(converted, Cow::Borrowed(slice) if slice == bytes));
    }

    #[test]
    fn empty_input_passes_through() {
        assert!(matches!(
            text_bytes_to_utf8(b""),
            Cow::Borrowed(slice) if slice.is_empty()
        ));
    }

    #[test]
    fn gbk_bytes_transcode_to_utf8() {
        // "中文注释" encoded as GBK.
        let gbk: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4, 0xD7, 0xA2, 0xCA, 0xCD];
        let converted = text_bytes_to_utf8(gbk);
        let text = std::str::from_utf8(converted.as_ref()).expect("output is UTF-8");
        assert_eq!(text, "中文注释");
    }

    #[test]
    fn gbk_source_file_with_ascii_code_transcodes_to_utf8() {
        // `int main() { return 0; } // 中文` with the comment in GBK — the
        // shape of a legacy Windows source file.
        let mut gbk = b"int main() { return 0; } // ".to_vec();
        gbk.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        gbk.push(b'\n');
        let converted = text_bytes_to_utf8(gbk.as_slice());
        let text = std::str::from_utf8(converted.as_ref()).expect("output is UTF-8");
        assert_eq!(text, "int main() { return 0; } // 中文\n");
    }

    #[test]
    fn undecodable_bytes_still_yield_valid_utf8() {
        // Whatever legacy encoding the detector commits to, the output must
        // always be valid UTF-8 — sequences that decode nowhere come out as
        // U+FFFD rather than failing the conversion.
        let converted = text_bytes_to_utf8(&[0x80, 0x81, 0x82, 0x83, 0xFE, 0xFF, 0xC3, 0x28]);
        let text = std::str::from_utf8(converted.as_ref()).expect("output is UTF-8");
        assert!(!text.is_empty());
    }

    #[test]
    fn string_variant_round_trips() {
        let gbk: Vec<u8> = vec![0xD6, 0xD0, 0xCE, 0xC4];
        assert_eq!(text_bytes_to_utf8_string(gbk), "中文");
        assert_eq!(text_bytes_to_utf8_string(b"plain".to_vec()), "plain");
    }
}
