use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_core::outbox::HeldCount;
use fundamental_db::{QueryBuilder, SchemaName};
use sqlx::Postgres;

use crate::query::{build_where_clause, pg_query};
use crate::{SqlError, SqlResult, bind_filter_value};

/// Counts the rows `query` selects, reading no more than `cap`, and says whether it stopped there.
///
/// # Errors
/// [`SqlError::Query`] for a filter naming an unsafe identifier or a count that is negative, or
/// the driver's error.
pub(crate) async fn count_held_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    cap: u64,
) -> SqlResult<HeldCount>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel,
{
    let (predicate, values, _) = build_where_clause(query.get_filters(), 1)?;
    let filter = if predicate.is_empty() {
        String::new()
    } else {
        format!(" WHERE {predicate}")
    };
    let sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM {}{filter} LIMIT {cap}) AS held",
        schema.qualify(M::TABLE),
    );
    let mut count = pg_query(sql);
    for value in values {
        count = bind_filter_value(count, value);
    }
    let row = count.fetch_one(exec).await.map_err(SqlError::from)?;
    let counted: i64 = sqlx::Row::try_get(&row, 0)?;
    let counted = u64::try_from(counted)
        .map_err(|_| SqlError::Query(format!("a count over {} came back negative", M::TABLE)))?;
    Ok(HeldCount::under_cap(counted, cap))
}
