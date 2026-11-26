//! Top-level observability settings ([`ObservabilitySettings`]).

use super::{DevtoolsSettings, OtlpSettings, PrometheusSettings, SamplingSettings};

use fundamental_core::Environment;

use serde::{Deserialize, Serialize};

/// Observability configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ObservabilitySettings {
    /// Log level (trace, debug, info, warn, error).
    pub log_level: String,
    /// Log format (pretty, json).
    pub log_format: String,
    /// Whether to enable tracing.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub tracing_enabled: bool,
    /// The level at or above which spans are recorded, separate from `log_level`.
    pub span_level: String,
    /// Tracing endpoint URL (deprecated, use otlp.endpoint).
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tracing_endpoint: Option<String>,
    /// Service name for tracing.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub service_name: Option<String>,
    /// OTLP export configuration.
    pub otlp: OtlpSettings,
    /// Prometheus metrics configuration.
    pub prometheus: PrometheusSettings,
    /// Trace sampling configuration.
    pub sampling: SamplingSettings,
    /// Devtools configuration for in-process monitoring.
    pub devtools: DevtoolsSettings,
}

impl ObservabilitySettings {
    /// Creates observability settings appropriate for the given environment.
    #[must_use]
    pub fn for_environment(env: Environment) -> Self {
        let is_deployed = env.is_deployed();
        Self {
            log_level: env.default_log_level().to_string(),
            // Not environment-derived: `#[instrument]` is written at `info` everywhere.
            span_level: "info".to_string(),
            log_format: env.default_log_format().to_string(),
            tracing_enabled: is_deployed,
            tracing_endpoint: None,
            service_name: None,
            otlp: OtlpSettings {
                enabled: is_deployed,
                ..Default::default()
            },
            prometheus: PrometheusSettings {
                enabled: is_deployed,
                ..Default::default()
            },
            sampling: SamplingSettings {
                ratio: if is_deployed { 0.1 } else { 1.0 },
                always_sample_errors: true,
            },
            devtools: DevtoolsSettings {
                enabled: !is_deployed,
                port: 0,
                api_key: None,
            },
        }
    }
}

impl Default for ObservabilitySettings {
    fn default() -> Self {
        Self::for_environment(Environment::default())
    }
}
