use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use fundamental_core::secrets::Secret;
use keyring::Entry;
use tracing::instrument;

use crate::{DeviceId, KeyError, SeedStore};

/// A [`SeedStore`] backed by the operating system keychain.
///
/// The seed is stored as unpadded base64 under the service name and the device id.
pub struct KeyringSeedStore {
    service: String,
}

impl KeyringSeedStore {
    /// Creates a store that keeps seeds under the keychain `service` name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, device: &DeviceId) -> Result<Entry, KeyError> {
        Entry::new(&self.service, device.as_str())
            .map_err(|e| KeyError::Storage(format!("keychain entry: {e}")))
    }
}

impl SeedStore for KeyringSeedStore {
    fn load(&self, device: &DeviceId) -> Result<Option<Secret<[u8; 32]>>, KeyError> {
        match self.entry(device)?.get_password() {
            Ok(encoded) => {
                let bytes = STANDARD_NO_PAD
                    .decode(encoded)
                    .map_err(|e| KeyError::Storage(format!("seed base64: {e}")))?;
                let seed: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| KeyError::Storage("stored seed is not 32 bytes".to_string()))?;
                Ok(Some(Secret::new(seed)))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyError::Storage(format!("keychain read: {e}"))),
        }
    }

    fn store(&self, device: &DeviceId, seed: &Secret<[u8; 32]>) -> Result<(), KeyError> {
        let encoded = STANDARD_NO_PAD.encode(seed.expose());
        self.entry(device)?
            .set_password(&encoded)
            .map_err(|e| KeyError::Storage(format!("keychain write: {e}")))
    }

    #[instrument(skip_all, fields(service = %self.service, device = device.as_str()), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn clear(&self, device: &DeviceId) -> Result<(), KeyError> {
        match self.entry(device)?.delete_credential() {
            // Nothing stored is already cleared.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyError::Storage(format!("keychain delete: {e}"))),
        }
    }
}
