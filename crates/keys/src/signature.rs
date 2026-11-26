use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::KeyError;

/// A 64-byte ed25519 signature.
///
/// Unpadded base64 in `Display`, `FromStr` and serde.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Signature([u8; 64]);

impl Signature {
    /// Wraps 64 raw signature bytes.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Returns the raw signature bytes.
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&STANDARD_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({self})")
    }
}

impl FromStr for Signature {
    type Err = KeyError;

    /// Parses an unpadded-base64 signature.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the string is not base64 or not 64 bytes.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = STANDARD_NO_PAD
            .decode(s)
            .map_err(|e| KeyError::Malformed(format!("signature base64: {e}")))?;
        let bytes: [u8; 64] = bytes
            .try_into()
            .map_err(|_| KeyError::Malformed("signature must be 64 bytes".to_string()))?;
        Ok(Self(bytes))
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_base64() {
        let sig = Signature::from_bytes([7u8; 64]);
        let text = sig.to_string();
        let back: Signature = text.parse().expect("parse");
        assert_eq!(back, sig);
    }

    #[test]
    fn rejects_wrong_length() {
        let err = Signature::from_str(&STANDARD_NO_PAD.encode([0u8; 10])).expect_err("too short");
        assert!(matches!(err, KeyError::Malformed(_)));
    }

    #[test]
    fn rejects_non_base64() {
        assert!(matches!(
            Signature::from_str("not base64!!"),
            Err(KeyError::Malformed(_))
        ));
    }
}
