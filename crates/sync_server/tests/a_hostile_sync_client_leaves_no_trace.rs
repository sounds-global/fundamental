//! A sync client's frames leave nothing of theirs in the logs.
//!
//! One test in its own binary, because it installs the process's global subscriber.

use std::collections::BTreeMap;
use std::sync::Arc;

mod support;

use fundamental_auth::JwtVerifier;
use fundamental_db::Filter;
use fundamental_keys::{DeviceKeyFingerprint, SigningKey};
use fundamental_macros::Model;
use fundamental_observability::testing::CapturedLogs;
use fundamental_sync::wire::{PROTOCOL_VERSION, WireRecord, action, canonical_bytes};
use fundamental_sync::{BroadcastChangePublisher, MemorySliceStore, SyncMeta, SyncRecord};
use fundamental_sync_server::{
    EnrollmentRegistry, NoParams, RateLimiter, SliceRegistry, SliceServerSource,
    VaultChallengeGauge, live_slice_sync_router, signed_slice_sync_router, vault_slice_sync_router,
};
use fundamental_ws::WsListener;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, encode};
use serde_json::{Value, json};
use support::{RawWs, call, serve};
use tokio_tungstenite::connect_async;

const EMAIL: &str = "jan@example.nl";
const OTHER_EMAIL: &str = "piet@example.nl";
const PHONE: u64 = 31_612_345_678;
const SECRET: &[u8] = b"sync-invariant-secret";
const FUTURE: i64 = 4_102_444_800;

#[derive(Clone, Debug, Model)]
#[model(offline, syncable, bidirectional, table = "notes")]
struct Note {
    #[model(id)]
    id: String,
    body: String,
    owner_id: String,
}

fn source() -> Arc<SliceServerSource<MemorySliceStore>> {
    let registry = SliceRegistry::new()
        .define::<Note, NoParams, _, _>("notes", |ctx, _params| async move {
            Ok(Filter::eq("owner_id", ctx.user_id().as_str()))
        });
    Arc::new(SliceServerSource::new(registry, MemorySliceStore::new()))
}

fn verifier() -> Arc<JwtVerifier> {
    Arc::new(JwtVerifier::new(
        DecodingKey::from_secret(SECRET),
        Validation::new(Algorithm::HS256),
    ))
}

fn token(subject: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &json!({ "sub": subject, "exp": FUTURE }),
        &EncodingKey::from_secret(SECRET),
    )
    .expect("encodes")
}

async fn connect(listener: &WsListener, user: &str) -> RawWs {
    let (mut ws, _) = connect_async(format!("ws://{}", listener.local_addr()))
        .await
        .expect("connect");
    let reply = call(
        &mut ws,
        action::AUTH,
        json!({ "version": PROTOCOL_VERSION, "request_id": PHONE, "token": token(user) }),
    )
    .await;
    assert!(reply.get("error").is_none(), "authenticated: {reply}");
    ws
}

fn note(id: &str, updated_at: i64) -> SyncRecord {
    let mut payload = BTreeMap::new();
    payload.insert("body".to_owned(), EMAIL.to_owned());
    payload.insert("owner_id".to_owned(), "alice".to_owned());
    SyncRecord::new(id, SyncMeta::new(updated_at, 1), payload)
}

fn upsert(slice: &str, record: &SyncRecord, device: &str, key: &SigningKey) -> Value {
    let signature = key.sign(&canonical_bytes(slice, record)).to_string();
    json!({
        "version": PROTOCOL_VERSION,
        "request_id": PHONE,
        "slice": slice,
        "params": null,
        "record": WireRecord::from(record),
        "device_id": device,
        "signature": signature,
    })
}

fn enroll(device: &str, key: &SigningKey) -> Value {
    json!({
        "version": PROTOCOL_VERSION, "request_id": PHONE, "device_id": device,
        "public_key": key.verifying_key().to_string(),
    })
}

