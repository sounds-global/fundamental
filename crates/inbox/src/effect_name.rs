//! [`EffectName`] — the name an effect's ledger is kept under.

use std::fmt;

use crate::InboxError;

/// The declared name of one effect — `billing.book-deposit` — under which the inbox records
/// every event it has applied.
///
/// Keyed on this, not on stream and consumer, so a renamed consumer replays into the same ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectName(&'static str);

impl EffectName {
    /// The longest name accepted.
    pub const MAX_LEN: usize = 100;

    /// Checks `name` against `[a-z0-9][a-z0-9._-]{0,99}`.
    ///
    /// # Errors
    ///
    /// [`InboxError::InvalidEffectName`] for anything else.
    pub fn new(name: &'static str) -> Result<Self, InboxError> {
        if Self::is_valid(name) {
            Ok(Self(name))
        } else {
            Err(InboxError::InvalidEffectName {
                name: name.to_owned(),
            })
        }
    }

    /// Whether `name` is a valid effect name; the one rule, shared with `#[subscribe]`.
    #[must_use]
    pub const fn is_valid(name: &str) -> bool {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.len() > Self::MAX_LEN {
            return false;
        }
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            let alnum = byte.is_ascii_lowercase() || byte.is_ascii_digit();
            let punctuation = byte == b'.' || byte == b'_' || byte == b'-';
            if !(alnum || (index > 0 && punctuation)) {
                return false;
            }
            index += 1;
        }
        true
    }

    /// The name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for EffectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_dotted_lower_case_name_is_accepted() {
        let name = EffectName::new("billing.book-deposit").expect("valid");
        assert_eq!(name.as_str(), "billing.book-deposit");
    }

    #[test]
    fn every_malformed_name_is_refused_with_its_code() {
        for name in [
            "",
            "Billing",
            ".leading",
            "has space",
            "ü",
            &*"x".repeat(101).leak(),
        ] {
            let error = EffectName::new(name).expect_err(name);
            assert!(
                matches!(error, InboxError::InvalidEffectName { .. }),
                "{name}"
            );
            assert_eq!(error.error_code(), "SINBOX_011");
        }
    }

    #[test]
    fn a_hundred_characters_is_the_limit() {
        assert!(EffectName::is_valid(&"a".repeat(100)));
        assert!(!EffectName::is_valid(&"a".repeat(101)));
    }
}
