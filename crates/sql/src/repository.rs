//! Type-safe CRUD repository for YSQL-bound models.

use fundamental_db::SchemaName;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPool;
use tracing::field::Empty;
use tracing::{Span, debug, instrument};

use crate::SqlConnector;
use crate::SqlQueryExecutor;
use crate::ident::validate_identifier;
use crate::metrics::TimingGuard;
use crate::query::pg_query;
use crate::repository_ops as ops;
use crate::{FromRow, SqlError, SqlResult, ToRow};
use fundamental_core::Model;
use fundamental_core::model::{BoundTo, LeadingKeyAddressed, Satisfied, SqlDb, SqlModel};
use fundamental_db::{
    DatabaseError, DatabaseMetrics, DatabaseResult, FilterValue, KeyedRepository,
    KeyedRepositoryBatch, QueryBuilder, QueryExecutor, Repository, RepositoryAggregate,
    RepositoryBatch, RepositoryExt, RepositorySoftDelete, RepositoryTranslation,
};
use fundamental_encryption::SharedKeyManager;

/// YSQL repository for a specific model.
pub struct SqlRepository<M> {
    connector: SqlConnector,
    key_manager: Option<SharedKeyManager>,
    metrics: Option<(Arc<dyn DatabaseMetrics>, Duration)>,
    schema: SchemaName,
    _marker: PhantomData<M>,
}

impl<M> SqlRepository<M> {
    pub(crate) fn new(connector: SqlConnector) -> Self {
        Self {
            connector,
            key_manager: None,
            metrics: None,
            schema: SchemaName::default(),
            _marker: PhantomData,
        }
    }

    /// Attaches a key manager, encrypting the model's `Secret` fields at rest.
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Attaches the collector this repository records its query timings to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>, slow_after: Duration) -> Self {
        self.metrics = Some((metrics, slow_after));
        self
    }

    /// Qualifies this repository's statements with `schema`.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaName) -> Self {
        self.schema = schema;
        self
    }

    /// The schema this repository's statements qualify their tables with.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// A query executor over this repository's pool, carrying its schema, key manager and metrics.
    #[must_use]
    pub fn query_executor(&self) -> SqlQueryExecutor<M> {
        let executor =
            SqlQueryExecutor::new(self.connector.clone()).with_schema(self.schema.clone());
        let executor = match &self.key_manager {
            Some(km) => executor.with_key_manager(km.clone()),
            None => executor,
        };
        match &self.metrics {
            Some((metrics, slow_after)) => executor.with_metrics(Arc::clone(metrics), *slow_after),
            None => executor,
        }
    }

    /// Returns the pool serving now; take it for one operation and never keep it.
    pub fn pool(&self) -> PgPool {
        self.connector.pool()
    }

    /// Returns the attached key manager, if any.
    pub fn key_manager(&self) -> Option<&SharedKeyManager> {
        self.key_manager.as_ref()
    }
}

impl<M: SqlModel> SqlRepository<M> {
    /// Runs `operation` under a timing guard when a collector is attached; a failure marks the guard.
    async fn timed<T, E, F>(&self, operation: &'static str, run: F) -> Result<T, E>
    where
        F: Future<Output = Result<T, E>>,
    {
        let Some((metrics, slow_after)) = self.metrics.as_ref() else {
            return run.await;
        };
        let mut guard = TimingGuard::new(metrics.as_ref(), operation, M::TABLE, *slow_after);
        let result = run.await;
        if result.is_err() {
            guard.mark_failed();
        }
        result
    }
}

/// Hand-written because `M` is a marker; every field is carried over, `schema` included.
impl<M> Clone for SqlRepository<M> {
    fn clone(&self) -> Self {
        Self {
            connector: self.connector.clone(),
            key_manager: self.key_manager.clone(),
            metrics: self.metrics.clone(),
            schema: self.schema.clone(),
            _marker: PhantomData,
        }
    }
}

/// The id-free half, available to every YSQL model including composite-key ones.
impl<M> Repository<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync,
{
    type Id = M::Id;

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn insert(&self, model: &M) -> DatabaseResult<M> {
        self.timed("insert", async {
            Ok(ops::insert_with(
                &mut *self.connector.acquire().await?,
                &self.schema,
                self.key_manager.as_deref(),
                model,
            )
            .await?)
        })
        .await
    }

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn update(&self, model: &M) -> DatabaseResult<M> {
        self.timed("update", async {
            Ok(ops::update_with(
                &mut *self.connector.acquire().await?,
                &self.schema,
                self.key_manager.as_deref(),
                model,
            )
            .await?)
        })
        .await
    }

    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn upsert(&self, model: &M) -> DatabaseResult<M> {
        self.timed("upsert", async {
            Ok(ops::upsert_with(
                &mut *self.connector.acquire().await?,
                &self.schema,
                self.key_manager.as_deref(),
                model,
            )
            .await?)
        })
        .await
    }

