//! [`TransitAlgorithm`] — how a signature algorithm is spoken to a Transit engine.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use fundamental_keys::{
    Ed25519, Es256, KeyError, P256Signature, P256VerifyingKey, Rs256, RsaSignature,
    RsaVerifyingKey, Signature, SignatureAlgorithm, VerifyingKey,
};
use serde_json::{Map, Value};

use crate::transit::{TransitKeyType, ed25519_public_key_bytes, ed25519_signature_bytes};

mod sealed {
    pub trait Sealed {}
}

/// A signature algorithm a Transit engine signs with, and its wire encoding; sealed.
pub trait TransitAlgorithm: SignatureAlgorithm + sealed::Sealed {
    /// The engine's key type for this algorithm.
    const KEY_TYPE: TransitKeyType;

    /// The path segment after `sign/{name}`, if the algorithm needs one.
    fn sign_path_suffix() -> &'static str;

    /// Extra fields for the sign request body.
    fn sign_body(body: &mut Map<String, Value>);

    /// Decodes a published public key.
    ///
    /// # Errors
    /// [`KeyError::Malformed`] if it is not a key of this algorithm.
    fn parse_public_key(encoded: &str) -> Result<Self::PublicKey, KeyError>;

    /// Decodes the signature bytes after the `vault:vN:` prefix.
    ///
    /// # Errors
    /// [`KeyError::Malformed`] if they are not a signature of this algorithm.
    fn parse_signature(encoded: &str) -> Result<Self::Signature, KeyError>;
}

impl sealed::Sealed for Es256 {}

impl TransitAlgorithm for Es256 {
    const KEY_TYPE: TransitKeyType = TransitKeyType::EcdsaP256;

    fn sign_path_suffix() -> &'static str {
        "/sha2-256"
    }

    fn sign_body(body: &mut Map<String, Value>) {
        // JWS marshalling gives the fixed-width `r ‖ s` RFC 7518 §3.4 requires, not DER.
        body.insert("marshaling_algorithm".to_owned(), Value::from("jws"));
        body.insert("prehashed".to_owned(), Value::from(false));
    }

    fn parse_public_key(encoded: &str) -> Result<P256VerifyingKey, KeyError> {
        P256VerifyingKey::from_public_key_pem(encoded)
    }

    fn parse_signature(encoded: &str) -> Result<P256Signature, KeyError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| KeyError::Malformed("transit signature is not base64url".to_owned()))?;
        P256Signature::from_slice(&bytes)
    }
}

impl sealed::Sealed for Ed25519 {}

impl TransitAlgorithm for Ed25519 {
    const KEY_TYPE: TransitKeyType = TransitKeyType::Ed25519;

    fn sign_path_suffix() -> &'static str {
        ""
    }

    fn sign_body(_body: &mut Map<String, Value>) {}

    fn parse_public_key(encoded: &str) -> Result<VerifyingKey, KeyError> {
        let bytes = ed25519_public_key_bytes(encoded).ok_or_else(|| {
            KeyError::Malformed("transit ed25519 key is not 32 base64 bytes".to_owned())
        })?;
        Ok(VerifyingKey::from_bytes(bytes))
    }

    fn parse_signature(encoded: &str) -> Result<Signature, KeyError> {
        let bytes = ed25519_signature_bytes(encoded).ok_or_else(|| {
            KeyError::Malformed("transit ed25519 signature is not 64 base64 bytes".to_owned())
        })?;
        Ok(Signature::from_bytes(bytes))
    }
}

impl sealed::Sealed for Rs256 {}

impl TransitAlgorithm for Rs256 {
    const KEY_TYPE: TransitKeyType = TransitKeyType::Rsa2048;

