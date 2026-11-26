//! Atomic counter operations for YCQL.

use crate::CqlSession;
use std::{marker::PhantomData, sync::Arc, time::Duration};

use fundamental_db::DatabaseMetrics;
use tracing::{debug, instrument, warn};

use crate::ident::validate_cql_identifier;
use crate::metrics::TimingGuard;
use crate::{
    CounterSelectBuilder, CounterUpdateBuilder, CqlError, CqlRepository, CqlResult, PreparedCache,
};

/// Counter operations for YCQL.
pub struct CounterOps {
    session: CqlSession,
    cache: Arc<PreparedCache>,
    keyspace: String,
    table: String,
    /// Where this handle's timings are recorded, and the slow threshold.
    metrics: Option<(Arc<dyn DatabaseMetrics>, Duration)>,
}

impl CounterOps {
    /// Creates counter operations for a specific table.
    ///
    /// Prefer [`CqlPool::counters`](crate::CqlPool::counters); built by hand, this handle is
    /// unmeasured.
    pub fn new(
        session: CqlSession,
        cache: Arc<PreparedCache>,
        keyspace: impl Into<String>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            session,
            cache,
            keyspace: keyspace.into(),
            table: table.into(),
            metrics: None,
        }
    }

    /// Attaches the collector this handle records its timings to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>, slow_after: Duration) -> Self {
        self.metrics = Some((metrics, slow_after));
        self
    }

    /// Runs `run` under a timing guard when a collector is attached.
    async fn timed<T, F>(&self, operation: &'static str, run: F) -> CqlResult<T>
    where
        F: std::future::Future<Output = CqlResult<T>>,
    {
        let Some((metrics, slow_after)) = self.metrics.as_ref() else {
            return run.await;
        };
        let mut guard = TimingGuard::new(metrics.as_ref(), operation, &self.table, *slow_after);
        let result = run.await;
        if result.is_err() {
            guard.mark_failed();
        }
        result
    }

    /// Increments a counter column for a text key and i64 window.
    ///
    /// Returns the new counter value after incrementing.
    #[instrument(
        skip(self, key),
        fields(table = self.table, counter_column, window_start, increment),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn increment_rate_limit(
        &self,
        counter_column: &str,
        key: &str,
        window_start: i64,
        increment: i64,
    ) -> CqlResult<i64> {
        // `counter_column` is the only caller-supplied identifier; the
        // `where_columns` are trusted in-crate literals.
        validate_cql_identifier(counter_column, false)?;

        let cql = CounterUpdateBuilder::new(&self.keyspace, &self.table, counter_column)
            .where_columns(&["key", "window_start"])
            .build();

        debug!(cql = %cql, key = key, window_start = window_start, increment = increment, "Incrementing rate limit counter");

        let stmt = self.cache.get_or_prepare(&cql).await?;

        // Bound as a plain `i64`: YCQL declares the parameter `Native(BigInt)`, so a `Counter` bind
        // fails the driver's type check.
        self.timed("counter_increment", async {
            self.session
                .inner()
                .execute_unpaged(&stmt, (increment, key, window_start))
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            Ok(())
        })
        .await?;

        self.get_rate_limit_counter(counter_column, key, window_start)
            .await
    }

    /// Clears one window's counter cell.
    ///
    /// Counter columns cannot expire, so windows are cleared explicitly. Only clear a window
    /// already in the past: a deleted and re-incremented counter cell has an undefined value.
    ///
    /// # Errors
    /// Returns [`CqlError`] if the delete could not be executed.
    #[instrument(
        skip(self, key),
        fields(table = self.table, counter_column, window_start),
        err(Debug)
    )]
    pub async fn clear_rate_limit_window(
        &self,
        counter_column: &str,
        key: &str,
        window_start: i64,
    ) -> CqlResult<()> {
        validate_cql_identifier(counter_column, false)?;

        let cql = format!(
            "DELETE {counter_column} FROM {} WHERE key = ? AND window_start = ?",
            crate::qualify(&self.keyspace, &self.table)
        );

        debug!(cql = %cql, window_start, "Clearing rate limit counter window");

        let stmt = self.cache.get_or_prepare(&cql).await?;
        self.timed("counter_delete", async {
            self.session
                .inner()
                .execute_unpaged(&stmt, (key, window_start))
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            Ok(())
        })
        .await
    }

    /// Increments a rate limit counter with TTL.
    ///
    /// Only for a plain integer column: YCQL rejects `USING TTL` on a counter update.
    #[instrument(
        skip(self, key),
        fields(table = self.table, counter_column, window_start, increment, ttl = ttl_seconds),
        err(Debug)
    )]
    #[expect(
        clippy::too_many_arguments,
        clippy::missing_errors_doc,
        reason = "readability baseline"
    )]
    pub async fn increment_rate_limit_with_ttl(
        &self,
        counter_column: &str,
        key: &str,
        window_start: i64,
        increment: i64,
        ttl_seconds: i32,
    ) -> CqlResult<i64> {
        validate_cql_identifier(counter_column, false)?;

        let cql = CounterUpdateBuilder::new(&self.keyspace, &self.table, counter_column)
            .where_columns(&["key", "window_start"])
            .ttl(ttl_seconds)
            .build();

        debug!(cql = %cql, key = key, window_start = window_start, increment = increment, ttl = ttl_seconds, "Incrementing rate limit counter with TTL");

        let stmt = self.cache.get_or_prepare(&cql).await?;

        self.timed("counter_increment", async {
            self.session
                .inner()
                .execute_unpaged(&stmt, (increment, key, window_start))
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            Ok(())
        })
        .await?;

        self.get_rate_limit_counter(counter_column, key, window_start)
            .await
    }

    /// Gets the current counter value for a rate limit key.
    #[instrument(
        level = "debug",
        skip(self, key),
        fields(table = self.table, counter_column, window_start)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn get_rate_limit_counter(
        &self,
        counter_column: &str,
        key: &str,
        window_start: i64,
    ) -> CqlResult<i64> {
        validate_cql_identifier(counter_column, false)?;

        let cql = CounterSelectBuilder::new(&self.keyspace, &self.table, counter_column)
            .where_columns(&["key", "window_start"])
            .build();

        debug!(cql = %cql, key = key, window_start = window_start, "Reading rate limit counter");

        let stmt = self.cache.get_or_prepare(&cql).await?;

        let result = self
            .timed("counter_select", async {
                self.session
                    .inner()
                    .execute_unpaged(&stmt, (key, window_start))
                    .await
                    .map_err(|e| CqlError::Query(Box::new(e)))
            })
            .await?;

        // A decode fault must not read as a zero counter, which would let traffic through.
        let rows_result = result.into_rows_result().map_err(|e| {
            warn!(table = %self.table, counter_column, error = %e, "rate-limit counter: query did not return a rows result");
            CqlError::RowConversion(format!(
                "{}.{counter_column}: expected rows result for counter SELECT: {e}",
                self.table
            ))
        })?;
        // Read untyped: Cassandra sends `Native(Counter)` and YCQL `Native(BigInt)` for a counter.
        let mut rows = rows_result.rows::<(scylla::value::CqlValue,)>().map_err(|e| {
            warn!(table = %self.table, counter_column, error = %e, "rate-limit counter: failed to decode counter row");
            CqlError::RowConversion(format!(
                "{}.{counter_column}: failed to decode counter value: {e}",
                self.table
            ))
        })?;
        if let Some(row) = rows.next() {
            let (counter,) = row.map_err(|e| {
                warn!(table = %self.table, counter_column, error = %e, "rate-limit counter: failed to read counter row");
                CqlError::RowConversion(format!(
                    "{}.{counter_column}: failed to read counter value: {e}",
                    self.table
                ))
            })?;
            return match counter {
                scylla::value::CqlValue::Counter(value) => Ok(value.0),
                scylla::value::CqlValue::BigInt(value) => Ok(value),
                other => {
                    warn!(
                        table = %self.table, counter_column, ?other,
                        "rate-limit counter: the counter column is neither a counter nor a bigint"
                    );
                    Err(CqlError::RowConversion(format!(
                        "{}.{counter_column}: counter column returned {other:?}",
                        self.table
                    )))
                }
            };
        }

        // No row exists yet → the counter is genuinely 0 (legitimate, not a fault).
        Ok(0)
    }
}

impl std::fmt::Debug for CounterOps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CounterOps")
            .field("keyspace", &self.keyspace)
            .field("table", &self.table)
            .finish()
    }
}

impl<M> Clone for CqlRepository<M> {
    fn clone(&self) -> Self {
        Self {
            session: self.session.clone(),
            cache: Arc::clone(&self.cache),
            keyspace: self.keyspace.clone(),
            read_consistency: self.read_consistency,
            write_consistency: self.write_consistency,
            key_manager: self.key_manager.clone(),
            metrics: self.metrics.clone(),
            _marker: PhantomData,
        }
    }
}
