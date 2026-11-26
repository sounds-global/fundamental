use core::future::Future;

use fundamental_core::AnyError;
use fundamental_server::HttpContext;

/// Data a page loads on the server, before its view renders, read back with `use_context::<Self>()`.
///
/// The data type must compile to `wasm32`; the `impl` belongs behind the app's `ssr` feature.
///
/// ```
/// use fundamental_core::AnyError;
/// use fundamental_server::HttpContext;
/// use fundamental_web::PageData;
///
/// #[derive(Clone)]
/// struct Store {
///     greeting: &'static str,
/// }
///
/// /// What the page renders. Portable: no store, no connection, no driver.
/// #[derive(Clone)]
/// pub struct Data {
///     pub greeting: String,
/// }
///
/// impl PageData<Store> for Data {
///     async fn load(ctx: &HttpContext<Store>) -> Result<Self, AnyError> {
///         Ok(Self {
///             greeting: ctx.state().greeting.to_owned(),
///         })
///     }
/// }
/// ```
///
/// # Errors
///
/// `load` returns [`AnyError`], rendered by the router like any other refusal.
pub trait PageData<S>: Clone + Send + Sync + Sized + 'static {
    /// Load this page's data for `ctx`.
    ///
    /// # Errors
    /// Whatever the page's stores answer with; the generated route propagates it.
    fn load(ctx: &HttpContext<S>) -> impl Future<Output = Result<Self, AnyError>> + Send;
}
