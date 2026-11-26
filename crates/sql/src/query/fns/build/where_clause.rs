use super::build_filter_condition;
use crate::SqlError;

use fundamental_db::{Filter, FilterValue};

/// Builds a WHERE clause from filters, refusing an unsafe column identifier.
pub(crate) fn build_where_clause(
    filters: &[Filter],
    start_index: usize,
) -> Result<(String, Vec<FilterValue>, usize), SqlError> {
    let mut clauses = Vec::new();
    let mut values = Vec::new();
    let mut index = start_index;

    for filter in filters {
        let (clause, filter_values, next_index) = build_filter_condition(&filter.condition, index)?;
        clauses.push(clause);
        values.extend(filter_values);
        index = next_index;
    }

    Ok((clauses.join(" AND "), values, index))
}