    fn query(&self) -> QueryBuilder<M, SqlDb> {
        QueryBuilder::new()
    }
}

/// The id-taking half; a composite-key model's `Id` has no `sqlx::Encode`, so this impl does not apply to it.
impl<M> KeyedRepository<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        self.timed("select", async {
            let found = ops::find_with::<_, M>(
                &mut *self.connector.acquire().await?,
                &self.schema,
                self.key_manager.as_deref(),
                id,
            )
            .await?;
            Ok(found)
        })
        .await
    }

    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME))]
    async fn find_required(&self, id: &Self::Id) -> DatabaseResult<M> {
        self.find(id)
            .await?
            .ok_or_else(|| DatabaseError::not_found(M::TYPE_NAME, "<id>".to_string()))
    }

    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn exists(&self, id: &Self::Id) -> DatabaseResult<bool> {
        self.timed("exists", async {
            Ok(
                ops::exists_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, id)
                    .await?,
            )
        })
        .await
    }

    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        self.timed("delete", async {
            Ok(
                ops::delete_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, id)
                    .await?,
            )
        })
        .await
    }
}

// No `M::Id` bound: no method here takes an id, and composite-key models need `insert_or_ignore`.
impl<M> SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()))]
    async fn insert_chunk(&self, models: &[M]) -> SqlResult<Vec<M>> {
        ops::insert_chunk_with(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            models,
        )
        .await
    }

    /// Finds all records with pagination, excluding soft-deleted rows.
    #[instrument(
        level = "debug",
        skip(self),
        fields(table = M::TABLE, model = M::TYPE_NAME, limit, offset, rows = Empty),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn find_all(&self, limit: u64, offset: u64) -> SqlResult<Vec<M>> {
        let where_clause = if M::SOFT_DELETE {
            format!(" WHERE {} IS NULL", M::DELETED_AT_COLUMN)
        } else {
            String::new()
        };
        let sql = format!(
            "{}{} LIMIT {} OFFSET {}",
            crate::model::translations::select_from::<M>(&self.schema),
            where_clause,
            limit,
            offset
        );

        let rows = pg_query(sql)
            .fetch_all(&mut *self.connector.acquire().await?)
            .await?;
        Span::current().record("rows", rows.len());

        let enc = self.key_manager.as_deref();
        rows.iter()
            .map(|row| crate::model::translations::decode::<M>(row, enc))
            .collect()
    }

    /// Executes a raw, developer-authored SQL query returning models.
    ///
    /// # Safety
    ///
    /// `sql` is sent verbatim, so it must never contain interpolated user input.
    #[instrument(
        level = "debug",
        skip(self, sql),
        fields(table = M::TABLE, model = M::TYPE_NAME, rows = Empty),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn raw_query(&self, sql: &str) -> SqlResult<Vec<M>> {
        let sql = if M::translation_table().is_some() {
            format!(
                "SELECT {} FROM ({sql}) AS raw",
                crate::model::translations::select_list::<M>(&self.schema, "raw.*", "raw")
            )
        } else {
            sql.to_owned()
        };
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *self.connector.acquire().await?)
            .await?;
        Span::current().record("rows", rows.len());

        let enc = self.key_manager.as_deref();
        rows.iter()
            .map(|row| crate::model::translations::decode::<M>(row, enc))
            .collect()
    }

    /// Finds a single model by a string field; `field` is validated and `value` bound.
    #[instrument(
        level = "debug",
        skip(self, value),
        fields(table = M::TABLE, model = M::TYPE_NAME, field),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn find_by_field(&self, field: &str, value: &str) -> SqlResult<Option<M>> {
        validate_identifier(field, false)?;
        let sql = format!(
            "{} WHERE {} = $1 LIMIT 1",
            crate::model::translations::select_from::<M>(&self.schema),
            field
        );
        let row = pg_query(sql)
            .bind(value)
            .fetch_optional(&mut *self.connector.acquire().await?)
            .await?;
        row.map(|row| crate::model::translations::decode::<M>(&row, self.key_manager.as_deref()))
            .transpose()
    }

    /// Inserts a model, ignoring the write if it conflicts on `conflict_target`, the columns of
    /// a `UNIQUE` or primary-key constraint; `None` when a row already existed.
    ///
    /// # Errors
    /// [`SqlError::Query`] if `conflict_target` is empty or holds an invalid
    /// identifier, or the underlying database error on a write failure.
    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME, conflict_target = conflict_target.join(",")), err(Debug))]
    pub async fn insert_or_ignore(
        &self,
        model: &M,
        conflict_target: &[&str],
    ) -> SqlResult<Option<M>> {
        ops::insert_or_ignore_with(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            model,
            conflict_target,
        )
        .await
    }

    /// Upserts a model, overwriting an existing row only when `guard` holds, in one statement.
    ///
    /// # Errors
    /// [`SqlError::Query`] if an identifier is invalid, or the underlying
    /// database error on a write failure.
    #[instrument(skip(self, model, guard), fields(table = M::TABLE, model = M::TYPE_NAME, conflict_target = conflict_target.join(",")), err(Debug))]
    pub async fn upsert_guarded(
        &self,
        model: &M,
        conflict_target: &[&str],
        guard: &crate::ColumnGuard,
    ) -> SqlResult<Option<M>> {
        ops::upsert_guarded_with(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            model,
            conflict_target,
            guard,
        )
        .await
    }

    /// Idempotently appends a row keyed by `unique_column`, assigning its primary key from a
    /// gap-free [`SequenceRef`] consumed only when a row is inserted.
    ///
    /// # Errors
    /// [`SqlError::Query`] if an identifier is invalid or `unique_column` is
    /// not a non-primary-key column of the model, or the underlying database error
    /// on a write failure.
    ///
    /// [`SequenceRef`]: crate::SequenceRef
    #[instrument(skip(self, model, sequence), fields(table = M::TABLE, model = M::TYPE_NAME, unique_column), err(Debug))]
    pub async fn insert_or_get_sequenced(
        &self,
        model: &M,
        unique_column: &str,
        sequence: &crate::SequenceRef<'_>,
    ) -> SqlResult<(M, crate::WriteOutcome)> {
        ops::insert_or_get_sequenced_with(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            model,
            unique_column,
            sequence,
        )
        .await
    }

    /// Inserts a single model without importing the trait.
    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn insert_one(&self, model: &M) -> SqlResult<M>
    where
        M: Clone,
    {
        let results = self.insert_chunk(std::slice::from_ref(model)).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| SqlError::query("Insert returned no result"))
    }

    /// Upserts multiple models using INSERT ... ON CONFLICT DO UPDATE, in chunks.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[derive(Clone, fundamental_macros::Model)]
    /// # #[model(sql, table = "users")]
    /// # struct User {
    /// #     #[model(id)]
    /// #     id: uuid::Uuid,
    /// #     name: String,
    /// # }
    /// # async fn example(
    /// #     pool: &fundamental_sql::SqlPool,
    /// #     new_user: User,
    /// #     existing_user: User,
    /// # ) -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = pool.repository::<User>();
    /// let users = vec![new_user, existing_user];
    /// let results = repo.upsert_many(&users).await?;
    /// # let _ = results;
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn upsert_many(&self, models: &[M]) -> SqlResult<Vec<M>> {
        if models.is_empty() {
            debug!("upsert_many called with no models; nothing to do");
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: usize = 1000;
        let mut all_results = Vec::with_capacity(models.len());

        for chunk in models.chunks(CHUNK_SIZE) {
            let results = self.upsert_chunk(chunk).await?;
            all_results.extend(results);
        }

        Ok(all_results)
    }

    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()))]
    async fn upsert_chunk(&self, models: &[M]) -> SqlResult<Vec<M>> {
        crate::model::translations::whole_rows_only::<M>("upsert_many")?;
        if models.is_empty() {
            debug!("upsert_chunk called with no models; nothing to do");
            return Ok(Vec::new());
        }

        let columns = M::columns();
        // The whole key: conflicting on its first column would overwrite rows it does not address.
        let key = M::KEY_COLUMNS.join(", ");
        let table = self.schema.qualify(M::TABLE);
        let num_cols = columns.len();

        let mut sql = format!("INSERT INTO {} ({}) VALUES ", table, columns.join(", "));

        let mut value_groups = Vec::with_capacity(models.len());
        let mut param_idx = 1;

        for _ in models {
            let placeholders: Vec<String> = (0..num_cols)
                .map(|_| {
                    let p = format!("${}", param_idx);
                    param_idx += 1;
                    p
                })
                .collect();
            value_groups.push(format!("({})", placeholders.join(", ")));
        }

        sql.push_str(&value_groups.join(", "));

        let update_columns: Vec<String> = M::columns_without_pk()
            .iter()
            .map(|col| format!("{} = EXCLUDED.{}", col, col))
            .collect();

        sql.push_str(&format!(
            " ON CONFLICT ({}) DO UPDATE SET {}",
            key,
            update_columns.join(", ")
        ));
        sql.push_str(" RETURNING *");

        let enc = self.key_manager.as_deref();
        let mut query = pg_query(sql);
        for model in models {
            query = model.bind_to_encrypted(query, enc)?;
        }

        let rows = query
            .fetch_all(&mut *self.connector.acquire().await?)
            .await?;

        rows.iter()
            .map(|row| M::from_row_encrypted(row, enc))
            .collect()
    }
}

