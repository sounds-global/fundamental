//! Definition of a single clustering key column for a YCQL table.

use super::ClusteringOrder;

/// Clustering key definition for YCQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusteringKeyDef {
    /// Column name.
    pub column: &'static str,
    /// Sort order.
    pub order: ClusteringOrder,
}

impl ClusteringKeyDef {
    /// Creates a clustering key with ascending order.
    pub const fn asc(column: &'static str) -> Self {
        Self {
            column,
            order: ClusteringOrder::Asc,
        }
    }

    /// Creates a clustering key with descending order.
    pub const fn desc(column: &'static str) -> Self {
        Self {
            column,
            order: ClusteringOrder::Desc,
        }
    }
}
