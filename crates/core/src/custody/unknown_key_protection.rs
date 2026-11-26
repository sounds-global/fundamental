//! [`UnknownKeyProtection`] — a custody posture token nobody defined.

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;
#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
#[cfg(feature = "std")]
use std::borrow::Cow;

/// A key-protection token that is not `software`, `hsm` or `enclave`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownKeyProtection {
    /// The token as written.
    pub token: String,
}

impl fmt::Display for UnknownKeyProtection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown key protection: {}", self.token)
    }
}

impl core::error::Error for UnknownKeyProtection {}

impl crate::Error for UnknownKeyProtection {
    fn error_code(&self) -> &'static str {
        "CUSTODY_001"
    }

    fn display_message(&self) -> Cow<'_, str> {
        Cow::Borrowed("The key protection is not one this system defines.")
    }

    fn http_status_code(&self) -> u16 {
        400
    }
}

#[cfg(test)]
mod tests {
    use super::UnknownKeyProtection;
    use crate::Error as _;

    #[test]
    fn it_names_the_token_to_an_operator_and_not_to_a_caller() {
        let error = UnknownKeyProtection {
            token: "hsm-ish".to_owned(),
        };
        assert_eq!(error.error_code(), "CUSTODY_001");
        assert!(error.to_string().contains("hsm-ish"));
        assert!(!error.display_message().contains("hsm-ish"));
    }
}
