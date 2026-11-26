use std::future::Future;
use std::pin::Pin;

use fundamental_core::AnyError;

use crate::{PushRegistration, PushUnregistered};

/// Records a device's [`PushRegistration`] wherever the app's sender will look
/// for it.
///
/// Called only for a registration not yet recorded: the first, a rotation, or a retry.
///
/// # Example
///
/// ```no_run
/// # use std::future::Future;
/// # use std::pin::Pin;
/// # use fundamental_native::{PushRegistrar, PushRegistration};
/// # use fundamental_core::AnyError;
/// struct RegisterWithApi;
///
/// impl PushRegistrar for RegisterWithApi {
///     fn register(
///         &self,
///         registration: PushRegistration,
///     ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>> {
///         Box::pin(async move {
///             // The one place the token is unwrapped: the request that carries it.
///             let _body = serde_json::json!({
///                 "provider": format!("{:?}", registration.provider()),
///                 "token": registration.token().expose(),
///             });
///             Ok(())
///         })
///     }
/// }
/// ```
pub trait PushRegistrar: Send + Sync + 'static {
    /// Records `registration` wherever the app's notifications are sent from.
    ///
    /// # Errors
    /// Any failure to record it; the registration is then retried, so never return `Ok` for a
    /// write that did not happen.
    fn register(
        &self,
        registration: PushRegistration,
    ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>>;

    /// The device's registration is gone until a new token arrives.
    fn unregistered(&self, reason: PushUnregistered) {
        let _ = reason;
    }
}

/// The no-op registrar: an app that only *receives* pushes registers `()`.
impl PushRegistrar for () {
    fn register(
        &self,
        registration: PushRegistration,
    ) -> Pin<Box<dyn Future<Output = Result<(), AnyError>> + Send + '_>> {
        let _ = registration;
        Box::pin(async { Ok(()) })
    }
}
