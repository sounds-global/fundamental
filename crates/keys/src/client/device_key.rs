use std::fmt;

use super::KeyCustody;
use crate::KeyError;

/// A P-256 key a device holds and signs with, whose private half the caller never sees.
///
/// Synchronous because a TLS handshake signs synchronously.
pub trait DeviceKey: Send + Sync + fmt::Debug {
    /// Its public key, as a DER `SubjectPublicKeyInfo`.
    fn public_key_der(&self) -> &[u8];

    /// Sign `message`: ECDSA P-256 over SHA-256, DER.
    ///
    /// # Errors
    /// [`KeyError::Sign`] when the custody refuses or fails to sign.
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyError>;

    /// Where it is kept.
    fn custody(&self) -> KeyCustody;
}

impl DeviceKey for Box<dyn DeviceKey> {
    fn public_key_der(&self) -> &[u8] {
        (**self).public_key_der()
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, KeyError> {
        (**self).sign(message)
    }

    fn custody(&self) -> KeyCustody {
        (**self).custody()
    }
}
