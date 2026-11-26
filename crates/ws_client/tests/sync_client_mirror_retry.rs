//! A device whose database cannot create the mirror table waits and retries, then goes live.

#![cfg(not(target_arch = "wasm32"))]

mod support;

use std::rc::Rc;
use std::time::Duration;

use fundamental_core::secrets::SecretString;
use fundamental_macros::subscribe;
use fundamental_sync::Syncable;
use fundamental_sync::wire::{SlicePullRequest, SlicePullResponse};
use fundamental_ws::{WsContext, WsResult};
use fundamental_ws_client::{ConnectionStatus, SyncClient, SyncClientConfig, WaitCause};
use support::{Note, hub_router, memory_db, notes_binding, serve};
use tokio::task::LocalSet;

#[subscribe(socket = "slice.pull")]
async fn on_pull(context: WsContext, request: SlicePullRequest) -> WsResult<()> {
    context
        .send(SlicePullResponse::last(request.request_id, Vec::new()))
        .await
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_mirror_table_that_cannot_be_created_waits_and_is_retried() {
    let (_listener, url) = serve(hub_router().route(on_pull_route())).await;
    let db = memory_db().await;
    // SQLite refuses `CREATE TABLE IF NOT EXISTS` for a name an index already holds.
    let occupied = format!(
        "CREATE TABLE occupant (id TEXT); CREATE INDEX {} ON occupant (id);",
        Note::MIRROR_TABLE
    );
    db.execute_batch(&occupied)
        .unwrap_or_else(|error| panic!("occupy the mirror table's name: {error}"));
    let config = SyncClientConfig::new(url, SecretString::new("token".into()));

    LocalSet::new()
        .run_until(async move {
            let client = SyncClient::start::<Note>(config, notes_binding(), Rc::clone(&db));
            let mut watch = client.status();
            let waiting = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(status) = watch.changed().await {
                    if let ConnectionStatus::Waiting { cause, .. } = status {
                        return Some(cause);
                    }
                }
                None
            })
            .await;
            assert_eq!(waiting, Ok(Some(WaitCause::Local)));

            db.execute_batch(&format!("DROP INDEX {};", Note::MIRROR_TABLE))
                .unwrap_or_else(|error| panic!("free the name: {error}"));
            let live = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(status) = watch.changed().await {
                    if status == ConnectionStatus::Live {
                        return true;
                    }
                }
                false
            })
            .await;
            assert_eq!(
                live,
                Ok(true),
                "the retried session creates the table and goes live"
            );
            client.shutdown();
        })
        .await;
}
