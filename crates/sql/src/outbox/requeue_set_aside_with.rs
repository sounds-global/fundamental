use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_core::outbox::OutboxColumns;
use fundamental_db::{QueryBuilder, SchemaName};
use sqlx::PgConnection;

use super::ClaimedKey;
use crate::ident::validate_identifier;
use crate::query::{build_where_clause, pg_query};
use crate::{SqlError, SqlResult, bind_filter_value};

/// Clears the set-aside columns of up to `batch` of `query`'s set-aside rows, locked `SKIP LOCKED`; returns how many.
///
/// # Errors
/// [`SqlError::OutboxMarkMissed`] when a clear does not land on exactly the row locked for it,
/// [`SqlError::Query`] for a key column that is not a safe identifier, or the driver's error.
pub(crate) async fn requeue_set_aside_with<M, State>(
    conn: &mut PgConnection,
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    batch: u64,
) -> SqlResult<u64>
where
    M: SqlModel,
{
    let mut columns = Vec::with_capacity(M::KEY_COLUMNS.len() * 2);
    for (index, column) in M::KEY_COLUMNS.iter().enumerate() {
        validate_identifier(column, false)?;
        columns.push((*column).to_owned());
        columns.push(format!("{column}::text AS {}", ClaimedKey::alias(index)));
    }
    let (predicate, values, _) = build_where_clause(query.get_filters(), 1)?;
    let filter = if predicate.is_empty() {
        String::new()
    } else {
        format!("{predicate} AND ")
    };
    let sql = format!(
        "SELECT {} FROM {} WHERE {filter}{} IS NOT NULL LIMIT {batch} FOR UPDATE SKIP LOCKED",
        columns.join(", "),
        schema.qualify(M::TABLE),
        OutboxColumns::SET_ASIDE_AT,
    );
    let mut read = pg_query(sql);
    for value in values {
        read = bind_filter_value(read, value);
    }
    let rows = read.fetch_all(&mut *conn).await.map_err(SqlError::from)?;
    let keys = rows
        .iter()
        .map(|row| ClaimedKey::read::<M>(row))
        .collect::<SqlResult<Vec<_>>>()?;

    let mut requeued = 0_u64;
    for key in &keys {
        let (predicate, values) = key.predicate(1);
        let sql = format!(
            "UPDATE {} SET {} = NULL, {} = NULL, {} = NULL WHERE {predicate}",
            schema.qualify(M::TABLE),
            OutboxColumns::SET_ASIDE_AT,
            OutboxColumns::SET_ASIDE_REASON,
            OutboxColumns::SET_ASIDE_SEAL,
        );
        let mut clear = pg_query(sql);
        for value in values {
            clear = clear.bind(value);
        }
        let affected = clear
            .execute(&mut *conn)
            .await
            .map_err(SqlError::from)?
            .rows_affected();
        if affected != 1 {
            return Err(SqlError::OutboxMarkMissed {
                table: M::TABLE,
                affected,
            });
        }
        requeued += 1;
    }
    Ok(requeued)
}
