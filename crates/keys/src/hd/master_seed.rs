use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::hd::Mnemonic;

/// The 512-bit master seed from which every per-profile signing key derives.
///
/// Made from a [`Mnemonic`] by BIP-39's PBKDF2; zeroized on drop, `Debug` redacted, and its
/// bytes reachable only inside this crate.
#[derive(Zeroize, ZeroizeOnDrop, Clone, PartialEq, Eq)]
pub struct MasterSeed {
    bytes: [u8; 64],
}

impl MasterSeed {
    /// Derives the master seed from a [`Mnemonic`] and an optional passphrase
    /// (BIP-39's "25th word"), via PBKDF2-HMAC-SHA512.
    #[must_use]
    pub fn from_mnemonic(mnemonic: &Mnemonic, passphrase: Option<&str>) -> Self {
        mnemonic.to_seed(passphrase)
    }

    /// Wraps raw seed material; in-crate only, so a seed comes only from a mnemonic or sealed blob.
    pub(crate) fn from_bytes(bytes: [u8; 64]) -> Self {
        Self { bytes }
    }

    pub(crate) fn expose(&self) -> &[u8; 64] {
        &self.bytes
    }
}

impl core::fmt::Debug for MasterSeed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_seed() {
        let seed = MasterSeed::from_bytes([7u8; 64]);
        assert_eq!(format!("{seed:?}"), "[REDACTED]");
    }

    #[test]
    fn zeroize_wipes_the_seed() {
        let mut seed = MasterSeed::from_bytes([0xABu8; 64]);
        seed.zeroize();
        assert_eq!(seed.expose(), &[0u8; 64]);
    }

    #[test]
    fn clone_is_equal_and_independent() {
        let a = MasterSeed::from_bytes([3u8; 64]);
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.expose(), b.expose());
    }
}
