//! End-to-end tests of `WsRemoteSource` against a real `fundamental_ws` server.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;

use fundamental_macros::subscribe;
use fundamental_sync::wire::{
    SliceCursor, SlicePullRequest, SlicePullResponse, SliceUpsertRequest, UpsertAck, WireRecord,
};
use fundamental_sync::{SyncError, SyncMeta, SyncRecord};
use fundamental_ws::{WsConfig, WsContext, WsResult, WsRouter, WsServer};
use fundamental_ws_client::{TransportError, WsRemoteSource};
use serde_json::Value;

fn row(id: &str, updated_at: i64) -> WireRecord {
    WireRecord::from(&SyncRecord::new(
        id.to_string(),
        SyncMeta::new(updated_at, 1),
        BTreeMap::new(),
    ))
}

// Two-page slice pull: the first page continues at a keyset cursor.
#[subscribe(socket = "slice.pull")]
async fn on_slice_pull(context: WsContext, request: SlicePullRequest) -> WsResult<()> {
    let response = match request.cursor {
        None => SlicePullResponse::page(
            request.request_id,
            vec![row("a", 10), row("b", 20)],
            SliceCursor::new(20, "b"),
        ),
        Some(_) => SlicePullResponse::last(request.request_id, vec![row("c", 30)]),
    };
    context.send(response).await
}

#[subscribe(socket = "slice.upsert")]
async fn on_slice_upsert(context: WsContext, request: SliceUpsertRequest) -> WsResult<()> {
    context.send(UpsertAck::applied(request.request_id)).await
}

async fn serve(router: WsRouter) -> fundamental_ws::WsListener {
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    WsServer::new(config, router).serve().await.expect("serve")
}

#[tokio::test]
async fn pull_follows_the_cursor_across_pages_over_a_real_socket() {
    let listener = serve(WsRouter::new().route(on_slice_pull_route())).await;
    let url = format!("ws://{}", listener.local_addr());

    let client = WsRemoteSource::connect(&url).await.expect("connect");
    let records = client
        .pull_slice("mine", Value::Null, 0)
        .await
        .expect("pull");

    assert_eq!(
        records.iter().map(|r| r.id.to_string()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn upsert_is_acknowledged() {
    let listener = serve(WsRouter::new().route(on_slice_upsert_route())).await;
    let url = format!("ws://{}", listener.local_addr());

    let client = WsRemoteSource::connect(&url).await.expect("connect");
    let record = SyncRecord::new("row-1", SyncMeta::new(5, 1), BTreeMap::new());
    client
        .push_slice("mine", Value::Null, &record)
        .await
        .expect("upsert");
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn concurrent_requests_correlate_to_the_right_caller() {
    let listener = serve(WsRouter::new().route(on_slice_upsert_route())).await;
    let url = format!("ws://{}", listener.local_addr());

    let client = WsRemoteSource::connect(&url).await.expect("connect");
    let a = SyncRecord::new("a", SyncMeta::new(1, 1), BTreeMap::new());
    let b = SyncRecord::new("b", SyncMeta::new(2, 1), BTreeMap::new());
    // Both in flight on one socket at once; each must get its own ack.
    let (ra, rb) = tokio::join!(
        client.push_slice("mine", Value::Null, &a),
        client.push_slice("mine", Value::Null, &b)
    );
    ra.expect("upsert a");
    rb.expect("upsert b");
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn an_unknown_action_surfaces_as_a_push_error() {
    // An upsert draws a framework error frame, which must fail the push rather than hang.
    let listener = serve(WsRouter::new().route(on_slice_pull_route())).await;
    let url = format!("ws://{}", listener.local_addr());

    let client = WsRemoteSource::connect(&url).await.expect("connect");
    let record = SyncRecord::new("row-1", SyncMeta::new(5, 1), BTreeMap::new());
    let err = client
        .push_slice("mine", Value::Null, &record)
        .await
        .expect_err("must fail");
    assert!(matches!(err, SyncError::Push(_)));
    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn connecting_to_a_closed_port_fails_with_connect() {
    let err = WsRemoteSource::connect("ws://127.0.0.1:1")
        .await
        .expect_err("must fail to connect");
    assert!(matches!(err, TransportError::Connect(_)));
}
