use std::fmt;
use std::str::FromStr;

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use ed25519_dalek::{Verifier, VerifyingKey as DalekVerifyingKey};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use tracing::warn;

use crate::{KeyError, Signature};

/// A device's ed25519 public key (32 bytes).
///
/// Unpadded base64 in `Display`, `FromStr` and serde.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VerifyingKey([u8; 32]);

impl VerifyingKey {
    /// Wraps 32 raw public-key bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw public-key bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Verifies that `signature` is a valid signature of `message` by this key.
    ///
    /// # Errors
    /// Returns [`KeyError::BadSignature`] if the signature does not verify, or
    /// [`KeyError::Malformed`] if the stored key bytes are not a valid point.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), KeyError> {
        let key = DalekVerifyingKey::from_bytes(&self.0)
            .map_err(|e| KeyError::Malformed(format!("public key: {e}")))?;
        let dalek_sig = ed25519_dalek::Signature::from_bytes(&signature.to_bytes());
        key.verify(message, &dalek_sig).map_err(|_| {
            warn!("device signature did not verify");
            KeyError::BadSignature
        })
    }
}

impl fmt::Display for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&STANDARD_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifyingKey({self})")
    }
}

impl FromStr for VerifyingKey {
    type Err = KeyError;

    /// Parses an unpadded-base64 public key.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the string is not base64 or not 32 bytes.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = STANDARD_NO_PAD
            .decode(s)
            .map_err(|e| KeyError::Malformed(format!("public key base64: {e}")))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| KeyError::Malformed("public key must be 32 bytes".to_string()))?;
        Ok(Self(bytes))
    }
}

impl Serialize for VerifyingKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for VerifyingKey {
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
        let key = VerifyingKey::from_bytes([3u8; 32]);
        let back: VerifyingKey = key.to_string().parse().expect("parse");
        assert_eq!(back, key);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(
            VerifyingKey::from_str(&STANDARD_NO_PAD.encode([0u8; 5])),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn verify_rejects_a_garbage_key() {
        // All-zero bytes are not a valid Edwards point; verify must error, not panic.
        let key = VerifyingKey::from_bytes([0u8; 32]);
        let err = key
            .verify(b"hello", &Signature::from_bytes([0u8; 64]))
            .expect_err("must fail");
        assert!(matches!(
            err,
            KeyError::BadSignature | KeyError::Malformed(_)
        ));
    }
}
