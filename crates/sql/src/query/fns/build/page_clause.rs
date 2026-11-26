use super::build_order_clause;
use crate::SqlError;

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::QueryBuilder;

/// Builds a query's ` ORDER BY … LIMIT … OFFSET …` tail — empty when it has none — so a
/// `SELECT` and the key subquery of a bounded `DELETE` page identically.
pub(crate) fn build_page_clause<M: SqlModel, State>(
    query: &QueryBuilder<M, SqlDb, State>,
) -> Result<String, SqlError> {
    let mut clause = String::new();
    if !query.get_order_by().is_empty() {
        clause.push_str(" ORDER BY ");
        clause.push_str(&build_order_clause(query.get_order_by())?);
    }
    if let Some(pagination) = query.get_pagination() {
        // A limit past the largest bigint is no limit, and the database refuses it as out of range.
        if i64::try_from(pagination.limit).is_ok() {
            clause.push_str(&format!(" LIMIT {}", pagination.limit));
        }
        if pagination.offset > 0 {
            clause.push_str(&format!(" OFFSET {}", pagination.offset));
        }
    }
    Ok(clause)
}
