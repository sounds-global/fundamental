use crate::server::signature_algorithm::Sealed;
use crate::server::{SignatureAlgorithm, SignatureAlgorithmId};
use crate::{KeyError, Signature, VerifyingKey};

/// Ed25519 (RFC 8032): the framework's device and log-cosigning curve.
#[derive(Debug)]
pub enum Ed25519 {}

impl Sealed for Ed25519 {}

impl SignatureAlgorithm for Ed25519 {
    const ID: SignatureAlgorithmId = SignatureAlgorithmId::Ed25519;
    type PublicKey = VerifyingKey;
    type Signature = Signature;

    fn verify(key: &VerifyingKey, message: &[u8], signature: &Signature) -> Result<(), KeyError> {
        key.verify(message, signature)
    }

    fn signature_bytes(signature: &Signature) -> Vec<u8> {
        signature.to_bytes().to_vec()
    }
}
