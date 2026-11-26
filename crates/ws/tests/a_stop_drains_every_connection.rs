//! Stopping a server drains it: `1001` at each frame boundary, running handlers finish, and a stuck one
//! is aborted at `drain_timeout`.

use std::future::pending;
use std::io::ErrorKind;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fundamental_ws::test_support::{
    PATIENCE, Socket, close_frame, connect, next_text, serve, until,
};
use fundamental_ws::{CloseReason, Route, WsConfig, WsContext, WsListener, WsResult, WsRouter};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::{Instant, timeout};
use tokio_tungstenite::tungstenite::{Error as TungsteniteError, Message};

#[derive(Deserialize)]
struct Empty {}

#[derive(Clone)]
struct Work {
    dispatched: Arc<AtomicUsize>,
    started: watch::Sender<usize>,
    release: watch::Sender<bool>,
}

impl Work {
    fn new() -> Self {
        Self {
            dispatched: Arc::default(),
            started: watch::Sender::new(0),
            release: watch::Sender::new(false),
        }
    }

    fn router(&self) -> WsRouter {
        let work = self.clone();
        let stuck = self.clone();
        WsRouter::new()
            .route(Route::new("work", move |context: WsContext, _: Empty| {
                let work = work.clone();
                async move {
                    let frame = work.dispatched.fetch_add(1, Ordering::SeqCst) + 1;
                    work.started.send_replace(frame);
                    let mut release = work.release.subscribe();
                    let _ = release.wait_for(|open| *open).await;
                    context.send(json!({ "done": frame })).await
                }
            }))
            .route(Route::new("forever", move |_: WsContext, _: Empty| {
                let stuck = stuck.clone();
                async move {
                    let frame = stuck.dispatched.fetch_add(1, Ordering::SeqCst) + 1;
                    stuck.started.send_replace(frame);
                    pending::<WsResult<()>>().await
                }
            }))
            .route(Route::new("noop", |_: WsContext, _: Empty| async {
                Ok(())
            }))
    }

    async fn started(&self, frames: usize) {
        let mut started = self.started.subscribe();
        timeout(PATIENCE, started.wait_for(|n| *n >= frames))
            .await
            .expect("the handler started in time")
            .expect("the gate is held");
    }

    fn release(&self) {
        self.release.send_replace(true);
    }

    fn dispatched(&self) -> usize {
        self.dispatched.load(Ordering::SeqCst)
    }
}

fn public() -> WsConfig {
    WsConfig::new("127.0.0.1:0".parse().expect("addr")).public()
}

async fn send(socket: &mut Socket, action: &str) {
    socket
        .send(Message::text(
            json!({ "action": action, "data": {} }).to_string(),
        ))
        .await
        .expect("the frame is sent");
}

/// Starts a stop in the background, returning once an idle client has been told `1001`.
async fn begin_stop(
    listener: WsListener,
    idle: &mut Socket,
) -> tokio::task::JoinHandle<WsResult<()>> {
    let stopping = tokio::spawn(listener.stop());
    assert_eq!(close_frame(idle).await, (1001, "going_away".to_string()));
    stopping
}

#[tokio::test]
async fn every_client_is_told_the_server_is_going_away() {
    let (listener, stats) = serve(public(), Work::new().router()).await;
    let mut clients = Vec::new();
    for _ in 0..3 {
        clients.push(connect(&listener).await);
    }
    until(|| stats.public() == 3, "three connections admitted").await;

    timeout(PATIENCE, listener.stop())
        .await
        .expect("the stop finished in time")
        .expect("the server drains");

    for client in &mut clients {
        assert_eq!(close_frame(client).await, (1001, "going_away".to_string()));
    }
    assert_eq!(stats.closes(CloseReason::GoingAway), 3);
    assert_eq!(stats.closes(CloseReason::DrainTimeout), 0);
    assert_eq!(stats.public(), 0);
}

