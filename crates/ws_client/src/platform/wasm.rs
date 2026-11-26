//! The browser transport: a client connection over `web_sys::WebSocket`.
//!
//! Callback-driven and single-threaded: the handlers live as long as the connection,
//! and deadlines race a `gloo-timers` sleep.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fundamental_ws_wire::WsEnvelope;
use futures::channel::oneshot;
use futures::future::{Either, select};
use futures::pin_mut;
use gloo_timers::future::sleep;
use serde_json::Value;
use tracing::{debug, info, warn};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::TransportError;
use crate::connect_failure::ConnectFailure;
use crate::connection::{
    CONNECT_DEADLINE, Inbound, LiveReceiver, LiveSender, REQUEST_DEADLINE, Transport, classify,
    encode, route_push,
};
use crate::reconnect_hint::ReconnectHint;

type Pending = Rc<RefCell<HashMap<u64, oneshot::Sender<Result<Value, TransportError>>>>>;
type OpenSlot = Rc<RefCell<Option<oneshot::Sender<Result<(), TransportError>>>>>;

/// A browser WebSocket transport; the handler closures live as long as the connection.
pub(crate) struct PlatformTransport {
    socket: WebSocket,
    pending: Pending,
    next_id: AtomicU64,
    timeout: Duration,
    live_rx: RefCell<Option<LiveReceiver>>,
    close_code: Rc<Cell<Option<u16>>>,
    lost: Rc<Cell<bool>>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
}

impl PlatformTransport {
    /// Opens a connection to `url` within [`CONNECT_DEADLINE`]; the browser never exposes a
    /// refused upgrade's response, so a failure carries no retry hint.
    /// # Errors
    /// Returns [`TransportError::Connect`] if the socket cannot be created, closes or
    /// errors before it opens, or does not open within the deadline.
    pub(crate) async fn connect(url: &str) -> Result<Self, ConnectFailure> {
        Self::connect_within(url, CONNECT_DEADLINE).await
    }

