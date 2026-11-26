use keyring::Entry;
use tracing::instrument;

use crate::{KeyError, SecretStore};
use fundamental_core::secrets::SecretString;

/// A [`SecretStore`] backed by the operating system keychain.
pub struct KeyringSecretStore {
    service: String,
}

impl KeyringSecretStore {
    /// Creates a store that keeps secrets under the keychain `service` name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<Entry, KeyError> {
        Entry::new(&self.service, account)
            .map_err(|e| KeyError::Storage(format!("keychain entry: {e}")))
    }
}

impl SecretStore for KeyringSecretStore {
    // `account` is a name, never the secret, so it is safe in a span.
    #[instrument(skip_all, fields(service = %self.service, account = %account), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn load(&self, account: &str) -> Result<Option<SecretString>, KeyError> {
        match self.entry(account)?.get_password() {
            Ok(secret) => Ok(Some(SecretString::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyError::Storage(format!("keychain read: {e}"))),
        }
    }

    #[instrument(skip_all, fields(service = %self.service, account = %account), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn store(&self, account: &str, secret: &SecretString) -> Result<(), KeyError> {
        self.entry(account)?
            .set_password(secret.expose())
            .map_err(|e| KeyError::Storage(format!("keychain write: {e}")))
    }

    #[instrument(skip_all, fields(service = %self.service, account = %account), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn clear(&self, account: &str) -> Result<(), KeyError> {
        match self.entry(account)?.delete_credential() {
            // Nothing stored is already cleared.
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyError::Storage(format!("keychain delete: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs no platform store; the round trip is proved by `MemorySecretStore`.
    #[test]
    fn a_store_carries_the_service_it_was_built_for() {
        let store = KeyringSecretStore::new("acme-person");
        assert_eq!(store.service, "acme-person");
    }

    #[test]
    fn an_entry_is_addressed_by_service_and_account() {
        let store = KeyringSecretStore::new("acme-person");
        assert!(store.entry("session").is_ok());
        assert!(store.entry("refresh").is_ok());
    }
}
