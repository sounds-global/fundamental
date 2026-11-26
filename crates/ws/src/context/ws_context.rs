//! [`WsContext`]: what a handler receives alongside its typed payload.

use std::any::Any;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;

use crate::context::connection_state::ConnectionState;
use crate::error::{WsError, WsResult};
use crate::session::ConnectionId;
use crate::slots::SlotCell;

/// The per-message context handed to a handler: its connection, a reply channel, per-connection
/// state and the connection's admission, shared by every clone.
#[derive(Debug, Clone)]
pub struct WsContext {
    connection_id: ConnectionId,
    outbound: Sender<Message>,
    state: Arc<ConnectionState>,
    cell: Arc<SlotCell>,
}

impl WsContext {
    pub(crate) fn new(
        connection_id: ConnectionId,
        outbound: Sender<Message>,
        state: Arc<ConnectionState>,
        cell: Arc<SlotCell>,
    ) -> Self {
        Self {
            connection_id,
            outbound,
            state,
            cell,
        }
    }

    /// The id of the connection this message arrived on.
    #[must_use]
    pub fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    /// Stores a value on the connection, readable by every later message on it.
    ///
    /// # Errors
    /// Returns [`WsError::Internal`] if connection state has already been set.
    pub fn set_state<T: Any + Send + Sync>(&self, value: T) -> WsResult<()> {
        self.state
            .set(value)
            .map_err(|_| WsError::internal("connection state already set"))
    }

    /// The connection's stored value, if present and of type `T`.
    #[must_use]
    pub fn state<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.state.get::<T>()
    }

    /// Binds the principal a handler has verified to this connection, admitting it.
    ///
    /// `holder` is what every connection of one principal shares; it is never logged.
    /// # Errors
    /// - [`WsError::AlreadyAuthenticated`] if a principal is already bound.
    /// - [`WsError::PrincipalLimit`] if `holder` already holds its most connections; the
    ///   connection then closes with `1008`.
    /// - [`WsError::Saturated`] if no authenticated slot is free; the connection then closes
    ///   with `1013` once the handler returns, so reply before returning.
    /// - [`WsError::Closed`] if the connection is already ending.
    pub fn bind_principal<P: Any + Send + Sync>(&self, holder: &str, principal: P) -> WsResult<()> {
        self.cell.bind(holder, principal)
    }

    /// The principal bound to this connection, if one is and it is of type `P`.
    #[must_use]
    pub fn principal<P: Any + Send + Sync>(&self) -> Option<&P> {
        self.cell.principal::<P>()
    }

    /// Whether a principal has been bound to this connection.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.cell.has_principal()
    }

    /// Counts one refused frame, returning whether its warning may be written.
    #[must_use = "a refusal's warning is written only when this returns true"]
    pub fn count_refusal(&self) -> bool {
        self.cell.count_refusal()
    }

    /// Sends `message` back on this connection as a JSON text frame, awaiting a full buffer.
    /// # Errors
    /// Returns [`WsError::Serialization`] if `message` cannot be serialized, or
    /// [`WsError::Closed`] if the connection has gone away.
    pub async fn send(&self, message: impl Serialize) -> WsResult<()> {
        let text = serde_json::to_string(&message)?;
        self.outbound
            .send(Message::text(text))
            .await
            .map_err(|_| WsError::Closed)
    }

    pub(crate) fn cell(&self) -> &SlotCell {
        &self.cell
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::WsConfig;
    use crate::slots::{AdmissionControl, SlotGuard};
    use fundamental_core::GeneratedId;
    use tokio::sync::mpsc::{Receiver, channel};
    use tokio::time::Instant;

    /// A context on a connection admitted to a server that requires authentication, its
    /// outbound frames delivered to the receiver, and the guard that holds its slot.
    pub(crate) async fn anonymous_context() -> (WsContext, Receiver<Message>, SlotGuard) {
        let config = WsConfig::new("127.0.0.1:0".parse().expect("addr"));
        let control = AdmissionControl::new(&config, Instant::now());
        let guard = control.admit().await.expect("admitted");
        let (tx, rx) = channel(8);
        let context = WsContext::new(
            ConnectionId::generate(),
            tx,
            Arc::default(),
            Arc::clone(guard.cell()),
        );
        (context, rx, guard)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::anonymous_context;
    use super::*;
    use fundamental_core::Error as _;

    #[tokio::test]
    async fn binding_a_principal_authenticates_the_connection() {
        let (context, _rx, _guard) = anonymous_context().await;
        assert!(!context.is_authenticated());
        context
            .bind_principal("owner", "owner".to_string())
            .expect("bound");
        assert!(context.is_authenticated());
        assert_eq!(
            context.principal::<String>().map(String::as_str),
            Some("owner")
        );
        assert!(context.principal::<u32>().is_none());
        let again = context
            .bind_principal("other", "other".to_string())
            .expect_err("once");
        assert_eq!(again.error_code(), "WS_009");
    }

    #[tokio::test]
    async fn state_and_principal_are_separate_slots() {
        let (context, _rx, _guard) = anonymous_context().await;
        context.set_state(1u8).expect("state");
        assert!(!context.is_authenticated());
        context.bind_principal("holder", 2u16).expect("bound");
        assert_eq!(context.state::<u8>(), Some(&1));
        assert_eq!(context.principal::<u16>(), Some(&2));
    }

    #[tokio::test]
    async fn a_context_whose_connection_is_gone_cannot_send() {
        let (context, rx, _guard) = anonymous_context().await;
        drop(rx);
        let error = context.send("late").await.expect_err("gone");
        assert!(matches!(error, WsError::Closed));
    }
}
