use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_core::outbox::OutboxColumns;
use fundamental_db::{QueryBuilder, SchemaName};
use sqlx::{Postgres, Row};

use super::{ClaimedKey, SetAsideRow};
use crate::ident::validate_identifier;
use crate::query::{build_where_clause, pg_query};
use crate::{SqlError, SqlResult, bind_filter_value};

/// Reads up to `cap` of `query`'s set-aside rows, selecting only the key and framework columns.
///
/// # Errors
/// [`SqlError::Query`] for a key column that is not a safe identifier,
/// [`SqlError::UnaddressableKey`] for a key column whose text copy does not cast back, or the
/// driver's error.
pub(crate) async fn read_set_aside_with<'e, E, M, State>(
    exec: E,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    cap: u64,
) -> SqlResult<Vec<SetAsideRow>>
where
    E: sqlx::Executor<'e, Database = Postgres>,
    M: SqlModel,
{
    let mut columns = Vec::with_capacity(M::KEY_COLUMNS.len() * 2 + 3);
    for (index, column) in M::KEY_COLUMNS.iter().enumerate() {
        validate_identifier(column, false)?;
        columns.push((*column).to_owned());
        columns.push(format!("{column}::text AS {}", ClaimedKey::alias(index)));
    }
    columns.extend(
        [
            OutboxColumns::SET_ASIDE_AT,
            OutboxColumns::SET_ASIDE_REASON,
            OutboxColumns::SET_ASIDE_SEAL,
        ]
        .map(str::to_owned),
    );
    let (predicate, values, _) = build_where_clause(query.get_filters(), 1)?;
    let filter = if predicate.is_empty() {
        String::new()
    } else {
        format!("{predicate} AND ")
    };
    let sql = format!(
        "SELECT {} FROM {} WHERE {filter}{} IS NOT NULL LIMIT {cap}",
        columns.join(", "),
        schema.qualify(M::TABLE),
        OutboxColumns::SET_ASIDE_AT,
    );
    let mut read = pg_query(sql);
    for value in values {
        read = bind_filter_value(read, value);
    }
    let rows = read.fetch_all(exec).await.map_err(SqlError::from)?;
    rows.iter()
        .map(|row| {
            Ok(SetAsideRow {
                key: ClaimedKey::read::<M>(row)?,
                at: row.try_get(OutboxColumns::SET_ASIDE_AT)?,
                reason: row.try_get(OutboxColumns::SET_ASIDE_REASON)?,
                seal: row.try_get(OutboxColumns::SET_ASIDE_SEAL)?,
            })
        })
        .collect()
}
