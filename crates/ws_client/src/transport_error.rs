//! Error type for the WebSocket transport client.

use fundamental_core::AnyError;
use fundamental_macros::Error;

/// Errors raised by the device-side WebSocket transport, folded into
/// [`SyncError`](fundamental_sync::SyncError) at the engine boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TransportError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// The initial connection to the server could not be established.
    #[error(
        code = "WSC_001",
        message = "WebSocket connect failed: {0}",
        status = 503,
        retryable
    )]
    Connect(String),

    /// The connection closed before the request completed.
    #[error(
        code = "WSC_002",
        message = "WebSocket connection closed",
        status = 503,
        retryable
    )]
    Closed,

    /// The server returned an error, or a frame the client could not read as a reply.
    #[error(code = "WSC_003", message = "Sync protocol error: {0}", status = 502)]
    Protocol(String),

    /// A request did not receive a reply within the configured timeout.
    #[error(
        code = "WSC_004",
        message = "Sync request timed out",
        status = 504,
        retryable
    )]
    Timeout,

    /// A message could not be encoded to or decoded from JSON.
    #[error(
        code = "WSC_005",
        message = "Sync message codec error: {0}",
        status = 500
    )]
    Codec(String),
}
