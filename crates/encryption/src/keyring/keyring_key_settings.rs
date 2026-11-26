use fundamental_core::secrets::InMemorySecretString;
use serde::Deserialize;

/// One version of a [`Keyring`](crate::Keyring)'s key, as configuration declares it.
///
/// The material reads `<purpose>:<version>:<64 hex>`, so a key file in the wrong slot refuses.
#[derive(Debug, Deserialize)]
pub struct KeyringKeySettings {
    /// The version number, non-zero and unique within the keyring.
    pub version: u16,
    /// The 32-byte key, as 64 hex characters.
    pub material: InMemorySecretString,
}
