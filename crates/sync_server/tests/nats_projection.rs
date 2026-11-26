//! Consumers sharing a queue group apply each change exactly once. Needs Docker.
#![cfg(all(feature = "nats", not(target_arch = "wasm32")))]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use fundamental_db::Filter;
use fundamental_macros::Model;
use fundamental_nats::NatsClient;
use fundamental_sync::wire::SliceCursor;
use fundamental_sync::{
    ChangePublisher, NatsChangePublisher, SliceStore, SliceWriteStore, SyncChange, SyncError,
    SyncMeta, SyncRecord,
};
use fundamental_sync_server::{ProjectionRegistry, ProjectionUpdate, nats_projection_consumer};
use fundamental_test_containers::container_name::NATS_03_PROJECTION;
use fundamental_test_containers::nats::NatsServer;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "channel_summaries")]
struct ChannelSummary {
    #[model(id)]
    id: String,
    #[model(owner)]
    channel_id: String,
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "messages")]
struct Message {
    #[model(id)]
    id: String,
    #[model(owner)]
    channel_id: String,
}

/// A store recording every projection row written, shared by both consumers.
#[derive(Default)]
struct RecordingStore {
    writes: Mutex<Vec<String>>,
}

impl SliceStore for RecordingStore {
    async fn changed_page(
        &self,
        _table: &str,
        _data_columns: &[&str],
        _key_columns: &[&str],
        _filters: &[Filter],
        _since: i64,
        _after: Option<&SliceCursor>,
        _limit: u32,
    ) -> Result<Vec<SyncRecord>, SyncError> {
        Ok(Vec::new())
    }
}

impl SliceWriteStore for RecordingStore {
    async fn upsert(
        &self,
        _table: &str,
        _data_columns: &[&str],
        _key_columns: &[&str],
        record: &SyncRecord,
    ) -> Result<(), SyncError> {
        self.writes
            .lock()
            .unwrap()
            .push(record.id.as_single().unwrap_or_default().to_owned());
        Ok(())
    }
}

async fn client() -> NatsClient {
    NatsServer::shared(NATS_03_PROJECTION)
        .await
        .expect("a NATS server")
        .client()
        .await
        .expect("connect")
}

fn registry() -> Arc<ProjectionRegistry> {
    Arc::new(
        ProjectionRegistry::new().define::<ChannelSummary, Message, _, _>(
            "channel_summary",
            |_ctx, change| async move {
                Ok(vec![ProjectionUpdate::upsert(SyncRecord::new(
                    change.owner.clone(),
                    SyncMeta::new(0, 0),
                    BTreeMap::from([("channel_id".to_string(), change.owner)]),
                ))])
            },
        ),
    )
}

fn message_change(channel: &str) -> SyncChange {
    SyncChange::new(
        channel,
        "messages",
        SyncRecord::new(
            format!("m-{channel}"),
            SyncMeta::new(1, 1),
            BTreeMap::from([("channel_id".to_string(), channel.to_string())]),
        ),
        1,
    )
}

#[tokio::test]
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
async fn a_queue_group_recomputes_each_change_on_exactly_one_node() {
    let store = Arc::new(RecordingStore::default());
    let _node_a = nats_projection_consumer(&client().await, registry(), store.clone())
        .await
        .expect("consumer A subscribes");
    let _node_b = nats_projection_consumer(&client().await, registry(), store.clone())
        .await
        .expect("consumer B subscribes");

    let publisher = NatsChangePublisher::new(client().await);

    // Core NATS interest can lag fresh subscriptions, so a sentinel goes first.
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            publisher
                .publish(message_change("warmup"))
                .await
                .expect("publish warmup");
            tokio::time::sleep(Duration::from_millis(200)).await;
            let seen = store.writes.lock().unwrap().iter().any(|id| id == "warmup");
            if seen {
                break;
            }
        }
    })
    .await
    .expect("warmup change is delivered");

    let channels: Vec<String> = (0..20).map(|i| format!("c-{i}")).collect();
    for channel in &channels {
        publisher
            .publish(message_change(channel))
            .await
            .expect("publish change");
    }

    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let done = {
                let seen = store.writes.lock().unwrap();
                channels.iter().all(|c| seen.contains(c))
            };
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("every change is applied");

    let writes = store.writes.lock().unwrap();
    for channel in &channels {
        let applied = writes.iter().filter(|id| *id == channel).count();
        assert_eq!(applied, 1, "change {channel} was applied {applied} times");
    }
}
