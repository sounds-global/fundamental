use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// TLS configuration for a NATS connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsTlsConfig {
    /// Whether TLS is required.
    #[serde(default = "default_true")]
    pub required: bool,
    /// Path to the CA certificate.
    #[serde(default)]
    pub ca_cert: Option<PathBuf>,
    /// Path to the client certificate (for mutual TLS).
    #[serde(default)]
    pub client_cert: Option<PathBuf>,
    /// Path to the client private key (for mutual TLS).
    #[serde(default)]
    pub client_key: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl Default for NatsTlsConfig {
    fn default() -> Self {
        Self {
            required: true,
            ca_cert: None,
            client_cert: None,
            client_key: None,
        }
    }
}

impl NatsTlsConfig {
    /// Creates a configuration requiring TLS with no client certificate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the CA certificate path.
    #[must_use]
    pub fn with_ca_cert(mut self, path: impl Into<PathBuf>) -> Self {
        self.ca_cert = Some(path.into());
        self
    }

    /// Sets the client certificate and key paths for mutual TLS.
    #[must_use]
    pub fn with_client_cert(mut self, cert: impl Into<PathBuf>, key: impl Into<PathBuf>) -> Self {
        self.client_cert = Some(cert.into());
        self.client_key = Some(key.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_require_tls() {
        let tls = NatsTlsConfig::new();
        assert!(tls.required);
        assert!(tls.ca_cert.is_none());
    }

    #[test]
    fn builders_set_paths() {
        let tls = NatsTlsConfig::new()
            .with_ca_cert("/ca.pem")
            .with_client_cert("/cert.pem", "/key.pem");
        assert_eq!(tls.ca_cert.unwrap().to_str(), Some("/ca.pem"));
        assert_eq!(tls.client_cert.unwrap().to_str(), Some("/cert.pem"));
        assert_eq!(tls.client_key.unwrap().to_str(), Some("/key.pem"));
    }
}
