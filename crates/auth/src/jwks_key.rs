//! [`JwksKey`] — one key from a JWKS document, with the algorithms it alone may verify.

use jsonwebtoken::jwk::{
    AlgorithmParameters, EllipticCurve, Jwk, KeyAlgorithm, KeyOperations, PublicKeyUse,
};
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation, decode};
use serde::de::DeserializeOwned;

use crate::AuthError;
use crate::claim_pins::ClaimPins;
use crate::jwt_verifier::map_jwt_error;
use crate::refused_jwk::RefusedJwk;

const RSA_DEFAULT: [Algorithm; 3] = [Algorithm::RS256, Algorithm::RS384, Algorithm::RS512];
const RSA_ANY: [Algorithm; 6] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
];

/// A verification key and its policy, listing only the algorithms this key can verify.
pub(crate) struct JwksKey {
    key: DecodingKey,
    validation: Validation,
}

impl JwksKey {
    ///
    /// # Errors
    /// The [`RefusedJwk`] class when the key is symmetric, not meant for signatures, on an
    /// unsupported curve, declares an algorithm it cannot verify, or cannot be read.
    pub(crate) fn from_jwk(jwk: &Jwk, pins: &ClaimPins) -> Result<Self, RefusedJwk> {
        let algorithms = algorithms_for(jwk)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|e| RefusedJwk::Malformed(e.to_string()))?;
        let mut validation = Validation::new(algorithms[0]);
        validation.algorithms = algorithms;
        pins.apply(&mut validation);
        Ok(Self { key, validation })
    }

    /// Verifies `token` under this key and its own algorithms, and decodes its claims.
    pub(crate) fn decode<T: DeserializeOwned>(
        &self,
        token: &str,
    ) -> Result<TokenData<T>, AuthError> {
        decode::<T>(token, &self.key, &self.validation).map_err(map_jwt_error)
    }

    #[cfg(test)]
    pub(crate) fn algorithms(&self) -> &[Algorithm] {
        &self.validation.algorithms
    }
}

/// The algorithms `jwk` may verify, narrowed to its declared `alg`; never empty.
fn algorithms_for(jwk: &Jwk) -> Result<Vec<Algorithm>, RefusedJwk> {
    let (admitted, default): (&[Algorithm], &[Algorithm]) = match &jwk.algorithm {
        AlgorithmParameters::OctetKey(_) => return Err(RefusedJwk::Symmetric),
        AlgorithmParameters::RSA(_) => (&RSA_ANY, &RSA_DEFAULT),
        AlgorithmParameters::EllipticCurve(params) => match params.curve {
            EllipticCurve::P256 => (&[Algorithm::ES256], &[Algorithm::ES256]),
            EllipticCurve::P384 => (&[Algorithm::ES384], &[Algorithm::ES384]),
            EllipticCurve::P521 | EllipticCurve::Ed25519 => {
                return Err(RefusedJwk::UnsupportedCurve);
            }
        },
        AlgorithmParameters::OctetKeyPair(params) => match params.curve {
            EllipticCurve::Ed25519 => (&[Algorithm::EdDSA], &[Algorithm::EdDSA]),
            EllipticCurve::P256 | EllipticCurve::P384 | EllipticCurve::P521 => {
                return Err(RefusedJwk::UnsupportedCurve);
            }
        },
    };
    if !is_for_signatures(jwk) {
        return Err(RefusedJwk::NotForSignatures);
    }
    match jwk.common.key_algorithm {
        None => Ok(default.to_vec()),
        Some(declared) => match signature_algorithm(declared)? {
            Some(algorithm) if admitted.contains(&algorithm) => Ok(vec![algorithm]),
            _ => Err(RefusedJwk::AlgorithmMismatch),
        },
    }
}

/// Whether `use` and `key_ops`, where published, permit verifying signatures.
fn is_for_signatures(jwk: &Jwk) -> bool {
    let use_permits = matches!(
        jwk.common.public_key_use,
        None | Some(PublicKeyUse::Signature)
    );
    let ops_permit = jwk
        .common
        .key_operations
        .as_ref()
        .is_none_or(|ops| ops.contains(&KeyOperations::Verify));
    use_permits && ops_permit
}

