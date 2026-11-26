//! ES256 signers for the suites that serve a JWKS, which refuses symmetric keys.

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::Value;

#[allow(dead_code, reason = "each suite uses part of the shared key file")]
#[path = "../../src/test_keys.rs"]
#[expect(clippy::allow_attributes, reason = "readability baseline")]
mod test_keys;

use test_keys::{es256_key, public_jwk};

pub struct Es256 {
    kid: String,
    encoding: EncodingKey,
}

impl Es256 {
    /// The key whose private scalar is `seed` repeated 32 times, published as `kid`.
    pub fn new(kid: &str, seed: u8) -> Self {
        Self {
            kid: kid.to_owned(),
            encoding: es256_key(seed).encoding,
        }
    }

    /// The public JWK, carrying `kid` and `alg`.
    pub fn jwk(&self) -> Value {
        public_jwk(&self.encoding, Algorithm::ES256, Some(&self.kid))
    }

    pub fn jwks(&self) -> String {
        serde_json::json!({ "keys": [self.jwk()] }).to_string()
    }

    /// `claims`, signed by this key under a header naming `kid`.
    pub fn sign(&self, kid: &str, claims: &Value) -> String {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(kid.to_owned());
        encode(&header, claims, &self.encoding).expect("encode")
    }
}
