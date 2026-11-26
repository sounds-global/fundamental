//! [`ConnectionStatus`] — where a sync session is.

use web_time::Instant;

use crate::WaitCause;

/// Where a [`SyncClient`](crate::SyncClient)'s session is, as its
/// [`ConnectionWatch`](crate::ConnectionWatch) reports it.
///
/// Nothing the network or a hub does ends it; `Stopped` is reached only through the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Opening the socket, authenticating, catching up and subscribing.
    Connecting,
    /// Connected, subscribed and applying live changes; a pull answered with an error stays `Live`.
    Live,
    /// A failure is being waited out; the next attempt starts at `until`.
    Waiting {
        /// When the next attempt starts.
        until: Instant,
        /// The class of the failure.
        cause: WaitCause,
    },
    /// The app ended the session. Final.
    Stopped,
}
