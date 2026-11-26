//! Ory Hydra OAuth2/OIDC server settings ([`HydraSettings`]).

use serde::{Deserialize, Serialize};

/// Ory Hydra OAuth2/OIDC server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HydraSettings {
    /// Whether the Hydra provider is enabled.
    pub enabled: bool,
    /// Hydra public API URL (used for OAuth2 flows).
    pub public_url: String,
    /// Hydra admin API URL (used for client/consent management).
    pub admin_url: String,
}

impl Default for HydraSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            public_url: "http://localhost:4444".to_string(),
            admin_url: "http://localhost:4445".to_string(),
        }
    }
}
