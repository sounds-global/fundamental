//! [`JwtVerifier`] — a [`PrincipalVerifier`] that verifies JWTs against configured keys.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use fundamental_config::settings::AuthSettings;
use fundamental_core::secrets::SecretString;
use fundamental_keys::DeviceId;
use jsonwebtoken::errors::{Error as JwtError, ErrorKind};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tracing::{Span, field, info, instrument};

use crate::claim_pins::ClaimPins;
use crate::jwks_keys::JwksKeys;
use crate::jwks_snapshot::JwksSnapshot;
use crate::record_refusal::record_refusal;
use crate::{
    AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier, UntrustedToken, UserId,
};

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
    /// The device the token was issued for.
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
}

/// Where a [`JwtVerifier`] gets its decoding keys.
enum KeySource {
    /// A single key with a fixed validation policy; boxed to keep the enum small.
    Static {
        key: DecodingKey,
        validation: Box<Validation>,
    },
    /// A JWKS file, read once, keyed by `kid`.
    File(JwksSnapshot),
    /// A remote JWKS, refreshed in the background, keyed by `kid`.
    Jwks(Arc<JwksKeys>),
}

/// Verifies a JWT against a configured key or JWKS and maps its claims to an
/// [`AuthenticatedPrincipal`].
pub struct JwtVerifier {
    key_source: KeySource,
}

impl JwtVerifier {
    /// Builds a verifier directly from a decoding key and validation policy.
    pub fn new(decoding_key: DecodingKey, validation: Validation) -> Self {
        Self {
            key_source: KeySource::Static {
                key: decoding_key,
                validation: Box::new(validation),
            },
        }
    }

    /// Builds a verifier from [`AuthSettings`]: a JWKS file, an RSA public-key PEM, or a refreshed
    /// remote JWKS, pinned to the configured issuer.
    ///
    /// # Errors
    /// Returns [`AuthError::Config`] if no verification key is configured, a local key
    /// cannot be read, or a JWKS file holds no usable key (or not the one `token_key_id`
    /// names); or [`AuthError::Jwks`] if a remote JWKS cannot be fetched.
    pub async fn from_settings(settings: &AuthSettings) -> Result<Self, AuthError> {
        if let Some(path) = &settings.token_jwks_file {
            return Self::from_jwks_file(path, settings);
        }
        if let Some(path) = &settings.token_public_key_file {
            return Self::from_rsa_pem_file(path, settings);
        }
        if let Some(url) = &settings.token_jwks_url {
            let keys = JwksKeys::connect(
                url,
                ClaimPins::from_settings(settings),
                Duration::from_secs(settings.token_jwks_refresh_secs),
                Duration::from_secs(settings.token_jwks_min_refetch_secs),
            )
            .await?;
            return Ok(Self {
                key_source: KeySource::Jwks(keys),
            });
        }
        Err(AuthError::Config(
            "no token_jwks_file, token_public_key_file, or token_jwks_url configured".to_string(),
        ))
    }

    fn from_jwks_file(path: &str, settings: &AuthSettings) -> Result<Self, AuthError> {
        let json = fs::read_to_string(path)
            .map_err(|e| AuthError::Config(format!("reading JWKS file `{path}`: {e}")))?;
        let snapshot = JwksSnapshot::parse(&json, &ClaimPins::from_settings(settings), path)
            .map_err(|error| match error {
                AuthError::Jwks(reason) => {
                    AuthError::Config(format!("JWKS file `{path}`: {reason}"))
                }
                other => other,
            })?;
        let snapshot = match &settings.token_key_id {
            Some(kid) => snapshot.narrowed_to(kid).ok_or_else(|| {
                AuthError::Config(format!(
                    "JWKS file `{path}` has no usable key with kid `{kid}`"
                ))
            })?,
            None => snapshot,
        };
        info!(path, keys = snapshot.len(), "loaded JWKS file");
        Ok(Self {
            key_source: KeySource::File(snapshot),
        })
    }

    fn from_rsa_pem_file(path: &str, settings: &AuthSettings) -> Result<Self, AuthError> {
        let pem = fs::read(path)
            .map_err(|e| AuthError::Config(format!("reading public key `{path}`: {e}")))?;
        let decoding_key = DecodingKey::from_rsa_pem(&pem)
            .map_err(|e| AuthError::Config(format!("parsing RSA public key `{path}`: {e}")))?;
        Ok(Self::new(decoding_key, asymmetric_validation(settings)))
    }
}

impl PrincipalVerifier for JwtVerifier {
    #[instrument(name = "auth.verify", skip_all, fields(user_id = field::Empty, refetch = field::Empty))]
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        let verified = self.verify_token(token.expose()).await;
        if let Err(error) = &verified {
            record_refusal(error);
        }
        verified
    }
}

