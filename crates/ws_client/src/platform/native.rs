//! The native transport: a client connection over `tokio-tungstenite`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use fundamental_ws_wire::WsEnvelope;
use futures::channel::oneshot;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::http::header::RETRY_AFTER;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tracing::{debug, info, warn};

use crate::TransportError;
use crate::connect_failure::ConnectFailure;
use crate::connection::{
    CONNECT_DEADLINE, Inbound, LiveReceiver, LiveSender, REQUEST_DEADLINE, Transport, classify,
    encode, route_push,
};
use crate::reconnect_hint::ReconnectHint;
use crate::reconnect_schedule::ReconnectSchedule;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, TransportError>>>>>;

/// A native WebSocket transport: a writer task drains the outbound queue and a
/// reader task resolves pending requests as replies arrive.
pub(crate) struct PlatformTransport {
    outbound: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
    timeout: Duration,
    live_rx: Mutex<Option<LiveReceiver>>,
    close_code: Arc<OnceLock<u16>>,
    lost: Arc<AtomicBool>,
    reader: JoinHandle<()>,
    writer: JoinHandle<()>,
}

impl PlatformTransport {
    /// Connects to `url` within [`CONNECT_DEADLINE`], spawning the reader and writer tasks.
    ///
    /// # Errors
    /// Returns [`TransportError::Connect`] if the handshake fails or the deadline
    /// passes, with the refusal's retry hint when the server sent one.
    pub(crate) async fn connect(url: &str) -> Result<Self, ConnectFailure> {
        Self::connect_within(url, CONNECT_DEADLINE).await
    }

