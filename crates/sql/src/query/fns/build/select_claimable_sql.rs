use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};

use super::build_select_sql;
use crate::SqlError;
use crate::ident::validate_identifier;
use crate::outbox::ClaimedKey;
use crate::query::RowLock;

/// An outbox claim's locked read: every column, plus a text copy of each key column, so a row that refuses to decode can still be addressed.
///
/// # Errors
/// [`SqlError::Query`] for a query that names its own projection or a key column that is not a
/// safe identifier, or the errors [`build_select_sql`] returns.
pub(crate) fn build_select_claimable_sql<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    row_lock: Option<RowLock>,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    if query.get_select().is_some() {
        return Err(SqlError::Query(
            "an outbox claim reads every column of its row".to_owned(),
        ));
    }
    let (sql, values) = build_select_sql::<M, State>(schema, query, row_lock)?;
    let mut copies = Vec::with_capacity(M::KEY_COLUMNS.len());
    for (index, column) in M::KEY_COLUMNS.iter().enumerate() {
        validate_identifier(column, false)?;
        copies.push(format!("{column}::text AS {}", ClaimedKey::alias(index)));
    }
    let Some(rest) = sql.strip_prefix("SELECT * FROM ") else {
        return Err(SqlError::Query(
            "an outbox claim's read must select every column".to_owned(),
        ));
    };
    Ok((
        format!("SELECT *, {} FROM {rest}", copies.join(", ")),
        values,
    ))
}
