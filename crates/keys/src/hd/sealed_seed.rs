use crate::KeyError;

/// The frozen envelope version.
const ENVELOPE_VERSION: u8 = 1;

const NONCE_LEN: usize = 12;

/// An encrypted master seed at rest: KDF `salt` (empty unless password-derived), AEAD `nonce`,
/// and `ciphertext` with its tag; safe to persist anywhere.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedSeed {
    salt: Vec<u8>,
    nonce: [u8; NONCE_LEN],
    ciphertext: Vec<u8>,
}

impl SealedSeed {
    #[cfg(any(
        test,
        target_arch = "wasm32",
        feature = "os-keychain",
        feature = "keys-hd-password"
    ))]
    pub(crate) fn new(salt: Vec<u8>, nonce: [u8; NONCE_LEN], ciphertext: Vec<u8>) -> Self {
        Self {
            salt,
            nonce,
            ciphertext,
        }
    }

    #[cfg(any(
        test,
        target_arch = "wasm32",
        feature = "os-keychain",
        feature = "keys-hd-password"
    ))]
    pub(crate) fn salt(&self) -> &[u8] {
        &self.salt
    }

    #[cfg(any(
        test,
        target_arch = "wasm32",
        feature = "os-keychain",
        feature = "keys-hd-password"
    ))]
    pub(crate) fn nonce(&self) -> &[u8; NONCE_LEN] {
        &self.nonce
    }

    #[cfg(any(
        test,
        target_arch = "wasm32",
        feature = "os-keychain",
        feature = "keys-hd-password"
    ))]
    pub(crate) fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// The canonical byte encoding: `version ‖ salt_len ‖ salt ‖ nonce ‖ ciphertext`.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + self.salt.len() + NONCE_LEN + self.ciphertext.len());
        out.push(ENVELOPE_VERSION);
        out.push(u8::try_from(self.salt.len()).unwrap_or(u8::MAX));
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Parses the canonical encoding.
    ///
    /// # Errors
    /// [`KeyError::Unseal`] if the version is unknown or the buffer is truncated.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        if bytes.len() < 2 + NONCE_LEN + 16 || bytes[0] != ENVELOPE_VERSION {
            return Err(KeyError::Unseal);
        }
        let salt_len = bytes[1] as usize;
        let salt_end = 2 + salt_len;
        let nonce_end = salt_end + NONCE_LEN;
        if bytes.len() < nonce_end + 16 {
            return Err(KeyError::Unseal);
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[salt_end..nonce_end]);
        Ok(Self {
            salt: bytes[2..salt_end].to_vec(),
            nonce,
            ciphertext: bytes[nonce_end..].to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_bytes() {
        let sealed = SealedSeed::new(vec![1, 2, 3, 4], [9u8; NONCE_LEN], vec![0xAB; 80]);
        let parsed = SealedSeed::from_bytes(&sealed.to_bytes()).expect("round trip");
        assert_eq!(parsed, sealed);
    }

    #[test]
    fn round_trips_with_an_empty_salt() {
        let sealed = SealedSeed::new(Vec::new(), [3u8; NONCE_LEN], vec![0xCD; 80]);
        let parsed = SealedSeed::from_bytes(&sealed.to_bytes()).expect("round trip");
        assert_eq!(parsed, sealed);
        assert!(parsed.salt().is_empty());
    }

    #[test]
    fn rejects_a_wrong_version() {
        let mut bytes = SealedSeed::new(vec![], [0u8; NONCE_LEN], vec![0; 80]).to_bytes();
        bytes[0] = 2;
        assert!(matches!(
            SealedSeed::from_bytes(&bytes),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn rejects_a_truncated_envelope() {
        assert!(matches!(
            SealedSeed::from_bytes(&[1, 0, 0]),
            Err(KeyError::Unseal)
        ));
        assert!(matches!(SealedSeed::from_bytes(&[]), Err(KeyError::Unseal)));
    }
}
