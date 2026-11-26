//! The [`CqlExecutor`] — the one place this crate runs CQL.

use crate::CqlSession;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use fundamental_db::DatabaseMetrics;
use futures::StreamExt;
use scylla::client::pager::{NextPageError, NextRowError};
use scylla::client::session::Session;
use scylla::deserialize::row::DeserializeRow;
use scylla::errors::PagerExecutionError;
use scylla::statement::Statement;
use scylla::statement::prepared::PreparedStatement;
use tracing::field::Empty;
use tracing::{Span, instrument};

use crate::metrics::TimingGuard;
use crate::{
    Consistency, Cql, CqlError, CqlResult, DEFAULT_DDL_TIMEOUT, Ddl, PagingConfig, PreparedCache,
};

/// Runs CQL against a keyspace: prepares, applies consistency, executes, maps
/// the driver's error, materialises rows, and records the timing.
///
/// Every read is paged and bounded. Timings are labelled by keyspace and a fixed operation
/// name, never by statement text, so the metric's cardinality stays bounded.
pub struct CqlExecutor {
    session: CqlSession,
    cache: Arc<PreparedCache>,
    keyspace: String,
    read_consistency: Consistency,
    write_consistency: Consistency,
    metrics: Option<Arc<dyn DatabaseMetrics>>,
    slow_after: Duration,
    ddl_timeout: Duration,
}

impl CqlExecutor {
    /// Builds an executor over a session's keyspace.
    #[must_use]
    pub fn new(
        session: CqlSession,
        cache: Arc<PreparedCache>,
        keyspace: impl Into<String>,
        read_consistency: Consistency,
        write_consistency: Consistency,
    ) -> Self {
        Self {
            session,
            cache,
            keyspace: keyspace.into(),
            read_consistency,
            write_consistency,
            metrics: None,
            slow_after: DEFAULT_SLOW_AFTER,
            ddl_timeout: DEFAULT_DDL_TIMEOUT,
        }
    }

    /// Sets the deadline a schema statement runs under.
    #[must_use]
    pub fn with_ddl_timeout(mut self, timeout: Duration) -> Self {
        self.ddl_timeout = timeout;
        self
    }

