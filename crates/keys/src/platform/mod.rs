#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::StoredSigner;

#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
mod keyring_seed_store;
#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
pub use keyring_seed_store::KeyringSeedStore;

#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
mod keyring_secret_store;
#[cfg(all(not(target_arch = "wasm32"), feature = "os-keychain"))]
pub use keyring_secret_store::KeyringSecretStore;

#[cfg(target_arch = "wasm32")]
mod idb_key_store;
#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::WebCryptoSigner;

#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
mod wasm_aead;
#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
pub use wasm_aead::{AEAD_NONCE_LEN, WebCryptoAead};

#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
mod wasm_seed_sealer;
#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
pub use wasm_seed_sealer::WebCryptoSealer;

#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
mod wasm_seed_store;
#[cfg(all(target_arch = "wasm32", feature = "keys-hd"))]
pub use wasm_seed_store::{load_sealed_seed, store_sealed_seed};
