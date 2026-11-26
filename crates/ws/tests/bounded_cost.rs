//! An anonymous connection has a bounded cost, driven against a real server over loopback.

mod support;

use std::time::Duration;

use fundamental_ws::{AnonymousLimits, CloseReason};
use futures::SinkExt;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep, timeout};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::tungstenite::Message;

use support::{
    PATIENCE, close_frame, closed_for, config, connect, log_in, reply, router, send, serve, until,
};

fn quick() -> AnonymousLimits {
    AnonymousLimits::default()
        .within(Duration::from_millis(600))
        .displace_after(Duration::from_millis(200))
}

#[tokio::test]
async fn an_idle_upgrade_is_closed_at_the_handshake_deadline() {
    let (listener, stats) = serve(
        config().handshake_timeout(Duration::from_millis(200)),
        router(),
    )
    .await;
    let mut raw = TcpStream::connect(listener.local_addr())
        .await
        .expect("tcp");
    let mut byte = [0u8; 1];
    let read = timeout(PATIENCE, raw.read(&mut byte))
        .await
        .expect("dropped in time");
    assert!(
        matches!(read, Ok(0) | Err(_)),
        "the server kept an idle upgrade: {read:?}"
    );
    closed_for(&stats, CloseReason::HandshakeTimeout).await;
    until(|| stats.anonymous() == 0, "the slot is given back").await;
}

#[tokio::test]
async fn an_unauthenticated_connection_is_closed_at_its_deadline() {
    let (listener, stats) = serve(config().authenticate(quick()), router()).await;
    let mut socket = connect(&listener).await;
    assert_eq!(
        close_frame(&mut socket).await,
        (4408, "auth_deadline".to_string())
    );
    closed_for(&stats, CloseReason::AuthDeadline).await;
    until(|| stats.anonymous() == 0, "the slot is given back").await;
}

#[tokio::test]
async fn a_peer_that_never_answers_the_close_holds_no_slot_while_it_is_waited_on() {
    let (listener, stats) = serve(config().authenticate(quick()), router()).await;
    let socket = connect(&listener).await;
    // The raw socket never answers the close, so the server waits a close grace.
    let MaybeTlsStream::Plain(mut raw) = socket.into_inner() else {
        panic!("a plain loopback socket");
    };
    let mut header = [0u8; 2];
    timeout(PATIENCE, raw.read_exact(&mut header))
        .await
        .expect("the close frame in time")
        .expect("the close frame");
    assert_eq!(header[0], 0x88, "a close frame");
    let told = Instant::now();
    until(|| stats.anonymous() == 0, "the slot is given back").await;
    assert!(
        told.elapsed() < Duration::from_millis(500),
        "the slot was held {:?} while the server waited for an answer",
        told.elapsed()
    );
    closed_for(&stats, CloseReason::AuthDeadline).await;
}

#[tokio::test]
async fn an_authenticated_connection_outlives_the_deadline() {
    let (listener, stats) = serve(config().authenticate(quick()), router()).await;
    let mut socket = connect(&listener).await;
    log_in(&mut socket, "device").await;
    sleep(quick().within * 2).await;
    send(&mut socket, "ping", serde_json::json!({})).await;
    assert_eq!(reply(&mut socket).await["pong"], true);
    assert_eq!(stats.authenticated(), 1);
    assert_eq!(stats.closes(CloseReason::AuthDeadline), 0);
}

#[tokio::test]
async fn anonymous_connections_cannot_take_an_authenticated_slot() {
    let limits = quick()
        .within(Duration::from_secs(30))
        .displace_after(Duration::from_secs(20));
    let (listener, stats) = serve(
        config()
            .max_connections(4)
            .authenticate(limits.max_connections(2)),
        router(),
    )
    .await;
    let _first = connect(&listener).await;
    let _second = connect(&listener).await;
    until(|| stats.anonymous() == 2, "two anonymous connections").await;
    let mut third = connect(&listener).await;
    let (code, reason) = close_frame(&mut third).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "over_share;retry_after_ms=20000");
    assert_eq!(stats.anonymous(), 2);
    assert_eq!(stats.authenticated(), 0);
}

