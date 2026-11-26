//! End-to-end tests: a real `WsServer` on an ephemeral port, driven by a tokio-tungstenite client.

use fundamental_ws::{Route, WsConfig, WsContext, WsResult, WsRouter, WsServer};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Deserialize)]
struct Chat {
    text: String,
}

#[derive(Serialize)]
struct Echo {
    echo: String,
}

async fn chat(context: WsContext, message: Chat) -> WsResult<()> {
    context.send(Echo { echo: message.text }).await
}

async fn start() -> (String, fundamental_ws::WsListener) {
    let router = WsRouter::new().route(Route::new("chat", chat));
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let url = format!("ws://{}", listener.local_addr());
    (url, listener)
}

async fn next_text<S>(socket: &mut S) -> String
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match socket.next().await.expect("a frame").expect("ok frame") {
            Message::Text(text) => return text.to_string(),
            _ => continue,
        }
    }
}

#[tokio::test]
async fn round_trips_a_chat_message() {
    let (url, listener) = start().await;
    let (mut socket, _) = connect_async(url).await.expect("connect");

    socket
        .send(Message::text(
            r#"{"action":"chat","data":{"text":"hi there"}}"#,
        ))
        .await
        .expect("send");

    let reply: serde_json::Value =
        serde_json::from_str(&next_text(&mut socket).await).expect("json reply");
    assert_eq!(reply["echo"], "hi there");
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn unknown_action_replies_ws_003() {
    let (url, listener) = start().await;
    let (mut socket, _) = connect_async(url).await.expect("connect");

    socket
        .send(Message::text(r#"{"action":"nope","data":{}}"#))
        .await
        .expect("send");

    let reply: serde_json::Value =
        serde_json::from_str(&next_text(&mut socket).await).expect("json reply");
    assert_eq!(reply["error"]["code"], "WS_003");
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn malformed_payload_replies_ws_002() {
    let (url, listener) = start().await;
    let (mut socket, _) = connect_async(url).await.expect("connect");

    socket
        .send(Message::text(r#"{"action":"chat","data":{"text":42}}"#))
        .await
        .expect("send");

    let reply: serde_json::Value =
        serde_json::from_str(&next_text(&mut socket).await).expect("json reply");
    assert_eq!(reply["error"]["code"], "WS_002");
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn non_envelope_frame_replies_ws_002() {
    let (url, listener) = start().await;
    let (mut socket, _) = connect_async(url).await.expect("connect");

    socket
        .send(Message::text("this is not json"))
        .await
        .expect("send");

    let reply: serde_json::Value =
        serde_json::from_str(&next_text(&mut socket).await).expect("json reply");
    assert_eq!(reply["error"]["code"], "WS_002");
    listener.stop().await.expect("the server drains");
}
