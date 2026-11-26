//! SSR server settings ([`ServerSettings`]).

use super::TlsPosture;
use serde::{Deserialize, Serialize};

/// SSR server configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    /// Host to bind to.
    pub host: String,
    /// Port to listen on.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub port: u16,
    /// How this listener's transport security is arranged; accepts the legacy `tls_enabled`.
    #[serde(alias = "tls_enabled")]
    pub tls: TlsPosture,
    /// Path to TLS certificate file.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_cert_path: Option<String>,
    /// Path to TLS key file.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_key_path: Option<String>,
    /// Request timeout in seconds.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub request_timeout_secs: u64,
    /// Maximum request body size in bytes.
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub max_body_size: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 11001,
            tls: TlsPosture::default(),
            tls_cert_path: None,
            tls_key_path: None,
            request_timeout_secs: 30,
            max_body_size: 10 * 1024 * 1024, // 10 MB
        }
    }
}
