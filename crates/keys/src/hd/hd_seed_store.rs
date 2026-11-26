use fundamental_core::secrets::Secret;

use crate::hd::{DerivationPath, MasterSeed, derive_ed25519};
use crate::{DeviceId, KeyError, SeedStore};

/// A [`SeedStore`] that returns the SLIP-0010 child seed for a fixed profile path,
/// derived on demand from a [`MasterSeed`].
///
/// Handing it to [`StoredSigner::load_or_create`](crate::StoredSigner::load_or_create) gives a
/// signer over the derived key; [`store`](SeedStore::store) is a no-op.
pub struct HdSeedStore<'a> {
    seed: &'a MasterSeed,
    path: DerivationPath,
}

impl<'a> HdSeedStore<'a> {
    /// Creates a store that yields the child seed at `path` under `seed`.
    #[must_use]
    pub fn new(seed: &'a MasterSeed, path: DerivationPath) -> Self {
        Self { seed, path }
    }
}

impl SeedStore for HdSeedStore<'_> {
    fn load(&self, _device: &DeviceId) -> Result<Option<Secret<[u8; 32]>>, KeyError> {
        Ok(Some(derive_ed25519(self.seed, &self.path)))
    }

    fn store(&self, _device: &DeviceId, _seed: &Secret<[u8; 32]>) -> Result<(), KeyError> {
        // Derived, so there is nothing to persist.
        Ok(())
    }

    fn clear(&self, _device: &DeviceId) -> Result<(), KeyError> {
        // A derived key cannot be removed; `Ok(())` would falsely report it destroyed.
        Err(KeyError::Storage(
            "an HD child seed is derived, not stored: clear the master seed instead".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceSigner, StoredSigner};

    fn path(profile: u32) -> DerivationPath {
        DerivationPath::hardened(&[44, 5_689_413, profile, 0, 0]).expect("path")
    }

    #[test]
    fn clear_refuses_rather_than_reporting_a_key_it_cannot_destroy() {
        let seed = MasterSeed::from_bytes([0x42u8; 64]);
        let store = HdSeedStore::new(&seed, path(0));
        let error = store
            .clear(&DeviceId::new("p0"))
            .expect_err("a derived seed cannot be cleared");
        assert!(matches!(error, KeyError::Storage(_)));
        assert!(store.load(&DeviceId::new("p0")).expect("load").is_some());
    }

    #[test]
    fn stored_signer_over_hd_store_is_deterministic_across_stores() {
        let seed = MasterSeed::from_bytes([0x42u8; 64]);
        let a =
            StoredSigner::load_or_create(DeviceId::new("p0"), &HdSeedStore::new(&seed, path(0)))
                .expect("signer a");
        let b =
            StoredSigner::load_or_create(DeviceId::new("p0"), &HdSeedStore::new(&seed, path(0)))
                .expect("signer b");
        assert_eq!(a.verifying_key(), b.verifying_key());
    }

    #[test]
    fn distinct_profiles_get_distinct_keys() {
        let seed = MasterSeed::from_bytes([0x42u8; 64]);
        let p0 =
            StoredSigner::load_or_create(DeviceId::new("p0"), &HdSeedStore::new(&seed, path(0)))
                .expect("p0");
        let p1 =
            StoredSigner::load_or_create(DeviceId::new("p1"), &HdSeedStore::new(&seed, path(1)))
                .expect("p1");
        assert_ne!(p0.verifying_key(), p1.verifying_key());
    }

    #[test]
    fn load_always_yields_the_derived_seed() {
        let seed = MasterSeed::from_bytes([7u8; 64]);
        let store = HdSeedStore::new(&seed, path(3));
        let loaded = store
            .load(&DeviceId::new("ignored"))
            .expect("load")
            .expect("always present");
        assert_eq!(loaded.expose(), derive_ed25519(&seed, &path(3)).expose());
    }
}
