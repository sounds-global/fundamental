//! Database observability traits and configuration, shared by every adapter.

mod config;
mod database;
mod noop;
mod percentile;
mod snapshot;

pub use config::{DEFAULT_DURATION_BUCKETS, MetricsConfig};
pub use database::DatabaseMetrics;
pub use noop::NoopMetrics;
pub use snapshot::MetricsSnapshot;

pub use percentile::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "std")]
    use std::time::Duration;

    #[test]
    fn test_metrics_config_builder() {
        let config = MetricsConfig::new()
            .slow_query_threshold(Duration::from_millis(500))
            .include_query_text(true)
            .per_table_metrics(false);

        assert_eq!(config.slow_query_threshold, Duration::from_millis(500));
        assert!(config.include_query_text);
        assert!(!config.per_table_metrics);
    }

    #[test]
    fn test_metrics_snapshot_calculations() {
        let snapshot = MetricsSnapshot {
            queries_total: 100,
            queries_failed: 5,
            slow_queries: 10,
            total_query_time_us: 500_000, // 500ms total for 100 queries
            transactions_total: 50,
            transactions_committed: 45,
            serialization_conflicts: 5,
            connection_acquires: 100,
            connection_timeouts: 2,
        };

        assert!((snapshot.query_success_rate() - 95.0).abs() < 0.01);
        assert!((snapshot.average_query_duration_ms() - 5.0).abs() < 0.01);
        assert!((snapshot.transaction_commit_rate() - 90.0).abs() < 0.01);
        assert!((snapshot.slow_query_rate() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_metrics_snapshot_edge_cases() {
        let empty = MetricsSnapshot::default();

        assert_eq!(empty.query_success_rate(), 100.0);
        assert_eq!(empty.average_query_duration_ms(), 0.0);
        assert_eq!(empty.transaction_commit_rate(), 100.0);
        assert_eq!(empty.slow_query_rate(), 0.0);
    }

    #[test]
    fn test_noop_metrics() {
        let noop = NoopMetrics;
        noop.record_query("select", "users", Duration::from_millis(10), true);
        noop.record_connection_acquire(Duration::from_millis(5), true);
        noop.record_slow_query("select", "users", Duration::from_secs(2));
        noop.record_transaction(true, Duration::from_millis(100));
    }

    mod percentile_tests {
        use super::*;

        #[test]
        fn test_latency_histogram_basic() {
            let hist = LatencyHistogram::new();

            hist.record(Duration::from_micros(100));
            hist.record(Duration::from_micros(200));
            hist.record(Duration::from_micros(300));

            assert_eq!(hist.count(), 3);
            assert!(hist.min() <= 100);
            assert!(hist.max() >= 300);
        }

        #[test]
        fn test_latency_histogram_percentiles() {
            let hist = LatencyHistogram::new();

            for i in 1..=100 {
                hist.record(Duration::from_millis(i));
            }

            let p50 = hist.p50();
            assert!((45_000..=55_000).contains(&p50), "p50 was {p50}");

            let p99 = hist.p99();
            assert!((95_000..=100_000).contains(&p99), "p99 was {p99}");
        }

        #[test]
        fn test_latency_histogram_snapshot() {
            let hist = LatencyHistogram::new();

            for i in 1..=100 {
                hist.record(Duration::from_millis(i));
            }

            let snapshot = hist.snapshot();
            assert_eq!(snapshot.count, 100);
            assert!(snapshot.p50_us > 0);
            assert!(snapshot.p99_us > snapshot.p50_us);
        }

        #[test]
        fn test_latency_histogram_reset() {
            let hist = LatencyHistogram::new();

            hist.record(Duration::from_millis(10));
            assert_eq!(hist.count(), 1);

            hist.reset();
            assert_eq!(hist.count(), 0);
        }

        #[test]
        fn test_latency_histogram_clone() {
            let hist = LatencyHistogram::new();
            hist.record(Duration::from_millis(10));

            let cloned = hist.clone();
            assert_eq!(cloned.count(), 1);

            hist.record(Duration::from_millis(20));
            assert_eq!(hist.count(), 2);
            assert_eq!(cloned.count(), 1);
        }

        #[test]
        fn test_percentile_snapshot_conversions() {
            let snapshot = PercentileSnapshot {
                count: 100,
                min_us: 1000,
                max_us: 100000,
                mean_us: 50000.0,
                p50_us: 50000,
                p95_us: 95000,
                p99_us: 99000,
                p99_9_us: 99900,
            };

            assert!((snapshot.p50_ms() - 50.0).abs() < 0.01);
            assert!((snapshot.p95_ms() - 95.0).abs() < 0.01);
            assert!((snapshot.p99_ms() - 99.0).abs() < 0.01);
            assert!((snapshot.mean_ms() - 50.0).abs() < 0.01);
        }
    }
}
