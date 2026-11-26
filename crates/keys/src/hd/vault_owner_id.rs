use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{KeyError, VerifyingKey};

/// The BLAKE3 `derive_key` context for the owner commitment; frozen and shared by every project.
const OWNER_COMMIT_CONTEXT: &str = "fundamental/vault/owner-v1";

/// A vault hub's owner: `BLAKE3::derive_key(OWNER_COMMIT_CONTEXT, owner_public_key)`, the only
/// routing key the server sees, linked to no account.
///
/// Lowercase hex in `Display`, `FromStr` and serde.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VaultOwnerId([u8; 32]);

impl VaultOwnerId {
    /// Commits to `public_key`, yielding the owner id the hub is partitioned by.
    #[must_use]
    pub fn from_public_key(public_key: &VerifyingKey) -> Self {
        Self(blake3::derive_key(
            OWNER_COMMIT_CONTEXT,
            &public_key.to_bytes(),
        ))
    }

    /// The raw 32 commitment bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for VaultOwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for VaultOwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VaultOwnerId({self})")
    }
}

impl FromStr for VaultOwnerId {
    type Err = KeyError;

    /// Parses a 64-character lowercase-hex owner id.
    ///
    /// # Errors
    /// [`KeyError::Malformed`] if the string is not 64 hex characters.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(KeyError::Malformed(
                "vault owner id must be 64 hex characters".into(),
            ));
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|_| KeyError::Malformed("vault owner id is not valid hex".into()))?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for VaultOwnerId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for VaultOwnerId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_is_deterministic_for_a_key() {
        let pk = VerifyingKey::from_bytes([7u8; 32]);
        assert_eq!(
            VaultOwnerId::from_public_key(&pk),
            VaultOwnerId::from_public_key(&pk)
        );
    }

    #[test]
    fn distinct_keys_commit_to_distinct_owner_ids() {
        let a = VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([1u8; 32]));
        let b = VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([2u8; 32]));
        assert_ne!(a, b);
    }

    #[test]
    fn commitment_is_domain_separated_from_a_bare_hash() {
        let pk = VerifyingKey::from_bytes([9u8; 32]);
        let bare: [u8; 32] = blake3::hash(&pk.to_bytes()).into();
        assert_ne!(VaultOwnerId::from_public_key(&pk).as_bytes(), &bare);
    }

    #[test]
    fn commitment_matches_the_frozen_known_answer() {
        // A frozen vector: changing it re-derives every owner id.
        let pk = VerifyingKey::from_bytes([7u8; 32]);
        assert_eq!(
            VaultOwnerId::from_public_key(&pk).to_string(),
            "0745889351cda614855be46a395ba7c19e14a97549d09911e7d1777c2236946b",
        );
    }

    #[test]
    fn hex_round_trips() {
        let id = VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([5u8; 32]));
        let text = id.to_string();
        assert_eq!(text.len(), 64);
        assert_eq!(VaultOwnerId::from_str(&text).expect("parse"), id);
    }

    #[test]
    fn rejects_malformed_hex() {
        assert!(matches!(
            VaultOwnerId::from_str("nothex"),
            Err(KeyError::Malformed(_))
        ));
        assert!(matches!(
            VaultOwnerId::from_str(&"zz".repeat(32)),
            Err(KeyError::Malformed(_))
        ));
    }
}
