//! [`MultiIssuerVerifier`] — accept tokens from several issuers, each bounded on its own.

use std::collections::BTreeMap;

use fundamental_core::secrets::SecretString;
use jsonwebtoken::dangerous::insecure_decode;
use jsonwebtoken::{Validation, decode, decode_header};
use serde::Deserialize;
use tracing::{Span, field, info, instrument, warn};

use crate::jwt_verifier::map_jwt_error;
use crate::record_refusal::record_refusal;
use crate::{
    AuthError, AuthenticatedPrincipal, InvalidToken, PrincipalVerifier, TrustedIssuer,
    UntrustedToken, UserId,
};
use fundamental_core::DeviceId;

/// A [`PrincipalVerifier`] accepting tokens from several issuers, each verified with its own key,
/// algorithm, audience and scope ceiling.
#[derive(Debug)]
pub struct MultiIssuerVerifier {
    issuers: BTreeMap<String, TrustedIssuer>,
}

#[derive(Deserialize)]
struct Unverified {
    iss: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
}

impl MultiIssuerVerifier {
    /// A verifier over `issuers`.
    ///
    /// # Errors
    /// [`AuthError::Config`] if two issuers share an identifier.
    pub fn new(issuers: impl IntoIterator<Item = TrustedIssuer>) -> Result<Self, AuthError> {
        let mut by_id = BTreeMap::new();
        for issuer in issuers {
            let id = issuer.issuer.clone();
            if by_id.insert(id.clone(), issuer).is_some() {
                return Err(AuthError::Config(format!(
                    "issuer `{id}` is configured twice"
                )));
            }
        }
        if by_id.is_empty() {
            return Err(AuthError::Config(
                "no trusted issuer is configured".to_owned(),
            ));
        }
        Ok(Self { issuers: by_id })
    }
}

impl PrincipalVerifier for MultiIssuerVerifier {
    #[instrument(
        name = "auth.multi_issuer_verify",
        skip_all,
        fields(iss = field::Empty, user_id = field::Empty)
    )]
    async fn verify(&self, token: &SecretString) -> Result<AuthenticatedPrincipal, AuthError> {
        let verified = self.verify_token(token.expose());
        if let Err(error) = &verified {
            record_refusal(error);
        }
        verified
    }
}

