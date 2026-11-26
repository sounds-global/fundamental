//! Circuit breaker for YSQL connections: closed, open, and half-open while testing recovery.

mod breaker;
mod config;
mod error;
mod internals;
mod state;
mod stats;

/// Circuit breaker for YSQL operations.
///
/// # Example
///
/// ```rust,no_run
/// use fundamental_sql::{CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError};
///
/// # async fn example(
/// #     pool: &fundamental_sql::SqlPool,
/// #     id: uuid::Uuid,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// let config = CircuitBreakerConfig::default();
/// let cb = CircuitBreaker::new(config);
///
/// // Wrap database operations. The closure must return a `SqlError` on
/// // failure, so the breaker can classify retryable faults.
/// let result = cb
///     .call(|| async {
///         pool.fetch_optional(&format!("SELECT * FROM users WHERE id = '{id}'"))
///             .await
///     })
///     .await;
///
/// match result {
///     Ok(row) => println!("Got row: {:?}", row),
///     Err(CircuitBreakerError::Open) => println!("Circuit is open, failing fast"),
///     Err(CircuitBreakerError::Inner(e)) => println!("Query failed: {}", e),
///     Err(e) => println!("Circuit error: {}", e),
/// }
/// # Ok(())
/// # }
/// ```
pub use breaker::CircuitBreaker;
pub use config::CircuitBreakerConfig;
pub use error::CircuitBreakerError;
pub(crate) use internals::CircuitInternals;
pub use state::CircuitState;
pub use stats::CircuitBreakerStats;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_initial_state() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.is_allowed());
    }

    #[test]
    fn test_opens_after_failures() {
        let config = CircuitBreakerConfig::default().failure_threshold(3);
        let cb = CircuitBreaker::new(config);

        cb.record_failure(true);
        cb.record_failure(true);
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure(true);
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.is_allowed());
    }

    #[test]
    fn test_ignores_non_retryable_errors() {
        let config = CircuitBreakerConfig::default()
            .failure_threshold(3)
            .only_track_retryable(true);
        let cb = CircuitBreaker::new(config);

        cb.record_failure(false);
        cb.record_failure(false);
        cb.record_failure(false);
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure(true);
        cb.record_failure(true);
        cb.record_failure(true);
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_force_open_close() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());

        cb.force_open();
        assert_eq!(cb.state(), CircuitState::Open);

        cb.force_close();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_stats() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());

        cb.record_success();
        cb.record_success();
        cb.record_failure(true);

        let stats = cb.stats();
        assert_eq!(stats.total_successes, 2);
        assert_eq!(stats.total_failures, 1);
        assert_eq!(stats.recent_failures, 1);
    }

    #[test]
    fn test_config_presets() {
        let ha = CircuitBreakerConfig::high_availability();
        assert_eq!(ha.failure_threshold, 3);
        assert_eq!(ha.open_duration, Duration::from_secs(10));

        let tolerant = CircuitBreakerConfig::tolerant();
        assert_eq!(tolerant.failure_threshold, 10);
        assert_eq!(tolerant.open_duration, Duration::from_secs(60));
    }
}
