use js_sys::Uint8Array;

use super::idb_key_store;
use crate::KeyError;
use crate::hd::SealedSeed;

/// Store the sealed seed `sealed` under `name` in IndexedDB, replacing any existing entry.
///
/// # Errors
/// [`KeyError::Storage`] if IndexedDB is unavailable or the write fails.
pub async fn store_sealed_seed(name: &str, sealed: &SealedSeed) -> Result<(), KeyError> {
    let db = idb_key_store::open().await?;
    let bytes = Uint8Array::from(sealed.to_bytes().as_slice());
    idb_key_store::put(&db, name, bytes.as_ref()).await
}

/// Load the sealed seed stored under `name`, or `None` if the browser holds none yet.
///
/// # Errors
/// [`KeyError::Storage`] if IndexedDB is unavailable, the read fails, or the stored blob is
/// not a well-formed [`SealedSeed`] (a corrupt store, fail-closed).
pub async fn load_sealed_seed(name: &str) -> Result<Option<SealedSeed>, KeyError> {
    let db = idb_key_store::open().await?;
    let Some(value) = idb_key_store::get(&db, name).await? else {
        return Ok(None);
    };
    let bytes = Uint8Array::new(&value).to_vec();
    Ok(Some(SealedSeed::from_bytes(&bytes)?))
}
