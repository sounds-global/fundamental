//! `#[subscribe(socket)]` (bare) binds a `Message` payload, its action the message's subject.

use fundamental_macros::{mount_messages, subscribe};
use fundamental_nats::Message;
use fundamental_ws::{WsConfig, WsContext, WsResult, WsRouter, WsServer};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[derive(Serialize, Deserialize, Message)]
#[subject = "chat.say"]
struct Say {
    text: String,
}

// Without this line, `on_say` would not compile.
mount_messages! { Say }

#[subscribe(socket)]
async fn on_say(context: WsContext, say: Say) -> WsResult<()> {
    context.send(serde_json::json!({ "echo": say.text })).await
}

#[tokio::test]
async fn bare_socket_routes_on_the_message_subject() {
    let router = WsRouter::new().route(on_say_route());
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let url = format!("ws://{}", listener.local_addr());

    let (mut socket, _) = connect_async(url).await.expect("connect");
    socket
        .send(WsMessage::text(
            r#"{"action":"chat.say","data":{"text":"hi"}}"#,
        ))
        .await
        .expect("send");

    let reply = loop {
        match socket.next().await.expect("frame").expect("ok") {
            WsMessage::Text(text) => break text.to_string(),
            _ => continue,
        }
    };
    let value: serde_json::Value = serde_json::from_str(&reply).expect("json");
    assert_eq!(value["echo"], "hi");
    listener.stop().await.expect("the server drains");
}