impl MultiIssuerVerifier {
    fn verify_token(&self, raw: &str) -> Result<AuthenticatedPrincipal, AuthError> {
        if raw.trim().is_empty() {
            return Err(AuthError::MissingToken);
        }
        decode_header(raw).map_err(|_| AuthError::Invalid(InvalidToken::MalformedHeader))?;
        // Unverified: used only to choose the issuer, never recorded.
        let named = insecure_decode::<Unverified>(raw)
            .map_err(|_| AuthError::Invalid(InvalidToken::Malformed))?
            .claims
            .iss
            .ok_or(AuthError::Untrusted(UntrustedToken::NoIssuer))?;
        let issuer = self
            .issuers
            .get(&named)
            .ok_or(AuthError::Untrusted(UntrustedToken::IssuerNotAccepted))?;
        Span::current().record("iss", issuer.issuer.as_str());

        let mut validation = Validation::new(issuer.algorithm);
        validation.set_issuer(&[issuer.issuer.as_str()]);
        validation.set_audience(&[issuer.audience.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let claims = decode::<Claims>(raw, &issuer.key, &validation)
            .map_err(map_jwt_error)?
            .claims;

        if let Some(excess) = claims
            .scopes
            .iter()
            .find(|scope| !issuer.scope_ceiling.contains(*scope))
        {
            warn!(iss = %issuer.issuer, scope = %excess, "token asks for a scope beyond its issuer's ceiling");
            return Err(AuthError::ScopeBeyondIssuerCeiling {
                issuer: issuer.issuer.clone(),
            });
        }

        Span::current().record("user_id", claims.sub.as_str());
        info!("principal authenticated");
        Ok(AuthenticatedPrincipal {
            user_id: UserId::new(claims.sub),
            device_id: claims.device_id.map(DeviceId::new),
            scopes: claims.scopes,
            expires_at: claims.exp,
        })
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;
    use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, encode};
    use serde_json::json;

    use super::*;
    use crate::test_keys::{Es256Key, es256_key};

    const FUTURE: i64 = 4_102_444_800;
    const OPERATOR_SECRET: &[u8] = b"operator-hs256-secret";

    fn verifier(business: &Es256Key) -> MultiIssuerVerifier {
        MultiIssuerVerifier::new([
            TrustedIssuer::new(
                "https://operator.example",
                "https://layer0.example",
                DecodingKey::from_secret(OPERATOR_SECRET),
                Algorithm::HS256,
                ["federation:accredit", "federation:trust_mark"],
            ),
            TrustedIssuer::new(
                "https://business.example",
                "https://layer0.example",
                business.decoding.clone(),
                Algorithm::ES256,
                ["tenancy:provision", "tenancy:read"],
            ),
        ])
        .expect("configured")
    }

    fn token(iss: &str, scopes: &[&str], key: &EncodingKey, alg: Algorithm) -> SecretString {
        let claims = json!({
            "iss": iss, "aud": "https://layer0.example", "sub": "org-1",
            "exp": FUTURE, "scopes": scopes,
        });
        SecretString::new(encode(&Header::new(alg), &claims, key).expect("encodes"))
    }

    #[tokio::test]
    async fn an_es256_issuer_and_an_hs256_issuer_coexist() {
        let business = es256_key(3);
        let verifier = verifier(&business);
        let from_business = token(
            "https://business.example",
            &["tenancy:provision"],
            &business.encoding,
            Algorithm::ES256,
        );
        assert_eq!(
            verifier
                .verify(&from_business)
                .await
                .expect("business")
                .user_id
                .as_str(),
            "org-1"
        );
        let from_operator = token(
            "https://operator.example",
            &["federation:accredit"],
            &EncodingKey::from_secret(OPERATOR_SECRET),
            Algorithm::HS256,
        );
        assert!(verifier.verify(&from_operator).await.is_ok());
    }

    #[tokio::test]
    async fn a_token_beyond_its_issuer_ceiling_is_refused() {
        let business = es256_key(3);
        let verifier = verifier(&business);
        let greedy = token(
            "https://business.example",
            &["tenancy:provision", "federation:accredit"],
            &business.encoding,
            Algorithm::ES256,
        );
        let error = verifier.verify(&greedy).await.expect_err("refused");
        assert!(matches!(error, AuthError::ScopeBeyondIssuerCeiling { .. }));
        assert_eq!(error.error_code(), "AUTH_010");
        assert_eq!(error.http_status_code(), 403);
    }

    #[tokio::test]
    async fn an_unknown_issuer_is_refused() {
        let business = es256_key(3);
        let stranger = token(
            "https://stranger.example",
            &[],
            &business.encoding,
            Algorithm::ES256,
        );
        assert!(matches!(
            verifier(&business).verify(&stranger).await,
            Err(AuthError::Untrusted(UntrustedToken::IssuerNotAccepted))
        ));
    }

    #[tokio::test]
    async fn an_issuer_cannot_use_another_issuers_key() {
        let business = es256_key(3);
        let impostor = es256_key(4);
        let forged = token(
            "https://business.example",
            &["tenancy:provision"],
            &impostor.encoding,
            Algorithm::ES256,
        );
        assert!(matches!(
            verifier(&business).verify(&forged).await,
            Err(AuthError::Invalid(InvalidToken::BadSignature))
        ));
    }

    #[tokio::test]
    async fn a_token_for_another_audience_is_refused() {
        let business = es256_key(3);
        let claims = json!({
            "iss": "https://business.example", "aud": "https://layer2.example", "sub": "org-1",
            "exp": FUTURE, "scopes": ["tenancy:read"],
        });
        let other = SecretString::new(
            encode(&Header::new(Algorithm::ES256), &claims, &business.encoding).expect("encodes"),
        );
        assert!(verifier(&business).verify(&other).await.is_err());
    }

    #[test]
    fn two_issuers_with_one_identifier_do_not_configure() {
        let business = es256_key(3);
        let twice = MultiIssuerVerifier::new([
            TrustedIssuer::new(
                "https://b",
                "a",
                business.decoding.clone(),
                Algorithm::ES256,
                ["x"],
            ),
            TrustedIssuer::new(
                "https://b",
                "a",
                business.decoding.clone(),
                Algorithm::ES256,
                ["x"],
            ),
        ]);
        assert!(matches!(twice, Err(AuthError::Config(_))));
    }
}
