//! A real server with the routes the bound tests drive, and a client that reads what it is told.

#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::time::Duration;

use fundamental_ws::{CloseReason, Route, WsConfig, WsContext, WsResult, WsRouter, WsStats};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

// Each suite that includes this module uses a different part of it.
#[allow(unused_imports)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
pub use fundamental_ws::test_support::{Socket, close_frame, connect, serve, until};

pub const PATIENCE: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
pub struct Login {
    pub user: String,
}

#[derive(Deserialize)]
pub struct Burst {
    pub frames: usize,
    pub bytes: usize,
}

#[derive(Deserialize)]
pub struct Empty {}

/// Binds `user` as the principal: a stand-in for a verified login.
async fn login(context: WsContext, login: Login) -> WsResult<()> {
    let holder = login.user.clone();
    match context.bind_principal(&holder, login.user) {
        Ok(()) => {
            context
                .send(serde_json::json!({ "authenticated": true }))
                .await
        }
        Err(error) => {
            use fundamental_core::Error as _;
            context
                .send(serde_json::json!({ "error": { "code": error.error_code() } }))
                .await
        }
    }
}

async fn ping(context: WsContext, _: Empty) -> WsResult<()> {
    context.send(serde_json::json!({ "pong": true })).await
}

/// Queues `frames` replies of `bytes` each, for a peer that is not reading.
async fn burst(context: WsContext, burst: Burst) -> WsResult<()> {
    let body = "x".repeat(burst.bytes);
    for _ in 0..burst.frames {
        context.send(serde_json::json!({ "burst": body })).await?;
    }
    Ok(())
}

async fn explode(_context: WsContext, _: Empty) -> WsResult<()> {
    panic!("a handler bug");
}

pub fn router() -> WsRouter {
    WsRouter::new()
        .route(Route::new("login", login))
        .route(Route::new("ping", ping))
        .route(Route::new("burst", burst))
        .route(Route::new("explode", explode))
}

pub fn config() -> WsConfig {
    WsConfig::new("127.0.0.1:0".parse().expect("addr"))
}

pub async fn send(socket: &mut Socket, action: &str, data: serde_json::Value) {
    let frame = serde_json::json!({ "action": action, "data": data }).to_string();
    socket.send(Message::text(frame)).await.expect("send");
}

pub async fn reply(socket: &mut Socket) -> serde_json::Value {
    loop {
        let frame = timeout(PATIENCE, socket.next())
            .await
            .expect("a reply in time")
            .expect("a frame")
            .expect("an ok frame");
        match frame {
            Message::Text(text) => return serde_json::from_str(&text).expect("json"),
            Message::Close(frame) => panic!("closed instead of replying: {frame:?}"),
            _ => {}
        }
    }
}

pub async fn log_in(socket: &mut Socket, user: &str) {
    send(socket, "login", serde_json::json!({ "user": user })).await;
    assert_eq!(reply(socket).await["authenticated"], true);
}

pub async fn closed_for(stats: &WsStats, reason: CloseReason) {
    until(|| stats.closes(reason) > 0, reason.label()).await;
}
