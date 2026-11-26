//! The [`CreateTableChange`] schema change.

use crate::migration::{TableCheckConstraint, TableColumn, TableForeignKey, TableIndex};
use fundamental_core::model::cql::{ClusteringOrder, TableOptions};

/// Create table change.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateTableChange {
    /// Table name.
    pub table_name: String,
    /// Column definitions.
    pub columns: Vec<TableColumn>,
    /// Primary key columns.
    pub primary_key: Vec<String>,
    /// Indexes.
    pub indexes: Vec<TableIndex>,
    /// Foreign keys.
    pub foreign_keys: Vec<TableForeignKey>,
    /// Check constraints.
    pub check_constraints: Vec<TableCheckConstraint>,
    /// YCQL: partition key columns.
    pub partition_key: Option<Vec<String>>,
    /// YCQL: clustering key columns.
    pub clustering_key: Option<Vec<(String, ClusteringOrder)>>,
    /// YCQL: table options.
    pub table_options: Option<TableOptions>,
}
