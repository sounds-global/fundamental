use std::future::Future;

use crate::{KeyError, Signature, VerifyingKey};

/// Signs messages with a device's private key, without exposing that key.
///
/// Asynchronous because the browser backend signs through WebCrypto.
pub trait DeviceSigner {
    /// Returns the public key that verifies this signer's signatures.
    fn verifying_key(&self) -> VerifyingKey;

    /// Signs `message` with the device's private key.
    ///
    /// # Errors
    /// Returns a [`KeyError`] if the signing operation fails.
    fn sign(&self, message: &[u8]) -> impl Future<Output = Result<Signature, KeyError>>;
}
