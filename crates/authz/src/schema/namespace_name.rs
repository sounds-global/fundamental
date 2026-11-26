//! [`NamespaceName`]: the validated name of an object type.

use core::fmt;

use fundamental_db::ident::is_valid_identifier;
use serde::{Deserialize, Serialize};

use crate::error::AuthzError;

/// The name of a namespace (object type), e.g. `document`, `folder`, `group`.
///
/// A safe identifier, validated at construction, since it is interpolated into stored
/// subjects and query identifiers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NamespaceName(String);

impl NamespaceName {
    /// Validates `raw` and wraps it as a namespace name.
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
                kind: "namespace",
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

impl fmt::Display for NamespaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for NamespaceName {
    type Error = AuthzError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NamespaceName> for String {
    fn from(value: NamespaceName) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_safe_identifier() {
        let ns = NamespaceName::new("document").expect("valid");
        assert_eq!(ns.as_str(), "document");
        assert_eq!(ns.to_string(), "document");
    }

    #[test]
    fn rejects_an_injection_payload() {
        let err = NamespaceName::new("doc; DROP TABLE x").expect_err("must reject");
        match err {
            AuthzError::InvalidName { kind, value } => {
                assert_eq!(kind, "namespace");
                assert_eq!(value, "doc; DROP TABLE x");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_and_leading_digit() {
        assert!(NamespaceName::new("").is_err());
        assert!(NamespaceName::new("1doc").is_err());
    }

    #[test]
    fn deserializes_through_validation() {
        let ok: NamespaceName = serde_json::from_str("\"folder\"").expect("valid json");
        assert_eq!(ok.as_str(), "folder");
        let bad = serde_json::from_str::<NamespaceName>("\"a b\"");
        assert!(bad.is_err(), "invalid identifier must fail to deserialize");
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let ns = NamespaceName::new("group").expect("valid");
        assert_eq!(serde_json::to_string(&ns).expect("serialize"), "\"group\"");
    }
}
