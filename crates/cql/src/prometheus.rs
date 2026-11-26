//! Prometheus metrics export for YCQL operations.
//!
//! ## Usage
//!
//! ```rust
//! use fundamental_cql::PrometheusMetrics;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let metrics = PrometheusMetrics::new("myapp")?;
//!
//! // Record operations (the trait-based `record_query` is also available via
//! // `fundamental_db::DatabaseMetrics`).
//! metrics.record_batch(3);
//! metrics.record_cache_hit();
//! metrics.record_lwt(true);
//!
//! // Get metrics for export
//! let encoded = metrics.encode()?;
//! # Ok(())
//! # }
//! ```

// The doctest above documents the re-exported `PrometheusMetrics`.
#![allow(rustdoc::private_doc_tests)]
#![expect(
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]

use prometheus::core::Collector;
use prometheus::{
    Counter, CounterVec, Encoder, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};

use std::time::Duration;

use fundamental_db::DatabaseMetrics;

/// Default histogram buckets for query duration (in seconds).
pub const DEFAULT_DURATION_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Prometheus metrics collector for YCQL.
#[derive(Debug, Clone)]
pub struct PrometheusMetrics {
    registry: Registry,
    queries_total: CounterVec,
    query_duration: HistogramVec,
    query_errors: CounterVec,
    slow_queries: CounterVec,
    batch_operations: Counter,
    batch_items: Counter,
    cache_hits: Counter,
    cache_misses: Counter,
    lwt_operations: Counter,
    lwt_applied: Counter,
}

impl PrometheusMetrics {
    /// Creates a new Prometheus metrics collector.
    ///
    /// # Errors
    ///
    /// Returns an error if metrics registration fails (e.g., duplicate names).
    pub fn new(namespace: &str) -> Result<Self, prometheus::Error> {
        Self::with_buckets(namespace, DEFAULT_DURATION_BUCKETS.to_vec())
    }

