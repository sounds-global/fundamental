//! [`read_loop`]: routes each inbound frame to its handler until the connection must end.

use std::time::Duration;

use fundamental_core::Error as _;
use futures::StreamExt;
use futures::stream::SplitStream;
use tokio::net::TcpStream;
use tokio::time::{Instant, sleep_until};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tokio_tungstenite::tungstenite::{Error as TungsteniteError, Message};
use tracing::{Instrument, Span, debug, error, field, info_span, warn};

use fundamental_ws_wire::WsEnvelope;

use crate::context::WsContext;
use crate::error::WsError;
use crate::observability::{FixedWindow, millis};
use crate::routing::WsRouter;
use crate::session::CloseReason;

const PING_WINDOW: Duration = Duration::from_secs(60);
/// A keep-alive every two seconds; every ping is answered with a pong, so pings must be bounded.
const PINGS_PER_WINDOW: u64 = 30;

/// Reads frames and dispatches each to `router`, one at a time, returning why the connection must end.
///
/// The slot is checked after every frame, so a refusal reply is queued before the close; waiting for
/// the next frame is cancel-safe because the stream keeps a partly read frame.
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub(crate) async fn read_loop(
    source: &mut SplitStream<WebSocketStream<TcpStream>>,
    router: &WsRouter,
    context: &WsContext,
) -> CloseReason {
    let cell = context.cell();
    let limits = cell.control().limits();
    let mut received: u64 = 0;
    let mut pings = FixedWindow::new(Instant::now(), PING_WINDOW, PINGS_PER_WINDOW);
    let draining = cell.control().drain().begun();
    tokio::pin!(draining);
    loop {
        let frame = tokio::select! {
            biased;
            () = &mut draining => {
                let turn = cell.control().drain().turn(context.connection_id());
                debug!(
                    in_ms = millis(turn.saturating_duration_since(Instant::now())),
                    "the server is draining; closing the connection at its turn"
                );
                sleep_until(turn).await;
                return CloseReason::GoingAway;
            }
            frame = source.next() => frame,
        };
        let Some(frame) = frame else { break };
        match frame {
            Ok(Message::Text(text)) => {
                received += 1;
                let oversized = limits.is_some_and(|limits| {
                    text.len() > limits.max_frame_size && cell.is_anonymous()
                });
                if oversized {
                    refuse(context, received, &WsError::FrameTooLarge).await;
                } else {
                    handle_text(text.as_str(), received, router, context).await;
                }
            }
            Ok(Message::Binary(_)) => {
                received += 1;
                refuse(context, received, &WsError::BinaryUnsupported).await;
            }
            Ok(Message::Close(_)) => {
                debug!("client closed the connection");
                return CloseReason::ClientClosed;
            }
            Ok(Message::Ping(_)) => {
                if !pings.permit(Instant::now()) {
                    if context.count_refusal() {
                        warn!(
                            limit = PINGS_PER_WINDOW,
                            window_ms = millis(PING_WINDOW),
                            "pings above the allowance; closing the connection"
                        );
                    }
                    return CloseReason::Refused;
                }
            }
            Ok(_) => {}
            Err(TungsteniteError::Protocol(ProtocolError::ResetWithoutClosingHandshake)) => {
                debug!("the peer went away without a close frame");
                return CloseReason::ClientClosed;
            }
            Err(error) => {
                debug!(%error, "error reading frame; closing connection");
                return CloseReason::TransportError;
            }
        }
        if let Some(reason) = cell.ending() {
            return reason;
        }
        if let Some(limits) = limits
            && cell.is_anonymous()
            && cell.refusals().refused() >= limits.max_refusals
        {
            return CloseReason::Refused;
        }
    }
    debug!("the peer went away without a close frame");
    CloseReason::ClientClosed
}

/// Parses, routes and replies with an error envelope on failure, inside a `ws.message` span that
/// records nothing the client wrote.
async fn handle_text(text: &str, frame: u64, router: &WsRouter, context: &WsContext) {
    let span = info_span!(
        "ws.message",
        connection_id = %context.connection_id(),
        frame,
        action = field::Empty,
    );
    async {
        let envelope: WsEnvelope = match serde_json::from_str(text) {
            Ok(envelope) => envelope,
            Err(e) => {
                let error = WsError::invalid_message(&e);
                if context.count_refusal() {
                    warn!(error = %error, error.code = error.error_code(), "discarding malformed frame");
                }
                reply_error(context, &error).await;
                return;
            }
        };
        Span::current().record(
            "action",
            router.declared(&envelope.action).unwrap_or("unknown"),
        );
        match router
            .dispatch(&envelope.action, envelope.data, context.clone())
            .await
        {
            Ok(()) => debug!("handled"),
            // The client's share of a full server, not a handler fault.
            Err(e) if e.http_status_code() >= 500 && !matches!(e, WsError::Saturated) => {
                error!(error = %e, error.code = e.error_code(), "handler failed");
                reply_error(context, &e).await;
            }
            Err(e) => {
                if context.count_refusal() {
                    warn!(error = %e, error.code = e.error_code(), "frame refused");
                }
                reply_error(context, &e).await;
            }
        }
    }
    .instrument(span)
    .await;
}

async fn refuse(context: &WsContext, frame: u64, error: &WsError) {
    if context.count_refusal() {
        warn!(
            frame,
            error = %error,
            error.code = error.error_code(),
            "frame refused unparsed"
        );
    }
    reply_error(context, error).await;
}

/// Sends a `{ "error": { code, message } }` envelope back to the client.
async fn reply_error(context: &WsContext, error: &WsError) {
    let body = serde_json::json!({
        "error": { "code": error.error_code(), "message": error.to_string() }
    });
    if let Err(e) = context.send(body).await {
        debug!(error = %e, "error reply not sent; the connection is closing");
    }
}
