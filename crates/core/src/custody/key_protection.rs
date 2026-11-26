//! [`KeyProtection`] — how a signing key is held, and whether that posture is accreditable.

#[cfg(not(feature = "std"))]
use alloc::borrow::ToOwned;

use super::UnknownKeyProtection;

/// How a signing key's private half is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyProtection {
    /// An in-process software key (dev/fixture only, **not accreditable**).
    Software,
    /// A key held in a PKCS#11 HSM (or cloud/managed HSM), non-exportable.
    Hsm,
    /// A key held in a hardware-backed secure enclave, non-exportable.
    Enclave,
}

impl KeyProtection {
    /// The stable wire token for this posture (`software` / `hsm` / `enclave`).
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Hsm => "hsm",
            Self::Enclave => "enclave",
        }
    }

    /// Parses a posture token, failing closed on anything unrecognised.
    ///
    /// # Errors
    /// Returns [`UnknownKeyProtection`] for a token other than `software`, `hsm` or `enclave`.
    pub fn parse(token: &str) -> Result<Self, UnknownKeyProtection> {
        match token {
            "software" => Ok(Self::Software),
            "hsm" => Ok(Self::Hsm),
            "enclave" => Ok(Self::Enclave),
            other => Err(UnknownKeyProtection {
                token: other.to_owned(),
            }),
        }
    }

    /// Whether a key with this posture may be accredited: only hardware-backed keys qualify.
    #[must_use]
    pub fn is_accreditable(&self) -> bool {
        !matches!(self, Self::Software)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_is_not_accreditable_hardware_is() {
        assert!(!KeyProtection::Software.is_accreditable());
        assert!(KeyProtection::Hsm.is_accreditable());
        assert!(KeyProtection::Enclave.is_accreditable());
    }

    #[test]
    fn as_str_round_trips_through_parse() {
        for protection in [
            KeyProtection::Software,
            KeyProtection::Hsm,
            KeyProtection::Enclave,
        ] {
            assert_eq!(
                KeyProtection::parse(protection.as_str()).expect("token parses"),
                protection
            );
        }
    }

    #[test]
    fn parse_rejects_an_unknown_token() {
        assert_eq!(
            KeyProtection::parse("software-ish"),
            Err(UnknownKeyProtection {
                token: "software-ish".to_owned()
            })
        );
        assert!(KeyProtection::parse("").is_err());
    }
}
