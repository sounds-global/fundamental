//! One DevTools WebSocket: commands answered by id, events fanned out to every listener.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, warn};

use crate::{CdpEvent, TestBrowserError};

/// How long one command may take before the test gives up on it.
const COMMAND_BUDGET: Duration = Duration::from_secs(30);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, Value>>>>>;

#[derive(Clone)]
pub(crate) struct Connection {
    outgoing: mpsc::UnboundedSender<String>,
    pending: Pending,
    events: broadcast::Sender<CdpEvent>,
    next_id: Arc<AtomicU64>,
}

impl Connection {
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub(crate) async fn open(endpoint: &str) -> Result<Self, TestBrowserError> {
        let (socket, _) = tokio_tungstenite::connect_async(endpoint)
            .await
            .map_err(|error| TestBrowserError::Unavailable {
                detail: format!("DevTools at {endpoint}: {error}"),
            })?;
        let (mut sink, mut stream) = socket.split();
        let (outgoing, mut queue) = mpsc::unbounded_channel::<String>();
        let pending: Pending = Arc::default();
        let (events, _) = broadcast::channel(4096);
        tokio::spawn(async move {
            while let Some(text) = queue.recv().await {
                if let Err(error) = sink.send(Message::Text(text.into())).await {
                    warn!(%error, "DevTools: a command could not be sent");
                    break;
                }
            }
        });
        let reader_pending = Arc::clone(&pending);
        let reader_events = events.clone();
        tokio::spawn(async move {
            while let Some(message) = stream.next().await {
                let text = match message {
                    Ok(Message::Text(text)) => text,
                    Ok(Message::Close(_)) => break,
                    Ok(_) => continue,
                    Err(error) => {
                        warn!(%error, "DevTools: the connection failed");
                        break;
                    }
                };
                let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                    warn!("DevTools: a message was not JSON");
                    continue;
                };
                dispatch(&value, &reader_pending, &reader_events);
            }
            debug!("DevTools: the connection closed");
        });
        Ok(Self {
            outgoing,
            pending,
            events,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub(crate) async fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, TestBrowserError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        let (answer, answered) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| unavailable("the command table is poisoned"))?
            .insert(id, answer);
        self.outgoing
            .send(command.to_string())
            .map_err(|_| unavailable("the connection is closed"))?;
        match tokio::time::timeout(COMMAND_BUDGET, answered).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(TestBrowserError::Protocol {
                method: method.to_owned(),
                detail: error.to_string(),
            }),
            Ok(Err(_)) => Err(unavailable("the connection closed before answering")),
            Err(_) => Err(TestBrowserError::Timeout {
                what: method.to_owned(),
                waited_ms: COMMAND_BUDGET.as_millis().try_into().unwrap_or(u64::MAX),
            }),
        }
    }

    pub(crate) fn events(&self) -> broadcast::Receiver<CdpEvent> {
        self.events.subscribe()
    }
}

fn dispatch(value: &Value, pending: &Pending, events: &broadcast::Sender<CdpEvent>) {
    if let Some(id) = value["id"].as_u64() {
        let answer = match value.get("error") {
            Some(error) => Err(error.clone()),
            None => Ok(value["result"].clone()),
        };
        let waiting = pending.lock().ok().and_then(|mut table| table.remove(&id));
        if let Some(waiting) = waiting
            && waiting.send(answer).is_err()
        {
            debug!(id, "DevTools: an answer arrived after its command gave up");
        }
        return;
    }
    if let Some(method) = value["method"].as_str() {
        // No page listening is not a failure: a page records only what happens while it exists.
        events
            .send(CdpEvent {
                session: value["sessionId"].as_str().map(str::to_owned),
                method: method.to_owned(),
                params: value["params"].clone(),
            })
            .ok();
    }
}

fn unavailable(detail: &str) -> TestBrowserError {
    TestBrowserError::Unavailable {
        detail: detail.to_owned(),
    }
}
