//! Fluent, model-centric query builder for YSQL reads and deletes.

use super::cursor_boundary::cursor_after;
use super::key_predicate::key_predicate;
use super::{SqlCursor, SqlPage};
use crate::query::{BindKey, pg_query};
use crate::{
    FromRow, SqlConn, ToRow, bind_filter_value, build_count_sql, build_delete_sql, build_select_sql,
};

use fundamental_core::i18n::Locale;
use fundamental_core::model::{
    BoundTo, CompositeKeyModel, HasLeadingKey, LeadingKeyAddressed, Model, NoLeadingKey, Satisfied,
    SqlDb, SqlModel,
};
use fundamental_db::{DatabaseError, DatabaseResult, Filter, OrderBy, QueryBuilder};
use tracing::field::Empty;
use tracing::{Span, instrument};

/// A fluent query builder for YSQL models, run on a pool or a transaction.
pub struct SqlModelQuery<'a, M, State = NoLeadingKey>
where
    M: BoundTo<SqlDb>,
{
    conn: SqlConn<'a>,
    builder: QueryBuilder<M, SqlDb, State>,
    page_size: Option<u64>,
    language: Option<Locale>,
}

impl<M: BoundTo<SqlDb>, State> std::fmt::Debug for SqlModelQuery<'_, M, State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlModelQuery")
            .field("conn", &self.conn)
            .field("page_size", &self.page_size)
            .finish()
    }
}

impl<'a, M> SqlModelQuery<'a, M, NoLeadingKey>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + Send + Sync,
{
    /// Finds a row by primary key, binding the id at its native wire type.
    #[instrument(skip(conn, id), fields(table = M::TABLE, model = M::TYPE_NAME, op = "find_by_id"), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn find_by_id<Id>(conn: impl Into<SqlConn<'a>>, id: &Id) -> DatabaseResult<Option<M>>
    where
        Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
        M: ToRow,
    {
        let mut conn = conn.into();
        let sql = format!(
            "{} WHERE {} = $1 LIMIT 1",
            super::translations::select_from::<M>(conn.schema()),
            M::PRIMARY_KEY
        );
        let query = pg_query(sql).bind(id);
        let row = conn.fetch_optional_raw(query).await?;
        match row {
            Some(row) => {
                let model = super::translations::decode::<M>(&row, None)
                    .map_err(|e| DatabaseError::serialization(e.to_string()))?;
                Ok(Some(model))
            }
            None => Ok(None),
        }
    }