#[tokio::test]
async fn a_full_authenticated_pool_refuses_with_ws_008_then_1013() {
    let (listener, stats) = serve(
        config()
            .max_connections(3)
            .authenticate(quick().max_connections(2)),
        router(),
    )
    .await;
    let mut first = connect(&listener).await;
    log_in(&mut first, "first").await;
    let mut second = connect(&listener).await;
    send(
        &mut second,
        "login",
        serde_json::json!({ "user": "second" }),
    )
    .await;
    assert_eq!(reply(&mut second).await["error"]["code"], "WS_008");
    let (code, reason) = close_frame(&mut second).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "saturated;retry_after_ms=600");
    closed_for(&stats, CloseReason::Saturated).await;
    assert_eq!(stats.authenticated(), 1);
}

#[tokio::test]
async fn a_device_authenticates_while_the_anonymous_share_is_full() {
    let limits = quick()
        .within(Duration::from_secs(30))
        .displace_after(Duration::from_millis(300))
        .max_connections(2);
    let (listener, stats) =
        serve(config().max_connections(10).authenticate(limits), router()).await;
    let mut attackers = [connect(&listener).await, connect(&listener).await];
    until(|| stats.anonymous() == 2, "the anonymous share is full").await;
    sleep(Duration::from_millis(400)).await;

    let mut device = connect(&listener).await;
    log_in(&mut device, "device").await;
    assert_eq!(stats.authenticated(), 1);
    closed_for(&stats, CloseReason::Displaced).await;
    let (code, reason) = close_frame(&mut attackers[0]).await;
    assert_eq!(code, 1013);
    assert_eq!(reason, "displaced;retry_after_ms=300");
    assert_eq!(
        stats.closes(CloseReason::OverShare),
        0,
        "the device was never refused"
    );
}

#[tokio::test]
async fn a_connection_over_a_young_share_is_told_when_to_come_back() {
    let limits = quick()
        .within(Duration::from_secs(30))
        .displace_after(Duration::from_secs(20))
        .max_connections(1);
    let (listener, stats) = serve(config().max_connections(4).authenticate(limits), router()).await;
    let _held = connect(&listener).await;
    until(|| stats.anonymous() == 1, "the share is full").await;
    let mut late = connect(&listener).await;
    assert_eq!(
        close_frame(&mut late).await,
        (1013, "over_share;retry_after_ms=20000".to_string())
    );
    closed_for(&stats, CloseReason::OverShare).await;
    until(
        || stats.refusing() == 0,
        "the over-share slot is given back",
    )
    .await;
}

#[tokio::test]
async fn a_peer_that_stops_reading_is_closed() {
    let (listener, stats) = serve(
        config().public().send_timeout(Duration::from_millis(300)),
        router(),
    )
    .await;
    let mut socket = connect(&listener).await;
    send(
        &mut socket,
        "burst",
        serde_json::json!({ "frames": 256, "bytes": 256 * 1024 }),
    )
    .await;
    closed_for(&stats, CloseReason::SlowPeer).await;
    until(|| stats.public() == 0, "the slot is given back").await;
    drop(socket);
}

#[tokio::test]
async fn an_oversized_anonymous_frame_is_refused_unparsed() {
    let (listener, _stats) = serve(
        config().authenticate(quick().within(Duration::from_secs(30)).max_frame_size(1024)),
        router(),
    )
    .await;
    let mut socket = connect(&listener).await;
    let padding = "x".repeat(2048);
    send(
        &mut socket,
        "ping",
        serde_json::json!({ "padding": padding }),
    )
    .await;
    assert_eq!(reply(&mut socket).await["error"]["code"], "WS_011");

    log_in(&mut socket, "device").await;
    send(
        &mut socket,
        "ping",
        serde_json::json!({ "padding": padding }),
    )
    .await;
    assert_eq!(
        reply(&mut socket).await["pong"],
        true,
        "an authenticated connection is held to max_frame_size, not the anonymous bound"
    );
}

