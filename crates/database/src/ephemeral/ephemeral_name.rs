//! [`EphemeralName`] — the name of one ephemeral store, carrying its own birthday.

use core::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use super::EphemeralNameError;
use crate::ident::is_valid_identifier;

/// A minted store name — `<prefix><millis>_<random>`, valid as unquoted DDL.
///
/// Minted only through [`mint`](Self::mint), so it is always a valid unquoted identifier,
/// within the engine's length limit, and recognisable to a sweep by [`stamp_of`](Self::stamp_of).
///
/// # The grammar
///
/// ```text
/// t_1787095838000_0f3a91c47b2e6d58
/// │ │             └── 16 hex characters of randomness
/// │ └── 13 digits: milliseconds since the epoch, zero-padded
/// └── the caller's prefix, including its trailing separator
/// ```
///
/// The stamp is fixed-width and zero-padded, so lexicographic order is chronological order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EphemeralName(String);

impl EphemeralName {
    /// Width of the millisecond stamp, in decimal digits.
    pub const STAMP_DIGITS: usize = 13;

    /// Hex characters of randomness appended after the stamp.
    ///
    /// 64 bits, and short enough for a readable prefix inside the 48-character keyspace limit.
    pub const RANDOM_CHARS: usize = 16;

    /// The grammar as a POSIX regular expression, for a sweep that asks the engine to list
    ///
    /// It narrows; [`stamp_of`](Self::stamp_of) decides. The one capture group is the stamp.
    pub const SQL_PATTERN: &'static str = "_([0-9]{13})_[0-9a-f]{16}$";

    /// Mints a name for a store that is about to be created.
    ///
    /// `prefix` includes its trailing separator; `max_len` is the engine's limit for the name.
    ///
    /// # Errors
    /// [`EphemeralNameError::InvalidPrefix`] if `prefix` is not a plain identifier,
    /// [`EphemeralNameError::PrefixNotSeparated`] if it does not end with `_`,
    /// [`EphemeralNameError::TooLong`] if the minted name would exceed `max_len`.
    pub fn mint(prefix: &str, max_len: usize) -> Result<Self, EphemeralNameError> {
        if !is_valid_identifier(prefix) {
            return Err(EphemeralNameError::InvalidPrefix {
                prefix: prefix.to_owned(),
            });
        }
        if !prefix.ends_with('_') {
            return Err(EphemeralNameError::PrefixNotSeparated {
                prefix: prefix.to_owned(),
            });
        }
        // A clock before the epoch gives a zero stamp, which a sweep reads as ancient, never a panic.
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let random = &Uuid::new_v4().simple().to_string()[..Self::RANDOM_CHARS];
        let name = format!(
            "{prefix}{millis:0width$}_{random}",
            width = Self::STAMP_DIGITS
        );
        if name.len() > max_len {
            return Err(EphemeralNameError::TooLong {
                prefix: prefix.to_owned(),
                actual: name.len(),
                max_len,
            });
        }
        Ok(Self(name))
    }

    /// The name, as it goes into DDL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The millisecond stamp `name` carries, or `None` if it was not minted here.
    ///
    /// Independent of the prefix and fails closed: anything not exactly this grammar is `None`.
    #[must_use]
    pub fn stamp_of(name: &str) -> Option<u64> {
        let tail = 1 + Self::STAMP_DIGITS + 1 + Self::RANDOM_CHARS;
        let split = name.len().checked_sub(tail).filter(|start| *start > 0)?;
        let (_, minted) = name.split_at(split);
        let bytes = minted.as_bytes();
        if bytes[0] != b'_' || bytes[1 + Self::STAMP_DIGITS] != b'_' {
            return None;
        }
        let stamp = minted.get(1..1 + Self::STAMP_DIGITS)?;
        let random = minted.get(2 + Self::STAMP_DIGITS..)?;
        if !random
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        stamp.parse().ok()
    }
}

