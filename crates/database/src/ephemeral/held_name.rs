//! [`HeldName`] — the name of an ephemeral store whose run holds a lock on it for its whole life.

use core::fmt;

use super::{EphemeralName, EphemeralNameError};

/// A minted store name — `<prefix><millis>_<random>_h` — for a store its run holds a lock on.
///
/// The run that minted it holds a session advisory lock keyed by the name, so a sweep can
/// reclaim it once the run is gone. The trailing [`MARKER`](Self::MARKER) keeps it outside
/// [`EphemeralName`]'s grammar, so an older age-based sweep never drops one, and only a
/// marked name promises a lock.
///
/// # The grammar
///
/// ```text
/// s_1787095838000_0f3a91c47b2e6d58_h
/// │ │             │                └── the marker: this store's run holds its lock
/// │ │             └── 16 hex characters of a UUIDv4 (60 random bits), which are also the lock's key
/// │ └── 13 digits: milliseconds since the epoch, zero-padded
/// └── the caller's prefix, including its trailing separator
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HeldName {
    name: String,
    key: (i32, i32),
}

impl HeldName {
    /// What ends every held name, and what no other grammar here ends with.
    ///
    /// `h` is not a hex digit and not a decimal digit, so no other grammar can match it.
    pub const MARKER: &'static str = "_h";

    /// The grammar as a POSIX regular expression for a sweep's listing; its one capture group is
    /// the stamp. [`advisory_key_of`](Self::advisory_key_of) decides.
    pub const SQL_PATTERN: &'static str = "_([0-9]{13})_[0-9a-f]{16}_h$";

    /// Mints a name for a store that is about to be created and held.
    ///
    /// `prefix` and `max_len` as [`EphemeralName::mint`] takes them, marker included in the length.
    ///
    /// # Errors
    /// As [`EphemeralName::mint`], with [`TooLong`](EphemeralNameError::TooLong) reporting the
    /// marked name's length.
    pub fn mint(prefix: &str, max_len: usize) -> Result<Self, EphemeralNameError> {
        let marker = Self::MARKER.len();
        let minted = EphemeralName::mint(prefix, max_len.saturating_sub(marker)).map_err(
            |error| match error {
                EphemeralNameError::TooLong { prefix, actual, .. } => EphemeralNameError::TooLong {
                    prefix,
                    actual: actual + marker,
                    max_len,
                },
                other => other,
            },
        )?;
        let name = format!("{minted}{}", Self::MARKER);
        // Read back through the recogniser so the holder and every sweep derive one key.
        let Some(key) = Self::advisory_key_of(&name) else {
            return Err(EphemeralNameError::InvalidPrefix {
                prefix: prefix.to_owned(),
            });
        };
        Ok(Self { name, key })
    }

    /// The name, as it goes into DDL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The advisory-lock key this store is held under, as the two `int4` halves of its random
    /// tail; that lock space never overlaps the ledger's single-`bigint` lock.
    #[must_use]
    pub fn advisory_key(&self) -> (i32, i32) {
        self.key
    }

    /// The advisory-lock key `name` is held under, or `None` if it is not a held name.
    ///
    /// Independent of the prefix and fails closed, like [`EphemeralName::stamp_of`].
    #[must_use]
    pub fn advisory_key_of(name: &str) -> Option<(i32, i32)> {
        let unmarked = name.strip_suffix(Self::MARKER)?;
        EphemeralName::stamp_of(unmarked)?;
        let tail = unmarked.get(unmarked.len() - EphemeralName::RANDOM_CHARS..)?;
        let key = u64::from_str_radix(tail, 16).ok()?.to_be_bytes();
        let class = i32::from_be_bytes([key[0], key[1], key[2], key[3]]);
        let object = i32::from_be_bytes([key[4], key[5], key[6], key[7]]);
        Some((class, object))
    }

    /// The millisecond stamp a held name carries, or `None` if `name` is not one.
    #[must_use]
    pub fn stamp_of(name: &str) -> Option<u64> {
        Self::advisory_key_of(name)?;
        EphemeralName::stamp_of(name.strip_suffix(Self::MARKER)?)
    }
}

impl fmt::Display for HeldName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

impl AsRef<str> for HeldName {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ident::is_valid_identifier;

    const SQL_MAX: usize = 63;

    const PREFIXES: [&str; 7] = [
        "s_",
        "t_",
        "layer5_test_",
        "layer5_mirror_test_",
        "layer5_witness_test_",
        "kill_probe_",
        "holder_vault_test_",
    ];

    fn old_sweep_matches(name: &str) -> bool {
        let tail = 1 + EphemeralName::STAMP_DIGITS + 1 + EphemeralName::RANDOM_CHARS;
        let Some(start) = name.len().checked_sub(tail) else {
            return false;
        };
        let bytes = &name.as_bytes()[start..];
        bytes[0] == b'_'
            && bytes[1..=EphemeralName::STAMP_DIGITS]
                .iter()
                .all(u8::is_ascii_digit)
            && bytes[1 + EphemeralName::STAMP_DIGITS] == b'_'
            && bytes[2 + EphemeralName::STAMP_DIGITS..]
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    }

