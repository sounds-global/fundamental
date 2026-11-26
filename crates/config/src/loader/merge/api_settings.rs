//! Merges a surface entry's runtime overrides into [`ApiSettings`].

use crate::settings::SurfaceEntryConfig;

/// Applies surface entry overrides to API settings.
pub(crate) fn merge_api_settings(
    api: &mut crate::settings::ApiSettings,
    entry: &SurfaceEntryConfig,
) {
    if let Some(port) = entry.port {
        api.port = port;
    }
    if let Some(ref host) = entry.host {
        api.host = host.clone();
    }
    if let Some(tls) = entry.tls {
        api.tls = tls;
    }
    if let Some(ref path) = entry.tls_cert_path {
        api.tls_cert_path = Some(path.clone());
    }
    if let Some(ref path) = entry.tls_key_path {
        api.tls_key_path = Some(path.clone());
    }
    if let Some(timeout) = entry.request_timeout_secs {
        api.request_timeout_secs = timeout;
    }
    if let Some(size) = entry.max_body_size {
        api.max_body_size = size;
    }
    if let Some(ref origins) = entry.cors_allowed_origins {
        api.cors_allowed_origins = origins.clone();
    }
}