    async fn connect_within(url: &str, deadline: Duration) -> Result<Self, ConnectFailure> {
        let socket = WebSocket::new(url).map_err(|e| TransportError::Connect(js_error(&e)))?;
        let pending: Pending = Rc::new(RefCell::new(HashMap::new()));

        let (open_tx, open_rx) = oneshot::channel::<Result<(), TransportError>>();
        let open_slot: OpenSlot = Rc::new(RefCell::new(Some(open_tx)));

        let (live_tx, live_rx): (LiveSender, LiveReceiver) = futures::channel::mpsc::unbounded();
        let close_code = Rc::new(Cell::new(None));
        let lost = Rc::new(Cell::new(false));

        let message_pending = pending.clone();
        let message_live = live_tx.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Some(text) = event.data().as_string() {
                dispatch(&message_pending, &message_live, &text);
            }
        });
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let open_for_open = open_slot.clone();
        let on_open = Closure::<dyn FnMut()>::new(move || {
            if let Some(tx) = open_for_open.borrow_mut().take() {
                let _ = tx.send(Ok(()));
            }
        });
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        let close_pending = pending.clone();
        let close_open = open_slot.clone();
        let close_live = live_tx.clone();
        let close_slot = close_code.clone();
        let close_lost = lost.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |event: CloseEvent| {
            let code = event.code();
            info!(code, reason = %event.reason(), "server closed the connection");
            close_slot.set(Some(code));
            close_lost.set(true);
            if let Some(tx) = close_open.borrow_mut().take() {
                let _ = tx.send(Err(TransportError::Connect(
                    "closed before open".to_string(),
                )));
            }
            fail_all(&close_pending);
            // Ending the live stream is how the session learns the connection is gone.
            close_live.close_channel();
        });
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let error_pending = pending.clone();
        let error_open = open_slot.clone();
        let error_live = live_tx;
        let error_lost = lost.clone();
        let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |_event: ErrorEvent| {
            if let Some(tx) = error_open.borrow_mut().take() {
                let _ = tx.send(Err(TransportError::Connect(
                    "error before open".to_string(),
                )));
            }
            error_lost.set(true);
            fail_all(&error_pending);
            error_live.close_channel();
        });
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        let timer = sleep(deadline);
        pin_mut!(timer);
        let opened = match select(open_rx, timer).await {
            Either::Left((Ok(Ok(())), _)) => Ok(()),
            Either::Left((Ok(Err(error)), _)) => Err(error),
            Either::Left((Err(_canceled), _)) => Err(TransportError::Closed),
            Either::Right(((), _)) => {
                warn!(
                    url,
                    deadline_ms = deadline.as_millis(),
                    "websocket connect deadline passed"
                );
                Err(TransportError::Connect(format!(
                    "no handshake within {} ms",
                    deadline.as_millis()
                )))
            }
        };
        if let Err(error) = opened {
            // Detach first so the browser never calls into a freed closure.
            detach(&socket);
            return Err(error.into());
        }

        Ok(Self {
            socket,
            pending,
            next_id: AtomicU64::new(1),
            timeout: REQUEST_DEADLINE,
            live_rx: RefCell::new(Some(live_rx)),
            close_code,
            lost,
            _on_message: on_message,
            _on_open: on_open,
            _on_close: on_close,
            _on_error: on_error,
        })
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
        self.pending.borrow_mut().insert(request_id, tx);
        if self.socket.send_with_str(&text).is_err() {
            self.pending.borrow_mut().remove(&request_id);
            return Err(TransportError::Closed);
        }
        let timer = sleep(self.timeout);
        pin_mut!(timer);
        let result = match select(rx, timer).await {
            Either::Left((Ok(result), _)) => result,
            Either::Left((Err(_canceled), _)) => Err(TransportError::Closed),
            Either::Right(((), _)) => {
                self.pending.borrow_mut().remove(&request_id);
                Err(TransportError::Timeout)
            }
        };
        if matches!(
            result,
            Err(TransportError::Closed | TransportError::Timeout)
        ) {
            self.lost.set(true);
        }
        result
    }

    fn take_live_changes(&self) -> Option<LiveReceiver> {
        self.live_rx.borrow_mut().take()
    }

    fn close_hint(&self) -> Option<ReconnectHint> {
        self.close_code
            .get()
            .and_then(ReconnectHint::from_close_code)
    }

    fn set_request_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    fn is_lost(&self) -> bool {
        self.lost.get()
    }
}

impl Drop for PlatformTransport {
    fn drop(&mut self) {
        detach(&self.socket);
    }
}

/// Unhooks every handler and closes the socket.
fn detach(socket: &WebSocket) {
    socket.set_onmessage(None);
    socket.set_onopen(None);
    socket.set_onclose(None);
    socket.set_onerror(None);
    if let Err(error) = socket.close() {
        debug!(error = %js_error(&error), "closing the websocket failed");
    }
}

/// Resolves the request an inbound frame names, or fails all on a connection-level error.
fn dispatch(pending: &Pending, live: &LiveSender, text: &str) {
    match classify(text) {
        Inbound::Response { request_id, result } => {
            let entry = pending.borrow_mut().remove(&request_id);
            match entry {
                Some(tx) => {
                    let _ = tx.send(result);
                }
                None => warn!(request_id, "reply for unknown request id; dropped"),
            }
        }
        Inbound::Push { action, data } => route_push(live, &action, data),
        Inbound::Fatal(error) => {
            warn!(error = %error, "server reported a connection-level error");
            fail_all(pending);
        }
        Inbound::Ignored => {}
    }
}

/// Fails every pending request with [`TransportError::Closed`].
fn fail_all(pending: &Pending) {
    let drained: Vec<_> = pending.borrow_mut().drain().map(|(_, tx)| tx).collect();
    if drained.is_empty() {
        return;
    }
    warn!(
        count = drained.len(),
        "connection error; failing pending requests"
    );
    for tx in drained {
        let _ = tx.send(Err(TransportError::Closed));
    }
}

