//! Model-scoped YCQL repository.

use crate::CqlSession;
use std::{marker::PhantomData, sync::Arc};

use fundamental_core::model::{BoundTo, CqlDb, CqlModel};
use scylla::response::query_result::QueryResult;
use scylla::statement::prepared::PreparedStatement;
use tracing::{debug, instrument, warn};

use crate::{
    BatchConfig, BatchType, Consistency, CqlError, CqlQuery, CqlResult, DeleteBatch, DeleteBuilder,
    FromRow, InsertBatch, InsertBuilder, NoPartitionKey, PreparedCache, ToRow, UpdateBuilder,
};

/// YCQL repository for a specific model.
///
/// A query must bind a partition key before it runs, enforced by the builder's typestate.
pub struct CqlRepository<M> {
    /// The underlying session.
    pub session: CqlSession,
    /// Prepared statement cache.
    pub cache: Arc<PreparedCache>,
    /// The keyspace queries are issued against; empty means the session's `USE`d keyspace.
    pub keyspace: String,
    /// Default read consistency.
    pub read_consistency: Consistency,
    /// Default write consistency.
    pub write_consistency: Consistency,
    /// Where this repository's query timings are recorded, and the slow-query threshold.
    pub metrics: Option<(
        Arc<dyn fundamental_db::DatabaseMetrics>,
        std::time::Duration,
    )>,
    /// The key manager that encrypts `Secret` model fields at rest.
    pub key_manager: Option<fundamental_encryption::SharedKeyManager>,
    /// Model marker.
    pub _marker: PhantomData<M>,
}

impl<M> CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + Send + Sync,
{
    /// Creates a new repository.
    pub(crate) fn new(
        session: CqlSession,
        cache: Arc<PreparedCache>,
        keyspace: String,
        read_consistency: Consistency,
        write_consistency: Consistency,
    ) -> Self {
        Self {
            session,
            cache,
            keyspace,
            read_consistency,
            write_consistency,
            key_manager: None,
            metrics: None,
            _marker: PhantomData,
        }
    }

    /// Attaches a key manager, enabling transparent at-rest encryption of the
    /// model's `Secret` fields.
    pub fn with_key_manager(
        mut self,
        key_manager: fundamental_encryption::SharedKeyManager,
    ) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Attaches the collector this repository records its query timings to.
    #[must_use]
    pub fn with_metrics(
        mut self,
        metrics: Arc<dyn fundamental_db::DatabaseMetrics>,
        slow_after: std::time::Duration,
    ) -> Self {
        self.metrics = Some((metrics, slow_after));
        self
    }

    /// Prepares `cql` and stamps this repository's **read** consistency on it.
    ///
    /// # Errors
    /// [`CqlError::Prepared`](crate::CqlError::Prepared) if the statement
    /// cannot be prepared.
    pub(crate) async fn prepared_read(&self, cql: &str) -> CqlResult<PreparedStatement> {
        self.prepared(cql, self.read_consistency).await
    }

    /// Prepares `cql` and stamps this repository's **write** consistency on it.
    ///
    /// # Errors
    /// [`CqlError::Prepared`](crate::CqlError::Prepared) if the statement
    /// cannot be prepared.
    pub(crate) async fn prepared_write(&self, cql: &str) -> CqlResult<PreparedStatement> {
        self.prepared(cql, self.write_consistency).await
    }

    /// Prepares `cql` at `consistency`.
    async fn prepared(&self, cql: &str, consistency: Consistency) -> CqlResult<PreparedStatement> {
        let cached = self.cache.get_or_prepare(cql).await?;
        let mut prepared = (*cached).clone();
        prepared.set_consistency(consistency.to_driver());
        Ok(prepared)
    }

    /// Runs `operation` under a timing guard when a collector is attached.
    pub(crate) async fn timed<T, E, F>(&self, operation: &'static str, run: F) -> Result<T, E>
    where
        F: std::future::Future<Output = Result<T, E>>,
    {
        let Some((metrics, slow_after)) = self.metrics.as_ref() else {
            return run.await;
        };
        let mut guard =
            crate::metrics::TimingGuard::new(metrics.as_ref(), operation, M::TABLE, *slow_after);
        let result = run.await;
        if result.is_err() {
            guard.mark_failed();
        }
        result
    }

    /// Returns the attached key manager, if any.
    pub fn key_manager(&self) -> Option<&fundamental_encryption::SharedKeyManager> {
        self.key_manager.as_ref()
    }

    /// Returns a query builder for this model.
    ///
    /// The query starts in `NoPartitionKey` state and must have a partition
    /// key specified before execution.
    pub fn query(&self) -> CqlQuery<M, NoPartitionKey> {
        CqlQuery::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.keyspace.clone(),
            self.key_manager.clone(),
        )
    }