fn refused(reply: &Value) -> bool {
    reply.get("error").is_some() || reply.get("code").is_some()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn a_hostile_sync_client_leaves_no_trace() {
    let captured = CapturedLogs::install().expect("the only subscriber");

    let limiter = Arc::new(RateLimiter::new(1000, 1000.0, 65_536));
    let signed = serve(signed_slice_sync_router(
        verifier(),
        source(),
        limiter.clone(),
        Arc::new(EnrollmentRegistry::new()),
    ))
    .await;
    let live = serve(live_slice_sync_router(
        verifier(),
        source(),
        limiter,
        &BroadcastChangePublisher::new(16),
    ))
    .await;

    let device_key = SigningKey::generate().expect("a key");
    let rotated_key = SigningKey::generate().expect("a key");
    let bobs_key = SigningKey::generate().expect("a key");
    let mut alice = connect(&signed, "alice").await;
    let mut bob = connect(&signed, "bob").await;

    let reply = call(
        &mut bob,
        action::DEVICE_ENROLL,
        enroll(OTHER_EMAIL, &bobs_key),
    )
    .await;
    assert!(!refused(&reply), "{reply}");
    let reply = call(
        &mut alice,
        action::DEVICE_ENROLL,
        enroll(OTHER_EMAIL, &device_key),
    )
    .await;
    assert!(refused(&reply), "{reply}");
    let reply = call(
        &mut alice,
        action::DEVICE_ENROLL,
        enroll(EMAIL, &device_key),
    )
    .await;
    assert!(!refused(&reply), "{reply}");

    let frames = [
        (
            action::SLICE_UPSERT,
            upsert("notes", &note(EMAIL, 5), "never@example.nl", &bobs_key),
            true,
        ),
        (
            action::SLICE_UPSERT,
            upsert("notes", &note(EMAIL, 5), EMAIL, &device_key),
            false,
        ),
        (
            action::SLICE_UPSERT,
            upsert("notes", &note(EMAIL, -31_612_345_678), EMAIL, &device_key),
            true,
        ),
        (
            action::SLICE_UPSERT,
            upsert(EMAIL, &note(EMAIL, 5), EMAIL, &device_key),
            true,
        ),
        (
            action::SLICE_PULL,
            json!({
                "version": PROTOCOL_VERSION, "request_id": PHONE, "slice": EMAIL, "params": null,
                "since": PHONE, "limit": 7,
            }),
            true,
        ),
        (
            action::SLICE_PULL,
            json!({
                "version": PROTOCOL_VERSION, "request_id": PHONE, "slice": "notes", "params": null,
                "since": PHONE, "cursor": { "updated_at": 1, "key": EMAIL }, "limit": 7,
            }),
            false,
        ),
        (
            action::SLICE_PULL,
            json!({
                "version": PROTOCOL_VERSION, "request_id": PHONE, "slice": "notes",
                "params": { EMAIL: EMAIL }, "since": EMAIL,
            }),
            true,
        ),
        (
            action::DEVICE_ROTATE,
            json!({
                "version": PROTOCOL_VERSION, "request_id": PHONE, "device_id": EMAIL,
                "new_public_key": rotated_key.verifying_key().to_string(),
            }),
            false,
        ),
        (
            action::DEVICE_REVOKE,
            json!({ "version": PROTOCOL_VERSION, "request_id": PHONE, "device_id": OTHER_EMAIL }),
            true,
        ),
        (
            action::DEVICE_REVOKE,
            json!({ "version": PROTOCOL_VERSION, "request_id": PHONE, "device_id": EMAIL }),
            false,
        ),
    ];
    for (index, (action, data, expect_refusal)) in frames.into_iter().enumerate() {
        let reply = call(&mut alice, action, data).await;
        assert_eq!(refused(&reply), expect_refusal, "frame {index}: {reply}");
    }

    let mut subscriber = connect(&live, "alice").await;
    let unknown = call(
        &mut subscriber,
        action::SLICE_SUBSCRIBE,
        json!({ "version": PROTOCOL_VERSION, "request_id": PHONE, "slice": EMAIL, "params": null }),
    )
    .await;
    assert!(unknown.to_string().contains("SLICE_001"), "{unknown}");
    let known = call(
        &mut subscriber,
        action::SLICE_SUBSCRIBE,
        json!({ "version": PROTOCOL_VERSION, "request_id": PHONE, "slice": "notes", "params": null }),
    )
    .await;
    assert!(!refused(&known), "{known}");

    let vault = serve(vault_slice_sync_router(
        Arc::new(VaultChallengeGauge::new()),
        source(),
        Arc::new(RateLimiter::new(1000, 1000.0, 65_536)),
        65_536,
    ))
    .await;
    let (mut anonymous, _) = connect_async(format!("ws://{}", vault.local_addr()))
        .await
        .expect("connect");
    let challenge = call(
        &mut anonymous,
        action::VAULT_CHALLENGE,
        json!({ "version": PROTOCOL_VERSION, "request_id": PHONE }),
    )
    .await;
    assert!(!refused(&challenge), "{challenge}");

    signed.stop().await.expect("the server drains");
    live.stop().await.expect("the server drains");
    vault.stop().await.expect("the server drains");

    let written = captured.text();
    for signal in [
        "device enrolled",
        "enroll rejected",
        "push from an unenrolled device",
        "device push signature verified",
        "slice upsert applied",
        "slice pull served",
        "slice operation refused",
        "device key rotated",
        "device revoked",
        "slice subscription registered",
        "vault challenge issued",
        "slice=\"notes\"",
        "has_cursor=true",
    ] {
        assert!(
            written.contains(signal),
            "`{signal}` was recorded:\n{written}"
        );
    }
    assert!(
        !written.contains("slice operation failed") && !written.contains(" ERROR "),
        "a client's refusal was logged as a server failure:\n{written}"
    );
    for key in [&device_key, &rotated_key] {
        let fingerprint = DeviceKeyFingerprint::of(&key.verifying_key()).to_string();
        assert!(
            written.contains(&fingerprint),
            "the device is recorded as its key's fingerprint {fingerprint}:\n{written}"
        );
    }
    for needle in [
        EMAIL.to_owned(),
        OTHER_EMAIL.to_owned(),
        "never@example.nl".to_owned(),
        PHONE.to_string(),
    ] {
        assert!(
            !written.contains(&needle),
            "`{needle}` reached the logs:\n{written}"
        );
    }
}
