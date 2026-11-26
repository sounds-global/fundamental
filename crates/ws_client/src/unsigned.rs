//! [`Unsigned`] — the placeholder signer for a client that does not sign pushes.

use std::future::Future;

use fundamental_keys::{DeviceSigner, KeyError, Signature, VerifyingKey};

/// The signer type of an unsigned [`WsRemoteSource`](crate::WsRemoteSource).
///
/// Uninhabited: it exists only so unsigned and signed clients share one type.
pub enum Unsigned {}

impl DeviceSigner for Unsigned {
    fn verifying_key(&self) -> VerifyingKey {
        match *self {}
    }

    #[expect(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        reason = "readability baseline"
    )]
    fn sign(&self, _message: &[u8]) -> impl Future<Output = Result<Signature, KeyError>> {
        match *self {}
        #[allow(unreachable_code)]
        std::future::ready(unreachable!())
    }
}
