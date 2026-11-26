use std::cell::RefCell;
use std::rc::Rc;

use futures::channel::oneshot;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{IdbDatabase, IdbFactory, IdbObjectStore, IdbRequest, IdbTransactionMode};

use crate::KeyError;

const DB_NAME: &str = "fundamental_device_keys";
const STORE_NAME: &str = "keypairs";

/// Opens (creating on first use) the device-keys database.
///
/// # Errors
/// [`KeyError::Storage`] if IndexedDB is unavailable or the open fails.
pub(super) async fn open() -> Result<IdbDatabase, KeyError> {
    let factory = idb_factory()?;
    let request = factory
        .open_with_u32(DB_NAME, 1)
        .map_err(|e| KeyError::Storage(format!("opening IndexedDB: {}", js_message(&e))))?;

    let upgrade_request = request.clone();
    let on_upgrade = Closure::<dyn FnMut()>::new(move || {
        if let Ok(result) = upgrade_request.result() {
            let db: IdbDatabase = result.unchecked_into();
            let _ = db.create_object_store(STORE_NAME);
        }
    });
    request.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));

    let result = await_request(request.as_ref()).await?;
    request.set_onupgradeneeded(None);
    Ok(result.unchecked_into())
}

/// The stored key pair for `key`, if any.
pub(super) async fn get(db: &IdbDatabase, key: &str) -> Result<Option<JsValue>, KeyError> {
    let store = readonly_store(db)?;
    let request = store
        .get(&JsValue::from_str(key))
        .map_err(|e| KeyError::Storage(format!("IndexedDB get: {}", js_message(&e))))?;
    let value = await_request(&request).await?;
    if value.is_undefined() || value.is_null() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

/// Stores `value` (a `CryptoKeyPair`) under `key`, replacing any existing entry.
pub(super) async fn put(db: &IdbDatabase, key: &str, value: &JsValue) -> Result<(), KeyError> {
    let store = readwrite_store(db)?;
    let request = store
        .put_with_key(value, &JsValue::from_str(key))
        .map_err(|e| KeyError::Storage(format!("IndexedDB put: {}", js_message(&e))))?;
    await_request(&request).await?;
    Ok(())
}

fn idb_factory() -> Result<IdbFactory, KeyError> {
    let global = js_sys::global();
    let idb = js_sys::Reflect::get(&global, &JsValue::from_str("indexedDB"))
        .map_err(|_| KeyError::Storage("no global `indexedDB`".to_string()))?;
    if idb.is_undefined() || idb.is_null() {
        return Err(KeyError::Storage("IndexedDB is unavailable".to_string()));
    }
    idb.dyn_into::<IdbFactory>()
        .map_err(|_| KeyError::Storage("`indexedDB` is not an IdbFactory".to_string()))
}

fn readonly_store(db: &IdbDatabase) -> Result<IdbObjectStore, KeyError> {
    let transaction = db
        .transaction_with_str(STORE_NAME)
        .map_err(|e| KeyError::Storage(format!("IndexedDB transaction: {}", js_message(&e))))?;
    transaction
        .object_store(STORE_NAME)
        .map_err(|e| KeyError::Storage(format!("IndexedDB object store: {}", js_message(&e))))
}

fn readwrite_store(db: &IdbDatabase) -> Result<IdbObjectStore, KeyError> {
    let transaction = db
        .transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readwrite)
        .map_err(|e| KeyError::Storage(format!("IndexedDB transaction: {}", js_message(&e))))?;
    transaction
        .object_store(STORE_NAME)
        .map_err(|e| KeyError::Storage(format!("IndexedDB object store: {}", js_message(&e))))
}

async fn await_request(request: &IdbRequest) -> Result<JsValue, KeyError> {
    let (sender, receiver) = oneshot::channel::<bool>();
    let slot = Rc::new(RefCell::new(Some(sender)));

    let success_slot = slot.clone();
    let on_success = Closure::<dyn FnMut()>::new(move || {
        if let Some(sender) = success_slot.borrow_mut().take() {
            let _ = sender.send(true);
        }
    });
    let error_slot = slot;
    let on_error = Closure::<dyn FnMut()>::new(move || {
        if let Some(sender) = error_slot.borrow_mut().take() {
            let _ = sender.send(false);
        }
    });
    request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
    request.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    let succeeded = receiver
        .await
        .map_err(|_| KeyError::Storage("IndexedDB request was dropped".to_string()))?;
    request.set_onsuccess(None);
    request.set_onerror(None);

    if succeeded {
        request
            .result()
            .map_err(|e| KeyError::Storage(format!("IndexedDB result: {}", js_message(&e))))
    } else {
        Err(KeyError::Storage("IndexedDB request failed".to_string()))
    }
}

fn js_message(value: &JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
