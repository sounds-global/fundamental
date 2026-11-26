//! Cluster-level metadata for a connected YCQL session.

use crate::NodeInfo;

/// Information about the YCQL cluster.
#[derive(Debug, Clone)]
pub struct ClusterInfo {
    /// Nodes in the cluster.
    pub nodes: Vec<NodeInfo>,
    /// Total number of nodes in the cluster.
    pub total_nodes: usize,
}

impl ClusterInfo {
    /// Returns the number of nodes that are up.
    pub fn nodes_up(&self) -> usize {
        self.nodes.iter().filter(|n| n.is_up).count()
    }

    /// Returns nodes grouped by datacenter.
    pub fn nodes_by_datacenter(&self) -> std::collections::HashMap<String, Vec<&NodeInfo>> {
        let mut by_dc = std::collections::HashMap::new();
        for node in &self.nodes {
            let dc = node
                .datacenter
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            by_dc.entry(dc).or_insert_with(Vec::new).push(node);
        }
        by_dc
    }
}
