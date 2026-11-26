//! [`TransitKeyName`] — the name of one key in a Transit engine.

use std::fmt;

use fundamental_config::settings::is_transit_key_name;

use crate::error::SecretError;

/// A Transit key name, validated to `[a-z0-9_-]{1,128}`; derived from a server-generated id, never a customer's name.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TransitKeyName(String);

impl TransitKeyName {
    /// Parses a key name.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] if it does not match `[a-z0-9_-]{1,128}`.
    pub fn parse(value: &str) -> Result<Self, SecretError> {
        if is_transit_key_name(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(SecretError::InvalidConfig(
                "a transit key name is 1-128 characters of [a-z0-9_-]".to_owned(),
            ))
        }
    }

    /// The key name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TransitKeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// States only the length, so a stray `?name` in a log cannot map a key to its tenant.
impl fmt::Debug for TransitKeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TransitKeyName({} chars)", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_name_parses() {
        assert!(TransitKeyName::parse("t-0f3a_k1").is_ok());
    }

    #[test]
    fn a_name_that_could_escape_its_path_is_refused() {
        for bad in ["", "a/b", "..", "K1", "a.b", &"x".repeat(129)] {
            assert!(
                TransitKeyName::parse(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn debug_does_not_print_the_name() {
        let name = TransitKeyName::parse("t-secret-tenant").expect("valid");
        assert!(!format!("{name:?}").contains("secret-tenant"));
    }
}
