use async_trait::async_trait;
use fundamental_secrets::VaultClient;
use tracing::instrument;

use crate::{KeyError, KeyResult, KmsProvider};

/// Default Vault Transit secrets-engine mount.
pub const DEFAULT_TRANSIT_MOUNT: &str = "transit";

/// A [`KmsProvider`] backed by HashiCorp Vault's Transit secrets engine.
///
/// It unwraps the raw Transit ciphertext (`vault:v1:…`) into the 32-byte data encryption key.
pub struct VaultKmsProvider {
    vault: VaultClient,
    transit_mount: String,
}

impl VaultKmsProvider {
    /// Creates a provider against the given Transit mount (e.g. `"transit"`).
    pub fn new(vault: VaultClient, transit_mount: impl Into<String>) -> Self {
        Self {
            vault,
            transit_mount: transit_mount.into(),
        }
    }
}

#[async_trait]
impl KmsProvider for VaultKmsProvider {
    #[instrument(
        skip(self, ciphertext),
        fields(mount = %self.transit_mount, key_id, ciphertext_len = ciphertext.len()),
        err(Debug)
    )]
    async fn decrypt(&self, ciphertext: &[u8], key_id: &str) -> KeyResult<Vec<u8>> {
        let token = std::str::from_utf8(ciphertext)
            .map_err(|e| KeyError::Kms(format!("transit ciphertext is not valid UTF-8: {e}")))?;
        let plaintext = self
            .vault
            .transit_decrypt(&self.transit_mount, key_id, token)
            .await?;
        Ok(plaintext)
    }

    #[instrument(
        skip(self, plaintext),
        fields(mount = %self.transit_mount, key_id, plaintext_len = plaintext.len()),
        err(Debug)
    )]
    async fn encrypt(&self, plaintext: &[u8], key_id: &str) -> KeyResult<Vec<u8>> {
        let ciphertext = self
            .vault
            .transit_encrypt(&self.transit_mount, key_id, plaintext)
            .await?;
        Ok(ciphertext.into_bytes())
    }
}