    #[expect(
        clippy::too_many_lines,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    async fn connect_within(url: &str, deadline: Duration) -> Result<Self, ConnectFailure> {
        let (stream, _response) = match tokio::time::timeout(deadline, connect_async(url)).await {
            Ok(Ok(connected)) => connected,
            Ok(Err(error)) => return Err(refusal(error)),
            Err(_elapsed) => {
                warn!(
                    url,
                    deadline_ms = deadline.as_millis(),
                    "websocket connect deadline passed"
                );
                return Err(TransportError::Connect(format!(
                    "no handshake within {} ms",
                    deadline.as_millis()
                ))
                .into());
            }
        };
        info!(url, "websocket connected");
        let (mut write, mut read) = stream.split();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (outbound, mut rx) = mpsc::unbounded_channel::<Message>();
        let (live_tx, live_rx): (LiveSender, LiveReceiver) = futures::channel::mpsc::unbounded();
        let close_code = Arc::new(OnceLock::new());
        let lost = Arc::new(AtomicBool::new(false));

        let writer = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(error) = write.send(message).await {
                    debug!(%error, "websocket write failed; stopping the writer");
                    break;
                }
            }
            if let Err(error) = write.close().await {
                debug!(%error, "websocket close after the writer stopped failed");
            }
        });

        let reader_pending = pending.clone();
        let reader_close = close_code.clone();
        let reader_lost = lost.clone();
        let reader = tokio::spawn(async move {
            while let Some(frame) = read.next().await {
                match frame {
                    Ok(Message::Text(text)) => dispatch(&reader_pending, &live_tx, &text),
                    Ok(Message::Close(frame)) => {
                        if let Some(frame) = frame {
                            let code = u16::from(frame.code);
                            info!(
                                code,
                                reason = frame.reason.as_str(),
                                "server closed the connection"
                            );
                            if let Err(code) = reader_close.set(code) {
                                warn!(
                                    code,
                                    "a second close code on one connection; kept the first"
                                );
                            }
                        } else {
                            info!("server closed the connection without a code");
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        info!(%error, "websocket read failed; the connection is lost");
                        break;
                    }
                }
            }
            reader_lost.store(true, Ordering::Release);
            fail_all(&reader_pending, || TransportError::Closed);
        });

        Ok(Self {
            outbound,
            pending,
            next_id: AtomicU64::new(1),
            timeout: REQUEST_DEADLINE,
            live_rx: Mutex::new(Some(live_rx)),
            close_code,
            lost,
            reader,
            writer,
        })
    }

    /// Connects to `url`, retrying on the full-jitter schedule up to `max_attempts` times.
    /// # Errors
    /// Returns the last [`TransportError::Connect`] once the retries are exhausted.
    pub(crate) async fn connect_with_retry(
        url: &str,
        max_attempts: u32,
    ) -> Result<Self, TransportError> {
        let mut schedule = ReconnectSchedule::new();
        let mut failures = 0u32;
        loop {
            match Self::connect(url).await {
                Ok(transport) => return Ok(transport),
                Err(failure) if failures >= max_attempts => return Err(failure.error),
                Err(failure) => {
                    failures += 1;
                    let delay = schedule.after_failed_connect(failure.hint);
                    debug!(
                        error = %failure.error,
                        attempt = failures,
                        delay_ms = delay.as_millis(),
                        hint = ?failure.hint,
                        "connect failed; retrying after backoff"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

impl Transport for PlatformTransport {
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn request(
        &self,
        request_id: u64,
        envelope: WsEnvelope,
    ) -> Result<Value, TransportError> {
        let text = encode(&envelope)?;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(request_id, tx);
        if self.outbound.send(Message::text(text)).is_err() {
            self.pending.lock().remove(&request_id);
            return Err(TransportError::Closed);
        }
        let result = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_canceled)) => Err(TransportError::Closed),
            Err(_elapsed) => {
                self.pending.lock().remove(&request_id);
                Err(TransportError::Timeout)
            }
        };
        if matches!(
            result,
            Err(TransportError::Closed | TransportError::Timeout)
        ) {
            self.lost.store(true, Ordering::Release);
        }
        result
    }

    fn take_live_changes(&self) -> Option<LiveReceiver> {
        self.live_rx.lock().take()
    }

    fn close_hint(&self) -> Option<ReconnectHint> {
        self.close_code
            .get()
            .copied()
            .and_then(ReconnectHint::from_close_code)
    }

    fn set_request_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

impl Drop for PlatformTransport {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}

/// A failed handshake, keeping a refused upgrade's `Retry-After` as a typed hint.
fn refusal(error: WsError) -> ConnectFailure {
    let hint = match &error {
        WsError::Http(response) => {
            let status = response.status().as_u16();
            let retry_after = response.headers().get(RETRY_AFTER).and_then(|value| {
                value
                    .to_str()
                    .inspect_err(|_| {
                        warn!(
                            status,
                            retry_after_len = value.len(),
                            "reconnect: ignoring a Retry-After that is not visible ASCII"
                        );
                    })
                    .ok()
            });
            let hint = ReconnectHint::from_refusal(status, retry_after);
            info!(status, ?hint, "server refused the websocket upgrade");
            hint
        }
        _ => None,
    };
    ConnectFailure {
        error: TransportError::Connect(error.to_string()),
        hint,
    }
}

/// Resolves the request an inbound frame names, or fails all on a connection-level error.
fn dispatch(pending: &Pending, live: &LiveSender, text: &str) {
    match classify(text) {
        Inbound::Response { request_id, result } => {
            if let Some(tx) = pending.lock().remove(&request_id) {
                let _ = tx.send(result);
            } else {
                warn!(request_id, "reply for unknown request id; dropped");
            }
        }
        Inbound::Push { action, data } => route_push(live, &action, data),
        Inbound::Fatal(error) => {
            let message = error.to_string();
            fail_all(pending, move || TransportError::Protocol(message.clone()));
        }
        Inbound::Ignored => {}
    }
}

/// Fails every pending request, each with its own error: the type is not `Clone`.
fn fail_all(pending: &Pending, make: impl Fn() -> TransportError) {
    let drained: Vec<_> = {
        let mut map = pending.lock();
        map.drain().map(|(_, tx)| tx).collect()
    };
    if drained.is_empty() {
        return;
    }
    warn!(
        count = drained.len(),
        "connection error; failing pending requests"
    );
    for tx in drained {
        let _ = tx.send(Err(make()));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    use super::*;

    /// A server that upgrades one connection and closes it with `code`.
    async fn closing_server(code: CloseCode) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut ws) = tokio_tungstenite::accept_async(socket).await else {
                return;
            };
            let frame = CloseFrame {
                code,
                reason: "".into(),
            };
            let _ = ws.close(Some(frame)).await;
            while ws.next().await.is_some() {}
        });
        url
    }

    /// A server that answers the upgrade request with `response` verbatim.
    async fn refusing_server(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        url
    }

    async fn hint_after_close(code: CloseCode) -> Option<ReconnectHint> {
        let url = closing_server(code).await;
        let Ok(transport) = PlatformTransport::connect(&url).await else {
            panic!("the closing server accepts the upgrade");
        };
        let mut live = transport.take_live_changes().expect("live stream");
        let ended = tokio::time::timeout(Duration::from_secs(5), live.next()).await;
        assert!(
            matches!(ended, Ok(None)),
            "a closed connection ends the live stream"
        );
        transport.close_hint()
    }

    #[tokio::test]
    async fn a_1013_close_is_read_as_try_again_later() {
        assert_eq!(
            hint_after_close(CloseCode::Again).await,
            Some(ReconnectHint::TryAgainLater)
        );
    }

    #[tokio::test]
    async fn a_1001_close_is_read_as_going_away() {
        assert_eq!(
            hint_after_close(CloseCode::Away).await,
            Some(ReconnectHint::GoingAway)
        );
    }

    #[tokio::test]
    async fn a_normal_close_carries_no_hint() {
        assert_eq!(hint_after_close(CloseCode::Normal).await, None);
    }

    #[tokio::test]
    async fn a_503_refusal_carries_its_retry_after() {
        let url = refusing_server(
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 42\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        let Err(failure) = PlatformTransport::connect(&url).await else {
            panic!("a 503 is a refusal");
        };
        assert!(matches!(failure.error, TransportError::Connect(_)));
        assert_eq!(
            failure.hint,
            Some(ReconnectHint::RetryAfter(Duration::from_secs(42)))
        );
    }

    #[tokio::test]
    async fn a_refusal_beyond_the_ceiling_is_clamped() {
        let url = refusing_server(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 86400\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        let Err(failure) = PlatformTransport::connect(&url).await else {
            panic!("a 429 is a refusal");
        };
        assert_eq!(
            failure.hint.map(ReconnectHint::floor),
            Some(Duration::from_secs(150))
        );
    }

    #[tokio::test]
    async fn a_403_refusal_carries_no_hint() {
        let url = refusing_server(
            "HTTP/1.1 403 Forbidden\r\nRetry-After: 5\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        let Err(failure) = PlatformTransport::connect(&url).await else {
            panic!("a 403 is a refusal");
        };
        assert_eq!(failure.hint, None);
    }

    /// A server that answers each request with `reply(request_id)`, or never when `None`.
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn answering_server(reply: Option<fn(u64) -> String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut ws) = tokio_tungstenite::accept_async(socket).await else {
                return;
            };
            while let Some(Ok(frame)) = ws.next().await {
                let (Some(reply), Message::Text(text)) = (reply, frame) else {
                    continue;
                };
                let request: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                let id = request["data"]["request_id"].as_u64().unwrap_or_default();
                if ws.send(Message::text(reply(id))).await.is_err() {
                    return;
                }
            }
        });
        url
    }

    fn envelope(id: u64) -> WsEnvelope {
        WsEnvelope::new("slice.pull", serde_json::json!({ "request_id": id }))
    }

    #[tokio::test]
    async fn an_unanswered_request_marks_the_connection_lost() {
        let url = answering_server(None).await;
        let Ok(mut transport) = PlatformTransport::connect(&url).await else {
            panic!("the silent server completes the upgrade");
        };
        transport.set_request_timeout(Duration::from_millis(200));
        assert!(!transport.is_lost());
        let id = transport.next_request_id();
        let result = transport.request(id, envelope(id)).await;
        assert!(
            matches!(result, Err(TransportError::Timeout)),
            "got {result:?}"
        );
        assert!(
            transport.is_lost(),
            "a half-open socket is a lost connection"
        );
    }

    #[tokio::test]
    async fn an_error_reply_leaves_the_connection_live() {
        let url = answering_server(Some(|id| {
            format!(r#"{{"request_id":{id},"error":{{"code":"TEST_001","message":"no"}}}}"#)
        }))
        .await;
        let Ok(transport) = PlatformTransport::connect(&url).await else {
            panic!("the refusing server completes the upgrade");
        };
        let id = transport.next_request_id();
        let result = transport.request(id, envelope(id)).await;
        assert!(
            matches!(result, Err(TransportError::Protocol(_))),
            "got {result:?}"
        );
        assert!(!transport.is_lost(), "an error reply is an answer");
    }

    #[tokio::test]
    async fn an_upgrade_that_never_answers_meets_the_connect_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let url = format!("ws://{}", listener.local_addr().expect("addr"));
        let held = tokio::spawn(async move {
            let accepted = listener.accept().await;
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(accepted);
        });
        let started = Instant::now();
        let result = PlatformTransport::connect_within(&url, Duration::from_millis(300)).await;
        held.abort();
        let Err(failure) = result else {
            panic!("a silent upgrade never connects");
        };
        assert!(matches!(failure.error, TransportError::Connect(_)));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the deadline bounded the wait"
        );
    }
}
