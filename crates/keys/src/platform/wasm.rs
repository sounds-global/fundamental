use js_sys::{Array, Uint8Array};
use tracing::info;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Crypto, CryptoKey, CryptoKeyPair, IdbDatabase, SubtleCrypto};

use super::idb_key_store;
use crate::{DeviceId, DeviceSigner, KeyError, Signature, VerifyingKey};

/// A [`DeviceSigner`] backed by a non-extractable WebCrypto Ed25519 key.
pub struct WebCryptoSigner {
    subtle: SubtleCrypto,
    private_key: CryptoKey,
    verifying_key: VerifyingKey,
}

impl WebCryptoSigner {
    /// Generates a fresh non-extractable keypair in the browser, not persisted.
    ///
    /// # Errors
    /// Returns [`KeyError::Generate`] if WebCrypto is unavailable or key generation
    /// fails, or [`KeyError::Malformed`] if the exported public key is not 32 bytes.
    pub async fn generate() -> Result<Self, KeyError> {
        let subtle = subtle_crypto()?;
        let pair = generate_pair(&subtle).await?;
        Self::from_pair(subtle, pair).await
    }

    /// Loads the device's persisted key pair, and mints nothing when there is none.
    ///
    /// A sign-in must be able to ask whether a key exists without creating one.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if IndexedDB cannot be read, or
    /// [`KeyError::Malformed`] if the stored public key is not 32 bytes.
    pub async fn load(device_id: DeviceId) -> Result<Option<Self>, KeyError> {
        let subtle = subtle_crypto()?;
        let db = idb_key_store::open().await?;
        Self::stored(subtle, &db, &device_id).await
    }

    /// Loads the device's persisted key pair from IndexedDB, creating one the first time.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if IndexedDB cannot be read or written,
    /// [`KeyError::Generate`] if a new key cannot be created, or
    /// [`KeyError::Malformed`] if the exported public key is not 32 bytes.
    pub async fn load_or_create(device_id: DeviceId) -> Result<Self, KeyError> {
        let subtle = subtle_crypto()?;
        let db = idb_key_store::open().await?;
        if let Some(signer) = Self::stored(subtle.clone(), &db, &device_id).await? {
            return Ok(signer);
        }
        let pair = generate_pair(&subtle).await?;
        idb_key_store::put(&db, device_id.as_str(), pair.as_ref()).await?;
        info!(
            device = device_id.as_str(),
            "generated and stored new device key"
        );
        Self::from_pair(subtle, pair).await
    }

    /// The pair `device_id` already has, or `None` — the half both constructors share.
    async fn stored(
        subtle: SubtleCrypto,
        db: &IdbDatabase,
        device_id: &DeviceId,
    ) -> Result<Option<Self>, KeyError> {
        let Some(value) = idb_key_store::get(db, device_id.as_str()).await? else {
            return Ok(None);
        };
        info!(
            device = device_id.as_str(),
            "loaded device key from IndexedDB"
        );
        Self::from_pair(subtle, value.unchecked_into())
            .await
            .map(Some)
    }

    async fn from_pair(subtle: SubtleCrypto, pair: CryptoKeyPair) -> Result<Self, KeyError> {
        let verifying_key = export_verifying_key(&subtle, &pair.get_public_key()).await?;
        Ok(Self {
            subtle,
            private_key: pair.get_private_key(),
            verifying_key,
        })
    }