#[tokio::test]
async fn an_anonymous_connection_is_closed_after_its_refusals() {
    let (listener, stats) = serve(
        config().authenticate(quick().within(Duration::from_secs(30)).max_refusals(3)),
        router(),
    )
    .await;
    let mut socket = connect(&listener).await;
    for _ in 0..3 {
        socket.send(Message::text("not json")).await.expect("send");
    }
    let (code, reason) = close_frame(&mut socket).await;
    assert_eq!((code, reason.as_str()), (1008, "refused"));
    closed_for(&stats, CloseReason::Refused).await;
}

#[tokio::test]
async fn a_binary_frame_is_refused() {
    let (listener, _stats) = serve(config().public(), router()).await;
    let mut socket = connect(&listener).await;
    socket
        .send(Message::binary(vec![0u8; 16]))
        .await
        .expect("send");
    assert_eq!(reply(&mut socket).await["error"]["code"], "WS_012");
}

#[tokio::test]
async fn a_panicking_handler_releases_its_slot() {
    let (listener, stats) = serve(config().public(), router()).await;
    let mut socket = connect(&listener).await;
    until(|| stats.public() == 1, "connected").await;
    send(&mut socket, "explode", serde_json::json!({})).await;
    until(
        || stats.public() == 0,
        "the slot is given back after a panic",
    )
    .await;
}

#[tokio::test]
async fn a_public_socket_is_never_closed_for_not_authenticating() {
    let (listener, stats) = serve(config().public(), router()).await;
    let mut socket = connect(&listener).await;
    sleep(AnonymousLimits::default().within + Duration::from_millis(500)).await;
    send(&mut socket, "ping", serde_json::json!({})).await;
    assert_eq!(reply(&mut socket).await["pong"], true);
    assert_eq!(stats.public(), 1);
    assert_eq!(stats.closes(CloseReason::AuthDeadline), 0);
}

#[tokio::test]
async fn one_credential_cannot_hold_the_authenticated_pool() {
    let (listener, stats) = serve(
        config()
            .max_connections(20)
            .max_per_principal(2)
            .authenticate(quick().within(Duration::from_secs(30))),
        router(),
    )
    .await;
    let mut first = connect(&listener).await;
    log_in(&mut first, "replayed").await;
    let mut second = connect(&listener).await;
    log_in(&mut second, "replayed").await;
    let mut third = connect(&listener).await;
    send(
        &mut third,
        "login",
        serde_json::json!({ "user": "replayed" }),
    )
    .await;
    assert_eq!(reply(&mut third).await["error"]["code"], "WS_013");
    assert_eq!(
        close_frame(&mut third).await,
        (1008, "principal_limit".to_string())
    );
    assert_eq!(stats.authenticated(), 2);

    let mut other = connect(&listener).await;
    log_in(&mut other, "someone else").await;
    assert_eq!(stats.authenticated(), 3);
}

#[tokio::test]
async fn a_ping_flood_is_closed() {
    let (listener, stats) = serve(config().public(), router()).await;
    let mut socket = connect(&listener).await;
    for _ in 0..40 {
        socket
            .send(Message::Ping(vec![0u8; 125].into()))
            .await
            .expect("send");
    }
    assert_eq!(
        close_frame(&mut socket).await,
        (1008, "refused".to_string())
    );
    closed_for(&stats, CloseReason::Refused).await;
}

#[tokio::test]
async fn a_stalled_over_share_upgrade_gives_its_slot_back_within_a_second() {
    let limits = quick()
        .within(Duration::from_secs(30))
        .displace_after(Duration::from_secs(20))
        .max_connections(1);
    let (listener, stats) = serve(config().max_connections(4).authenticate(limits), router()).await;
    let _held = connect(&listener).await;
    until(|| stats.anonymous() == 1, "the share is full").await;
    let _stalled = TcpStream::connect(listener.local_addr())
        .await
        .expect("tcp");
    until(|| stats.refusing() == 1, "admitted over the share").await;
    let started = tokio::time::Instant::now();
    until(
        || stats.refusing() == 0,
        "the over-share slot is given back",
    )
    .await;
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "held for {:?}",
        started.elapsed()
    );
    closed_for(&stats, CloseReason::HandshakeTimeout).await;
}
