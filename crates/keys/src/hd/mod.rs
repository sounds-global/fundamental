//! Hierarchical-deterministic (HD) seed custody: BIP-39 mnemonics, SLIP-0010 derivation and
//! the seed-at-rest sealers. Frozen: changing either standard re-derives every key.

mod bip39_wordlist;
mod derivation_path;
mod derive_ed25519;
mod device_key_fingerprint;
mod hkdf_sha512;
mod hmac_sha512;
mod master_seed;
mod mnemonic;
mod sealed_seed;
mod seed_sealer;
mod vault_content_key;
mod vault_owner_id;

pub use derivation_path::DerivationPath;
pub use derive_ed25519::derive_ed25519;
pub use device_key_fingerprint::DeviceKeyFingerprint;
pub use master_seed::MasterSeed;
pub use mnemonic::Mnemonic;
pub use sealed_seed::SealedSeed;
pub use seed_sealer::SeedSealer;
pub use vault_content_key::VaultContentKey;
pub use vault_owner_id::VaultOwnerId;

mod seal_aead;
mod seed_transport;
mod vault_cipher;
mod vault_owner_key;
pub use seed_transport::{open_master_seed, seal_master_seed};
pub use vault_cipher::VaultCipher;
pub use vault_owner_key::{VAULT_AUTH_DOMAIN, VaultOwnerKey, verify_owner_challenge};

#[cfg(not(target_arch = "wasm32"))]
mod hd_seed_store;
#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
mod keychain_sealer;
#[cfg(not(target_arch = "wasm32"))]
pub use hd_seed_store::HdSeedStore;
#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
pub use keychain_sealer::KeychainSealer;

#[cfg(all(not(target_arch = "wasm32"), feature = "keys-hd-password"))]
mod password_sealer;
#[cfg(all(not(target_arch = "wasm32"), feature = "keys-hd-password"))]
pub use password_sealer::PasswordSealer;
