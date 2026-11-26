//! [`IntoCanonicalId`]: the object-id inputs the ergonomic API accepts.

use super::CanonicalId;
use crate::error::AuthzError;

/// A value that can be validated into a [`CanonicalId`].
///
/// Fallible; the conversion runs in the method it is passed to, so its error is the
/// operation's own. A typed id converts through its `Display`.
pub trait IntoCanonicalId {
    /// Converts `self` into a validated [`CanonicalId`].
    ///
    /// # Errors
    ///
    /// Returns [`AuthzError::InvalidCanonicalId`] if the value is not a safe identifier.
    fn into_canonical_id(self) -> Result<CanonicalId, AuthzError>;
}

impl IntoCanonicalId for CanonicalId {
    fn into_canonical_id(self) -> Result<CanonicalId, AuthzError> {
        Ok(self)
    }
}

impl IntoCanonicalId for &str {
    fn into_canonical_id(self) -> Result<CanonicalId, AuthzError> {
        CanonicalId::new(self)
    }
}

impl IntoCanonicalId for String {
    fn into_canonical_id(self) -> Result<CanonicalId, AuthzError> {
        CanonicalId::new(self)
    }
}

impl IntoCanonicalId for &String {
    fn into_canonical_id(self) -> Result<CanonicalId, AuthzError> {
        CanonicalId::new(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_core::Error;

    #[test]
    fn a_valid_str_converts() {
        let id = "readme".into_canonical_id().expect("valid");
        assert_eq!(id.as_str(), "readme");
    }

    #[test]
    fn an_owned_string_converts() {
        let id = String::from("spec").into_canonical_id().expect("valid");
        assert_eq!(id.as_str(), "spec");
    }

    #[test]
    fn an_existing_canonical_id_passes_through() {
        let original = CanonicalId::new("readme").unwrap();
        let round = original.clone().into_canonical_id().expect("identity");
        assert_eq!(round, original);
    }

    #[test]
    fn an_invalid_id_surfaces_the_authz_code() {
        let err = "has a space".into_canonical_id().expect_err("invalid");
        assert_eq!(err.error_code(), "AUTHZ_006");
    }
}
