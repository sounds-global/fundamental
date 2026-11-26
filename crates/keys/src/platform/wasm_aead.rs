use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CryptoKey, SubtleCrypto};

use super::idb_key_store;
use super::wasm::{random_bytes, subtle_crypto};
use crate::KeyError;

/// AES-GCM nonce width in bytes.
pub const AEAD_NONCE_LEN: usize = 12;

/// Seals and opens bytes under a non-extractable WebCrypto AES-256-GCM key kept in IndexedDB
/// under its name.
#[derive(Clone, Debug)]
pub struct WebCryptoAead {
    key_name: String,
}

impl WebCryptoAead {
    /// A sealer whose key is stored under `key_name`, made on the first seal.
    #[must_use]
    pub fn new(key_name: impl Into<String>) -> Self {
        Self {
            key_name: key_name.into(),
        }
    }

    /// `plaintext` sealed under the key, bound to `aad`: the nonce and `ciphertext ‖ tag`.
    ///
    /// # Errors
    /// [`KeyError::Seal`] when WebCrypto, IndexedDB or the random source fails.
    pub async fn seal(
        &self,
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<([u8; AEAD_NONCE_LEN], Vec<u8>), KeyError> {
        let subtle = subtle_crypto()?;
        let key = load_or_create_key(&subtle, &self.key_name).await?;
        let mut nonce = [0u8; AEAD_NONCE_LEN];
        random_bytes(&mut nonce)?;
        let promise = subtle
            .encrypt_with_object_and_u8_array(&gcm_params(&nonce, aad), &key, plaintext)
            .map_err(seal_err)?;
        let result = JsFuture::from(promise).await.map_err(seal_err)?;
        Ok((nonce, Uint8Array::new(&result).to_vec()))
    }

    /// The plaintext of `ciphertext`, sealed under the key with `nonce` and bound to `aad`.
    ///
    /// # Errors
    /// [`KeyError::Unseal`] when the key is gone or the value was sealed elsewhere, for another
    /// `aad`, or altered, all alike.
    pub async fn open(
        &self,
        aad: &[u8],
        nonce: &[u8; AEAD_NONCE_LEN],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, KeyError> {
        let subtle = subtle_crypto()?;
        let key = load_key(&self.key_name).await?.ok_or(KeyError::Unseal)?;
        let promise = subtle
            .decrypt_with_object_and_u8_array(&gcm_params(nonce, aad), &key, ciphertext)
            .map_err(|_| KeyError::Unseal)?;
        let result = JsFuture::from(promise)
            .await
            .map_err(|_| KeyError::Unseal)?;
        Ok(Uint8Array::new(&result).to_vec())
    }
}

pub(super) async fn load_key(name: &str) -> Result<Option<CryptoKey>, KeyError> {
    let db = idb_key_store::open().await?;
    Ok(idb_key_store::get(&db, name)
        .await?
        .map(JsCast::unchecked_into))
}

pub(super) async fn load_or_create_key(
    subtle: &SubtleCrypto,
    name: &str,
) -> Result<CryptoKey, KeyError> {
    if let Some(key) = load_key(name).await? {
        return Ok(key);
    }
    let key = generate_aes_key(subtle).await?;
    let db = idb_key_store::open().await?;
    idb_key_store::put(&db, name, key.as_ref()).await?;
    Ok(key)
}

async fn generate_aes_key(subtle: &SubtleCrypto) -> Result<CryptoKey, KeyError> {
    let params = Object::new();
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("name"),
        &JsValue::from_str("AES-GCM"),
    );
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("length"),
        &JsValue::from_f64(256.0),
    );
    let usages = Array::of2(&JsValue::from_str("encrypt"), &JsValue::from_str("decrypt"));
    let promise = subtle
        .generate_key_with_object(&params, false, usages.as_ref())
        .map_err(seal_err)?;
    let key = JsFuture::from(promise).await.map_err(seal_err)?;
    Ok(key.unchecked_into())
}

/// Builds `{ name: "AES-GCM", iv, additionalData }`; no additional data when `aad` is empty.
pub(super) fn gcm_params(iv: &[u8], aad: &[u8]) -> Object {
    let params = Object::new();
    // `Reflect::set` on a fresh, non-frozen object cannot fail.
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("name"),
        &JsValue::from_str("AES-GCM"),
    );
    let _ = Reflect::set(
        &params,
        &JsValue::from_str("iv"),
        Uint8Array::from(iv).as_ref(),
    );
    if !aad.is_empty() {
        let _ = Reflect::set(
            &params,
            &JsValue::from_str("additionalData"),
            Uint8Array::from(aad).as_ref(),
        );
    }
    params
}

pub(super) fn seal_err(value: JsValue) -> KeyError {
    KeyError::Seal(value.as_string().unwrap_or_else(|| format!("{value:?}")))
}
