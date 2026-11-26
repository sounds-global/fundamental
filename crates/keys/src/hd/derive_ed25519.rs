use fundamental_core::secrets::Secret;
use zeroize::Zeroize;

use crate::hd::hmac_sha512::hmac_sha512;
use crate::hd::{DerivationPath, MasterSeed};

const ED25519_CURVE: &[u8] = b"ed25519 seed";

/// Derives the SLIP-0010 child seed for `path` from `seed` (Ed25519, hardened-only).
///
/// Total for every hardened path. Frozen: locked by the SLIP-0010 vectors, it is what makes a
/// mnemonic derive the same identity on every device.
#[must_use]
pub fn derive_ed25519(seed: &MasterSeed, path: &DerivationPath) -> Secret<[u8; 32]> {
    derive_from_seed_bytes(seed.expose(), path)
}

/// The SLIP-0010 core over seed bytes of any length, as the official vectors need.
pub(crate) fn derive_from_seed_bytes(seed: &[u8], path: &DerivationPath) -> Secret<[u8; 32]> {
    // Master node: I = HMAC-SHA512("ed25519 seed", seed); split into key ‖ chain.
    let mut i = hmac_sha512(ED25519_CURVE, &[seed]);
    let mut key = [0u8; 32];
    let mut chain = [0u8; 32];
    key.copy_from_slice(&i[0..32]);
    chain.copy_from_slice(&i[32..64]);

    // Each hardened child: I = HMAC-SHA512(chain, 0x00 ‖ key ‖ ser32(iₕ)).
    for index in path.hardened_indices() {
        i = hmac_sha512(&chain, &[&[0x00], &key, &index.to_be_bytes()]);
        key.copy_from_slice(&i[0..32]);
        chain.copy_from_slice(&i[32..64]);
    }

    i.zeroize();
    chain.zeroize();
    Secret::new(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigningKey;

    fn hex_lower(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn decode_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect()
    }

    fn derive_hex(seed_hex: &str, children: &[u32]) -> String {
        let seed = decode_hex(seed_hex);
        let path = DerivationPath::hardened(children).expect("hardened path");
        hex_lower(derive_from_seed_bytes(&seed, &path).expose())
    }

    /// SLIP-0010 Test Vector 1 for ed25519: the master key and `m/0H/1H`.
    #[test]
    fn matches_slip0010_ed25519_test_vector_1() {
        const SEED: &str = "000102030405060708090a0b0c0d0e0f";
        assert_eq!(
            derive_hex(SEED, &[]),
            "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7",
            "m master private key"
        );
        assert_eq!(
            derive_hex(SEED, &[0]),
            "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3",
            "m/0H private key"
        );
        assert_eq!(
            derive_hex(SEED, &[0, 1]),
            "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2",
            "m/0H/1H private key"
        );
    }

    #[test]
    fn derivation_is_deterministic_across_runs() {
        let seed = MasterSeed::from_bytes([0x11u8; 64]);
        let path = DerivationPath::hardened(&[44, 5_689_413, 0, 0, 0]).expect("path");
        let a = derive_ed25519(&seed, &path);
        let b = derive_ed25519(&seed, &path);
        assert_eq!(a.expose(), b.expose());
    }

    #[test]
    fn two_profiles_yield_distinct_one_way_keys() {
        let seed = MasterSeed::from_bytes([0x22u8; 64]);
        let k0 = derive_ed25519(
            &seed,
            &DerivationPath::hardened(&[44, 0, 0, 0, 0]).expect("p0"),
        );
        let k1 = derive_ed25519(
            &seed,
            &DerivationPath::hardened(&[44, 0, 1, 0, 0]).expect("p1"),
        );
        assert_ne!(k0.expose(), k1.expose());
        let pk0 = SigningKey::from_seed(&k0).verifying_key();
        let pk1 = SigningKey::from_seed(&k1).verifying_key();
        assert_ne!(pk0, pk1);
    }

    #[test]
    fn a_child_seed_drives_a_working_signer() {
        let seed = MasterSeed::from_bytes([0x33u8; 64]);
        let child = derive_ed25519(
            &seed,
            &DerivationPath::hardened(&[44, 1, 0, 0, 0]).expect("p"),
        );
        let key = SigningKey::from_seed(&child);
        let sig = key.sign(b"profile message");
        key.verifying_key()
            .verify(b"profile message", &sig)
            .expect("derived key signs and verifies");
    }
}
