//! [`HybridClock`]: a process-wide monotonic source of [`HybridTs`].

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::HybridTs;

static LAST: OnceLock<Mutex<HybridTs>> = OnceLock::new();

/// A monotonic hybrid-logical clock minting commit timestamps on the backends with no cluster
/// timestamp (offline and YCQL).
pub struct HybridClock;

impl HybridClock {
    /// Returns a fresh timestamp, strictly greater than every previous one.
    #[must_use]
    pub fn now() -> HybridTs {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let mut last = LAST
            .get_or_init(|| Mutex::new(HybridTs::ZERO))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = if wall > last.wall {
            HybridTs::new(wall, 0)
        } else {
            HybridTs::new(last.wall, last.logical.saturating_add(1))
        };
        *last = next;
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_strictly_monotonic() {
        let mut previous = HybridClock::now();
        for _ in 0..1000 {
            let next = HybridClock::now();
            assert!(next > previous, "clock must be strictly increasing");
            previous = next;
        }
    }
}
