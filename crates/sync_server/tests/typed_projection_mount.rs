//! `#[projection]` and `mount_projections!` drive a recompute from a source change.

use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_macros::{Model, mount_projections, projection};
use fundamental_sync::change::{BroadcastChangePublisher, ChangePublisher};
use fundamental_sync::{MemorySliceStore, SliceStore, SyncChange, SyncMeta, SyncRecord};
use fundamental_sync_server::{ProjectionRegistry, run_projection_refresh};

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "channel_summaries")]
struct ChannelSummary {
    #[model(id)]
    id: String,
    #[model(owner)]
    channel_id: String,
    message_count: i64,
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "messages")]
struct Message {
    #[model(id)]
    id: String,
    #[model(owner)]
    channel_id: String,
    body: String,
}

#[projection(ChannelSummary, sources(Message), "channel_summary")]
async fn summarize_channel(
    ctx: ProjectionContext,
    change: SyncChange,
) -> Result<Vec<ProjectionUpdate>, ProjectionError> {
    let channel = change.owner.clone();
    let messages = ctx
        .read::<Message>(&[Filter::eq("channel_id", channel.as_str())])
        .await?;
    let mut payload = BTreeMap::new();
    payload.insert("channel_id".to_string(), channel.clone());
    payload.insert("message_count".to_string(), messages.len().to_string());
    Ok(vec![ProjectionUpdate::upsert(SyncRecord::new(
        channel,
        SyncMeta::new(0, 0),
        payload,
    ))])
}

mount_projections! { SummarizeChannel }

// The handler names `Filter` bare.
use fundamental_db::Filter;

fn message(id: &str, channel: &str) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("channel_id".to_string(), channel.to_string());
    payload.insert("body".to_string(), format!("hello from {id}"));
    SyncRecord::new(id, SyncMeta::new(1000, 1), payload)
}

fn message_change(channel: &str, id: &str) -> SyncChange {
    SyncChange::new(channel, "messages", message(id, channel), 1000)
}

#[tokio::test]
async fn a_source_change_recomputes_the_projection_through_the_mounted_registry() {
    let store = Arc::new(MemorySliceStore::new());
    store.put("messages", message("m-1", "c-1"));
    store.put("messages", message("m-2", "c-1"));
    store.put("messages", message("m-3", "c-2"));

    let registry = Arc::new(register_projections(ProjectionRegistry::new()));
    let publisher = BroadcastChangePublisher::new(32);
    let handle = tokio::spawn(run_projection_refresh(
        publisher.subscribe(),
        registry,
        store.clone(),
    ));

    publisher
        .publish(message_change("c-1", "m-1"))
        .await
        .expect("publish");
    drop(publisher);
    handle.await.expect("refresh loop joins");

    // From `-1`: the in-memory store keeps the handler's placeholder `updated_at` of `0`.
    let summary = store
        .changed_since(
            "channel_summaries",
            &["channel_id", "message_count"],
            &["id"],
            &[Filter::eq("channel_id", "c-1")],
            -1,
        )
        .await
        .expect("read summary");
    assert_eq!(summary.len(), 1, "one summary row for c-1");
    assert_eq!(summary[0].id, "c-1");
    assert_eq!(
        summary[0].payload.get("message_count").map(String::as_str),
        Some("2"),
        "the summary counts both messages in the channel"
    );
}