    /// Returns the session.
    pub fn session(&self) -> &CqlSession {
        &self.session
    }

    /// Returns the prepared statement cache.
    pub fn cache(&self) -> &Arc<PreparedCache> {
        &self.cache
    }
}

/// Private write and delete helpers shared by the abstract repository impls and the YCQL
/// extension traits.
impl<M> CqlRepository<M>
where
    M: BoundTo<CqlDb> + CqlModel + ToRow + FromRow + Send + Sync,
{
    /// Executes an `INSERT` (which in YCQL is an upsert), optionally with a
    /// TTL and/or `IF NOT EXISTS`, then writes any translation companion rows.
    ///
    /// Returns the LWT `[applied]` flag when `if_not_exists` is set, `true` otherwise.
    #[instrument(
        skip(self, model),
        fields(table = M::TABLE, model = M::TYPE_NAME, operation = "insert", ttl = ?ttl, if_not_exists)
    )]
    pub(super) async fn exec_insert(
        &self,
        model: &M,
        ttl: Option<i32>,
        write_time: Option<i64>,
        if_not_exists: bool,
    ) -> CqlResult<bool> {
        let mut builder = InsertBuilder::new(&self.keyspace, M::TABLE).columns_from::<M>();
        if let Some(ttl_seconds) = ttl {
            builder = builder.ttl(ttl_seconds);
        }
        if let Some(micros) = write_time {
            builder = builder.write_time(micros);
        }
        if if_not_exists {
            builder = builder.if_not_exists();
        }
        let cql = builder.build();

        debug!(cql = %cql, ttl = ?ttl, if_not_exists, "Executing insert");

        let stmt = self.prepared_write(&cql).await?;

        // Bind model values, encrypting any `Secret` columns through the key manager.
        let values = model.to_values_encrypted(self.key_manager.as_deref())?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, &values)
            .await
            .map_err(|e| CqlError::Query(Box::new(e)))?;

        // A failed condition leaves the record's translations as they are.
        let applied = !if_not_exists || parse_lwt_result(result)?;
        if applied {
            crate::translations::write_translations(
                &self.session,
                &self.cache,
                &self.keyspace,
                model,
            )
            .await?;
        }
        Ok(applied)
    }

    /// Executes an `UPDATE`, optionally with a TTL and/or `IF EXISTS`, then
    /// writes any translation companion rows.
    ///
    /// Returns the LWT `[applied]` flag when `if_exists` is set, `true` otherwise.
    #[instrument(
        skip(self, model),
        fields(table = M::TABLE, model = M::TYPE_NAME, operation = "update", ttl = ?ttl, if_exists)
    )]
    pub(super) async fn exec_update(
        &self,
        model: &M,
        ttl: Option<i32>,
        write_time: Option<i64>,
        if_exists: bool,
    ) -> CqlResult<bool> {
        let mut builder = UpdateBuilder::new(&self.keyspace, M::TABLE)
            .set_columns_from::<M>()
            .where_columns_from::<M>();
        if let Some(ttl_seconds) = ttl {
            builder = builder.ttl(ttl_seconds);
        }
        if let Some(micros) = write_time {
            builder = builder.write_time(micros);
        }
        if if_exists {
            builder = builder.if_exists();
        }
        let cql = builder.build();

        debug!(cql = %cql, ttl = ?ttl, if_exists, "Executing update");

        let stmt = self.prepared_write(&cql).await?;

        // Bind values (SET cols then WHERE cols), encrypting any `Secret` columns.
        let values = model.to_update_values_encrypted(self.key_manager.as_deref())?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, &values)
            .await
            .map_err(|e| CqlError::Query(Box::new(e)))?;

        // A record that was not there gets no translations either.
        let applied = !if_exists || parse_lwt_result(result)?;
        if applied {
            crate::translations::write_translations(
                &self.session,
                &self.cache,
                &self.keyspace,
                model,
            )
            .await?;
        }
        Ok(applied)
    }

    /// Executes a `DELETE` addressed by the composite primary key, optionally
    /// with `IF EXISTS`.
    ///
    /// Returns the LWT `[applied]` flag when `if_exists` is set, `true` otherwise. Deletes only
    /// the main row.
    #[instrument(
        skip(self, keys),
        fields(table = M::TABLE, model = M::TYPE_NAME, operation = "delete", if_exists)
    )]
    pub(super) async fn exec_delete_keys(
        &self,
        keys: &M::KeyValues,
        if_exists: bool,
    ) -> CqlResult<bool>
    where
        M::KeyValues: scylla::serialize::row::SerializeRow,
    {
        let mut builder = DeleteBuilder::new(&self.keyspace, M::TABLE).where_columns_from::<M>();
        if if_exists {
            builder = builder.if_exists();
        }
        let cql = builder.build();

        debug!(cql = %cql, if_exists, "Executing delete");

        let stmt = self.prepared_write(&cql).await?;
        let result = self
            .session
            .inner()
            .execute_unpaged(&stmt, keys)
            .await
            .map_err(|e| CqlError::Query(Box::new(e)))?;

        if if_exists {
            return parse_lwt_result(result);
        }
        Ok(true)
    }

    /// Removes the translation companion rows of the record keyed `key`. Used by the
    /// delete paths after the main row has been removed.
    #[instrument(
        skip(self, key),
        fields(table = M::TABLE, model = M::TYPE_NAME, operation = "delete_translations")
    )]
    pub(super) async fn exec_delete_translations(&self, key: &M::KeyValues) -> CqlResult<()> {
        crate::translations::delete_translations::<M>(
            &self.session,
            &self.cache,
            &self.keyspace,
            key,
        )
        .await
    }

    /// Finds the keys of the records whose translatable `field` exactly equals `value`
    /// in `locale`. Backs [`fundamental_db::RepositoryTranslation`].
    #[instrument(
        level = "debug",
        skip(self, value),
        fields(table = M::TABLE, model = M::TYPE_NAME, operation = "find_by_translation", field, locale)
    )]
    pub(super) async fn exec_find_keys_by_translation(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> CqlResult<Vec<M::KeyValues>>
    where
        M::KeyValues:
            for<'frame, 'metadata> scylla::deserialize::row::DeserializeRow<'frame, 'metadata>,
    {
        crate::translations::find_keys::<M>(
            &self.session,
            &self.cache,
            &self.keyspace,
            field,
            locale,
            value,
        )
        .await
    }

    /// Executes a batched `INSERT`, optionally with a TTL. Backs
    /// [`CqlBatch`](super::CqlBatch) / [`CqlTtl`](super::CqlTtl).
    #[instrument(
        skip(self, models),
        fields(
            table = M::TABLE,
            model = M::TYPE_NAME,
            operation = "insert_batch",
            batch_type = ?batch_type,
            ttl = ?ttl,
            count = models.len()
        )
    )]
    pub(super) async fn exec_insert_batch(
        &self,
        models: &[M],
        batch_type: BatchType,
        ttl: Option<i32>,
    ) -> CqlResult<()>
    where
        M::InsertValues: scylla::serialize::row::SerializeRow + Send + Sync,
    {
        let mut config = BatchConfig::default().batch_type(batch_type);
        if let Some(ttl_seconds) = ttl {
            config = config.ttl(ttl_seconds);
        }
        let mut batch = InsertBatch::with_config(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.keyspace.clone(),
            config,
        );
        batch.add_all(models);
        batch.execute().await
    }

    /// Executes a batched `DELETE` by keys. Backs
    /// [`CqlBatch`](super::CqlBatch) and the abstract delete paths.
    #[instrument(
        skip(self, keys),
        fields(
            table = M::TABLE,
            model = M::TYPE_NAME,
            operation = "delete_batch",
            batch_type = ?batch_type,
            count = keys.len()
        )
    )]
    pub(super) async fn exec_delete_batch(
        &self,
        keys: Vec<M::KeyValues>,
        batch_type: BatchType,
    ) -> CqlResult<()>
    where
        M::KeyValues: scylla::serialize::row::SerializeRow + Send + Sync,
    {
        let config = BatchConfig::default().batch_type(batch_type);
        let mut batch: DeleteBatch<M> = DeleteBatch::with_config(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.keyspace.clone(),
            config,
        );
        batch.add_all(keys);
        batch.execute().await
    }

    /// Builds an [`InsertBatch`] for direct configuration. Backs
    /// [`CqlBatch::insert_batch_builder`](super::CqlBatch::insert_batch_builder).
    pub(super) fn build_insert_batch(&self) -> InsertBatch<M>
    where
        M::InsertValues: scylla::serialize::row::SerializeRow + Send + Sync,
    {
        InsertBatch::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.keyspace.clone(),
        )
    }

    /// Builds a [`DeleteBatch`] for direct configuration. Backs
    /// [`CqlBatch::delete_batch_builder`](super::CqlBatch::delete_batch_builder).
    pub(super) fn build_delete_batch(&self) -> DeleteBatch<M>
    where
        M::KeyValues: scylla::serialize::row::SerializeRow + Send + Sync,
    {
        DeleteBatch::new(
            self.session.clone(),
            Arc::clone(&self.cache),
            self.keyspace.clone(),
        )
    }
}

