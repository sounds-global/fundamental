//! [`DrainState`] — where a server is in stopping.

/// Where a server is in stopping. It only moves forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DrainState {
    /// Accepting connections and dispatching frames.
    Serving,
    /// Stopped accepting; every session ends at its next frame boundary with `1001`.
    Draining,
    /// The drain outlived `drain_timeout`, and the sessions still running are being aborted.
    Expired,
}
