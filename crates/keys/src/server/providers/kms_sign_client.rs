use std::future::Future;
use std::pin::Pin;

use crate::KeyError;
use crate::server::providers::KmsKeyDescription;
use crate::server::{SignFuture, SignatureAlgorithm};

/// A boxed future resolving to a key description.
pub type DescribeFuture<'a, A> =
    Pin<Box<dyn Future<Output = Result<KmsKeyDescription<A>, KeyError>> + Send + 'a>>;

/// A key held by a network custody service whose private material never leaves it.
///
/// [`KmsSigner`](super::KmsSigner) wraps it and adds the checks every backend needs.
pub trait KmsSignClient<A: SignatureAlgorithm>: Send + Sync {
    /// Reads the key's public half, protection and exportability from the service.
    ///
    /// # Errors
    /// [`KeyError::CustodyUnavailable`] if the service did not answer, or
    /// [`KeyError::Custody`] if it refused or the key does not exist.
    fn describe(&self) -> DescribeFuture<'_, A>;

    /// Asks the service to sign `message` with the key.
    ///
    /// # Errors
    /// [`KeyError::CustodyUnavailable`] if the service did not answer, or
    /// [`KeyError::Custody`] if it refused.
    fn sign<'a>(&'a self, message: &'a [u8]) -> SignFuture<'a, A>;
}
