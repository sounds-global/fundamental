//! Settings for an individual named API endpoint used by a client surface.

use serde::{Deserialize, Serialize};

/// Named API endpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEndpointSettings {
    /// Base URL for the API.
    pub base_url: String,
    /// Optional timeout override in seconds.
    pub timeout_secs: Option<u64>,
}
