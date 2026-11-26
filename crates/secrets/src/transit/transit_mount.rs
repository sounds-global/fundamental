//! [`TransitMount`] — the path a Transit engine is mounted at.

use std::fmt;

use fundamental_config::settings::is_transit_mount;

use crate::error::SecretError;

/// The mount path of a Transit engine, validated to `[a-z0-9-]{1,64}` since it reaches a request path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransitMount(String);

impl TransitMount {
    /// Parses a mount name.
    ///
    /// # Errors
    /// [`SecretError::InvalidConfig`] if it does not match `[a-z0-9-]{1,64}`.
    pub fn parse(value: &str) -> Result<Self, SecretError> {
        if is_transit_mount(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(SecretError::InvalidConfig(
                "a transit mount is 1-64 characters of [a-z0-9-]".to_owned(),
            ))
        }
    }

    /// The mount name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TransitMount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_mount_parses() {
        assert_eq!(
            TransitMount::parse("transit-tenants")
                .expect("valid")
                .as_str(),
            "transit-tenants"
        );
    }

    #[test]
    fn a_mount_that_could_escape_its_path_is_refused() {
        for bad in ["", "transit/other", "..", "Transit", "a b", &"x".repeat(65)] {
            assert!(TransitMount::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }
}
