use fundamental_core::secrets::Secret;

use crate::hd::VaultOwnerId;
use crate::hd::hkdf_sha512::hkdf_expand;

const CONTENT_MASTER_INFO: &[u8] = b"fundamental/vault/content-master-v1";

const ROW_SUBKEY_INFO: &[u8] = b"fundamental/vault/row-v1";

/// The vault's content-encryption master key, derived from the seed and never seen by a server.
///
/// `HKDF-Expand(SHA-512)` of its own hardened child seed; [`row_subkey`](Self::row_subkey) binds
/// a per-row key to `(owner_id, row_id)`. Frozen: the `info` strings decrypt every sealed row.
pub struct VaultContentKey {
    master: Secret<[u8; 32]>,
}

impl VaultContentKey {
    /// Derives the content master from the vault content child seed.
    #[must_use]
    pub fn from_child_seed(child_seed: &Secret<[u8; 32]>) -> Self {
        let mut master = [0u8; 32];
        hkdf_expand(child_seed.expose(), CONTENT_MASTER_INFO, &mut master);
        Self {
            master: Secret::new(master),
        }
    }

    /// Derives the AEAD subkey for a single vault row, bound to its owner and row id.
    #[must_use]
    pub fn row_subkey(&self, owner_id: &VaultOwnerId, row_id: &str) -> Secret<[u8; 32]> {
        let mut info = Vec::with_capacity(ROW_SUBKEY_INFO.len() + 32 + row_id.len());
        info.extend_from_slice(ROW_SUBKEY_INFO);
        info.extend_from_slice(owner_id.as_bytes());
        info.extend_from_slice(row_id.as_bytes());
        let mut subkey = [0u8; 32];
        hkdf_expand(self.master.expose(), &info, &mut subkey);
        Secret::new(subkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerifyingKey;

    fn owner() -> VaultOwnerId {
        VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([4u8; 32]))
    }

    #[test]
    fn master_is_deterministic_for_a_seed() {
        let seed = Secret::new([0x31u8; 32]);
        let a = VaultContentKey::from_child_seed(&seed);
        let b = VaultContentKey::from_child_seed(&seed);
        assert_eq!(
            a.row_subkey(&owner(), "row-1").expose(),
            b.row_subkey(&owner(), "row-1").expose()
        );
    }

    #[test]
    fn distinct_rows_get_distinct_subkeys() {
        let key = VaultContentKey::from_child_seed(&Secret::new([0x32u8; 32]));
        let o = owner();
        assert_ne!(
            key.row_subkey(&o, "row-1").expose(),
            key.row_subkey(&o, "row-2").expose()
        );
    }

    #[test]
    fn distinct_owners_get_distinct_subkeys_for_the_same_row() {
        let key = VaultContentKey::from_child_seed(&Secret::new([0x33u8; 32]));
        let a = VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([1u8; 32]));
        let b = VaultOwnerId::from_public_key(&VerifyingKey::from_bytes([2u8; 32]));
        assert_ne!(
            key.row_subkey(&a, "row").expose(),
            key.row_subkey(&b, "row").expose()
        );
    }

    #[test]
    fn distinct_content_seeds_get_distinct_masters() {
        let a = VaultContentKey::from_child_seed(&Secret::new([0x01u8; 32]));
        let b = VaultContentKey::from_child_seed(&Secret::new([0x02u8; 32]));
        let o = owner();
        assert_ne!(
            a.row_subkey(&o, "row").expose(),
            b.row_subkey(&o, "row").expose()
        );
    }
}
