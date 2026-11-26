//! The statuses a `SyncClient` reports as a hub lets a session go live, stops answering and
//! takes the redial; `Stopped` only when the app ends it.

#![cfg(not(target_arch = "wasm32"))]

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_macros::subscribe;
use fundamental_sync::wire::{SlicePullRequest, SlicePullResponse};
use fundamental_ws::{WsContext, WsResult};
use fundamental_ws_client::{ConnectionStatus, SyncClient, SyncClientConfig};
use support::{AUTHENTICATED, Note, hub_router, memory_db, notes_binding, serve};
use tokio::task::LocalSet;

/// Pulls the hub has received.
static PULLS: AtomicUsize = AtomicUsize::new(0);

/// Answers the first pull, then goes silent, so the session's probe times out.
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

/// Reduces a status to its shape, so the sequence can be compared whole.
fn shape(status: ConnectionStatus) -> String {
    match status {
        ConnectionStatus::Connecting => "connecting".into(),
        ConnectionStatus::Live => "live".into(),
        ConnectionStatus::Waiting { cause, .. } => format!("waiting:{cause:?}"),
        ConnectionStatus::Stopped => "stopped".into(),
    }
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn the_status_follows_the_session_through_a_redial_and_stops_only_on_shutdown() {
    let (_listener, url) = serve(hub_router().route(on_pull_route())).await;
    let db = memory_db().await;
    let config = SyncClientConfig::new(url, SecretString::new("token".into()))
        .with_reconcile_interval(None)
        .with_liveness_interval(Duration::from_millis(100))
        .with_request_timeout(Duration::from_millis(200));

    LocalSet::new()
        .run_until(async move {
            let client = SyncClient::start::<Note>(config, notes_binding(), db);
            let mut watch = client.status();
            let mut seen = vec![shape(watch.current())];
            let mut lives = 0;
            let observed = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(status) = watch.changed().await {
                    seen.push(shape(status));
                    if status == ConnectionStatus::Live {
                        lives += 1;
                        if lives == 2 {
                            break;
                        }
                    }
                }
            })
            .await;
            assert!(
                observed.is_ok(),
                "the session never went live twice: {seen:?}"
            );
            // A lost-session wait that drew 0 ms is followed by `Connecting` before this reader runs;
            // the loop tests in `sync_client.rs` prove the `Waiting` step.
            let redial: Vec<&str> = seen.iter().map(String::as_str).collect();
            assert!(
                redial
                    == [
                        "connecting",
                        "live",
                        "waiting:TimedOut",
                        "connecting",
                        "live"
                    ]
                    || redial == ["connecting", "live", "connecting", "live"],
                "an unanswered probe waits as TimedOut and redials: {redial:?}"
            );

            let mut after = client.status();
            client.shutdown();
            assert_eq!(after.current(), ConnectionStatus::Stopped);
            assert_eq!(after.changed().await, Some(ConnectionStatus::Stopped));
            let closed = tokio::time::timeout(Duration::from_secs(1), after.changed()).await;
            assert_eq!(closed, Ok(None), "the session is gone once it has stopped");
        })
        .await;
    assert!(AUTHENTICATED.load(Ordering::SeqCst) >= 2);
}
