//! [`TokenIssuer`] — signs short-lived JWTs that a [`JwtVerifier`] accepts.

use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fundamental_config::settings::AuthSettings;
use fundamental_core::secrets::SecretString;
use fundamental_keys::DeviceId;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use tracing::{info, instrument};

use crate::AuthError;

/// Signs JWTs from a surface's RSA private key; the sign-side counterpart of
/// [`JwtVerifier`](crate::JwtVerifier).
pub struct TokenIssuer {
    encoding_key: EncodingKey,
    issuer: Option<String>,
    audience: Option<String>,
    key_id: Option<String>,
    default_ttl: Duration,
}

impl std::fmt::Debug for TokenIssuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenIssuer")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("key_id", &self.key_id)
            .field("default_ttl", &self.default_ttl)
            .finish_non_exhaustive()
    }
}

/// The claims a [`TokenIssuer`] writes.
#[derive(Serialize)]
struct IssuedClaims {
    sub: String,
    exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    scopes: Vec<String>,
}

impl TokenIssuer {
    /// Builds an issuer from [`AuthSettings`], reading the RSA private key from
    /// `token_private_key_file`.
    ///
    /// # Errors
    /// Returns [`AuthError::Config`] if `token_private_key_file` is unset, unreadable,
    /// or not a valid RSA private key.
    pub fn from_settings(settings: &AuthSettings) -> Result<Self, AuthError> {
        let path = settings.token_private_key_file.as_deref().ok_or_else(|| {
            AuthError::Config("no token_private_key_file configured for signing".to_string())
        })?;
        let pem = fs::read(path)
            .map_err(|e| AuthError::Config(format!("reading private key `{path}`: {e}")))?;
        let encoding_key = EncodingKey::from_rsa_pem(&pem)
            .map_err(|e| AuthError::Config(format!("parsing RSA private key `{path}`: {e}")))?;
        Ok(Self {
            encoding_key,
            issuer: settings.issuer.clone(),
            audience: settings.audience.clone(),
            key_id: settings.token_key_id.clone(),
            default_ttl: Duration::from_secs(settings.access_token_ttl),
        })
    }

