use std::future::Future;
use std::pin::Pin;

use crate::KeyError;
use crate::server::SignatureAlgorithm;

/// The future a signing call resolves through.
///
/// Boxed so `Arc<dyn AsyncSigner<A>>` is object-safe.
pub type SignFuture<'a, A> = Pin<
    Box<dyn Future<Output = Result<<A as SignatureAlgorithm>::Signature, KeyError>> + Send + 'a>,
>;
