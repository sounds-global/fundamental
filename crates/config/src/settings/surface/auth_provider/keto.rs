//! Ory Keto fine-grained authorization settings ([`KetoSettings`]).

use serde::{Deserialize, Serialize};

/// Ory Keto fine-grained authorization configuration (Zanzibar model).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KetoSettings {
    /// Whether the Keto provider is enabled.
    pub enabled: bool,
    /// Keto read API URL (used for permission checks).
    pub read_url: String,
    /// Keto write API URL (used for relationship management).
    pub write_url: String,
}

impl Default for KetoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            read_url: "http://localhost:4466".to_string(),
            write_url: "http://localhost:4467".to_string(),
        }
    }
}
