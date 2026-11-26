//! [`BoxedVerifier`] — an object-safe view of a [`PrincipalVerifier`], whose `verify` returns `impl Future`.

use std::future::Future;
use std::pin::Pin;

use fundamental_auth::{AuthError, AuthenticatedPrincipal, PrincipalVerifier};
use fundamental_core::secrets::SecretString;

pub(crate) type VerifyFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AuthenticatedPrincipal, AuthError>> + Send + 'a>>;

/// An object-safe verifier, the concrete [`PrincipalVerifier`] type erased.
pub(crate) trait BoxedVerifier: Send + Sync + 'static {
    fn verify<'a>(&'a self, token: &'a SecretString) -> VerifyFuture<'a>;
}

impl<V> BoxedVerifier for V
where
    V: PrincipalVerifier + Send + Sync + 'static,
{
    fn verify<'a>(&'a self, token: &'a SecretString) -> VerifyFuture<'a> {
        Box::pin(PrincipalVerifier::verify(self, token))
    }
}
