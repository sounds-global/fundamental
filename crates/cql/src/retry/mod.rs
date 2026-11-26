//! Retry logic with exponential backoff for YCQL operations.

mod config;
mod helper;

pub use config::RetryConfig;
pub(crate) use helper::calculate_delay;
pub use helper::with_retry;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CqlError, CqlResult};
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.base_delay, Duration::from_millis(10));
        assert_eq!(config.max_delay, Duration::from_secs(1));
        assert!((config.jitter_factor - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_config_builder() {
        let config = RetryConfig::new()
            .max_retries(5)
            .base_delay(Duration::from_millis(100))
            .max_delay(Duration::from_secs(10))
            .jitter_factor(0.5);

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.base_delay, Duration::from_millis(100));
        assert_eq!(config.max_delay, Duration::from_secs(10));
        assert!((config.jitter_factor - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jitter_factor_clamped() {
        let config = RetryConfig::new().jitter_factor(2.0);
        assert!((config.jitter_factor - 1.0).abs() < f64::EPSILON);

        let config = RetryConfig::new().jitter_factor(-0.5);
        assert!((config.jitter_factor - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_low_latency_config() {
        let config = RetryConfig::low_latency();
        assert_eq!(config.max_retries, 2);
        assert!(config.base_delay < Duration::from_millis(10));
    }

    #[test]
    fn test_high_reliability_config() {
        let config = RetryConfig::high_reliability();
        assert_eq!(config.max_retries, 5);
        assert!(config.base_delay >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_with_retry_success_first_try() {
        let result: CqlResult<i32> =
            with_retry(RetryConfig::default(), || async { Ok::<_, CqlError>(42) }).await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_retry_non_retryable_error() {
        let result: CqlResult<i32> = with_retry(RetryConfig::default(), || async {
            Err::<i32, _>(CqlError::Config("not retryable".into()))
        })
        .await;

        assert!(result.is_err());
    }
}
