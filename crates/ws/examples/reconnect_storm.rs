//! A reconnect storm against a real server, measured across anonymous shares, under a `lockstep`
//! (no jitter) or `jitter` (a copy of `fundamental_ws_client`'s crate-private `Backoff`) schedule.
//!
//! ```text
//! cargo run --release -p fundamental_ws --example reconnect_storm -- 10000 1,2,5,10,20,50 jitter
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fundamental_ws::{
    AnonymousLimits, CloseReason, Route, WsConfig, WsContext, WsResult, WsRouter, WsServer,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::Barrier;
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// How long a storm may take before it is reported as not converging.
const GIVE_UP: Duration = Duration::from_secs(300);
/// One simulated mobile round trip is drawn uniformly from this range, in milliseconds.
const ROUND_TRIP_MS: (u64, u64) = (50, 1_000);

#[derive(Deserialize)]
struct Login {
    user: String,
}

async fn login(context: WsContext, login: Login) -> WsResult<()> {
    let holder = login.user.clone();
    context.bind_principal(&holder, login.user)?;
    context
        .send(serde_json::json!({ "authenticated": true }))
        .await
}

const CAP: Duration = Duration::from_secs(10);
/// The floor the jittered client sets after a `1013` close.
const TRY_AGAIN_LATER_FLOOR: Duration = Duration::from_secs(5);

/// Which of the sync client's reconnect schedules the fleet runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Schedule {
    Lockstep,
    Jitter,
}

