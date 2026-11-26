//! Builds `SELECT` statements with an optional row-locking clause.

use super::{build_page_clause, build_where_clause};
use crate::SqlError;
use crate::ident::validate_identifier;
use crate::query::RowLock;

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};

/// Builds a SELECT SQL statement with an optional row-locking clause, refusing an unsafe identifier.
pub(crate) fn build_select_sql<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
    row_lock: Option<RowLock>,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let mut values = Vec::new();
    let param_index = 1;

    let select = match query.get_select() {
        Some(cols) => {
            for col in cols {
                validate_identifier(col, true)?;
            }
            cols.join(", ")
        }
        None => "*".to_string(),
    };

    let table = schema.qualify(M::TABLE);
    let select = crate::model::translations::select_list::<M>(schema, &select, &table);
    let mut sql = format!("SELECT {select} FROM {table}");

    let mut where_parts: Vec<String> = Vec::new();
    if !query.get_filters().is_empty() {
        let (where_clause, where_values, _) = build_where_clause(query.get_filters(), param_index)?;
        if !where_clause.is_empty() {
            where_parts.push(where_clause);
        }
        values.extend(where_values);
    }
    if M::SOFT_DELETE {
        where_parts.push(format!("{} IS NULL", M::DELETED_AT_COLUMN));
    }
    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }

    sql.push_str(&build_page_clause(query)?);

    // PostgreSQL grammar puts the locking clause last, after LIMIT/OFFSET.
    if let Some(lock) = row_lock {
        sql.push(' ');
        sql.push_str(lock.to_sql());
    }

    Ok((sql, values))
}
