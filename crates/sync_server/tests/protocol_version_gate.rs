//! The protocol-version gate refuses every action every sync router serves.
//!
//! The actions come from [`WsRouter::actions`], so an ungated or unexercised route fails here.

use std::collections::BTreeMap;
use std::sync::Arc;

use fundamental_auth::JwtVerifier;
use fundamental_db::Filter;
use fundamental_db::RecordKey;
use fundamental_macros::Model;
use fundamental_sync::wire::{
    AuthRequest, EnrollRequest, ErrorReply, RevokeRequest, RotateRequest, SlicePullRequest,
    SliceSubscribeRequest, SliceUpsertRequest, VaultAuthRequest, VaultChallengeRequest, WireMeta,
    WireRecord, action,
};
use fundamental_sync::{BroadcastChangePublisher, MemorySliceStore};
use fundamental_sync_server::{
    EnrollmentRegistry, NoParams, RateLimiter, SliceRegistry, SliceServerSource,
    VaultChallengeGauge, live_slice_sync_router, signed_slice_sync_router, slice_sync_router,
    vault_slice_sync_router,
};
use fundamental_ws::{WsConfig, WsEnvelope, WsListener, WsRouter, WsServer};
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

/// A version no build serves.
const UNSPEAKABLE: u32 = 999;

const PROBE_REQUEST_ID: u64 = 4242;

type RawWs = WebSocketStream<MaybeTlsStream<TcpStream>>;

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

fn verifier() -> JwtVerifier {
    JwtVerifier::new(
        DecodingKey::from_secret(b"protocol-version-gate"),
        Validation::new(Algorithm::HS256),
    )
}

fn limiter() -> Arc<RateLimiter> {
    Arc::new(RateLimiter::new(1000, 1000.0, 65_536))
}

/// A shape-valid frame body for `action`, so only the gate can refuse it.
fn frame_for(action: &str) -> Value {
    let record = || WireRecord {
        id: RecordKey::single("row-1"),
        meta: WireMeta {
            updated_at: 1,
            version: 1,
            tombstone: false,
        },
        payload: BTreeMap::new(),
    };
    let dto = match action {
        action::AUTH => serde_json::to_value(AuthRequest::new(PROBE_REQUEST_ID, "a-token")),
        action::SLICE_PULL => serde_json::to_value(SlicePullRequest::new(
            PROBE_REQUEST_ID,
            "notes",
            Value::Null,
            0,
        )),
        action::SLICE_UPSERT => serde_json::to_value(SliceUpsertRequest::new(
            PROBE_REQUEST_ID,
            "notes",
            Value::Null,
            record(),
        )),
        action::SLICE_SUBSCRIBE => serde_json::to_value(SliceSubscribeRequest::new(
            PROBE_REQUEST_ID,
            "notes",
            Value::Null,
        )),
        action::DEVICE_ENROLL => {
            serde_json::to_value(EnrollRequest::new(PROBE_REQUEST_ID, "device-1", "key"))
        }
        action::DEVICE_ROTATE => {
            serde_json::to_value(RotateRequest::new(PROBE_REQUEST_ID, "device-1", "key"))
        }
        action::DEVICE_REVOKE => {
            serde_json::to_value(RevokeRequest::new(PROBE_REQUEST_ID, "device-1"))
        }
        action::VAULT_CHALLENGE => {
            serde_json::to_value(VaultChallengeRequest::new(PROBE_REQUEST_ID))
        }
        action::VAULT_AUTH => serde_json::to_value(VaultAuthRequest::new(
            PROBE_REQUEST_ID,
            "owner-key",
            vec![0u8; 32],
            "signature",
        )),
        other => panic!(
            "`{other}` is served by a router but this test has no frame for it: add one, so the \
             version gate is proved for it too"
        ),
    };
    dto.expect("a request serializes")
}

