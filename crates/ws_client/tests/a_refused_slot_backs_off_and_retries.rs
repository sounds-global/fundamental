//! A device refused a slot backs off and comes back, rather than treating the close as fatal.
//!
//!
//! A never-authenticating connection holds the anonymous share full, so the device is closed
//! with `1013` until it frees.

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
use fundamental_ws::{
    AnonymousLimits, CloseReason, Route, WsConfig, WsContext, WsResult, WsRouter, WsServer, WsStats,
};
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use serde_json::json;
use tokio::task::LocalSet;
use tokio::time::Instant;
use tokio_tungstenite::connect_async;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "notes")]
struct Note {
    #[model(id)]
    id: String,
    body: String,
}

/// The least a client waits after a `1013` close before it tries again.
const TRY_AGAIN_LATER_FLOOR: Duration = Duration::from_secs(5);
/// How much earlier than the floor the server may count the next attempt: it polls every 10 ms.
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

fn authenticated_and_refused(stats: &WsStats) -> bool {
    stats.authenticated() == 1 && stats.closes(CloseReason::OverShare) > 0
}

#[tokio::test]
async fn a_refused_slot_backs_off_and_retries() {
    let router = WsRouter::new()
        .route(Route::new("auth", auth))
        .route(Route::new("slice.pull", pull));
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
        .max_connections(4)
        .authenticate(
            AnonymousLimits::default()
                .max_connections(1)
                .within(Duration::from_secs(30))
                .displace_after(Duration::from_secs(20)),
        );
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let stats = listener.stats();
    let url = format!("ws://{}", listener.local_addr());

    let (holder, _) = connect_async(&url).await.expect("the holder connects");
    until(|| stats.anonymous() == 1, "the anonymous share is full").await;

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
                || stats.closes(CloseReason::OverShare) >= 1,
                "the device was refused a slot",
            )
            .await;
            let first = Instant::now();
            until(
                || stats.closes(CloseReason::OverShare) >= 2,
                "the device tried again while the share was full",
            )
            .await;
            let gap = first.elapsed();
            assert!(
                gap + MEASUREMENT_SLACK >= TRY_AGAIN_LATER_FLOOR,
                "retried {gap:?} after a 1013, under the {TRY_AGAIN_LATER_FLOOR:?} floor"
            );
            assert_eq!(stats.authenticated(), 0);

            drop(holder);
            until(
                || authenticated_and_refused(&stats),
                "the device came back and was bound",
            )
            .await;
        })
        .await;
}