/// The composite-key point read, on the repository where the key manager lives.
impl<M> SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M: fundamental_core::model::CompositeKeyModel,
{
    /// Fetches a single row by its whole composite primary key, decrypting through this repository's key manager.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`] if the statement fails, the row cannot be deserialized, or an
    /// encrypted column cannot be decrypted with the configured key manager.
    #[instrument(level = "debug", skip(self, key), fields(table = M::TABLE, model = M::TYPE_NAME, op = "find_by_key"), err(Debug))]
    pub async fn find_by_key<'k>(&self, key: &'k M::KeyValues) -> DatabaseResult<Option<M>>
    where
        M::KeyValues: crate::BindKey<'k>,
    {
        let found = ops::find_by_key_with::<_, M>(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            key,
        )
        .await?;
        Ok(found)
    }

    /// Deletes a single row by its whole composite primary key, returning whether one was removed.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`] if the statement fails.
    #[instrument(level = "debug", skip(self, key), fields(table = M::TABLE, model = M::TYPE_NAME, op = "delete_by_key"), err(Debug))]
    pub async fn delete_by_key<'k>(&self, key: &'k M::KeyValues) -> DatabaseResult<bool>
    where
        M::KeyValues: crate::BindKey<'k>,
    {
        Ok(ops::delete_by_key_with::<_, M>(
            &mut *self.connector.acquire().await?,
            &self.schema,
            key,
        )
        .await?)
    }
}