/// Reads whether a lightweight transaction (LWT) applied, from its `[applied]` column.
///
/// `Ok(false)` means the store is sure the condition failed; a result that says neither is
/// [`CqlError::LwtUnconfirmed`].
///
/// # Errors
///
/// [`CqlError::LwtUnconfirmed`], naming which part of the result could not be read, logged once
/// here with its cause.
pub(crate) fn parse_lwt_result(result: QueryResult) -> CqlResult<bool> {
    // `[applied]` is always the first column. When the condition failed the store appends the
    // conflicting row's columns, so the count varies, and the row is read dynamically for that
    // reason: a fixed `(bool,)` tuple fails on the wider result.
    let rows_result = result
        .into_rows_result()
        .map_err(|cause| unconfirmed("not_rows", &cause))?;
    let mut rows = rows_result
        .rows::<scylla::value::Row>()
        .map_err(|cause| unconfirmed("rows_undecodable", &cause))?;
    match rows.next() {
        Some(Ok(row)) => applied_flag(row.columns.first()).map_err(|reason| {
            unconfirmed(reason, &"the first column is not a boolean `[applied]`")
        }),
        Some(Err(cause)) => Err(unconfirmed("row_unreadable", &cause)),
        None => Err(unconfirmed("no_rows", &"the result held no row")),
    }
}

