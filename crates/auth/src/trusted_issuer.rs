//! [`TrustedIssuer`] — one party a [`MultiIssuerVerifier`](crate::MultiIssuerVerifier) accepts tokens from.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;

use fundamental_config::settings::AuthSettings;
use jsonwebtoken::{Algorithm, DecodingKey};

use crate::AuthError;

/// A party whose tokens a service accepts, bounded by its key, algorithm, audience and scope ceiling.
pub struct TrustedIssuer {
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) key: DecodingKey,
    pub(crate) algorithm: Algorithm,
    pub(crate) scope_ceiling: BTreeSet<String>,
}

impl TrustedIssuer {
    /// An issuer identified by `issuer` (`iss`), signing for `audience` (`aud`) with `key`
    /// under `algorithm`, trusted to grant at most `scope_ceiling`.
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        key: DecodingKey,
        algorithm: Algorithm,
        scope_ceiling: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audience: audience.into(),
            key,
            algorithm,
            scope_ceiling: scope_ceiling.into_iter().map(Into::into).collect(),
        }
    }

    /// An RS256 issuer described by a surface's [`AuthSettings`], trusted to grant at most
    /// `scope_ceiling`.
    ///
    /// # Errors
    /// [`AuthError::Config`] when the settings name no `issuer`, no `audience` or no
    /// `token_public_key_file`, or the key cannot be read as an RSA public key.
    pub fn from_auth_settings(
        settings: &AuthSettings,
        scope_ceiling: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, AuthError> {
        let required = |value: &Option<String>, name: &str| {
            value
                .clone()
                .ok_or_else(|| AuthError::Config(format!("a trusted issuer needs `{name}`")))
        };
        let issuer = required(&settings.issuer, "issuer")?;
        let audience = required(&settings.audience, "audience")?;
        let path = required(&settings.token_public_key_file, "token_public_key_file")?;
        let pem = fs::read(&path)
            .map_err(|e| AuthError::Config(format!("reading public key `{path}`: {e}")))?;
        let key = DecodingKey::from_rsa_pem(&pem)
            .map_err(|e| AuthError::Config(format!("parsing RSA public key `{path}`: {e}")))?;
        Ok(Self::new(
            issuer,
            audience,
            key,
            Algorithm::RS256,
            scope_ceiling,
        ))
    }

    /// The issuer identifier.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
}

impl fmt::Debug for TrustedIssuer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustedIssuer")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("algorithm", &self.algorithm)
            .field("scope_ceiling", &self.scope_ceiling)
            .finish_non_exhaustive()
    }
}
