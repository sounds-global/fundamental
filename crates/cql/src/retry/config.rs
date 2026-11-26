//! Tunable retry policy for YCQL operations.

use std::time::Duration;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not including the initial attempt).
    pub max_retries: u32,

    /// Base delay between retries (before exponential backoff).
    pub base_delay: Duration,

    /// Maximum delay between retries (cap for exponential backoff).
    pub max_delay: Duration,

    /// Jitter factor (0.0 to 1.0) to randomize delays and prevent thundering herd.
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_secs(1),
            jitter_factor: 0.25,
        }
    }
}

impl RetryConfig {
    /// Creates a new retry configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of retries.
    pub fn max_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    /// Sets the base delay between retries.
    pub fn base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Sets the maximum delay between retries.
    pub fn max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Sets the jitter factor (0.0 to 1.0).
    pub fn jitter_factor(mut self, factor: f64) -> Self {
        self.jitter_factor = factor.clamp(0.0, 1.0);
        self
    }

    /// Creates a configuration optimized for low-latency operations.
    pub fn low_latency() -> Self {
        Self {
            max_retries: 2,
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(100),
            jitter_factor: 0.1,
        }
    }

    /// Creates a configuration optimized for high-reliability operations.
    pub fn high_reliability() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.25,
        }
    }
}
