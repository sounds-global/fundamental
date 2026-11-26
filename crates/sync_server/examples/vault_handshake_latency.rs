//! The hub's own cost of a vault handshake over loopback, repeated a thousand times.
//!
//! Run with `cargo run --release -p fundamental_sync_server --example vault_handshake_latency -- 1000`.

use std::sync::Arc;
use std::time::Duration;

use fundamental_core::secrets::Secret;
use fundamental_keys::VaultOwnerKey;
use fundamental_sync::MemorySliceStore;
use fundamental_sync::wire::{PROTOCOL_VERSION, action};
use fundamental_sync_server::{
    RateLimiter, SliceRegistry, SliceServerSource, VaultChallengeGauge, vault_slice_sync_router,
};
use fundamental_ws::{WsConfig, WsEnvelope, WsServer};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn call(socket: &mut Socket, action: &str, data: Value) -> Value {
    let frame = serde_json::to_string(&WsEnvelope::new(action, data)).expect("frame");
    socket.send(Message::text(frame)).await.expect("send");
    loop {
        if let Message::Text(text) = socket.next().await.expect("a frame").expect("ok") {
            return serde_json::from_str(&text).expect("json");
        }
    }
}

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    sorted[(sorted.len() * p / 100).min(sorted.len() - 1)]
}

#[tokio::main(flavor = "multi_thread")]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn main() {
    let handshakes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000);
    let router = vault_slice_sync_router(
        Arc::new(VaultChallengeGauge::new()),
        Arc::new(SliceServerSource::new(
            SliceRegistry::new(),
            MemorySliceStore::new(),
        )),
        Arc::new(RateLimiter::new(1_000, 1_000.0, 65_536)),
        65_536,
    );
    let listener = WsServer::new(WsConfig::new("127.0.0.1:0".parse().expect("addr")), router)
        .serve()
        .await
        .expect("serve");
    let url = format!("ws://{}", listener.local_addr());

    let mut upgrade = Vec::with_capacity(handshakes);
    let mut proof = Vec::with_capacity(handshakes);
    for index in 0..handshakes {
        let key = VaultOwnerKey::from_child_seed(&Secret::new([(index % 251) as u8 + 1; 32]));
        let began = Instant::now();
        let (mut socket, _) = connect_async(&url).await.expect("connect");
        upgrade.push(began.elapsed());

        let proving = Instant::now();
        let challenge = call(
            &mut socket,
            action::VAULT_CHALLENGE,
            json!({ "version": PROTOCOL_VERSION, "request_id": 1 }),
        )
        .await;
        let nonce: Vec<u8> = serde_json::from_value(challenge["nonce"].clone()).expect("nonce");
        let signed: [u8; 32] = nonce.as_slice().try_into().expect("32 bytes");
        let answer = call(
            &mut socket,
            action::VAULT_AUTH,
            json!({
                "version": PROTOCOL_VERSION,
                "request_id": 2,
                "owner_public_key": key.public_key().to_string(),
                "nonce": nonce,
                "signature": key.sign_challenge(&signed).to_string(),
            }),
        )
        .await;
        assert_eq!(answer["ok"], true, "{answer}");
        proof.push(proving.elapsed());
        socket.close(None).await.ok();
    }
    upgrade.sort_unstable();
    proof.sort_unstable();
    println!("{handshakes} vault handshakes over loopback (release build)");
    for (name, sorted) in [("upgrade", &upgrade), ("challenge+answer", &proof)] {
        println!(
            "{name:>17}: p50 {:>7.3} ms  p99 {:>7.3} ms  max {:>7.3} ms",
            percentile(sorted, 50).as_secs_f64() * 1e3,
            percentile(sorted, 99).as_secs_f64() * 1e3,
            sorted[sorted.len() - 1].as_secs_f64() * 1e3,
        );
    }
    listener.stop().await.expect("the server drains");
}
