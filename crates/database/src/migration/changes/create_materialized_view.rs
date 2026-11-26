//! The [`CreateMaterializedViewChange`] schema change (YCQL).

use fundamental_core::model::cql::ClusteringOrder;

/// Create materialized view change (YCQL).
#[derive(Debug, Clone, PartialEq)]
pub struct CreateMaterializedViewChange {
    /// View name.
    pub view_name: String,
    /// Base table name.
    pub base_table: String,
    /// Columns to include.
    pub columns: Vec<String>,
    /// Partition key columns.
    pub partition_key: Vec<String>,
    /// Clustering key.
    pub clustering_key: Vec<(String, ClusteringOrder)>,
    /// WHERE clause.
    pub where_clause: Option<String>,
}
