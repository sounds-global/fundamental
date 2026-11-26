//! The crate-internal [`CircuitInternals`] state guarded by the breaker's lock.

use std::time::Instant;

use crate::CircuitState;

/// Internal state tracking.
pub(crate) struct CircuitInternals {
    pub state: CircuitState,
    pub opened_at: Option<Instant>,
    /// Timestamps of recent failures within the window.
    pub failure_times: Vec<Instant>,
    /// Consecutive successes in half-open state.
    pub half_open_successes: u32,
}
