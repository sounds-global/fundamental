//! Prometheus metrics export for YSQL operations.

use prometheus::core::Collector;
use prometheus::{
    Counter, CounterVec, Encoder, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};
use std::time::Duration;

use fundamental_db::DatabaseMetrics;

/// Default histogram buckets for query duration in seconds.
pub use fundamental_db::DEFAULT_DURATION_BUCKETS;

/// Default histogram buckets for connection acquisition in seconds.
pub const DEFAULT_ACQUIRE_BUCKETS: &[f64] =
    &[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 5.0];

/// Prometheus metrics collector for YSQL.
///
/// # Example
///
/// ```rust
/// use std::time::Duration;
/// use fundamental_sql::PrometheusMetrics;
/// // `record_query` / `record_transaction` come from the `DatabaseMetrics` trait.
/// use fundamental_db::DatabaseMetrics;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let metrics = PrometheusMetrics::new("myapp")?;
///
/// // Record operations
/// metrics.record_query("select", "users", Duration::from_millis(10), true);
/// metrics.record_transaction(true, Duration::from_millis(50));
///
/// // Get metrics for export
/// let encoded = metrics.encode()?;
/// # let _ = encoded;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PrometheusMetrics {
    registry: Registry,
    queries_total: CounterVec,
    query_duration: HistogramVec,
    query_errors: CounterVec,
    slow_queries: CounterVec,
    transactions_total: Counter,
    transactions_committed: Counter,
    transactions_rolled_back: Counter,
    transaction_duration: HistogramVec,
    serialization_conflicts: Counter,
    deadlocks: Counter,
    retry_attempts: CounterVec,
    retry_budget_exhausted: CounterVec,
    connection_acquires: Counter,
    connection_timeouts: Counter,
    connection_acquire_duration: HistogramVec,
    savepoints_total: Counter,
    savepoints_rolled_back: Counter,
    leak_warnings: Counter,
}

impl PrometheusMetrics {
    /// Creates a new Prometheus metrics collector with `namespace` as every metric's prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if metrics registration fails (e.g., duplicate names).
    pub fn new(namespace: &str) -> Result<Self, prometheus::Error> {
        Self::with_buckets(
            namespace,
            DEFAULT_DURATION_BUCKETS.to_vec(),
            DEFAULT_ACQUIRE_BUCKETS.to_vec(),
        )
    }

    /// Creates a collector whose query-duration buckets come from `config`.
    ///
    /// # Errors
    ///
    /// Returns an error if metrics registration fails (e.g., duplicate names).
    pub fn from_metrics_config(
        namespace: &str,
        config: &fundamental_db::MetricsConfig,
    ) -> Result<Self, prometheus::Error> {
        Self::with_buckets(
            namespace,
            config.duration_buckets.clone(),
            DEFAULT_ACQUIRE_BUCKETS.to_vec(),
        )
    }

