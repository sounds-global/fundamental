//! The slice endpoint end to end: a real server, a real `WsRemoteSource`, per-principal scoping.

use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_auth::{JwtVerifier, UserId};
use fundamental_core::secrets::SecretString;
use fundamental_db::Filter;
use fundamental_keys::{DeviceId, DeviceSigner, MemorySeedStore, StoredSigner};
use fundamental_macros::Model;
use fundamental_sync::wire::{
    AuthRequest, SliceChange, SliceSubscribeAck, SliceSubscribeRequest, action,
};
use fundamental_sync::{
    BroadcastChangePublisher, ChangePublisher, MemorySliceStore, SyncChange, SyncMeta, SyncRecord,
};
use fundamental_sync_server::{
    EnrollmentRegistry, NoParams, RateLimiter, SliceRegistry, SliceServerSource,
    live_slice_sync_router, signed_slice_sync_router, slice_sync_router,
};
use fundamental_ws::{WsConfig, WsEnvelope, WsListener, WsServer};
use fundamental_ws_client::WsRemoteSource;
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// A raw client, to observe pushed frames the client library drops.
type RawWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

const SECRET: &[u8] = b"slice-e2e-secret";
const FUTURE: i64 = 4_102_444_800; // 2100-01-01

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, table = "animals")]
struct Animal {
    #[model(id)]
    id: String,
    name: String,
    owner_id: String,
}

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, bidirectional, table = "notes")]
struct Note {
    #[model(id)]
    id: String,
    body: String,
    owner_id: String,
}

fn verifier() -> JwtVerifier {
    JwtVerifier::new(
        DecodingKey::from_secret(SECRET),
        Validation::new(Algorithm::HS256),
    )
}

fn token_for(subject: &str) -> SecretString {
    let token = encode(
        &Header::new(Algorithm::HS256),
        &json!({ "sub": subject, "exp": FUTURE }),
        &EncodingKey::from_secret(SECRET),
    )
    .expect("encode");
    SecretString::new(token)
}

fn animal(id: &str, owner: &str, updated_at: i64) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("name".to_string(), id.to_string());
    payload.insert("owner_id".to_string(), owner.to_string());
    SyncRecord::new(id, SyncMeta::new(updated_at, 1), payload)
}

fn build_source() -> Arc<SliceServerSource<MemorySliceStore>> {
    let store = MemorySliceStore::new();
    store.put("animals", animal("cat", "alice", 10));
    store.put("animals", animal("dog", "alice", 30));
    store.put("animals", animal("rex", "bob", 20));

    let registry = SliceRegistry::new()
        .define::<Animal, NoParams, _, _>("mine", |ctx, _params| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        })
        .define::<Note, NoParams, _, _>("notes", |ctx, _params| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        });

    Arc::new(SliceServerSource::new(registry, store))
}

async fn serve() -> WsListener {
    let limiter = Arc::new(RateLimiter::new(1000, 1000.0, 65_536));
    let router = slice_sync_router(Arc::new(verifier()), build_source(), limiter);
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    WsServer::new(config, router).serve().await.expect("serve")
}

async fn signed_serve() -> (WsListener, Arc<EnrollmentRegistry>) {
    let registry = Arc::new(EnrollmentRegistry::new());
    let limiter = Arc::new(RateLimiter::new(1000, 1000.0, 65_536));
    let router = signed_slice_sync_router(
        Arc::new(verifier()),
        build_source(),
        limiter,
        registry.clone(),
    );
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    (listener, registry)
}

fn device_signer(id: &str) -> (DeviceId, StoredSigner) {
    let device_id = DeviceId::new(id);
    let signer =
        StoredSigner::load_or_create(device_id.clone(), &MemorySeedStore::new()).expect("key");
    (device_id, signer)
}

#[tokio::test]
async fn authenticate_then_pull_a_slice_scoped_to_the_principal() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");

    let mut rows = alice
        .pull_slice("mine", Value::Null, 0)
        .await
        .expect("slice pull");
    rows.sort_by_key(|r| r.meta.updated_at);
    let ids: Vec<String> = rows.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(
        ids,
        vec!["cat", "dog"],
        "alice sees only her animals, ordered"
    );

    let after = alice
        .pull_slice("mine", Value::Null, 10)
        .await
        .expect("slice pull");
    let ids: Vec<String> = after.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(ids, vec!["dog"]);

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn the_same_slice_is_isolated_per_principal() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let bob = WsRemoteSource::connect(&url).await.expect("connect");
    bob.authenticate(&token_for("bob"))
        .await
        .expect("authenticate");

    let rows = bob
        .pull_slice("mine", Value::Null, 0)
        .await
        .expect("slice pull");
    let ids: Vec<String> = rows.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(ids, vec!["rex"], "the `mine` slice resolves to bob's rows");

    listener.stop().await.expect("the server drains");
}

