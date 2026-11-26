//! One connection's session: the handshake, then its frames until a bound or its peer ends it.

use std::sync::Arc;
use std::time::Duration;

use fundamental_core::GeneratedId;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{self, Receiver};
use tokio::time::{Instant, sleep_until, timeout};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::accept_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tracing::{Instrument, debug, info, info_span, warn};

use crate::config::WsConfig;
use crate::context::WsContext;
use crate::routing::WsRouter;
use crate::session::read_loop::read_loop;
use crate::session::write_loop::write_loop;
use crate::session::{CloseReason, ConnectionId};
use crate::slots::{SlotCell, SlotGuard, instant_after};

/// How long each step of a goodbye may take: the flush, the close frame, the wait for the peer's close.
pub(crate) const CLOSE_GRACE: Duration = Duration::from_secs(1);
/// An over-share connection is upgraded only to hear when to come back, so its upgrade must not hold that pool.
const OVER_SHARE_HANDSHAKE: Duration = Duration::from_secs(1);

type Sink = SplitSink<WebSocketStream<TcpStream>, Message>;
type Source = SplitStream<WebSocketStream<TcpStream>>;

/// Serves one accepted connection; the guard is owned here so the slot goes back however it ends.
pub(crate) async fn run(
    stream: TcpStream,
    router: Arc<WsRouter>,
    config: Arc<WsConfig>,
    mut guard: SlotGuard,
) {
    let connection_id = ConnectionId::generate();
    let span = info_span!("ws.connection", connection_id = %connection_id);
    let cell = Arc::clone(guard.cell());
    let (reason, closed) = serve(stream, &router, &config, &cell, connection_id)
        .instrument(span.clone())
        .await;
    span.in_scope(|| record_close(&cell, reason));
    guard.ended(reason);
    let lingering = closed.and_then(|socket| Some((socket, cell.control().linger_permit()?)));
    // The slot goes back before waiting for the peer's close, so a silent client holds no admission.
    drop(guard);
    drop(cell);
    match lingering {
        Some(((sink, mut source), _permit)) => {
            await_peer_close(&mut source, reason).instrument(span).await;
            drop(sink);
        }
        None => span.in_scope(|| {
            debug!(
                reason = reason.label(),
                "closing without waiting for the peer"
            )
        }),
    }
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn serve(
    stream: TcpStream,
    router: &WsRouter,
    config: &WsConfig,
    cell: &Arc<SlotCell>,
    connection_id: ConnectionId,
) -> (CloseReason, Option<(Sink, Source)>) {
    let handshake = tokio::select! {
        biased;
        () = cell.changed() => return (cell.ending().unwrap_or(CloseReason::Displaced), None),
        handshake = timeout(
            handshake_deadline(config, cell),
            accept_async_with_config(stream, Some(bounds(config))),
        ) => handshake,
    };
    let websocket = match handshake {
        Ok(Ok(websocket)) => websocket,
        Ok(Err(error)) => {
            if cell.count_refusal() {
                warn!(%error, "websocket handshake failed");
            }
            return (CloseReason::HandshakeRejected, None);
        }
        Err(_elapsed) => return (CloseReason::HandshakeTimeout, None),
    };
    debug!("connection opened");

    let (mut sink, mut source) = websocket.split();
    let (outbound, mut inbox) = mpsc::channel::<Message>(config.send_buffer);
    let context = WsContext::new(connection_id, outbound, Arc::default(), Arc::clone(cell));
    let reason = {
        let reader = read_loop(&mut source, router, &context);
        let writer = write_loop(&mut sink, &mut inbox, config.send_timeout);
        tokio::pin!(reader, writer);
        let within = cell.control().limits().map(|limits| limits.within);
        let deadline = sleep_until(instant_after(
            cell.admitted_at(),
            within.unwrap_or(Duration::ZERO),
        ));
        tokio::pin!(deadline);
        let mut deadline_armed = within.is_some();
        loop {
            if let Some(reason) = cell.ending() {
                break reason;
            }
            tokio::select! {
                biased;
                () = cell.changed() => {}
                () = &mut deadline, if deadline_armed => {
                    if cell.is_anonymous() {
                        break CloseReason::AuthDeadline;
                    }
                    deadline_armed = false;
                }
                reason = &mut writer => break reason,
                reason = &mut reader => break reason,
            }
        }
    };
    drop(context);
    let told = say_goodbye(&mut sink, &mut inbox, reason, cell).await;
    (reason, told.then_some((sink, source)))
}

fn handshake_deadline(config: &WsConfig, cell: &SlotCell) -> Duration {
    if cell.ending() == Some(CloseReason::OverShare) {
        config.handshake_timeout.min(OVER_SHARE_HANDSHAKE)
    } else {
        config.handshake_timeout
    }
}

/// Closes the queue, flushes it, then sends the close frame, each within [`CLOSE_GRACE`]; returns whether
/// the frame was written. The queue closes first so no live sender can keep it full.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
async fn say_goodbye(
    sink: &mut Sink,
    inbox: &mut Receiver<Message>,
    reason: CloseReason,
    cell: &SlotCell,
) -> bool {
    inbox.close();
    if !reason.sends_close_frame() {
        return false;
    }
    let label = reason.label();
    if !matches!(reason, CloseReason::SlowPeer | CloseReason::TransportError) {
        let flush = async {
            while let Ok(message) = inbox.try_recv() {
                sink.feed(message).await?;
            }
            sink.flush().await
        };
        match timeout(CLOSE_GRACE, flush).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                debug!(%error, reason = label, "queued replies not delivered");
                return false;
            }
            Err(_elapsed) => debug!(reason = label, "queued replies not taken within the grace"),
        }
    }
    let frame = reason.close_frame(cell.control().retry_after(reason));
    match timeout(CLOSE_GRACE, sink.send(Message::Close(frame))).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            debug!(%error, reason = label, "close frame not delivered");
            false
        }
        Err(_elapsed) => {
            debug!(reason = label, "close frame not taken within the grace");
            false
        }
    }
}

