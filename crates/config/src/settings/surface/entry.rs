//! Per-surface entry settings ([`SurfaceEntryConfig`]).

use super::ListenMode;
use crate::settings::TlsPosture;
use serde::{Deserialize, Serialize};

/// Configuration for a single surface within an app; its runtime fields merge into the typed settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceEntryConfig {
    /// Whether this surface is enabled (default: true).
    #[serde(deserialize_with = "crate::settings::scalar_or_string")]
    pub enabled: bool,
    /// Port override.
    #[serde(deserialize_with = "crate::settings::optional_scalar_or_string")]
    pub port: Option<u16>,
    /// Directory override (relative to project root).
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub dir: Option<String>,
    /// Cargo package name override (for API surfaces).
    pub package: Option<String>,
    /// Host to bind to.
    pub host: Option<String>,
    /// Where the listener comes from: its own address (default) or a passed socket.
    pub listen: ListenMode,
    /// How this surface's transport security is arranged; accepts the legacy `tls_enabled`.
    #[serde(alias = "tls_enabled")]
    pub tls: Option<TlsPosture>,
    /// Path to TLS certificate file.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_cert_path: Option<String>,
    /// Path to TLS key file.
    #[serde(deserialize_with = "crate::settings::empty_as_none")]
    pub tls_key_path: Option<String>,
    /// Request timeout in seconds.
    #[serde(deserialize_with = "crate::settings::optional_scalar_or_string")]
    pub request_timeout_secs: Option<u64>,
    /// Maximum request body size in bytes.
    #[serde(deserialize_with = "crate::settings::optional_scalar_or_string")]
    pub max_body_size: Option<usize>,
    /// CORS allowed origins (API surfaces only).
    pub cors_allowed_origins: Option<Vec<String>>,
}

impl Default for SurfaceEntryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: None,
            dir: None,
            package: None,
            host: None,
            listen: ListenMode::Tcp,
            tls: None,
            tls_cert_path: None,
            tls_key_path: None,
            request_timeout_secs: None,
            max_body_size: None,
            cors_allowed_origins: None,
        }
    }
}
