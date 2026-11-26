//! A handler that keeps its context after the connection ends keeps no slot with it.

use std::sync::{Arc, Mutex};

use fundamental_ws::{Route, WsContext, WsError, WsResult, WsRouter};
use serde::Deserialize;

mod support;

use support::{config, connect, send, serve, until};

#[derive(Deserialize)]
struct Keep {}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_context_held_after_close_holds_no_slot() {
    let kept: Arc<Mutex<Option<WsContext>>> = Arc::default();
    let router = WsRouter::new().route(Route::new("keep", {
        let kept = Arc::clone(&kept);
        move |context: WsContext, _: Keep| {
            let kept = Arc::clone(&kept);
            async move {
                *kept.lock().expect("lock") = Some(context.clone());
                context.send(serde_json::json!({ "kept": true })).await
            }
        }
    }));
    let (listener, stats) = serve(config().public(), router).await;
    let mut socket = connect(&listener).await;
    send(&mut socket, "keep", serde_json::json!({})).await;
    assert_eq!(support::reply(&mut socket).await["kept"], true);
    drop(socket);

    until(|| stats.public() == 0, "the slot is given back").await;
    let context = kept.lock().expect("lock").take().expect("kept");
    let late: WsResult<()> = context.send("late").await;
    assert!(matches!(late, Err(WsError::Closed)));
}
