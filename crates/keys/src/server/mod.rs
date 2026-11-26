//! Server-side signing custody: [`Signer`], [`AsyncSigner`] and the [`KeyProtection`] posture.

pub mod jws;
pub mod providers;

mod async_signer;
mod ed25519;
mod es256;
mod key_id;
mod key_residence;
mod p256_signature;
mod p256_verifying_key;
mod rs256;
mod rsa_signature;
mod rsa_verifying_key;
mod sign_future;
pub(crate) mod signature_algorithm;
mod signature_algorithm_id;
mod signer;

pub use async_signer::AsyncSigner;
pub use ed25519::Ed25519;
pub use es256::Es256;
pub use fundamental_core::custody::KeyProtection;
pub use key_id::KeyId;
pub use key_residence::KeyResidence;
pub use p256_signature::P256Signature;
pub use p256_verifying_key::P256VerifyingKey;
pub use rs256::Rs256;
pub use rsa_signature::RsaSignature;
pub use rsa_verifying_key::RsaVerifyingKey;
pub use sign_future::SignFuture;
pub use signature_algorithm::SignatureAlgorithm;
pub use signature_algorithm_id::SignatureAlgorithmId;
pub use signer::Signer;

pub use providers::{
    DescribeFuture, DualControlSigner, Exportability, KmsKeyDescription, KmsSignClient, KmsSigner,
    SoftwareP256Signer, SoftwareSigner,
};

#[cfg(feature = "hsm")]
pub use providers::{Pkcs11Config, Pkcs11Signer};
