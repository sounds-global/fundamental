//! [`DropMaterializedViewChange`]: drops a YCQL materialized view.

/// Drop materialized view change (YCQL).
#[derive(Debug, Clone, PartialEq)]
pub struct DropMaterializedViewChange {
    /// View name.
    pub view_name: String,
}
