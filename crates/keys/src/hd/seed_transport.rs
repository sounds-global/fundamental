use fundamental_core::secrets::Secret;
use zeroize::Zeroize;

use crate::KeyError;
use crate::hd::{MasterSeed, VaultCipher};

/// Seals a [`MasterSeed`] under a caller-supplied 32-byte `subkey`, binding `aad` into
/// the AEAD tag, as a [`VaultCipher`] envelope.
///
/// The only way a seed leaves the crate: [`MasterSeed`] is not `Serialize`.
///
/// # Errors
/// [`KeyError::Seal`] if entropy or the AEAD fails.
pub fn seal_master_seed(
    subkey: &Secret<[u8; 32]>,
    aad: &[u8],
    seed: &MasterSeed,
) -> Result<Vec<u8>, KeyError> {
    VaultCipher::new(subkey.clone()).seal(aad, seed.expose())
}

/// Recovers a [`MasterSeed`] sealed by [`seal_master_seed`] under the same `subkey` and
/// `aad`.
///
///
/// # Errors
/// [`KeyError::Malformed`] if the envelope is structurally invalid; [`KeyError::Unseal`] if
/// the AEAD open fails or the plaintext is not 64 bytes.
pub fn open_master_seed(
    subkey: &Secret<[u8; 32]>,
    aad: &[u8],
    envelope: &[u8],
) -> Result<MasterSeed, KeyError> {
    let mut plaintext = VaultCipher::new(subkey.clone()).open(aad, envelope)?;
    let bytes: [u8; 64] = plaintext
        .as_slice()
        .try_into()
        .map_err(|_| KeyError::Unseal)?;
    plaintext.zeroize();
    Ok(MasterSeed::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subkey(byte: u8) -> Secret<[u8; 32]> {
        Secret::new([byte; 32])
    }

    #[test]
    fn seal_then_open_round_trips_the_seed() {
        let seed = MasterSeed::from_bytes([0x5Au8; 64]);
        let env = seal_master_seed(&subkey(0x10), b"owner|row", &seed).expect("seal");
        assert_ne!(&env[..], seed.expose().as_slice());
        let back = open_master_seed(&subkey(0x10), b"owner|row", &env).expect("open");
        assert_eq!(back.expose(), seed.expose());
    }

    #[test]
    fn open_under_a_wrong_subkey_fails_closed() {
        let seed = MasterSeed::from_bytes([0x11u8; 64]);
        let env = seal_master_seed(&subkey(0x20), b"aad", &seed).expect("seal");
        assert!(matches!(
            open_master_seed(&subkey(0x21), b"aad", &env),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn open_under_a_mismatched_aad_fails_closed() {
        let seed = MasterSeed::from_bytes([0x22u8; 64]);
        let env = seal_master_seed(&subkey(0x30), b"aad-a", &seed).expect("seal");
        assert!(matches!(
            open_master_seed(&subkey(0x30), b"aad-b", &env),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn a_tampered_envelope_fails_closed() {
        let seed = MasterSeed::from_bytes([0x33u8; 64]);
        let mut env = seal_master_seed(&subkey(0x40), b"aad", &seed).expect("seal");
        let last = env.len() - 1;
        env[last] ^= 0xFF;
        assert!(matches!(
            open_master_seed(&subkey(0x40), b"aad", &env),
            Err(KeyError::Unseal)
        ));
    }

    #[test]
    fn a_short_envelope_is_malformed() {
        assert!(matches!(
            open_master_seed(&subkey(0x50), b"aad", &[0x01, 0x02]),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn a_recovered_seed_still_redacts_in_debug() {
        let seed = MasterSeed::from_bytes([0x66u8; 64]);
        let env = seal_master_seed(&subkey(0x60), b"aad", &seed).expect("seal");
        let back = open_master_seed(&subkey(0x60), b"aad", &env).expect("open");
        assert_eq!(format!("{back:?}"), "[REDACTED]");
    }
}