enum Attempt {
    Bound(Box<Socket>),
    /// Closed with `1013`.
    TryAgainLater,
    Failed,
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn lockstep_delay(attempt: u32) -> Duration {
    Duration::from_millis(100)
        .saturating_mul(1u32 << attempt.min(20))
        .min(CAP)
}

fn jittered_delay(step: u32, hint: bool, seed: &mut u64) -> Duration {
    let window = Duration::from_secs(1)
        .saturating_mul(1u32 << step.min(20))
        .min(CAP);
    if hint {
        TRY_AGAIN_LATER_FLOOR + uniform(window.max(TRY_AGAIN_LATER_FLOOR), seed)
    } else {
        uniform(window, seed)
    }
}

fn uniform(max: Duration, seed: &mut u64) -> Duration {
    let max_ms = u64::try_from(max.as_millis()).unwrap_or(u64::MAX);
    let scaled = (u128::from(next(seed)) * (u128::from(max_ms) + 1)) >> 64;
    Duration::from_millis(u64::try_from(scaled).unwrap_or(max_ms))
}

/// Deterministic, so two runs of one share draw the same numbers.
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn round_trip(seed: &mut u64) -> Duration {
    let (low, high) = ROUND_TRIP_MS;
    Duration::from_millis(low + next(seed) % (high - low))
}

struct Outcome {
    share_percent: usize,
    anonymous_slots: usize,
    converged: Option<Duration>,
    to_authenticated: Vec<Duration>,
    attempts: u64,
    displaced: u64,
    over_share: u64,
}

#[expect(clippy::too_many_lines, reason = "readability baseline")]
async fn storm(clients: usize, share_percent: usize, schedule: Schedule) -> Outcome {
    let anonymous_slots = (clients * share_percent / 100).max(1);
    let router = WsRouter::new().route(Route::new("login", login));
    let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"))
        .max_connections(clients + anonymous_slots)
        .authenticate(AnonymousLimits::default().max_connections(anonymous_slots));
    let listener = WsServer::new(config, router).serve().await.expect("serve");
    let stats = listener.stats();
    let url = format!("ws://{}", listener.local_addr());

    let attempts = Arc::new(AtomicU64::new(0));
    let start = Arc::new(Barrier::new(clients + 1));
    let began = Instant::now();
    let mut devices = Vec::with_capacity(clients);
    for index in 0..clients {
        let url = url.clone();
        let attempts = Arc::clone(&attempts);
        let start = Arc::clone(&start);
        devices.push(tokio::spawn(async move {
            let mut seed = 0x9E37_79B9_7F4A_7C15_u64 ^ (index as u64 + 1);
            start.wait().await;
            let began = Instant::now();
            if schedule == Schedule::Jitter {
                sleep(uniform(Duration::from_secs(1), &mut seed)).await;
            }
            let mut attempt = 0u32;
            loop {
                attempts.fetch_add(1, Ordering::Relaxed);
                let hint = match try_once(&url, index, &mut seed).await {
                    Attempt::Bound(socket) => return Some((began.elapsed(), socket)),
                    Attempt::TryAgainLater => true,
                    Attempt::Failed => false,
                };
                attempt = attempt.saturating_add(1);
                sleep(match schedule {
                    Schedule::Lockstep => lockstep_delay(attempt - 1),
                    Schedule::Jitter => jittered_delay(attempt, hint, &mut seed),
                })
                .await;
            }
        }));
    }
    start.wait().await;

    let mut to_authenticated = Vec::with_capacity(clients);
    let mut held = Vec::with_capacity(clients);
    let all = timeout(GIVE_UP, async {
        for device in devices {
            if let Ok(Some((elapsed, socket))) = device.await {
                to_authenticated.push(elapsed);
                held.push(socket);
            }
        }
    })
    .await;
    let converged = all.ok().map(|()| began.elapsed());
    to_authenticated.sort_unstable();
    let outcome = Outcome {
        share_percent,
        anonymous_slots,
        converged,
        to_authenticated,
        attempts: attempts.load(Ordering::Relaxed),
        displaced: stats.closes(CloseReason::Displaced),
        over_share: stats.closes(CloseReason::OverShare),
    };
    drop(held);
    listener.stop().await.expect("the server drains");
    outcome
}

/// One connection: upgrade, two round trips of authentication, login; kept open on success.
async fn try_once(url: &str, index: usize, seed: &mut u64) -> Attempt {
    let Ok((mut socket, _)) = connect_async(url).await else {
        return Attempt::Failed;
    };
    sleep(round_trip(seed) + round_trip(seed)).await;
    let frame =
        serde_json::json!({ "action": "login", "data": { "user": format!("device-{index}") } });
    if socket.send(Message::text(frame.to_string())).await.is_err() {
        return Attempt::Failed;
    }
    loop {
        match timeout(Duration::from_secs(30), socket.next()).await {
            Ok(Some(Ok(Message::Text(text)))) if text.contains("authenticated") => {
                return Attempt::Bound(Box::new(socket));
            }
            Ok(Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(Some(Ok(Message::Close(Some(frame))))) if u16::from(frame.code) == 1013 => {
                return Attempt::TryAgainLater;
            }
            _ => return Attempt::Failed,
        }
    }
}

fn percentile(sorted: &[Duration], p: usize) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    sorted[(sorted.len() * p / 100).min(sorted.len() - 1)]
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let clients: usize = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10_000);
    let shares: Vec<usize> = args
        .next()
        .unwrap_or_else(|| "1,2,5,10,20,50".to_string())
        .split(',')
        .filter_map(|share| share.parse().ok())
        .collect();
    let schedule = match args.next().as_deref() {
        Some("jitter") => Schedule::Jitter,
        _ => Schedule::Lockstep,
    };

    println!(
        "{clients} clients, {} backoff, round trip {}–{} ms, two per login",
        match schedule {
            Schedule::Lockstep => "lockstep (no jitter)",
            Schedule::Jitter => "full-jitter",
        },
        ROUND_TRIP_MS.0,
        ROUND_TRIP_MS.1
    );
    println!("share  slots  converged   p50      p99      attempts  displaced  over_share");
    for share in shares {
        let outcome = storm(clients, share, schedule).await;
        let converged = outcome.converged.map_or_else(
            || "did not".to_string(),
            |at| format!("{:.2}s", at.as_secs_f64()),
        );
        println!(
            "{:>4}%  {:>5}  {:>9}  {:>6.2}s  {:>6.2}s  {:>8}  {:>9}  {:>10}",
            outcome.share_percent,
            outcome.anonymous_slots,
            converged,
            percentile(&outcome.to_authenticated, 50).as_secs_f64(),
            percentile(&outcome.to_authenticated, 99).as_secs_f64(),
            outcome.attempts,
            outcome.displaced,
            outcome.over_share,
        );
        sleep(Duration::from_secs(2)).await;
    }
}
