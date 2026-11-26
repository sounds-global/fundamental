//! The key types link and round-trip under `wasm-bindgen-test`.
//!
//! Run with `wasm-pack test --headless --firefox crates/keys` (or Chrome).

#![cfg(target_arch = "wasm32")]

use fundamental_core::Error as _;
use fundamental_keys::{
    DeviceId, DeviceSigner, KeyError, Signature, VerifyingKey, WebCryptoSigner,
};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn public_key_and_signature_round_trip_through_base64() {
    let key = VerifyingKey::from_bytes([5u8; 32]);
    let parsed: VerifyingKey = key.to_string().parse().expect("parse key");
    assert_eq!(parsed, key);

    let sig = Signature::from_bytes([6u8; 64]);
    let parsed: Signature = sig.to_string().parse().expect("parse sig");
    assert_eq!(parsed, sig);
}

#[wasm_bindgen_test]
fn key_error_metadata_is_stable() {
    assert_eq!(KeyError::BadSignature.error_code(), "KEYS_002");
    assert_eq!(KeyError::BadSignature.http_status_code(), 401);
}

// Needs a live WebCrypto and IndexedDB context.
#[wasm_bindgen_test]
async fn webcrypto_key_persists_across_loads() {
    let device = DeviceId::new("persist-smoke-test");
    let first = WebCryptoSigner::load_or_create(device.clone())
        .await
        .expect("create");
    let public_key = first.verifying_key();

    let second = WebCryptoSigner::load_or_create(device)
        .await
        .expect("reload");
    assert_eq!(
        second.verifying_key(),
        public_key,
        "the persisted key must be the same across loads"
    );
}

// Needs a live WebCrypto and IndexedDB context: asking whether a key exists never creates one.
#[wasm_bindgen_test]
async fn loading_a_key_that_is_not_there_mints_nothing() {
    let device = DeviceId::new("load-only-smoke-test");
    assert!(
        WebCryptoSigner::load(device.clone())
            .await
            .expect("load")
            .is_none(),
        "a device that has never been seen holds no key"
    );
    assert!(
        WebCryptoSigner::load(device.clone())
            .await
            .expect("load again")
            .is_none(),
        "asking twice must still answer no"
    );

    let minted = WebCryptoSigner::load_or_create(device.clone())
        .await
        .expect("create");
    let loaded = WebCryptoSigner::load(device)
        .await
        .expect("load after create");
    assert_eq!(
        loaded.map(|signer| signer.verifying_key()),
        Some(minted.verifying_key()),
        "once a key exists, the load-only path finds exactly it"
    );
}
