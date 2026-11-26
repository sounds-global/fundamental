use hmac::{Hmac, Mac};
use sha2::Sha512;

/// HMAC-SHA512 over the concatenation of `parts`, keyed by `key`, as 64 bytes.
///
/// Infallible: HMAC accepts a key of any length, so `new_from_slice` never errs.
pub(crate) fn hmac_sha512(key: &[u8], parts: &[&[u8]]) -> [u8; 64] {
    let mut mac =
        <Hmac<Sha512> as Mac>::new_from_slice(key).expect("HMAC-SHA512 admits a key of any length");
    for part in parts {
        mac.update(part);
    }
    let out = mac.finalize().into_bytes();
    let mut buf = [0u8; 64];
    buf.copy_from_slice(&out);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4231 Test Case 1.
    #[test]
    fn matches_rfc4231_test_case_1() {
        let key = [0x0b_u8; 20];
        let mac = hmac_sha512(&key, &[b"Hi There"]);
        let expected = concat!(
            "87aa7cdea5ef619d4ff0b4241a1d6cb0",
            "2379f4e2ce4ec2787ad0b30545e17cde",
            "daa833b7d6b8a702038b274eaea3f4e4",
            "be9d914eeb61f1702e696c203a126854",
        );
        assert_eq!(hex_lower(&mac), expected);
    }

    #[test]
    fn concatenates_parts_like_a_single_message() {
        assert_eq!(
            hmac_sha512(b"k", &[b"ab", b"cd"]),
            hmac_sha512(b"k", &[b"abcd"]),
        );
    }

    fn hex_lower(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}
