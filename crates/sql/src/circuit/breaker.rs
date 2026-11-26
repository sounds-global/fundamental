//! Thread-safe [`CircuitBreaker`] implementation.

use super::{
    CircuitBreakerConfig, CircuitBreakerError, CircuitBreakerStats, CircuitInternals, CircuitState,
};
use crate::SqlError;

use parking_lot::RwLock;
use std::future::Future;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use tracing::{debug, info, warn};

/// Circuit breaker for preventing cascading failures.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    internals: RwLock<CircuitInternals>,
    total_failures: AtomicU64,
    total_successes: AtomicU64,
    times_opened: AtomicU32,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let internals = self.internals.read();
        f.debug_struct("CircuitBreaker")
            .field("state", &internals.state)
            .field("config", &self.config)
            .field(
                "total_failures",
                &self.total_failures.load(Ordering::Relaxed),
            )
            .field(
                "total_successes",
                &self.total_successes.load(Ordering::Relaxed),
            )
            .field("times_opened", &self.times_opened.load(Ordering::Relaxed))
            .finish()
    }
}

impl CircuitBreaker {
    /// Creates a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            internals: RwLock::new(CircuitInternals {
                state: CircuitState::Closed,
                opened_at: None,
                failure_times: Vec::new(),
                half_open_successes: 0,
            }),
            total_failures: AtomicU64::new(0),
            total_successes: AtomicU64::new(0),
            times_opened: AtomicU32::new(0),
        }
    }

    /// Returns the current state of the circuit breaker.
    pub fn state(&self) -> CircuitState {
        let mut internals = self.internals.write();
        self.maybe_transition(&mut internals);
        internals.state
    }

    /// Returns true if requests are allowed through.
    pub fn is_allowed(&self) -> bool {
        let state = self.state();
        matches!(state, CircuitState::Closed | CircuitState::HalfOpen)
    }

    /// Records a successful operation.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn record_success(&self) {
        self.total_successes.fetch_add(1, Ordering::Relaxed);

        let mut internals = self.internals.write();

        match internals.state {
            CircuitState::HalfOpen => {
                internals.half_open_successes += 1;
                debug!(
                    successes = internals.half_open_successes,
                    threshold = self.config.success_threshold,
                    "Circuit breaker half-open success"
                );

                if internals.half_open_successes >= self.config.success_threshold {
                    info!("Circuit breaker closing after recovery");
                    internals.state = CircuitState::Closed;
                    internals.failure_times.clear();
                    internals.half_open_successes = 0;
                }
            }
            CircuitState::Closed => {
                let now = Instant::now();
                internals
                    .failure_times
                    .retain(|t| now.duration_since(*t) < self.config.failure_window);
            }
            CircuitState::Open => {}
        }
    }

    /// Records a failed operation; a non-retryable one is not counted when `only_track_retryable` is set.
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn record_failure(&self, is_retryable: bool) {
        self.total_failures.fetch_add(1, Ordering::Relaxed);

        if self.config.only_track_retryable && !is_retryable {
            debug!("Skipping non-retryable failure for circuit breaker");
            return;
        }

        let mut internals = self.internals.write();
        let now = Instant::now();

        match internals.state {
            CircuitState::Closed => {
                internals
                    .failure_times
                    .retain(|t| now.duration_since(*t) < self.config.failure_window);

                internals.failure_times.push(now);

                let failure_count = internals.failure_times.len() as u32;
                debug!(
                    failures = failure_count,
                    threshold = self.config.failure_threshold,
                    "Circuit breaker recorded failure"
                );

                if failure_count >= self.config.failure_threshold {
                    warn!(
                        failures = failure_count,
                        "Circuit breaker opening due to failures"
                    );
                    internals.state = CircuitState::Open;
                    internals.opened_at = Some(now);
                    self.times_opened.fetch_add(1, Ordering::Relaxed);
                }
            }
            CircuitState::HalfOpen => {
                warn!("Circuit breaker reopening after half-open failure");
                internals.state = CircuitState::Open;
                internals.opened_at = Some(now);
                internals.half_open_successes = 0;
                self.times_opened.fetch_add(1, Ordering::Relaxed);
            }
            CircuitState::Open => {
                internals.opened_at = Some(now);
            }
        }
    }

    /// Executes an operation through the circuit breaker, refusing with `CircuitBreakerError::Open` while open.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn call<F, Fut, T>(&self, f: F) -> Result<T, CircuitBreakerError<SqlError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, SqlError>>,
    {
        if !self.is_allowed() {
            debug!("Circuit breaker rejecting request (open)");
            return Err(CircuitBreakerError::Open);
        }

        match f().await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure(e.is_retryable());
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    /// Executes an operation, treating all errors as retryable.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn call_all_retryable<F, Fut, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        if !self.is_allowed() {
            debug!("Circuit breaker rejecting request (open)");
            return Err(CircuitBreakerError::Open);
        }

        match f().await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure(true);
                Err(CircuitBreakerError::Inner(e))
            }
        }
    }

    /// Forces the circuit to open.
    pub fn force_open(&self) {
        let mut internals = self.internals.write();
        if internals.state != CircuitState::Open {
            warn!("Circuit breaker forced open");
            internals.state = CircuitState::Open;
            internals.opened_at = Some(Instant::now());
            self.times_opened.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Forces the circuit to close.
    pub fn force_close(&self) {
        let mut internals = self.internals.write();
        if internals.state != CircuitState::Closed {
            info!("Circuit breaker forced closed");
            internals.state = CircuitState::Closed;
            internals.opened_at = None;
            internals.failure_times.clear();
            internals.half_open_successes = 0;
        }
    }

    /// Resets the circuit breaker to its initial state.
    pub fn reset(&self) {
        let mut internals = self.internals.write();
        internals.state = CircuitState::Closed;
        internals.opened_at = None;
        internals.failure_times.clear();
        internals.half_open_successes = 0;
        info!("Circuit breaker reset");
    }

    /// Returns circuit breaker statistics.
    pub fn stats(&self) -> CircuitBreakerStats {
        let internals = self.internals.read();
        CircuitBreakerStats {
            state: internals.state,
            total_failures: self.total_failures.load(Ordering::Relaxed),
            total_successes: self.total_successes.load(Ordering::Relaxed),
            times_opened: self.times_opened.load(Ordering::Relaxed),
            recent_failures: internals.failure_times.len() as u32,
            half_open_successes: internals.half_open_successes,
        }
    }

    fn maybe_transition(&self, internals: &mut CircuitInternals) {
        if internals.state == CircuitState::Open
            && let Some(opened_at) = internals.opened_at
            && opened_at.elapsed() >= self.config.open_duration
        {
            info!("Circuit breaker transitioning to half-open");
            internals.state = CircuitState::HalfOpen;
            internals.half_open_successes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn opening_the_circuit_emits_a_warn_with_the_failure_count() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            ..CircuitBreakerConfig::default()
        });

        breaker.record_failure(true);
        assert_eq!(breaker.state(), CircuitState::Closed);
        breaker.record_failure(true);

        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(
            logs_contain("Circuit breaker opening due to failures"),
            "crossing the failure threshold must warn"
        );
    }
}