    /// Finds a row by its whole composite key, one native-typed parameter per column; half a key is a type error.
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`] if the statement fails or the row cannot be deserialized.
    #[instrument(skip(conn, key), fields(table = M::TABLE, model = M::TYPE_NAME, op = "find_by_key"), err(Debug))]
    pub async fn find_by_key(
        conn: impl Into<SqlConn<'a>>,
        key: &'a M::KeyValues,
    ) -> DatabaseResult<Option<M>>
    where
        M: CompositeKeyModel + ToRow,
        M::KeyValues: BindKey<'a>,
    {
        let mut conn = conn.into();
        let sql = format!(
            "{} WHERE {}{} LIMIT 1",
            super::translations::select_from::<M>(conn.schema()),
            key_predicate::<M>()?,
            crate::repository_ops::soft_delete_and::<M>(),
        );
        let query = key.bind_all(pg_query(sql));
        let row = conn.fetch_optional_raw(query).await?;
        match row {
            Some(row) => {
                let model = super::translations::decode::<M>(&row, None)
                    .map_err(|e| DatabaseError::serialization(e.to_string()))?;
                Ok(Some(model))
            }
            None => Ok(None),
        }
    }

    /// Deletes a row by its whole composite key, returning the number of rows deleted (0 or 1).
    ///
    /// # Errors
    ///
    /// Returns a [`DatabaseError`] if the statement fails.
    #[instrument(skip(conn, key), fields(table = M::TABLE, model = M::TYPE_NAME, op = "delete_by_key"), err(Debug))]
    pub async fn delete_by_key(
        conn: impl Into<SqlConn<'a>>,
        key: &'a M::KeyValues,
    ) -> DatabaseResult<u64>
    where
        M: CompositeKeyModel,
        M::KeyValues: BindKey<'a>,
    {
        let mut conn = conn.into();
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            conn.schema().qualify(M::TABLE),
            key_predicate::<M>()?
        );
        match super::translations::with_companion_delete::<M>(conn.schema(), &sql) {
            Some(sql) => {
                let row = conn.fetch_one_raw(key.bind_all(pg_query(sql))).await?;
                let deleted: i64 = sqlx::Row::get(&row, 0);
                Ok(deleted.unsigned_abs())
            }
            None => Ok(conn.execute_raw(key.bind_all(pg_query(sql))).await?),
        }
    }

    /// Deletes a row by primary key, returning the number deleted (0 or 1); the `M::Id` bound keeps composite-key models off it.
    #[instrument(skip(conn, id), fields(table = M::TABLE, model = M::TYPE_NAME, op = "delete_by_id"), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn delete_by_id<Id>(conn: impl Into<SqlConn<'a>>, id: &Id) -> DatabaseResult<u64>
    where
        Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        let mut conn = conn.into();
        let sql = format!(
            "DELETE FROM {} WHERE {} = $1",
            conn.schema().qualify(M::TABLE),
            M::PRIMARY_KEY
        );
        match super::translations::with_companion_delete::<M>(conn.schema(), &sql) {
            Some(sql) => {
                let row = conn.fetch_one_raw(pg_query(sql).bind(id)).await?;
                let deleted: i64 = sqlx::Row::get(&row, 0);
                Ok(deleted.unsigned_abs())
            }
            None => Ok(conn.execute_raw(pg_query(sql).bind(id)).await?),
        }
    }

    /// Creates a new model query for a `&SqlPool` or a `&mut SqlTransaction`.
    pub fn new(conn: impl Into<SqlConn<'a>>) -> Self {
        Self {
            conn: conn.into(),
            builder: QueryBuilder::new(),
            page_size: None,
            language: None,
        }
    }

    /// Pins the query's leading key column to one value, so it can be executed.
    #[must_use]
    pub fn pin_key(
        self,
        value: impl Into<fundamental_db::FilterValue>,
    ) -> SqlModelQuery<'a, M, HasLeadingKey> {
        SqlModelQuery {
            conn: self.conn,
            builder: self.builder.pin_key(value),
            page_size: self.page_size,
            language: self.language,
        }
    }

    /// Pins the leading column of a secondary index `M` declares, so the query executes through it.
    #[must_use]
    pub fn pin_index(
        self,
        index: fundamental_core::model::IndexKey<M>,
        value: impl Into<fundamental_db::FilterValue>,
    ) -> SqlModelQuery<'a, M, HasLeadingKey> {
        SqlModelQuery {
            conn: self.conn,
            builder: self.builder.pin_index(index, value),
            page_size: self.page_size,
            language: self.language,
        }
    }
}

impl<'a, M, State> SqlModelQuery<'a, M, State>
where
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + Send + Sync,
{
    /// Adds a filter to the query.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.builder = self.builder.filter(filter);
        self
    }

    /// Adds multiple filters to the query.
    pub fn filters(mut self, filters: impl IntoIterator<Item = Filter>) -> Self {
        self.builder = self.builder.filters(filters);
        self
    }

    /// Adds an ORDER BY clause.
    pub fn order_by(mut self, order: OrderBy) -> Self {
        self.builder = self.builder.order_by(order);
        self
    }

    /// Sets the maximum number of results to return.
    pub fn limit(mut self, limit: u64) -> Self {
        self.builder = self.builder.limit(limit);
        self
    }

    /// Sets the offset for results.
    pub fn offset(mut self, offset: u64) -> Self {
        self.builder = self.builder.offset(offset);
        self
    }

    /// Selects specific columns instead of all columns.
    pub fn select(mut self, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.builder = self.builder.select(columns);
        self
    }

    /// Sets the page size for paginated queries.
    pub fn page_size(mut self, size: u64) -> Self {
        self.page_size = Some(size);
        self
    }

    /// Reads the model's `Localized` fields in `language`, falling back as
    /// [`Localized::resolve`](fundamental_core::i18n::Localized::resolve) does.
    pub fn in_language(mut self, language: &Locale) -> Self {
        self.language = Some(language.clone());
        self
    }

    /// The page boundary `row` makes in `order_by`, with a localized field's text in that language.
    fn boundary(
        &self,
        row: &sqlx::postgres::PgRow,
        order_by: &[OrderBy],
    ) -> DatabaseResult<SqlCursor> {
        if self.language.is_none() {
            return Ok(cursor_after(row, order_by)?);
        }
        let shown = super::in_language::shown_orders::<M>(order_by);
        let mut cursor = cursor_after(row, &shown)?;
        for (entry, order) in cursor.values.iter_mut().zip(order_by) {
            entry.column.clone_from(&order.column);
        }
        Ok(cursor)
    }

    fn select_sql(
        &self,
        query: &QueryBuilder<M, SqlDb, State>,
    ) -> DatabaseResult<(String, Vec<fundamental_db::FilterValue>)> {
        Ok(match &self.language {
            Some(language) if !M::translatable_columns().is_empty() => {
                super::in_language::build_select_sql_in::<M, _>(
                    self.conn.schema(),
                    query,
                    language,
                )?
            }
            _ => build_select_sql::<M, _>(self.conn.schema(), query, None)?,
        })
    }

    /// Executes the query and returns all matching models.
    #[instrument(skip(self), fields(table = M::TABLE, model = M::TYPE_NAME, op = "execute", rows = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute(mut self) -> DatabaseResult<Vec<M>>
    where
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
        M: ToRow,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (sql, values) = self.select_sql(&self.builder)?;
        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        let rows = self.conn.fetch_all_raw(sqlx_query).await?;
        Span::current().record("rows", rows.len());
        rows.iter()
            .map(|row| {
                super::translations::decode::<M>(row, None)
                    .map_err(|e| DatabaseError::serialization(e.to_string()))
            })
            .collect()
    }

    /// Executes the query and returns the first matching model.
    #[instrument(skip(self), fields(table = M::TABLE, model = M::TYPE_NAME, op = "execute_one", found = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn execute_one(mut self) -> DatabaseResult<Option<M>>
    where
        M::Id: Send + Sync + for<'q> sqlx::Encode<'q, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
        M: ToRow,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let limited_query = if self.builder.get_pagination().is_none() {
            self.builder.clone().limit(1)
        } else {
            self.builder.clone()
        };
        let (sql, values) = self.select_sql(&limited_query)?;
        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        let row = self.conn.fetch_optional_raw(sqlx_query).await?;
        Span::current().record("found", row.is_some());
        match row {
            Some(row) => {
                let model = super::translations::decode::<M>(&row, None)
                    .map_err(|e| DatabaseError::serialization(e.to_string()))?;
                Ok(Some(model))
            }
            None => Ok(None),
        }
    }

    /// Returns the count of matching models.
    #[instrument(skip(self), fields(table = M::TABLE, model = M::TYPE_NAME, op = "count", count = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn count(mut self) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (sql, values) = match &self.language {
            Some(language) if !M::translatable_columns().is_empty() => {
                super::in_language::build_count_sql_in::<M, _>(
                    self.conn.schema(),
                    &self.builder,
                    language,
                )?
            }
            _ => build_count_sql::<M, _>(self.conn.schema(), &self.builder)?,
        };
        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        let row = self.conn.fetch_one_raw(sqlx_query).await?;
        let count: i64 = sqlx::Row::get(&row, 0);
        Span::current().record("count", count);
        Ok(count as u64)
    }

    /// Deletes matching models and returns the number of rows affected.
    #[instrument(skip(self), fields(table = M::TABLE, model = M::TYPE_NAME, op = "delete", affected = Empty), err(Debug))]
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    pub async fn delete(mut self) -> DatabaseResult<u64>
    where
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let (sql, values) = build_delete_sql::<M, _>(self.conn.schema(), &self.builder)?;
        let counted = super::translations::with_companion_delete::<M>(self.conn.schema(), &sql);
        let mut sqlx_query = pg_query(counted.clone().unwrap_or(sql));
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        let affected = if counted.is_some() {
            let row = self.conn.fetch_one_raw(sqlx_query).await?;
            sqlx::Row::get::<i64, _>(&row, 0).unsigned_abs()
        } else {
            self.conn.execute_raw(sqlx_query).await?
        };
        Span::current().record("affected", affected);
        Ok(affected)
    }

    /// Executes a paginated query using keyset pagination: `None` for the first page, then the returned cursor.
    ///
    /// A query that declares no ordering is ordered by primary key.
    ///
    /// # Errors
    /// [`DatabaseError`] if the read fails, if the cursor cannot be turned into a
    /// resume predicate, or if an ordering column is absent from the selected row,
    /// `NULL`, or of a type a cursor cannot carry.
    #[instrument(
        skip(self, cursor),
        fields(table = M::TABLE, model = M::TYPE_NAME, op = "execute_page", page_size = Empty, rows = Empty, has_next = Empty),
        err(Debug)
    )]
    pub async fn execute_page(mut self, cursor: Option<SqlCursor>) -> DatabaseResult<SqlPage<M>>
    where
        M: ToRow,
        LeadingKeyAddressed<M, SqlDb, State>: Satisfied,
    {
        let page_size = self.page_size.unwrap_or(50);
        Span::current().record("page_size", page_size);

        if self.builder.get_order_by().is_empty() {
            self.builder = self.builder.order_by(OrderBy::asc(M::PRIMARY_KEY));
        }
        let order_by: Vec<OrderBy> = self.builder.get_order_by().to_vec();

        // One row-value comparison over the whole tuple; one `>` per column drops rows.
        let cursor_given = cursor.is_some();
        if let Some(cursor) = cursor {
            let filter = cursor
                .into_filter()
                .map_err(|error| DatabaseError::query(error.to_string()))?;
            self.builder = self.builder.filter(filter);
        }

        self.builder = self.builder.limit(page_size + 1);

        let (sql, values) = self.select_sql(&self.builder)?;
        let mut sqlx_query = pg_query(sql);
        for value in values {
            sqlx_query = bind_filter_value(sqlx_query, value);
        }
        let mut rows = self.conn.fetch_all_raw(sqlx_query).await?;

        let has_next = rows.len() > page_size as usize;
        if has_next {
            rows.truncate(page_size as usize);
        }
        // From the raw row, so the boundary is the column the ORDER BY names.
        let next_page = if has_next {
            match rows.last() {
                Some(last) => Some(self.boundary(last, &order_by)?),
                None => None,
            }
        } else {
            None
        };

        let previous_page = match rows.first() {
            Some(first) if cursor_given => Some(self.boundary(first, &order_by)?),
            _ => None,
        };

        let items: Vec<M> = rows
            .iter()
            .map(|row| {
                super::translations::decode::<M>(row, None)
                    .map_err(|e| DatabaseError::serialization(e.to_string()))
            })
            .collect::<DatabaseResult<Vec<M>>>()?;
        Span::current().record("rows", items.len());
        Span::current().record("has_next", has_next);

        Ok(SqlPage {
            items,
            next_page,
            previous_page,
        })
    }
}
