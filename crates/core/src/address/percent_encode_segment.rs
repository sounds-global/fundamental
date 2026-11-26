//! [`percent_encode_segment`] — one path segment as a request line may carry it.

#[cfg(not(feature = "std"))]
use alloc::{borrow::Cow, string::String};
#[cfg(feature = "std")]
use std::borrow::Cow;

/// RFC 3986's unreserved set only, so a canonical address and a request agree on one spelling.
const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encodes one path segment for a URL.
///
/// Uppercase hex, as browsers send it: the canonical address is compared byte for byte
/// against the request.
///
/// ```
/// # extern crate fundamental_core;
/// use fundamental_core::address::percent_encode_segment;
///
/// assert_eq!(percent_encode_segment("aan-de-slag"), "aan-de-slag");
/// assert_eq!(percent_encode_segment("ログ"), "%E3%83%AD%E3%82%B0");
/// ```
#[must_use]
pub fn percent_encode_segment(segment: &str) -> Cow<'_, str> {
    if segment.bytes().all(is_unreserved) {
        return Cow::Borrowed(segment);
    }
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(segment.len() * 3);
    for byte in segment.bytes() {
        match is_unreserved(byte) {
            true => out.push(byte as char),
            false => {
                out.push('%');
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0x0f)] as char);
            }
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::percent_decode_segment;

    #[test]
    fn an_ascii_slug_is_borrowed_unchanged() {
        assert!(matches!(
            percent_encode_segment("aan-de-slag"),
            Cow::Borrowed("aan-de-slag")
        ));
    }

    #[test]
    fn every_script_survives_a_round_trip() {
        for word in [
            "ログイン",
            "вход",
            "دخول",
            "登录",
            "로그인",
            "εγγραφή",
            "כניסה",
            "प्रवेश",
            "ግባ",
            "เข้าสู่ระบบ",
            "verifieren",
            "chave-de-acesso",
        ] {
            let encoded = percent_encode_segment(word);
            assert!(
                encoded.is_ascii(),
                "a request line carries ASCII: {word} became {encoded}"
            );
            assert_eq!(percent_decode_segment(&encoded), word);
        }
    }

    #[test]
    fn the_hex_is_uppercase_because_the_answer_is_compared() {
        assert_eq!(percent_encode_segment("ログ"), "%E3%83%AD%E3%82%B0");
    }

    /// A separator inside a segment is escaped, never emitted.
    #[test]
    fn a_separator_never_leaves_this_function_as_itself() {
        assert_eq!(percent_encode_segment("a/b"), "a%2Fb");
        assert_eq!(percent_encode_segment("a?b"), "a%3Fb");
        assert_eq!(percent_encode_segment("a#b"), "a%23b");
        assert_eq!(percent_encode_segment("a b"), "a%20b");
    }
}
