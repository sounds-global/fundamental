use crate::server::{KeyId, KeyProtection};
use crate::{KeyError, Signature, VerifyingKey};

/// A server-side signing key held behind its custody boundary.
///
/// Synchronous, so it stays object-safe as `Arc<dyn Signer>`; Ed25519 throughout.
pub trait Signer: Send + Sync {
    /// The logical identifier of this signer's key, for logs and routing.
    fn key_id(&self) -> KeyId;

    /// The public key that verifies this signer's signatures.
    fn public_key(&self) -> VerifyingKey;

    /// How this signer's private key is protected — the accreditation input.
    fn protection(&self) -> KeyProtection;

    /// Whether producing a signature requires a second independent signer.
    fn dual_control(&self) -> bool {
        false
    }

    /// Produces the raw Ed25519 signature over `message`.
    ///
    /// # Errors
    /// Returns [`KeyError::Custody`] if the backend cannot sign, or [`KeyError::DualControl`]
    /// when a guardian refuses; never a partial or unverified signature.
    fn sign(&self, message: &[u8]) -> Result<Signature, KeyError>;
}
