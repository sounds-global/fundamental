use serde::{Deserialize, Serialize};

/// Configuration for a JetStream context: a domain or a custom API prefix.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JetStreamConfig {
    /// The JetStream domain, for multi-tenancy.
    #[serde(default)]
    pub domain: Option<String>,
    /// A custom JetStream API prefix.
    #[serde(default)]
    pub api_prefix: Option<String>,
}
