//! On the account-less vault hub, a slot is earned by a stored vault, never by a key.

mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use fundamental_auth::JwtVerifier;
use fundamental_core::secrets::Secret;
use fundamental_db::Filter;
use fundamental_keys::VaultOwnerKey;
use fundamental_macros::Model;
use fundamental_sync::wire::{
    PROTOCOL_VERSION, SlicePullRequest, SliceUpsertRequest, WireRecord, action,
};
use fundamental_sync::{MemorySliceStore, SyncMeta, SyncRecord};
use fundamental_sync_server::{
    NoParams, RateLimiter, SliceRegistry, SliceServerSource, VaultChallengeGauge,
    slice_sync_router, vault_slice_sync_router,
};
use fundamental_ws::test_support::{close_frame, connect};
use fundamental_ws::{AnonymousLimits, WsConfig, WsListener, WsRouter, WsStats};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde_json::{Value, json};
use support::{RawWs, call};

const WITHIN: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, bidirectional, table = "blobs")]
struct Blob {
    #[model(id)]
    id: String,
    ciphertext: String,
    owner_id: String,
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, bidirectional, table = "keys")]
struct Key {
    #[model(id)]
    id: String,
    ciphertext: String,
    owner_id: String,
}

fn registry() -> SliceRegistry {
    SliceRegistry::new()
        .define::<Blob, NoParams, _, _>("mine", |ctx, _params| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        })
        .define::<Key, NoParams, _, _>("keys", |ctx, _params| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        })
}

fn key(seed: u8) -> VaultOwnerKey {
    VaultOwnerKey::from_child_seed(&Secret::new([seed; 32]))
}

fn blob(owner: &VaultOwnerKey, id: &str, updated_at: i64) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("owner_id".to_string(), owner.owner_id().to_string());
    payload.insert("ciphertext".to_string(), "sealed".to_string());
    SyncRecord::new(id, SyncMeta::new(updated_at, 1), payload)
}

async fn serve(router: WsRouter) -> (WsListener, Arc<WsStats>) {
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr")).authenticate(
        AnonymousLimits::default()
            .within(WITHIN)
            .displace_after(WITHIN / 2),
    );
    fundamental_ws::test_support::serve(config, router).await
}

async fn vault_hub(store: MemorySliceStore) -> (WsListener, Arc<WsStats>) {
    serve(vault_slice_sync_router(
        Arc::new(VaultChallengeGauge::new()),
        Arc::new(SliceServerSource::new(registry(), store)),
        Arc::new(RateLimiter::new(1000, 1000.0, 65_536)),
        65_536,
    ))
    .await
}

async fn prove(ws: &mut RawWs, owner: &VaultOwnerKey) {
    let challenge = call(
        ws,
        action::VAULT_CHALLENGE,
        json!({ "version": PROTOCOL_VERSION, "request_id": 1 }),
    )
    .await;
    let nonce: Vec<u8> = serde_json::from_value(challenge["nonce"].clone()).expect("a nonce");
    let signed: [u8; 32] = nonce.as_slice().try_into().expect("32 bytes");
    let answer = call(
        ws,
        action::VAULT_AUTH,
        json!({
            "version": PROTOCOL_VERSION,
            "request_id": 2,
            "owner_public_key": owner.public_key().to_string(),
            "nonce": nonce,
            "signature": owner.sign_challenge(&signed).to_string(),
        }),
    )
    .await;
    assert_eq!(answer["ok"], true, "{answer}");
}

async fn pull(ws: &mut RawWs, since: i64) -> Value {
    pull_slice(ws, "mine", since).await
}

async fn pull_slice(ws: &mut RawWs, slice: &str, since: i64) -> Value {
    let request = SlicePullRequest::new(3, slice, json!({}), since);
    call(
        ws,
        action::SLICE_PULL,
        serde_json::to_value(request).expect("json"),
    )
    .await
}

