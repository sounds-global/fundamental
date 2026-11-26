//! [`UntrustedToken`] — why a token is not trusted for what it asks.

use std::fmt;

/// Why a token is not trusted here: a closed set, so nothing the caller wrote is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UntrustedToken {
    /// The token names an issuer this service does not trust.
    IssuerNotAccepted,
    /// The token names no issuer, and this verifier chooses its key by issuer.
    NoIssuer,
    /// The token's principal does not own the device it acts for.
    DeviceNotOwned,
}

impl UntrustedToken {
    /// The class as it is recorded.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IssuerNotAccepted => "issuer_not_accepted",
            Self::NoIssuer => "no_issuer",
            Self::DeviceNotOwned => "device_not_owned",
        }
    }
}

impl fmt::Display for UntrustedToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_displays_as_it_is_recorded() {
        assert_eq!(
            UntrustedToken::IssuerNotAccepted.to_string(),
            "issuer_not_accepted"
        );
        assert_eq!(UntrustedToken::DeviceNotOwned.as_str(), "device_not_owned");
    }
}