#[tokio::test]
async fn a_handler_in_flight_finishes_and_its_reply_arrives_before_the_close() {
    let work = Work::new();
    let (listener, stats) = serve(public(), work.router()).await;
    let mut busy = connect(&listener).await;
    let mut idle = connect(&listener).await;
    until(|| stats.public() == 2, "two connections admitted").await;
    send(&mut busy, "work").await;
    work.started(1).await;

    let stopping = begin_stop(listener, &mut idle).await;
    assert!(!stopping.is_finished(), "the drain waits for the handler");
    work.release();

    assert_eq!(next_text(&mut busy).await, json!({ "done": 1 }));
    assert_eq!(
        close_frame(&mut busy).await,
        (1001, "going_away".to_string())
    );
    timeout(PATIENCE, stopping)
        .await
        .expect("the stop finished in time")
        .expect("not panicked")
        .expect("the server drains");
    assert_eq!(stats.closes(CloseReason::GoingAway), 2);
}

#[tokio::test]
async fn a_frame_sent_after_the_drain_began_is_not_dispatched() {
    let work = Work::new();
    let (listener, stats) = serve(public(), work.router()).await;
    let mut busy = connect(&listener).await;
    let mut idle = connect(&listener).await;
    until(|| stats.public() == 2, "two connections admitted").await;
    send(&mut busy, "work").await;
    work.started(1).await;

    let stopping = begin_stop(listener, &mut idle).await;
    send(&mut busy, "work").await;
    work.release();

    assert_eq!(next_text(&mut busy).await, json!({ "done": 1 }));
    assert_eq!(
        close_frame(&mut busy).await,
        (1001, "going_away".to_string())
    );
    timeout(PATIENCE, stopping)
        .await
        .expect("the stop finished in time")
        .expect("not panicked")
        .expect("the server drains");
    assert_eq!(work.dispatched(), 1, "the frame sent during the drain ran");
}

#[tokio::test]
async fn a_new_connection_is_refused_from_the_moment_the_stop_begins() {
    let work = Work::new();
    let (listener, stats) = serve(public(), work.router()).await;
    let addr = listener.local_addr();
    let mut busy = connect(&listener).await;
    let mut idle = connect(&listener).await;
    until(|| stats.public() == 2, "two connections admitted").await;
    send(&mut busy, "work").await;
    work.started(1).await;

    let stopping = begin_stop(listener, &mut idle).await;
    let refused = TcpStream::connect(addr)
        .await
        .expect_err("the listener is closed while the drain runs");
    assert_eq!(refused.kind(), ErrorKind::ConnectionRefused);

    work.release();
    timeout(PATIENCE, stopping)
        .await
        .expect("the stop finished in time")
        .expect("not panicked")
        .expect("the server drains");
}

#[tokio::test]
async fn a_handler_that_never_returns_is_aborted_at_the_drain_timeout_and_not_before() {
    const DRAIN: Duration = Duration::from_secs(4);
    const CLOSE_GRACE: Duration = Duration::from_secs(1);
    let work = Work::new();
    let config = public()
        .handshake_timeout(Duration::from_secs(1))
        .drain_timeout(DRAIN);
    let (listener, stats) = serve(config, work.router()).await;
    let mut stuck = connect(&listener).await;
    until(|| stats.public() == 1, "the connection admitted").await;
    send(&mut stuck, "forever").await;
    work.started(1).await;
    let started = Instant::now();
    timeout(PATIENCE + DRAIN, listener.stop())
        .await
        .expect("the stop finished in time")
        .expect("the server drains");
    let took = started.elapsed();

    assert!(
        took >= DRAIN,
        "aborted after {took:?}, before the {DRAIN:?} drain"
    );
    assert!(
        took < DRAIN + CLOSE_GRACE + Duration::from_secs(2),
        "the stop took {took:?}, past its budget"
    );
    assert_eq!(stats.closes(CloseReason::DrainTimeout), 1);
    assert_eq!(stats.closes(CloseReason::GoingAway), 0);
    let ended = timeout(PATIENCE, stuck.next())
        .await
        .expect("ended in time");
    assert!(
        !matches!(ended, Some(Ok(Message::Close(_)))),
        "an aborted session cannot send a close frame, got {ended:?}"
    );
}

