//! [`ed25519_signature_bytes`] — a Transit Ed25519 signature, as bytes.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

/// Decodes a Transit Ed25519 signature after its `vault:v<n>:` prefix (64 bytes, standard base64), or `None`.
#[must_use]
pub fn ed25519_signature_bytes(encoded: &str) -> Option<[u8; 64]> {
    STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::ed25519_signature_bytes;

    #[test]
    fn a_recorded_signature_decodes_to_64_bytes() {
        // Recorded from OpenBao 2.6.2.
        assert!(
            ed25519_signature_bytes(
                "QHSbrGxmGorGRF4NncR8K8w01umObAgGSEr4aaMPkqtD0e01cvkwP2ahnIZSkhoD9k9jb0YRSI9WTdHUz91FAA=="
            )
            .is_some()
        );
    }

    #[test]
    fn a_short_or_url_safe_signature_is_refused() {
        assert!(ed25519_signature_bytes("QHSbrGxmGorGRF4N").is_none());
        assert!(ed25519_signature_bytes("xAZq-wszmSMQui_1SEIi65nAjdqyyi5Tr76OQmDppD5Tl3INE4bXQ0h3evTQ6TD1gGJyUBJJvIZeUwstuB8D6g").is_none());
    }
}