    /// Creates a new Prometheus metrics collector with custom histogram buckets.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn with_buckets(namespace: &str, buckets: Vec<f64>) -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        let (metrics, collectors) = Self::build(namespace, registry.clone(), buckets)?;
        for collector in collectors {
            registry.register(collector)?;
        }
        Ok(metrics)
    }

    /// Registers this metric set into a registry the caller already owns.
    ///
    /// [`new`](Self::new) builds a private registry, which a process-wide `/metrics` scrape cannot
    /// read.
    ///
    /// ```rust,no_run
    /// # extern crate fundamental_cql;
    /// # extern crate prometheus;
    /// use fundamental_cql::PrometheusMetrics;
    /// use prometheus::Registry;
    ///
    /// # fn main() -> Result<(), prometheus::Error> {
    /// let registry = Registry::new();
    /// let metrics = PrometheusMetrics::in_registry("acme", &registry)?;
    /// # let _ = metrics;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns an error if registration fails, most often because a set with the same `namespace`
    /// is already in this registry.
    pub fn in_registry(namespace: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        Self::in_registry_with_buckets(namespace, registry, DEFAULT_DURATION_BUCKETS.to_vec())
    }

    /// [`in_registry`](Self::in_registry) with custom histogram buckets.
    ///
    /// # Errors
    /// As [`in_registry`](Self::in_registry).
    pub fn in_registry_with_buckets(
        namespace: &str,
        registry: &Registry,
        buckets: Vec<f64>,
    ) -> Result<Self, prometheus::Error> {
        let (metrics, collectors) = Self::collectors_with_buckets(namespace, registry, buckets)?;
        for collector in collectors {
            registry.register(collector)?;
        }
        Ok(metrics)
    }

    /// Builds the whole metric set **without registering it**, handing back the
    /// collectors so a caller can inspect them first.
    ///
    /// For a deployment that checks label cardinality: `Collector::desc` is readable only before
    /// registration. The caller owes the registration.
    ///
    /// # Errors
    /// As [`in_registry`](Self::in_registry), minus the registration failures.
    pub fn collectors_with_buckets(
        namespace: &str,
        registry: &Registry,
        buckets: Vec<f64>,
    ) -> Result<(Self, Vec<Box<dyn Collector>>), prometheus::Error> {
        Self::build(namespace, registry.clone(), buckets)
    }

    /// [`collectors_with_buckets`](Self::collectors_with_buckets) with the default
    /// histogram buckets.
    ///
    /// # Errors
    /// As [`collectors_with_buckets`](Self::collectors_with_buckets).
    pub fn collectors(
        namespace: &str,
        registry: &Registry,
    ) -> Result<(Self, Vec<Box<dyn Collector>>), prometheus::Error> {
        Self::collectors_with_buckets(namespace, registry, DEFAULT_DURATION_BUCKETS.to_vec())
    }

    /// Builds the whole metric set, returning it with the collectors still unregistered.
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn build(
        namespace: &str,
        registry: Registry,
        buckets: Vec<f64>,
    ) -> Result<(Self, Vec<Box<dyn Collector>>), prometheus::Error> {
        let queries_total = CounterVec::new(
            Opts::new("cql_queries_total", "Total number of YCQL queries").namespace(namespace),
            &["operation", "table"],
        )?;

        let query_duration = HistogramVec::new(
            HistogramOpts::new(
                "cql_query_duration_seconds",
                "YCQL query duration in seconds",
            )
            .namespace(namespace)
            .buckets(buckets),
            &["operation", "table"],
        )?;

        let query_errors = CounterVec::new(
            Opts::new(
                "cql_query_errors_total",
                "Total number of failed YCQL queries",
            )
            .namespace(namespace),
            &["operation", "table"],
        )?;

        let slow_queries = CounterVec::new(
            Opts::new(
                "cql_slow_queries_total",
                "YCQL queries exceeding slow threshold",
            )
            .namespace(namespace),
            &["operation", "table"],
        )?;

        let batch_operations = Counter::with_opts(
            Opts::new("cql_batch_operations_total", "Total YCQL batch operations")
                .namespace(namespace),
        )?;

        let batch_items = Counter::with_opts(
            Opts::new(
                "cql_batch_items_total",
                "Total items processed in YCQL batches",
            )
            .namespace(namespace),
        )?;

        let cache_hits = Counter::with_opts(
            Opts::new("cql_cache_hits_total", "YCQL prepared statement cache hits")
                .namespace(namespace),
        )?;

        let cache_misses = Counter::with_opts(
            Opts::new(
                "cql_cache_misses_total",
                "YCQL prepared statement cache misses",
            )
            .namespace(namespace),
        )?;

        let lwt_operations = Counter::with_opts(
            Opts::new(
                "cql_lwt_operations_total",
                "Total YCQL lightweight transaction operations",
            )
            .namespace(namespace),
        )?;

        let lwt_applied = Counter::with_opts(
            Opts::new(
                "cql_lwt_applied_total",
                "YCQL lightweight transactions that were applied",
            )
            .namespace(namespace),
        )?;

        // Every collector this set defines; the caller registers them.
        let collectors: Vec<Box<dyn Collector>> = vec![
            Box::new(queries_total.clone()),
            Box::new(query_duration.clone()),
            Box::new(query_errors.clone()),
            Box::new(slow_queries.clone()),
            Box::new(batch_operations.clone()),
            Box::new(batch_items.clone()),
            Box::new(cache_hits.clone()),
            Box::new(cache_misses.clone()),
            Box::new(lwt_operations.clone()),
            Box::new(lwt_applied.clone()),
        ];

        Ok((
            Self {
                registry,
                queries_total,
                query_duration,
                query_errors,
                slow_queries,
                batch_operations,
                batch_items,
                cache_hits,
                cache_misses,
                lwt_operations,
                lwt_applied,
            },
            collectors,
        ))
    }

    /// Records a batch operation.
    pub fn record_batch(&self, item_count: u64) {
        self.batch_operations.inc();
        self.batch_items.inc_by(item_count as f64);
    }

    /// Records a prepared statement cache hit.
    pub fn record_cache_hit(&self) {
        self.cache_hits.inc();
    }

    /// Records a prepared statement cache miss.
    pub fn record_cache_miss(&self) {
        self.cache_misses.inc();
    }

    /// Records a lightweight transaction operation.
    pub fn record_lwt(&self, applied: bool) {
        self.lwt_operations.inc();
        if applied {
            self.lwt_applied.inc();
        }
    }

    /// Encodes all metrics in Prometheus text format.
    ///
    /// # Errors
    ///
    /// Returns a [`prometheus::Error`] if the registry fails to encode, or the encoder produces
    /// non-UTF-8 bytes.
    pub fn encode(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        String::from_utf8(buffer).map_err(|e| {
            prometheus::Error::Msg(format!(
                "prometheus text encoder produced non-UTF-8 output: {e}"
            ))
        })
    }

    /// Returns a reference to the underlying registry.
    ///
    /// Use this to register additional custom metrics.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}

