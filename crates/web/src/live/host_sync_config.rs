use fundamental_ws_client::SyncClientConfig;

/// Reads the sync connection a native host injected as `window.__FUNDAMENTAL_SYNC__`, if any.
#[must_use]
pub fn host_sync_config() -> Option<SyncClientConfig> {
    #[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
    {
        read_handoff()
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "hydrate")))]
    {
        None
    }
}

#[cfg(all(target_arch = "wasm32", feature = "hydrate"))]
fn read_handoff() -> Option<SyncClientConfig> {
    use fundamental_core::secrets::SecretString;
    use js_sys::Reflect;
    use tracing::{debug, warn};
    use wasm_bindgen::JsValue;

    const GLOBAL: &str = "__FUNDAMENTAL_SYNC__";

    let handoff = Reflect::get(&js_sys::global(), &JsValue::from_str(GLOBAL)).ok()?;
    if handoff.is_undefined() || handoff.is_null() {
        debug!("no sync handoff global; not running under a native host");
        return None;
    }

    let field = |name: &str| {
        Reflect::get(&handoff, &JsValue::from_str(name))
            .ok()
            .and_then(|value| value.as_string())
    };

    let Some(url) = field("url") else {
        warn!("sync handoff global is present but its url is missing or not a string");
        return None;
    };

    let token = match field("token") {
        Some(token) => SecretString::new(token),
        None => {
            debug!("sync handoff carries no token; connecting unauthenticated");
            SecretString::new(String::new())
        }
    };

    Some(SyncClientConfig::new(url, token))
}
