//! Per-node metadata for a YCQL cluster.

/// Information about a YCQL node.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node address.
    pub address: String,
    /// Datacenter.
    pub datacenter: Option<String>,
    /// Rack within datacenter.
    pub rack: Option<String>,
    /// Whether the node is up.
    pub is_up: bool,
}
