use std::future::Future;

use crate::KeyError;
use crate::hd::{MasterSeed, SealedSeed};

/// Encrypts a [`MasterSeed`] at rest and recovers it.
///
/// Every failure collapses to [`KeyError::Seal`] or [`KeyError::Unseal`] and carries no secret.
pub trait SeedSealer {
    /// Seals `seed` into a persistable [`SealedSeed`].
    ///
    /// # Errors
    /// [`KeyError::Seal`] if the wrapping key is unavailable or the AEAD fails.
    fn seal(&self, seed: &MasterSeed) -> impl Future<Output = Result<SealedSeed, KeyError>>;

    /// Recovers the [`MasterSeed`] from a [`SealedSeed`].
    ///
    /// # Errors
    /// [`KeyError::Unseal`] on a wrong key, password or passkey, a missing wrapping key, or a
    /// tampered envelope.
    fn unseal(&self, sealed: &SealedSeed) -> impl Future<Output = Result<MasterSeed, KeyError>>;
}
