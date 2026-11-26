use std::sync::LazyLock;

/// The BIP-39 English wordlist, vendored verbatim; frozen, locked by the checksum test.
const WORDLIST: &str = include_str!("bip39_english.txt");

/// The wordlist as a sorted slice, so lookups binary search.
static WORDS: LazyLock<Vec<&'static str>> = LazyLock::new(|| WORDLIST.lines().collect());

pub(crate) fn word(index: u16) -> Option<&'static str> {
    WORDS.get(index as usize).copied()
}

pub(crate) fn index_of(word: &str) -> Option<u16> {
    WORDS
        .binary_search(&word)
        .ok()
        .map(|i| u16::try_from(i).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn wordlist_file_matches_the_canonical_sha256() {
        let digest = Sha256::digest(WORDLIST.as_bytes());
        assert_eq!(
            hex_lower(&digest),
            "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda"
        );
    }

    #[test]
    fn holds_exactly_2048_words() {
        assert_eq!(WORDS.len(), 2048);
    }

    #[test]
    fn is_sorted_so_binary_search_is_valid() {
        assert!(WORDS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn known_index_endpoints() {
        assert_eq!(word(0), Some("abandon"));
        assert_eq!(word(2047), Some("zoo"));
        assert_eq!(word(2048), None);
        assert_eq!(index_of("abandon"), Some(0));
        assert_eq!(index_of("zoo"), Some(2047));
        assert_eq!(index_of("not-a-word"), None);
    }

    #[test]
    fn word_and_index_round_trip() {
        for i in 0..2048u16 {
            let w = word(i).expect("word present");
            assert_eq!(index_of(w), Some(i));
        }
    }

    fn hex_lower(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}