    #[test]
    fn the_sql_pattern_is_the_grammar() {
        let ephemeral = EphemeralName::SQL_PATTERN
            .strip_suffix('$')
            .expect("the ephemeral pattern is anchored");
        assert_eq!(
            HeldName::SQL_PATTERN,
            format!("{ephemeral}{}$", HeldName::MARKER)
        );
    }

    #[test]
    fn a_held_name_is_a_plain_identifier_within_the_limit() {
        for prefix in PREFIXES {
            let name = HeldName::mint(prefix, SQL_MAX).expect("a plain prefix is accepted");
            assert!(name.as_str().starts_with(prefix), "{name}");
            assert!(name.as_str().ends_with(HeldName::MARKER), "{name}");
            assert!(is_valid_identifier(name.as_str()), "{name}");
            assert!(name.as_str().len() <= SQL_MAX, "{name}");
        }
    }

    /// No older sweep can age-drop a held store.
    #[test]
    fn no_held_name_is_visible_to_the_age_sweep_or_its_old_pattern() {
        for prefix in PREFIXES {
            for _ in 0..64 {
                let name = HeldName::mint(prefix, SQL_MAX).expect("a plain prefix is accepted");
                assert!(EphemeralName::stamp_of(name.as_str()).is_none(), "{name}");
                assert!(!old_sweep_matches(name.as_str()), "{name}");
            }
        }
    }

    /// An unmarked name never promises a lock it does not hold.
    #[test]
    fn no_ephemeral_name_is_read_as_held() {
        for prefix in PREFIXES {
            let name = EphemeralName::mint(prefix, SQL_MAX).expect("a plain prefix is accepted");
            assert!(
                old_sweep_matches(name.as_str()),
                "the old pattern is modelled: {name}"
            );
            assert!(HeldName::advisory_key_of(name.as_str()).is_none(), "{name}");
            assert!(HeldName::stamp_of(name.as_str()).is_none(), "{name}");
        }
    }

    #[test]
    fn nothing_outside_the_grammar_is_held() {
        for foreign in [
            "public",
            "s_1787095838000_0f3a91c47b2e6d58",
            "s_1787095838000_0f3a91c47b2e6d58_H",
            "s_1787095838000_0F3A91C47B2E6D58_h",
            "s_178709583800_0f3a91c47b2e6d58_h",
            "_1787095838000_0f3a91c47b2e6d58_h",
            "layer5_test_pool_20f605805a75_3",
            "_h",
        ] {
            assert!(HeldName::advisory_key_of(foreign).is_none(), "{foreign}");
        }
    }

    #[test]
    fn the_key_is_the_random_tail_split_in_two() {
        let (class, object) =
            HeldName::advisory_key_of("s_1787095838000_0f3a91c4fb2e6d58_h").expect("held");
        assert_eq!(class, 0x0f3a_91c4);
        // A half above `i32::MAX` reads as its two's-complement value, as the engine's `int4` does.
        assert_eq!(object, i32::from_be_bytes([0xfb, 0x2e, 0x6d, 0x58]));
    }

    #[test]
    fn a_minted_name_and_its_recogniser_agree_on_the_key() {
        for _ in 0..256 {
            let name = HeldName::mint("s_", SQL_MAX).expect("a plain prefix is accepted");
            assert_eq!(
                HeldName::advisory_key_of(name.as_str()),
                Some(name.advisory_key()),
                "{name}"
            );
        }
    }

    #[test]
    fn two_held_names_never_share_a_key() {
        let first = HeldName::mint("s_", SQL_MAX).expect("valid prefix");
        let second = HeldName::mint("s_", SQL_MAX).expect("valid prefix");
        assert_ne!(first.advisory_key(), second.advisory_key());
    }

    #[test]
    fn a_held_name_reads_its_own_stamp_back() {
        let name = HeldName::mint("s_", SQL_MAX).expect("valid prefix");
        assert!(HeldName::stamp_of(name.as_str()).is_some(), "{name}");
    }

    #[test]
    fn an_injection_prefix_is_refused() {
        let error = HeldName::mint("x_; DROP SCHEMA prod; --", SQL_MAX).expect_err("fail closed");
        assert!(matches!(error, EphemeralNameError::InvalidPrefix { .. }));
    }

    #[test]
    fn a_prefix_without_its_separator_is_refused() {
        let error = HeldName::mint("s", SQL_MAX).expect_err("a prefix must carry its separator");
        assert!(matches!(
            error,
            EphemeralNameError::PrefixNotSeparated { .. }
        ));
    }

    #[test]
    fn too_long_reports_the_whole_marked_name() {
        let overhead = EphemeralName::STAMP_DIGITS + 1 + EphemeralName::RANDOM_CHARS;
        let exact = format!(
            "{}_",
            "a".repeat(SQL_MAX - overhead - HeldName::MARKER.len() - 1)
        );
        let fits = HeldName::mint(&exact, SQL_MAX).expect("one that fits exactly");
        assert_eq!(fits.as_str().len(), SQL_MAX);

        let over = format!("a{exact}");
        let error = HeldName::mint(&over, SQL_MAX).expect_err("one character too long");
        assert!(matches!(
            error,
            EphemeralNameError::TooLong { actual, max_len, .. } if actual == SQL_MAX + 1 && max_len == SQL_MAX
        ));
    }
}
