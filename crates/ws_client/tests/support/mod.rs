//! The stub hub the sync-client integration tests share; each test adds its own `slice.pull`.

#![allow(dead_code)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use std::net::SocketAddr;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fundamental_core::model::offline::ConflictStrategy;
use fundamental_macros::{Model, subscribe};
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_sync::wire::{AuthAck, AuthRequest, SliceSubscribeAck, SliceSubscribeRequest};
use fundamental_sync::{SliceBinding, Syncable};
use fundamental_ws::{WsConfig, WsContext, WsListener, WsResult, WsRouter, WsServer};
use serde_json::Value;

/// The row a test binding syncs.
#[derive(Clone, Debug, Model)]
#[model(offline, syncable)]
pub struct Note {
    #[model(id)]
    id: String,
    body: String,
}

/// Connections the hub has authenticated, in this test binary.
pub static AUTHENTICATED: AtomicUsize = AtomicUsize::new(0);

#[subscribe(socket = "auth")]
pub async fn on_auth(context: WsContext, request: AuthRequest) -> WsResult<()> {
    AUTHENTICATED.fetch_add(1, Ordering::SeqCst);
    context
        .send(AuthAck::authenticated(request.request_id))
        .await
}

#[subscribe(socket = "slice.subscribe")]
pub async fn on_subscribe(context: WsContext, request: SliceSubscribeRequest) -> WsResult<()> {
    context
        .send(SliceSubscribeAck::new(request.request_id, request.slice))
        .await
}

/// A router that authenticates and subscribes; the test routes `slice.pull` itself.
pub fn hub_router() -> WsRouter {
    WsRouter::new()
        .route(on_auth_route())
        .route(on_subscribe_route())
}

/// Serves `router` on an ephemeral loopback port, returning the guard and the `ws://` URL.
pub async fn serve(router: WsRouter) -> (WsListener, String) {
    let bind: SocketAddr = "127.0.0.1:0"
        .parse()
        .unwrap_or_else(|error| panic!("loopback address: {error}"));
    let listener = WsServer::new(WsConfig::new(bind), router)
        .serve()
        .await
        .unwrap_or_else(|error| panic!("serve the stub hub: {error}"));
    let url = format!("ws://{}", listener.local_addr());
    (listener, url)
}

/// A fresh in-memory device database.
pub async fn memory_db() -> Rc<OfflineDatabase> {
    Rc::new(
        OfflineDatabase::open(OfflineConfig::memory())
            .await
            .unwrap_or_else(|error| panic!("open in-memory db: {error}")),
    )
}

/// The binding every test syncs: all notes, last write wins.
pub fn notes_binding() -> SliceBinding {
    SliceBinding::new(
        "notes",
        Note::MIRROR_TABLE,
        Value::Null,
        ConflictStrategy::LastWriteWins,
    )
}
