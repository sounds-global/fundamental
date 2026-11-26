use chrono::{DateTime, Utc};
use fundamental_core::model::SqlModel;
use fundamental_core::outbox::OutboxColumns;
use fundamental_db::SchemaName;
use sqlx::Postgres;

use super::ClaimedKey;
use crate::query::pg_query;
use crate::{SqlError, SqlResult};

/// Leaves the row `key` addresses unclaimable until `until`, for a row this process's keys could not open.
///
/// # Errors
/// [`SqlError::OutboxMarkMissed`] when the write does not land on exactly one row, or the
/// driver's error.
pub(crate) async fn mark_skipped_with<'e, E, M>(
    exec: E,
    schema: &SchemaName,
    key: &ClaimedKey,
    until: DateTime<Utc>,
) -> SqlResult<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel,
{
    let (predicate, values) = key.predicate(2);
    let sql = format!(
        "UPDATE {} SET {} = $1 WHERE {predicate}",
        schema.qualify(M::TABLE),
        OutboxColumns::SKIP_UNTIL,
    );
    let mut write = pg_query(sql).bind(until);
    for value in values {
        write = write.bind(value);
    }
    let affected = write
        .execute(exec)
        .await
        .map_err(SqlError::from)?
        .rows_affected();
    if affected == 1 {
        Ok(())
    } else {
        Err(SqlError::OutboxMarkMissed {
            table: M::TABLE,
            affected,
        })
    }
}
