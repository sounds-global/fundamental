use std::sync::Mutex;

use fundamental_core::secrets::Secret;

use crate::{DeviceId, KeyError};

/// Persists a device's 32-byte ed25519 seed under its [`DeviceId`].
pub trait SeedStore {
    /// Loads the seed for `device`, or `None` if no seed is stored yet.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be read.
    fn load(&self, device: &DeviceId) -> Result<Option<Secret<[u8; 32]>>, KeyError>;

    /// Stores `seed` for `device`, replacing any existing seed.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be written.
    fn store(&self, device: &DeviceId, seed: &Secret<[u8; 32]>) -> Result<(), KeyError>;

    /// Removes the seed for `device`, so the key it reconstructs cannot be used again.
    ///
    /// Clearing a device that holds no seed converges rather than failing.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be written, or if the store
    /// derives its seeds rather than holding them and so has nothing it can remove.
    fn clear(&self, device: &DeviceId) -> Result<(), KeyError>;
}

/// An in-memory [`SeedStore`] for tests and ephemeral use.
#[derive(Default)]
pub struct MemorySeedStore {
    seeds: Mutex<Vec<(String, [u8; 32])>>,
}

impl MemorySeedStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SeedStore for MemorySeedStore {
    fn load(&self, device: &DeviceId) -> Result<Option<Secret<[u8; 32]>>, KeyError> {
        let seeds = self
            .seeds
            .lock()
            .map_err(|_| KeyError::Storage("seed store mutex poisoned".to_string()))?;
        Ok(seeds
            .iter()
            .find(|(id, _)| id == device.as_str())
            .map(|(_, seed)| Secret::new(*seed)))
    }

    fn store(&self, device: &DeviceId, seed: &Secret<[u8; 32]>) -> Result<(), KeyError> {
        let mut seeds = self
            .seeds
            .lock()
            .map_err(|_| KeyError::Storage("seed store mutex poisoned".to_string()))?;
        seeds.retain(|(id, _)| id != device.as_str());
        seeds.push((device.as_str().to_string(), *seed.expose()));
        Ok(())
    }

    fn clear(&self, device: &DeviceId) -> Result<(), KeyError> {
        let mut seeds = self
            .seeds
            .lock()
            .map_err(|_| KeyError::Storage("seed store mutex poisoned".to_string()))?;
        seeds.retain(|(id, _)| id != device.as_str());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_loads_a_seed() {
        let store = MemorySeedStore::new();
        let device = DeviceId::new("d1");
        assert!(store.load(&device).expect("load").is_none());

        store
            .store(&device, &Secret::new([9u8; 32]))
            .expect("store");
        let loaded = store.load(&device).expect("load").expect("present");
        assert_eq!(*loaded.expose(), [9u8; 32]);
    }

    #[test]
    fn store_replaces_an_existing_seed() {
        let store = MemorySeedStore::new();
        let device = DeviceId::new("d1");
        store.store(&device, &Secret::new([1u8; 32])).expect("a");
        store.store(&device, &Secret::new([2u8; 32])).expect("b");
        let loaded = store.load(&device).expect("load").expect("present");
        assert_eq!(*loaded.expose(), [2u8; 32]);
    }

    #[test]
    fn clear_removes_the_seed() {
        let store = MemorySeedStore::new();
        let device = DeviceId::new("d1");
        store
            .store(&device, &Secret::new([7u8; 32]))
            .expect("store");
        store.clear(&device).expect("clear");
        assert!(store.load(&device).expect("load").is_none());
    }

    #[test]
    fn clearing_an_absent_device_converges_rather_than_erring() {
        let store = MemorySeedStore::new();
        store.clear(&DeviceId::new("never-seen")).expect("clear");
    }

    #[test]
    fn clearing_one_device_leaves_another_alone() {
        let store = MemorySeedStore::new();
        let kept = DeviceId::new("kept");
        store.store(&kept, &Secret::new([1u8; 32])).expect("store");
        store
            .store(&DeviceId::new("gone"), &Secret::new([2u8; 32]))
            .expect("store");
        store.clear(&DeviceId::new("gone")).expect("clear");
        assert!(store.load(&kept).expect("load").is_some());
    }

    #[test]
    fn seeds_are_isolated_by_device() {
        let store = MemorySeedStore::new();
        store
            .store(&DeviceId::new("a"), &Secret::new([1u8; 32]))
            .expect("a");
        assert!(store.load(&DeviceId::new("b")).expect("load").is_none());
    }
}
