//! A flood of vault challenges holds one challenge, and a closed connection holds none.

mod support;

use std::sync::Arc;
use std::time::Duration;

use fundamental_core::secrets::Secret;
use fundamental_keys::VaultOwnerKey;
use fundamental_sync::MemorySliceStore;
use fundamental_sync::wire::{PROTOCOL_VERSION, action};
use fundamental_sync_server::{
    RateLimiter, SliceRegistry, SliceServerSource, VaultChallengeGauge, vault_slice_sync_router,
};
use fundamental_ws::WsListener;
use serde_json::{Value, json};
use support::{RawWs, call, serve};
use tokio_tungstenite::connect_async;

const FLOOD: u64 = 10_000;

async fn hub(gauge: Arc<VaultChallengeGauge>) -> WsListener {
    let source = Arc::new(SliceServerSource::new(
        SliceRegistry::new(),
        MemorySliceStore::new(),
    ));
    serve(vault_slice_sync_router(
        gauge,
        source,
        Arc::new(RateLimiter::new(1000, 1000.0, 65_536)),
        65_536,
    ))
    .await
}

async fn connect(listener: &WsListener) -> RawWs {
    connect_async(format!("ws://{}", listener.local_addr()))
        .await
        .expect("connect")
        .0
}

async fn challenge(ws: &mut RawWs, request_id: u64) -> Vec<u8> {
    let reply = call(
        ws,
        action::VAULT_CHALLENGE,
        json!({ "version": PROTOCOL_VERSION, "request_id": request_id }),
    )
    .await;
    serde_json::from_value(reply["nonce"].clone()).expect("a nonce")
}

async fn answer(ws: &mut RawWs, key: &VaultOwnerKey, nonce: &[u8]) -> Value {
    let signed: [u8; 32] = nonce.try_into().expect("a 32-byte nonce");
    call(
        ws,
        action::VAULT_AUTH,
        json!({
            "version": PROTOCOL_VERSION,
            "request_id": 0,
            "owner_public_key": key.public_key().to_string(),
            "nonce": nonce,
            "signature": key.sign_challenge(&signed).to_string(),
        }),
    )
    .await
}

/// Waits for a closed connection's task to drop what it held.
async fn until_nothing_pending(gauge: &VaultChallengeGauge) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while gauge.pending() != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the closed connection's challenge is dropped within ten seconds");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vault_challenge_flood_holds_one_challenge() {
    let gauge = Arc::new(VaultChallengeGauge::new());
    let listener = hub(gauge.clone()).await;
    let mut ws = connect(&listener).await;

    let first = challenge(&mut ws, 0).await;
    for request_id in 1..FLOOD {
        challenge(&mut ws, request_id).await;
    }
    assert_eq!(gauge.pending(), 1, "one connection holds one challenge");
    assert_eq!(gauge.replaced(), FLOOD - 1);

    let key = VaultOwnerKey::from_child_seed(&Secret::new([0x42; 32]));
    let stale = answer(&mut ws, &key, &first).await;
    assert_eq!(stale["error"]["code"], "VAULT_003", "{stale}");
    assert_eq!(gauge.pending(), 0, "the answer spent the pending challenge");

    let current = challenge(&mut ws, FLOOD).await;
    let accepted = answer(&mut ws, &key, &current).await;
    assert_eq!(accepted["ok"], true, "{accepted}");

    let reissued = challenge(&mut ws, FLOOD + 1).await;
    assert_eq!(reissued.len(), 32);
    assert_eq!(gauge.pending(), 1);
    ws.close(None).await.expect("close");
    until_nothing_pending(&gauge).await;

    listener.stop().await.expect("the server drains");
}

#[tokio::test(flavor = "multi_thread")]
async fn many_connections_hold_one_challenge_each_and_none_once_closed() {
    let gauge = Arc::new(VaultChallengeGauge::new());
    let listener = hub(gauge.clone()).await;
    let mut connections = Vec::new();
    for _ in 0..8 {
        let mut ws = connect(&listener).await;
        for request_id in 0..16 {
            challenge(&mut ws, request_id).await;
        }
        connections.push(ws);
    }
    assert_eq!(gauge.pending(), 8);

    for mut ws in connections {
        ws.close(None).await.expect("close");
    }
    until_nothing_pending(&gauge).await;

    listener.stop().await.expect("the server drains");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_answers_wrongly_three_times_gets_no_fourth_challenge() {
    let gauge = Arc::new(VaultChallengeGauge::new());
    let listener = hub(gauge.clone()).await;
    let mut ws = connect(&listener).await;
    let key = VaultOwnerKey::from_child_seed(&Secret::new([0x43; 32]));

    for request_id in 0..3 {
        challenge(&mut ws, request_id).await;
        let wrong = answer(&mut ws, &key, &[0u8; 32]).await;
        assert_eq!(wrong["error"]["code"], "VAULT_003", "{wrong}");
    }
    let refused = call(
        &mut ws,
        action::VAULT_CHALLENGE,
        json!({ "version": PROTOCOL_VERSION, "request_id": 3 }),
    )
    .await;
    assert_eq!(refused["error"]["code"], "VAULT_003", "{refused}");
    assert_eq!(gauge.pending(), 0);

    listener.stop().await.expect("the server drains");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_on_a_connection_that_asked_for_nothing_is_refused() {
    let gauge = Arc::new(VaultChallengeGauge::new());
    let listener = hub(gauge.clone()).await;
    let mut asker = connect(&listener).await;
    let mut other = connect(&listener).await;
    let key = VaultOwnerKey::from_child_seed(&Secret::new([0x44; 32]));

    let nonce = challenge(&mut asker, 0).await;
    let refused = answer(&mut other, &key, &nonce).await;
    assert_eq!(refused["error"]["code"], "VAULT_003", "{refused}");
    let accepted = answer(&mut asker, &key, &nonce).await;
    assert_eq!(accepted["ok"], true, "{accepted}");

    listener.stop().await.expect("the server drains");
}