impl JwtVerifier {
    /// The token's own claims as `C`, verified exactly as [`verify`](PrincipalVerifier::verify)
    /// verifies a principal's.
    /// # Errors
    /// As [`verify`](PrincipalVerifier::verify); [`AuthError::Invalid`] also when the token
    /// lacks a claim `C` requires or holds one of another shape.
    #[instrument(name = "auth.verify_claims", skip_all, fields(refetch = field::Empty))]
    pub async fn verify_claims<C: DeserializeOwned>(
        &self,
        token: &SecretString,
    ) -> Result<C, AuthError> {
        let verified = self.decode_verified::<C>(token.expose()).await;
        if let Err(error) = &verified {
            record_refusal(error);
        }
        verified
    }

    async fn verify_token(&self, raw: &str) -> Result<AuthenticatedPrincipal, AuthError> {
        let claims = self.decode_verified::<Claims>(raw).await?;
        Span::current().record("user_id", claims.sub.as_str());
        info!("principal authenticated");
        Ok(AuthenticatedPrincipal {
            user_id: UserId::new(claims.sub),
            device_id: claims.device_id.map(DeviceId::new),
            scopes: claims.scopes,
            expires_at: claims.exp,
        })
    }

    async fn decode_verified<C: DeserializeOwned>(&self, raw: &str) -> Result<C, AuthError> {
        if raw.trim().is_empty() {
            return Err(AuthError::MissingToken);
        }
        let header =
            decode_header(raw).map_err(|_| AuthError::Invalid(InvalidToken::MalformedHeader))?;
        Ok(match &self.key_source {
            KeySource::Static { key, validation } => {
                decode::<C>(raw, key, validation)
                    .map_err(map_jwt_error)?
                    .claims
            }
            KeySource::File(snapshot) => {
                snapshot
                    .key_for(header.kid.as_deref())
                    .ok_or(AuthError::Invalid(InvalidToken::UnknownKey))?
                    .decode::<C>(raw)?
                    .claims
            }
            KeySource::Jwks(keys) => keys.decode::<C>(raw, header.kid.as_deref()).await?.claims,
        })
    }
}

/// An RSA-only validation policy: `jsonwebtoken` refuses a mixed-family algorithm list.
fn asymmetric_validation(settings: &AuthSettings) -> Validation {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.algorithms = vec![
        Algorithm::RS256,
        Algorithm::RS384,
        Algorithm::RS512,
        Algorithm::PS256,
        Algorithm::PS384,
        Algorithm::PS512,
    ];
    ClaimPins::from_settings(settings).apply(&mut validation);
    validation
}

