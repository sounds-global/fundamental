//! Builder for a single clustering-column filter clause.

use std::ops::Bound;

use crate::{ClusteringFilter, ClusteringOp, CqlValue};

/// Builds a clustering filter clause and returns the SQL fragment and values.
pub(crate) fn build_clustering_filter(filter: &ClusteringFilter) -> (String, Vec<CqlValue>) {
    let mut values = Vec::new();
    let clauses = match &filter.op {
        ClusteringOp::Eq(v) => {
            values.push(v.clone());
            vec![format!("{} = ?", filter.column)]
        }
        ClusteringOp::Gt(v) => {
            values.push(v.clone());
            vec![format!("{} > ?", filter.column)]
        }
        ClusteringOp::Gte(v) => {
            values.push(v.clone());
            vec![format!("{} >= ?", filter.column)]
        }
        ClusteringOp::Lt(v) => {
            values.push(v.clone());
            vec![format!("{} < ?", filter.column)]
        }
        ClusteringOp::Lte(v) => {
            values.push(v.clone());
            vec![format!("{} <= ?", filter.column)]
        }
        ClusteringOp::Range { low, high } => {
            let mut range_clauses = Vec::new();
            match low {
                Bound::Included(v) => {
                    range_clauses.push(format!("{} >= ?", filter.column));
                    values.push(v.clone());
                }
                Bound::Excluded(v) => {
                    range_clauses.push(format!("{} > ?", filter.column));
                    values.push(v.clone());
                }
                Bound::Unbounded => {}
            }
            match high {
                Bound::Included(v) => {
                    range_clauses.push(format!("{} <= ?", filter.column));
                    values.push(v.clone());
                }
                Bound::Excluded(v) => {
                    range_clauses.push(format!("{} < ?", filter.column));
                    values.push(v.clone());
                }
                Bound::Unbounded => {}
            }
            range_clauses
        }
    };
    (clauses.join(" AND "), values)
}