    fn sign_path_suffix() -> &'static str {
        "/sha2-256"
    }

    fn sign_body(body: &mut Map<String, Value>) {
        // The engine's RSA default is PSS; RS256 is PKCS#1 v1.5 (RFC 7518 §3.3).
        body.insert("signature_algorithm".to_owned(), Value::from("pkcs1v15"));
        body.insert("prehashed".to_owned(), Value::from(false));
    }

    fn parse_public_key(encoded: &str) -> Result<RsaVerifyingKey, KeyError> {
        RsaVerifyingKey::from_public_key_pem(encoded)
    }

    fn parse_signature(encoded: &str) -> Result<RsaSignature, KeyError> {
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| KeyError::Malformed("transit signature is not base64".to_owned()))?;
        RsaSignature::from_slice(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_ed25519_key_and_signature_parse() {
        // Recorded from OpenBao 2.6.2.
        assert!(Ed25519::parse_public_key("mw0hx09eOCQl5Pb6dK4fj2UeqyIsUj66XUhreBeTAPA=").is_ok());
        assert!(Ed25519::parse_signature(
            "QHSbrGxmGorGRF4NncR8K8w01umObAgGSEr4aaMPkqtD0e01cvkwP2ahnIZSkhoD9k9jb0YRSI9WTdHUz91FAA=="
        )
        .is_ok());
    }

    #[test]
    fn a_recorded_es256_signature_parses() {
        assert!(Es256::parse_signature(
            "xAZq-wszmSMQui_1SEIi65nAjdqyyi5Tr76OQmDppD5Tl3INE4bXQ0h3evTQ6TD1gGJyUBJJvIZeUwstuB8D6g"
        )
        .is_ok());
    }

    #[test]
    fn a_recorded_rs256_key_and_signature_verify() {
        // Recorded from OpenBao 2.6.2 (rsa-2048, pkcs1v15 over `b"recorded"`).
        let key = Rs256::parse_public_key(RECORDED_RSA_KEY).expect("an SPKI PEM");
        let signature = Rs256::parse_signature(RECORDED_RSA_SIGNATURE).expect("256 bytes");
        Rs256::verify(&key, b"recorded", &signature).expect("verifies");
        assert!(Rs256::verify(&key, b"tampered", &signature).is_err());
    }

    #[test]
    fn a_url_safe_rsa_signature_is_refused() {
        let url_safe = RECORDED_RSA_SIGNATURE.replace('/', "_").replace('+', "-");
        assert!(matches!(
            Rs256::parse_signature(&url_safe),
            Err(KeyError::Malformed(_))
        ));
    }

    const RECORDED_RSA_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA4M/ksCWlsadHmBShk3IV
wKUsZm5avfDgQ5OEYR34ulLxSe5iRh7A/vD0dVbVvDIaVPkEjYSEi2aBYxk0Ksd5
abJyvp8q5GNjbHMnt97uX02MlBjewCB+XuXBlmbl2AQ/PZwRBhzAc7hEmXyevi82
AoeJ+wkk4IGa4Kg5nE0tPrXtAfGcK4DLxFz8IcDcPZa0ayjxnUPJLHv1UbtCfYsV
oH9XVl2qN8mltJemJOGiVVG+k1oj1FvLnG5nsNUHGQtHYqERBUjFbxa16EWzjPNO
Op2Uu0CLzKJHV7IEqtfu5rMke/gXp1+HtZt3Q4oF5g/BancfVtktJ2un0pmtu0/B
8QIDAQAB
-----END PUBLIC KEY-----
";

    const RECORDED_RSA_SIGNATURE: &str = "cjINZ9NykCr1I4lpTBICLZgzvDNfc8KglgyhFdvI2Dkp0h/S4GfR/d0Gz0oiak0IzAPpQXLBq1WEVmXxxS1iKj0+HINXWghPuA9mA9B3SBYU+zoD4pcsa8s7DUEDph1eMEJfX3ZM1WHsROhNlJFiRHDzo0GBgpggUwbNy9ZRIZJs13aaR3v6Vu5GLwIl8neVMCeVLROg0K2d3DFXoeuiuw/XIYf+AuA9mn++xQZLxKi7REtdMrMJbCvQmLsyQ0BJYBeOZjb7i86UmNoqVuDg7n6TldeZkbNFuerxXj/kZonv40ctUNNrnqZOVuJ/Br4yAl56Vr4RxvG7UvQmn2Ivmg==";

    #[test]
    fn a_der_signature_is_not_mistaken_for_jws() {
        assert!(matches!(
            Es256::parse_signature("MEUCIQ"),
            Err(KeyError::Malformed(_))
        ));
    }
}
