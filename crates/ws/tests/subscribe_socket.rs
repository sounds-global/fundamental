//! `#[subscribe(socket = "…")]` expands to a `Route` a `WsRouter` can serve.

use fundamental_macros::subscribe;
use fundamental_ws::{WsConfig, WsContext, WsResult, WsRouter, WsServer};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Deserialize)]
struct Ping {
    n: u64,
}

#[derive(Serialize)]
struct Pong {
    n: u64,
}

#[subscribe(socket = "ping")]
async fn on_ping(context: WsContext, ping: Ping) -> WsResult<()> {
    context.send(Pong { n: ping.n + 1 }).await
}

#[tokio::test]
async fn socket_handler_dispatches_through_a_router() {
    let router = WsRouter::new().route(on_ping_route());
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let url = format!("ws://{}", listener.local_addr());

    let (mut socket, _) = connect_async(url).await.expect("connect");
    socket
        .send(Message::text(r#"{"action":"ping","data":{"n":41}}"#))
        .await
        .expect("send");

    let reply = loop {
        match socket.next().await.expect("frame").expect("ok") {
            Message::Text(text) => break text.to_string(),
            _ => continue,
        }
    };
    let pong: serde_json::Value = serde_json::from_str(&reply).expect("json");
    assert_eq!(pong["n"], 42);
    listener.stop().await.expect("the server drains");
}
