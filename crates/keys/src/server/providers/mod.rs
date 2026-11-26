//! Custody backends for the server-side signing seams.

mod dual_control;
mod exportability;
mod kms_key_description;
mod kms_sign_client;
mod kms_signer;
mod software;
mod software_p256_signer;

pub use dual_control::DualControlSigner;
pub use exportability::Exportability;
pub use kms_key_description::KmsKeyDescription;
pub use kms_sign_client::{DescribeFuture, KmsSignClient};
pub use kms_signer::KmsSigner;
pub use software::SoftwareSigner;
pub use software_p256_signer::SoftwareP256Signer;

#[cfg(feature = "hsm")]
mod pkcs11;
#[cfg(feature = "hsm")]
mod pkcs11_config;

#[cfg(feature = "hsm")]
pub use pkcs11::Pkcs11Signer;
#[cfg(feature = "hsm")]
pub use pkcs11_config::Pkcs11Config;
