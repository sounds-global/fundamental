//! Fluent, model-centric UPDATE builder for YSQL.

use crate::ident::validate_identifier;
use crate::query::pg_query;
use crate::{FromRow, SqlConn, bind_filter_value, build_update_sql};

use fundamental_core::model::{
    BoundTo, LeadingKeyAddressed, Model, NoLeadingKey, Satisfied, SqlDb, SqlModel,
};
use fundamental_db::{DatabaseResult, Filter, FilterValue, QueryBuilder};
use tracing::field::Empty;
use tracing::{Span, instrument};

type PgQuery<'q> = sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>;

/// A primary-key predicate whose id is bound at its real wire type.
struct PkPredicate {
    column: String,
    bind: Box<dyn for<'q> FnOnce(PgQuery<'q>) -> PgQuery<'q> + Send>,
}

/// A model-centric update builder for YSQL, run on a pool or a transaction.
pub struct SqlUpdateBuilder<'a, M> {
    conn: SqlConn<'a>,
    filters: Vec<Filter>,
    updates: Vec<(String, FilterValue)>,
    pk: Option<PkPredicate>,
    _marker: std::marker::PhantomData<M>,
}

impl<M> std::fmt::Debug for SqlUpdateBuilder<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlUpdateBuilder")
            .field("conn", &self.conn)
            .field("filters", &self.filters)
            .field("updates", &self.updates)
            .field("has_pk", &self.pk.is_some())
            .finish()
    }
}

impl<'a, M> SqlUpdateBuilder<'a, M>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + Send + Sync,
{
    /// Creates a new update builder.
    pub fn new(conn: impl Into<SqlConn<'a>>) -> Self {
        Self {
            conn: conn.into(),
            filters: Vec::new(),
            updates: Vec::new(),
            pk: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates a new update builder targeting a primary key, bound at its native wire type.
    pub fn for_id<Id>(conn: impl Into<SqlConn<'a>>, id: Id) -> Self
    where
        Id: Send + 'static + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        Self {
            conn: conn.into(),
            filters: Vec::new(),
            updates: Vec::new(),
            pk: Some(PkPredicate {
                column: M::PRIMARY_KEY.to_string(),
                bind: Box::new(move |q| q.bind(id)),
            }),
            _marker: std::marker::PhantomData,
        }
    }

    /// Adds a filter to the WHERE clause.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Sets a column to a value.
    pub fn set(mut self, column: impl Into<String>, value: impl Into<FilterValue>) -> Self {
        self.updates.push((column.into(), value.into()));
        self
    }

    /// Sets a column to NULL.
    pub fn set_null(mut self, column: impl Into<String>) -> Self {
        self.updates.push((column.into(), FilterValue::Null));
        self
    }

    /// Sets a column to the value if `Some`, and leaves it untouched (not NULL) if `None`.
    pub fn set_optional(
        self,
        column: impl Into<String>,
        value: Option<impl Into<FilterValue>>,
    ) -> Self {
        match value {
            Some(v) => self.set(column, v),
            None => self,
        }
    }

    /// Executes the update and returns the number of rows affected.
    #[instrument(
        skip(self),
        fields(table = M::TABLE, model = M::TYPE_NAME, op = "update", columns = self.updates.len(), has_pk = self.pk.is_some(), affected = Empty),
        err(Debug)
    )]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(mut self) -> DatabaseResult<u64>
    where
        // Unpinned, so never for a tenancy-led key; the pinned route is `Model::query(conn).pin_key(value)`.
        LeadingKeyAddressed<M, SqlDb, NoLeadingKey>: Satisfied,
    {
        let mut builder: QueryBuilder<M, SqlDb> = QueryBuilder::new();
        for filter in &self.filters {
            builder = builder.filter(filter.clone());
        }

        let (mut sql, values) =
            build_update_sql::<M, _>(self.conn.schema(), &builder, &self.updates)?;

        // Its `$N` follows every parameter already in `values`.
        if let Some(pk) = &self.pk {
            validate_identifier(&pk.column, false)?;
            let next_index = values.len() + 1;
            if self.filters.is_empty() {
                sql.push_str(&format!(" WHERE {} = ${}", pk.column, next_index));
            } else {
                sql.push_str(&format!(" AND {} = ${}", pk.column, next_index));
            }
        }

        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        if let Some(pk) = self.pk.take() {
            sqlx_query = (pk.bind)(sqlx_query);
        }

        let affected = self.conn.execute_raw(sqlx_query).await?;
        Span::current().record("affected", affected);
        Ok(affected)
    }
}
