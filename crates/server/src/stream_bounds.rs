use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::StreamPermit;

/// The limits a [streamed route](crate::Route::streamed) reads its body under, enforced by the backend.
///
/// Clones share one in-flight count.
#[derive(Debug, Clone)]
pub struct StreamBounds {
    max_bytes: u64,
    idle: Duration,
    min_bytes_per_sec: u64,
    grace: Duration,
    max_in_flight: usize,
    deadline: Duration,
    in_flight: Arc<AtomicUsize>,
}

impl StreamBounds {
    /// Bounds for bodies of at most `max_bytes`, with defaults: idle 20 s, 8 KiB/s after 30 s, 64 at once.
    #[must_use]
    pub fn new(max_bytes: u64) -> Self {
        let min_bytes_per_sec = 8 * 1024;
        let grace = Duration::from_secs(30);
        Self {
            max_bytes,
            idle: Duration::from_secs(20),
            min_bytes_per_sec,
            grace,
            max_in_flight: 64,
            deadline: grace + Duration::from_secs(max_bytes / min_bytes_per_sec + 60),
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Sets the longest a body may send nothing.
    #[must_use]
    pub fn with_idle(mut self, idle: Duration) -> Self {
        self.idle = idle;
        self
    }

    /// Sets the slowest average rate a body may keep after `grace`, and the deadline it implies.
    #[must_use]
    pub fn with_min_rate(mut self, min_bytes_per_sec: u64, grace: Duration) -> Self {
        self.min_bytes_per_sec = min_bytes_per_sec.max(1);
        self.grace = grace;
        self.deadline = grace + Duration::from_secs(self.max_bytes / self.min_bytes_per_sec + 60);
        self
    }

    /// Sets how many bodies this route reads at once on one process.
    #[must_use]
    pub fn with_max_in_flight(mut self, max_in_flight: usize) -> Self {
        self.max_in_flight = max_in_flight.max(1);
        self
    }

    /// Sets the whole request's deadline, if it is to be longer than the rate implies.
    #[must_use]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The longest body.
    #[must_use]
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// The longest silence.
    #[must_use]
    pub fn idle(&self) -> Duration {
        self.idle
    }

    /// The slowest average rate after the grace period, in bytes per second.
    #[must_use]
    pub fn min_bytes_per_sec(&self) -> u64 {
        self.min_bytes_per_sec
    }

    /// How long a body may start slowly before its rate is judged.
    #[must_use]
    pub fn grace(&self) -> Duration {
        self.grace
    }

    /// The whole request's deadline.
    #[must_use]
    pub fn deadline(&self) -> Duration {
        self.deadline
    }

    /// A permit to read one body, or `None` when `max_in_flight` are already being read.
    #[must_use]
    pub fn admit(&self) -> Option<StreamPermit> {
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.max_in_flight {
                return None;
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(StreamPermit::holding(Arc::clone(&self.in_flight)));
                }
                Err(actual) => current = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StreamBounds;
    use std::time::Duration;

    #[test]
    fn the_permit_count_holds_and_returns() {
        let bounds = StreamBounds::new(1_000).with_max_in_flight(2);
        let first = bounds.admit().expect("one");
        let shared = bounds.clone();
        let second = shared.admit().expect("two");
        assert!(bounds.admit().is_none(), "a clone shares the count");
        drop(first);
        assert!(bounds.admit().is_some());
        drop(second);
    }

    #[test]
    fn the_deadline_covers_a_body_at_the_minimum_rate() {
        let bounds =
            StreamBounds::new(25 * 1024 * 1024).with_min_rate(64 * 1024, Duration::from_secs(10));
        assert!(bounds.deadline() >= Duration::from_secs(10 + 400));
    }
}
