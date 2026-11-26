//! YSQL metrics collection and observability.

mod collector;
mod config;
mod guards;
mod snapshot;

/// YSQL metrics collector.
///
/// # Example
///
/// ```rust,no_run
/// # use std::time::Duration;
/// use fundamental_sql::{SqlMetrics, SqlMetricsConfig};
/// # use fundamental_db::{KeyedRepository, Repository};
/// # #[derive(Clone, fundamental_macros::Model)]
/// # #[model(sql, table = "users")]
/// # struct User {
/// #     #[model(id)]
/// #     id: uuid::Uuid,
/// # }
/// # async fn example(
/// #     pool: &fundamental_sql::SqlPool,
/// #     user_id: uuid::Uuid,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// let config = SqlMetricsConfig::default()
///     .slow_query_threshold(Duration::from_millis(500));
///
/// let metrics = SqlMetrics::new(config);
/// let repository = pool.repository::<User>();
///
/// // Wrap operations with timing
/// let result = metrics.timed("select", "users", async {
///     repository.find(&user_id).await
/// }).await;
///
/// // Get statistics
/// let stats = metrics.snapshot();
/// println!("Transaction commit rate: {:.2}%", stats.transaction_commit_rate());
/// # let _ = result;
/// # Ok(())
/// # }
/// ```
pub use collector::SqlMetrics;
pub use config::SqlMetricsConfig;
pub use snapshot::SqlMetricsSnapshot;

pub use guards::*;

#[cfg(test)]
mod tests {
    use super::*;
    use fundamental_db::DatabaseMetrics;
    use std::time::Duration;

    #[test]
    fn test_metrics_config_builder() {
        let config = SqlMetricsConfig::new()
            .slow_query_threshold(Duration::from_millis(500))
            .track_transaction_stats(false)
            .track_pool_metrics(true)
            .log_slow_query_text(false);

        assert_eq!(config.base.slow_query_threshold, Duration::from_millis(500));
        assert!(!config.track_transaction_stats);
        assert!(config.track_pool_metrics);
        assert!(!config.log_slow_query_text);
    }

    #[test]
    fn test_metrics_recording() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_query("select", "users", Duration::from_millis(10), true);
        metrics.record_query("insert", "users", Duration::from_millis(20), true);
        metrics.record_query("select", "users", Duration::from_millis(15), false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 3);
        assert_eq!(snapshot.base.queries_failed, 1);
        assert_eq!(snapshot.base.total_query_time_us, 45_000);
    }

    #[test]
    fn test_slow_query_detection() {
        let config = SqlMetricsConfig::new().slow_query_threshold(Duration::from_millis(50));
        let metrics = SqlMetrics::new(config);

        metrics.record_query("select", "users", Duration::from_millis(10), true);
        metrics.record_slow_query("select", "users", Duration::from_millis(100));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.slow_queries, 1);
    }

    #[test]
    fn test_transaction_metrics() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_transaction_start();
        metrics.record_transaction_commit(Duration::from_millis(50));

        metrics.record_transaction_start();
        metrics.record_transaction_rollback(Duration::from_millis(30));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.transactions_total, 2);
        assert_eq!(snapshot.base.transactions_committed, 1);
        assert_eq!(snapshot.transactions_rolled_back, 1);
        assert!((snapshot.transaction_commit_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_serialization_conflict_metrics() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_transaction_start();
        // One call: the conflict counter is derived from the retry.
        metrics.record_retry("40001");
        metrics.record_transaction_commit(Duration::from_millis(100));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.serialization_conflicts, 1);
        assert_eq!(snapshot.retry_attempts, 1);
        assert!((snapshot.serialization_conflict_rate() - 100.0).abs() < 0.01);
    }

    /// Contention reported as `40001` elsewhere reports `40P01` here.
    #[test]
    fn a_deadlock_counts_as_a_retry_under_its_own_class() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_transaction_start();
        metrics.record_retry("40P01");

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.retry_attempts, 1);
        assert_eq!(snapshot.deadlocks_detected, 1);
        assert_eq!(
            snapshot.base.serialization_conflicts, 0,
            "a deadlock is not a serialization failure"
        );
    }

    #[test]
    fn test_connection_pool_metrics() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_connection_acquire(Duration::from_millis(5), true);
        metrics.record_connection_acquire(Duration::from_millis(10), true);
        metrics.record_connection_acquire(Duration::from_millis(0), false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.connection_acquires, 3);
        assert_eq!(snapshot.base.connection_timeouts, 1);
        assert!((snapshot.average_acquire_time_ms() - 5.0).abs() < 0.01);
        assert!((snapshot.connection_timeout_rate() - 33.33).abs() < 0.1);
    }

    #[test]
    fn test_savepoint_metrics() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_savepoint_created();
        metrics.record_savepoint_created();
        metrics.record_savepoint_rollback();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.savepoints_created, 2);
        assert_eq!(snapshot.savepoints_rolled_back, 1);
        assert!((snapshot.savepoint_rollback_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = SqlMetrics::with_defaults();

        metrics.record_query("select", "users", Duration::from_millis(10), true);
        metrics.record_transaction_start();
        metrics.record_connection_acquire(Duration::from_millis(5), true);

        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 0);
        assert_eq!(snapshot.base.transactions_total, 0);
        assert_eq!(snapshot.base.connection_acquires, 0);
    }

    #[test]
    fn test_timing_guard() {
        let metrics = SqlMetrics::with_defaults();

        {
            let _guard = TimingGuard::new(&metrics, "select", "users", Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(5));
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 1);
        assert!(snapshot.base.total_query_time_us >= 5000);
    }

    #[test]
    fn test_timing_guard_failure() {
        let metrics = SqlMetrics::with_defaults();

        {
            let mut guard = TimingGuard::new(&metrics, "insert", "users", Duration::from_secs(1));
            guard.mark_failed();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 1);
        assert_eq!(snapshot.base.queries_failed, 1);
    }

    #[test]
    fn test_transaction_timing_guard() {
        let metrics = SqlMetrics::with_defaults();

        {
            let mut guard = TransactionTimingGuard::new(&metrics);
            std::thread::sleep(Duration::from_millis(5));
            guard.mark_committed();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.transactions_total, 1);
        assert_eq!(snapshot.base.transactions_committed, 1);
    }

    #[test]
    fn test_transaction_timing_guard_rollback() {
        let metrics = SqlMetrics::with_defaults();

        {
            let mut guard = TransactionTimingGuard::new(&metrics);
            guard.mark_rolled_back();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.transactions_total, 1);
        assert_eq!(snapshot.transactions_rolled_back, 1);
    }

    #[test]
    fn test_transaction_timing_guard_implicit_rollback() {
        let metrics = SqlMetrics::with_defaults();

        {
            let _guard = TransactionTimingGuard::new(&metrics);
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.transactions_total, 1);
        assert_eq!(snapshot.transactions_rolled_back, 1);
    }
}
