//! Device identity for the [Fundamental](https://docs.rs/fundamental) framework.
//!
//! Each device signs with its own ed25519 key ([`DeviceSigner`]) kept in platform storage:
//! the OS keychain natively, a non-extractable WebCrypto key in the browser. Credentials that
//! are not seeds go through [`SecretStore`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

mod device_signer;
mod key_error;
mod platform;
mod signature;
mod verifying_key;

/// Hierarchical-deterministic seed custody (feature `keys-hd`).
#[cfg(feature = "keys-hd")]
mod hd;

#[cfg(all(feature = "client-identity", not(target_arch = "wasm32")))]
mod client;
#[cfg(not(target_arch = "wasm32"))]
mod secret_store;
#[cfg(not(target_arch = "wasm32"))]
mod seed_store;
#[cfg(not(target_arch = "wasm32"))]
mod server;
mod signing_key;

#[cfg(all(feature = "client-tpm", target_os = "linux"))]
pub use client::TpmDeviceKey;
#[cfg(all(feature = "client-identity", not(target_arch = "wasm32")))]
pub use client::{
    DeviceKey, KeyCustody, SealedDeviceKey, certificate_key, certificate_request, open_device_key,
};
pub use device_signer::DeviceSigner;
pub use key_error::KeyError;
pub use signature::Signature;
pub use verifying_key::VerifyingKey;

// Pure data, so it lives in `fundamental_core`; re-exported for `fundamental::keys::DeviceId`.
pub use fundamental_core::DeviceId;

#[cfg(not(target_arch = "wasm32"))]
pub use platform::StoredSigner;
#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
pub use platform::{KeyringSecretStore, KeyringSeedStore};
#[cfg(not(target_arch = "wasm32"))]
pub use secret_store::{MemorySecretStore, SecretStore};
#[cfg(not(target_arch = "wasm32"))]
pub use seed_store::{MemorySeedStore, SeedStore};
pub use signing_key::SigningKey;

#[cfg(not(target_arch = "wasm32"))]
pub use server::{
    AsyncSigner, DescribeFuture, DualControlSigner, Ed25519, Es256, Exportability, KeyId,
    KeyProtection, KeyResidence, KmsKeyDescription, KmsSignClient, KmsSigner, P256Signature,
    P256VerifyingKey, Rs256, RsaSignature, RsaVerifyingKey, SignFuture, SignatureAlgorithm,
    SignatureAlgorithmId, Signer, SoftwareP256Signer, SoftwareSigner, jws,
};
#[cfg(all(not(target_arch = "wasm32"), feature = "hsm"))]
pub use server::{Pkcs11Config, Pkcs11Signer};

#[cfg(target_arch = "wasm32")]
pub use platform::WebCryptoSigner;
#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
pub use platform::{AEAD_NONCE_LEN, WebCryptoAead};

#[cfg(all(feature = "keys-hd", not(target_arch = "wasm32")))]
pub use hd::HdSeedStore;
#[cfg(all(
    feature = "keys-hd",
    feature = "os-keychain",
    not(target_arch = "wasm32")
))]
pub use hd::KeychainSealer;
#[cfg(all(feature = "keys-hd-password", not(target_arch = "wasm32")))]
pub use hd::PasswordSealer;
#[cfg(feature = "keys-hd")]
pub use hd::{
    DerivationPath, DeviceKeyFingerprint, MasterSeed, Mnemonic, SealedSeed, SeedSealer,
    derive_ed25519,
};
#[cfg(feature = "keys-hd")]
pub use hd::{
    VAULT_AUTH_DOMAIN, VaultCipher, VaultContentKey, VaultOwnerId, VaultOwnerKey, open_master_seed,
    seal_master_seed, verify_owner_challenge,
};
#[cfg(all(feature = "keys-hd", target_arch = "wasm32"))]
pub use platform::{WebCryptoSealer, load_sealed_seed, store_sealed_seed};
