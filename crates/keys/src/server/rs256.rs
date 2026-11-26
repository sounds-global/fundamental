use crate::KeyError;
use crate::server::signature_algorithm::Sealed;
use crate::server::{RsaSignature, RsaVerifyingKey, SignatureAlgorithm, SignatureAlgorithmId};

/// RSASSA-PKCS1-v1_5 with SHA-256, JOSE `RS256` (RFC 7518 §3.3), for counterparties that fix it.
#[derive(Debug)]
pub enum Rs256 {}

impl Sealed for Rs256 {}

impl SignatureAlgorithm for Rs256 {
    const ID: SignatureAlgorithmId = SignatureAlgorithmId::RsaPkcs1Sha256;
    type PublicKey = RsaVerifyingKey;
    type Signature = RsaSignature;

    fn verify(
        key: &RsaVerifyingKey,
        message: &[u8],
        signature: &RsaSignature,
    ) -> Result<(), KeyError> {
        key.verify(message, signature)
    }

    fn signature_bytes(signature: &RsaSignature) -> Vec<u8> {
        signature.as_bytes().to_vec()
    }
}
