use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A token-bucket rate limiter keyed by principal.
///
/// A full bucket equals a fresh one, so eviction drops full buckets first and only then the
/// least recently touched, holding at most `max_principals`.
pub struct RateLimiter {
    capacity: f64,
    refill_per_second: f64,
    max_principals: usize,
    buckets: Mutex<HashMap<String, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl RateLimiter {
    /// Creates a limiter where each principal may burst `capacity` requests and
    /// then sustain `refill_per_second` requests per second, holding at most
    /// `max_principals` buckets.
    ///
    /// A zero `max_principals` is raised to one, which still throttles.
    #[must_use]
    pub fn new(capacity: u32, refill_per_second: f64, max_principals: usize) -> Self {
        Self {
            capacity: f64::from(capacity),
            refill_per_second,
            max_principals: max_principals.max(1),
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// How many buckets are held.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Tries to spend one token for `key`.
    ///
    /// # Errors
    /// Returns the [`Duration`] to wait before retrying when the bucket is empty.
    pub fn check(&self, key: &str) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }

    /// [`check`](Self::check) at a caller-supplied instant.
    ///
    ///
    /// # Errors
    /// Returns the [`Duration`] to wait before retrying when the bucket is empty.
    pub fn check_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !buckets.contains_key(key) && buckets.len() >= self.max_principals {
            evict_one(&mut buckets, self.capacity, self.refill_per_second, now);
        }
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: self.capacity,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_per_second).min(self.capacity);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - bucket.tokens;
            Err(Duration::from_secs_f64(deficit / self.refill_per_second))
        }
    }
}

/// Drops a full bucket if there is one, otherwise the least recently touched.
fn evict_one(
    buckets: &mut HashMap<String, Bucket>,
    capacity: f64,
    refill_per_second: f64,
    now: Instant,
) {
    let refilled = buckets.iter().find(|(_, bucket)| {
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens + elapsed * refill_per_second >= capacity
    });
    let victim = match refilled {
        Some((key, _)) => Some(key.clone()),
        None => buckets
            .iter()
            .min_by_key(|(_, bucket)| bucket.last)
            .map(|(key, _)| key.clone()),
    };
    if let Some(key) = victim {
        buckets.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_a_burst_up_to_capacity_then_throttles() {
        let limiter = RateLimiter::new(2, 0.000_001, 65_536);
        assert!(limiter.check("alice").is_ok());
        assert!(limiter.check("alice").is_ok());
        let retry = limiter.check("alice").expect_err("third is throttled");
        assert!(retry > Duration::ZERO);
    }

    #[test]
    fn principals_have_independent_buckets() {
        let limiter = RateLimiter::new(1, 0.000_001, 65_536);
        assert!(limiter.check("alice").is_ok());
        assert!(limiter.check("alice").is_err());
        assert!(limiter.check("bob").is_ok());
    }

    #[test]
    fn refills_over_time() {
        let limiter = RateLimiter::new(1, 1_000_000.0, 65_536);
        assert!(limiter.check("alice").is_ok());
        std::thread::sleep(Duration::from_millis(2));
        assert!(limiter.check("alice").is_ok());
    }

    #[test]
    fn the_bucket_map_is_bounded_however_many_principals_arrive() {
        let limiter = RateLimiter::new(1, 0.000_001, 8);
        for n in 0..512 {
            let _ = limiter.check(&format!("principal-{n}"));
        }
        assert!(
            limiter.tracked() <= 8,
            "a per-principal map must not grow without limit; held {}",
            limiter.tracked(),
        );
    }

    /// Evicting a refilled bucket changes nothing for its principal.
    #[test]
    fn evicting_a_refilled_bucket_changes_nothing_the_principal_can_observe() {
        let limiter = RateLimiter::new(1, 1_000_000.0, 1);
        assert!(limiter.check("alice").is_ok());
        assert!(limiter.check("bob").is_ok());
        assert_eq!(limiter.tracked(), 1);
        assert!(
            limiter.check("alice").is_ok(),
            "a refilled bucket carries no state, so losing it must be invisible",
        );
    }

    /// A throttled principal keeps its throttle when a newcomer forces an eviction.
    #[test]
    fn a_refilled_bucket_is_evicted_before_a_throttled_one() {
        let limiter = RateLimiter::new(2, 1.0, 2);
        let start = Instant::now();

        assert!(limiter.check_at("refilled", start).is_ok());

        let later = start + Duration::from_secs(10);
        assert!(limiter.check_at("spent", later).is_ok());
        assert!(limiter.check_at("spent", later).is_ok());
        assert!(limiter.check_at("spent", later).is_err());

        // The eviction must take `refilled`.
        assert!(limiter.check_at("newcomer", later).is_ok());
        assert_eq!(limiter.tracked(), 2);
        assert!(
            limiter.check_at("spent", later).is_err(),
            "the throttled principal must still be throttled: evicting it would return \
             an allowance it had already spent",
        );
    }

    #[test]
    fn a_bucket_refills_at_the_declared_rate() {
        let limiter = RateLimiter::new(2, 1.0, 8);
        let start = Instant::now();
        assert!(limiter.check_at("alice", start).is_ok());
        assert!(limiter.check_at("alice", start).is_ok());
        assert!(
            limiter.check_at("alice", start).is_err(),
            "both tokens are spent"
        );
        assert!(
            limiter
                .check_at("alice", start + Duration::from_secs(1))
                .is_ok(),
            "one second buys exactly one token",
        );
    }
}
