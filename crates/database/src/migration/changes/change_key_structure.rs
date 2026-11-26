//! The [`ChangeKeyStructureChange`] schema change (YCQL), which needs a new table.

use fundamental_core::model::cql::ClusteringOrder;

/// Change key structure (YCQL - requires new table).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeKeyStructureChange {
    /// Table name.
    pub table_name: String,
    /// Old partition key.
    pub old_partition_key: Vec<String>,
    /// New partition key.
    pub new_partition_key: Vec<String>,
    /// Old clustering key.
    pub old_clustering_key: Vec<(String, ClusteringOrder)>,
    /// New clustering key.
    pub new_clustering_key: Vec<(String, ClusteringOrder)>,
}
