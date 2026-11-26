use core::cmp::Ordering;

/// Orders two member names the way RFC 8785 §3.2.3 does: by their UTF-16 code units.
///
/// This differs from UTF-8 byte order above U+E000.
pub(crate) fn utf16_cmp(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

#[cfg(test)]
mod tests {
    use super::utf16_cmp;
    use core::cmp::Ordering;

    #[test]
    fn utf16_and_utf8_disagree_above_the_surrogate_block() {
        let private_use = "\u{E000}";
        let astral = "\u{10000}";
        assert_eq!(
            private_use.as_bytes().cmp(astral.as_bytes()),
            Ordering::Less
        );
        assert_eq!(utf16_cmp(private_use, astral), Ordering::Greater);
    }

    #[test]
    fn a_prefix_sorts_first() {
        assert_eq!(utf16_cmp("a", "ab"), Ordering::Less);
        assert_eq!(utf16_cmp("ab", "ab"), Ordering::Equal);
    }
}
