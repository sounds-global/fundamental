//! Reasons a pooled connection is closed.

/// Reasons for closing a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// Connection exceeded max lifetime.
    MaxLifetimeExceeded,
    /// Connection exceeded idle timeout.
    IdleTimeout,
    /// Connection health check failed.
    HealthCheckFailed,
    /// Connection was explicitly closed.
    Explicit,
    /// Pool was shut down.
    PoolShutdown,
    /// Connection error.
    Error,
}