impl fmt::Display for EphemeralName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for EphemeralName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<EphemeralName> for String {
    fn from(name: EphemeralName) -> Self {
        name.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQL_MAX: usize = 63;
    const CQL_MAX: usize = 48;

    #[test]
    fn mints_a_valid_unquoted_identifier() {
        let name = EphemeralName::mint("t_", CQL_MAX).expect("a plain prefix is accepted");
        assert!(
            name.as_str().starts_with("t_"),
            "{name} must carry its prefix"
        );
        assert!(
            is_valid_identifier(name.as_str()),
            "{name} must be safe to inline into DDL"
        );
        assert!(
            name.as_str().len() <= CQL_MAX,
            "{name} is {} chars",
            name.as_str().len()
        );
    }

    #[test]
    fn two_names_minted_in_the_same_millisecond_still_differ() {
        // Uniqueness rests on the random tail; a suite mints faster than the clock ticks.
        let first = EphemeralName::mint("t_", SQL_MAX).expect("valid prefix");
        let second = EphemeralName::mint("t_", SQL_MAX).expect("valid prefix");
        assert_ne!(first, second);
    }

    #[test]
    fn the_stamp_is_a_fixed_width_field() {
        let name = EphemeralName::mint("ks_", SQL_MAX).expect("valid prefix");
        let stamp = &name.as_str()["ks_".len().."ks_".len() + EphemeralName::STAMP_DIGITS];
        assert!(stamp.chars().all(|c| c.is_ascii_digit()), "{stamp}");
        assert_eq!(
            name.as_str().as_bytes()["ks_".len() + EphemeralName::STAMP_DIGITS],
            b'_',
            "the stamp must be followed by its separator"
        );
    }

    #[test]
    fn a_minted_name_reads_its_own_stamp_back() {
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_millis() as u64;
        let name = EphemeralName::mint("layer5_test_", SQL_MAX).expect("valid prefix");
        let stamp = EphemeralName::stamp_of(name.as_str()).expect("a minted name is recognised");
        assert!(stamp >= before, "{stamp} is before the name was minted");
    }

    #[test]
    fn the_recogniser_does_not_depend_on_the_prefix() {
        for prefix in [
            "t_",
            "layer5_test_",
            "layer5_witness_test_",
            "marketing_test_",
        ] {
            let name = EphemeralName::mint(prefix, SQL_MAX).expect("valid prefix");
            assert!(
                EphemeralName::stamp_of(name.as_str()).is_some(),
                "{name} must be recognised"
            );
        }
    }

    #[test]
    fn a_name_this_grammar_did_not_mint_is_left_alone() {
        for foreign in [
            "postgres",
            "template1",
            "layer5",
            "layer5_test_03dc5fb61bcf4e13be0ef8a184d83e2e",
            "t_1787095838000_0F3A91C47B2E6D58",
            "t_abcdefghijklm_0f3a91c47b2e6d58",
            "_1787095838000_0f3a91c47b2e6d58",
        ] {
            assert!(
                EphemeralName::stamp_of(foreign).is_none(),
                "{foreign} must not read as ours"
            );
        }
    }

    #[test]
    fn the_sql_pattern_is_the_grammar() {
        assert_eq!(
            EphemeralName::SQL_PATTERN,
            format!(
                "_([0-9]{{{}}})_[0-9a-f]{{{}}}$",
                EphemeralName::STAMP_DIGITS,
                EphemeralName::RANDOM_CHARS
            )
        );
    }

    #[test]
    fn rejects_an_injection_prefix() {
        let error = EphemeralName::mint("x_; DROP DATABASE prod; --", SQL_MAX)
            .expect_err("must fail closed");
        assert!(matches!(error, EphemeralNameError::InvalidPrefix { .. }));
    }

    #[test]
    fn rejects_an_empty_prefix() {
        let error =
            EphemeralName::mint("", SQL_MAX).expect_err("an empty prefix is not an identifier");
        assert!(matches!(error, EphemeralNameError::InvalidPrefix { .. }));
    }

    #[test]
    fn rejects_a_prefix_that_would_leave_the_name_unsweepable() {
        let error =
            EphemeralName::mint("t", SQL_MAX).expect_err("a prefix must carry its separator");
        assert!(matches!(
            error,
            EphemeralNameError::PrefixNotSeparated { .. }
        ));
    }

    #[test]
    fn rejects_a_prefix_that_would_overflow_the_engine_limit() {
        let overhead = EphemeralName::STAMP_DIGITS + 1 + EphemeralName::RANDOM_CHARS;
        let too_long = format!("{}_", "a".repeat(CQL_MAX - overhead));
        let error = EphemeralName::mint(&too_long, CQL_MAX).expect_err("the full name is too long");
        assert!(matches!(error, EphemeralNameError::TooLong { .. }));

        let exact = format!("{}_", "a".repeat(CQL_MAX - overhead - 1));
        assert!(
            EphemeralName::mint(&exact, CQL_MAX).is_ok(),
            "one character shorter fits exactly"
        );
    }

    #[test]
    fn the_longest_real_keyspace_prefix_fits_cql() {
        let name = EphemeralName::mint("holder_vault_test_", CQL_MAX).expect("must fit a keyspace");
        assert!(
            name.as_str().len() <= CQL_MAX,
            "{name} is {} chars",
            name.as_str().len()
        );
    }
}