// No `M::Id` bound: each row is addressed by its own whole key.
impl<M> RepositoryBatch<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync,
{
    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn insert_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        if models.is_empty() {
            debug!("insert_many called with no models; nothing to do");
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: usize = 1000;
        let mut all_results = Vec::with_capacity(models.len());
        for chunk in models.chunks(CHUNK_SIZE) {
            all_results.extend(self.insert_chunk(chunk).await?);
        }
        Ok(all_results)
    }

    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()), err(Debug))]
    async fn update_many(&self, models: &[M]) -> DatabaseResult<Vec<M>> {
        crate::model::translations::whole_rows_only::<M>("update_many")?;
        if models.is_empty() {
            debug!("update_many called with no models; nothing to do");
            return Ok(Vec::new());
        }

        const CHUNK_SIZE: usize = 500;
        let mut all_results = Vec::with_capacity(models.len());
        for chunk in models.chunks(CHUNK_SIZE) {
            // Not the inherent `update_chunk`, whose impl block needs an encodable id.
            all_results.extend(
                ops::update_chunk_with(
                    &mut *self.connector.acquire().await?,
                    &self.schema,
                    self.key_manager.as_deref(),
                    chunk,
                )
                .await?,
            );
        }
        Ok(all_results)
    }
}

impl<M> KeyedRepositoryBatch<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    #[instrument(level = "debug", skip(self, ids), fields(table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn find_many(&self, ids: &[Self::Id]) -> DatabaseResult<Vec<M>> {
        let rows = ops::find_many_with::<_, M>(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            ids,
        )
        .await?;
        Ok(rows)
    }

    #[instrument(skip(self, ids), fields(table = M::TABLE, model = M::TYPE_NAME, count = ids.len()), err(Debug))]
    async fn delete_many(&self, ids: &[Self::Id]) -> DatabaseResult<u64> {
        Ok(
            ops::delete_many_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, ids)
                .await?,
        )
    }
}

