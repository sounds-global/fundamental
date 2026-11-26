use fundamental_offline::store::OfflineRecord;
use fundamental_sync::SliceBinding;
use fundamental_sync::wire::HostedRows;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::{Value, json};
use tracing::{debug, error};
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::{Closure, wasm_bindgen};

use crate::call_action;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = invoke, catch)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    /// Registers a callback with Tauri's IPC and returns its handler id.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = transformCallback)]
    fn transform_callback(callback: &Closure<dyn FnMut(JsValue)>, once: bool) -> JsValue;
}

/// Binds `binding` on the host and returns a signal of its rows, re-read on every change.
pub(super) fn hosted_signal<M>(binding: SliceBinding) -> Signal<Vec<M>>
where
    M: OfflineRecord + Clone + Send + Sync + 'static,
{
    let rows = RwSignal::new(Vec::<M>::new());

    // The cleanup closure must be `Send + Sync` and teardown is async, so unmounting only signals.
    let (shutdown_tx, mut shutdown_rx) = unbounded::<()>();
    on_cleanup(move || {
        let _ = shutdown_tx.unbounded_send(());
    });

    spawn_local(async move {
        let slice = binding.slice().to_owned();
        let request = json!({ "slice": &slice, "params": binding.params() });

        if let Err(error) =
            call_action::<_, Value>("POST", "/_fundamental/live/bind", &request).await
        {
            error!(%error, slice, "hosted live bind failed; live view is empty");
            return;
        }

        // The closure stays owned by this task: it is unlistened before it drops.
        let (changed_tx, mut changed_rx) = unbounded::<()>();
        let event = format!("fundamental:live:{}", binding.local_table());
        let callback = Closure::new(move |_: JsValue| notify(&changed_tx));
        let listener = match listen(&event, &callback).await {
            Ok(listener) => Some(listener),
            Err(error) => {
                // The first read still lands; it just never refreshes.
                error!(
                    error,
                    slice, "hosted live listen failed; view will not refresh"
                );
                None
            }
        };

        let refresh = |rows: RwSignal<Vec<M>>, slice: String| async move {
            let request = json!({ "slice": &slice });
            match call_action::<_, HostedRows>("POST", "/_fundamental/live/rows", &request).await {
                Ok(payload) => match payload.typed::<M>() {
                    Ok(items) => rows.set(items),
                    Err(error) => error!(%error, slice, "hosted rows did not fit the model"),
                },
                Err(error) => error!(%error, slice, "hosted rows read failed"),
            }
        };

        refresh(rows, slice.clone()).await; // seed from whatever the host holds
        loop {
            futures::select! {
                changed = changed_rx.next() => {
                    if changed.is_none() {
                        break;
                    }
                    refresh(rows, slice.clone()).await;
                }
                _ = shutdown_rx.next() => break,
            }
        }

        // Stop host-side delivery before the callback drops with this task.
        if let Some(listener) = listener
            && let Err(error) = unlisten(&event, &listener).await
        {
            debug!(error, slice, "hosted live unlisten failed");
        }
        let request = json!({ "slice": &slice, "params": binding.params() });
        if let Err(error) =
            call_action::<_, Value>("POST", "/_fundamental/live/unbind", &request).await
        {
            debug!(%error, slice, "hosted live unbind failed");
        }
        drop(callback);
    });

    rows.into()
}

/// Forwards one change notification; a full queue means a refresh is already pending.
fn notify(changed: &UnboundedSender<()>) {
    let _ = changed.unbounded_send(());
}

async fn listen(event: &str, callback: &Closure<dyn FnMut(JsValue)>) -> Result<JsValue, String> {
    let handler = transform_callback(callback, false);
    let args = js_sys::Object::new();
    let target = js_sys::Object::new();
    set(&target, "kind", &JsValue::from_str("Any"))?;
    set(&args, "event", &JsValue::from_str(event))?;
    set(&args, "target", &target)?;
    set(&args, "handler", &handler)?;
    tauri_invoke("plugin:event|listen", args.into())
        .await
        .map_err(render_js)
}

async fn unlisten(event: &str, id: &JsValue) -> Result<(), String> {
    let args = js_sys::Object::new();
    set(&args, "event", &JsValue::from_str(event))?;
    set(&args, "eventId", id)?;
    tauri_invoke("plugin:event|unlisten", args.into())
        .await
        .map(|_| ())
        .map_err(render_js)
}

fn set(target: &js_sys::Object, key: &str, value: &JsValue) -> Result<(), String> {
    js_sys::Reflect::set(target, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(render_js)
}

fn render_js(value: JsValue) -> String {
    value
        .as_string()
        .or_else(|| js_sys::Error::from(value).message().as_string())
        .unwrap_or_else(|| "JavaScript error".into())
}
