//! The transport seam and the shared frame-classification logic.

use std::future::Future;
use std::time::Duration;

use fundamental_sync::wire::SliceChange;
use fundamental_sync::wire::action::SLICE_CHANGE;
use fundamental_ws_wire::WsEnvelope;
use futures::channel::mpsc;
use serde_json::Value;
use tracing::warn;

use crate::TransportError;
use crate::reconnect_hint::ReconnectHint;

/// How long TCP, TLS and the upgrade together may take (gRPC's `MIN_CONNECT_TIMEOUT`):
/// shorter never connects on a weak link; none parks the device on a silent hub.
pub(crate) const CONNECT_DEADLINE: Duration = Duration::from_secs(20);

pub(crate) const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

/// Filled by the receive loop from server pushes.
pub(crate) type LiveSender = mpsc::UnboundedSender<SliceChange>;

pub(crate) type LiveReceiver = mpsc::UnboundedReceiver<SliceChange>;

/// One multiplexed request/response socket that also surfaces server pushes.
///
/// The seam the [`RemoteSource`](fundamental_sync::RemoteSource) impl is written against.
pub(crate) trait Transport {
    /// Returns the next monotonic request id for correlating a reply.
    fn next_request_id(&self) -> u64;

    /// Sends `envelope` and awaits the reply whose `request_id` matches.
    fn request(
        &self,
        request_id: u64,
        envelope: WsEnvelope,
    ) -> impl Future<Output = Result<Value, TransportError>>;

    /// Takes the live-change receiver; available once per connection.
    fn take_live_changes(&self) -> Option<LiveReceiver>;

    /// The retry hint the server's close frame carried.
    fn close_hint(&self) -> Option<ReconnectHint>;

    /// Sets how long each later request waits for its reply.
    fn set_request_timeout(&mut self, timeout: Duration);

    /// Whether the socket closed or a request got no reply in time; an error reply never counts.
    fn is_lost(&self) -> bool;
}

/// The meaning of one inbound frame, as decided by [`classify`].
pub(crate) enum Inbound {
    /// A reply correlated to a pending request.
    Response {
        /// The request id this reply answers.
        request_id: u64,
        /// The reply payload, or a server-reported error.
        result: Result<Value, TransportError>,
    },
    /// An unsolicited server push (a `{ action, data }` envelope), routed by action.
    Push {
        /// The push's action string (e.g. `slice.change`).
        action: String,
        /// The push's payload.
        data: Value,
    },
    /// A connection-level error with no correlation; every pending request fails.
    Fatal(TransportError),
    /// A frame that is not a reply, push, or error; dropped.
    Ignored,
}

/// Classifies one inbound text frame.
///
/// A `request_id` marks a reply; without one, an `action` is a push and an `error` fails
/// every pending request. Anything else is ignored.
pub(crate) fn classify(text: &str) -> Inbound {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Inbound::Ignored;
    };
    if let Some(request_id) = value.get("request_id").and_then(Value::as_u64) {
        if let Some(error) = value.get("error") {
            return Inbound::Response {
                request_id,
                result: Err(TransportError::Protocol(error_message(error))),
            };
        }
        return Inbound::Response {
            request_id,
            result: Ok(value),
        };
    }
    if let Some(action) = value.get("action").and_then(Value::as_str) {
        return Inbound::Push {
            action: action.to_string(),
            data: value.get("data").cloned().unwrap_or(Value::Null),
        };
    }
    if let Some(error) = value.get("error") {
        return Inbound::Fatal(TransportError::Protocol(error_message(error)));
    }
    Inbound::Ignored
}

/// Routes a `slice.change` push to the live channel; a malformed one is dropped.
#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
pub(crate) fn route_push(live: &LiveSender, action: &str, data: Value) {
    if action != SLICE_CHANGE {
        warn!(action, "ignoring an unknown server push");
        return;
    }
    match serde_json::from_value::<SliceChange>(data) {
        Ok(change) => {
            let _ = live.unbounded_send(change);
        }
        Err(error) => warn!(error = %error, "dropping a malformed slice change push"),
    }
}

/// Serializes an outbound envelope, mapping a serde failure to [`TransportError::Codec`].
pub(crate) fn encode(envelope: &WsEnvelope) -> Result<String, TransportError> {
    serde_json::to_string(envelope).map_err(|e| TransportError::Codec(e.to_string()))
}

/// Renders a server error object (`{ code, message }`) into a single string.
fn error_message(error: &Value) -> String {
    let code = error.get("code").and_then(Value::as_str);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unspecified error");
    match code {
        Some(code) => format!("[{code}] {message}"),
        None => message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_a_correlated_success_reply() {
        match classify(r#"{"request_id":7,"records":[],"has_more":false}"#) {
            Inbound::Response { request_id, result } => {
                assert_eq!(request_id, 7);
                assert!(result.is_ok());
            }
            _ => panic!("expected a correlated response"),
        }
    }

    #[test]
    fn classifies_a_correlated_error_reply() {
        match classify(r#"{"request_id":3,"error":{"code":"AUTH_001","message":"no token"}}"#) {
            Inbound::Response { request_id, result } => {
                assert_eq!(request_id, 3);
                let err = result.expect_err("error reply");
                assert!(matches!(err, TransportError::Protocol(_)));
                assert!(err.to_string().contains("AUTH_001"));
            }
            _ => panic!("expected a correlated error response"),
        }
    }

    #[test]
    fn classifies_an_uncorrelated_framework_error_as_fatal() {
        match classify(r#"{"error":{"code":"WS_003","message":"unknown action"}}"#) {
            Inbound::Fatal(err) => assert!(err.to_string().contains("WS_003")),
            _ => panic!("expected a fatal error"),
        }
    }

    #[test]
    fn ignores_a_non_reply_frame() {
        assert!(matches!(classify(r#"{"hello":"world"}"#), Inbound::Ignored));
        assert!(matches!(classify("not json"), Inbound::Ignored));
    }
}
