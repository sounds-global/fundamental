//! Unified connection type for YSQL.

use sqlx::postgres::{PgArguments, PgRow};
use tracing::field::Empty;
use tracing::{Span, instrument};

use crate::{SqlError, SqlPool, SqlTransaction};
use fundamental_db::SchemaName;

/// Dispatches to a pool or a transaction, so model methods work with both.
///
/// # Example
///
/// ```rust,no_run
/// # use fundamental_db::{IsolationLevel, Transaction, Transactional};
/// # #[derive(Clone, fundamental_macros::Model)]
/// # #[model(sql, table = "users")]
/// # struct User {
/// #     #[model(id)]
/// #     id: uuid::Uuid,
/// #     name: String,
/// # }
/// # async fn example(
/// #     pool: &fundamental_sql::SqlPool,
/// #     id: uuid::Uuid,
/// #     model: User,
/// #     other_model: User,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// // Pool usage — `User::find` accepts `&SqlPool` directly:
/// let user = User::find(pool, &id).await?;
///
/// // Transaction usage (same methods!):
/// let mut tx = pool.begin(IsolationLevel::Serializable).await?;
/// model.insert(&mut tx).await?;
/// other_model.insert(&mut tx).await?;
/// tx.commit().await?;
/// # let _ = user;
/// # Ok(())
/// # }
/// ```
pub enum SqlConn<'a> {
    /// A pool reference for standalone operations.
    Pool(&'a SqlPool),
    /// A raw sqlx pool together with the schema its owner was configured with.
    RawPool(&'a sqlx::postgres::PgPool, &'a SchemaName),
    /// A mutable transaction reference for transactional operations.
    Tx(&'a mut SqlTransaction),
}

impl SqlConn<'_> {
    /// The schema statements built for this connection qualify their tables with.
    #[must_use]
    pub fn schema(&self) -> &SchemaName {
        match self {
            Self::Pool(pool) => pool.schema(),
            Self::Tx(tx) => tx.schema(),
            Self::RawPool(_, schema) => schema,
        }
    }
}

impl std::fmt::Debug for SqlConn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlConn::Pool(_) => f.debug_tuple("SqlConn::Pool").finish(),
            SqlConn::RawPool(..) => f.debug_tuple("SqlConn::RawPool").finish(),
            SqlConn::Tx(_) => f.debug_tuple("SqlConn::Tx").finish(),
        }
    }
}

impl<'a> From<&'a SqlPool> for SqlConn<'a> {
    fn from(pool: &'a SqlPool) -> Self {
        SqlConn::Pool(pool)
    }
}

impl<'a> From<&'a mut SqlTransaction> for SqlConn<'a> {
    fn from(tx: &'a mut SqlTransaction) -> Self {
        SqlConn::Tx(tx)
    }
}

impl<'a> SqlConn<'a> {
    /// The dispatch variant, recorded as a structured field.
    pub(crate) fn variant_name(&self) -> &'static str {
        match self {
            SqlConn::Pool(_) => "pool",
            SqlConn::RawPool(..) => "raw_pool",
            SqlConn::Tx(_) => "tx",
        }
    }

    /// Executes an INSERT/UPDATE/DELETE query, returning rows affected; for generated model code.
    #[doc(hidden)]
    #[instrument(level = "debug", skip(self, query), fields(op = "execute_raw", conn = self.variant_name(), rows_affected = Empty), err(Debug))]
    pub async fn execute_raw(
        &mut self,
        query: sqlx::query::Query<'_, sqlx::Postgres, PgArguments>,
    ) -> Result<u64, SqlError> {
        let affected = match self {
            SqlConn::Pool(pool) => query.execute(&pool.inner()).await?.rows_affected(),
            SqlConn::RawPool(pool, _) => query.execute(*pool).await?.rows_affected(),
            SqlConn::Tx(tx) => {
                let inner = tx
                    .inner_mut()
                    .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;
                let out = query.execute(&mut **inner).await.map_err(SqlError::from);
                tx.record(out)?.rows_affected()
            }
        };
        Span::current().record("rows_affected", affected);
        Ok(affected)
    }

    /// Executes a SELECT query, returns all matching rows.
    #[instrument(level = "debug", skip(self, query), fields(op = "fetch_all_raw", conn = self.variant_name(), rows = Empty), err(Debug))]
    pub(crate) async fn fetch_all_raw(
        &mut self,
        query: sqlx::query::Query<'_, sqlx::Postgres, PgArguments>,
    ) -> Result<Vec<PgRow>, SqlError> {
        let rows = match self {
            SqlConn::Pool(pool) => query.fetch_all(&pool.inner()).await?,
            SqlConn::RawPool(pool, _) => query.fetch_all(*pool).await?,
            SqlConn::Tx(tx) => {
                let inner = tx
                    .inner_mut()
                    .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;
                let out = query.fetch_all(&mut **inner).await.map_err(SqlError::from);
                tx.record(out)?
            }
        };
        Span::current().record("rows", rows.len());
        Ok(rows)
    }

    /// Executes a SELECT query, returns an optional single row.
    #[instrument(level = "debug", skip(self, query), fields(op = "fetch_optional_raw", conn = self.variant_name(), found = Empty), err(Debug))]
    pub(crate) async fn fetch_optional_raw(
        &mut self,
        query: sqlx::query::Query<'_, sqlx::Postgres, PgArguments>,
    ) -> Result<Option<PgRow>, SqlError> {
        let row = match self {
            SqlConn::Pool(pool) => query.fetch_optional(&pool.inner()).await?,
            SqlConn::RawPool(pool, _) => query.fetch_optional(*pool).await?,
            SqlConn::Tx(tx) => {
                let inner = tx
                    .inner_mut()
                    .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;
                let out = query
                    .fetch_optional(&mut **inner)
                    .await
                    .map_err(SqlError::from);
                tx.record(out)?
            }
        };
        Span::current().record("found", row.is_some());
        Ok(row)
    }

    /// Executes a SELECT query, returns exactly one row.
    #[instrument(level = "debug", skip(self, query), fields(op = "fetch_one_raw", conn = self.variant_name()), err(Debug))]
    pub(crate) async fn fetch_one_raw(
        &mut self,
        query: sqlx::query::Query<'_, sqlx::Postgres, PgArguments>,
    ) -> Result<PgRow, SqlError> {
        match self {
            SqlConn::Pool(pool) => Ok(query.fetch_one(&pool.inner()).await?),
            SqlConn::RawPool(pool, _) => Ok(query.fetch_one(*pool).await?),
            SqlConn::Tx(tx) => {
                let inner = tx
                    .inner_mut()
                    .ok_or_else(|| SqlError::Query("Transaction already completed".into()))?;
                let out = query.fetch_one(&mut **inner).await.map_err(SqlError::from);
                tx.record(out)
            }
        }
    }
}
