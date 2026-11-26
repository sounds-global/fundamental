//! A hub that stops answering ends the `SyncClient` session and is redialled, even with no
//! reconcile interval set.

#![cfg(not(target_arch = "wasm32"))]

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_macros::subscribe;
use fundamental_sync::wire::{SlicePullRequest, SlicePullResponse};
use fundamental_ws::{WsContext, WsResult};
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use support::{AUTHENTICATED, Note, hub_router, memory_db, notes_binding, serve};
use tokio::task::LocalSet;

/// Pulls the hub has received.
static PULLS: AtomicUsize = AtomicUsize::new(0);

/// Answers the first pull, then never again while keeping the socket open: a half-open path.
#[subscribe(socket = "slice.pull")]
async fn on_pull(context: WsContext, request: SlicePullRequest) -> WsResult<()> {
    if PULLS.fetch_add(1, Ordering::SeqCst) == 0 {
        context
            .send(SlicePullResponse::last(request.request_id, Vec::new()))
            .await
    } else {
        Ok(())
    }
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_hub_that_stops_answering_is_redialled_without_a_reconcile_interval() {
    let (_listener, url) = serve(hub_router().route(on_pull_route())).await;
    let db = memory_db().await;
    let config = SyncClientConfig::new(url, SecretString::new("token".into()))
        .with_reconcile_interval(None)
        .with_liveness_interval(Duration::from_millis(100))
        .with_request_timeout(Duration::from_millis(200));

    LocalSet::new()
        .run_until(async move {
            let _client = SyncClient::start::<Note>(config, notes_binding(), db);
            let redialled = tokio::time::timeout(Duration::from_secs(5), async {
                while AUTHENTICATED.load(Ordering::SeqCst) < 2 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            assert!(
                redialled.is_ok(),
                "a session whose probe goes unanswered must reconnect; hub authenticated {} time(s)",
                AUTHENTICATED.load(Ordering::SeqCst)
            );
        })
        .await;
    assert!(
        PULLS.load(Ordering::SeqCst) >= 2,
        "the unanswered probe was the trigger"
    );
}
