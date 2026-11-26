//! A device whose hub drains for a deploy waits at least the going-away floor before it
//! redials — the server's `1001` and the client's backoff meeting end to end.
//!
//! The test holds a clone of the socket, so the redial lands on the new hub.

#![cfg(not(target_arch = "wasm32"))]

use std::rc::Rc;
use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_macros::Model;
use fundamental_offline::db::{OfflineConfig, OfflineDatabase};
use fundamental_sync::ConflictStrategy;
use fundamental_sync::SliceBinding;
use fundamental_sync::wire::{AuthAck, AuthRequest, SlicePullRequest, SlicePullResponse};
use fundamental_ws::test_support::until;
use fundamental_ws::{CloseReason, Route, WsConfig, WsContext, WsResult, WsRouter, WsServer};
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::LocalSet;
use tokio::time::Instant;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "notes")]
struct Note {
    #[model(id)]
    id: String,
    body: String,
}

/// The least a client waits after a `1001` close before it redials.
const GOING_AWAY_FLOOR: Duration = Duration::from_secs(1);
/// How much earlier than the floor the redial may be seen: timer and clock are read on
/// different tasks, and the server is polled every 10 ms.
const MEASUREMENT_SLACK: Duration = Duration::from_millis(100);

async fn auth(context: WsContext, request: AuthRequest) -> WsResult<()> {
    let holder = request.token.clone();
    context.bind_principal(&holder, request.token)?;
    context
        .send(AuthAck::authenticated(request.request_id))
        .await
}

async fn pull(context: WsContext, request: SlicePullRequest) -> WsResult<()> {
    context
        .send(SlicePullResponse::last(request.request_id, Vec::new()))
        .await
}

/// A hub serving `socket`, which the test also holds.
fn hub(socket: &std::net::TcpListener) -> WsServer {
    let clone = socket.try_clone().expect("the socket is cloned");
    let listener = TcpListener::from_std(clone).expect("a tokio listener");
    let addr = listener.local_addr().expect("an address");
    let router = WsRouter::new()
        .route(Route::new("auth", auth))
        .route(Route::new("slice.pull", pull));
    WsServer::new(WsConfig::new(addr), router).with_listener(listener)
}

#[tokio::test]
async fn a_going_away_close_waits_its_floor_and_the_device_comes_back() {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("bound");
    socket.set_nonblocking(true).expect("non-blocking");
    let url = format!("ws://{}", socket.local_addr().expect("an address"));
    let first = hub(&socket).serve().await.expect("the first hub serves");
    let first_stats = first.stats();

    let db = Rc::new(
        OfflineDatabase::open(OfflineConfig::memory())
            .await
            .expect("an in-memory database"),
    );
    let local = LocalSet::new();
    local
        .run_until(async {
            let _client = SyncClient::start::<Note>(
                SyncClientConfig::new(&url, SecretString::new("device".to_string()))
                    .with_live_subscribe(false)
                    .with_reconcile_interval(None),
                SliceBinding::new("mine", "notes", json!({}), ConflictStrategy::default()),
                Rc::clone(&db),
            );
            until(
                || first_stats.authenticated() == 1,
                "the device is bound to the first hub",
            )
            .await;

            let stop_began = Instant::now();
            first.stop().await.expect("the first hub drains");
            assert_eq!(first_stats.closes(CloseReason::GoingAway), 1);

            let second = hub(&socket).serve().await.expect("the second hub serves");
            let second_stats = second.stats();
            until(
                || second_stats.authenticated() == 1,
                "the device came back to the second hub",
            )
            .await;
            let redialled = stop_began.elapsed();
            assert!(
                redialled + MEASUREMENT_SLACK >= GOING_AWAY_FLOOR,
                "redialled {redialled:?} after a 1001, under the {GOING_AWAY_FLOOR:?} floor"
            );
            second.stop().await.expect("the second hub drains");
        })
        .await;
}
