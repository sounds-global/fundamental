//! Builder for a `SELECT COUNT(*)` CQL statement.

use crate::ident::validate_cql_identifier;
use crate::{ClusteringFilter, CqlResult, CqlValue, build_where_clause};

/// Builds a partition-scoped `SELECT COUNT(*)` statement and its bind values.
///
/// Only the clustering-filter column names come from the caller, and they are validated here.
///
/// # Errors
///
/// Returns [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier)
/// if any clustering-filter column is not a safe identifier.
pub(crate) fn build_count_cql(
    table_name: &str,
    pk_columns: &[&str],
    pk_values: &[CqlValue],
    clustering_filters: &[ClusteringFilter],
) -> CqlResult<(String, Vec<CqlValue>)> {
    for filter in clustering_filters {
        validate_cql_identifier(&filter.column, false)?;
    }

    let (where_clause, values) = build_where_clause(pk_columns, pk_values, clustering_filters)?;
    let cql = format!("SELECT COUNT(*) FROM {table_name} WHERE {where_clause}");
    Ok((cql, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClusteringOp, CqlError};

    #[test]
    fn builds_partition_scoped_count() {
        let pk = vec!["tenant_id"];
        let vals = vec![CqlValue::uuid(uuid::Uuid::nil())];
        let filters = vec![ClusteringFilter {
            column: "bucket".to_string(),
            op: ClusteringOp::Gte(CqlValue::bigint(10)),
        }];
        let (cql, bound) = build_count_cql("ks.events", &pk, &vals, &filters).expect("valid");
        assert_eq!(
            cql,
            "SELECT COUNT(*) FROM ks.events WHERE tenant_id = ? AND bucket >= ?"
        );
        // Bind order: partition key first, then clustering values.
        assert_eq!(bound.len(), 2);
        assert!(matches!(bound[0], CqlValue::Uuid(_)));
        assert!(matches!(bound[1], CqlValue::BigInt(10)));
    }

    #[test]
    fn rejects_injection_in_clustering_column() {
        let pk = vec!["tenant_id"];
        let vals = vec![CqlValue::uuid(uuid::Uuid::nil())];
        let filters = vec![ClusteringFilter {
            column: "bucket = 1 OR 1=1".to_string(),
            op: ClusteringOp::Eq(CqlValue::bigint(1)),
        }];
        let err = build_count_cql("ks.events", &pk, &vals, &filters)
            .expect_err("malicious clustering column must be rejected");
        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
    }
}
