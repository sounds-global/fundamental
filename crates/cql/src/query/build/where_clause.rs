//! Builder for the `WHERE` clause of a `SELECT` statement.

use crate::{ClusteringFilter, CqlError, CqlResult, CqlValue, build_clustering_filter};

/// Builds the WHERE clause from partition key columns, values, and clustering filters.
///
/// # Errors
///
/// Returns [`CqlError::InvalidQuery`] when the number of partition values does not
/// match the number of partition **columns**.
pub(crate) fn build_where_clause(
    pk_columns: &[&str],
    pk_values: &[CqlValue],
    clustering_filters: &[ClusteringFilter],
) -> CqlResult<(String, Vec<CqlValue>)> {
    if pk_columns.len() != pk_values.len() {
        return Err(CqlError::invalid_query(format!(
            "partition key has {} column(s) ({}) and {} value(s) were bound. A partition is \
             addressed by every one of its columns or by none of them — bind through \
             `CqlQuery::partition`, where the model's own `PartitionKey` decides the count",
            pk_columns.len(),
            pk_columns.join(", "),
            pk_values.len(),
        )));
    }

    let mut where_clauses = Vec::new();
    let mut values = Vec::new();

    for (col, value) in pk_columns.iter().zip(pk_values) {
        where_clauses.push(format!("{} = ?", col));
        values.push(value.clone());
    }

    for filter in clustering_filters {
        let (clause, filter_values) = build_clustering_filter(filter);
        if !clause.is_empty() {
            where_clauses.push(clause);
            values.extend(filter_values);
        }
    }

    Ok((where_clauses.join(" AND "), values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_a_partition_key_is_refused_rather_than_bound_short() {
        let err = build_where_clause(&["zone", "subject_ppid"], &[CqlValue::text("eu")], &[])
            .expect_err("a two-column partition addressed by one value must be refused");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
        // The message names both columns, because the caller's mistake is which column it
        // forgot and a count alone does not say.
        assert!(err.to_string().contains("subject_ppid"), "got: {err}");
    }

    #[test]
    fn more_values_than_columns_is_the_same_disagreement_and_is_refused_too() {
        let err = build_where_clause(
            &["attribute_id"],
            &[CqlValue::text("eu"), CqlValue::uuid(uuid::Uuid::nil())],
            &[],
        )
        .expect_err("a one-column partition addressed by two values must be refused");
        assert!(matches!(err, CqlError::InvalidQuery(_)));
    }

    #[test]
    fn a_whole_partition_key_binds_every_column_in_order() {
        let (clause, values) = build_where_clause(
            &["zone", "subject_ppid"],
            &[CqlValue::text("eu"), CqlValue::text("ppid")],
            &[],
        )
        .expect("a whole partition key builds");
        assert_eq!(clause, "zone = ? AND subject_ppid = ?");
        assert_eq!(values.len(), 2);
    }
}
