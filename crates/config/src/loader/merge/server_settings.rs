//! Merges a surface entry's runtime overrides into [`ServerSettings`].

use crate::settings::SurfaceEntryConfig;

/// Applies surface entry overrides to server (SSR) settings.
pub(crate) fn merge_server_settings(
    server: &mut crate::settings::ServerSettings,
    entry: &SurfaceEntryConfig,
) {
    if let Some(port) = entry.port {
        server.port = port;
    }
    if let Some(ref host) = entry.host {
        server.host = host.clone();
    }
    if let Some(tls) = entry.tls {
        server.tls = tls;
    }
    if let Some(ref path) = entry.tls_cert_path {
        server.tls_cert_path = Some(path.clone());
    }
    if let Some(ref path) = entry.tls_key_path {
        server.tls_key_path = Some(path.clone());
    }
    if let Some(timeout) = entry.request_timeout_secs {
        server.request_timeout_secs = timeout;
    }
    if let Some(size) = entry.max_body_size {
        server.max_body_size = size;
    }
}
