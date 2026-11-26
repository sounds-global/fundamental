use fundamental_core::Model;
use fundamental_core::model::{BoundTo, SqlDb, SqlModel};
use fundamental_db::SchemaName;
use fundamental_encryption::KeyManager;
use sqlx::Postgres;

use super::{ClaimedKey, ClaimedRow};
use crate::query::{SqlQuery, build_select_claimable_sql, pg_query};
use crate::{FromRow, SqlError, SqlResult, ToRow, bind_filter_value};

/// Runs an outbox claim's locked read and decodes every row separately, so one refusal costs one row.
///
/// # Errors
/// The read's own failure, a key that cannot be addressed, or any decode failure that is not a
/// row's refusal.
pub(crate) async fn select_claimable_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    enc: Option<&KeyManager>,
    query: &SqlQuery<M, State>,
) -> SqlResult<Vec<ClaimedRow<M>>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: BoundTo<SqlDb> + SqlModel + Model + FromRow + ToRow + Send + Sync,
{
    let (sql, values) =
        build_select_claimable_sql::<M, _>(schema, query.inner(), query.get_row_lock())?;
    let mut read = pg_query(sql);
    for value in values {
        read = bind_filter_value(read, value);
    }
    let rows = read.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter()
        .map(|row| {
            let key = ClaimedKey::read::<M>(row)?;
            match M::from_row_encrypted(row, enc) {
                Ok(model) => Ok(ClaimedRow::Decoded(model)),
                Err(SqlError::RowRefused { column, kind, .. }) => {
                    Ok(ClaimedRow::Refused { key, column, kind })
                }
                Err(other) => Err(other),
            }
        })
        .collect()
}
