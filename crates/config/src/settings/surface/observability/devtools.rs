//! Devtools settings ([`DevtoolsSettings`]).

use serde::{Deserialize, Serialize};

/// Devtools configuration for in-process monitoring and remote access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DevtoolsSettings {
    /// Whether devtools collection is enabled.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub enabled: bool,
    /// TCP port for devtools server (0 = OS-assigned ephemeral).
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub port: u16,
    /// Optional API key clients must provide; `None` accepts every connection.
    pub api_key: Option<String>,
}

impl Default for DevtoolsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 0,
            api_key: None,
        }
    }
}