fn js_error(value: &JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "JavaScript WebSocket error".to_string())
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    // A wasm test cannot open a native socket, so the hub runs in the Node runner.
    // The accept key's SHA-1 is fixed by RFC 6455 §4.2.2.
    #[wasm_bindgen(inline_js = r#"
        const net = process.getBuiltinModule('node:net');
        const crypto = process.getBuiltinModule('node:crypto');
        const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
        export function serve(mode) {
            return new Promise((resolve) => {
                const server = net.createServer((sock) => {
                    sock.on('error', () => {});
                    if (mode === 'silent-tcp') return;
                    let head = '';
                    sock.on('data', (chunk) => {
                        if (head === null) {
                            if (mode === 'close-1013') sock.end();
                            return;
                        }
                        head += chunk.toString('latin1');
                        if (head.indexOf('\r\n\r\n') < 0) return;
                        const key = /Sec-WebSocket-Key: *([^\r\n]+)/i.exec(head)[1].trim();
                        head = null;
                        const accept = crypto.createHash('sha1').update(key + GUID).digest('base64');
                        sock.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n' +
                            'Connection: Upgrade\r\nSec-WebSocket-Accept: ' + accept + '\r\n\r\n');
                        if (mode === 'close-1013') sock.write(Buffer.from([0x88, 0x02, 0x03, 0xF5]));
                    });
                });
                server.listen(0, '127.0.0.1', () => {
                    server.unref();
                    resolve(server.address().port);
                });
            });
        }
    "#)]
    extern "C" {
        async fn serve(mode: &str) -> JsValue;
    }

    async fn hub(mode: &str) -> String {
        let port = serve(mode).await.as_f64().unwrap_or_default();
        format!("ws://127.0.0.1:{port}")
    }

    #[wasm_bindgen_test]
    async fn an_upgrade_that_never_answers_meets_the_connect_deadline() {
        let url = hub("silent-tcp").await;
        let result = PlatformTransport::connect_within(&url, Duration::from_millis(300)).await;
        let Err(failure) = result else {
            panic!("a silent upgrade never connects");
        };
        assert!(matches!(failure.error, TransportError::Connect(_)));
        assert_eq!(
            failure.hint, None,
            "the browser never sees a refusal's hint"
        );
    }

    #[wasm_bindgen_test]
    async fn a_request_nobody_answers_meets_its_deadline() {
        let url = hub("silent-ws").await;
        let Ok(mut transport) = PlatformTransport::connect(&url).await else {
            panic!("the silent hub completes the upgrade");
        };
        transport.set_request_timeout(Duration::from_millis(300));
        let id = transport.next_request_id();
        let result = transport
            .request(id, WsEnvelope::new("slice.pull", Value::Null))
            .await;
        assert!(
            matches!(result, Err(TransportError::Timeout)),
            "got {result:?}"
        );
        assert!(
            transport.pending.borrow().is_empty(),
            "the timed-out request is forgotten"
        );
        assert!(
            transport.is_lost(),
            "an unanswered request marks the connection lost"
        );
    }

    #[wasm_bindgen_test]
    async fn a_1013_close_ends_the_live_stream_and_is_read_as_try_again_later() {
        let url = hub("close-1013").await;
        let Ok(transport) = PlatformTransport::connect(&url).await else {
            panic!("the closing hub completes the upgrade");
        };
        let Some(mut live) = transport.take_live_changes() else {
            panic!("the live stream is available once");
        };
        let next = live.next();
        let limit = sleep(Duration::from_secs(5));
        pin_mut!(limit);
        match select(next, limit).await {
            Either::Left((change, _)) => {
                assert!(change.is_none(), "a closed socket ends the live stream");
            }
            Either::Right(((), _)) => panic!("a closed socket left the live stream open"),
        }
        assert_eq!(transport.close_hint(), Some(ReconnectHint::TryAgainLater));
    }
}
