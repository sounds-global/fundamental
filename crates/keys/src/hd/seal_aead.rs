//! AES-256-GCM over pure-Rust `aes-gcm`, one backend on native and `wasm32`.

use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Aead, KeyInit, Payload};

use crate::KeyError;
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
use crate::hd::{MasterSeed, SealedSeed};
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
use zeroize::Zeroize;

pub(crate) const NONCE_LEN: usize = 12;

/// Seals a [`MasterSeed`] under a 32-byte wrapping `key`, tagging the envelope with
/// `salt` (the KDF salt for a password sealer; empty for a keychain key).
///
/// # Errors
/// [`KeyError::Seal`] if entropy or the AEAD fails.
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
pub(crate) fn seal_seed(
    key: &[u8; 32],
    salt: Vec<u8>,
    seed: &MasterSeed,
) -> Result<SealedSeed, KeyError> {
    let nonce = random_nonce()?;
    let ciphertext = seal(key, &nonce, seed.expose())?;
    Ok(SealedSeed::new(salt, nonce, ciphertext))
}

/// Recovers a [`MasterSeed`] from `sealed` under wrapping `key`.
///
/// # Errors
/// [`KeyError::Unseal`] on a wrong key or a tampered or short envelope.
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
pub(crate) fn unseal_seed(key: &[u8; 32], sealed: &SealedSeed) -> Result<MasterSeed, KeyError> {
    let mut plaintext = open(key, sealed.nonce(), sealed.ciphertext())?;
    let bytes: [u8; 64] = plaintext
        .as_slice()
        .try_into()
        .map_err(|_| KeyError::Unseal)?;
    plaintext.zeroize();
    Ok(MasterSeed::from_bytes(bytes))
}

/// Encrypts `plaintext` under `key` and `nonce`, returning `ciphertext ‖ tag`.
///
/// # Errors
/// [`KeyError::Seal`] if the key is rejected or the AEAD fails.
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
pub(crate) fn seal(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    seal_with_aad(key, nonce, &[], plaintext)
}

/// Decrypts `ciphertext` (`ciphertext ‖ tag`) under `key` and `nonce`.
///
/// # Errors
/// [`KeyError::Unseal`] on any failure, all alike.
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "os-keychain", feature = "keys-hd-password")
))]
pub(crate) fn open(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    open_with_aad(key, nonce, &[], ciphertext)
}

/// Encrypts `plaintext` under `key`, `nonce`, and additional authenticated data
/// `aad`, returning `ciphertext ‖ tag`; opening requires the identical `aad`.
///
/// # Errors
/// [`KeyError::Seal`] if the key is rejected or the AEAD fails.
pub(crate) fn seal_with_aad(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    // The nonce array converts by inference, so no deprecated `GenericArray` is named.
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| KeyError::Seal("invalid AEAD key".into()))?
        .encrypt(
            &(*nonce).into(),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| KeyError::Seal("AEAD seal failed".into()))
}

/// Decrypts `ciphertext` (`ciphertext ‖ tag`) under `key`, `nonce`, and `aad`.
///
/// # Errors
/// [`KeyError::Unseal`] on any failure, all alike.
pub(crate) fn open_with_aad(
    key: &[u8; 32],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, KeyError> {
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| KeyError::Unseal)?
        .decrypt(
            &(*nonce).into(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| KeyError::Unseal)
}

/// A fresh random AEAD nonce from the OS CSPRNG.
///
/// # Errors
/// [`KeyError::Seal`] if the system random source is unavailable.
pub(crate) fn random_nonce() -> Result<[u8; NONCE_LEN], KeyError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|_| KeyError::Seal("nonce entropy failed".into()))?;
    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_round_trips() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; NONCE_LEN];
        let ct = seal(&key, &nonce, b"the master seed bytes").expect("seal");
        assert_ne!(ct, b"the master seed bytes");
        let pt = open(&key, &nonce, &ct).expect("open");
        assert_eq!(pt, b"the master seed bytes");
    }

    #[test]
    fn open_with_a_wrong_key_fails_closed() {
        let nonce = [0x22u8; NONCE_LEN];
        let ct = seal(&[0x11u8; 32], &nonce, b"seed").expect("seal");
        assert!(matches!(
            open(&[0x33u8; 32], &nonce, &ct),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn open_a_tampered_ciphertext_fails_closed() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; NONCE_LEN];
        let mut ct = seal(&key, &nonce, b"seed").expect("seal");
        ct[0] ^= 0xFF;
        assert!(matches!(open(&key, &nonce, &ct), Err(KeyError::Unseal)));
    }

    #[test]
    fn random_nonces_differ() {
        assert_ne!(random_nonce().expect("a"), random_nonce().expect("b"));
    }

    #[test]
    fn aad_seal_open_round_trips() {
        let key = [0x44u8; 32];
        let nonce = [0x55u8; NONCE_LEN];
        let ct = seal_with_aad(&key, &nonce, b"owner|row|1", b"credential bytes").expect("seal");
        let pt = open_with_aad(&key, &nonce, b"owner|row|1", &ct).expect("open");
        assert_eq!(pt, b"credential bytes");
    }

    #[test]
    fn open_with_a_mismatched_aad_fails_closed() {
        let key = [0x44u8; 32];
        let nonce = [0x55u8; NONCE_LEN];
        let ct = seal_with_aad(&key, &nonce, b"owner|row|1", b"secret").expect("seal");
        assert!(matches!(
            open_with_aad(&key, &nonce, b"owner|row|2", &ct),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn aad_and_empty_aad_are_distinct() {
        let key = [0x44u8; 32];
        let nonce = [0x55u8; NONCE_LEN];
        let with = seal_with_aad(&key, &nonce, b"ctx", b"x").expect("seal aad");
        assert!(matches!(open(&key, &nonce, &with), Err(KeyError::Unseal)));
    }

    #[test]
    fn seed_seals_and_unseals_under_a_key() {
        let key = [0x55u8; 32];
        let seed = MasterSeed::from_bytes([0xEEu8; 64]);
        let sealed = seal_seed(&key, vec![1, 2, 3], &seed).expect("seal");
        assert_eq!(sealed.salt(), &[1, 2, 3]);
        let recovered = unseal_seed(&key, &sealed).expect("unseal");
        assert_eq!(recovered.expose(), seed.expose());
    }

    #[test]
    fn seed_unseal_with_a_wrong_key_fails_closed() {
        let seed = MasterSeed::from_bytes([0xEEu8; 64]);
        let sealed = seal_seed(&[0x55u8; 32], Vec::new(), &seed).expect("seal");
        assert!(matches!(
            unseal_seed(&[0x66u8; 32], &sealed),
            Err(KeyError::Unseal)
        ));
    }
}
