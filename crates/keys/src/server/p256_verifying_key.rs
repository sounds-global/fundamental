use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::EncodedPoint;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature as EcdsaSignature, VerifyingKey as EcdsaVerifyingKey};
use tracing::warn;

use crate::KeyError;
use crate::server::P256Signature;

/// A NIST P-256 public key whose point has been validated on the curve.
#[derive(Clone, PartialEq, Eq)]
pub struct P256VerifyingKey(EcdsaVerifyingKey);

impl P256VerifyingKey {
    /// Parses a SEC1-encoded point (compressed or uncompressed).
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the bytes are not a valid P-256 point.
    pub fn from_sec1(bytes: &[u8]) -> Result<Self, KeyError> {
        EcdsaVerifyingKey::from_sec1_bytes(bytes)
            .map(Self)
            .map_err(|_| KeyError::Malformed("not a valid P-256 point".to_owned()))
    }

    /// Parses a PEM `SubjectPublicKeyInfo` document holding a P-256 key.
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the document is not a P-256 public key.
    pub fn from_public_key_pem(pem: &str) -> Result<Self, KeyError> {
        use p256::pkcs8::DecodePublicKey;
        EcdsaVerifyingKey::from_public_key_pem(pem)
            .map(Self)
            .map_err(|_| KeyError::Malformed("not a P-256 SubjectPublicKeyInfo PEM".to_owned()))
    }

    /// Builds a key from the fixed-width affine coordinates of a JWK (`x`, `y`).
    ///
    /// # Errors
    /// Returns [`KeyError::Malformed`] if the coordinates are not a point on the curve.
    pub fn from_coordinates(x: &[u8; 32], y: &[u8; 32]) -> Result<Self, KeyError> {
        let point = EncodedPoint::from_affine_coordinates(x.into(), y.into(), false);
        Self::from_sec1(point.as_bytes())
    }

    /// The 65-byte uncompressed SEC1 encoding (`0x04 ‖ x ‖ y`).
    #[must_use]
    pub fn to_sec1_uncompressed(&self) -> [u8; 65] {
        let point = self.0.to_encoded_point(false);
        let mut out = [0u8; 65];
        out.copy_from_slice(point.as_bytes());
        out
    }

    /// The JWK affine coordinates (`x`, `y`), each 32 bytes big-endian.
    #[must_use]
    pub fn jwk_coordinates(&self) -> ([u8; 32], [u8; 32]) {
        let sec1 = self.to_sec1_uncompressed();
        let mut x = [0u8; 32];
        let mut y = [0u8; 32];
        x.copy_from_slice(&sec1[1..33]);
        y.copy_from_slice(&sec1[33..65]);
        (x, y)
    }

    /// Verifies `signature` over `message`, hashing with SHA-256 as ES256 requires.
    ///
    /// # Errors
    /// Returns [`KeyError::BadSignature`] if the signature does not verify.
    pub fn verify(&self, message: &[u8], signature: &P256Signature) -> Result<(), KeyError> {
        let parsed = EcdsaSignature::from_slice(&signature.to_bytes())
            .map_err(|_| KeyError::Malformed("not a valid P-256 signature".to_owned()))?;
        self.0.verify(message, &parsed).map_err(|_| {
            warn!("P-256 signature did not verify");
            KeyError::BadSignature
        })
    }
}

impl fmt::Debug for P256VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "P256VerifyingKey({})",
            URL_SAFE_NO_PAD.encode(self.to_sec1_uncompressed())
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::SigningKey as EcdsaSigningKey;

    fn fixed_key() -> EcdsaSigningKey {
        EcdsaSigningKey::from_slice(&[7u8; 32]).expect("valid scalar")
    }

    #[test]
    fn coordinates_round_trip_to_the_same_key() {
        let key = P256VerifyingKey(*fixed_key().verifying_key());
        let (x, y) = key.jwk_coordinates();
        assert_eq!(
            P256VerifyingKey::from_coordinates(&x, &y).expect("on curve"),
            key
        );
    }

    #[test]
    fn a_sec1_key_off_the_curve_is_refused() {
        let mut bytes = P256VerifyingKey(*fixed_key().verifying_key()).to_sec1_uncompressed();
        bytes[64] ^= 0x01;
        assert!(matches!(
            P256VerifyingKey::from_sec1(&bytes),
            Err(KeyError::Malformed(_))
        ));
    }

    #[test]
    fn a_spki_pem_parses_to_the_same_key() {
        use p256::pkcs8::EncodePublicKey;
        let signing = fixed_key();
        let pem = signing
            .verifying_key()
            .to_public_key_pem(p256::pkcs8::LineEnding::LF)
            .expect("encodes");
        assert_eq!(
            P256VerifyingKey::from_public_key_pem(&pem).expect("parses"),
            P256VerifyingKey(*signing.verifying_key())
        );
        assert!(
            P256VerifyingKey::from_public_key_pem(
                "-----BEGIN PUBLIC KEY-----\nAA==\n-----END PUBLIC KEY-----\n"
            )
            .is_err()
        );
    }

    #[test]
    fn garbage_is_refused() {
        assert!(P256VerifyingKey::from_sec1(&[0x04; 10]).is_err());
    }
}
