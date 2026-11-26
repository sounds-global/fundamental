use crate::server::providers::Exportability;
use crate::server::{KeyProtection, SignatureAlgorithm};

/// What a custody backend reports about a key, read once when a signer opens it.
///
/// The protection comes from the backend, never the caller, so custody cannot be over-claimed.
#[derive(Clone, Debug)]
pub struct KmsKeyDescription<A: SignatureAlgorithm> {
    /// The key's public half.
    pub public_key: A::PublicKey,
    /// How the backend holds the private half.
    pub protection: KeyProtection,
    /// Whether the backend would ever release the private half.
    pub exportability: Exportability,
}
