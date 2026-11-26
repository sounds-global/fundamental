//! Configuration for the circuit breaker.

use std::time::Duration;

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit (default 5).
    pub failure_threshold: u32,

    /// Duration to stay open before testing recovery (default 30 s).
    pub open_duration: Duration,

    /// Number of successes needed to close the circuit from half-open (default 3).
    pub success_threshold: u32,

    /// Time window for counting failures (default 60 s).
    pub failure_window: Duration,

    /// Whether only retryable errors count toward the threshold (default true).
    pub only_track_retryable: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 3,
            failure_window: Duration::from_secs(60),
            only_track_retryable: true,
        }
    }
}

impl CircuitBreakerConfig {
    /// Creates a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the failure threshold.
    pub fn failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    /// Sets the open duration.
    pub fn open_duration(mut self, duration: Duration) -> Self {
        self.open_duration = duration;
        self
    }

    /// Sets the success threshold for half-open state.
    pub fn success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }

    /// Sets the failure window.
    pub fn failure_window(mut self, duration: Duration) -> Self {
        self.failure_window = duration;
        self
    }

    /// Sets whether to only track retryable errors.
    pub fn only_track_retryable(mut self, enabled: bool) -> Self {
        self.only_track_retryable = enabled;
        self
    }

    /// Creates a configuration for high availability: threshold 3, open 10 s, success 2.
    pub fn high_availability() -> Self {
        Self {
            failure_threshold: 3,
            open_duration: Duration::from_secs(10),
            success_threshold: 2,
            failure_window: Duration::from_secs(30),
            only_track_retryable: true,
        }
    }

    /// Creates a configuration more tolerant of transient failures: threshold 10, open 60 s, success 5.
    pub fn tolerant() -> Self {
        Self {
            failure_threshold: 10,
            open_duration: Duration::from_secs(60),
            success_threshold: 5,
            failure_window: Duration::from_secs(120),
            only_track_retryable: true,
        }
    }
}
