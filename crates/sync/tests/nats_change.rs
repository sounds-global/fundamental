//! Change fan-out across nodes over a real NATS server. Requires Docker.
#![cfg(all(feature = "nats", not(target_arch = "wasm32")))]

use std::collections::BTreeMap;
use std::time::Duration;

use fundamental_nats::NatsClient;
use fundamental_sync::{
    BroadcastChangePublisher, ChangePublisher, NatsChangePublisher, SyncChange, SyncMeta,
    SyncRecord, nats_change_bridge,
};
use fundamental_test_containers::container_name::NATS_02_SYNC;
use fundamental_test_containers::nats::NatsServer;

async fn client() -> NatsClient {
    NatsServer::shared(NATS_02_SYNC)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

fn change(owner: &str, id: &str) -> SyncChange {
    let mut payload = BTreeMap::new();
    payload.insert("owner_id".to_string(), owner.to_string());
    SyncChange::new(
        owner,
        "notes",
        SyncRecord::new(id, SyncMeta::new(10, 1), payload),
        10,
    )
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_change_published_over_nats_reaches_a_node_via_the_bridge() {
    let publisher = NatsChangePublisher::new(client().await);

    let local = BroadcastChangePublisher::new(64);
    let mut stream = local.subscribe();
    let _bridge = nats_change_bridge(&client().await, local.clone())
        .await
        .expect("start the bridge");

    // Retried: core NATS interest can lag a fresh subscription across connections.
    let received = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            publisher
                .publish(change("alice", "n1"))
                .await
                .expect("publish");
            if let Ok(Ok(change)) =
                tokio::time::timeout(Duration::from_millis(300), stream.recv()).await
            {
                return change;
            }
        }
    })
    .await
    .expect("a change crosses NATS and the bridge in time");

    assert_eq!(received.owner, "alice");
    assert_eq!(received.table, "notes");
    assert_eq!(received.record.id, "n1");
}
