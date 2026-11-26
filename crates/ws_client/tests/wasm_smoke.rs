//! Browser smoke test: the crate's surface links and runs under `wasm-bindgen-test`.

#![cfg(target_arch = "wasm32")]

use fundamental_core::Error as _;
use fundamental_ws_client::TransportError;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn transport_error_metadata_is_stable() {
    assert_eq!(TransportError::Closed.error_code(), "WSC_002");
    assert!(TransportError::Closed.is_retryable());
    assert_eq!(TransportError::Timeout.http_status_code(), 504);
    assert_eq!(
        TransportError::Connect("x".to_string()).error_code(),
        "WSC_001"
    );
}
