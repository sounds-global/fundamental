//! `SIGTERM` to a runtime serving WebSockets drains it.
//!
//! One test in its own binary, because the signal is delivered to the whole process.

#![cfg(unix)]

use std::io::ErrorKind;
use std::process::Command;

use fundamental_core::ServiceState;
use fundamental_runtime::Runtime;
use fundamental_ws::test_support::{PATIENCE, close_frame, connect_to, next_text, until};
use fundamental_ws::{CloseReason, Route, WsConfig, WsContext, WsRouter, WsServer};
use futures::SinkExt;
use serde_json::json;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

#[derive(serde::Deserialize)]
struct Empty {}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
async fn sigterm_drains_a_runtime_serving_websockets() {
    // Tokio's handler now covers the process, so a SIGTERM reaches the runtime instead of ending the test.
    let _handler = signal(SignalKind::terminate()).expect("a SIGTERM handler");

    let started = watch::Sender::new(false);
    let release = watch::Sender::new(false);
    let router = {
        let (started, release) = (started.clone(), release.clone());
        WsRouter::new().route(Route::new("work", move |context: WsContext, _: Empty| {
            let (started, mut release) = (started.clone(), release.subscribe());
            async move {
                started.send_replace(true);
                let _ = release.wait_for(|open| *open).await;
                context.send(json!({ "done": true })).await
            }
        }))
    };
    let bound = TcpListener::bind("127.0.0.1:0").await.expect("bound");
    let addr = bound.local_addr().expect("an address");
    let server = WsServer::new(WsConfig::new(addr).public(), router).with_listener(bound);
    let stats = server.stats();
    let state = ServiceState::new();
    let runtime = tokio::spawn(
        Runtime::new()
            .with(server)
            .with_service_state(state.clone())
            .run(),
    );
    until(|| state.is_ready(), "the runtime is serving").await;

    let mut busy = connect_to(addr).await;
    let mut idle = connect_to(addr).await;
    until(|| stats.public() == 2, "two connections admitted").await;
    busy.send(Message::text(
        json!({ "action": "work", "data": {} }).to_string(),
    ))
    .await
    .expect("the frame is sent");
    timeout(PATIENCE, started.subscribe().wait_for(|begun| *begun))
        .await
        .expect("the handler started in time")
        .expect("the gate is held");

    let killed = Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success(), "SIGTERM was sent");

    assert_eq!(
        close_frame(&mut idle).await,
        (1001, "going_away".to_string())
    );
    assert!(state.is_draining());
    let refused = TcpStream::connect(addr)
        .await
        .expect_err("the socket refuses connections while it drains");
    assert_eq!(refused.kind(), ErrorKind::ConnectionRefused);
    assert!(!runtime.is_finished(), "the drain waits for the handler");

    release.send_replace(true);
    assert_eq!(next_text(&mut busy).await, json!({ "done": true }));
    assert_eq!(
        close_frame(&mut busy).await,
        (1001, "going_away".to_string())
    );
    timeout(PATIENCE, runtime)
        .await
        .expect("the runtime finished in time")
        .expect("not panicked")
        .expect("a clean shutdown");
    assert_eq!(stats.closes(CloseReason::GoingAway), 2);
}
