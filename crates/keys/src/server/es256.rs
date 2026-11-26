use crate::KeyError;
use crate::server::signature_algorithm::Sealed;
use crate::server::{P256Signature, P256VerifyingKey, SignatureAlgorithm, SignatureAlgorithmId};

/// ECDSA over NIST P-256 with SHA-256, JOSE `ES256` (RFC 7518 §3.4).
#[derive(Debug)]
pub enum Es256 {}

impl Sealed for Es256 {}

impl SignatureAlgorithm for Es256 {
    const ID: SignatureAlgorithmId = SignatureAlgorithmId::EcdsaP256Sha256;
    type PublicKey = P256VerifyingKey;
    type Signature = P256Signature;

    fn verify(
        key: &P256VerifyingKey,
        message: &[u8],
        signature: &P256Signature,
    ) -> Result<(), KeyError> {
        key.verify(message, signature)
    }

    fn signature_bytes(signature: &P256Signature) -> Vec<u8> {
        signature.to_bytes().to_vec()
    }
}
