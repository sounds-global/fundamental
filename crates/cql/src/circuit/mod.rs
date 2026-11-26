//! Circuit breaker pattern for YCQL connections.
//!
//! # Example
//!
//! ```rust,no_run
//! use fundamental_cql::{
//!     BatchType, CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CqlBatch,
//!     CqlPool,
//! };
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     payload: String,
//! # }
//! # async fn example(pool: CqlPool, event: Event) {
//! let config = CircuitBreakerConfig::default();
//! let cb = CircuitBreaker::new(config);
//!
//! // Wrap database operations. The wrapped error must implement `IsRetryable`
//! // so the breaker can classify failures — `CqlError` (the repository's
//! // error type) does, which is why the breaker can fail fast on `Open`.
//! let result = cb.call(|| async {
//!     pool.repository::<Event>()
//!         .insert_batch(std::slice::from_ref(&event), BatchType::Unlogged)
//!         .await
//! }).await;
//!
//! match result {
//!     Ok(()) => println!("Insert succeeded"),
//!     Err(CircuitBreakerError::Open) => println!("Circuit is open, failing fast"),
//!     Err(CircuitBreakerError::Inner(e)) => println!("Insert failed: {}", e),
//!     // `CircuitBreakerError` is `#[non_exhaustive]`, so a caller decides what an
//!     // outcome it has not been taught about means rather than failing to compile the
//!     // day one is added.
//!     Err(_) => println!("Insert failed for a reason this caller does not classify"),
//! }
//! # }
//! ```

// The doctest above documents the re-exported circuit types.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod breaker;
mod breaker_state;
mod config;
mod error;
mod internals;
mod is_retryable;
mod state;

pub use breaker::CircuitBreaker;
pub use breaker_state::CircuitBreakerStats;
pub use config::CircuitBreakerConfig;
pub use error::CircuitBreakerError;
pub(crate) use internals::CircuitInternals;
pub use is_retryable::IsRetryable;
pub use state::CircuitState;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[derive(Debug)]
    struct TestError {
        retryable: bool,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestError(retryable={})", self.retryable)
        }
    }

    impl std::error::Error for TestError {}

    impl IsRetryable for TestError {
        fn is_retryable(&self) -> bool {
            self.retryable
        }
    }

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

    #[tokio::test]
    async fn test_call_success() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());

        let result: Result<i32, CircuitBreakerError<TestError>> =
            cb.call(|| async { Ok::<_, TestError>(42) }).await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(cb.stats().total_successes, 1);
    }

    #[tokio::test]
    async fn test_call_failure() {
        let config = CircuitBreakerConfig::default().failure_threshold(2);
        let cb = CircuitBreaker::new(config);

        let result: Result<i32, CircuitBreakerError<TestError>> = cb
            .call(|| async { Err::<i32, _>(TestError { retryable: true }) })
            .await;

        assert!(result.is_err());
        assert_eq!(cb.stats().total_failures, 1);
    }

    #[tokio::test]
    async fn test_rejects_when_open() {
        let config = CircuitBreakerConfig::default().failure_threshold(1);
        let cb = CircuitBreaker::new(config);

        cb.record_failure(true);
        assert_eq!(cb.state(), CircuitState::Open);

        let result: Result<i32, CircuitBreakerError<TestError>> =
            cb.call(|| async { Ok::<_, TestError>(42) }).await;

        assert!(matches!(result, Err(CircuitBreakerError::Open)));
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
