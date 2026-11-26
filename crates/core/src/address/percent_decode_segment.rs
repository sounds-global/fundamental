//! [`percent_decode_segment`] — one path segment as its author wrote it.

#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::String, vec::Vec};
#[cfg(feature = "std")]
use std::borrow::Cow;

/// Decodes the percent-escapes in one already-split path segment.
///
/// A lookup key only: the caller splits on `/` first and emits the original segment, so a
/// decoded `%2F` never becomes a separator. A malformed escape decodes to itself.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::percent_decode_segment;
///
/// assert_eq!(percent_decode_segment("aan-de-slag"), "aan-de-slag");
/// assert_eq!(percent_decode_segment("%E3%83%AD%E3%82%B0"), "ログ");
/// // A separator survives as an escape rather than becoming one.
/// assert_eq!(percent_decode_segment("a%2Fb"), "a/b");
/// // Malformed input is its own answer.
/// assert_eq!(percent_decode_segment("%ZZ"), "%ZZ");
/// ```
#[must_use]
pub fn percent_decode_segment(segment: &str) -> Cow<'_, str> {
    if !segment.contains('%') {
        return Cow::Borrowed(segment);
    }
    let bytes = segment.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match (bytes[at], bytes.get(at + 1), bytes.get(at + 2)) {
            (b'%', Some(high), Some(low)) => match (hex(*high), hex(*low)) {
                (Some(high), Some(low)) => {
                    out.push(high << 4 | low);
                    at += 3;
                }
                // Not an escape, so a literal `%`, as a browser's address bar reads it.
                _ => {
                    out.push(b'%');
                    at += 1;
                }
            },
            _ => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    match String::from_utf8(out) {
        Ok(decoded) => Cow::Owned(decoded),
        Err(_) => Cow::Borrowed(segment),
    }
}

/// One hex digit's value, in either case.
const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_with_no_escape_is_borrowed_unchanged() {
        assert!(matches!(
            percent_decode_segment("aan-de-slag"),
            Cow::Borrowed("aan-de-slag")
        ));
    }

    #[test]
    fn a_word_in_any_script_decodes_to_itself() {
        for (encoded, decoded) in [
            ("%E3%83%AD%E3%82%B0%E3%82%A4%E3%83%B3", "ログイン"),
            ("%D0%B2%D1%85%D0%BE%D0%B4", "вход"),
            ("%D8%AF%D8%AE%D9%88%D9%84", "دخول"),
            ("%E7%99%BB%E5%BD%95", "登录"),
        ] {
            assert_eq!(percent_decode_segment(encoded), decoded);
        }
    }

    #[test]
    fn lowercase_and_uppercase_hex_read_the_same() {
        assert_eq!(
            percent_decode_segment("%e3%83%ad"),
            percent_decode_segment("%E3%83%AD")
        );
    }

    /// A decoded key may contain a separator; the caller never emits it.
    #[test]
    fn an_encoded_separator_decodes_to_a_character_not_to_a_boundary() {
        assert_eq!(percent_decode_segment("%2E%2E%2Fetc"), "../etc");
    }

    #[test]
    fn a_malformed_escape_is_the_segment_itself() {
        for malformed in ["%", "%Z", "%ZZ", "%E3", "100%"] {
            assert_eq!(percent_decode_segment(malformed), malformed);
        }
    }

    #[test]
    fn bytes_that_are_not_utf8_leave_the_segment_alone() {
        // A lone continuation byte: no string this build could compare against.
        assert_eq!(percent_decode_segment("%80"), "%80");
    }
}
