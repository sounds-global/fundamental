//! In-browser proof of the sync handoff contract read by `host_sync_config`.
//!
//! Run with:
//!   wasm-pack test --headless --firefox crates/web --no-default-features --features hydrate,live

#![cfg(all(target_arch = "wasm32", feature = "live"))]

use fundamental_web::live::host_sync_config;
use fundamental_ws_client::SyncAuth;
use js_sys::Reflect;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const GLOBAL: &str = "__FUNDAMENTAL_SYNC__";

fn set_global(value: &JsValue) {
    Reflect::set(&js_sys::global(), &JsValue::from_str(GLOBAL), value).unwrap();
}

fn clear_global() {
    set_global(&JsValue::UNDEFINED);
}

#[wasm_bindgen_test]
fn absent_global_yields_none() {
    clear_global();
    assert!(host_sync_config().is_none());
}

#[wasm_bindgen_test]
fn a_global_without_a_url_is_rejected() {
    let handoff = js_sys::Object::new();
    Reflect::set(&handoff, &"token".into(), &"t".into()).unwrap();
    set_global(&handoff);
    assert!(host_sync_config().is_none());
    clear_global();
}

#[wasm_bindgen_test]
fn a_non_string_url_is_rejected() {
    let handoff = js_sys::Object::new();
    Reflect::set(&handoff, &"url".into(), &JsValue::from_f64(42.0)).unwrap();
    set_global(&handoff);
    assert!(host_sync_config().is_none());
    clear_global();
}

#[wasm_bindgen_test]
fn url_and_token_round_trip_from_the_injected_script() {
    let handoff = js_sys::Object::new();
    Reflect::set(&handoff, &"url".into(), &"ws://localhost:9100".into()).unwrap();
    Reflect::set(&handoff, &"token".into(), &"dev-token".into()).unwrap();
    set_global(&handoff);

    let config = host_sync_config().expect("a well-formed handoff parses");
    assert_eq!(config.url, "ws://localhost:9100");
    assert!(matches!(&config.auth, SyncAuth::Jwt(t) if t.expose() == "dev-token"));
    clear_global();
}

#[wasm_bindgen_test]
fn a_missing_token_still_connects_unauthenticated() {
    let handoff = js_sys::Object::new();
    Reflect::set(&handoff, &"url".into(), &"ws://localhost:9100".into()).unwrap();
    set_global(&handoff);

    let config = host_sync_config().expect("url alone is enough");
    assert_eq!(config.url, "ws://localhost:9100");
    assert!(matches!(&config.auth, SyncAuth::Jwt(t) if t.expose().is_empty()));
    clear_global();
}