    /// Creates a new Prometheus metrics collector with custom histogram buckets.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub fn with_buckets(
        namespace: &str,
        query_buckets: Vec<f64>,
        acquire_buckets: Vec<f64>,
    ) -> Result<Self, prometheus::Error> {
        let registry = Registry::new();
        let (metrics, collectors) =
            Self::build(namespace, registry.clone(), query_buckets, acquire_buckets)?;
        for collector in collectors {
            registry.register(collector)?;
        }
        Ok(metrics)
    }

    /// Registers this metric set into a registry the caller already owns, so a process-wide scrape reads it.
    ///
    /// ```rust,no_run
    /// # extern crate fundamental_sql;
    /// # extern crate prometheus;
    /// use fundamental_sql::PrometheusMetrics;
    /// use prometheus::Registry;
    ///
    /// # fn main() -> Result<(), prometheus::Error> {
    /// let registry = Registry::new();
    /// let metrics = PrometheusMetrics::in_registry("acme", &registry)?;
    /// // `registry` now also carries whatever else the process registers, and
    /// // one `encode` renders all of it.
    /// # let _ = metrics;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Returns an error if registration fails, most often because a set with the
    /// same `namespace` is already in this registry.
    pub fn in_registry(namespace: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        Self::in_registry_with_buckets(
            namespace,
            registry,
            DEFAULT_DURATION_BUCKETS.to_vec(),
            DEFAULT_ACQUIRE_BUCKETS.to_vec(),
        )
    }

    /// [`in_registry`](Self::in_registry) with custom histogram buckets.
    ///
    /// # Errors
    /// As [`in_registry`](Self::in_registry).
    pub fn in_registry_with_buckets(
        namespace: &str,
        registry: &Registry,
        query_buckets: Vec<f64>,
        acquire_buckets: Vec<f64>,
    ) -> Result<Self, prometheus::Error> {
        let (metrics, collectors) =
            Self::collectors_with_buckets(namespace, registry, query_buckets, acquire_buckets)?;
        for collector in collectors {
            registry.register(collector)?;
        }
        Ok(metrics)
    }

    /// Builds the whole metric set without registering it, so a caller can read each
    /// collector's labels first; the caller owes the registration.
    ///
    /// # Errors
    /// As [`in_registry`](Self::in_registry), minus the registration failures.
    pub fn collectors_with_buckets(
        namespace: &str,
        registry: &Registry,
        query_buckets: Vec<f64>,
        acquire_buckets: Vec<f64>,
    ) -> Result<(Self, Vec<Box<dyn Collector>>), prometheus::Error> {
        Self::build(namespace, registry.clone(), query_buckets, acquire_buckets)
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
        Self::collectors_with_buckets(
            namespace,
            registry,
            DEFAULT_DURATION_BUCKETS.to_vec(),
            DEFAULT_ACQUIRE_BUCKETS.to_vec(),
        )
    }

    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    fn build(
        namespace: &str,
        registry: Registry,
        query_buckets: Vec<f64>,
        acquire_buckets: Vec<f64>,
    ) -> Result<(Self, Vec<Box<dyn Collector>>), prometheus::Error> {
        let queries_total = CounterVec::new(
            Opts::new("sql_queries_total", "Total number of YSQL queries").namespace(namespace),
            &["operation", "table"],
        )?;

        let query_duration = HistogramVec::new(
            HistogramOpts::new(
                "sql_query_duration_seconds",
                "YSQL query duration in seconds",
            )
            .namespace(namespace)
            .buckets(query_buckets),
            &["operation", "table"],
        )?;

        let query_errors = CounterVec::new(
            Opts::new(
                "sql_query_errors_total",
                "Total number of failed YSQL queries",
            )
            .namespace(namespace),
            &["operation", "table"],
        )?;

        let slow_queries = CounterVec::new(
            Opts::new(
                "sql_slow_queries_total",
                "YSQL queries exceeding slow threshold",
            )
            .namespace(namespace),
            &["operation", "table"],
        )?;

        let transactions_total = Counter::with_opts(
            Opts::new("sql_transactions_total", "Total YSQL transactions started")
                .namespace(namespace),
        )?;

        let transactions_committed = Counter::with_opts(
            Opts::new(
                "sql_transactions_committed_total",
                "Successfully committed YSQL transactions",
            )
            .namespace(namespace),
        )?;

        let transactions_rolled_back = Counter::with_opts(
            Opts::new(
                "sql_transactions_rolled_back_total",
                "Rolled back YSQL transactions",
            )
            .namespace(namespace),
        )?;

        let transaction_duration = HistogramVec::new(
            HistogramOpts::new(
                "sql_transaction_duration_seconds",
                "YSQL transaction duration in seconds",
            )
            .namespace(namespace)
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
            &["outcome"],
        )?;

        let serialization_conflicts = Counter::with_opts(
            Opts::new(
                "sql_serialization_conflicts_total",
                "YSQL serialization conflicts (SQLSTATE 40001)",
            )
            .namespace(namespace),
        )?;

        let deadlocks = Counter::with_opts(
            Opts::new(
                "sql_deadlocks_total",
                "YSQL deadlock detections (SQLSTATE 40P01)",
            )
            .namespace(namespace),
        )?;

        // The retries the loop did not absorb: what contention broke, which an alert can be written against.
        let retry_budget_exhausted = CounterVec::new(
            Opts::new(
                "sql_retry_budget_exhausted_total",
                "Retry budgets spent without success, by the SQLSTATE that spent them",
            )
            .namespace(namespace),
            &["sqlstate"],
        )?;

        let retry_attempts = CounterVec::new(
            Opts::new(
                "sql_retry_attempts_total",
                "Transaction retry attempts, by the SQLSTATE that caused them",
            )
            .namespace(namespace),
            &["sqlstate"],
        )?;

        let connection_acquires = Counter::with_opts(
            Opts::new(
                "sql_connection_acquires_total",
                "YSQL connection pool acquisitions",
            )
            .namespace(namespace),
        )?;

        let connection_timeouts = Counter::with_opts(
            Opts::new(
                "sql_connection_timeouts_total",
                "YSQL connection acquisition timeouts",
            )
            .namespace(namespace),
        )?;

        let connection_acquire_duration = HistogramVec::new(
            HistogramOpts::new(
                "sql_connection_acquire_duration_seconds",
                "YSQL connection acquisition duration in seconds",
            )
            .namespace(namespace)
            .buckets(acquire_buckets),
            &["success"],
        )?;

        let savepoints_total = Counter::with_opts(
            Opts::new("sql_savepoints_total", "YSQL savepoints created").namespace(namespace),
        )?;

        let savepoints_rolled_back = Counter::with_opts(
            Opts::new(
                "sql_savepoints_rolled_back_total",
                "YSQL savepoints rolled back",
            )
            .namespace(namespace),
        )?;

        let leak_warnings = Counter::with_opts(
            Opts::new("sql_leak_warnings_total", "YSQL connection leak warnings")
                .namespace(namespace),
        )?;

        let collectors: Vec<Box<dyn Collector>> = vec![
            Box::new(queries_total.clone()),
            Box::new(query_duration.clone()),
            Box::new(query_errors.clone()),
            Box::new(slow_queries.clone()),
            Box::new(transactions_total.clone()),
            Box::new(transactions_committed.clone()),
            Box::new(transactions_rolled_back.clone()),
            Box::new(transaction_duration.clone()),
            Box::new(serialization_conflicts.clone()),
            Box::new(deadlocks.clone()),
            Box::new(retry_attempts.clone()),
            Box::new(retry_budget_exhausted.clone()),
            Box::new(connection_acquires.clone()),
            Box::new(connection_timeouts.clone()),
            Box::new(connection_acquire_duration.clone()),
            Box::new(savepoints_total.clone()),
            Box::new(savepoints_rolled_back.clone()),
            Box::new(leak_warnings.clone()),
        ];

        Ok((
            Self {
                registry,
                queries_total,
                query_duration,
                query_errors,
                slow_queries,
                transactions_total,
                transactions_committed,
                transactions_rolled_back,
                transaction_duration,
                serialization_conflicts,
                deadlocks,
                retry_attempts,
                retry_budget_exhausted,
                connection_acquires,
                connection_timeouts,
                connection_acquire_duration,
                savepoints_total,
                savepoints_rolled_back,
                leak_warnings,
            },
            collectors,
        ))
    }

    /// Records a serialization conflict (SQLSTATE 40001).
    pub fn record_serialization_conflict(&self) {
        self.serialization_conflicts.inc();
    }

    /// Records a deadlock detection (SQLSTATE 40P01).
    pub fn record_deadlock(&self) {
        self.deadlocks.inc();
    }

    /// Records one retry budget spent without success, under the SQLSTATE that spent it.
    pub fn record_retry_exhausted(&self, sqlstate: &str) {
        self.retry_budget_exhausted
            .with_label_values(&[sqlstate])
            .inc();
    }

    /// Records one retry attempt under the SQLSTATE that caused it, moving the per-class counters too.
    pub fn record_retry(&self, sqlstate: &str) {
        self.retry_attempts.with_label_values(&[sqlstate]).inc();
        match sqlstate {
            "40001" => self.serialization_conflicts.inc(),
            "40P01" => self.deadlocks.inc(),
            _ => {}
        }
    }

    /// Records a transaction start.
    pub fn record_transaction_start(&self) {
        self.transactions_total.inc();
    }

    /// Records a transaction completion.
    pub fn record_transaction_completion(&self, committed: bool, duration: Duration) {
        if committed {
            self.transactions_committed.inc();
            self.transaction_duration
                .with_label_values(&["committed"])
                .observe(duration.as_secs_f64());
        } else {
            self.transactions_rolled_back.inc();
            self.transaction_duration
                .with_label_values(&["rolled_back"])
                .observe(duration.as_secs_f64());
        }
    }

    /// Records a savepoint creation.
    pub fn record_savepoint(&self) {
        self.savepoints_total.inc();
    }

    /// Records a savepoint rollback.
    pub fn record_savepoint_rollback(&self) {
        self.savepoints_rolled_back.inc();
    }

    /// Records a connection leak warning.
    pub fn record_leak_warning(&self) {
        self.leak_warnings.inc();
    }

    /// Encodes all metrics in Prometheus text format.
    ///
    /// # Errors
    ///
    /// Returns a [`prometheus::Error`] if the registry fails to encode, or if the
    /// encoder produces non-UTF-8 bytes.
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

    /// Returns the underlying registry, for registering custom metrics.
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

    fn record_connection_acquire(&self, duration: Duration, success: bool) {
        self.connection_acquires.inc();

        let success_str = if success { "true" } else { "false" };
        self.connection_acquire_duration
            .with_label_values(&[success_str])
            .observe(duration.as_secs_f64());

        if !success {
            self.connection_timeouts.inc();
        }
    }

    fn record_retry(&self, sqlstate: &str) {
        Self::record_retry(self, sqlstate);
    }

    fn record_retry_exhausted(&self, sqlstate: &str) {
        Self::record_retry_exhausted(self, sqlstate);
    }

    fn record_slow_query(&self, operation: &str, table: &str, _duration: Duration) {
        self.slow_queries
            .with_label_values(&[operation, table])
            .inc();
    }

    fn record_transaction(&self, committed: bool, duration: Duration) {
        self.record_transaction_completion(committed, duration);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_configured_duration_buckets_reach_the_histogram() {
        let config = fundamental_db::MetricsConfig::default().duration_buckets(vec![0.5, 42.0]);
        let metrics = PrometheusMetrics::from_metrics_config("cfg_probe", &config)
            .expect("the collector registers");
        // A `HistogramVec` exports no series until a labelled observation exists.
        metrics.record_query("select", "widgets", Duration::from_millis(1), true);
        let exported = metrics.encode().expect("the registry encodes");
        assert!(
            exported.contains("le=\"42\""),
            "a configured bucket must appear in the export; got:\n{exported}"
        );
    }

    #[test]
    fn the_default_buckets_are_the_configs_default() {
        assert_eq!(
            DEFAULT_DURATION_BUCKETS.to_vec(),
            fundamental_db::MetricsConfig::default().duration_buckets,
        );
    }

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
        assert_eq!(collectors.len(), 18, "every family is handed over");

        let names: Vec<String> = collectors
            .iter()
            .flat_map(|collector| collector.desc())
            .map(|desc| desc.fq_name.clone())
            .collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            names.len(),
            "two families answering to one name: the caller registers the first and the \
             second is silently the same series — {names:?}"
        );

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
            .find(|desc| desc.fq_name == "acme_sql_queries_total")
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
            rendered.contains("primary_sql_"),
            "the first pool must be in the scrape: {rendered}"
        );
        assert!(
            rendered.contains("replica_sql_"),
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

        metrics.record_query("select", "users", Duration::from_millis(10), true);
        metrics.record_query("insert", "users", Duration::from_millis(20), true);
        metrics.record_query("select", "users", Duration::from_millis(15), false);

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_queries_total"));
        assert!(output.contains("sql_query_duration_seconds"));
        assert!(output.contains("sql_query_errors_total"));
    }

    #[test]
    fn test_record_slow_query() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_slow_query("select", "users", Duration::from_secs(2));

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_slow_queries_total"));
    }

    #[test]
    fn test_record_transaction() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_transaction_start();
        metrics.record_transaction(true, Duration::from_millis(50));

        metrics.record_transaction_start();
        metrics.record_transaction(false, Duration::from_millis(30));

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_transactions_total"));
        assert!(output.contains("sql_transactions_committed_total"));
        assert!(output.contains("sql_transactions_rolled_back_total"));
        assert!(output.contains("sql_transaction_duration_seconds"));
    }

    #[test]
    fn test_record_serialization_conflict() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_retry("40001");

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_serialization_conflicts_total"));
        assert!(output.contains("sql_retry_attempts_total"));
    }

    #[test]
    fn a_retry_lands_under_its_own_sqlstate() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_retry("40P01");
        metrics.record_retry("40P01");
        metrics.record_retry("40001");
        metrics.record_retry("unknown");

        let output = metrics.encode().unwrap();
        assert!(
            output.contains(r#"sql_retry_attempts_total{sqlstate="40P01"} 2"#),
            "deadlocks counted under 40P01:\n{output}"
        );
        assert!(
            output.contains(r#"sql_retry_attempts_total{sqlstate="40001"} 1"#),
            "serialization failures counted under 40001:\n{output}"
        );
        assert!(
            output.contains(r#"sql_retry_attempts_total{sqlstate="unknown"} 1"#),
            "an unlabelled retry is visible, not dropped:\n{output}"
        );
        assert!(output.contains("sql_deadlocks_total 2"), "{output}");
        assert!(
            output.contains("sql_serialization_conflicts_total 1"),
            "{output}"
        );
    }

    #[test]
    fn test_record_connection_acquire() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_connection_acquire(Duration::from_millis(5), true);
        metrics.record_connection_acquire(Duration::from_millis(100), false);

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_connection_acquires_total"));
        assert!(output.contains("sql_connection_timeouts_total"));
        assert!(output.contains("sql_connection_acquire_duration_seconds"));
    }

    #[test]
    fn test_record_savepoints() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_savepoint();
        metrics.record_savepoint();
        metrics.record_savepoint_rollback();

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_savepoints_total"));
        assert!(output.contains("sql_savepoints_rolled_back_total"));
    }

    #[test]
    fn test_record_deadlock() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_deadlock();

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_deadlocks_total"));
    }

    #[test]
    fn test_record_leak_warning() {
        let metrics = PrometheusMetrics::new("test").unwrap();

        metrics.record_leak_warning();

        let output = metrics.encode().unwrap();
        assert!(output.contains("sql_leak_warnings_total"));
    }
}