/// Maps a `jsonwebtoken` error by its kind alone: its text can quote the token's bytes.
pub(crate) fn map_jwt_error(error: JwtError) -> AuthError {
    match error.kind() {
        ErrorKind::ExpiredSignature => AuthError::Expired,
        ErrorKind::InvalidIssuer => AuthError::Untrusted(UntrustedToken::IssuerNotAccepted),
        kind => {
            AuthError::Invalid(InvalidToken::from_kind(kind).unwrap_or(InvalidToken::Unverifiable))
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serde_json::json;

    use super::*;

    const SECRET: &[u8] = b"test-signing-secret";
    const FUTURE: i64 = 4_102_444_800; // 2100-01-01
    const PAST: i64 = 1_000_000_000; // 2001-09-09

    fn hs256_verifier(issuer: Option<&str>) -> JwtVerifier {
        let mut validation = Validation::new(Algorithm::HS256);
        if let Some(iss) = issuer {
            validation.set_issuer(&[iss]);
        }
        JwtVerifier::new(DecodingKey::from_secret(SECRET), validation)
    }

    fn mint(claims: serde_json::Value) -> SecretString {
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .expect("encode");
        SecretString::new(token)
    }

    #[tokio::test]
    async fn verifies_a_valid_token_into_a_principal() {
        let verifier = hs256_verifier(Some("trusted"));
        let token = mint(json!({
            "sub": "user-1", "exp": FUTURE, "iss": "trusted",
            "device_id": "device-9", "scopes": ["sync:read", "sync:write"]
        }));
        let principal = verifier.verify(&token).await.expect("verify");
        assert_eq!(principal.user_id.as_str(), "user-1");
        assert_eq!(
            principal.device_id.as_ref().map(|d| d.as_str()),
            Some("device-9")
        );
        assert!(principal.has_scope("sync:write"));
        assert_eq!(principal.expires_at, FUTURE);
    }

    #[tokio::test]
    async fn rejects_an_empty_token_as_missing() {
        let err = hs256_verifier(None)
            .verify(&SecretString::new(String::new()))
            .await
            .expect_err("must fail");
        assert!(matches!(err, AuthError::MissingToken));
        assert_eq!(err.error_code(), "AUTH_001");
    }

    #[tokio::test]
    async fn rejects_an_expired_token() {
        let verifier = hs256_verifier(None);
        let token = mint(json!({ "sub": "u", "exp": PAST }));
        let err = verifier.verify(&token).await.expect_err("must fail");
        assert!(matches!(err, AuthError::Expired));
        assert_eq!(err.http_status_code(), 401);
    }

    #[tokio::test]
    async fn rejects_a_wrong_issuer_as_untrusted() {
        let verifier = hs256_verifier(Some("trusted"));
        let token = mint(json!({ "sub": "u", "exp": FUTURE, "iss": "evil" }));
        let err = verifier.verify(&token).await.expect_err("must fail");
        assert!(matches!(
            err,
            AuthError::Untrusted(UntrustedToken::IssuerNotAccepted)
        ));
        assert_eq!(err.http_status_code(), 403);
    }

    #[tokio::test]
    async fn rejects_a_bad_signature_as_invalid() {
        let token = encode(
            &Header::new(Algorithm::HS256),
            &json!({ "sub": "u", "exp": FUTURE }),
            &EncodingKey::from_secret(b"other-secret"),
        )
        .expect("encode");
        let err = hs256_verifier(None)
            .verify(&SecretString::new(token))
            .await
            .expect_err("must fail");
        assert!(matches!(
            err,
            AuthError::Invalid(InvalidToken::BadSignature)
        ));
        assert_eq!(err.error_code(), "AUTH_002");
    }

    #[tokio::test]
    async fn a_token_without_a_device_or_scopes_still_verifies() {
        let verifier = hs256_verifier(None);
        let token = mint(json!({ "sub": "u", "exp": FUTURE }));
        let principal = verifier.verify(&token).await.expect("verify");
        assert!(principal.device_id.is_none());
        assert!(principal.scopes.is_empty());
    }

    /// A JWKS file publishing a P-256 key as `ec` and an RSA key as `rsa`, and the P-256 signer.
    fn jwks_file(dir: &tempfile::TempDir) -> (String, EncodingKey) {
        use crate::test_keys::{es256_key, public_jwk, rsa_key};

        let ec = es256_key(7).encoding;
        let ec_jwk = public_jwk(&ec, Algorithm::ES256, Some("ec"));
        let rsa_jwk = public_jwk(&rsa_key(), Algorithm::RS256, Some("rsa"));
        let path = dir.path().join("jwks.json");
        fs::write(&path, json!({ "keys": [ec_jwk, rsa_jwk] }).to_string()).expect("write jwks");
        (path.to_string_lossy().into_owned(), ec)
    }

    fn ec_token(signer: &EncodingKey, kid: &str) -> SecretString {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(kid.to_owned());
        SecretString::new(
            encode(&header, &json!({ "sub": "user-1", "exp": FUTURE }), signer).expect("encode"),
        )
    }

    #[tokio::test]
    async fn a_jwks_file_holding_an_ec_key_verifies_its_tokens() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (path, signer) = jwks_file(&dir);
        let verifier = JwtVerifier::from_settings(&AuthSettings {
            token_jwks_file: Some(path),
            ..AuthSettings::default()
        })
        .await
        .expect("verifier");

        let principal = verifier
            .verify(&ec_token(&signer, "ec"))
            .await
            .expect("verifies under the EC key");
        assert_eq!(principal.user_id.as_str(), "user-1");
        let refused = verifier
            .verify(&ec_token(&signer, "rsa"))
            .await
            .expect_err("an ES256 token is refused under the RSA key");
        assert!(matches!(
            refused,
            AuthError::Invalid(InvalidToken::AlgorithmNotAllowed)
        ));
    }

    #[tokio::test]
    async fn token_key_id_narrows_a_jwks_file_to_that_key() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (path, signer) = jwks_file(&dir);
        let narrowed = JwtVerifier::from_settings(&AuthSettings {
            token_jwks_file: Some(path.clone()),
            token_key_id: Some("rsa".to_owned()),
            ..AuthSettings::default()
        })
        .await
        .expect("verifier");
        let refused = narrowed
            .verify(&ec_token(&signer, "ec"))
            .await
            .expect_err("the EC key was narrowed away");
        assert!(matches!(
            refused,
            AuthError::Invalid(InvalidToken::UnknownKey)
        ));

        let missing = JwtVerifier::from_settings(&AuthSettings {
            token_jwks_file: Some(path),
            token_key_id: Some("absent".to_owned()),
            ..AuthSettings::default()
        })
        .await;
        assert!(matches!(missing, Err(AuthError::Config(ref m)) if m.contains("kid `absent`")));
    }

    #[tokio::test]
    async fn a_jwks_file_of_only_oct_keys_is_a_configuration_error() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().join("jwks.json");
        fs::write(
            &path,
            r#"{"keys":[{"kty":"oct","kid":"k1","alg":"HS256","k":"c2VjcmV0"}]}"#,
        )
        .expect("write jwks");
        let refused = JwtVerifier::from_settings(&AuthSettings {
            token_jwks_file: Some(path.to_string_lossy().into_owned()),
            ..AuthSettings::default()
        })
        .await;
        let Err(error) = refused else {
            panic!("a JWKS file of shared secrets must not load");
        };
        assert!(matches!(error, AuthError::Config(_)));
        assert_eq!(error.error_code(), "AUTH_005");
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct JobClaims {
        repository_id: String,
        sha: String,
    }

    #[tokio::test]
    async fn a_token_s_own_claims_verify_as_the_reader_s_type() {
        let verifier = hs256_verifier(Some("https://token.actions.githubusercontent.com"));
        let token = mint(json!({
            "sub": "repo:acme-corp/shop:ref:refs/heads/main",
            "iss": "https://token.actions.githubusercontent.com",
            "exp": FUTURE,
            "repository_id": "812345",
            "sha": "457b1258c0ffee",
        }));

        let claims: JobClaims = verifier.verify_claims(&token).await.expect("verifies");

        assert_eq!(
            claims,
            JobClaims {
                repository_id: "812345".to_owned(),
                sha: "457b1258c0ffee".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn typed_claims_are_refused_as_a_principal_is() {
        let issuer = "https://token.actions.githubusercontent.com";
        let verifier = hs256_verifier(Some(issuer));
        let job = |exp: i64, iss: &str| json!({ "iss": iss, "exp": exp, "repository_id": "812345", "sha": "457b125" });

        let expired = verifier
            .verify_claims::<JobClaims>(&mint(job(PAST, issuer)))
            .await
            .expect_err("expired");
        assert!(matches!(expired, AuthError::Expired), "{expired:?}");

        let foreign = verifier
            .verify_claims::<JobClaims>(&mint(job(FUTURE, "https://elsewhere.example")))
            .await
            .expect_err("another issuer");
        assert!(matches!(foreign, AuthError::Untrusted(_)), "{foreign:?}");

        let forged = SecretString::new(
            encode(
                &Header::new(Algorithm::HS256),
                &job(FUTURE, issuer),
                &EncodingKey::from_secret(b"another-secret"),
            )
            .expect("encode"),
        );
        let forged = verifier
            .verify_claims::<JobClaims>(&forged)
            .await
            .expect_err("forged");
        assert!(matches!(forged, AuthError::Invalid(_)), "{forged:?}");

        let lacking = verifier
            .verify_claims::<JobClaims>(&mint(
                json!({ "iss": issuer, "exp": FUTURE, "sha": "457b125" }),
            ))
            .await
            .expect_err("no repository_id");
        assert!(matches!(lacking, AuthError::Invalid(_)), "{lacking:?}");
        assert_eq!(
            lacking.error_code(),
            AuthError::Invalid(InvalidToken::Unverifiable).error_code()
        );

        let empty = verifier
            .verify_claims::<JobClaims>(&SecretString::new(String::new()))
            .await
            .expect_err("empty");
        assert!(matches!(empty, AuthError::MissingToken));
    }

    #[tokio::test]
    async fn typed_claims_verify_under_the_key_a_jwks_names() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let (path, signer) = jwks_file(&dir);
        let verifier = JwtVerifier::from_settings(&AuthSettings {
            token_jwks_file: Some(path),
            ..AuthSettings::default()
        })
        .await
        .expect("verifier");
        let token = |kid: &str| {
            let mut header = Header::new(Algorithm::ES256);
            header.kid = Some(kid.to_owned());
            SecretString::new(
                encode(
                    &header,
                    &json!({ "exp": FUTURE, "repository_id": "812345", "sha": "457b125" }),
                    &signer,
                )
                .expect("encode"),
            )
        };

        let claims: JobClaims = verifier
            .verify_claims(&token("ec"))
            .await
            .expect("verifies");
        assert_eq!(claims.repository_id, "812345");
        let refused = verifier
            .verify_claims::<JobClaims>(&token("rsa"))
            .await
            .expect_err("refused under the RSA key");
        assert!(matches!(
            refused,
            AuthError::Invalid(InvalidToken::AlgorithmNotAllowed)
        ));
    }
}
