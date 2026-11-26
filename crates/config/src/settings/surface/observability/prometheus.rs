//! Prometheus settings ([`PrometheusSettings`]).

use serde::{Deserialize, Serialize};

/// Prometheus metrics configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrometheusSettings {
    /// Whether Prometheus metrics are enabled.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub enabled: bool,
    /// HTTP endpoint path for metrics (e.g., "/metrics").
    pub endpoint: String,
}

impl Default for PrometheusSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "/metrics".to_string(),
        }
    }
}