    /// Imports an HD-derived Ed25519 child seed as a non-extractable WebCrypto signing key.
    ///
    /// The transient PKCS#8 buffer is zeroized; drop `child_seed` right after.
    ///
    /// # Errors
    /// [`KeyError::Generate`] if WebCrypto is unavailable or the import fails.
    #[cfg(feature = "keys-hd")]
    pub async fn import_nonextractable(
        child_seed: &fundamental_core::secrets::Secret<[u8; 32]>,
    ) -> Result<Self, KeyError> {
        use zeroize::Zeroize;

        let subtle = subtle_crypto()?;

        // Ed25519 PKCS#8 v1: a fixed 16-byte prefix followed by the 32-byte seed.
        const PKCS8_PREFIX: [u8; 16] = [
            0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22,
            0x04, 0x20,
        ];
        let mut pkcs8 = [0u8; 48];
        pkcs8[..16].copy_from_slice(&PKCS8_PREFIX);
        pkcs8[16..].copy_from_slice(child_seed.expose());
        let key_data = Uint8Array::from(&pkcs8[..]);

        let usages = Array::of1(&JsValue::from_str("sign"));
        let imported = JsFuture::from(
            subtle
                .import_key_with_str(
                    "pkcs8",
                    key_data.as_ref(),
                    "Ed25519",
                    false,
                    usages.as_ref(),
                )
                .map_err(generate_error)?,
        )
        .await
        .map_err(generate_error)?;
        pkcs8.zeroize();

        let verifying = ed25519_dalek::SigningKey::from_bytes(child_seed.expose()).verifying_key();
        Ok(Self {
            subtle,
            private_key: imported.unchecked_into(),
            verifying_key: VerifyingKey::from_bytes(verifying.to_bytes()),
        })
    }
}

async fn generate_pair(subtle: &SubtleCrypto) -> Result<CryptoKeyPair, KeyError> {
    let usages = Array::of2(&JsValue::from_str("sign"), &JsValue::from_str("verify"));
    let pair = JsFuture::from(
        subtle
            .generate_key_with_str("Ed25519", false, usages.as_ref())
            .map_err(generate_error)?,
    )
    .await
    .map_err(generate_error)?;
    Ok(pair.unchecked_into())
}

async fn export_verifying_key(
    subtle: &SubtleCrypto,
    public_key: &CryptoKey,
) -> Result<VerifyingKey, KeyError> {
    let raw = JsFuture::from(
        subtle
            .export_key("raw", public_key)
            .map_err(generate_error)?,
    )
    .await
    .map_err(generate_error)?;
    let bytes = Uint8Array::new(&raw).to_vec();
    let public: [u8; 32] = bytes
        .try_into()
        .map_err(|_| KeyError::Malformed("exported public key is not 32 bytes".to_string()))?;
    Ok(VerifyingKey::from_bytes(public))
}

impl DeviceSigner for WebCryptoSigner {
    fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    async fn sign(&self, message: &[u8]) -> Result<Signature, KeyError> {
        let signed = JsFuture::from(
            self.subtle
                .sign_with_str_and_u8_array("Ed25519", &self.private_key, message)
                .map_err(sign_error)?,
        )
        .await
        .map_err(sign_error)?;
        let bytes = Uint8Array::new(&signed).to_vec();
        let signature: [u8; 64] = bytes
            .try_into()
            .map_err(|_| KeyError::Sign("signature is not 64 bytes".to_string()))?;
        Ok(Signature::from_bytes(signature))
    }
}

pub(super) fn crypto() -> Result<Crypto, KeyError> {
    let global = js_sys::global();
    let crypto = js_sys::Reflect::get(&global, &JsValue::from_str("crypto"))
        .map_err(|_| KeyError::Generate("no global `crypto` object".to_string()))?;
    crypto
        .dyn_into()
        .map_err(|_| KeyError::Generate("global `crypto` is not a Crypto".to_string()))
}

pub(super) fn subtle_crypto() -> Result<SubtleCrypto, KeyError> {
    Ok(crypto()?.subtle())
}

/// Fills `buf` from the platform CSPRNG.
///
/// # Errors
/// [`KeyError::Seal`] if `crypto.getRandomValues` is unavailable or fails.
#[cfg(feature = "keys-hd")]
pub(super) fn random_bytes(buf: &mut [u8]) -> Result<(), KeyError> {
    crypto()?
        .get_random_values_with_u8_array(buf)
        .map_err(|e| KeyError::Seal(format!("getRandomValues: {}", js_message(&e))))?;
    Ok(())
}

fn generate_error(value: JsValue) -> KeyError {
    KeyError::Generate(js_message(&value))
}

fn sign_error(value: JsValue) -> KeyError {
    KeyError::Sign(js_message(&value))
}

fn js_message(value: &JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
