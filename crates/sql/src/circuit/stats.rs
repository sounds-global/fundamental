//! Snapshot statistics for the circuit breaker.

use super::CircuitState;

/// Statistics from the circuit breaker.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerStats {
    /// Current state.
    pub state: CircuitState,
    /// Total failures recorded.
    pub total_failures: u64,
    /// Total successes recorded.
    pub total_successes: u64,
    /// Number of times the circuit has opened.
    pub times_opened: u32,
    /// Recent failures within the window.
    pub recent_failures: u32,
    /// Consecutive successes in half-open state.
    pub half_open_successes: u32,
}
