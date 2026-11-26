//! [`RelationName`]: the validated name of a relation on a namespace.

use core::fmt;

use fundamental_db::ident::is_valid_identifier;
use serde::{Deserialize, Serialize};

use crate::error::AuthzError;

/// The name of a relation, e.g. `owner`, `editor`, `viewer`, `parent`, `member`.
///
/// A safe identifier, validated at construction, for the same reason as
/// [`NamespaceName`](super::NamespaceName).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelationName(String);

impl RelationName {
    /// Validates `raw` and wraps it as a relation name.
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidName`] if `raw` is not a safe identifier.
    pub fn new(raw: impl Into<String>) -> Result<Self, AuthzError> {
        let raw = raw.into();
        if is_valid_identifier(&raw) {
            Ok(Self(raw))
        } else {
            Err(AuthzError::InvalidName {
                kind: "relation",
                value: raw,
            })
        }
    }

    /// Returns the name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RelationName {
    type Error = AuthzError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<RelationName> for String {
    fn from(value: RelationName) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_safe_identifier() {
        let rel = RelationName::new("viewer").expect("valid");
        assert_eq!(rel.as_str(), "viewer");
    }

    #[test]
    fn rejects_an_injection_payload() {
        let err = RelationName::new("viewer\" OR \"1\"=\"1").expect_err("must reject");
        assert!(matches!(
            err,
            AuthzError::InvalidName {
                kind: "relation",
                ..
            }
        ));
    }

    #[test]
    fn deserializes_through_validation() {
        let ok: RelationName = serde_json::from_str("\"member\"").expect("valid");
        assert_eq!(ok.as_str(), "member");
        assert!(serde_json::from_str::<RelationName>("\"a.b\"").is_err());
    }
}
