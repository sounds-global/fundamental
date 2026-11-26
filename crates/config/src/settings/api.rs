//! API server configuration ([`ApiSettings`]).

use super::TlsPosture;
use serde::{Deserialize, Serialize};

/// API server configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiSettings {
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
    /// CORS allowed origins.
    pub cors_allowed_origins: Vec<String>,
    /// CORS allowed request headers; the defaults include the RFC 9449 `DPoP` proof.
    pub cors_allowed_headers: Vec<String>,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 16001,
            tls: TlsPosture::default(),
            tls_cert_path: None,
            tls_key_path: None,
            request_timeout_secs: 30,
            max_body_size: 10 * 1024 * 1024, // 10 MB
            cors_allowed_origins: vec![
                "http://127.0.0.1:12001".to_string(),
                "http://localhost:12001".to_string(),
                "http://127.0.0.1:11001".to_string(),
                "http://localhost:11001".to_string(),
                "http://127.0.0.1:13001".to_string(),
                "http://localhost:13001".to_string(),
                "tauri://localhost".to_string(),
                "https://tauri.localhost".to_string(),
            ],
            cors_allowed_headers: vec![
                "content-type".to_string(),
                "authorization".to_string(),
                "dpop".to_string(),
            ],
        }
    }
}

impl ApiSettings {
    /// The URL of this socket's own bind address: `http` behind a TLS-terminating ingress.
    #[must_use]
    pub fn url(&self) -> String {
        let scheme = if self.tls.terminates_tls() {
            "https"
        } else {
            "http"
        };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }
}