impl<M> RepositoryAggregate<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    // `Send + Sync` only: neither method takes an id.
    M::Id: Send + Sync,
{
    #[instrument(level = "debug", skip(self), fields(table = M::TABLE, model = M::TYPE_NAME, count = Empty), err(Debug))]
    async fn count(&self) -> DatabaseResult<u64> {
        let count =
            ops::count_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema).await?;
        Span::current().record("count", count);
        Ok(count)
    }

    #[instrument(level = "debug", skip(self, query), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn count_where<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        Ok(
            ops::count_where_with(&mut *self.connector.acquire().await?, &self.schema, query)
                .await?,
        )
    }
}

/// A repository answers the queries it builds, as its [`query_executor`](SqlRepository::query_executor) does.
impl<M> QueryExecutor<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    async fn execute<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.query_executor().execute(query).await
    }

    async fn execute_one<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Option<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.query_executor().execute_one(query).await
    }

    async fn count<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        QueryExecutor::count(&self.query_executor(), query).await
    }

    async fn delete_where<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.query_executor().delete_where(query).await
    }

    async fn update_where<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
        updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.query_executor().update_where(query, updates).await
    }
}

impl<M> RepositoryExt<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    #[instrument(level = "debug", skip(self, id, create), fields(table = M::TABLE, model = M::TYPE_NAME))]
    async fn find_or_create<F>(&self, id: &Self::Id, create: F) -> DatabaseResult<M>
    where
        F: FnOnce() -> M + Send,
    {
        if let Some(found) = self.find(id).await? {
            return Ok(found);
        }
        self.insert(&create()).await
    }

    #[instrument(level = "debug", skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME))]
    async fn update_if_exists(&self, model: &M) -> DatabaseResult<Option<M>> {
        if self.exists(model.id()).await? {
            Ok(Some(self.update(model).await?))
        } else {
            Ok(None)
        }
    }
}

impl<M> RepositorySoftDelete<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn soft_delete(&self, id: &Self::Id) -> DatabaseResult<bool> {
        Ok(
            ops::soft_delete_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, id)
                .await?,
        )
    }

    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn restore(&self, id: &Self::Id) -> DatabaseResult<bool> {
        Ok(
            ops::restore_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, id)
                .await?,
        )
    }

    #[instrument(level = "debug", skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn find_with_deleted(&self, id: &Self::Id) -> DatabaseResult<Option<M>> {
        let found = ops::find_with_deleted_with::<_, M>(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            id,
        )
        .await?;
        Ok(found)
    }

    #[instrument(skip(self, id), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    async fn purge(&self, id: &Self::Id) -> DatabaseResult<bool> {
        Ok(
            ops::delete_with::<_, M>(&mut *self.connector.acquire().await?, &self.schema, id)
                .await?,
        )
    }
}

impl<M> RepositoryTranslation<M, SqlDb> for SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send
        + Sync
        + Unpin
        + for<'q> sqlx::Encode<'q, sqlx::Postgres>
        + for<'r> sqlx::Decode<'r, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>,
{
    type TranslationId = M::Id;

    #[instrument(
        level = "debug",
        skip(self, value),
        fields(table = M::TABLE, model = M::TYPE_NAME, field, locale),
        err(Debug)
    )]
    async fn find_ids_by_translation(
        &self,
        field: &str,
        locale: &str,
        value: &str,
    ) -> DatabaseResult<Vec<Self::TranslationId>> {
        if M::translation_table().is_none() {
            return Ok(Vec::new());
        }
        let pool = self.connector.pool();
        let conn = crate::SqlConn::RawPool(&pool, &self.schema);
        crate::find_translation_ids::<M>(conn, field, locale, value).await
    }
}

