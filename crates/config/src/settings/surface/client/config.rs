//! Client surface settings ([`ClientSettings`]).

use super::ApiEndpointSettings;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Client configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    /// Primary API URL.
    pub api_url: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Named API endpoints.
    #[serde(default)]
    pub apis: HashMap<String, ApiEndpointSettings>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            api_url: "http://127.0.0.1:16001".to_string(),
            timeout_secs: 30,
            apis: HashMap::new(),
        }
    }
}
