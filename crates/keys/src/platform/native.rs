use tracing::{info, instrument};

use crate::{DeviceId, DeviceSigner, KeyError, SeedStore, Signature, SigningKey, VerifyingKey};

/// A [`DeviceSigner`] whose seed is loaded from, or created in, a [`SeedStore`].
pub struct StoredSigner {
    device: DeviceId,
    key: SigningKey,
}

impl StoredSigner {
    /// Loads the device's keypair from `store`, creating and storing one the first time.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store fails, or [`KeyError::Generate`]
    /// if a new key cannot be created.
    #[instrument(skip(store), fields(device = device.as_str()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn load_or_create<S: SeedStore>(device: DeviceId, store: &S) -> Result<Self, KeyError> {
        let key = match store.load(&device)? {
            Some(seed) => SigningKey::from_seed(&seed),
            None => {
                let key = SigningKey::generate()?;
                store.store(&device, &key.to_seed())?;
                info!("generated new device keypair");
                key
            }
        };
        Ok(Self { device, key })
    }

    /// Returns the device this signer is for.
    pub fn device_id(&self) -> &DeviceId {
        &self.device
    }
}

impl DeviceSigner for StoredSigner {
    fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    async fn sign(&self, message: &[u8]) -> Result<Signature, KeyError> {
        Ok(self.key.sign(message))
    }
}

#[cfg(test)]
mod tests {
    use crate::MemorySeedStore;

    use super::*;

    #[tokio::test]
    async fn load_or_create_generates_then_reuses_the_same_key() {
        let store = MemorySeedStore::new();
        let device = DeviceId::new("device-1");

        let first = StoredSigner::load_or_create(device.clone(), &store).expect("create");
        let pubkey = first.verifying_key();

        let second = StoredSigner::load_or_create(device, &store).expect("reload");
        assert_eq!(second.verifying_key(), pubkey);
    }

    #[tokio::test]
    async fn signs_a_message_that_verifies_against_its_public_key() {
        let store = MemorySeedStore::new();
        let signer =
            StoredSigner::load_or_create(DeviceId::new("device-1"), &store).expect("create");
        let signature = signer.sign(b"push payload").await.expect("sign");
        signer
            .verifying_key()
            .verify(b"push payload", &signature)
            .expect("verifies");
    }

    #[tokio::test]
    async fn different_devices_get_different_keys() {
        let store = MemorySeedStore::new();
        let a = StoredSigner::load_or_create(DeviceId::new("a"), &store).expect("a");
        let b = StoredSigner::load_or_create(DeviceId::new("b"), &store).expect("b");
        assert_ne!(a.verifying_key(), b.verifying_key());
    }
}
