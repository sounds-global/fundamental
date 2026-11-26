use std::future::Future;
use std::pin::Pin;

use fundamental_auth::AuthenticatedPrincipal;
use fundamental_core::AnyError;

/// Resolves the app's current principal ahead of in-process dispatch.
///
/// Attached to each request before [`Router::dispatch`](fundamental_server::Router::dispatch).
/// `Ok(None)` is deliberately unauthenticated; an `Err` aborts the call.
pub trait PrincipalSource: Send + Sync + 'static {
    /// Resolves the current principal, or `None` when no one is signed in.
    ///
    /// # Errors
    /// Any failure of the underlying store; the bridge maps it to `NATIVE_001` and does not
    /// dispatch.
    fn resolve(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<AuthenticatedPrincipal>, AnyError>> + Send + '_>>;
}
