use std::time::Duration;

use fundamental_ws::WsEnvelope;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;

use super::RawWs;

pub async fn call(ws: &mut RawWs, action: &str, data: Value) -> Value {
    let text = serde_json::to_string(&WsEnvelope::new(action, data)).expect("frame");
    ws.send(Message::text(text)).await.expect("send");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Message::Text(text) = ws.next().await.expect("a frame").expect("ok") {
                return serde_json::from_str::<Value>(&text).expect("json");
            }
        }
    })
    .await
    .expect("a reply within ten seconds")
}
