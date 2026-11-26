//! Ory Oathkeeper API gateway settings ([`OathkeeperSettings`]).

use serde::{Deserialize, Serialize};

/// Ory Oathkeeper API gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OathkeeperSettings {
    /// Whether the Oathkeeper provider is enabled.
    pub enabled: bool,
    /// Oathkeeper decisions API URL.
    pub decisions_url: String,
}

impl Default for OathkeeperSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            decisions_url: "http://localhost:4456/decisions".to_string(),
        }
    }
}
