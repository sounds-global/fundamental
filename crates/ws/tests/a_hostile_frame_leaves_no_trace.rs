//! A frame the server refuses leaves nothing of itself in the logs.
//!
//! One test in its own binary, because it installs the process's global subscriber.

use fundamental_observability::testing::CapturedLogs;
use fundamental_ws::{Route, WsConfig, WsContext, WsResult, WsRouter, WsServer};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const EMAIL: &str = "jan@example.nl";
/// A phone number, as a JSON number a client can put where a string belongs.
const PHONE: u64 = 31_612_345_678;

#[derive(Deserialize)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
struct Pull {
    #[allow(dead_code)]
    since: i64,
}

async fn pull(context: WsContext, _pull: Pull) -> WsResult<()> {
    context.send(serde_json::json!({ "ok": true })).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn a_hostile_frame_leaves_no_trace() {
    let captured = CapturedLogs::install().expect("the only subscriber");

    let router = WsRouter::new().route(Route::new("slice.pull", pull));
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let (mut socket, _) = connect_async(format!("ws://{}", listener.local_addr()))
        .await
        .expect("connect");

    let frames = [
        (format!(r#"{{"action":"{EMAIL}","data":{{}}}}"#), "WS_003"),
        (
            format!(r#"{{"action":"slice.pull","data":{{"since":"{EMAIL}"}}}}"#),
            "WS_002",
        ),
        (
            format!(r#"{{"action":"slice.pull","data":{{"{EMAIL}":1}}}}"#),
            "WS_002",
        ),
        (
            format!(r#"{{"action":{{"{EMAIL}":1}},"data":{{}}}}"#),
            "WS_002",
        ),
        (format!("{EMAIL}\nforged line"), "WS_002"),
        // serde quotes a number of the wrong type: ``invalid type: integer `…` ``.
        (format!(r#"{{"action":{PHONE},"data":{{}}}}"#), "WS_002"),
    ];
    for (frame, code) in &frames {
        socket
            .send(Message::text(frame.as_str()))
            .await
            .expect("send");
        let reply = loop {
            if let Message::Text(text) = socket.next().await.expect("a frame").expect("ok") {
                break text.to_string();
            }
        };
        let reply: serde_json::Value = serde_json::from_str(&reply).expect("json reply");
        assert_eq!(reply["error"]["code"], *code, "{frame}");
        assert!(
            !reply.to_string().contains(EMAIL) && !reply.to_string().contains(&PHONE.to_string()),
            "the reply quotes the frame: {reply}"
        );
    }
    socket
        .send(Message::text(
            r#"{"action":"slice.pull","data":{"since":0}}"#,
        ))
        .await
        .expect("send");
    let _ = socket.next().await;
    listener.stop().await.expect("the server drains");

    let written = captured.text();
    assert!(written.contains("action=\"unknown\""), "{written}");
    assert!(written.contains("action=\"slice.pull\""), "{written}");
    assert!(
        written.contains("frame=7"),
        "the server numbers frames: {written}"
    );
    for needle in [EMAIL, "forged line", &PHONE.to_string()] {
        assert!(
            !written.contains(needle),
            "`{needle}` reached the logs:\n{written}"
        );
    }
}