    /// Issues a signed JWT for `sub` granting `scopes`, expiring after `ttl` (default: the configured
    /// `access_token_ttl`).
    ///
    /// # Errors
    /// Returns [`AuthError::Signing`] if the system clock is before the Unix epoch,
    /// the expiry overflows, or the token cannot be signed.
    #[instrument(name = "auth.issue", skip_all, fields(sub = %sub), err(Debug))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn issue(
        &self,
        sub: &str,
        scopes: &[String],
        device_id: Option<&DeviceId>,
        ttl: Option<Duration>,
    ) -> Result<SecretString, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AuthError::Signing(format!("system clock before Unix epoch: {e}")))?
            .as_secs();
        let ttl = ttl.unwrap_or(self.default_ttl);
        let exp = i64::try_from(now.saturating_add(ttl.as_secs()))
            .map_err(|e| AuthError::Signing(format!("token expiry out of range: {e}")))?;

        let claims = IssuedClaims {
            sub: sub.to_string(),
            exp,
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            device_id: device_id.map(|d| d.as_str().to_string()),
            scopes: scopes.to_vec(),
        };

        let mut header = Header::new(Algorithm::RS256);
        header.kid = self.key_id.clone();

        let token = encode(&header, &claims, &self.encoding_key)
            .map_err(|e| AuthError::Signing(format!("signing token: {e}")))?;
        info!("token issued");
        Ok(SecretString::new(token))
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use tempfile::TempDir;

    use super::*;
    use crate::test_keys::{RSA_PRIVATE_PEM, RSA_PUBLIC_PEM};
    use crate::{InvalidToken, JwtVerifier, PrincipalVerifier, TrustedIssuer};

    fn settings_with_keys(dir: &TempDir, issuer: Option<&str>) -> AuthSettings {
        let priv_path = dir.path().join("private.pem");
        let pub_path = dir.path().join("public.pem");
        fs::write(&priv_path, RSA_PRIVATE_PEM).expect("write private key");
        fs::write(&pub_path, RSA_PUBLIC_PEM).expect("write public key");
        AuthSettings {
            token_private_key_file: Some(priv_path.to_string_lossy().into_owned()),
            token_public_key_file: Some(pub_path.to_string_lossy().into_owned()),
            issuer: issuer.map(str::to_string),
            token_key_id: Some("test-1".to_string()),
            ..AuthSettings::default()
        }
    }

    #[tokio::test]
    async fn a_token_minted_for_one_audience_is_refused_by_another() {
        let dir = TempDir::new().expect("temp dir");
        let minted_for = AuthSettings {
            audience: Some("https://layer0.example".to_owned()),
            ..settings_with_keys(&dir, Some("https://operator.example"))
        };
        let token = TokenIssuer::from_settings(&minted_for)
            .expect("issuer builds")
            .issue("operator", &["federation:accredit".to_owned()], None, None)
            .expect("token issued");

        let intended = JwtVerifier::from_settings(&minted_for)
            .await
            .expect("verifier");
        assert!(intended.verify(&token).await.is_ok());

        let elsewhere = AuthSettings {
            audience: Some("https://layer5.example".to_owned()),
            ..minted_for.clone()
        };
        let refused = JwtVerifier::from_settings(&elsewhere)
            .await
            .expect("verifier")
            .verify(&token)
            .await
            .expect_err("a token for layer0 is not a token for layer5");
        assert!(matches!(
            refused,
            AuthError::Invalid(InvalidToken::WrongAudience)
        ));
    }

    #[tokio::test]
    async fn an_audience_bound_verifier_refuses_a_token_that_names_none() {
        let dir = TempDir::new().expect("temp dir");
        let unbound = settings_with_keys(&dir, Some("https://operator.example"));
        let token = TokenIssuer::from_settings(&unbound)
            .expect("issuer builds")
            .issue("operator", &[], None, None)
            .expect("token issued");
        let bound = AuthSettings {
            audience: Some("https://layer0.example".to_owned()),
            ..unbound
        };
        assert!(
            JwtVerifier::from_settings(&bound)
                .await
                .expect("verifier")
                .verify(&token)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn an_issuer_built_from_settings_is_trusted_up_to_its_ceiling() {
        use crate::{MultiIssuerVerifier, TrustedIssuer};

        let dir = TempDir::new().expect("temp dir");
        let settings = AuthSettings {
            audience: Some("https://layer0.example".to_owned()),
            ..settings_with_keys(&dir, Some("https://operator.example"))
        };
        let issuer = TokenIssuer::from_settings(&settings).expect("issuer builds");
        let verifier = MultiIssuerVerifier::new([TrustedIssuer::from_auth_settings(
            &settings,
            ["federation:accredit"],
        )
        .expect("trusted issuer builds")])
        .expect("verifier builds");

        let within = issuer
            .issue("operator", &["federation:accredit".to_owned()], None, None)
            .expect("token");
        assert_eq!(
            verifier
                .verify(&within)
                .await
                .expect("within the ceiling")
                .scopes,
            ["federation:accredit"]
        );
        let beyond = issuer
            .issue("operator", &["subject:erase".to_owned()], None, None)
            .expect("token");
        assert!(matches!(
            verifier.verify(&beyond).await,
            Err(AuthError::ScopeBeyondIssuerCeiling { .. })
        ));
    }

    #[test]
    fn a_trusted_issuer_without_an_audience_does_not_build() {
        let dir = TempDir::new().expect("temp dir");
        let settings = settings_with_keys(&dir, Some("https://operator.example"));
        assert!(matches!(
            TrustedIssuer::from_auth_settings(&settings, ["federation:accredit"]),
            Err(AuthError::Config(_))
        ));
    }

    #[tokio::test]
    async fn issued_token_verifies_and_carries_its_claims() {
        let dir = TempDir::new().expect("temp dir");
        let settings = settings_with_keys(&dir, Some("todo-test"));

        let issuer = TokenIssuer::from_settings(&settings).expect("issuer builds");
        let token = issuer
            .issue(
                "demo-user",
                &["sync:read".to_string()],
                Some(&DeviceId::new("dev-1")),
                Some(Duration::from_secs(300)),
            )
            .expect("token issued");

        let verifier = JwtVerifier::from_settings(&settings)
            .await
            .expect("verifier builds");
        let principal = verifier.verify(&token).await.expect("token verifies");

        assert_eq!(principal.user_id.as_str(), "demo-user");
        assert_eq!(principal.scopes, vec!["sync:read".to_string()]);
        assert_eq!(
            principal.device_id.as_ref().map(DeviceId::as_str),
            Some("dev-1")
        );
        assert!(principal.expires_at > 0);
    }

    #[tokio::test]
    async fn a_mismatched_issuer_is_rejected() {
        let dir = TempDir::new().expect("temp dir");
        let signing = settings_with_keys(&dir, Some("issuer-a"));
        let verifying = settings_with_keys(&dir, Some("issuer-b"));

        let token = TokenIssuer::from_settings(&signing)
            .expect("issuer builds")
            .issue("demo-user", &[], None, None)
            .expect("token issued");

        let err = JwtVerifier::from_settings(&verifying)
            .await
            .expect("verifier builds")
            .verify(&token)
            .await
            .expect_err("mismatched issuer must be rejected");

        assert!(matches!(err, AuthError::Untrusted(_)));
        assert_eq!(err.error_code(), "AUTH_004");
        assert_eq!(err.http_status_code(), 403);
    }

    #[test]
    fn a_missing_private_key_is_a_config_error() {
        let settings = AuthSettings::default();
        let err = TokenIssuer::from_settings(&settings).expect_err("must fail without a key");
        assert!(matches!(err, AuthError::Config(_)));
        assert_eq!(err.error_code(), "AUTH_005");
    }
}
