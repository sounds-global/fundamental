//! YCQL metrics collection and observability.
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use std::time::Duration;
//! use fundamental_cql::{CqlValue, CqlMetrics, CqlMetricsConfig, CqlPool};
//! # #[derive(Clone, Debug, PartialEq, fundamental_macros::Model)]
//! # #[model(cql, table = "events")]
//! # struct Event {
//! #     #[model(partition_key)]
//! #     tenant_id: uuid::Uuid,
//! #     payload: String,
//! # }
//! # async fn example(pool: CqlPool, tenant_id: uuid::Uuid) {
//! let config = CqlMetricsConfig::default()
//!     .slow_query_threshold(Duration::from_millis(500));
//!
//! let metrics = CqlMetrics::new(config);
//! let repository = pool.repository::<Event>();
//!
//! // Wrap operations with timing
//! let result = metrics.timed("select", "events", async {
//!     repository.query().partition_key(CqlValue::uuid(tenant_id)).execute().await
//! }).await;
//! # let _ = result;
//!
//! // Get statistics
//! let stats = metrics.snapshot();
//! println!("Slow query rate: {:.2}%", stats.slow_query_rate());
//! # }
//! ```

// The doctest above documents the re-exported metrics types.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

mod collector;
mod config;
mod guards;
mod snapshot;

pub use guards::*;

pub use collector::CqlMetrics;
pub use config::CqlMetricsConfig;
pub use snapshot::CqlMetricsSnapshot;

#[cfg(test)]
mod tests {
    use fundamental_db::DatabaseMetrics;
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_metrics_config_builder() {
        let config = CqlMetricsConfig::new()
            .slow_query_threshold(Duration::from_millis(500))
            .track_cache_stats(false)
            .track_batch_stats(true);

        assert_eq!(config.base.slow_query_threshold, Duration::from_millis(500));
        assert!(!config.track_cache_stats);
        assert!(config.track_batch_stats);
    }

    #[test]
    fn test_metrics_recording() {
        let metrics = CqlMetrics::with_defaults();

        metrics.record_query("select", "events", Duration::from_millis(10), true);
        metrics.record_query("insert", "events", Duration::from_millis(20), true);
        metrics.record_query("select", "events", Duration::from_millis(15), false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 3);
        assert_eq!(snapshot.base.queries_failed, 1);
        assert_eq!(snapshot.base.total_query_time_us, 45_000);
    }

    #[test]
    fn test_slow_query_detection() {
        let config = CqlMetricsConfig::new().slow_query_threshold(Duration::from_millis(50));
        let metrics = CqlMetrics::new(config);

        metrics.record_query("select", "events", Duration::from_millis(10), true);
        metrics.record_slow_query("select", "events", Duration::from_millis(100));

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.slow_queries, 1);
    }

    #[test]
    fn test_batch_metrics() {
        let metrics = CqlMetrics::with_defaults();

        metrics.record_batch(10);
        metrics.record_batch(20);
        metrics.record_batch(15);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.batch_operations, 3);
        assert_eq!(snapshot.batch_items_total, 45);
        assert!((snapshot.average_batch_size() - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = CqlMetrics::with_defaults();

        for _ in 0..80 {
            metrics.record_cache_hit();
        }
        for _ in 0..20 {
            metrics.record_cache_miss();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.cache_hits, 80);
        assert_eq!(snapshot.cache_misses, 20);
        assert!((snapshot.cache_hit_rate() - 80.0).abs() < 0.01);
    }

    #[test]
    fn test_lwt_metrics() {
        let metrics = CqlMetrics::with_defaults();

        metrics.record_lwt(true);
        metrics.record_lwt(true);
        metrics.record_lwt(false);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.lwt_operations, 3);
        assert_eq!(snapshot.lwt_applied, 2);
        assert!((snapshot.lwt_success_rate() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_metrics_reset() {
        let metrics = CqlMetrics::with_defaults();

        metrics.record_query("select", "events", Duration::from_millis(10), true);
        metrics.record_cache_hit();
        metrics.record_batch(5);

        metrics.reset();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 0);
        assert_eq!(snapshot.cache_hits, 0);
        assert_eq!(snapshot.batch_operations, 0);
    }

    #[test]
    fn test_timing_guard() {
        let metrics = CqlMetrics::with_defaults();

        {
            let _guard = TimingGuard::new(&metrics, "select", "events", Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(5));
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 1);
        assert!(snapshot.base.total_query_time_us >= 5000);
    }

    #[test]
    fn test_timing_guard_failure() {
        let metrics = CqlMetrics::with_defaults();

        {
            let mut guard = TimingGuard::new(&metrics, "insert", "events", Duration::from_secs(1));
            guard.mark_failed();
        }

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.base.queries_total, 1);
        assert_eq!(snapshot.base.queries_failed, 1);
    }
}
