//! Trace sampling settings ([`SamplingSettings`]).

use serde::{Deserialize, Serialize};

/// Trace sampling configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplingSettings {
    /// Sampling ratio, 0.0 (none) to 1.0 (all, the default).
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub ratio: f64,
    /// Whether to always sample traces with errors.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub always_sample_errors: bool,
}

impl Default for SamplingSettings {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            always_sample_errors: true,
        }
    }
}
