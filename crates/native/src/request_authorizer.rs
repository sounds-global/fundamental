use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;
use fundamental_core::secrets::SecretString;
use fundamental_server::Method;

/// The headers one [`RequestAuthorizer::authorize`] call yields, still being produced.
pub type AuthorizedHeaders<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<(String, String)>, AnyError>> + Send + 'a>>;

/// Produces the credential headers for one outbound request.
///
/// A DPoP proof binds method and URL, so it is made per request.
/// [`RemoteBridge`](crate::RemoteBridge) applies these headers last and never forwards
/// `authorization` or `dpop` from the webview.
pub trait RequestAuthorizer: Send + Sync + 'static {
    /// Returns the headers to attach, given the request this call is about.
    ///
    /// `token` is what the bridge's [`TokenSource`](crate::TokenSource) yielded, for DPoP's `ath`.
    ///
    /// # Errors
    /// Returns an [`AnyError`] if the credential cannot be produced; the call is aborted.
    fn authorize<'a>(
        &'a self,
        method: Method,
        url: &'a str,
        token: Option<&'a SecretString>,
    ) -> AuthorizedHeaders<'a>;
}
