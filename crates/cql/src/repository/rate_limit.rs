//! [`RateLimit`] — a fixed-window rate limiter over a YCQL counter table.

use core::time::Duration;

use tracing::{instrument, warn};

use super::{CounterOps, RateLimitDecision};
use crate::{CqlError, CqlResult};

/// A fixed-window rate limiter shared across every replica of a service.
///
/// `check` returns a [`RateLimitDecision`], never a raw count, so an error cannot read as an
/// empty window.
///
/// # The table
///
/// [`CounterOps`] pins the key columns, so the table must be exactly:
///
/// ```cql
/// CREATE TABLE rate_limits (
///     key          text,
///     window_start bigint,
///     hits         counter,
///     PRIMARY KEY ((key, window_start))
/// ) WITH compaction = { 'class': 'TimeWindowCompactionStrategy' };
/// ```
///
/// # Expiry
///
/// Counter columns cannot expire, so a key's first request of a new window clears its previous
/// one, best-effort.
///
/// # Limits
///
/// A fixed window: the worst case over an arbitrary interval is twice the limit.
pub struct RateLimit {
    counters: CounterOps,
    column: String,
    window: Duration,
    limit: u64,
}

impl RateLimit {
    /// Builds a limiter allowing `limit` requests per `window` on `column`.
    ///
    /// # Errors
    /// [`CqlError::Config`] if `window` or `limit` is zero, or `window` does not fit whole seconds.
    pub fn new(
        counters: CounterOps,
        column: impl Into<String>,
        window: Duration,
        limit: u64,
    ) -> CqlResult<Self> {
        let window_secs = window.as_secs();
        if window_secs == 0 {
            return Err(CqlError::Config(
                "rate-limit window must be at least one second".to_string(),
            ));
        }
        if i64::try_from(window_secs).is_err() {
            return Err(CqlError::Config(
                "rate-limit window is too long to express as a window index".to_string(),
            ));
        }
        if limit == 0 {
            return Err(CqlError::Config(
                "rate-limit limit must be at least one; zero would shed every request".to_string(),
            ));
        }
        Ok(Self {
            counters,
            column: column.into(),
            window,
            limit,
        })
    }

    /// The window length.
    #[must_use]
    pub const fn window(&self) -> Duration {
        self.window
    }

    /// The number of requests allowed per window.
    #[must_use]
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// Counts one request from `key` at `now_unix_secs` and decides.
    ///
    /// The increment happens first, so a shed request still counts.
    ///
    /// # Errors
    ///
    /// [`CqlError`] if the counter could not be incremented or read back; never treat that as an
    /// allow.
    #[instrument(
        skip(self, key),
        fields(window_start, hits, limit = self.limit, decision),
        err(Debug)
    )]
    pub async fn check(&self, key: &str, now_unix_secs: i64) -> CqlResult<RateLimitDecision> {
        let window_secs = self.window_secs();
        let window_start = now_unix_secs.div_euclid(window_secs) * window_secs;
        tracing::Span::current().record("window_start", window_start);

        let hits = self
            .counters
            .increment_rate_limit(&self.column, key, window_start, 1)
            .await?;
        tracing::Span::current().record("hits", hits);

        // The first request of a new window is the moment this key's previous window
        // became garbage — and the only moment we can know that without a scan.
        if hits == 1 {
            self.clear_previous_window(key, window_start - window_secs)
                .await;
        }

        let used = u64::try_from(hits).unwrap_or(u64::MAX);
        let decision = if used > self.limit {
            let elapsed = now_unix_secs.saturating_sub(window_start);
            let remaining_secs = window_secs.saturating_sub(elapsed).max(1);
            RateLimitDecision::Shed {
                retry_after: Duration::from_secs(remaining_secs.unsigned_abs()),
            }
        } else {
            RateLimitDecision::Allow {
                remaining: self.limit - used,
            }
        };
        tracing::Span::current().record(
            "decision",
            if decision.is_allowed() {
                "allow"
            } else {
                "shed"
            },
        );
        Ok(decision)
    }

    /// The window length in whole seconds, validated by [`new`](Self::new).
    fn window_secs(&self) -> i64 {
        i64::try_from(self.window.as_secs()).unwrap_or(i64::MAX)
    }

    /// Best-effort reclamation of a key's previous window; a failure is logged, never propagated.
    async fn clear_previous_window(&self, key: &str, previous_window_start: i64) {
        if let Err(error) = self
            .counters
            .clear_rate_limit_window(&self.column, key, previous_window_start)
            .await
        {
            warn!(
                %error,
                previous_window_start,
                "rate-limit window cleanup failed; the stale counter cell remains"
            );
        }
    }
}

impl core::fmt::Debug for RateLimit {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RateLimit")
            .field("column", &self.column)
            .field("window", &self.window)
            .field("limit", &self.limit)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    /// The window index arithmetic, which keeps the partition key stable across replicas.
    fn window_start(now_unix_secs: i64, window_secs: i64) -> i64 {
        now_unix_secs.div_euclid(window_secs) * window_secs
    }

    #[test]
    fn window_start_is_stable_within_a_window() {
        assert_eq!(window_start(1_000, 60), 960);
        assert_eq!(window_start(1_019, 60), 960);
        assert_eq!(window_start(1_020, 60), 1_020);
    }

    #[test]
    fn window_start_floors_before_the_epoch() {
        // `div_euclid` floors rather than truncating toward zero, so a pre-epoch
        // clock cannot land two adjacent instants in the same window twice.
        assert_eq!(window_start(-1, 60), -60);
        assert_eq!(window_start(-60, 60), -60);
        assert_eq!(window_start(-61, 60), -120);
    }
}