    /// Attaches the collector this executor records its timings to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>, slow_after: Duration) -> Self {
        self.metrics = Some(metrics);
        self.slow_after = slow_after;
        self
    }

    /// The keyspace this executor addresses.
    #[must_use]
    pub fn keyspace(&self) -> &str {
        &self.keyspace
    }

    /// The deadline a schema statement run through this executor gets.
    #[must_use]
    pub fn ddl_timeout(&self) -> Duration {
        self.ddl_timeout
    }

    /// The driver session, for a logged batch, which the driver takes as a `Batch`.
    pub(crate) fn session(&self) -> Arc<Session> {
        self.session.inner()
    }

    /// Runs a schema statement.
    ///
    /// Never prepared, as CQL does not allow it, and never retried: a CQL timeout does not say
    /// whether the statement applied. Its deadline is
    /// [`CqlConfig::ddl_timeout`](crate::CqlConfig::ddl_timeout).
    ///
    /// # Errors
    ///
    /// [`CqlError::Query`] if the statement fails.
    #[instrument(
        level = "debug",
        skip_all,
        fields(op = "ddl", keyspace = %self.keyspace, timeout_ms = self.ddl_timeout.as_millis()),
        err(Debug)
    )]
    pub async fn execute_ddl(&self, statement: &Ddl) -> CqlResult<()> {
        let mut unprepared = Statement::new(statement.text().to_owned());
        unprepared.set_request_timeout(Some(self.ddl_timeout));
        self.timed("ddl", async {
            self.session
                .inner()
                .query_unpaged(unprepared, &[])
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            Ok(())
        })
        .await
    }

    /// Runs a data statement that returns no rows.
    ///
    /// CQL has no affected-row count, so none is returned.
    ///
    /// # Errors
    ///
    /// [`CqlError::Prepared`] if the statement cannot be prepared, or
    /// [`CqlError::Query`] if it fails.
    #[instrument(level = "debug", skip_all, fields(op = "execute", keyspace = %self.keyspace), err(Debug))]
    pub async fn execute(&self, statement: &Cql) -> CqlResult<()> {
        self.timed("execute", async {
            let prepared = self.prepare(statement, self.write_consistency).await?;
            self.session
                .inner()
                .execute_unpaged(&prepared, statement.values())
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            Ok(())
        })
        .await
    }

    /// Runs a conditional write (`… IF NOT EXISTS`, `… IF <column> = ?`) and reports whether
    /// its condition held — the store's `[applied]`.
    ///
    /// # Errors
    ///
    /// [`CqlError::Prepared`] or [`CqlError::Query`] as [`execute`](Self::execute), and
    /// [`CqlError::LwtUnconfirmed`] when the result does not say whether it applied — which is
    /// never read as either answer.
    #[instrument(level = "debug", skip_all, fields(op = "execute_conditional", keyspace = %self.keyspace), err(Debug))]
    pub async fn execute_conditional(&self, statement: &Cql) -> CqlResult<bool> {
        self.timed("execute_conditional", async {
            let prepared = self.prepare(statement, self.write_consistency).await?;
            let result = self
                .session
                .inner()
                .execute_unpaged(&prepared, statement.values())
                .await
                .map_err(|e| CqlError::Query(Box::new(e)))?;
            crate::repository::parse_lwt_result(result)
        })
        .await
    }

    /// Reads rows, refusing a result set larger than
    /// [`DEFAULT_MAX_UNPAGED_RESULTS`](crate::DEFAULT_MAX_UNPAGED_RESULTS).
    ///
    /// The read stops at the cap, so its memory is bounded.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidQuery`] if the result set is larger than the cap,
    /// plus the errors of [`fetch_paged`](Self::fetch_paged).
    pub async fn fetch_all<R>(&self, statement: &Cql) -> CqlResult<Vec<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        self.fetch_paged(statement, crate::DEFAULT_MAX_UNPAGED_RESULTS)
            .await
    }

    /// Reads up to `max_rows` rows, a page at a time.
    ///
    /// At most `max_rows + 1` rows are ever held: the extra one is how the cap
    /// is detected, and it is not returned.
    ///
    /// # Errors
    ///
    /// [`CqlError::Prepared`] if the statement cannot be prepared,
    /// [`CqlError::Query`] if a page fails to fetch,
    /// [`CqlError::RowConversion`] if a row does not match `R`, and
    /// [`CqlError::InvalidQuery`] if there are more than `max_rows` rows.
    pub async fn fetch_paged<R>(&self, statement: &Cql, max_rows: usize) -> CqlResult<Vec<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        match self.read(statement, max_rows).await? {
            (rows, false) => Ok(rows),
            (_, true) => Err(CqlError::invalid_query(format!(
                "result set exceeds {max_rows} rows; narrow the partition or raise the \
                 bound with fetch_paged"
            ))),
        }
    }

    /// Reads up to `max_rows` rows, reporting whether there was at least one more.
    ///
    /// A flag rather than an error: the two callers mean different things by passing the bound.
    #[instrument(
        level = "debug",
        skip_all,
        fields(op = "fetch", keyspace = %self.keyspace, max_rows, rows = Empty, more = Empty),
        err(Debug)
    )]
    async fn read<R>(&self, statement: &Cql, max_rows: usize) -> CqlResult<(Vec<R>, bool)>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        self.timed("fetch", async {
            let prepared = self.prepare(statement, self.read_consistency).await?;
            let pager = self
                .session
                .inner()
                .execute_iter(prepared, statement.values())
                .await
                .map_err(pager_error)?;
            let mut stream = pager
                .rows_stream::<R>()
                .map_err(|e| CqlError::row_conversion(e.to_string()))?;

            let mut rows = Vec::new();
            let mut more = false;
            while let Some(row) = stream.next().await {
                let row = row.map_err(next_row_error)?;
                if rows.len() >= max_rows {
                    // One row past the bound is read and dropped: it is how the cap is detected.
                    more = true;
                    break;
                }
                rows.push(row);
            }
            let span = Span::current();
            span.record("rows", rows.len());
            span.record("more", more);
            Ok((rows, more))
        })
        .await
    }

    /// Reads exactly one row.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidQuery`] if the statement matched no row or more
    /// than one, plus the errors of [`fetch_paged`](Self::fetch_paged).
    pub async fn fetch_one<R>(&self, statement: &Cql) -> CqlResult<R>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        self.fetch_optional(statement)
            .await?
            .ok_or_else(|| CqlError::invalid_query("expected one row, found none"))
    }

    /// Reads at most one row.
    ///
    /// A second matching row is an error, not a silent discard.
    ///
    /// # Errors
    ///
    /// [`CqlError::InvalidQuery`] if the statement matched more than one
    /// row, plus the errors of [`fetch_paged`](Self::fetch_paged).
    pub async fn fetch_optional<R>(&self, statement: &Cql) -> CqlResult<Option<R>>
    where
        R: for<'frame, 'metadata> DeserializeRow<'frame, 'metadata>,
    {
        match self.read::<R>(statement, 1).await? {
            (mut rows, false) => Ok(rows.pop()),
            (_, true) => Err(CqlError::invalid_query(
                "expected at most one row, found more",
            )),
        }
    }

    /// Prepares `statement` through the cache and applies its options.
    async fn prepare(
        &self,
        statement: &Cql,
        default_consistency: Consistency,
    ) -> CqlResult<PreparedStatement> {
        let cached = self.cache.get_or_prepare(statement.text()).await?;
        let mut prepared = (*cached).clone();
        prepared.set_consistency(
            statement
                .consistency_override()
                .unwrap_or(default_consistency)
                .to_driver(),
        );
        prepared.set_page_size(
            statement
                .page_size_override()
                .unwrap_or(default_page_size()),
        );
        prepared.set_is_idempotent(statement.is_idempotent());
        Ok(prepared)
    }

    /// Runs `run` under a timing guard when a collector is attached.
    async fn timed<T, F>(&self, operation: &'static str, run: F) -> CqlResult<T>
    where
        F: Future<Output = CqlResult<T>>,
    {
        let Some(metrics) = self.metrics.as_ref() else {
            return run.await;
        };
        let mut guard =
            TimingGuard::new(metrics.as_ref(), operation, &self.keyspace, self.slow_after);
        let result = run.await;
        if result.is_err() {
            guard.mark_failed();
        }
        result
    }
}

