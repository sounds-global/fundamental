use super::ConnectionState;

/// Emitted on the client's event channel when the connection state changes.
#[derive(Debug, Clone)]
pub struct ConnectionEvent {
    /// The new state.
    pub state: ConnectionState,
    /// The error that drove the change, if any.
    pub error: Option<String>,
}
