//! Clustering-key filter conditions.

use crate::ClusteringOp;

/// A filter condition on a clustering key column.
#[derive(Debug, Clone)]
pub(crate) struct ClusteringFilter {
    pub column: String,
    pub op: ClusteringOp,
}
