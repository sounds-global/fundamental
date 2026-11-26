//! [`CaveatName`]: the validated name of a caveat (a conditional predicate).

use core::fmt;

use fundamental_db::ident::is_valid_identifier;
use serde::{Deserialize, Serialize};

use crate::error::AuthzError;

/// The name of a caveat, e.g. `business_hours`, `within_region`, `not_expired`.
///
/// A safe identifier, validated at construction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CaveatName(String);

impl CaveatName {
    /// Validates `raw` and wraps it as a caveat name.
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
                kind: "caveat",
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

impl fmt::Display for CaveatName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CaveatName {
    type Error = AuthzError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CaveatName> for String {
    fn from(value: CaveatName) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_rejects() {
        assert_eq!(
            CaveatName::new("business_hours").expect("valid").as_str(),
            "business_hours"
        );
        assert!(matches!(
            CaveatName::new("bad name").expect_err("reject"),
            AuthzError::InvalidName { kind: "caveat", .. }
        ));
    }
}
