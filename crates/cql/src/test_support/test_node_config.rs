//! [`test_node_config`] — the connection every test path opens against the local node.

use crate::CqlConfig;

/// A development config for the single local node a test suite runs against.
///
/// Token-aware routing is off: its metadata fetch reads `system_schema.scylla_tables`, which
/// YCQL lacks, and one node routes identically under every policy. TLS is off as in
/// [`CqlConfig::development`].
#[must_use]
pub fn test_node_config<I, S>(contact_points: I) -> CqlConfig
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    CqlConfig::development(contact_points).token_aware_routing(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_node_is_not_asked_for_a_routing_map_it_cannot_build() {
        let config = test_node_config(["127.0.0.1:9042"]);

        assert!(
            !config.token_aware_routing,
            "the metadata behind token-awareness is what fails on this wire"
        );
    }

    #[test]
    fn a_production_config_keeps_its_routing() {
        // The default stays on for engines that have those tables; only the test node opts out.
        let config = CqlConfig::new(["node1:9042"]);

        assert!(config.token_aware_routing);
    }
}
