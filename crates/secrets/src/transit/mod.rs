//! Signing with keys held by a Vault or OpenBao Transit engine.

mod ed25519_public_key_bytes;
mod ed25519_signature_bytes;
#[cfg(feature = "transit-signer")]
mod transit_algorithm;
mod transit_key_info;
mod transit_key_name;
mod transit_key_ref;
mod transit_key_type;
mod transit_mount;
mod transit_signer_limits;
#[cfg(feature = "transit-signer")]
mod vault_transit_client;

pub use ed25519_public_key_bytes::ed25519_public_key_bytes;
pub use ed25519_signature_bytes::ed25519_signature_bytes;
#[cfg(feature = "transit-signer")]
pub use transit_algorithm::TransitAlgorithm;
pub use transit_key_info::TransitKeyInfo;
pub use transit_key_name::TransitKeyName;
pub use transit_key_ref::TransitKeyRef;
pub use transit_key_type::TransitKeyType;
pub use transit_mount::TransitMount;
pub use transit_signer_limits::TransitSignerLimits;
#[cfg(feature = "transit-signer")]
pub use vault_transit_client::VaultTransitClient;
