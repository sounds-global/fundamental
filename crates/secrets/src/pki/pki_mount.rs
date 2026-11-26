//! [`PkiMount`] — the path a PKI engine is mounted at.

use std::fmt;

use crate::error::SecretError;

/// The name of the path a PKI engine is mounted at, validated to `[a-z0-9-]{1,64}` since it reaches a request path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PkiMount(String);

impl PkiMount {
    /// Parses a name.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] if it does not match `[a-z0-9-]{1,64}`.
    pub fn parse(value: &str) -> Result<Self, SecretError> {
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(SecretError::InvalidConfig(
                "a PKI mount is 1-64 characters of [a-z0-9-]".to_owned(),
            ))
        }
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PkiMount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_parses() {
        assert_eq!(
            PkiMount::parse("ops-device").expect("valid").as_str(),
            "ops-device"
        );
    }

    #[test]
    fn a_name_that_could_escape_its_path_is_refused() {
        for bad in ["", "pki/other", "..", "Pki", "a b", &"x".repeat(65)] {
            assert!(PkiMount::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }
}
