//! TLS configuration types for database connections, free of crypto dependencies.

/// TLS mode for database connections.
///
/// [`Disabled`](Self::Disabled) exists in every profile for [`TlsConfig::terminated_upstream`];
/// [`Preferred`](Self::Preferred) is debug-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// No TLS on this hop; in release builds only through [`TlsConfig::terminated_upstream`].
    Disabled,

    /// TLS is preferred but not required.
    #[cfg(debug_assertions)]
    Preferred,

    /// TLS is required (default).
    #[default]
    Required,

    /// TLS is required with certificate verification.
    VerifyFull,

    /// TLS is required with CA verification.
    VerifyCa,
}

impl TlsMode {
    /// Returns whether TLS is enabled for this mode.
    pub fn is_enabled(&self) -> bool {
        match self {
            // Not debug-gated: a release build must never report a plaintext hop as encrypted.
            Self::Disabled => false,
            _ => true,
        }
    }

    /// Returns whether certificate verification is enabled.
    pub fn verifies_certificate(&self) -> bool {
        matches!(self, Self::VerifyFull | Self::VerifyCa)
    }
}

/// TLS configuration for database connections.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// The TLS mode.
    pub mode: TlsMode,

    /// Path to the CA certificate file (PEM format).
    pub ca_cert_path: Option<String>,

    /// PEM-encoded CA certificate content.
    pub ca_cert_pem: Option<String>,

    /// Path to the client certificate file (PEM format), for mTLS.
    pub client_cert_path: Option<String>,

    /// PEM-encoded client certificate content.
    pub client_cert_pem: Option<String>,

    /// Path to the client private key file (PEM format), for mTLS.
    pub client_key_path: Option<String>,

    /// PEM-encoded client private key content.
    pub client_key_pem: Option<String>,

    /// The name the server's certificate is verified against, instead of the address dialled.
    ///
    /// For clusters whose node addresses are not stable identities; it is not SNI and does not
    /// relax the chain checks. Honoured by the YCQL adapter.
    pub server_name: Option<String>,

    /// Whether to accept invalid certificates (development only; never in production).
    #[cfg(debug_assertions)]
    pub accept_invalid_certs: bool,
}

impl TlsConfig {
    /// Creates a new TLS configuration with the default mode (Required).
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a TLS configuration that requires TLS but doesn't verify certificates (development only).
    #[cfg(debug_assertions)]
    pub fn required_insecure() -> Self {
        Self {
            mode: TlsMode::Required,
            accept_invalid_certs: true,
            ..Default::default()
        }
    }

    /// Creates a TLS configuration that disables TLS entirely (debug builds only).
    #[cfg(debug_assertions)]
    pub fn disabled() -> Self {
        Self {
            mode: TlsMode::Disabled,
            ..Default::default()
        }
    }

    /// Creates a TLS configuration for a hop encrypted by something in front of this driver,
    /// such as a sidecar proxy on loopback.
    ///
    /// The only way to a plaintext connection in a release build, for
    /// [`TlsPosture::TerminatedUpstream`].
    ///
    /// [`TlsPosture::TerminatedUpstream`]: https://docs.rs/fundamental_config
    pub fn terminated_upstream() -> Self {
        Self {
            mode: TlsMode::Disabled,
            ..Default::default()
        }
    }

    /// Creates a TLS configuration with full verification.
    pub fn verify_full(ca_cert_path: impl Into<String>) -> Self {
        Self {
            mode: TlsMode::VerifyFull,
            ca_cert_path: Some(ca_cert_path.into()),
            ..Default::default()
        }
    }

    /// Creates a TLS configuration with full verification using PEM content.
    pub fn verify_full_pem(ca_cert_pem: impl Into<String>) -> Self {
        Self {
            mode: TlsMode::VerifyFull,
            ca_cert_pem: Some(ca_cert_pem.into()),
            ..Default::default()
        }
    }

    /// Sets the TLS mode.
    pub fn mode(mut self, mode: TlsMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets the CA certificate path.
    pub fn ca_cert_path(mut self, path: impl Into<String>) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }

    /// Sets the CA certificate PEM content.
    pub fn ca_cert_pem(mut self, pem: impl Into<String>) -> Self {
        self.ca_cert_pem = Some(pem.into());
        self
    }

    /// Sets the client certificate for mTLS.
    pub fn client_cert(
        mut self,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
    ) -> Self {
        self.client_cert_path = Some(cert_path.into());
        self.client_key_path = Some(key_path.into());
        self
    }

    /// Sets the client certificate PEM content for mTLS.
    pub fn client_cert_pem(
        mut self,
        cert_pem: impl Into<String>,
        key_pem: impl Into<String>,
    ) -> Self {
        self.client_cert_pem = Some(cert_pem.into());
        self.client_key_pem = Some(key_pem.into());
        self
    }

    /// Sets the server name for SNI.
    pub fn server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Returns whether this configuration has CA certificate information.
    pub fn has_ca_cert(&self) -> bool {
        self.ca_cert_path.is_some() || self.ca_cert_pem.is_some()
    }

    /// Returns whether this configuration has client certificate information.
    pub fn has_client_cert(&self) -> bool {
        (self.client_cert_path.is_some() || self.client_cert_pem.is_some())
            && (self.client_key_path.is_some() || self.client_key_pem.is_some())
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            mode: TlsMode::Required,
            ca_cert_path: None,
            ca_cert_pem: None,
            client_cert_path: None,
            client_cert_pem: None,
            client_key_path: None,
            client_key_pem: None,
            server_name: None,
            #[cfg(debug_assertions)]
            accept_invalid_certs: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_mode_default() {
        assert_eq!(TlsMode::default(), TlsMode::Required);
    }

    #[test]
    fn test_tls_mode_is_enabled() {
        assert!(TlsMode::Required.is_enabled());
        assert!(TlsMode::VerifyFull.is_enabled());
        assert!(TlsMode::VerifyCa.is_enabled());

        #[cfg(debug_assertions)]
        {
            assert!(!TlsMode::Disabled.is_enabled());
            assert!(TlsMode::Preferred.is_enabled());
        }
    }

    #[test]
    fn test_tls_config_builder() {
        let config = TlsConfig::new()
            .mode(TlsMode::VerifyFull)
            .ca_cert_path("/path/to/ca.pem")
            .server_name("db.example.com");

        assert_eq!(config.mode, TlsMode::VerifyFull);
        assert_eq!(config.ca_cert_path, Some("/path/to/ca.pem".to_string()));
        assert_eq!(config.server_name, Some("db.example.com".to_string()));
    }
}
