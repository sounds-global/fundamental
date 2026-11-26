use std::fmt;

use crate::LeaseError;

/// Which singleton a lease guards: `[a-z][a-z0-9-]{0,62}`, the key of its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LeaseName(&'static str);

impl LeaseName {
    /// The name `name`.
    ///
    /// # Errors
    ///
    /// [`LeaseError::InvalidName`] outside `[a-z][a-z0-9-]{0,62}`.
    pub fn new(name: &'static str) -> Result<Self, LeaseError> {
        let mut chars = name.chars();
        let valid = name.len() <= 63
            && chars.next().is_some_and(|first| first.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid {
            Ok(Self(name))
        } else {
            Err(LeaseError::InvalidName {
                name: name.chars().take(64).collect(),
            })
        }
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for LeaseName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::*;

    #[test]
    fn a_lease_name_outside_the_grammar_is_refused() {
        assert!(LeaseName::new("layer1-maintenance").is_ok());
        for name in ["", "Keeper", "1keeper", "keeper_1", "kéeper"] {
            let error = LeaseName::new(name).expect_err(name);
            assert_eq!(error.error_code(), "SLEASE_003");
        }
        assert!(LeaseName::new(Box::leak("a".repeat(64).into_boxed_str())).is_err());
    }
}
