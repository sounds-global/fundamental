//! Observability events emitted by a connection pool.

use std::time::Duration;

use super::CloseReason;

/// Events emitted by the connection pool.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// A connection was acquired from the pool.
    Acquired {
        /// Time spent waiting for the connection.
        wait_time: Duration,
    },
    /// A connection was released back to the pool.
    Released {
        /// Time the connection was held.
        hold_time: Duration,
    },
    /// A new connection was created.
    Created,
    /// A connection was closed.
    Closed {
        /// The reason for closing.
        reason: CloseReason,
    },
    /// The pool timed out waiting for a connection.
    AcquireTimeout,
    /// A connection health check failed.
    HealthCheckFailed,
}
