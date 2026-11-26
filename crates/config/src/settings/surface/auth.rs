//! Authentication settings ([`AuthSettings`]).

use super::AuthProviderSettings;

use serde::{Deserialize, Serialize};

/// Authentication configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthSettings {
    /// Whether authentication is enabled.
    pub enabled: bool,
    /// Path to RSA private key PEM file for JWT signing.
    pub token_private_key_file: Option<String>,
    /// Path to RSA public key PEM file for JWT verification.
    pub token_public_key_file: Option<String>,
    /// Path to pre-computed JWKS JSON file.
    pub token_jwks_file: Option<String>,
    /// URL to fetch the JWKS from, for validation-only mode; `https` unless on a loopback host.
    pub token_jwks_url: Option<String>,
    /// JWKS key ID (`kid` in JWT header).
    pub token_key_id: Option<String>,
    /// How often (seconds) to re-fetch a remote JWKS, when `token_jwks_url` is set.
    pub token_jwks_refresh_secs: u64,
    /// The least time (seconds) between two fetches caused by an unknown `kid`.
    pub token_jwks_min_refetch_secs: u64,
    /// JWT issuer (`iss` claim).
    pub issuer: Option<String>,
    /// The service a token is for (`aud` claim); a verifier requires it when set.
    pub audience: Option<String>,
    /// Access token time-to-live in seconds.
    pub access_token_ttl: u64,
    /// Refresh token time-to-live in seconds.
    pub refresh_token_ttl: u64,
    /// Auth provider configuration (Kratos, Hydra, Keto, Oathkeeper).
    #[serde(default)]
    pub providers: AuthProviderSettings,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            token_private_key_file: None,
            token_public_key_file: None,
            token_jwks_file: None,
            token_jwks_url: None,
            token_key_id: None,
            token_jwks_refresh_secs: 3600, // 1 hour
            token_jwks_min_refetch_secs: 60,
            issuer: None,
            audience: None,
            access_token_ttl: 900,      // 15 minutes
            refresh_token_ttl: 604_800, // 7 days
            providers: AuthProviderSettings::default(),
        }
    }
}