#[tokio::test]
async fn a_stop_given_up_on_aborts_the_server_instead_of_leaving_it_draining() {
    let work = Work::new();
    let (listener, stats) = serve(public(), work.router()).await;
    let mut stuck = connect(&listener).await;
    until(|| stats.public() == 1, "the connection admitted").await;
    send(&mut stuck, "forever").await;
    work.started(1).await;

    let given_up = timeout(Duration::from_millis(200), listener.stop()).await;
    assert!(
        given_up.is_err(),
        "the drain was still waiting on the handler"
    );

    until(
        || stats.closes(CloseReason::DrainTimeout) == 1,
        "the abandoned server's session was aborted, not left running",
    )
    .await;
    assert_eq!(stats.public(), 0);
}

#[tokio::test]
async fn a_drain_spreads_its_closes_so_the_fleet_does_not_leave_at_once() {
    const OPEN: usize = 40;
    const SPREAD: Duration = Duration::from_secs(2);
    let config = public().drain_rate(20);
    let (listener, stats) = serve(config, Work::new().router()).await;
    let mut clients = Vec::with_capacity(OPEN);
    for _ in 0..OPEN {
        clients.push(connect(&listener).await);
    }
    until(
        || stats.public() == OPEN as u64,
        "every connection admitted",
    )
    .await;

    let began = Instant::now();
    let stopping = tokio::spawn(listener.stop());
    let told: Vec<_> = clients
        .into_iter()
        .map(|mut client| {
            tokio::spawn(async move {
                let code = close_frame(&mut client).await.0;
                (code, began.elapsed())
            })
        })
        .collect();
    let mut at = Vec::with_capacity(OPEN);
    for client in told {
        let (code, when) = client.await.expect("not panicked");
        assert_eq!(code, 1001);
        at.push(when);
    }
    timeout(PATIENCE, stopping)
        .await
        .expect("the stop finished in time")
        .expect("not panicked")
        .expect("the server drains");

    let (first, last) = (
        at.iter().min().copied().expect("closes"),
        at.iter().max().copied().expect("closes"),
    );
    assert!(
        last - first >= SPREAD / 2,
        "forty closes landed within {:?} of each other",
        last - first
    );
    assert!(
        last <= SPREAD + Duration::from_millis(500),
        "the last close came {last:?} after the stop, past the {SPREAD:?} spread"
    );
}

#[tokio::test]
async fn a_client_that_keeps_sending_during_the_drain_is_still_told_1001() {
    let (listener, stats) = serve(public(), Work::new().router()).await;
    let socket = connect(&listener).await;
    until(|| stats.public() == 1, "the connection admitted").await;
    let (mut sink, mut stream) = socket.split();
    let chatter = tokio::spawn(async move {
        // Unread data at close is what turns a close into a reset.
        let noop = json!({ "action": "noop", "data": {} }).to_string();
        while sink.send(Message::text(noop.clone())).await.is_ok() {
            tokio::task::yield_now().await;
        }
    });

    let stopping = tokio::spawn(listener.stop());
    let closed = loop {
        let frame = timeout(PATIENCE, stream.next())
            .await
            .expect("a frame in time")
            .expect("a close frame before the end")
            .expect("a close frame, not a reset");
        if let Message::Close(Some(frame)) = frame {
            break (u16::from(frame.code), frame.reason.to_string());
        }
    };
    assert_eq!(closed, (1001, "going_away".to_string()));
    // On loopback the reset arrives after the close frame, so the assertion is that no reset is sent.
    let after = timeout(PATIENCE, stream.next())
        .await
        .expect("the end in time");
    if let Some(Err(TungsteniteError::Io(error))) = &after {
        assert_ne!(
            error.kind(),
            ErrorKind::ConnectionReset,
            "the server reset the connection with the client's frames unread"
        );
    }
    chatter.abort();
    let started = Instant::now();
    timeout(PATIENCE, stopping)
        .await
        .expect("the stop finished in time")
        .expect("not panicked")
        .expect("the server drains");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the server waited out its grace instead of taking the client's close"
    );
}
