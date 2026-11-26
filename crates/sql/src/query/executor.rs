//! Executes [`fundamental_db::QueryBuilder`] queries over the SQL wire.

use crate::query::pg_query;
use crate::{FromRow, SqlError, SqlQuery, ToRow};
use crate::{bind_filter_value, build_count_sql, build_select_sql, build_update_sql};
use fundamental_db::SchemaName;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tracing::field::Empty;
use tracing::{Span, instrument};

use crate::SqlConnector;
use fundamental_core::model::{BoundTo, LeadingKeyAddressed, Model, Satisfied, SqlDb, SqlModel};
use fundamental_db::{
    DatabaseError, DatabaseMetrics, DatabaseResult, FilterValue, QueryBuilder, QueryExecutor,
};
use fundamental_encryption::SharedKeyManager;

/// YSQL query executor, decrypting `Secret` columns through an optional key manager.
pub struct SqlQueryExecutor<M> {
    connector: SqlConnector,
    key_manager: Option<SharedKeyManager>,
    metrics: Option<(Arc<dyn DatabaseMetrics>, Duration)>,
    schema: SchemaName,
    _marker: PhantomData<M>,
}

impl<M> SqlQueryExecutor<M> {
    /// Qualifies this executor's statements with `schema`.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaName) -> Self {
        self.schema = schema;
        self
    }

    /// The schema this executor's statements qualify their tables with.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Creates a query executor over a bare pool, answering about `public`; crate-private, as callers must carry their schema.
    pub(crate) fn new(connector: SqlConnector) -> Self {
        Self {
            connector,
            key_manager: None,
            metrics: None,
            schema: SchemaName::default(),
            _marker: PhantomData,
        }
    }

    /// Attaches a key manager, so a filtered read decrypts `Secret` columns.
    #[must_use]
    pub fn with_key_manager(mut self, key_manager: SharedKeyManager) -> Self {
        self.key_manager = Some(key_manager);
        self
    }

    /// Attaches the collector this executor records its query timings to.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn DatabaseMetrics>, slow_after: Duration) -> Self {
        self.metrics = Some((metrics, slow_after));
        self
    }
}

impl<M: SqlModel> SqlQueryExecutor<M> {
    /// Runs `operation` under a timing guard when a collector is attached, as the repository does.
    async fn timed<T, E, F>(&self, operation: &'static str, run: F) -> Result<T, E>
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
}

/// Hand-written because `M` is a marker; `schema` is carried over.
impl<M> Clone for SqlQueryExecutor<M> {
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

impl<M> SqlQueryExecutor<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    /// Executes a [`SqlQuery`]: a builder together with the row lock it carries.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use fundamental_sql::SqlQuery;
    /// # use fundamental_core::model::SqlDb;
    /// # use fundamental_db::QueryBuilder;
    /// # #[derive(Clone, fundamental_macros::Model)]
    /// # #[model(sql, table = "users")]
    /// # struct User {
    /// #     #[model(id)]
    /// #     id: uuid::Uuid,
    /// # }
    /// # async fn example(pool: &fundamental_sql::SqlPool) -> Result<(), Box<dyn std::error::Error>> {
    /// # let executor = pool.query_executor::<User>();
    /// let query = SqlQuery::new(QueryBuilder::<User, SqlDb>::new());
    ///
    /// let users = executor.execute_sql_query(&query).await?;
    /// # let _ = users;
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(
        level = "debug",
        skip(self, query),
        fields(table = M::TABLE, model = M::TYPE_NAME, rows = Empty),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute_sql_query<State>(
        &self,
        query: &SqlQuery<M, State>,
    ) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (sql, values) = build_select_sql::<M, _>(&self.schema, query.inner(), None)?;

        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }

        let rows = sqlx_query
            .fetch_all(&mut *self.connector.acquire().await?)
            .await
            .map_err(SqlError::from)?;
        Span::current().record("rows", rows.len());

        let models = rows
            .iter()
            .map(|row| {
                crate::model::translations::decode::<M>(row, self.key_manager.as_deref())
                    .map_err(|e| DatabaseError::serialization(e.to_string()))
            })
            .collect::<DatabaseResult<Vec<M>>>()?;
        Ok(models)
    }
}