/// The signature algorithm a declared `alg` names; `None` for one this crate cannot verify.
///
/// # Errors
/// [`RefusedJwk::NotForSignatures`] when `alg` names a key-encryption algorithm.
fn signature_algorithm(declared: KeyAlgorithm) -> Result<Option<Algorithm>, RefusedJwk> {
    Ok(Some(match declared {
        KeyAlgorithm::HS256 => Algorithm::HS256,
        KeyAlgorithm::HS384 => Algorithm::HS384,
        KeyAlgorithm::HS512 => Algorithm::HS512,
        KeyAlgorithm::ES256 => Algorithm::ES256,
        KeyAlgorithm::ES384 => Algorithm::ES384,
        KeyAlgorithm::RS256 => Algorithm::RS256,
        KeyAlgorithm::RS384 => Algorithm::RS384,
        KeyAlgorithm::RS512 => Algorithm::RS512,
        KeyAlgorithm::PS256 => Algorithm::PS256,
        KeyAlgorithm::PS384 => Algorithm::PS384,
        KeyAlgorithm::PS512 => Algorithm::PS512,
        KeyAlgorithm::EdDSA => Algorithm::EdDSA,
        KeyAlgorithm::RSA1_5 | KeyAlgorithm::RSA_OAEP | KeyAlgorithm::RSA_OAEP_256 => {
            return Err(RefusedJwk::NotForSignatures);
        }
        KeyAlgorithm::UNKNOWN_ALGORITHM => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::test_keys::{es256_key, public_jwk, rsa_key};

    fn jwk(value: Value) -> Jwk {
        serde_json::from_value(value).expect("a JWK")
    }

    fn ec(curve: &str, extra: &Value) -> Jwk {
        let mut value = json!({ "kty": "EC", "crv": curve, "x": "AQ", "y": "AQ" });
        merge(&mut value, extra);
        jwk(value)
    }

    fn rsa(extra: &Value) -> Jwk {
        let mut value = public_jwk(&rsa_key(), Algorithm::RS256, None);
        if let Value::Object(map) = &mut value {
            map.remove("alg");
        }
        merge(&mut value, extra);
        jwk(value)
    }

    fn merge(value: &mut Value, extra: &Value) {
        if let (Value::Object(into), Value::Object(from)) = (value, extra) {
            into.extend(from.clone());
        }
    }

    fn refusal(jwk: &Jwk) -> RefusedJwk {
        JwksKey::from_jwk(jwk, &ClaimPins::default())
            .err()
            .expect("the key is refused")
    }

    fn algorithms(jwk: &Jwk) -> Vec<Algorithm> {
        JwksKey::from_jwk(jwk, &ClaimPins::default())
            .expect("the key is accepted")
            .algorithms()
            .to_vec()
    }

    #[test]
    fn an_oct_key_is_refused_whatever_it_declares() {
        for extra in [json!({}), json!({ "alg": "HS256", "use": "sig" })] {
            let mut value = json!({ "kty": "oct", "k": "c2VjcmV0" });
            merge(&mut value, &extra);
            assert_eq!(refusal(&jwk(value)), RefusedJwk::Symmetric);
        }
    }

    #[test]
    fn a_p256_key_verifies_es256_only() {
        let p256 = jwk(public_jwk(&es256_key(7).encoding, Algorithm::ES256, None));
        assert_eq!(algorithms(&p256), [Algorithm::ES256]);
        assert_eq!(algorithms(&ec("P-256", &json!({}))), [Algorithm::ES256]);
        assert_eq!(algorithms(&ec("P-384", &json!({}))), [Algorithm::ES384]);
    }

    #[test]
    fn an_rsa_key_verifies_the_rsa_family_or_the_one_it_declares() {
        assert_eq!(algorithms(&rsa(&json!({}))), RSA_DEFAULT);
        assert_eq!(
            algorithms(&rsa(&json!({ "alg": "PS256" }))),
            [Algorithm::PS256]
        );
    }

    #[test]
    fn a_declared_alg_the_key_cannot_verify_is_refused() {
        assert_eq!(
            refusal(&ec("P-256", &json!({ "alg": "ES384" }))),
            RefusedJwk::AlgorithmMismatch
        );
        assert_eq!(
            refusal(&rsa(&json!({ "alg": "ES256" }))),
            RefusedJwk::AlgorithmMismatch
        );
        assert_eq!(
            refusal(&rsa(&json!({ "alg": "HS256" }))),
            RefusedJwk::AlgorithmMismatch
        );
        assert_eq!(
            refusal(&rsa(&json!({ "alg": "made-up" }))),
            RefusedJwk::AlgorithmMismatch
        );
    }

    #[test]
    fn a_key_meant_for_encryption_is_refused() {
        for extra in [
            json!({ "use": "enc" }),
            json!({ "use": "something-else" }),
            json!({ "key_ops": ["encrypt", "wrapKey"] }),
            json!({ "alg": "RSA-OAEP-256" }),
        ] {
            assert_eq!(
                refusal(&rsa(&extra)),
                RefusedJwk::NotForSignatures,
                "{extra}"
            );
        }
        assert_eq!(
            algorithms(&rsa(&json!({ "use": "sig", "key_ops": ["verify"] }))),
            RSA_DEFAULT
        );
    }

    #[test]
    fn a_curve_nothing_here_verifies_is_refused() {
        assert_eq!(
            refusal(&ec("P-521", &json!({}))),
            RefusedJwk::UnsupportedCurve
        );
        assert_eq!(
            refusal(&jwk(json!({ "kty": "OKP", "crv": "P-256", "x": "AQ" }))),
            RefusedJwk::UnsupportedCurve
        );
    }

    #[test]
    fn an_ed25519_key_verifies_eddsa_only() {
        let ed25519 = jwk(json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        }));
        assert_eq!(algorithms(&ed25519), [Algorithm::EdDSA]);
    }
}
