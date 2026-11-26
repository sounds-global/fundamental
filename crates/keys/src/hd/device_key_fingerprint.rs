use std::fmt;

use crate::VerifyingKey;

/// The BLAKE3 `derive_key` context for a device key's fingerprint; frozen and unique.
const FINGERPRINT_CONTEXT: &str = "fundamental/keys/device-fingerprint-v1";

/// Bytes kept: enough to tell a user's devices apart in a log, short of an index.
const FINGERPRINT_BYTES: usize = 8;

/// A short, stable name for a device's public key, derived by the server.
///
/// `BLAKE3::derive_key(context, public key)`, truncated, as lowercase hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceKeyFingerprint([u8; FINGERPRINT_BYTES]);

impl DeviceKeyFingerprint {
    /// The fingerprint of `key`.
    #[must_use]
    pub fn of(key: &VerifyingKey) -> Self {
        let derived = blake3::derive_key(FINGERPRINT_CONTEXT, &key.to_bytes());
        let mut kept = [0u8; FINGERPRINT_BYTES];
        kept.copy_from_slice(&derived[..FINGERPRINT_BYTES]);
        Self(kept)
    }
}

impl fmt::Display for DeviceKeyFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for DeviceKeyFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "DeviceKeyFingerprint({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VaultOwnerId;

    #[test]
    fn a_fingerprint_is_stable_and_names_one_key() {
        let one = VerifyingKey::from_bytes([7; 32]);
        let two = VerifyingKey::from_bytes([8; 32]);
        assert_eq!(
            DeviceKeyFingerprint::of(&one),
            DeviceKeyFingerprint::of(&one)
        );
        assert_ne!(
            DeviceKeyFingerprint::of(&one),
            DeviceKeyFingerprint::of(&two)
        );
        assert_eq!(DeviceKeyFingerprint::of(&one).to_string().len(), 16);
    }

    #[test]
    fn a_fingerprint_is_not_the_vault_owner_id_of_the_same_key() {
        let key = VerifyingKey::from_bytes([7; 32]);
        let owner = VaultOwnerId::from_public_key(&key).to_string();
        assert!(!owner.starts_with(&DeviceKeyFingerprint::of(&key).to_string()));
    }

    #[test]
    fn the_derivation_is_frozen() {
        let key = VerifyingKey::from_bytes([0; 32]);
        let expected = blake3::derive_key(FINGERPRINT_CONTEXT, &[0; 32]);
        let hex: String = expected[..8].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(DeviceKeyFingerprint::of(&key).to_string(), hex);
        assert_eq!(
            FINGERPRINT_CONTEXT,
            "fundamental/keys/device-fingerprint-v1"
        );
    }
}