/// The `[applied]` flag in a conditional write's first column, or why it is not there.
fn applied_flag(
    first_column: Option<&Option<scylla::value::CqlValue>>,
) -> Result<bool, &'static str> {
    match first_column {
        Some(Some(scylla::value::CqlValue::Boolean(applied))) => Ok(*applied),
        Some(Some(_)) => Err("applied_not_boolean"),
        Some(None) | None => Err("applied_missing"),
    }
}

/// The error for a result that did not say whether its write applied, logged with its cause.
fn unconfirmed(reason: &'static str, cause: &dyn std::fmt::Display) -> CqlError {
    warn!(reason, %cause, "a conditional write's result did not say whether it applied");
    CqlError::LwtUnconfirmed { reason }
}

impl<M> std::fmt::Debug for CqlRepository<M> {
    /// The collector is not `Debug`; whether one is attached is what is printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqlRepository")
            .field("keyspace", &self.keyspace)
            .field("read_consistency", &self.read_consistency)
            .field("write_consistency", &self.write_consistency)
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use scylla::value::CqlValue;

    use super::{applied_flag, unconfirmed};
    use crate::CqlError;

    #[test]
    fn a_boolean_first_column_is_the_answer_either_way() {
        assert_eq!(applied_flag(Some(&Some(CqlValue::Boolean(true)))), Ok(true));
        assert_eq!(
            applied_flag(Some(&Some(CqlValue::Boolean(false)))),
            Ok(false)
        );
    }

    #[test]
    fn a_first_column_that_is_not_a_boolean_is_not_an_answer() {
        assert_eq!(
            applied_flag(Some(&Some(CqlValue::Text("applied".to_owned())))),
            Err("applied_not_boolean")
        );
    }

    #[test]
    fn a_missing_or_null_first_column_is_not_an_answer() {
        assert_eq!(applied_flag(Some(&None)), Err("applied_missing"));
        assert_eq!(applied_flag(None), Err("applied_missing"));
    }

    #[test]
    fn an_unreadable_result_is_a_retryable_error_and_never_not_applied() {
        let error = unconfirmed("no_rows", &"the result held no row");
        assert!(matches!(
            error,
            CqlError::LwtUnconfirmed { reason: "no_rows" }
        ));
        assert!(error.is_retryable());
    }
}
