//! [`ed25519_public_key_bytes`] — a Transit Ed25519 public key, as bytes.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Decodes a Transit Ed25519 public key (32 bytes, standard base64), or `None`.
#[must_use]
pub fn ed25519_public_key_bytes(encoded: &str) -> Option<[u8; 32]> {
    STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::ed25519_public_key_bytes;

    #[test]
    fn a_recorded_key_decodes_to_32_bytes() {
        // Recorded from OpenBao 2.6.2.
        assert!(ed25519_public_key_bytes("mw0hx09eOCQl5Pb6dK4fj2UeqyIsUj66XUhreBeTAPA=").is_some());
    }

    #[test]
    fn a_pem_or_short_key_is_refused() {
        assert!(ed25519_public_key_bytes("-----BEGIN PUBLIC KEY-----").is_none());
        assert!(ed25519_public_key_bytes("mw0hx09eOCQl5Pb6").is_none());
    }
}
