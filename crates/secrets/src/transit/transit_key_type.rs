//! [`TransitKeyType`] — the Transit key types this client signs with.

use std::fmt;

/// A Transit key type this client can sign with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransitKeyType {
    /// `ecdsa-p256` — ES256.
    EcdsaP256,
    /// `ed25519`.
    Ed25519,
    /// `rsa-2048` — RS256, for a counterparty that fixes it.
    Rsa2048,
}

impl TransitKeyType {
    /// The engine's name for this type.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EcdsaP256 => "ecdsa-p256",
            Self::Ed25519 => "ed25519",
            Self::Rsa2048 => "rsa-2048",
        }
    }

    /// Reads the engine's name for a type, or `None` for a type this client does not sign with.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "ecdsa-p256" => Some(Self::EcdsaP256),
            "ed25519" => Some(Self::Ed25519),
            "rsa-2048" => Some(Self::Rsa2048),
            _ => None,
        }
    }
}

impl fmt::Display for TransitKeyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::TransitKeyType;

    #[test]
    fn every_type_reads_back_from_its_wire_name() {
        for key_type in [
            TransitKeyType::EcdsaP256,
            TransitKeyType::Ed25519,
            TransitKeyType::Rsa2048,
        ] {
            assert_eq!(TransitKeyType::from_wire(key_type.as_str()), Some(key_type));
        }
        assert_eq!(TransitKeyType::from_wire("rsa-4096"), None);
    }
}