/// Drains reads until the peer's close, within [`CLOSE_GRACE`]: closing with unread data sends a reset
/// that discards the close frame.
async fn await_peer_close(source: &mut Source, reason: CloseReason) {
    let peer_closed = async {
        while let Some(Ok(frame)) = source.next().await {
            if matches!(frame, Message::Close(_)) {
                break;
            }
        }
    };
    if timeout(CLOSE_GRACE, peer_closed).await.is_err() {
        debug!(
            reason = reason.label(),
            "the peer did not close within the grace"
        );
    }
}

/// Writes the connection's one close line, at `debug` for one that never authenticated.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn record_close(cell: &SlotCell, reason: CloseReason) {
    let lived_ms = u64::try_from(
        Instant::now()
            .saturating_duration_since(cell.admitted_at())
            .as_millis(),
    )
    .unwrap_or(u64::MAX);
    let refusals = cell.refusals().refused();
    let refusals_suppressed = cell.refusals().suppressed();
    if cell.has_principal() || cell.control().limits().is_none() {
        info!(
            reason = reason.label(),
            code = reason.code(),
            lived_ms,
            refusals,
            refusals_suppressed,
            "connection closed"
        );
    } else {
        debug!(
            reason = reason.label(),
            code = reason.code(),
            lived_ms,
            refusals,
            refusals_suppressed,
            "anonymous connection closed"
        );
    }
}

/// The protocol's limits; a reassembled message is held to the frame size, not the library's 64 MiB.
fn bounds(config: &WsConfig) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_frame_size(Some(config.max_frame_size))
        .max_message_size(Some(config.max_frame_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_held_to_the_frame_size() {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr")).max_frame_size(4096);
        let bounds = bounds(&config);
        assert_eq!(bounds.max_frame_size, Some(4096));
        assert_eq!(bounds.max_message_size, Some(4096));
    }
}