impl<M> QueryExecutor<M, SqlDb> for SqlQueryExecutor<M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    #[instrument(
        level = "debug",
        skip(self, query),
        fields(table = M::TABLE, model = M::TYPE_NAME, rows = Empty),
        err(Debug)
    )]
    async fn execute<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<Vec<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.timed("select", async {
            let (sql, values) = build_select_sql::<M, _>(&self.schema, query, None)?;

            let mut sqlx_query = pg_query(sql);
            for value in values {
                sqlx_query = bind_filter_value(sqlx_query, value);
            }

            let rows = sqlx_query
                .fetch_all(&mut *self.connector.acquire().await?)
                .await
                .map_err(SqlError::from)?;
            Span::current().record("rows", rows.len());

            let models = rows
                .iter()
                .map(|row| {
                    crate::model::translations::decode::<M>(row, self.key_manager.as_deref())
                        .map_err(|e| DatabaseError::serialization(e.to_string()))
                })
                .collect::<DatabaseResult<Vec<M>>>()?;
            Ok(models)
        })
        .await
    }

    #[instrument(
        level = "debug",
        skip(self, query),
        fields(table = M::TABLE, model = M::TYPE_NAME, found = Empty),
        err(Debug)
    )]
    async fn execute_one<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<Option<M>>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.timed("select", async {
            let limited_query = if query.get_pagination().is_none() {
                query.clone().limit(1)
            } else {
                query.clone()
            };

            let (sql, values) = build_select_sql::<M, _>(&self.schema, &limited_query, None)?;

            let mut sqlx_query = pg_query(sql);
            for value in values {
                sqlx_query = bind_filter_value(sqlx_query, value);
            }

            let row = sqlx_query
                .fetch_optional(&mut *self.connector.acquire().await?)
                .await
                .map_err(SqlError::from)?;
            Span::current().record("found", row.is_some());

            match row {
                Some(row) => {
                    let model =
                        crate::model::translations::decode::<M>(&row, self.key_manager.as_deref())
                            .map_err(|e| DatabaseError::serialization(e.to_string()))?;
                    Ok(Some(model))
                }
                None => Ok(None),
            }
        })
        .await
    }

    #[instrument(
        level = "debug",
        skip(self, query),
        fields(table = M::TABLE, model = M::TYPE_NAME, count = Empty),
        err(Debug)
    )]
    async fn count<State>(&self, query: &QueryBuilder<M, SqlDb, State>) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.timed("count", async {
            let (sql, values) = build_count_sql::<M, _>(&self.schema, query)?;

            let mut sqlx_query = pg_query(sql);
            for value in values {
                sqlx_query = bind_filter_value(sqlx_query, value);
            }

            let row = sqlx_query
                .fetch_one(&mut *self.connector.acquire().await?)
                .await
                .map_err(SqlError::from)?;

            let count: i64 = sqlx::Row::get(&row, 0);
            Span::current().record("count", count);
            Ok(count as u64)
        })
        .await
    }

    #[instrument(
        level = "debug",
        skip(self, query),
        fields(table = M::TABLE, model = M::TYPE_NAME, affected = Empty),
        err(Debug)
    )]
    async fn delete_where<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.timed("delete", async {
            let affected = crate::repository_ops::delete_where_with::<_, M, _>(
                &mut *self.connector.acquire().await?,
                &self.schema,
                query,
            )
            .await?;
            Span::current().record("affected", affected);
            Ok(affected)
        })
        .await
    }

    #[instrument(
        level = "debug",
        skip(self, query, updates),
        fields(table = M::TABLE, model = M::TYPE_NAME, columns = updates.len(), affected = Empty),
        err(Debug)
    )]
    async fn update_where<State>(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
        updates: &[(String, FilterValue)],
    ) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        self.timed("update", async {
            let (sql, values) = build_update_sql::<M, _>(&self.schema, query, updates)?;

            let mut sqlx_query = pg_query(sql);
            for value in values {
                sqlx_query = bind_filter_value(sqlx_query, value);
            }

            let result = sqlx_query
                .execute(&mut *self.connector.acquire().await?)
                .await
                .map_err(SqlError::from)?;

            let affected = result.rows_affected();
            Span::current().record("affected", affected);
            Ok(affected)
        })
        .await
    }
}

impl<M> std::fmt::Debug for SqlQueryExecutor<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlQueryExecutor")
            .field("connector", &self.connector)
            .field("has_key_manager", &self.key_manager.is_some())
            .field("has_metrics", &self.metrics.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, fundamental_macros::Model)]
    #[model(sql, table = "rows")]
    struct Row {
        #[model(id)]
        id: uuid::Uuid,
    }

    #[tokio::test]
    async fn a_cloned_query_executor_keeps_the_schema_it_was_built_with() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgresql://localhost:5433/unused")
            .expect("a lazy pool needs no server");
        let tenant = SchemaName::new("tenant_a").expect("a plain identifier");
        let executor =
            SqlQueryExecutor::<Row>::new(SqlConnector::new(pool)).with_schema(tenant.clone());

        assert_eq!(executor.clone().schema(), &tenant);
    }
}
