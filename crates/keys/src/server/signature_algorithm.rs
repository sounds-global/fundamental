use std::fmt;

use crate::KeyError;
use crate::server::SignatureAlgorithmId;

mod sealed {
    pub trait Sealed {}
}

pub(crate) use sealed::Sealed;

/// A signature scheme, named at the type level so a signer's curve is part of its type.
///
/// Sealed: adding an algorithm is this crate's security decision.
pub trait SignatureAlgorithm: Sealed + Send + Sync + 'static {
    /// The runtime name of this algorithm, for storage and headers.
    const ID: SignatureAlgorithmId;

    /// The public key that verifies this algorithm's signatures.
    type PublicKey: Clone + PartialEq + Eq + fmt::Debug + Send + Sync + 'static;

    /// A signature produced under this algorithm.
    type Signature: Clone + fmt::Debug + Send + Sync + 'static;

    /// Verifies `signature` over `message` under `key`.
    ///
    /// # Errors
    /// Returns [`KeyError::BadSignature`] if the signature does not verify, or
    /// [`KeyError::Malformed`] if the key is not usable.
    fn verify(
        key: &Self::PublicKey,
        message: &[u8],
        signature: &Self::Signature,
    ) -> Result<(), KeyError>;

    /// The signature's wire bytes — for ECDSA the fixed-width `r ‖ s` form JOSE uses.
    fn signature_bytes(signature: &Self::Signature) -> Vec<u8>;
}