async fn serve(router: WsRouter) -> WsListener {
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
    WsServer::new(config, router).serve().await.expect("serve")
}

async fn recv_text(ws: &mut RawWs) -> String {
    loop {
        match ws.next().await.expect("a frame").expect("an ok frame") {
            Message::Text(text) => return text.to_string(),
            Message::Close(_) => panic!("the connection closed before replying"),
            _ => continue,
        }
    }
}

async fn send(ws: &mut RawWs, action: &str, data: Value) {
    let envelope = WsEnvelope::new(action, data);
    let text = serde_json::to_string(&envelope).expect("to_string");
    ws.send(Message::text(text)).await.expect("send");
}

async fn every_action_is_gated(label: &str, router: WsRouter) {
    let actions: Vec<String> = router.actions().map(str::to_owned).collect();
    assert!(
        !actions.is_empty(),
        "{label} serves no actions; the sweep would pass vacuously"
    );

    let listener = serve(router).await;
    let url = format!("ws://{}", listener.local_addr());

    for action in &actions {
        let (mut ws, _) = connect_async(&url).await.expect("connect");
        let mut body = frame_for(action);
        body["version"] = json!(UNSPEAKABLE);
        send(&mut ws, action, body).await;

        let text = recv_text(&mut ws).await;
        let reply: ErrorReply = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!("{label}/{action}: expected a correlated ErrorReply, got `{text}` ({e})")
        });
        assert_eq!(
            reply.error.code, "SYNC_007",
            "{label}/{action} must be refused by the version gate, not by a handler: {text}"
        );
        assert_eq!(
            reply.request_id, PROBE_REQUEST_ID,
            "{label}/{action}: an uncorrelated refusal leaves the device's request hanging"
        );
        assert!(
            reply.error.message.contains("999"),
            "{label}/{action}: the refusal must name the version it refused: {}",
            reply.error.message
        );
    }

    listener.stop().await.expect("the server drains");
}

#[tokio::test]
async fn the_slice_router_gates_every_action() {
    every_action_is_gated(
        "slice_sync_router",
        slice_sync_router(Arc::new(verifier()), source(), limiter()),
    )
    .await;
}

#[tokio::test]
async fn the_live_slice_router_gates_every_action() {
    let publisher = BroadcastChangePublisher::new(8);
    every_action_is_gated(
        "live_slice_sync_router",
        live_slice_sync_router(Arc::new(verifier()), source(), limiter(), &publisher),
    )
    .await;
}

#[tokio::test]
async fn the_signed_slice_router_gates_every_action() {
    every_action_is_gated(
        "signed_slice_sync_router",
        signed_slice_sync_router(
            Arc::new(verifier()),
            source(),
            limiter(),
            Arc::new(EnrollmentRegistry::new()),
        ),
    )
    .await;
}

#[tokio::test]
async fn the_vault_router_gates_every_action() {
    every_action_is_gated(
        "vault_slice_sync_router",
        vault_slice_sync_router(
            Arc::new(VaultChallengeGauge::new()),
            source(),
            limiter(),
            64 * 1024,
        ),
    )
    .await;
}

/// A frame at the current version reaches its handler.
#[tokio::test]
async fn a_supported_version_reaches_the_handler() {
    let listener = serve(slice_sync_router(Arc::new(verifier()), source(), limiter())).await;
    let url = format!("ws://{}", listener.local_addr());
    let (mut ws, _) = connect_async(&url).await.expect("connect");

    send(&mut ws, action::AUTH, frame_for(action::AUTH)).await;

    let text = recv_text(&mut ws).await;
    let reply: ErrorReply = serde_json::from_str(&text).expect("a correlated reply");
    assert_ne!(
        reply.error.code, "SYNC_007",
        "a frame at the supported version must pass the gate, not be refused by it: {text}"
    );
    assert_eq!(reply.request_id, PROBE_REQUEST_ID);

    listener.stop().await.expect("the server drains");
}
