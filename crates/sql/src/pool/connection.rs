//! Connection leak detection.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::warn;

/// Warns on drop when a connection was held longer than the configured threshold.
#[derive(Debug)]
pub struct ConnectionGuard {
    acquired_at: Instant,
    threshold: Option<Duration>,
    leak_warnings: Arc<AtomicU64>,
}

impl ConnectionGuard {
    /// Creates a new connection guard.
    pub fn new(threshold: Option<Duration>, leak_warnings: Arc<AtomicU64>) -> Self {
        Self {
            acquired_at: Instant::now(),
            threshold,
            leak_warnings,
        }
    }

    /// Returns how long the connection has been held.
    pub fn held_duration(&self) -> Duration {
        self.acquired_at.elapsed()
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Some(threshold) = self.threshold {
            let held = self.acquired_at.elapsed();
            if held > threshold {
                warn!(
                    held_ms = held.as_millis(),
                    threshold_ms = threshold.as_millis(),
                    "Connection held longer than leak detection threshold"
                );
                self.leak_warnings.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
