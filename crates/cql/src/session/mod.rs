//! YCQL session management.

mod address_translation;
mod cluster;
mod node;
mod pinned_server_name;
mod wrapper;

pub use cluster::ClusterInfo;
pub use node::NodeInfo;
pub use wrapper::CqlSession;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_info_default() {
        let info = ClusterInfo {
            nodes: vec![],
            total_nodes: 0,
        };
        assert!(info.nodes.is_empty());
        assert_eq!(info.nodes_up(), 0);
    }

    #[test]
    fn test_cluster_info_nodes_up() {
        let info = ClusterInfo {
            nodes: vec![
                NodeInfo {
                    address: "192.168.1.1:9042".into(),
                    datacenter: Some("dc1".into()),
                    rack: Some("rack1".into()),
                    is_up: true,
                },
                NodeInfo {
                    address: "192.168.1.2:9042".into(),
                    datacenter: Some("dc1".into()),
                    rack: Some("rack1".into()),
                    is_up: false,
                },
            ],
            total_nodes: 2,
        };
        assert_eq!(info.nodes_up(), 1);
    }

    #[test]
    fn test_cluster_info_by_datacenter() {
        let info = ClusterInfo {
            nodes: vec![
                NodeInfo {
                    address: "192.168.1.1:9042".into(),
                    datacenter: Some("dc1".into()),
                    rack: Some("rack1".into()),
                    is_up: true,
                },
                NodeInfo {
                    address: "192.168.1.2:9042".into(),
                    datacenter: Some("dc2".into()),
                    rack: Some("rack1".into()),
                    is_up: true,
                },
            ],
            total_nodes: 2,
        };
        let by_dc = info.nodes_by_datacenter();
        assert_eq!(by_dc.len(), 2);
        assert_eq!(by_dc.get("dc1").unwrap().len(), 1);
        assert_eq!(by_dc.get("dc2").unwrap().len(), 1);
    }

    #[test]
    fn test_node_info() {
        let node = NodeInfo {
            address: "192.168.1.1:9042".into(),
            datacenter: Some("dc1".into()),
            rack: Some("rack1".into()),
            is_up: true,
        };
        assert!(node.is_up);
    }
}