impl std::fmt::Debug for CqlExecutor {
    /// The collector is not `Debug`; whether one is attached is what is printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqlExecutor")
            .field("keyspace", &self.keyspace)
            .field("read_consistency", &self.read_consistency)
            .field("write_consistency", &self.write_consistency)
            .field("has_metrics", &self.metrics.is_some())
            .finish()
    }
}

/// The elapsed time above which a statement is additionally recorded as slow.
///
/// `fundamental_db::MetricsConfig`'s default, as `CqlConfig` carries no threshold.
const DEFAULT_SLOW_AFTER: Duration = Duration::from_secs(1);

/// The page size a paged read uses when the statement names none.
fn default_page_size() -> i32 {
    PagingConfig::default().page_size
}

/// Maps the pager's setup failure onto this crate's error.
///
/// A transport failure widens into [`CqlError::Query`], keeping its retry classification.
fn pager_error(err: PagerExecutionError) -> CqlError {
    match err {
        PagerExecutionError::PrepareError(e) => CqlError::prepared(e.to_string()),
        PagerExecutionError::SerializationError(e) => CqlError::invalid_query(e.to_string()),
        PagerExecutionError::NextPageError(e) => next_page_error(e),
        other => CqlError::paged(other.to_string()),
    }
}

/// Maps a per-row streaming failure onto this crate's error.
fn next_row_error(err: NextRowError) -> CqlError {
    match err {
        NextRowError::NextPageError(e) => next_page_error(e),
        NextRowError::RowDeserializationError(e) => CqlError::row_conversion(e.to_string()),
        other => CqlError::paged(other.to_string()),
    }
}

/// Maps a page fetch failure, preserving retryability where the driver has it.
fn next_page_error(err: NextPageError) -> CqlError {
    match err {
        NextPageError::RequestFailure(e) => CqlError::Query(Box::new(e.into_execution_error())),
        NextPageError::TypeCheckError(e) => CqlError::row_conversion(e.to_string()),
        NextPageError::ResultMetadataParseError(e) => CqlError::row_conversion(e.to_string()),
        other => CqlError::paged(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_page_size_is_the_crates_own() {
        assert_eq!(default_page_size(), PagingConfig::default().page_size);
    }

    #[test]
    fn a_transport_failure_mid_page_stays_retryable() {
        let err = next_page_error(NextPageError::RequestFailure(
            scylla::errors::RequestError::RequestTimeout(Duration::from_secs(1)),
        ));

        assert!(matches!(err, CqlError::Query(_)));
        assert!(err.is_retryable(), "a request timeout is retryable");
    }
}