async fn upsert(ws: &mut RawWs, record: &SyncRecord) -> Value {
    let request = SliceUpsertRequest::new(4, "mine", json!({}), WireRecord::from(record));
    call(
        ws,
        action::SLICE_UPSERT,
        serde_json::to_value(request).expect("json"),
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vault_key_alone_earns_no_slot() {
    let (listener, stats) = vault_hub(MemorySliceStore::new()).await;
    let mut ws = connect(&listener).await;
    prove(&mut ws, &key(0x11)).await;
    let pulled = pull(&mut ws, 0).await;
    assert_eq!(pulled["records"], json!([]), "{pulled}");
    assert_eq!(
        stats.anonymous(),
        1,
        "a proven key with nothing stored stays anonymous"
    );
    assert_eq!(stats.authenticated(), 0);
    assert_eq!(close_frame(&mut ws).await.0, 4408);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_vault_owner_is_bound_by_its_first_write() {
    let (listener, stats) = vault_hub(MemorySliceStore::new()).await;
    let owner = key(0x22);
    let mut ws = connect(&listener).await;
    prove(&mut ws, &owner).await;
    let ack = upsert(&mut ws, &blob(&owner, "first", 10)).await;
    assert_eq!(ack["ok"], true, "{ack}");
    assert_eq!(stats.authenticated(), 1);
    assert_eq!(stats.anonymous(), 0);

    tokio::time::sleep(WITHIN * 2).await;
    let pulled = pull(&mut ws, 0).await;
    assert_eq!(
        pulled["records"].as_array().map(Vec::len),
        Some(1),
        "{pulled}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_known_vault_owner_is_bound_at_its_first_pull() {
    let owner = key(0x33);
    let store = MemorySliceStore::new();
    store.put("blobs", blob(&owner, "kept", 10));
    let (listener, stats) = vault_hub(store).await;
    let mut ws = connect(&listener).await;
    prove(&mut ws, &owner).await;
    // A pull from the high-water mark still admits: the probe reads from the start.
    let pulled = pull(&mut ws, 1_000).await;
    assert_eq!(pulled["records"], json!([]), "{pulled}");
    assert_eq!(stats.authenticated(), 1);

    tokio::time::sleep(WITHIN * 2).await;
    let again = pull(&mut ws, 0).await;
    assert_eq!(
        again["records"].as_array().map(Vec::len),
        Some(1),
        "{again}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_owner_stored_in_another_slice_is_bound_when_it_pulls_that_one() {
    let owner = key(0x44);
    let store = MemorySliceStore::new();
    store.put("keys", blob(&owner, "k1", 10));
    let (listener, stats) = vault_hub(store).await;
    let mut ws = connect(&listener).await;
    prove(&mut ws, &owner).await;
    let empty = pull_slice(&mut ws, "mine", 0).await;
    assert_eq!(empty["records"], json!([]), "{empty}");
    assert_eq!(stats.authenticated(), 0, "nothing stored in `mine`");
    let kept = pull_slice(&mut ws, "keys", 0).await;
    assert_eq!(kept["records"].as_array().map(Vec::len), Some(1), "{kept}");
    assert_eq!(
        stats.authenticated(),
        1,
        "`keys` holds its row, so it is admitted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unbound_owner_cannot_turn_its_deadline_into_a_stream_of_reads() {
    let (listener, stats) = vault_hub(MemorySliceStore::new()).await;
    let mut ws = connect(&listener).await;
    prove(&mut ws, &key(0x55)).await;
    for _ in 0..4 {
        let pulled = pull(&mut ws, 0).await;
        assert_eq!(pulled["records"], json!([]), "{pulled}");
    }
    let refused = pull(&mut ws, 0).await;
    assert_eq!(refused["error"]["code"], "WS_014", "{refused}");
    assert_eq!(stats.authenticated(), 0);
}

const SECRET: &[u8] = b"a-bound-connection-secret";

fn token(subject: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &json!({ "sub": subject, "exp": 4_102_444_800_i64 }),
        &EncodingKey::from_secret(SECRET),
    )
    .expect("encode")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bound_connection_cannot_buy_verifications() {
    let verifier = JwtVerifier::new(
        DecodingKey::from_secret(SECRET),
        Validation::new(Algorithm::HS256),
    );
    let listener = support::serve(slice_sync_router(
        Arc::new(verifier),
        Arc::new(SliceServerSource::new(registry(), MemorySliceStore::new())),
        Arc::new(RateLimiter::new(1000, 1000.0, 65_536)),
    ))
    .await;
    let stats = listener.stats();
    let mut ws = connect(&listener).await;
    let auth = |request_id: u64, token: String| json!({ "version": PROTOCOL_VERSION, "request_id": request_id, "token": token });
    let accepted = call(&mut ws, action::AUTH, auth(1, token("alice"))).await;
    assert_eq!(accepted["ok"], true, "{accepted}");
    assert_eq!(stats.authenticated(), 1);

    let again = call(&mut ws, action::AUTH, auth(2, "not a token".to_string())).await;
    assert_eq!(
        again["error"]["code"], "WS_009",
        "refused as already authenticated, before the token was verified: {again}"
    );
}