impl<M> std::fmt::Debug for SqlRepository<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlRepository")
            .field("connector", &self.connector)
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .finish()
    }
}

/// The batch update binds `model.id()` directly, so the encodable-id bound keeps composite-key models out.
impl<M> SqlRepository<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
    M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    /// Updates a single model without importing the trait.
    #[instrument(skip(self, model), fields(table = M::TABLE, model = M::TYPE_NAME), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn update_one(&self, model: &M) -> SqlResult<M>
    where
        M: Clone,
    {
        let results = self.update_chunk(std::slice::from_ref(model)).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| SqlError::query("Update returned no result"))
    }

    #[instrument(skip(self, models), fields(table = M::TABLE, model = M::TYPE_NAME, count = models.len()))]
    async fn update_chunk(&self, models: &[M]) -> SqlResult<Vec<M>> {
        crate::model::translations::whole_rows_only::<M>("update_one")?;
        ops::update_chunk_with(
            &mut *self.connector.acquire().await?,
            &self.schema,
            self.key_manager.as_deref(),
            models,
        )
        .await
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recording {
        queries: Mutex<Vec<(String, String, bool)>>,
    }

    impl DatabaseMetrics for Recording {
        fn record_query(&self, operation: &str, table: &str, _duration: Duration, success: bool) {
            if let Ok(mut queries) = self.queries.lock() {
                queries.push((operation.to_string(), table.to_string(), success));
            }
        }
        fn record_connection_acquire(&self, _duration: Duration, _success: bool) {}
        fn record_slow_query(&self, _operation: &str, _table: &str, _duration: Duration) {}
        fn record_transaction(&self, _committed: bool, _duration: Duration) {}
    }

    #[derive(Clone, fundamental_macros::Model)]
    #[model(sql, table = "rows")]
    struct Row {
        #[model(id)]
        id: uuid::Uuid,
    }

    fn repository(metrics: Arc<Recording>) -> SqlRepository<Row> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost:5433/unused")
            .expect("a lazy pool needs no server");
        SqlRepository::<Row>::new(SqlConnector::new(pool))
            .with_metrics(metrics, Duration::from_secs(1))
    }

    #[tokio::test]
    async fn a_successful_operation_is_recorded_against_its_table() {
        let metrics = Arc::new(Recording::default());
        let repo = repository(Arc::clone(&metrics));

        repo.timed("insert", async { Ok::<(), DatabaseError>(()) })
            .await
            .expect("the operation succeeds");

        let queries = metrics.queries.lock().expect("not poisoned");
        assert_eq!(
            queries.as_slice(),
            [("insert".to_string(), "rows".to_string(), true)]
        );
    }

    #[tokio::test]
    async fn a_failed_operation_is_recorded_as_failed() {
        let metrics = Arc::new(Recording::default());
        let repo = repository(Arc::clone(&metrics));

        let _ = repo
            .timed("insert", async {
                Err::<(), _>(DatabaseError::query("boom".to_string()))
            })
            .await;

        let queries = metrics.queries.lock().expect("not poisoned");
        assert_eq!(queries.first().map(|q| q.2), Some(false));
    }

    #[tokio::test]
    async fn a_cloned_repository_keeps_the_schema_it_was_built_with() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost:5433/unused")
            .expect("a lazy pool needs no server");
        let tenant = SchemaName::new("tenant_a").expect("a plain identifier");
        let repo = SqlRepository::<Row>::new(SqlConnector::new(pool)).with_schema(tenant.clone());

        assert_eq!(repo.clone().schema(), &tenant);
    }

    #[tokio::test]
    async fn a_repositorys_query_executor_carries_its_schema() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost:5433/unused")
            .expect("a lazy pool needs no server");
        let tenant = SchemaName::new("tenant_a").expect("a plain identifier");
        let repo = SqlRepository::<Row>::new(SqlConnector::new(pool)).with_schema(tenant.clone());

        assert_eq!(repo.query_executor().schema(), &tenant);
    }

    #[tokio::test]
    async fn no_collector_means_no_recording_and_no_cost() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost:5433/unused")
            .expect("a lazy pool needs no server");
        let repo = SqlRepository::<Row>::new(SqlConnector::new(pool));

        repo.timed("insert", async { Ok::<(), DatabaseError>(()) })
            .await
            .expect("the operation succeeds without a collector");
    }
}
