use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_core::secrets::SecretString;

/// Supplies the bearer token a [`RemoteBridge`](crate::RemoteBridge) attaches
/// to outgoing calls.
///
/// `Ok(None)` sends the request unauthenticated; an `Err` aborts the call.
pub trait TokenSource: Send + Sync + 'static {
    /// Resolves the current bearer token, or `None` when no one is signed in.
    ///
    /// # Errors
    /// Any failure of the underlying store; the bridge maps it to `NATIVE_004` and does not send.
    fn token(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<SecretString>, AnyError>> + Send + '_>>;
}
