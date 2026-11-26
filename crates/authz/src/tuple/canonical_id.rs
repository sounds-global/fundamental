//! [`CanonicalId`]: the erased stored form of any object or subject identifier.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::error::AuthzError;

/// The wildcard identifier: a public grant to every subject of a namespace.
pub const WILDCARD: &str = "*";

const MAX_ID_LEN: usize = 256;

/// The canonical, backend-neutral form of an object or subject id.
///
/// Ids of every namespace share one store, so they are erased to this string. It rejects
/// only empty, over-long, or control or whitespace characters; `*` ([`WILDCARD`]) is the
/// public grant. Ids are always bound as parameters.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CanonicalId(String);

impl CanonicalId {
    /// Validates `raw` and wraps it as a canonical id.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if `raw` is empty, longer than
    /// 256 bytes, or contains an ASCII control or whitespace character.
    pub fn new(raw: impl Into<String>) -> Result<Self, AuthzError> {
        let raw = raw.into();
        if raw.is_empty()
            || raw.len() > MAX_ID_LEN
            || raw.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(AuthzError::InvalidCanonicalId {
                value: raw.chars().take(64).collect(),
            });
        }
        Ok(Self(raw))
    }

    /// The wildcard id (`*`) — a public grant to every subject of a namespace.
    #[must_use]
    pub fn wildcard() -> Self {
        Self(WILDCARD.to_string())
    }

    /// Whether this id is the wildcard `*`.
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.0 == WILDCARD
    }

    /// Returns the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CanonicalId {
    type Error = AuthzError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CanonicalId> for String {
    fn from(value: CanonicalId) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_opaque_ids() {
        for ok in [
            "550e8400-e29b-41d4-a716-446655440000",
            "user@example.com",
            "urn:acme:doc:42",
            "1234567890",
            "*",
        ] {
            assert!(CanonicalId::new(ok).is_ok(), "{ok} should be accepted");
        }
    }

    #[test]
    fn wildcard_is_recognized() {
        assert!(CanonicalId::wildcard().is_wildcard());
        assert!(CanonicalId::new("*").expect("valid").is_wildcard());
        assert!(!CanonicalId::new("x").expect("valid").is_wildcard());
    }

    #[test]
    fn rejects_empty_control_and_whitespace() {
        assert!(CanonicalId::new("").is_err());
        assert!(CanonicalId::new("a b").is_err(), "whitespace rejected");
        assert!(CanonicalId::new("a\tb").is_err(), "tab rejected");
        assert!(CanonicalId::new("a\nb").is_err(), "newline rejected");
        assert!(CanonicalId::new("a\0b").is_err(), "NUL rejected");
    }

    #[test]
    fn rejects_oversized_and_truncates_in_error() {
        let too_long = "a".repeat(MAX_ID_LEN + 1);
        match CanonicalId::new(&too_long).expect_err("must reject") {
            AuthzError::InvalidCanonicalId { value } => assert_eq!(value.len(), 64),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn deserializes_through_validation() {
        assert!(serde_json::from_str::<CanonicalId>("\"ok-id\"").is_ok());
        assert!(serde_json::from_str::<CanonicalId>("\"bad id\"").is_err());
    }
}
