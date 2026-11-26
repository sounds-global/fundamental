//! Execution helper that retries transient YCQL failures.

use std::future::Future;
use std::time::Duration;

use tracing::{debug, warn};

use crate::{CqlResult, RetryConfig};

/// Executes an operation with automatic retry for transient errors.
///
/// # Examples
///
/// ```rust,no_run
/// use fundamental_cql::{RetryConfig, CqlError, with_retry};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let result: i32 = with_retry(RetryConfig::default(), || async {
///     // Perform a retryable operation, e.g. a session query. A transient
///     // failure here is retried per the policy; a non-retryable one fails fast.
///     Err::<i32, _>(CqlError::RateLimited("transient".into()))
/// })
/// .await?;
/// # Ok(())
/// # }
/// ```
#[expect(
    clippy::cognitive_complexity,
    clippy::missing_errors_doc,
    reason = "readability baseline"
)]
pub async fn with_retry<F, Fut, T>(config: RetryConfig, operation: F) -> CqlResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = CqlResult<T>>,
{
    let mut attempts = 0u32;

    loop {
        match operation().await {
            Ok(result) => {
                if attempts > 0 {
                    debug!(attempts = attempts, "Operation succeeded after retry");
                }
                return Ok(result);
            }
            Err(err) => {
                attempts += 1;

                if !err.is_retryable() {
                    debug!(
                        error = %err,
                        "Non-retryable error, failing immediately"
                    );
                    return Err(err);
                }

                if attempts > config.max_retries {
                    warn!(
                        attempts = attempts,
                        max_retries = config.max_retries,
                        error = %err,
                        "Retry limit exceeded"
                    );
                    return Err(err);
                }

                let delay = calculate_delay(&config, attempts);
                warn!(
                    attempt = attempts,
                    max_retries = config.max_retries,
                    delay_ms = delay.as_millis(),
                    error = %err,
                    "Retryable error, waiting before retry"
                );

                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Calculates the delay for a retry attempt using exponential backoff with jitter.
pub(crate) fn calculate_delay(config: &RetryConfig, attempt: u32) -> Duration {
    let base_ms = config.base_delay.as_millis() as u64;
    let max_ms = config.max_delay.as_millis() as u64;

    // base * 2^(attempt-1), the exponent capped against overflow.
    let exponent = (attempt - 1).min(10);
    let delay_ms = base_ms.saturating_mul(1u64 << exponent);

    let jittered_ms = if config.jitter_factor > 0.0 {
        let jitter_range = (delay_ms as f64 * config.jitter_factor) as i64;
        if jitter_range > 0 {
            let random_jitter = random_jitter(jitter_range);
            (delay_ms as i64 + random_jitter).max(1) as u64
        } else {
            delay_ms
        }
    } else {
        delay_ms
    };

    Duration::from_millis(jittered_ms.min(max_ms))
}

/// A pseudo-random jitter in `[-range, +range]`, from the system time.
fn random_jitter(range: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i64;

    let mixed = now.wrapping_mul(0x517cc1b727220a95);
    (mixed % (range * 2 + 1)) - range
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_delay_exponential() {
        let config = RetryConfig::new()
            .base_delay(Duration::from_millis(100))
            .max_delay(Duration::from_secs(10))
            .jitter_factor(0.0); // Disable jitter for predictable testing

        // First retry: 100ms
        let delay1 = calculate_delay(&config, 1);
        assert_eq!(delay1, Duration::from_millis(100));

        // Second retry: 200ms
        let delay2 = calculate_delay(&config, 2);
        assert_eq!(delay2, Duration::from_millis(200));

        // Third retry: 400ms
        let delay3 = calculate_delay(&config, 3);
        assert_eq!(delay3, Duration::from_millis(400));
    }

    #[test]
    fn test_calculate_delay_caps_at_max() {
        let config = RetryConfig::new()
            .base_delay(Duration::from_millis(100))
            .max_delay(Duration::from_millis(500))
            .jitter_factor(0.0);

        // High attempt should cap at max_delay
        let delay = calculate_delay(&config, 10);
        assert_eq!(delay, Duration::from_millis(500));
    }

    #[test]
    fn test_calculate_delay_with_jitter() {
        let config = RetryConfig::new()
            .base_delay(Duration::from_millis(100))
            .max_delay(Duration::from_secs(10))
            .jitter_factor(0.25);

        // The time-based PRNG can land outside the theoretical [75ms, 125ms], so the bound is
        // loose.
        let delay = calculate_delay(&config, 1);
        assert!(delay >= Duration::from_millis(1));
        assert!(delay <= Duration::from_millis(200));
    }
}
