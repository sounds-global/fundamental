//! [`ConnectFailure`] — a failed connect, with whatever the server said about retrying.

use crate::TransportError;
use crate::reconnect_hint::ReconnectHint;

/// Why a connection could not be opened, and the server's retry hint beside it.
#[derive(Debug)]
pub(crate) struct ConnectFailure {
    /// The failure a caller of the public API sees.
    pub(crate) error: TransportError,
    /// A 503 / 429 refusal's hint, if the server refused the upgrade with one.
    pub(crate) hint: Option<ReconnectHint>,
}

impl From<TransportError> for ConnectFailure {
    fn from(error: TransportError) -> Self {
        Self { error, hint: None }
    }
}