fn note(id: &str, owner: &str, body: &str, updated_at: i64) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("body".to_string(), body.to_string());
    payload.insert("owner_id".to_string(), owner.to_string());
    SyncRecord::new(id, SyncMeta::new(updated_at, 1), payload)
}

#[tokio::test]
async fn push_to_a_bidirectional_slice_round_trips() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");

    alice
        .push_slice("notes", Value::Null, &note("n1", "alice", "hello", 100))
        .await
        .expect("push to a bidirectional slice");
    let rows = alice
        .pull_slice("notes", Value::Null, 0)
        .await
        .expect("pull notes");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "n1");
    assert_eq!(rows[0].payload["body"], "hello");

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn push_to_a_pull_only_slice_is_refused() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");

    let err = alice
        .push_slice("mine", Value::Null, &animal("ghost", "alice", 1))
        .await
        .expect_err("pull-only slice refuses writes");
    assert!(matches!(err, fundamental_sync::SyncError::Push(_)));

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn push_of_an_out_of_scope_record_is_refused() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");

    let err = alice
        .push_slice("notes", Value::Null, &note("n2", "bob", "sneaky", 100))
        .await
        .expect_err("out-of-scope write refused");
    assert!(matches!(err, fundamental_sync::SyncError::Push(_)));

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn an_unknown_slice_is_rejected() {
    let listener = serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");

    let err = alice
        .pull_slice("ghost", Value::Null, 0)
        .await
        .expect_err("unknown slice rejected");
    assert!(matches!(err, fundamental_sync::SyncError::Pull(_)));

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn an_enrolled_device_signs_a_slice_push_that_is_accepted() {
    let (listener, registry) = signed_serve().await;
    let url = format!("ws://{}", listener.local_addr());
    let (device_id, signer) = device_signer("device-1");
    registry
        .enroll(
            UserId::new("alice"),
            device_id.clone(),
            signer.verifying_key(),
            0,
        )
        .await
        .expect("enroll");

    let alice = WsRemoteSource::connect_signed(&url, device_id, signer)
        .await
        .expect("connect signed");
    alice.authenticate(&token_for("alice")).await.expect("auth");

    alice
        .push_slice("notes", Value::Null, &note("n1", "alice", "signed", 100))
        .await
        .expect("signed slice push");
    let rows = alice
        .pull_slice("notes", Value::Null, 0)
        .await
        .expect("pull");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "n1");

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn a_slice_push_from_an_unenrolled_device_is_rejected() {
    let (listener, _registry) = signed_serve().await;
    let url = format!("ws://{}", listener.local_addr());
    let (device_id, signer) = device_signer("ghost");

    let alice = WsRemoteSource::connect_signed(&url, device_id, signer)
        .await
        .expect("connect signed");
    alice.authenticate(&token_for("alice")).await.expect("auth");

    let err = alice
        .push_slice("notes", Value::Null, &note("n1", "alice", "x", 100))
        .await
        .expect_err("unenrolled device rejected");
    assert!(matches!(err, fundamental_sync::SyncError::Push(_)));

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn an_unsigned_slice_push_is_rejected_by_the_signed_endpoint() {
    let (listener, _registry) = signed_serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice.authenticate(&token_for("alice")).await.expect("auth");

    let err = alice
        .push_slice("notes", Value::Null, &note("n1", "alice", "x", 100))
        .await
        .expect_err("unsigned push rejected");
    assert!(matches!(err, fundamental_sync::SyncError::Push(_)));

    listener.stop().await.expect("the server drains");
}

async fn send_env<T: Serialize>(ws: &mut RawWs, action: &str, dto: &T) {
    let env = WsEnvelope::new(action, serde_json::to_value(dto).expect("to_value"));
    let text = serde_json::to_string(&env).expect("to_string");
    ws.send(Message::text(text)).await.expect("send");
}

async fn recv_text(ws: &mut RawWs) -> String {
    // Bounded, so silence fails with a message instead of hanging.
    tokio::time::timeout(RECV_BUDGET, async {
        loop {
            match ws.next().await.expect("a frame").expect("an ok frame") {
                Message::Text(text) => return text.to_string(),
                Message::Close(_) => panic!("the connection closed unexpectedly"),
                _ => continue,
            }
        }
    })
    .await
    .expect(
        "a frame was expected and none arrived: the server stopped sending rather than \
             sending the wrong thing, which is a different defect from any this file asserts",
    )
}

/// How long any receive may wait; every awaited frame was already triggered.
const RECV_BUDGET: Duration = Duration::from_secs(30);

async fn recv_dto<T: DeserializeOwned>(ws: &mut RawWs) -> T {
    serde_json::from_str(&recv_text(ws).await).expect("decode a dto")
}

/// Serves the live endpoint, returning the listener and the publisher a CDC consumer would feed.
async fn live_serve() -> (WsListener, BroadcastChangePublisher) {
    let publisher = BroadcastChangePublisher::new(64);
    let limiter = Arc::new(RateLimiter::new(1000, 1000.0, 65_536));
    let router = live_slice_sync_router(Arc::new(verifier()), build_source(), limiter, &publisher);
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    (listener, publisher)
}

/// A subscribed connection receives every in-scope change and never another principal's.
#[tokio::test]
async fn subscribing_pushes_live_changes_scoped_to_the_principal() {
    let (listener, publisher) = live_serve().await;
    let url = format!("ws://{}", listener.local_addr());
    let (mut ws, _) = connect_async(&url).await.expect("connect");

    let token = token_for("alice");
    send_env(&mut ws, action::AUTH, &AuthRequest::new(1, token.expose())).await;
    let _auth_ack = recv_text(&mut ws).await;

    send_env(
        &mut ws,
        action::SLICE_SUBSCRIBE,
        &SliceSubscribeRequest::new(2, "mine", Value::Null),
    )
    .await;
    let ack: SliceSubscribeAck = recv_dto(&mut ws).await;
    assert_eq!(ack.request_id, 2);
    assert!(
        ack.subscribed,
        "the subscription is confirmed before changes flow"
    );

    publisher
        .publish(SyncChange::new(
            "alice",
            "animals",
            animal("cat", "alice", 50),
            50,
        ))
        .await
        .expect("publish alice change");
    let env: WsEnvelope = recv_dto(&mut ws).await;
    assert_eq!(env.action, action::SLICE_CHANGE);
    let change: SliceChange = serde_json::from_value(env.data).expect("slice change");
    assert_eq!(change.table, "animals");
    assert_eq!(change.record.id, "cat");

    // Bob's change, then alice's: if bob's leaked it would arrive first.
    publisher
        .publish(SyncChange::new(
            "bob",
            "animals",
            animal("rex", "bob", 60),
            60,
        ))
        .await
        .expect("publish bob change");
    publisher
        .publish(SyncChange::new(
            "alice",
            "animals",
            animal("dog", "alice", 70),
            70,
        ))
        .await
        .expect("publish alice's sentinel change");

    let env: WsEnvelope = recv_dto(&mut ws).await;
    assert_eq!(env.action, action::SLICE_CHANGE);
    let next: SliceChange = serde_json::from_value(env.data).expect("slice change");
    assert_eq!(
        next.record.id, "dog",
        "the next frame after bob's change must be alice's sentinel — receiving `rex` here is \
         the cross-principal leak"
    );

    listener.stop().await.expect("the server drains");
}

/// The device client applies pushed changes from its `live_changes` stream.
#[tokio::test]
async fn the_client_live_mode_receives_pushed_changes() {
    let (listener, publisher) = live_serve().await;
    let url = format!("ws://{}", listener.local_addr());

    let alice = WsRemoteSource::connect(&url).await.expect("connect");
    alice
        .authenticate(&token_for("alice"))
        .await
        .expect("authenticate");
    alice
        .subscribe_slice("mine", Value::Null)
        .await
        .expect("subscribe to mine");
    let mut changes = alice.live_changes().expect("the live-change stream");

    publisher
        .publish(SyncChange::new(
            "alice",
            "animals",
            animal("cat", "alice", 50),
            50,
        ))
        .await
        .expect("publish");
    let change = tokio::time::timeout(Duration::from_secs(5), changes.next())
        .await
        .expect("a change arrives in time")
        .expect("the stream stays open");
    assert_eq!(change.table, "animals");
    assert_eq!(change.record.id, "cat");
    let record = SyncRecord::try_from(change.record).expect("a valid record to apply");
    assert_eq!(record.payload["owner_id"], "alice");

    assert!(
        alice.live_changes().is_none(),
        "the live stream is taken once"
    );

    listener.stop().await.expect("the server drains");
}
