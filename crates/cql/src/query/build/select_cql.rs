//! Builder for a complete `SELECT` CQL statement.

use crate::ident::validate_cql_identifier;
use crate::{ClusteringFilter, CqlResult, CqlValue, build_where_clause};

/// Builds a complete SELECT CQL statement.
///
/// The one place caller-supplied identifiers reach CQL text; each is validated first.
///
/// # Errors
///
/// Returns [`CqlError::InvalidIdentifier`](crate::CqlError::InvalidIdentifier)
/// if any selected column or clustering-filter column is not a safe identifier.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
pub(crate) fn build_select_cql(
    table_name: String,
    select_columns: Option<&[String]>,
    pk_columns: &[&str],
    pk_values: &[CqlValue],
    clustering_filters: &[ClusteringFilter],
    limit: Option<u32>,
    allow_filtering: bool,
) -> CqlResult<(String, Vec<CqlValue>)> {
    // `*` is permitted only in the column list.
    if let Some(cols) = select_columns {
        for col in cols {
            validate_cql_identifier(col, true)?;
        }
    }
    for filter in clustering_filters {
        validate_cql_identifier(&filter.column, false)?;
    }

    let select = match select_columns {
        Some(cols) => cols.join(", "),
        None => "*".to_string(),
    };

    let (where_clause, values) = build_where_clause(pk_columns, pk_values, clustering_filters)?;

    let mut cql = format!(
        "SELECT {} FROM {} WHERE {}",
        select, table_name, where_clause
    );

    if let Some(limit) = limit {
        cql.push_str(&format!(" LIMIT {}", limit));
    }

    if allow_filtering {
        cql.push_str(" ALLOW FILTERING");
    }

    Ok((cql, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClusteringOp, CqlError};

    fn pk() -> (Vec<&'static str>, Vec<CqlValue>) {
        (vec!["user_id"], vec![CqlValue::uuid(uuid::Uuid::nil())])
    }

    #[test]
    fn rejects_injection_in_select_column() {
        let (cols, vals) = pk();
        let malicious = vec!["id".to_string(), "x FROM t; DROP TABLE t; --".to_string()];
        let err = build_select_cql(
            "ks.t".into(),
            Some(&malicious),
            &cols,
            &vals,
            &[],
            None,
            false,
        )
        .expect_err("malicious select column must be rejected");
        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
    }

    #[test]
    fn rejects_injection_in_clustering_column() {
        let (cols, vals) = pk();
        let filters = vec![ClusteringFilter {
            column: "ts = 1 OR 1=1".to_string(),
            op: ClusteringOp::Eq(CqlValue::bigint(1)),
        }];
        let err = build_select_cql("ks.t".into(), None, &cols, &vals, &filters, None, false)
            .expect_err("malicious clustering column must be rejected");
        assert!(matches!(err, CqlError::InvalidIdentifier(_)));
    }

    #[test]
    fn accepts_valid_columns_and_dotted_identifiers() {
        let (cols, vals) = pk();
        let select = vec!["user_id".to_string(), "events.ts".to_string()];
        let filters = vec![ClusteringFilter {
            column: "ts".to_string(),
            op: ClusteringOp::Eq(CqlValue::bigint(1)),
        }];
        let (cql, _) = build_select_cql(
            "ks.t".into(),
            Some(&select),
            &cols,
            &vals,
            &filters,
            None,
            false,
        )
        .expect("valid identifiers must pass");
        assert!(cql.starts_with("SELECT user_id, events.ts FROM ks.t WHERE"));
    }
}
