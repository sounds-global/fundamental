//! Ory Kratos identity management settings ([`KratosSettings`]).

use serde::{Deserialize, Serialize};

/// Ory Kratos identity management configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KratosSettings {
    /// Whether the Kratos provider is enabled.
    pub enabled: bool,
    /// Kratos public API URL (used for self-service flows).
    pub public_url: String,
    /// Kratos admin API URL (used for identity management).
    pub admin_url: String,
}

impl Default for KratosSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            public_url: "http://localhost:4433".to_string(),
            admin_url: "http://localhost:4434".to_string(),
        }
    }
}
