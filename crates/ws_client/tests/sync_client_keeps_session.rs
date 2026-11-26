//! A hub that answers every probe with an error keeps the `SyncClient` session up.

#![cfg(not(target_arch = "wasm32"))]

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_macros::subscribe;
use fundamental_sync::wire::{ErrorReply, SlicePullRequest, SlicePullResponse};
use fundamental_ws::{WsContext, WsResult};
use fundamental_ws_client::{SyncClient, SyncClientConfig};
use support::{AUTHENTICATED, Note, hub_router, memory_db, notes_binding, serve};
use tokio::task::LocalSet;

/// Pulls the hub has refused.
static REFUSED: AtomicUsize = AtomicUsize::new(0);
/// Pulls the hub has received.
static PULLS: AtomicUsize = AtomicUsize::new(0);

/// Answers the first pull, then refuses every one after it with an error reply.
#[subscribe(socket = "slice.pull")]
async fn on_pull(context: WsContext, request: SlicePullRequest) -> WsResult<()> {
    if PULLS.fetch_add(1, Ordering::SeqCst) == 0 {
        return context
            .send(SlicePullResponse::last(request.request_id, Vec::new()))
            .await;
    }
    REFUSED.fetch_add(1, Ordering::SeqCst);
    context
        .send(ErrorReply::new(
            request.request_id,
            "TEST_001",
            "pull refused",
        ))
        .await
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_hub_that_answers_with_an_error_keeps_the_session() {
    let (_listener, url) = serve(hub_router().route(on_pull_route())).await;
    let db = memory_db().await;
    let config = SyncClientConfig::new(url, SecretString::new("token".into()))
        .with_reconcile_interval(Some(Duration::from_millis(100)))
        .with_request_timeout(Duration::from_millis(200));

    LocalSet::new()
        .run_until(async move {
            let _client = SyncClient::start::<Note>(config, notes_binding(), db);
            let refused_thrice = tokio::time::timeout(Duration::from_secs(5), async {
                while REFUSED.load(Ordering::SeqCst) < 3 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await;
            assert!(refused_thrice.is_ok(), "the probe kept running");
        })
        .await;
    assert_eq!(
        AUTHENTICATED.load(Ordering::SeqCst),
        1,
        "an error reply is an answer: the session stays on its one connection"
    );
}
