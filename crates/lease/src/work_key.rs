use std::fmt;

use crate::LeaseError;

/// What a piece of fenced work is, derived from the work and never a clock: `due:4f1c…`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkKey(String);

impl WorkKey {
    /// The key `key`.
    ///
    /// # Errors
    ///
    /// [`LeaseError::InvalidWorkKey`] outside `[A-Za-z0-9._:/-]{1,200}`.
    pub fn new(key: impl Into<String>) -> Result<Self, LeaseError> {
        let key = key.into();
        let valid = (1..=200).contains(&key.len())
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'));
        if valid {
            Ok(Self(key))
        } else {
            Err(LeaseError::InvalidWorkKey {
                key: key.chars().take(200).collect(),
            })
        }
    }

    /// The key as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_work_key_is_bounded_and_printable() {
        assert!(WorkKey::new("due:4f1c-2026-10-08").is_ok());
        for key in [
            String::new(),
            "a b".to_owned(),
            "x".repeat(201),
            "ü".to_owned(),
        ] {
            assert_eq!(
                WorkKey::new(key).expect_err("refused").error_code(),
                "SLEASE_011"
            );
        }
    }
}
