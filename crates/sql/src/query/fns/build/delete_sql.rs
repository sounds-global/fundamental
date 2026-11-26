use super::{build_page_clause, build_where_clause};
use crate::SqlError;
use crate::ident::validate_identifier;

use fundamental_core::model::{SqlDb, SqlModel};
use fundamental_db::{FilterValue, QueryBuilder, SchemaName};

/// Builds a DELETE SQL statement; an order or limit chooses keys in a subquery, and the filter is
/// repeated outside it so a row changed meanwhile is spared.
pub(crate) fn build_delete_sql<M: SqlModel, State>(
    schema: &SchemaName,
    query: &QueryBuilder<M, SqlDb, State>,
) -> Result<(String, Vec<FilterValue>), SqlError> {
    let mut values = Vec::new();
    let param_index = 1;
    let table = schema.qualify(M::TABLE);

    let mut filter = String::new();
    let mut recheck = String::new();
    if !query.get_filters().is_empty() {
        let (where_clause, where_values, _) = build_where_clause(query.get_filters(), param_index)?;
        if !where_clause.is_empty() {
            filter.push_str(" WHERE ");
            filter.push_str(&where_clause);
            recheck.push_str(" AND (");
            recheck.push_str(&where_clause);
            recheck.push(')');
        }
        values.extend(where_values);
    }

    let bounded = query.get_pagination().is_some() || !query.get_order_by().is_empty();
    if !bounded {
        return Ok((format!("DELETE FROM {table}{filter}"), values));
    }

    for column in M::KEY_COLUMNS {
        validate_identifier(column, false)?;
    }
    let key = match M::KEY_COLUMNS {
        [single] => (*single).to_owned(),
        columns => format!("({})", columns.join(", ")),
    };
    let mut chosen = format!("SELECT {} FROM {table}{filter}", M::KEY_COLUMNS.join(", "));
    chosen.push_str(&build_page_clause(query)?);
    Ok((
        format!("DELETE FROM {table} WHERE {key} IN ({chosen}){recheck}"),
        values,
    ))
}
