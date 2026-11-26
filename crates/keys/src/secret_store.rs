use std::sync::Mutex;

use fundamental_core::secrets::SecretString;

use crate::KeyError;

/// Persists a caller-named secret under a service and an account.
///
/// Not a [`SeedStore`](crate::SeedStore): seeds may only come from a mnemonic or a sealed blob.
/// The service is fixed per store; the account is per call.
pub trait SecretStore {
    /// Loads the secret stored for `account`, or `None` if there is none.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be read.
    fn load(&self, account: &str) -> Result<Option<SecretString>, KeyError>;

    /// Stores `secret` for `account`, replacing any secret already there.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be written.
    fn store(&self, account: &str, secret: &SecretString) -> Result<(), KeyError>;

    /// Removes the secret stored for `account`.
    ///
    /// Clearing an empty account is not an error, so a repeated sign-out converges.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the store cannot be written.
    fn clear(&self, account: &str) -> Result<(), KeyError>;
}

/// An in-memory [`SecretStore`] for tests and ephemeral use.
#[derive(Default)]
pub struct MemorySecretStore {
    secrets: Mutex<Vec<(String, String)>>,
}

impl MemorySecretStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The entries, or [`KeyError::Storage`] if another thread poisoned the lock.
    ///
    /// A poisoned lock is reported, not recovered: a panic mid-write leaves an unknown entry.
    fn entries(&self) -> Result<std::sync::MutexGuard<'_, Vec<(String, String)>>, KeyError> {
        self.secrets
            .lock()
            .map_err(|_| KeyError::Storage("secret store mutex poisoned".to_string()))
    }
}

impl SecretStore for MemorySecretStore {
    fn load(&self, account: &str) -> Result<Option<SecretString>, KeyError> {
        Ok(self
            .entries()?
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, secret)| SecretString::new(secret.clone())))
    }

    fn store(&self, account: &str, secret: &SecretString) -> Result<(), KeyError> {
        let mut entries = self.entries()?;
        entries.retain(|(name, _)| name != account);
        entries.push((account.to_string(), secret.expose().clone()));
        Ok(())
    }

    fn clear(&self, account: &str) -> Result<(), KeyError> {
        self.entries()?.retain(|(name, _)| name != account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_loads_a_secret() {
        let store = MemorySecretStore::new();
        assert!(store.load("session").expect("load").is_none());

        store
            .store("session", &SecretString::new("a-token".to_string()))
            .expect("store");
        let loaded = store.load("session").expect("load").expect("present");
        assert_eq!(loaded.expose(), "a-token");
    }

    #[test]
    fn store_replaces_an_existing_secret() {
        let store = MemorySecretStore::new();
        store
            .store("session", &SecretString::new("first".to_string()))
            .expect("a");
        store
            .store("session", &SecretString::new("second".to_string()))
            .expect("b");
        let loaded = store.load("session").expect("load").expect("present");
        assert_eq!(loaded.expose(), "second");
    }

    #[test]
    fn secrets_are_isolated_by_account() {
        let store = MemorySecretStore::new();
        store
            .store("one", &SecretString::new("a".to_string()))
            .expect("store");
        assert!(store.load("two").expect("load").is_none());
    }

    #[test]
    fn clear_removes_the_secret() {
        let store = MemorySecretStore::new();
        store
            .store("session", &SecretString::new("a-token".to_string()))
            .expect("store");
        store.clear("session").expect("clear");
        assert!(store.load("session").expect("load").is_none());
    }

    /// A repeated sign-out does not fail.
    #[test]
    fn clearing_an_absent_account_converges_rather_than_erring() {
        let store = MemorySecretStore::new();
        store.clear("never-stored").expect("first clear");
        store.clear("never-stored").expect("second clear");
        assert!(store.load("never-stored").expect("load").is_none());
    }

    #[test]
    fn clearing_one_account_leaves_another_alone() {
        let store = MemorySecretStore::new();
        store
            .store("one", &SecretString::new("a".to_string()))
            .expect("store one");
        store
            .store("two", &SecretString::new("b".to_string()))
            .expect("store two");
        store.clear("one").expect("clear one");
        assert!(store.load("one").expect("load").is_none());
        assert_eq!(
            store.load("two").expect("load").expect("present").expose(),
            "b"
        );
    }
}
