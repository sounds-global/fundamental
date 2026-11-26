use super::build_where_clause;
use crate::SqlError;

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};

/// Builds a COUNT SQL statement from a query builder.
pub(crate) fn build_count_sql<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let mut values = Vec::new();
    let param_index = 1;

    let mut sql = format!("SELECT COUNT(*) FROM {}", schema.qualify(M::TABLE));

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

    Ok((sql, values))
}