impl DatabaseMetrics for PrometheusMetrics {
    fn record_query(&self, operation: &str, table: &str, duration: Duration, success: bool) {
        self.queries_total
            .with_label_values(&[operation, table])
            .inc();

        self.query_duration
            .with_label_values(&[operation, table])
            .observe(duration.as_secs_f64());

        if !success {
            self.query_errors
                .with_label_values(&[operation, table])
                .inc();
        }
    }

    fn record_connection_acquire(&self, _duration: Duration, _success: bool) {
        // YCQL uses session-based connections, not traditional pooling
    }

    fn record_slow_query(&self, operation: &str, table: &str, _duration: Duration) {
        self.slow_queries
            .with_label_values(&[operation, table])
            .inc();
    }

    fn record_transaction(&self, _committed: bool, _duration: Duration) {
        // YCQL doesn't have traditional transactions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collectors_does_not_register() {
        let registry = Registry::new();
        let (_metrics, collectors) =
            PrometheusMetrics::collectors("acme", &registry).expect("the set builds");

        assert!(
            registry.gather().is_empty(),
            "nothing may be registered until the caller says so"
        );
        assert_eq!(collectors.len(), 10, "every family is handed over");

        for collector in collectors {
            registry
                .register(collector)
                .expect("registers exactly once");
        }
    }

    #[test]
    fn a_collector_states_its_labels_before_it_is_registered() {
        let registry = Registry::new();
        let (_metrics, collectors) =
            PrometheusMetrics::collectors("acme", &registry).expect("the set builds");

        let queries = collectors
            .iter()
            .flat_map(|collector| collector.desc())
            .find(|desc| desc.fq_name == "acme_cql_queries_total")
            .expect("the query counter is in the set");

        assert_eq!(queries.variable_labels, vec!["operation", "table"]);
        assert!(
            registry.gather().is_empty(),
            "reading a descriptor must not register anything"
        );
    }

    #[test]
    fn a_shared_registry_renders_metrics_from_more_than_one_pool() {
        let registry = Registry::new();
        let primary =
            PrometheusMetrics::in_registry("primary", &registry).expect("registers the first set");
        let replica =
            PrometheusMetrics::in_registry("replica", &registry).expect("registers the second set");

        let rendered = primary.encode().expect("encodes");

        assert!(
            rendered.contains("primary_cql_"),
            "the first pool must be in the scrape: {rendered}"
        );
        assert!(
            rendered.contains("replica_cql_"),
            "so must the second — that is the point of one registry: {rendered}"
        );
        assert_eq!(
            rendered,
            replica.encode().expect("encodes"),
            "both handles render the same shared registry"
        );
    }

    #[test]
    fn registering_the_same_namespace_twice_is_an_error() {
        let registry = Registry::new();
        PrometheusMetrics::in_registry("dup", &registry).expect("first registration");

        assert!(PrometheusMetrics::in_registry("dup", &registry).is_err());
    }

    #[test]
    fn test_create_metrics() {
        let metrics = PrometheusMetrics::new("test").unwrap();
        assert!(metrics.encode().is_ok());
    }

    #[test]
    fn test_record_query() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_query("select", "events", Duration::from_millis(10), true);
        metrics.record_query("insert", "events", Duration::from_millis(20), true);
        metrics.record_query("select", "events", Duration::from_millis(15), false);

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_queries_total"));
        assert!(output.contains("cql_query_duration_seconds"));
        assert!(output.contains("cql_query_errors_total"));
    }

    #[test]
    fn test_record_slow_query() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_slow_query("select", "events", Duration::from_secs(2));

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_slow_queries_total"));
    }

    #[test]
    fn test_record_batch() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_batch(10);
        metrics.record_batch(20);

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_batch_operations_total"));
        assert!(output.contains("cql_batch_items_total"));
    }

    #[test]
    fn test_record_cache() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_cache_hits_total"));
        assert!(output.contains("cql_cache_misses_total"));
    }

    #[test]
    fn test_record_lwt() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_lwt(true);
        metrics.record_lwt(false);

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_lwt_operations_total"));
        assert!(output.contains("cql_lwt_applied_total"));
    }

    #[test]
    fn test_custom_buckets() {
        let buckets = vec![0.01, 0.1, 1.0, 10.0];
        let metrics = PrometheusMetrics::with_buckets("test", buckets).unwrap();

        metrics.record_query("select", "events", Duration::from_millis(50), true);

        let output = metrics.encode().unwrap();
        assert!(output.contains("cql_query_duration_seconds"));
    }
}
