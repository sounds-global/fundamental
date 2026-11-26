use fundamental_core::secrets::Secret;

use crate::KeyError;
use crate::hd::seal_aead::{NONCE_LEN, open_with_aad, random_nonce, seal_with_aad};

const ENVELOPE_VERSION: u8 = 0x01;

const MIN_ENVELOPE_LEN: usize = 1 + NONCE_LEN + 16;

/// Seals and opens a single vault row's plaintext under a **per-row content subkey**
/// (from [`VaultContentKey::row_subkey`](crate::hd::VaultContentKey::row_subkey)).
///
/// The envelope is `version(1) ‖ nonce(12) ‖ ciphertext ‖ tag(16)`, with `aad` (`owner_id ‖
/// row_id`) bound so a server cannot move a ciphertext to another row; the per-row subkey bounds
/// nonce reuse to one row.
pub struct VaultCipher {
    subkey: Secret<[u8; 32]>,
}

impl VaultCipher {
    /// Wraps a per-row subkey for sealing and opening that row.
    #[must_use]
    pub fn new(subkey: Secret<[u8; 32]>) -> Self {
        Self { subkey }
    }

    /// Seals `plaintext` into a self-describing envelope, binding `aad` into the tag.
    ///
    /// # Errors
    /// [`KeyError::Seal`] if entropy or the AEAD fails.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, KeyError> {
        let nonce = random_nonce()?;
        let ciphertext = seal_with_aad(self.subkey.expose(), &nonce, aad, plaintext)?;
        let mut envelope = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        envelope.push(ENVELOPE_VERSION);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    /// Opens an `envelope` produced by [`seal`](Self::seal) under the same `aad`.
    ///
    /// # Errors
    /// [`KeyError::Malformed`] if the envelope is too short or of an unknown version;
    /// [`KeyError::Unseal`] if the AEAD open fails.
    pub fn open(&self, aad: &[u8], envelope: &[u8]) -> Result<Vec<u8>, KeyError> {
        if envelope.len() < MIN_ENVELOPE_LEN {
            return Err(KeyError::Malformed("vault envelope too short".into()));
        }
        if envelope[0] != ENVELOPE_VERSION {
            return Err(KeyError::Malformed(format!(
                "unknown vault envelope version {}",
                envelope[0]
            )));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&envelope[1..1 + NONCE_LEN]);
        let ciphertext = &envelope[1 + NONCE_LEN..];
        open_with_aad(self.subkey.expose(), &nonce, aad, ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(seed: u8) -> VaultCipher {
        VaultCipher::new(Secret::new([seed; 32]))
    }

    #[test]
    fn seal_then_open_round_trips() {
        let c = cipher(0x10);
        let env = c
            .seal(b"owner|row|1", b"a stored credential")
            .expect("seal");
        assert_ne!(&env[1 + NONCE_LEN..], b"a stored credential");
        let pt = c.open(b"owner|row|1", &env).expect("open");
        assert_eq!(pt, b"a stored credential");
    }

    #[test]
    fn open_under_a_different_aad_fails_closed() {
        let c = cipher(0x11);
        let env = c.seal(b"owner|row|1", b"x").expect("seal");
        assert!(matches!(
            c.open(b"owner|row|2", &env),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn open_under_a_different_subkey_fails_closed() {
        let env = cipher(0x12).seal(b"aad", b"x").expect("seal");
        assert!(matches!(
            cipher(0x13).open(b"aad", &env),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn a_tampered_ciphertext_fails_closed() {
        let c = cipher(0x14);
        let mut env = c.seal(b"aad", b"secret").expect("seal");
        let last = env.len() - 1;
        env[last] ^= 0xFF;
        assert!(matches!(c.open(b"aad", &env), Err(KeyError::Unseal)));
    }

    #[test]
    fn a_short_envelope_is_malformed() {
        let c = cipher(0x15);
        assert!(matches!(
            c.open(b"aad", &[0x01, 0x02]),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn an_unknown_version_is_malformed() {
        let c = cipher(0x16);
        let mut env = c.seal(b"aad", b"x").expect("seal");
        env[0] = 0xFE;
        assert!(matches!(c.open(b"aad", &env), Err(KeyError::Malformed(_))));
    }

    #[test]
    fn nonces_differ_across_seals() {
        let c = cipher(0x17);
        let a = c.seal(b"aad", b"x").expect("a");
        let b = c.seal(b"aad", b"x").expect("b");
        assert_ne!(a[1..1 + NONCE_LEN], b[1..1 + NONCE_LEN]);
    }
}
