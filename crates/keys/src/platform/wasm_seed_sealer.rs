use js_sys::Uint8Array;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CryptoKey, SubtleCrypto};
use zeroize::Zeroize;

use super::wasm::{random_bytes, subtle_crypto};
use super::wasm_aead::{gcm_params, load_key, load_or_create_key, seal_err};
use crate::KeyError;
use crate::hd::{MasterSeed, SealedSeed, SeedSealer};

const IV_LEN: usize = 12;

/// A [`SeedSealer`] that wraps the master seed under a non-extractable WebCrypto
/// AES-256-GCM key kept in IndexedDB.
pub struct WebCryptoSealer {
    key_name: String,
}

impl WebCryptoSealer {
    /// Creates a sealer whose wrapping key is stored under `key_name` in IndexedDB.
    #[must_use]
    pub fn new(key_name: impl Into<String>) -> Self {
        Self {
            key_name: key_name.into(),
        }
    }

    async fn load_wrapping_key(&self) -> Result<Option<CryptoKey>, KeyError> {
        load_key(&self.key_name).await
    }

    async fn load_or_create_wrapping_key(
        &self,
        subtle: &SubtleCrypto,
    ) -> Result<CryptoKey, KeyError> {
        load_or_create_key(subtle, &self.key_name).await
    }
}

impl SeedSealer for WebCryptoSealer {
    async fn seal(&self, seed: &MasterSeed) -> Result<SealedSeed, KeyError> {
        let subtle = subtle_crypto()?;
        let key = self.load_or_create_wrapping_key(&subtle).await?;

        let mut iv = [0u8; IV_LEN];
        random_bytes(&mut iv)?;
        // WebCrypto copies the input at the call, so the plaintext can be wiped once it returns.
        let mut plaintext = seed.expose().to_vec();
        let promise = subtle
            .encrypt_with_object_and_u8_array(&gcm_params(&iv, &[]), &key, &plaintext)
            .map_err(seal_err)?;
        plaintext.zeroize();
        let result = JsFuture::from(promise).await.map_err(seal_err)?;
        let ciphertext = Uint8Array::new(&result).to_vec();
        Ok(SealedSeed::new(Vec::new(), iv, ciphertext))
    }

    async fn unseal(&self, sealed: &SealedSeed) -> Result<MasterSeed, KeyError> {
        // A salt marks a password envelope; refused explicitly rather than by the AEAD tag.
        if !sealed.salt().is_empty() {
            return Err(KeyError::Unseal);
        }
        let subtle = subtle_crypto()?;
        let key = self.load_wrapping_key().await?.ok_or(KeyError::Unseal)?;

        let ciphertext = sealed.ciphertext().to_vec();
        let promise = subtle
            .decrypt_with_object_and_u8_array(&gcm_params(sealed.nonce(), &[]), &key, &ciphertext)
            .map_err(|_| KeyError::Unseal)?;
        let result = JsFuture::from(promise)
            .await
            .map_err(|_| KeyError::Unseal)?;
        let mut plaintext = Uint8Array::new(&result).to_vec();
        let bytes: [u8; 64] = plaintext
            .as_slice()
            .try_into()
            .map_err(|_| KeyError::Unseal)?;
        plaintext.zeroize();
        Ok(MasterSeed::from_bytes(bytes))
    }
}
