//! OTLP export settings ([`OtlpSettings`]).

use serde::{Deserialize, Serialize};

/// OTLP (OpenTelemetry Protocol) export configuration; `grpc` needs the `otlp-grpc` feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OtlpSettings {
    /// Whether OTLP export is enabled.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub enabled: bool,
    /// The collector's base URL; defaults to `OTEL_EXPORTER_OTLP_ENDPOINT`, else the protocol's default.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub endpoint: Option<String>,
    /// The transport: `"http/protobuf"` (also spelled `"http"`), or `"grpc"`.
    pub protocol: String,
    /// Timeout in seconds for export operations.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub timeout_secs: u64,
}

impl Default for OtlpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            protocol: "http/protobuf".to_string(),
            timeout_secs: 10,
        }
    }
}

impl OtlpSettings {
    /// Returns the effective endpoint, checking environment variables.
    #[must_use]
    pub fn effective_endpoint(&self) -> String {
        self.endpoint
            .clone()
            .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
            .unwrap_or_else(|| self.default_endpoint().to_owned())
    }

    /// The collector's local address for the protocol: port 4317 for gRPC, 4318 for HTTP.
    #[must_use]
    pub fn default_endpoint(&self) -> &'static str {
        if self.protocol.eq_ignore_ascii_case("grpc") {
            "http://localhost:4317"
        } else {
            "http://localhost:4318"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OtlpSettings;

    #[test]
    fn the_default_export_is_http_protobuf_at_the_collectors_http_port() {
        let settings = OtlpSettings::default();

        assert_eq!(settings.protocol, "http/protobuf");
        assert_eq!(settings.default_endpoint(), "http://localhost:4318");
    }

    #[test]
    fn a_grpc_export_defaults_to_the_collectors_grpc_port() {
        let settings = OtlpSettings {
            protocol: "grpc".to_owned(),
            ..OtlpSettings::default()
        };

        assert_eq!(settings.default_endpoint(), "http://localhost:4317");
    }
}
